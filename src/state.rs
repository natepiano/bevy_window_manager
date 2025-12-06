//! Window state persistence.

use super::types::WindowState;
use bevy::prelude::*;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

const STATE_FILE: &str = "windows.ron";

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

/// Load the saved window state from the given path.
pub fn load_state(path: &Path) -> Option<WindowState> {
    let contents = fs::read_to_string(path).ok()?;
    ron::from_str(&contents).ok()
}

/// Save the window state to the given path.
pub fn save_state(path: &Path, state: &WindowState) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(contents) = ron::ser::to_string_pretty(state, ron::ser::PrettyConfig::default()) {
        let _ = fs::write(path, contents);
    }
}
