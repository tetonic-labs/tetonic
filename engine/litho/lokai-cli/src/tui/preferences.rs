//! Local presentation preferences; no workspace files or agent policy state.
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub(crate) struct Preferences {
    pub sidebar: bool,
    pub reduced_motion: bool,
    pub plain_colors: bool,
    pub hide_tools: bool,
    pub mouse: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            sidebar: false,
            reduced_motion: false,
            plain_colors: false,
            hide_tools: false,
            mouse: true,
        }
    }
}

fn path() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "Lokai", "lokai").map(|p| p.config_dir().join("tui.json"))
}

pub fn load() -> Preferences {
    let value = path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .unwrap_or_default();
    Preferences {
        sidebar: value["sidebar"].as_bool().unwrap_or(false),
        reduced_motion: value["reduced_motion"].as_bool().unwrap_or(false),
        plain_colors: value["plain_colors"].as_bool().unwrap_or(false)
            || std::env::var_os("NO_COLOR").is_some(),
        hide_tools: value["hide_tools"].as_bool().unwrap_or(false),
        mouse: value["mouse"].as_bool().unwrap_or(true),
    }
}

pub fn save(prefs: &Preferences) -> std::io::Result<()> {
    let Some(path) = path() else {
        return Err(std::io::Error::other("configuration directory unavailable"));
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let data = serde_json::json!({"sidebar": prefs.sidebar, "reduced_motion": prefs.reduced_motion,
        "plain_colors": prefs.plain_colors, "hide_tools": prefs.hide_tools, "mouse": prefs.mouse});
    std::fs::write(path, data.to_string())
}
