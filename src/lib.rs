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
//! # Example
//!
//! ```no_run
//! use bevy::prelude::*;
//! use bevy_restore_window::RestoreWindowPlugin;
//!
//! fn main() {
//!     App::new()
//!         .add_plugins(DefaultPlugins)
//!         .add_plugins(RestoreWindowPlugin::new("my_app"))
//!         .run();
//! }
//! ```

mod state;
mod systems;
mod types;

pub use state::RestoreWindowConfig;
pub use state::WindowState;
pub use types::MonitorInfo;
pub use types::TargetPosition;
pub use types::WinitInfo;

use bevy::prelude::*;

use systems::handle_window_events;
use systems::init_winit_info;
use systems::step1_move_to_monitor;
use systems::step2_apply_exact;

/// Plugin that saves and restores window position and size across sessions.
///
/// The window state is saved to a RON file in the system config directory
/// under a subdirectory named after your application.
///
/// # Example
///
/// ```no_run
/// use bevy::prelude::*;
/// use bevy_restore_window::RestoreWindowPlugin;
///
/// App::new()
///     .add_plugins(DefaultPlugins)
///     .add_plugins(RestoreWindowPlugin::new("my_game"))
///     .run();
/// ```
///
/// This will save window state to:
/// - macOS: `~/Library/Application Support/my_game/windows.ron`
/// - Linux: `~/.config/my_game/windows.ron`
/// - Windows: `C:\Users\<User>\AppData\Roaming\my_game\windows.ron`
pub struct RestoreWindowPlugin {
    /// Application name used for the config directory.
    pub app_name: String,
}

impl RestoreWindowPlugin {
    /// Create a new `RestoreWindowPlugin` with the given application name.
    ///
    /// The application name is used to create a subdirectory in the system
    /// config directory where the window state file will be stored.
    #[must_use]
    pub fn new(app_name: impl Into<String>) -> Self {
        Self {
            app_name: app_name.into(),
        }
    }
}

impl Plugin for RestoreWindowPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(RestoreWindowConfig {
            app_name: self.app_name.clone(),
        })
        .add_systems(PreStartup, (init_winit_info, step1_move_to_monitor).chain())
        .add_systems(Startup, step2_apply_exact)
        .add_systems(Update, handle_window_events);
    }
}
