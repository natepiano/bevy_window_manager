//! Window state persistence.

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

use bevy::prelude::*;

use super::types::WindowState;

const STATE_FILE: &str = "windows.ron";

/// Key used for the primary window in the state file.
pub const PRIMARY_WINDOW_KEY: &str = "primary";

/// Get the default state file path using the executable name.
///
/// Returns `config_dir()/<exe_name>/windows.ron`
pub fn get_default_state_path() -> Option<PathBuf> {
    let exe_name = std::env::current_exe()
        .ok()?
        .file_stem()?
        .to_str()?
        .to_string();
    dirs::config_dir().map(|d| d.join(exe_name).join(STATE_FILE))
}

/// Get the state file path for a given app name.
///
/// Returns `config_dir()/<app_name>/windows.ron`
pub fn get_state_path_for_app(app_name: &str) -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join(app_name).join(STATE_FILE))
}

/// Load all window states from the given path.
///
/// Supports migration from the old single-window format: if the file contains
/// a single `WindowState`, it is wrapped as `{"primary": state}`.
pub fn load_all_states(path: &Path) -> Option<HashMap<String, WindowState>> {
    let contents = fs::read_to_string(path).ok()?;

    // Try new HashMap format first
    if let Ok(states) = ron::from_str::<HashMap<String, WindowState>>(&contents) {
        return Some(states);
    }

    // Fall back to old single-window format (migration)
    if let Ok(state) = ron::from_str::<WindowState>(&contents) {
        debug!("[load_all_states] Migrated old single-window state file to multi-window format");
        let mut map = HashMap::new();
        map.insert((*PRIMARY_WINDOW_KEY).to_string(), state);
        return Some(map);
    }

    None
}

/// Save all window states to the given path.
pub fn save_all_states(path: &Path, states: &HashMap<String, WindowState>) {
    if let Some(parent) = path.parent()
        && let Err(e) = fs::create_dir_all(parent)
    {
        warn!("[save_all_states] Failed to create directory {parent:?}: {e}");
        return;
    }
    match ron::ser::to_string_pretty(states, ron::ser::PrettyConfig::default()) {
        Ok(contents) => {
            if let Err(e) = fs::write(path, &contents) {
                warn!("[save_all_states] Failed to write state file {path:?}: {e}");
            }
        },
        Err(e) => {
            warn!("[save_all_states] Failed to serialize state: {e}");
        },
    }
}
