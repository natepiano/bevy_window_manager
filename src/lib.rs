#![doc = include_str!("../README.md")]
//!
//! # Technical Details
//!
//! ## The Problem
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
//! ## The Solution
//!
//! This plugin uses winit directly to capture the actual window position at startup,
//! compensates for scale factor conversions, and properly restores windows across monitors.
//!
//! The plugin automatically hides the window during startup and shows it after positioning
//! is complete, preventing any visual flash at the default position.
//!
//! See the `custom_app_name` example for how to override the `app_name` used in the path
//! (default is to choose the executable name).
//!
//! See the `custom_path` example for how to override the full path to the state file.

mod config;
mod constants;
mod events;
#[cfg(target_os = "macos")]
mod macos_tabbing_fix;
mod managed;
mod monitor;
mod monitors;
mod observers;
mod persistence;
mod platform;
mod restore;
#[cfg(all(target_os = "windows", feature = "workaround-winit-4341"))]
mod windows_dpi_fix;
#[cfg(all(target_os = "linux", feature = "workaround-winit-4445"))]
mod x11_frame_extents;

use std::path::PathBuf;

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use config::RestoreWindowConfig;
pub use events::WindowRestoreMismatch;
pub use events::WindowRestored;
pub use managed::ManagedWindow;
pub use managed::ManagedWindowPersistence;
use managed::ManagedWindowRegistry;
pub use monitors::CurrentMonitor;
pub use monitors::MonitorInfo;
use monitors::MonitorPlugin;
pub use monitors::Monitors;
pub use persistence::WindowKey;
pub use platform::Platform;

/// The main plugin. See module docs for usage.
///
/// Default state file locations:
/// - macOS: `~/Library/Application Support/<exe_name>/windows.ron`
/// - Linux: `~/.config/<exe_name>/windows.ron`
/// - Windows: `C:\Users\<User>\AppData\Roaming\<exe_name>\windows.ron`
///
/// Unit struct version for convenience using `.add_plugins(WindowManagerPlugin)`.
pub struct WindowManagerPlugin;

impl WindowManagerPlugin {
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
        WindowManagerPluginCustomPath {
            path:        persistence::get_state_path_for_app(&app_name.into())
                .expect("Could not determine state file path"),
            persistence: ManagedWindowPersistence::default(),
        }
    }

    /// Create a plugin with a custom state file path.
    #[must_use]
    pub fn with_path(path: impl Into<PathBuf>) -> impl Plugin {
        WindowManagerPluginCustomPath {
            path:        path.into(),
            persistence: ManagedWindowPersistence::default(),
        }
    }

    /// Create a plugin with a specific persistence behavior.
    ///
    /// # Panics
    ///
    /// Panics if the config directory cannot be determined.
    #[must_use]
    #[expect(clippy::expect_used, reason = "fail fast if path cannot be determined")]
    pub fn with_persistence(persistence: ManagedWindowPersistence) -> impl Plugin {
        WindowManagerPluginCustomPath {
            path: persistence::get_default_state_path()
                .expect("Could not determine state file path"),
            persistence,
        }
    }
}

impl Plugin for WindowManagerPlugin {
    #[expect(clippy::expect_used, reason = "fail fast if path cannot be determined")]
    fn build(&self, app: &mut App) {
        app.add_plugins(WindowManagerPluginCustomPath {
            path:        persistence::get_default_state_path()
                .expect("Could not determine state file path"),
            persistence: ManagedWindowPersistence::default(),
        });
    }
}

/// Plugin variant with a custom state file path.
struct WindowManagerPluginCustomPath {
    path:        PathBuf,
    persistence: ManagedWindowPersistence,
}

impl Plugin for WindowManagerPluginCustomPath {
    fn build(&self, app: &mut App) {
        let path = self.path.clone();
        let persistence = self.persistence.clone();

        let platform = Platform::detect();
        app.insert_resource(platform);

        // Hide primary window to prevent flash at default position.
        // Two cases to handle:
        // 1. Window already exists (`WindowManagerPlugin` added after `DefaultPlugins`) — hide
        //    immediately
        // 2. Window doesn't exist yet (`WindowManagerPlugin` added before `DefaultPlugins`) — use
        //    observer
        //
        // EXCEPTION: On Linux X11 with frame extent compensation (workaround-winit-4445),
        // we cannot hide the window because the compensation system needs to query
        // `_NET_FRAME_EXTENTS`, which requires the window to be visible/mapped.
        let should_hide = platform.should_hide_on_startup();

        if should_hide {
            let mut query = app
                .world_mut()
                .query_filtered::<&mut Window, With<PrimaryWindow>>();
            if let Some(mut window) = query.iter_mut(app.world_mut()).next() {
                debug!("[build] Window already exists, hiding immediately");
                window.visible = false;
            } else {
                debug!("[build] Window doesn't exist yet, registering observer");
                app.add_observer(observers::hide_window_on_creation);
            }
        } else {
            debug!("[build] Linux X11: skipping window hide for frame extent compensation");
        }

        #[cfg(target_os = "macos")]
        {
            app.add_systems(Startup, macos_tabbing_fix::disable_tabbing_on_primary);
            app.add_systems(
                Update,
                macos_tabbing_fix::disable_tabbing_on_managed.before(restore::restore_windows),
            );
        }

        #[cfg(all(target_os = "windows", feature = "workaround-winit-4341"))]
        {
            app.add_systems(Startup, windows_dpi_fix::install_dpi_fix);
            app.add_systems(Update, windows_dpi_fix::install_dpi_fix_on_managed);
        }

        app.add_plugins(MonitorPlugin)
            .insert_resource(RestoreWindowConfig {
                path,
                loaded_states: std::collections::HashMap::new(),
            })
            .insert_resource(persistence)
            .init_resource::<ManagedWindowRegistry>()
            .add_observer(observers::on_managed_window_added)
            .add_observer(observers::on_managed_window_removed)
            .add_observer(observers::on_managed_window_load)
            .add_systems(PreStartup, {
                #[cfg(target_os = "linux")]
                {
                    // X11 fullscreen: move window to target monitor before first event loop.
                    // Must be chained (not `.after()`) so `apply_deferred` runs between
                    // `load_target_position` and `move_to_target_monitor` — otherwise the
                    // `TargetPosition` component inserted via deferred commands won't exist yet.
                    (
                        restore::init_winit_info,
                        restore::load_target_position,
                        restore::move_to_target_monitor,
                    )
                        .chain()
                        .after(monitors::init_monitors)
                }
                #[cfg(not(target_os = "linux"))]
                {
                    (restore::init_winit_info, restore::load_target_position)
                        .chain()
                        .after(monitors::init_monitors)
                }
            });

        // X11 frame extent compensation (Linux + W6 + X11 only)
        // Runs until all restoring windows have the `X11FrameCompensated` component
        #[cfg(all(target_os = "linux", feature = "workaround-winit-4445"))]
        app.add_systems(
            Update,
            x11_frame_extents::compensate_target_position
                .run_if(observers::has_restoring_windows)
                .run_if(|p: Res<Platform>| p.is_x11()),
        );

        // Restore windows — processes all entities with `TargetPosition` + `X11FrameCompensated`
        app.add_systems(
            Update,
            (
                restore::restore_windows,
                restore::check_restore_settling.after(restore::restore_windows),
            )
                .run_if(observers::has_restoring_windows),
        );

        // Unified monitor detection + save window state
        app.add_systems(
            Update,
            (
                monitor::update_current_monitor,
                persistence::save_window_state
                    .run_if(observers::no_restoring_windows)
                    .after(monitor::update_current_monitor),
                observers::on_persistence_changed
                    .run_if(resource_changed::<ManagedWindowPersistence>)
                    .run_if(observers::no_restoring_windows)
                    .after(monitor::update_current_monitor),
            ),
        );
    }
}
