//! Theme colors, sourced from the illogical-impulse generated palette so the
//! selector follows the current omarchy/material theme.
//!
//! Matches `RegionSelection.qml`:
//!   overlayColor        = transparentize("#000000", 0.4)      -> black @ 0.6a
//!   selectionBorderColor = mix(brightText, brightSecondary)   -> on_surface / secondary

use std::path::PathBuf;
use tiny_skia::Color;

#[derive(Clone, Copy, Debug)]
pub struct Theme {
    /// Dim applied outside the selection.
    pub overlay: Color,
    /// Selection border + dimension label.
    pub border: Color,
    /// Outline for targetable windows (and the hovered window's border/label).
    pub window: Color,
}

impl Default for Theme {
    fn default() -> Self {
        // Sensible dark-theme fallback if the palette can't be read.
        Self {
            overlay: Color::from_rgba8(0, 0, 0, 153), // black @ 0.6
            border: Color::from_rgba8(0xec, 0xce, 0xca, 255),
            window: Color::from_rgba8(0xe7, 0xbd, 0xb8, 255), // secondary
        }
    }
}

impl Theme {
    pub fn load() -> Self {
        Self::from_palette().unwrap_or_default()
    }

    fn palette_path() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))?;
        Some(base.join("quickshell/user/generated/colors.json"))
    }

    fn from_palette() -> Option<Self> {
        let path = Self::palette_path()?;
        let raw = std::fs::read_to_string(path).ok()?;
        let map: serde_json::Map<String, serde_json::Value> = serde_json::from_str(&raw).ok()?;

        let get = |key: &str| -> Option<(u8, u8, u8)> {
            let hex = map.get(key)?.as_str()?;
            parse_hex(hex)
        };

        // In dark mode brightText≈on_surface, brightSecondary≈secondary.
        let text = get("on_surface").unwrap_or((0xf1, 0xde, 0xdc));
        let secondary = get("secondary").unwrap_or((0xe7, 0xbd, 0xb8));
        let border = mix(text, secondary, 0.5);

        Some(Self {
            overlay: Color::from_rgba8(0, 0, 0, 153),
            border: Color::from_rgba8(border.0, border.1, border.2, 255),
            window: Color::from_rgba8(secondary.0, secondary.1, secondary.2, 255),
        })
    }
}

fn parse_hex(hex: &str) -> Option<(u8, u8, u8)> {
    let h = hex.trim().trim_start_matches('#');
    if h.len() < 6 {
        return None;
    }
    let r = u8::from_str_radix(&h[0..2], 16).ok()?;
    let g = u8::from_str_radix(&h[2..4], 16).ok()?;
    let b = u8::from_str_radix(&h[4..6], 16).ok()?;
    Some((r, g, b))
}

/// Weighted mix: `t` toward `a`, `1-t` toward `b` (matches QML ColorUtils.mix).
fn mix(a: (u8, u8, u8), b: (u8, u8, u8), t: f32) -> (u8, u8, u8) {
    let m = |ca: u8, cb: u8| (t * ca as f32 + (1.0 - t) * cb as f32).round() as u8;
    (m(a.0, b.0), m(a.1, b.1), m(a.2, b.2))
}
