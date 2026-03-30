//! Observers, run conditions, and plugin builder.
//!
//! All window-lifecycle logic lives here. [`build_plugin`] is called from the
//! thin `Plugin` impls in `lib.rs`.

use std::path::PathBuf;

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_kana::ToI32;
use bevy_kana::ToU32;

use super::ManagedWindow;
use super::ManagedWindowPersistence;
use super::WindowKey;
use super::monitors::CurrentMonitor;
use super::monitors::MonitorPlugin;
use super::monitors::Monitors;
use super::monitors::init_monitors;
use super::platform::Platform;
use super::restore_plan;
use super::state;
use super::systems;
use super::types::ManagedWindowRegistry;
use super::types::RestoreWindowConfig;
use super::types::SavedWindowMode;
use super::types::TargetPosition;
use super::types::WindowState;
use super::types::WinitInfo;
use super::types::X11FrameCompensated;

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

/// Observer: register a `ManagedWindow` name, deduplicate if needed, and save initial state if
/// needed.
fn on_managed_window_added(
    add: On<Add, ManagedWindow>,
    mut managed: Query<&mut ManagedWindow>,
    mut registry: ResMut<ManagedWindowRegistry>,
    config: Res<RestoreWindowConfig>,
    monitors: Res<Monitors>,
    windows: Query<&Window>,
    primary_query: Query<(), With<PrimaryWindow>>,
) {
    let entity = add.entity;
    let Ok(mut managed_window) = managed.get_mut(entity) else {
        return;
    };
    let name = managed_window.window_name.clone();

    // Primary window is managed automatically — reject explicit `ManagedWindow` on it
    if primary_query.get(entity).is_ok() {
        warn!(
            "[on_managed_window_added] `ManagedWindow` cannot be added to the primary window (entity {entity:?}). \
             The primary window is managed automatically under the key \"{key}\".",
            key = state::PRIMARY_WINDOW_KEY,
        );
        return;
    }

    let unique_name = if registry.names.contains(&name) {
        debug_assert!(false, "Duplicate ManagedWindow name: \"{name}\"");
        let mut suffix = 2;
        loop {
            let candidate = format!("{name}-{suffix}");
            if !registry.names.contains(&candidate) {
                break candidate;
            }
            suffix += 1;
        }
    } else {
        name.clone()
    };

    if unique_name != name {
        warn!(
            "[on_managed_window_added] Duplicate ManagedWindow name: \"{name}\" — renamed to \"{unique_name}\" for entity {entity:?}"
        );
        managed_window.window_name.clone_from(&unique_name);
    }

    registry.names.insert(unique_name.clone());
    registry.entities.insert(entity, unique_name.clone());
    debug!(
        "[on_managed_window_added] Registered managed window \"{unique_name}\" on entity {entity:?}"
    );

    // If no saved state exists for this window, save its current position/size immediately
    let existing = state::load_all_states(&config.path);
    let already_saved = existing
        .as_ref()
        .is_some_and(|s| s.contains_key(&WindowKey::Managed(unique_name.clone())));

    if !already_saved && let Ok(window) = windows.get(entity) {
        let monitor = match window.position {
            bevy::window::WindowPosition::At(pos) => {
                *monitors.monitor_for_window(pos, window.physical_width(), window.physical_height())
            },
            _ => *monitors.first(),
        };
        let logical_position = match window.position {
            bevy::window::WindowPosition::At(pos) => {
                let lx = (f64::from(pos.x) / monitor.scale).round().to_i32();
                let ly = (f64::from(pos.y) / monitor.scale).round().to_i32();
                Some((lx, ly))
            },
            _ => None,
        };
        let window_state = WindowState {
            logical_position,
            logical_width: window.width().to_u32(),
            logical_height: window.height().to_u32(),
            monitor_scale: monitor.scale,
            monitor_index: monitor.index,
            mode: SavedWindowMode::Windowed,
            app_name: String::new(),
        };

        let mut states = existing.unwrap_or_default();
        states.insert(WindowKey::Managed(unique_name.clone()), window_state);
        state::save_all_states(&config.path, &states);
        debug!("[on_managed_window_added] Saved initial state for \"{unique_name}\"");
    }
}

/// Observer: unregister a `ManagedWindow` name when removed, and update state file if `ActiveOnly`.
fn on_managed_window_removed(
    remove: On<Remove, ManagedWindow>,
    mut registry: ResMut<ManagedWindowRegistry>,
    config: Res<RestoreWindowConfig>,
    persistence: Res<ManagedWindowPersistence>,
    monitors: Res<Monitors>,
    all_windows: Query<
        (
            Entity,
            &Window,
            Option<&CurrentMonitor>,
            Option<&ManagedWindow>,
        ),
        Or<(With<PrimaryWindow>, With<ManagedWindow>)>,
    >,
    primary_q: Query<(), With<PrimaryWindow>>,
) {
    let entity = remove.entity;
    if let Some(name) = registry.entities.remove(&entity) {
        // If `ActiveOnly`, rebuild state from all remaining active windows.
        // The removed entity's `ManagedWindow` is being removed, so the query
        // naturally excludes it — but guard against it just in case.
        if *persistence == ManagedWindowPersistence::ActiveOnly {
            systems::save_active_window_state(
                &config,
                &monitors,
                &all_windows,
                &primary_q,
                Some(entity),
            );
            debug!(
                "[on_managed_window_removed] Rebuilt state file without \"{name}\" (ActiveOnly)"
            );
        }

        registry.names.remove(&name);
        debug!(
            "[on_managed_window_removed] Unregistered managed window \"{name}\" from entity {entity:?}"
        );
    }
}

/// When `ManagedWindowPersistence` switches to `ActiveOnly`, immediately rebuild the state
/// file from the currently-active windows so that any previously-remembered-but-closed
/// window entries are pruned.
fn on_persistence_changed(
    persistence: Res<ManagedWindowPersistence>,
    config: Res<RestoreWindowConfig>,
    monitors: Res<Monitors>,
    all_windows: Query<
        (
            Entity,
            &Window,
            Option<&CurrentMonitor>,
            Option<&ManagedWindow>,
        ),
        Or<(With<PrimaryWindow>, With<ManagedWindow>)>,
    >,
    primary_q: Query<(), With<PrimaryWindow>>,
) {
    if *persistence == ManagedWindowPersistence::ActiveOnly {
        systems::save_active_window_state(&config, &monitors, &all_windows, &primary_q, None);
        debug!("[on_persistence_changed] Rebuilt state file for ActiveOnly mode");
    }
}

/// Observer: hide a managed window on creation and load its saved state.
fn on_managed_window_load(
    add: On<Add, ManagedWindow>,
    mut commands: Commands,
    managed: Query<&ManagedWindow>,
    monitors: Res<Monitors>,
    winit_info: Option<Res<WinitInfo>>,
    config: Res<RestoreWindowConfig>,
    mut windows: Query<&mut Window>,
    primary_monitor: Query<&CurrentMonitor, With<PrimaryWindow>>,
    platform: Res<Platform>,
) {
    let entity = add.entity;
    let Ok(managed_window) = managed.get(entity) else {
        return;
    };
    let name = &managed_window.window_name;

    // Hide window during restore
    if let Ok(mut window) = windows.get_mut(entity) {
        // On Linux X11 with frame extent compensation, don't hide
        if platform.should_hide_on_startup() {
            window.visible = false;
        }
    }

    // Check the startup snapshot — not the file, which may have been modified by
    // `on_managed_window_added` saving initial state for brand-new windows.
    let key = WindowKey::Managed((*name).clone());
    let Some(saved_state) = config.loaded_states.get(&key).cloned() else {
        debug!("[on_managed_window_load] No saved state for \"{name}\", showing window");
        if let Ok(mut window) = windows.get_mut(entity) {
            window.visible = true;
        }
        return;
    };

    debug!(
        "[on_managed_window_load] Loaded state for \"{name}\": position={:?} logical_size={}x{} monitor_scale={} monitor={} mode={:?}",
        saved_state.logical_position,
        saved_state.logical_width,
        saved_state.logical_height,
        saved_state.monitor_scale,
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
        &saved_state,
        &monitors,
        &winit_info,
        &mut commands,
        primary_scale,
        *platform,
    );
}

/// Compute the target position for a managed window from saved state.
///
/// Inserts a `TargetPosition` component but does NOT modify `Window.position` or
/// `Window.resolution`. The actual restore is deferred to `restore_windows`, which
/// gates on the winit window existing (via `WINIT_WINDOWS`). This ensures
/// `create_windows` → `set_scale_factor_and_apply_to_physical_size()` runs first,
/// preventing the physical size from being doubled on high-DPI displays.
fn restore_managed_window(
    entity: Entity,
    saved_state: &WindowState,
    monitors: &Monitors,
    winit_info: &WinitInfo,
    commands: &mut Commands,
    primary_scale: f64,
    platform: Platform,
) {
    let (target_info, fallback_position, used_fallback) =
        restore_plan::resolve_target_monitor_and_position(
            saved_state.monitor_index,
            saved_state.logical_position,
            monitors,
        );
    if used_fallback {
        warn!(
            "[restore_managed_window] Target monitor {} not found, falling back to monitor 0",
            saved_state.monitor_index
        );
    }

    let decoration = winit_info.decoration();

    // The window is created on the focused window's monitor (the primary window's monitor)
    // without explicit positioning. Its starting scale matches the primary monitor, not the
    // target monitor.
    let target = restore_plan::compute_target_position(
        saved_state,
        target_info,
        fallback_position,
        decoration,
        primary_scale,
        platform,
    );

    debug!(
        "[restore_managed_window] saved_pos={:?} clamped_pos={:?} target_scale={} logical={}x{} physical={}x{} monitor={} mon_pos=({},{}) mon_size=({},{})",
        saved_state.logical_position,
        target.position,
        target.target_scale,
        target.logical_width,
        target.logical_height,
        target.width,
        target.height,
        target.target_monitor_index,
        target_info.position.x,
        target_info.position.y,
        target_info.size.x,
        target_info.size.y,
    );

    let is_fullscreen = saved_state.mode.is_fullscreen();
    commands.entity(entity).insert(target);

    // Insert `X11FrameCompensated` for platforms that don't need compensation.
    // For fullscreen modes, skip frame compensation — frame extents are irrelevant
    // and delaying restore gives the compositor time to revert position changes.
    if is_fullscreen || !platform.needs_frame_compensation() {
        commands.entity(entity).insert(X11FrameCompensated);
    }
}

/// Run condition: returns true if any entity has a `TargetPosition` component.
fn has_restoring_windows(q: Query<(), With<TargetPosition>>) -> bool { !q.is_empty() }

/// Run condition: returns true if no entity has a `TargetPosition` component.
fn no_restoring_windows(q: Query<(), With<TargetPosition>>) -> bool { q.is_empty() }

/// The run conditions allow us to separate the initial primary window restore from
/// subsequent positions saves - which we dont' want to do until AFTER we've done
/// the initial restore.
pub(super) fn build_plugin(app: &mut App, path: PathBuf, persistence: ManagedWindowPersistence) {
    let platform = Platform::detect();
    app.insert_resource(platform);

    // Hide primary window to prevent flash at default position.
    // Two cases to handle:
    // 1. Window already exists (WindowManagerPlugin added after DefaultPlugins) - hide immediately
    // 2. Window doesn't exist yet (WindowManagerPlugin added before DefaultPlugins) - use observer
    //
    // EXCEPTION: On Linux X11 with frame extent compensation (workaround-winit-4445),
    // we cannot hide the window because the compensation system needs to query
    // _NET_FRAME_EXTENTS, which requires the window to be visible/mapped.
    let should_hide = platform.should_hide_on_startup();

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

    #[cfg(target_os = "macos")]
    super::macos_tabbing_fix::init(app);

    #[cfg(all(target_os = "windows", feature = "workaround-winit-4341"))]
    super::windows_dpi_fix::init(app);

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
        .add_systems(PreStartup, {
            #[cfg(target_os = "linux")]
            {
                // X11 fullscreen: move window to target monitor before first event loop.
                // Must be chained (not .after()) so apply_deferred runs between
                // load_target_position and move_to_target_monitor — otherwise the
                // TargetPosition component inserted via deferred commands won't exist yet.
                (
                    systems::init_winit_info,
                    systems::load_target_position,
                    systems::move_to_target_monitor,
                )
                    .chain()
                    .after(init_monitors)
            }
            #[cfg(not(target_os = "linux"))]
            {
                (systems::init_winit_info, systems::load_target_position)
                    .chain()
                    .after(init_monitors)
            }
        });

    // X11 frame extent compensation (Linux + W6 + X11 only)
    // Runs until all restoring windows have the X11FrameCompensated component
    #[cfg(all(target_os = "linux", feature = "workaround-winit-4445"))]
    app.add_systems(
        Update,
        super::x11_frame_extents::compensate_target_position
            .run_if(has_restoring_windows)
            .run_if(|p: Res<Platform>| p.is_x11()),
    );

    // Restore windows - processes all entities with `TargetPosition` + `X11FrameCompensated`
    app.add_systems(
        Update,
        (
            systems::restore_windows,
            systems::check_restore_settling.after(systems::restore_windows),
        )
            .run_if(has_restoring_windows),
    );

    // Unified monitor detection + save window state
    app.add_systems(
        Update,
        (
            systems::update_current_monitor,
            systems::save_window_state
                .run_if(no_restoring_windows)
                .after(systems::update_current_monitor),
            on_persistence_changed
                .run_if(resource_changed::<ManagedWindowPersistence>)
                .run_if(no_restoring_windows)
                .after(systems::update_current_monitor),
        ),
    );
}
