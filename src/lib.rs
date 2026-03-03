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
//! See `examples/custom_app_name.rs` for how to override the `app_name` used in the path
//! (default is to choose executable name).
//!
//! See `examples/custom_path.rs` for how to override the full path to the state file.

#[cfg(all(target_os = "macos", feature = "workaround-winit-4441"))]
mod macos_drag_back_fix;
mod monitors;
mod state;
mod systems;
mod types;
mod window_ext;
#[cfg(all(target_os = "windows", feature = "workaround-winit-4341"))]
mod windows_dpi_fix;
#[cfg(all(target_os = "linux", feature = "workaround-winit-4445"))]
mod x11_frame_extents;

use std::path::PathBuf;

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
pub use monitors::CurrentMonitor;
pub use monitors::MonitorInfo;
use monitors::MonitorPlugin;
pub use monitors::Monitors;
use monitors::init_monitors;
pub use state::PRIMARY_WINDOW_KEY;
pub use state::load_all_states;
pub use types::ManagedWindow;
pub use types::ManagedWindowPersistence;
use types::ManagedWindowRegistry;
pub use types::RestoreWindowConfig;
pub use types::SavedWindowMode;
use types::TargetPosition;
pub use types::WindowIdentifier;
pub use types::WindowPositioned;
pub use types::WindowState;
pub use types::WindowTargetLoaded;
pub use window_ext::WindowExt;

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
            path:        state::get_state_path_for_app(&app_name.into())
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
            path: state::get_default_state_path().expect("Could not determine state file path"),
            persistence,
        }
    }
}

impl Plugin for WindowManagerPlugin {
    #[expect(clippy::expect_used, reason = "fail fast if path cannot be determined")]
    fn build(&self, app: &mut App) {
        let path = state::get_default_state_path().expect("Could not determine state file path");
        build_plugin(app, path, ManagedWindowPersistence::default());
    }
}

/// Plugin variant with a custom state file path.
struct WindowManagerPluginCustomPath {
    path:        PathBuf,
    persistence: ManagedWindowPersistence,
}

impl Plugin for WindowManagerPluginCustomPath {
    fn build(&self, app: &mut App) {
        build_plugin(app, self.path.clone(), self.persistence.clone());
    }
}

/// Hide the primary window when created, before winit creates the OS window.
///
/// Uses an observer on `PrimaryWindow` component addition, so it works regardless
/// of plugin order. The window will be shown after restore completes or immediately
/// if no saved state.
///
/// Note: We observe `Add<PrimaryWindow>` rather than `Add<Window>` because when
/// `Window` is added, `PrimaryWindow` may not exist yet. By observing `PrimaryWindow`,
/// we know the `Window` component already exists on the entity.
fn hide_window_on_creation(add: On<Add, PrimaryWindow>, mut windows: Query<&mut Window>) {
    debug!(
        "[hide_window_on_creation] Observer fired for entity {:?}",
        add.entity
    );
    if let Ok(mut window) = windows.get_mut(add.entity) {
        debug!("[hide_window_on_creation] Setting window.visible = false");
        window.visible = false;
    }
}

/// Observer: register a `ManagedWindow` name, panic on duplicates, and save initial state if
/// needed.
fn on_managed_window_added(
    add: On<Add, ManagedWindow>,
    managed: Query<&ManagedWindow>,
    mut registry: ResMut<ManagedWindowRegistry>,
    config: Res<RestoreWindowConfig>,
    monitors: Res<Monitors>,
    windows: Query<&Window>,
    primary_query: Query<(), With<PrimaryWindow>>,
) {
    let entity = add.entity;
    let Ok(managed_window) = managed.get(entity) else {
        return;
    };
    let name = &managed_window.window_name;

    // Primary window is managed automatically — reject explicit `ManagedWindow` on it
    if primary_query.get(entity).is_ok() {
        warn!(
            "[on_managed_window_added] `ManagedWindow` cannot be added to the primary window (entity {entity:?}). \
             The primary window is managed automatically under the key \"{key}\".",
            key = state::PRIMARY_WINDOW_KEY,
        );
        return;
    }

    assert!(
        registry.names.insert((*name).clone()),
        "Duplicate ManagedWindow name: \"{name}\" is already registered"
    );
    registry.entities.insert(entity, (*name).clone());
    debug!("[on_managed_window_added] Registered managed window \"{name}\" on entity {entity:?}");

    // If no saved state exists for this window, save its current position/size immediately
    let existing = state::load_all_states(&config.path);
    let already_saved = existing.as_ref().is_some_and(|s| s.contains_key(name));

    if !already_saved {
        if let Ok(window) = windows.get(entity) {
            let monitor = window.monitor(&monitors);
            let mode = SavedWindowMode::from(&window.effective_mode(&monitors));
            let position = match window.position {
                bevy::window::WindowPosition::At(pos) => Some((pos.x, pos.y)),
                _ => None,
            };
            let window_state = types::WindowState {
                position,
                width: window.physical_width(),
                height: window.physical_height(),
                monitor_index: monitor.index,
                mode,
                app_name: String::new(),
            };

            let mut states = existing.unwrap_or_default();
            states.insert((*name).clone(), window_state);
            state::save_all_states(&config.path, &states);
            debug!("[on_managed_window_added] Saved initial state for \"{name}\"");
        }
    }
}

/// Observer: unregister a `ManagedWindow` name when removed, and update state file if `ActiveOnly`.
fn on_managed_window_removed(
    remove: On<Remove, ManagedWindow>,
    mut registry: ResMut<ManagedWindowRegistry>,
    config: Res<RestoreWindowConfig>,
    persistence: Res<ManagedWindowPersistence>,
) {
    let entity = remove.entity;
    if let Some(name) = registry.entities.remove(&entity) {
        // If `ActiveOnly`, remove this window's entry from the state file
        if *persistence == ManagedWindowPersistence::ActiveOnly {
            if let Some(mut states) = state::load_all_states(&config.path) {
                if states.remove(&name).is_some() {
                    state::save_all_states(&config.path, &states);
                    debug!(
                        "[on_managed_window_removed] Removed \"{name}\" from state file (ActiveOnly)"
                    );
                }
            }
        }

        registry.names.remove(&name);
        debug!(
            "[on_managed_window_removed] Unregistered managed window \"{name}\" from entity {entity:?}"
        );
    }
}

/// Observer: hide a managed window on creation and load its saved state.
fn on_managed_window_load(
    add: On<Add, ManagedWindow>,
    mut commands: Commands,
    managed: Query<&ManagedWindow>,
    monitors: Res<Monitors>,
    winit_info: Option<Res<types::WinitInfo>>,
    config: Res<RestoreWindowConfig>,
    mut windows: Query<&mut Window>,
    primary_monitor: Query<&CurrentMonitor, With<PrimaryWindow>>,
) {
    let entity = add.entity;
    let Ok(managed_window) = managed.get(entity) else {
        return;
    };
    let name = &managed_window.window_name;

    // Hide window during restore
    if let Ok(mut window) = windows.get_mut(entity) {
        // On Linux X11 with frame extent compensation, don't hide
        #[cfg(all(target_os = "linux", feature = "workaround-winit-4445"))]
        let should_hide = systems::is_wayland();
        #[cfg(not(all(target_os = "linux", feature = "workaround-winit-4445")))]
        let should_hide = true;

        if should_hide {
            window.visible = false;
        }
    }

    // Check the startup snapshot — not the file, which may have been modified by
    // `on_managed_window_added` saving initial state for brand-new windows.
    let Some(saved_state) = config.loaded_states.get(name).cloned() else {
        debug!("[on_managed_window_load] No saved state for \"{name}\", showing window");
        if let Ok(mut window) = windows.get_mut(entity) {
            window.visible = true;
        }
        return;
    };

    debug!(
        "[on_managed_window_load] Loaded state for \"{name}\": position={:?} size={}x{} monitor={} mode={:?}",
        saved_state.position,
        saved_state.width,
        saved_state.height,
        saved_state.monitor_index,
        saved_state.mode
    );

    let Some(winit_info) = winit_info else {
        debug!("[on_managed_window_load] WinitInfo not available, showing window for \"{name}\"");
        if let Ok(mut window) = windows.get_mut(entity) {
            window.visible = true;
        }
        return;
    };

    if monitors.is_empty() {
        debug!("[on_managed_window_load] No monitors available, showing window for \"{name}\"");
        if let Ok(mut window) = windows.get_mut(entity) {
            window.visible = true;
        }
        return;
    }

    // The window will be created on the focused window's monitor (the primary window's
    // monitor), so use that scale as starting_scale for scale factor compensation.
    let primary_scale = primary_monitor.iter().next().map_or(1.0, |cm| cm.scale);

    restore_managed_window(
        entity,
        name,
        &saved_state,
        &monitors,
        &winit_info,
        &mut commands,
        &mut windows,
        primary_scale,
    );
}

/// Compute the target position for a managed window from saved state.
///
/// Unlike the primary window (where `create_windows` runs before our PreStartup systems),
/// managed windows are spawned at runtime: our observer fires first, then `create_windows`
/// creates the winit window. We must NOT set `window.position` or physical size here:
///
/// - **Size**: Bevy's `create_windows` calls `set_scale_factor_and_apply_to_physical_size` which
///   multiplies stored physical size by the monitor's scale factor, doubling it on 2x.
/// - **Position**: `create_windows` converts `PhysicalPosition` to logical using
///   `NSScreen::mainScreen` scale factor (the focused window's screen), not the target monitor's
///   scale. This produces the wrong position when scales differ.
///
/// Both are applied later by `restore_windows` → `try_apply_restore` → `changed_windows`,
/// which uses `set_outer_position` with the window's actual backing scale factor.
fn restore_managed_window(
    entity: Entity,
    window_name: &str,
    saved_state: &types::WindowState,
    monitors: &Monitors,
    winit_info: &types::WinitInfo,
    commands: &mut Commands,
    windows: &mut Query<&mut Window>,
    primary_scale: f64,
) {
    let Some(target_info) = monitors.by_index(saved_state.monitor_index) else {
        debug!(
            "[restore_managed_window] Target monitor {} not found, showing window",
            saved_state.monitor_index
        );
        if let Ok(mut window) = windows.get_mut(entity) {
            window.visible = true;
        }
        return;
    };

    let target_scale = target_info.scale;
    let width = saved_state.width;
    let height = saved_state.height;

    let decoration = winit_info.decoration();
    let outer_width = width + decoration.x;
    let outer_height = height + decoration.y;

    // The window is created on the focused window's monitor (the primary window's monitor)
    // without explicit positioning. Its starting scale matches the primary monitor, not the
    // target monitor.
    let starting_scale = primary_scale;
    let strategy = systems::determine_scale_strategy(starting_scale, target_scale);

    let position = saved_state.position.map(|(x, y)| {
        systems::clamp_position_to_monitor(x, y, target_info, outer_width, outer_height)
    });

    debug!(
        "[restore_managed_window] saved_pos={:?} clamped_pos={position:?} target_scale={target_scale} outer={}x{} size={width}x{height} monitor={} mon_pos=({},{}) mon_size=({},{})",
        saved_state.position,
        outer_width,
        outer_height,
        saved_state.monitor_index,
        target_info.position.x,
        target_info.position.y,
        target_info.size.x,
        target_info.size.y,
    );

    // Don't set position or size here. Both are applied by `try_apply_restore` after
    // `create_windows` runs. Setting position here would cause `create_windows` to use
    // `with_position`, which converts `PhysicalPosition` using the wrong scale factor
    // (`NSScreen::mainScreen` scale instead of the target monitor's scale).

    let window_mode = saved_state.mode.to_window_mode(saved_state.monitor_index);
    let target = TargetPosition {
        position,
        width,
        height,
        target_scale,
        starting_scale,
        monitor_scale_strategy: strategy,
        mode: saved_state.mode.clone(),
        target_monitor_index: saved_state.monitor_index,
        #[cfg(all(target_os = "windows", feature = "workaround-winit-3124"))]
        fullscreen_restore_state: types::FullscreenRestoreState::WaitingForSurface,
    };

    commands.entity(entity).insert(target);

    // Trigger `WindowTargetLoaded`
    let size = UVec2::new(width, height);
    let window_id = types::WindowIdentifier::Managed((*window_name).to_string());
    commands
        .entity(entity)
        .trigger(|entity| WindowTargetLoaded {
            entity,
            window_id,
            position,
            size,
            mode: window_mode,
        });

    // Insert `X11FrameCompensated` for platforms that don't need compensation
    #[cfg(not(all(target_os = "linux", feature = "workaround-winit-4445")))]
    commands.entity(entity).insert(types::X11FrameCompensated);

    #[cfg(all(target_os = "linux", feature = "workaround-winit-4445"))]
    if systems::is_wayland() {
        commands.entity(entity).insert(types::X11FrameCompensated);
    }
}

/// Run condition: returns true if any entity has a `TargetPosition` component.
fn has_restoring_windows(q: Query<(), With<TargetPosition>>) -> bool { !q.is_empty() }

/// Run condition: returns true if no entity has a `TargetPosition` component.
fn no_restoring_windows(q: Query<(), With<TargetPosition>>) -> bool { q.is_empty() }

/// The run conditions allow us to separate the initial primary window restore from
/// subsequent positions saves - which we dont' want to do until AFTER we've done
/// the initial restore.
fn build_plugin(app: &mut App, path: PathBuf, persistence: ManagedWindowPersistence) {
    // Hide primary window to prevent flash at default position.
    // Two cases to handle:
    // 1. Window already exists (WindowManagerPlugin added after DefaultPlugins) - hide immediately
    // 2. Window doesn't exist yet (WindowManagerPlugin added before DefaultPlugins) - use observer
    //
    // EXCEPTION: On Linux X11 with frame extent compensation (workaround-winit-4445),
    // we cannot hide the window because the compensation system needs to query
    // _NET_FRAME_EXTENTS, which requires the window to be visible/mapped.
    #[cfg(all(target_os = "linux", feature = "workaround-winit-4445"))]
    let should_hide = systems::is_wayland();
    #[cfg(not(all(target_os = "linux", feature = "workaround-winit-4445")))]
    let should_hide = true;

    if should_hide {
        let mut query = app
            .world_mut()
            .query_filtered::<&mut Window, With<PrimaryWindow>>();
        if let Some(mut window) = query.iter_mut(app.world_mut()).next() {
            debug!("[build_plugin] Window already exists, hiding immediately");
            window.visible = false;
        } else {
            debug!("[build_plugin] Window doesn't exist yet, registering observer");
            app.add_observer(hide_window_on_creation);
        }
    } else {
        debug!("[build_plugin] Linux X11: skipping window hide for frame extent compensation");
    }

    #[cfg(all(target_os = "macos", feature = "workaround-winit-4441"))]
    macos_drag_back_fix::init(app);

    #[cfg(all(target_os = "windows", feature = "workaround-winit-4341"))]
    windows_dpi_fix::init(app);

    app.add_plugins(MonitorPlugin)
        .insert_resource(RestoreWindowConfig {
            path,
            loaded_states: std::collections::HashMap::new(),
        })
        .insert_resource(persistence)
        .init_resource::<ManagedWindowRegistry>()
        .add_observer(on_managed_window_added)
        .add_observer(on_managed_window_removed)
        .add_observer(on_managed_window_load)
        .add_systems(
            PreStartup,
            (systems::init_winit_info, systems::load_target_position)
                .chain()
                .after(init_monitors),
        );

    // X11 frame extent compensation (Linux + W6 + X11 only)
    // Runs until all restoring windows have the X11FrameCompensated component
    #[cfg(all(target_os = "linux", feature = "workaround-winit-4445"))]
    app.add_systems(
        Update,
        x11_frame_extents::compensate_target_position
            .run_if(has_restoring_windows)
            .run_if(not(systems::is_wayland)),
    );

    // Restore windows - processes all entities with `TargetPosition` + `X11FrameCompensated`
    app.add_systems(
        Update,
        systems::restore_windows.run_if(has_restoring_windows),
    );

    // Linux: includes Wayland monitor detection with ordering constraint
    #[cfg(target_os = "linux")]
    app.add_systems(
        Update,
        (
            systems::update_wayland_monitor.run_if(systems::is_wayland),
            systems::save_window_state
                .run_if(no_restoring_windows)
                .after(systems::update_wayland_monitor),
        ),
    );

    // Non-Linux: no Wayland handling needed
    #[cfg(not(target_os = "linux"))]
    app.add_systems(
        Update,
        systems::save_window_state.run_if(no_restoring_windows),
    );
}
