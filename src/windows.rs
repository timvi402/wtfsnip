//! Window geometry for "click a window to grab it" targeting.
//!
//! Geometry isn't available through any generic Wayland protocol, so this is
//! Hyprland-specific (via `hyprctl -j`). It's best-effort: if hyprctl is absent
//! or fails, targeting is simply disabled and the rest of the tool still works.

use std::collections::HashMap;
use std::process::Command;

use serde::Deserialize;

#[derive(Deserialize)]
struct Workspace {
    id: i64,
}

#[derive(Deserialize)]
struct Monitor {
    name: String,
    x: i32,
    y: i32,
    #[serde(rename = "activeWorkspace")]
    active_workspace: Workspace,
}

#[derive(Deserialize)]
struct Client {
    at: [i32; 2],
    size: [i32; 2],
    workspace: Workspace,
    mapped: bool,
    hidden: bool,
    class: String,
    floating: bool,
}

/// A window's bounds in a single output's logical coordinate space.
#[derive(Clone, Debug)]
pub struct WinRegion {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub floating: bool,
    pub label: String,
}

impl WinRegion {
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.x + self.w && py >= self.y && py <= self.y + self.h
    }
    pub fn area(&self) -> f32 {
        self.w * self.h
    }
}

fn hyprctl_json(what: &str) -> Option<Vec<u8>> {
    let out = Command::new("hyprctl").arg("-j").arg(what).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(out.stdout)
}

/// Map of output name -> visible windows on that output's active workspace,
/// in the output's local logical coordinates.
pub fn regions_by_output() -> HashMap<String, Vec<WinRegion>> {
    let mut map = HashMap::new();
    let (Some(mraw), Some(craw)) = (hyprctl_json("monitors"), hyprctl_json("clients")) else {
        return map;
    };
    let (Ok(monitors), Ok(clients)) = (
        serde_json::from_slice::<Vec<Monitor>>(&mraw),
        serde_json::from_slice::<Vec<Client>>(&craw),
    ) else {
        return map;
    };

    for m in &monitors {
        let ws = m.active_workspace.id;
        let mut regions: Vec<WinRegion> = clients
            .iter()
            .filter(|c| {
                c.mapped && !c.hidden && c.workspace.id == ws && c.size[0] > 0 && c.size[1] > 0
            })
            .map(|c| WinRegion {
                x: (c.at[0] - m.x) as f32,
                y: (c.at[1] - m.y) as f32,
                w: c.size[0] as f32,
                h: c.size[1] as f32,
                floating: c.floating,
                label: c.class.clone(),
            })
            .collect();
        // Floating windows sit on top; check them first when targeting.
        regions.sort_by_key(|r| !r.floating);
        map.insert(m.name.clone(), regions);
    }
    map
}

/// Index of the window under the cursor: the smallest-area region that contains
/// the point (floating preferred via the pre-sort), i.e. the topmost/specific one.
pub fn target_at(regions: &[WinRegion], x: f32, y: f32) -> Option<usize> {
    regions
        .iter()
        .enumerate()
        .filter(|(_, r)| r.contains(x, y))
        .min_by(|(_, a), (_, b)| a.area().partial_cmp(&b.area()).unwrap())
        .map(|(i, _)| i)
}
