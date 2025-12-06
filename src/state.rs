//! Window state persistence.

use std::fs;
use std::path::PathBuf;

use bevy::prelude::*;
use serde::Deserialize;
use serde::Serialize;

const STATE_FILE: &str = "windows.ron";

/// Configuration for the `RestoreWindowPlugin`.
#[derive(Resource, Clone)]
pub struct RestoreWindowConfig {
    /// Application name used for the config directory path.
    pub app_name: String,
}

/// Saved window state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowState {
    pub position: Option<(i32, i32)>,
    pub width: f32,
    pub height: f32,
    pub monitor_index: usize,
}

/// Get the path to the state file for the given app name.
pub fn get_state_path(app_name: &str) -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join(app_name).join(STATE_FILE))
}

/// Load the saved window state.
pub fn load_state(app_name: &str) -> Option<WindowState> {
    let path = get_state_path(app_name)?;
    let contents = fs::read_to_string(&path).ok()?;
    ron::from_str(&contents).ok()
}

/// Save the window state.
pub fn save_state(app_name: &str, state: &WindowState) {
    let Some(path) = get_state_path(app_name) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(contents) = ron::ser::to_string_pretty(state, ron::ser::PrettyConfig::default()) {
        let _ = fs::write(&path, contents);
    }
}
