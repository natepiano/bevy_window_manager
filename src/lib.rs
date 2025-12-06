//! Window position and size restoration plugin for Bevy.
//!
//! This plugin saves and restores window position and size across application sessions,
//! with proper handling for multi-monitor setups with different scale factors.
//!
//! # The Problem
//!
//! On macOS with multiple monitors that have different scale factors (e.g., a Retina display
//! at scale 2.0 and an external monitor at scale 1.0), Bevy's window positioning has issues:
//!
//! 1. **`Window.position` is unreliable at startup**: When a window is created, `Window.position`
//!    is `Automatic` (not `At(pos)`), even though winit has placed the window at a specific
//!    physical position.
//!
//! 2. **Scale factor conversion in `changed_windows`**: When you modify `Window.resolution`,
//!    Bevy's `changed_windows` system applies scale factor conversion if
//!    `scale_factor != cached_scale_factor`. This corrupts the size when moving windows
//!    between monitors with different scale factors.
//!
//! 3. **Timing of scale factor updates**: The `CachedWindow` is updated after winit events are
//!    processed, but our systems run before we receive the `ScaleFactorChanged` event.
//!
//! # The Solution
//!
//! This plugin uses winit directly to capture the actual window position at startup,
//! compensates for scale factor conversions, and properly restores windows across monitors.
//!
//! # Usage
//!
//! ```no_run
//! use bevy::prelude::*;
//! use bevy_restore_window::RestoreWindowPlugin;
//!
//! App::new()
//!     .add_plugins(DefaultPlugins)
//!     // Uses executable name for config directory
//!     .add_plugins(RestoreWindowPlugin::default())
//!     .run();
//! ```
//!
//! See `examples/custom_app_name.rs` for how to override the `app_name` used in the path
//! (default is to choose executable name).
//!
//! See `examples/custom_path.rs` for how to override the full path to the state file.

mod state;
mod systems;
mod types;

use bevy::prelude::*;
use std::path::PathBuf;
use types::RestoreWindowConfig;

/// The main plugin. See module docs for usage.
///
/// Default state file locations:
/// - macOS: `~/Library/Application Support/<exe_name>/windows.ron`
/// - Linux: `~/.config/<exe_name>/windows.ron`
/// - Windows: `C:\Users\<User>\AppData\Roaming\<exe_name>\windows.ron`
pub struct RestoreWindowPlugin {
    path: PathBuf,
}

impl RestoreWindowPlugin {
    /// Create a plugin with a custom app name.
    ///
    /// Uses `config_dir()/<app_name>/windows.ron`.
    ///
    /// # Panics
    ///
    /// Panics if the config directory cannot be determined.
    #[must_use]
    #[expect(clippy::expect_used, reason = "fail fast if path cannot be determined")]
    pub fn with_app_name(app_name: impl Into<String>) -> Self {
        Self {
            path: state::get_state_path_for_app(&app_name.into())
                .expect("Could not determine state file path"),
        }
    }

    /// Create a plugin with a custom state file path.
    #[must_use]
    pub fn with_path(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

#[expect(clippy::expect_used, reason = "fail fast if path cannot be determined")]
impl Default for RestoreWindowPlugin {
    fn default() -> Self {
        Self {
            path: state::get_default_state_path().expect("Could not determine state file path"),
        }
    }
}

impl Plugin for RestoreWindowPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(RestoreWindowConfig {
            path: self.path.clone(),
        })
        .add_systems(
            PreStartup,
            (systems::init_winit_info, systems::step1_move_to_monitor).chain(),
        )
        .add_systems(Startup, systems::step2_apply_exact)
        .add_systems(Update, systems::handle_window_events);
    }
}
