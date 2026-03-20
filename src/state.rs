//! Window state persistence.

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

use bevy::prelude::*;

use super::WindowKey;
use super::state_format;
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
pub fn load_all_states(path: &Path) -> Option<HashMap<WindowKey, WindowState>> {
    let contents = fs::read_to_string(path).ok()?;
    state_format::decode(&contents)
}

/// Save all window states to the given path.
pub fn save_all_states(path: &Path, states: &HashMap<WindowKey, WindowState>) {
    if let Some(parent) = path.parent()
        && let Err(e) = fs::create_dir_all(parent)
    {
        warn!("[save_all_states] Failed to create directory {parent:?}: {e}");
        return;
    }
    match state_format::encode(states) {
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

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use std::fs;

    use tempfile::NamedTempFile;

    use super::load_all_states;
    use super::save_all_states;
    use crate::state_format::WindowKey;
    use crate::types::SavedWindowMode;
    use crate::types::WindowState;

    fn sample_state() -> WindowState {
        WindowState {
            logical_position: Some((10, 20)),
            logical_width:    800,
            logical_height:   600,
            monitor_scale:    1.0,
            monitor_index:    0,
            mode:             SavedWindowMode::Windowed,
            app_name:         "test-app".to_string(),
        }
    }

    #[test]
    fn save_then_load_roundtrip_v2() {
        let file = match NamedTempFile::new() {
            Ok(file) => file,
            Err(error) => panic!("failed to create temp file: {error}"),
        };
        let path = file.path();

        let states = std::collections::HashMap::from([
            (WindowKey::Primary, sample_state()),
            (WindowKey::Managed("primary".to_string()), sample_state()),
        ]);
        save_all_states(path, &states);

        let loaded = load_all_states(path);
        assert!(loaded.is_some(), "expected saved v1 state to load");
        let loaded = loaded.unwrap_or_default();
        assert!(loaded.contains_key(&WindowKey::Primary));
        assert!(loaded.contains_key(&WindowKey::Managed("primary".to_string())));
    }

    #[test]
    fn legacy_single_window_read_then_save_rewrites_as_v2() {
        let file = match NamedTempFile::new() {
            Ok(file) => file,
            Err(error) => panic!("failed to create temp file: {error}"),
        };
        let path = file.path();
        // Legacy format uses `width`/`height` field names (pre-multi-window era)
        let legacy_contents = "\
(
    position: Some((10, 20)),
    width: 800,
    height: 600,
    monitor_index: 0,
    mode: Windowed,
    app_name: \"test-app\",
)";

        if let Err(error) = fs::write(path, legacy_contents) {
            panic!("failed to write legacy content: {error}");
        }

        let states = load_all_states(path);
        assert!(states.is_some(), "expected legacy content to decode");
        let states = states.unwrap_or_default();
        save_all_states(path, &states);

        let contents = fs::read_to_string(path);
        assert!(contents.is_ok(), "expected rewritten file to be readable");
        let contents = contents.unwrap_or_default();
        assert!(
            contents.contains("version: 2"),
            "expected rewritten file to contain v2 version marker"
        );
        assert!(
            contents.contains("logical_width: 800"),
            "expected rewritten file to contain logical_width"
        );
    }
}
