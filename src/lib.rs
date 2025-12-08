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
//! 2. **Scale factor conversion in `changed_windows`**: When you modify `Window.resolution`, Bevy's
//!    `changed_windows` system applies scale factor conversion if `scale_factor !=
//!    cached_scale_factor`. This corrupts the size when moving windows between monitors with
//!    different scale factors.
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
//! use bevy_restore_windows::RestoreWindowsPlugin;
//!
//! App::new()
//!     .add_plugins(DefaultPlugins)
//!     // Uses executable name for config directory
//!     .add_plugins(RestoreWindowsPlugin)
//!     .run();
//! ```
//!
//! See `examples/custom_app_name.rs` for how to override the `app_name` used in the path
//! (default is to choose executable name).
//!
//! See `examples/custom_path.rs` for how to override the full path to the state file.

#[cfg(target_os = "macos")]
mod macos_fullscreen_fix;
mod monitors;
mod state;
mod systems;
mod types;
mod window_ext;

use std::path::PathBuf;

use bevy::prelude::*;
pub use monitors::MonitorInfo;
use monitors::MonitorPlugin;
pub use monitors::Monitors;
use monitors::init_monitors;
use types::RestoreWindowConfig;
use types::TargetPosition;
pub use window_ext::WindowExt;

/// The main plugin. See module docs for usage.
///
/// Default state file locations:
/// - macOS: `~/Library/Application Support/<exe_name>/windows.ron`
/// - Linux: `~/.config/<exe_name>/windows.ron`
/// - Windows: `C:\Users\<User>\AppData\Roaming\<exe_name>\windows.ron`
///
/// Unit struct version for convenience using `.add_plugins(RestoreWindowsPlugin)`.
pub struct RestoreWindowsPlugin;

impl RestoreWindowsPlugin {
    /// Create a plugin with a custom app name.
    ///
    /// Uses `config_dir()/<app_name>/windows.ron`.
    ///
    /// # Panics
    ///
    /// Panics if the config directory cannot be determined.
    #[must_use]
    #[expect(clippy::expect_used, reason = "fail fast if path cannot be determined")]
    pub fn with_app_name(app_name: impl Into<String>) -> impl Plugin {
        RestoreWindowsPluginCustomPath {
            path: state::get_state_path_for_app(&app_name.into())
                .expect("Could not determine state file path"),
        }
    }

    /// Create a plugin with a custom state file path.
    #[must_use]
    pub fn with_path(path: impl Into<PathBuf>) -> impl Plugin {
        RestoreWindowsPluginCustomPath { path: path.into() }
    }
}

impl Plugin for RestoreWindowsPlugin {
    #[expect(clippy::expect_used, reason = "fail fast if path cannot be determined")]
    fn build(&self, app: &mut App) {
        let path = state::get_default_state_path().expect("Could not determine state file path");
        build_plugin(app, path);
    }
}

/// Plugin variant with a custom state file path.
struct RestoreWindowsPluginCustomPath {
    path: PathBuf,
}

impl Plugin for RestoreWindowsPluginCustomPath {
    fn build(&self, app: &mut App) { build_plugin(app, self.path.clone()); }
}

/// The run conditions allow us to separate the initial primary window restore from
/// subsequent positions saves - which we dont' want to do until AFTER we've done
/// the initial restore.
fn build_plugin(app: &mut App, path: PathBuf) {
    #[cfg(target_os = "macos")]
    macos_fullscreen_fix::init(app);

    app.add_plugins(MonitorPlugin)
        .insert_resource(RestoreWindowConfig { path })
        .add_systems(
            PreStartup,
            (
                systems::init_winit_info,
                systems::load_target_position,
                systems::move_to_target_monitor.run_if(resource_exists::<TargetPosition>),
            )
                .chain()
                .after(init_monitors),
        )
        .add_systems(
            Update,
            (
                systems::restore_primary_window.run_if(resource_exists::<TargetPosition>),
                systems::save_window_state.run_if(not(resource_exists::<TargetPosition>)),
            ),
        );
}
