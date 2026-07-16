//! User configuration, read from `$XDG_CONFIG_HOME/wtfsnip/config.json`
//! (falling back to `~/.config/wtfsnip/config.json`).
//!
//! Every field is optional; anything omitted keeps its default, so an empty
//! `{}` — or no file at all — gives the built-in behaviour.

use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Config {
    /// Auto-save each screenshot to disk (in addition to copying it to the
    /// clipboard).
    pub save: bool,
    /// Directory screenshots are written to. `~` and `$HOME` are expanded.
    pub save_dir: PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            save: true,
            save_dir: default_save_dir(),
        }
    }
}

/// The raw on-disk shape: all fields optional so a partial file just overrides
/// the pieces it mentions.
#[derive(serde::Deserialize, Default)]
struct RawConfig {
    save: Option<bool>,
    save_dir: Option<String>,
}

impl Config {
    pub fn load() -> Self {
        let mut cfg = Self::default();
        let Some(raw) = Self::read_raw() else { return cfg };
        if let Some(save) = raw.save {
            cfg.save = save;
        }
        if let Some(dir) = raw.save_dir {
            cfg.save_dir = expand(&dir);
        }
        cfg
    }

    fn read_raw() -> Option<RawConfig> {
        let path = Self::config_path()?;
        let raw = std::fs::read_to_string(&path).ok()?;
        match serde_json::from_str(&raw) {
            Ok(cfg) => Some(cfg),
            Err(e) => {
                eprintln!("wtfsnip: {}: {e}", path.display());
                None
            }
        }
    }

    fn config_path() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
        Some(base.join("wtfsnip/config.json"))
    }
}

fn default_save_dir() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    home.join("Pictures/Screenshots")
}

/// Expand a leading `~/` (or `~`) and any `$HOME` occurrences.
fn expand(path: &str) -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    let expanded = if path == "~" {
        home.clone()
    } else if let Some(rest) = path.strip_prefix("~/") {
        format!("{home}/{rest}")
    } else {
        path.to_string()
    };
    PathBuf::from(expanded.replace("$HOME", &home))
}

/// A collision-free `wtfsnip_<timestamp>.png` path inside `dir`.
pub fn shot_path(dir: &Path) -> PathBuf {
    let stem = format!("wtfsnip_{}", timestamp());
    let mut path = dir.join(format!("{stem}.png"));
    let mut n = 1;
    while path.exists() {
        path = dir.join(format!("{stem}_{n}.png"));
        n += 1;
    }
    path
}

/// Local time as `YYYY-MM-DD_HH-MM-SS`, via `date` (already in the same spirit
/// as the tool's `wl-copy`/`hyprctl` shell-outs). Falls back to epoch seconds.
fn timestamp() -> String {
    std::process::Command::new("date")
        .arg("+%Y-%m-%d_%H-%M-%S")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs().to_string())
                .unwrap_or_else(|_| "screenshot".to_string())
        })
}
