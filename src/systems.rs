//! Systems for window restoration and state management.
//!
//! # Monitor Detection
//!
//! [`update_current_monitor`] is the unified system that maintains `CurrentMonitor` on all
//! managed windows. It uses winit's `current_monitor()` as the primary detection method,
//! with position-based center-point detection as a fallback. This ensures correct monitor
//! identification even for newly spawned windows whose `window.position` is still `Automatic`.
//!
//! On Wayland, `window.position` always returns `(0,0)` for security/privacy reasons, making
//! winit's `current_monitor()` the only viable detection method on that platform.

use bevy::ecs::system::NonSendMarker;
use bevy::prelude::*;
use bevy::window::MonitorSelection;
use bevy::window::PrimaryWindow;
use bevy::window::WindowMode;
use bevy::window::WindowScaleFactorChanged;
use bevy::winit::WINIT_WINDOWS;

use super::state;
use super::types::RestoreWindowConfig;
use super::types::WindowState;
use crate::ManagedWindow;
use crate::Platform;
use crate::WindowKey;
use crate::monitors::CurrentMonitor;
use crate::monitors::MonitorInfo;
use crate::monitors::Monitors;
use crate::restore_plan;
use crate::types::FullscreenRestoreState;
use crate::types::MonitorScaleStrategy;
use crate::types::SCALE_FACTOR_EPSILON;
use crate::types::SavedWindowMode;
use crate::types::SettleSnapshot;
use crate::types::SettleState;
use crate::types::TargetPosition;
use crate::types::WindowDecoration;
use crate::types::WindowRestoreMismatch;
use crate::types::WindowRestoreState;
use crate::types::WindowRestored;
use crate::types::WinitInfo;
use crate::types::X11FrameCompensated;

/// Populate `WinitInfo` resource from winit (decoration and starting monitor).
///
/// # Panics
///
/// Panics if no monitors are available (e.g., laptop lid closed at startup).
/// Window management requires at least one monitor to function.
pub fn init_winit_info(
    mut commands: Commands,
    window_entity: Single<Entity, With<PrimaryWindow>>,
    monitors: Res<Monitors>,
    _non_send: NonSendMarker,
) {
    assert!(
        !monitors.is_empty(),
        "No monitors available - cannot initialize window manager without a display"
    );

    WINIT_WINDOWS.with(|ww| {
        let ww = ww.borrow();
        if let Some(winit_window) = ww.get_window(*window_entity) {
            let outer = winit_window.outer_size();
            let inner = winit_window.inner_size();
            let decoration = WindowDecoration {
                width:  outer.width.saturating_sub(inner.width),
                height: outer.height.saturating_sub(inner.height),
            };

            // Get actual position from winit to determine starting monitor
            let pos = winit_window
                .outer_position()
                .map(|p| IVec2::new(p.x, p.y))
                .unwrap_or(IVec2::ZERO);

            debug!(
                "[init_winit_info] outer_position={pos:?} platform={:?}",
                Platform::detect()
            );

            // Use winit's current_monitor() as the primary source for starting monitor.
            // Falls back to position-based detection if current_monitor() returns None.
            let starting_monitor = winit_window
                .current_monitor()
                .and_then(|cm| {
                    let cm_pos = cm.position();
                    let info = monitors.at(cm_pos.x, cm_pos.y);
                    debug!(
                        "[init_winit_info] current_monitor() position=({}, {}) -> index={:?}",
                        cm_pos.x, cm_pos.y, info.map(|m| m.index)
                    );
                    info.copied()
                })
                .unwrap_or_else(|| {
                    debug!(
                        "[init_winit_info] current_monitor() unavailable, falling back to closest_to({}, {})",
                        pos.x, pos.y
                    );
                    *monitors.closest_to(pos.x, pos.y)
                });
            let starting_monitor_index = starting_monitor.index;

            debug!(
                "[init_winit_info] decoration={}x{} pos=({}, {}) starting_monitor={}",
                decoration.width, decoration.height, pos.x, pos.y, starting_monitor_index
            );

            // Insert initial CurrentMonitor component on window entity
            commands
                .entity(*window_entity)
                .insert(CurrentMonitor {
                    monitor:        starting_monitor,
                    effective_mode: WindowMode::Windowed,
                });

            commands.insert_resource(WinitInfo {
                starting_monitor_index,
                window_decoration: decoration,
            });
        }
    });
}

/// Load saved window state and insert `TargetPosition` component on the primary window entity.
///
/// Runs after `init_winit_info` so we have access to starting monitor info.
pub fn load_target_position(
    mut commands: Commands,
    window_entity: Single<Entity, With<PrimaryWindow>>,
    monitors: Res<Monitors>,
    winit_info: Res<WinitInfo>,
    mut config: ResMut<RestoreWindowConfig>,
    platform: Res<Platform>,
) {
    // Load all states from the file into `loaded_states` as a startup snapshot.
    // This must happen before any managed window observers fire so they can check
    // `loaded_states` instead of re-reading the file (which may have been modified
    // by `on_managed_window_added` saving initial state for new windows).
    if let Some(all_states) = state::load_all_states(&config.path) {
        config.loaded_states = all_states;
    }

    let Some(state) = config.loaded_states.get(&WindowKey::Primary).cloned() else {
        debug!("[load_target_position] No saved bevy_window_manager state, showing window");
        // No saved state - show window at default position (user may have started hidden)
        commands.queue(|world: &mut World| {
            let mut query = world.query_filtered::<&mut Window, With<PrimaryWindow>>();
            if let Some(mut window) = query.iter_mut(world).next() {
                window.visible = true;
            }
        });
        return;
    };

    debug!(
        "[load_target_position] Loaded state: position={:?} logical_size={}x{} monitor_scale={} monitor_index={} mode={:?}",
        state.logical_position,
        state.logical_width,
        state.logical_height,
        state.monitor_scale,
        state.monitor_index,
        state.mode
    );

    // Get starting monitor from WinitInfo
    let starting_monitor_index = winit_info.starting_monitor_index;
    let starting_info = monitors.by_index(starting_monitor_index);
    let starting_scale = starting_info.map_or(1.0, |m| m.scale);

    let (target_info, fallback_position, used_fallback) =
        restore_plan::resolve_target_monitor_and_position(
            state.monitor_index,
            state.logical_position,
            &monitors,
        );
    if used_fallback {
        warn!(
            "[load_target_position] Target monitor {} not found, falling back to monitor 0",
            state.monitor_index
        );
    }

    let target = restore_plan::compute_target_position(
        &state,
        target_info,
        fallback_position,
        winit_info.decoration(),
        starting_scale,
        *platform,
    );

    debug!(
        "[load_target_position] Starting monitor={} scale={}, Target monitor={} scale={}, strategy={:?}, position={:?}",
        starting_monitor_index,
        starting_scale,
        target.target_monitor_index,
        target.target_scale,
        target.monitor_scale_strategy,
        target.position
    );

    // Windows W3 workaround (winit #3124): For exclusive fullscreen restore, we must
    // show the window to ensure surfaces are created before the workaround applies
    // fullscreen mode. Otherwise, we want visible = false to prevent the flickering
    // jump from the default position to the restored position.
    #[cfg(all(target_os = "windows", feature = "workaround-winit-3124"))]
    if matches!(state.mode, SavedWindowMode::Fullscreen { .. }) {
        debug!(
            "[load_target_position] Windows exclusive fullscreen: showing window for surface creation"
        );
        commands.queue(|world: &mut World| {
            let mut query = world.query_filtered::<&mut Window, With<PrimaryWindow>>();
            if let Some(mut window) = query.iter_mut(world).next() {
                window.visible = true;
            }
        });
    }

    let entity = *window_entity;
    let is_fullscreen = state.mode.is_fullscreen();
    commands.entity(entity).insert(target);

    // Insert X11FrameCompensated token for platforms that don't need compensation.
    // On Linux + W6 + X11, the compensation system inserts this token after adjusting position.
    // For fullscreen modes, skip frame compensation entirely — the window will cover the whole
    // screen so frame extents are irrelevant, and delaying restore_windows by extra frames
    // gives the compositor time to revert our PreStartup position change.
    if is_fullscreen || !platform.needs_frame_compensation() {
        commands.entity(entity).insert(X11FrameCompensated);
    }
}

/// Move the primary window to the target monitor for fullscreen restore on X11.
///
/// On X11, the compositor (`KWin` via `XWayland`) reverts fullscreen if the position
/// and mode changes arrive in the same `changed_windows` pass. By setting position
/// in a separate `PreStartup` system (direct mutation, not commands.queue), the change
/// is processed by `bevy_winit` before the `Update` system applies fullscreen mode.
///
/// Skipped on Wayland (no position) and non-fullscreen modes.
/// For managed windows, the equivalent happens in `on_managed_window_load`.
#[cfg(target_os = "linux")]
pub fn move_to_target_monitor(
    mut window: Single<&mut Window, With<PrimaryWindow>>,
    targets: Query<&TargetPosition, With<PrimaryWindow>>,
    platform: Res<Platform>,
) {
    if platform.is_wayland() {
        return;
    }

    let Ok(target) = targets.single() else {
        return;
    };

    if !target.mode.is_fullscreen() {
        return;
    }

    if let Some(pos) = target.position {
        debug!("[move_to_target_monitor] X11 fullscreen: setting position={pos:?}");
        window.position = WindowPosition::At(pos);
    }
}

/// Apply the initial window move to the target monitor.
///
/// Sets position and size based on the `TargetPosition` strategy, handling fullscreen,
/// Wayland (no position), and cross-DPI scenarios. Called from `restore_windows` during
/// the `NeedInitialMove` phase for `HigherToLower` and `CompensateSizeOnly` strategies.
///
/// On macOS with `HigherToLower` strategy, the position is compensated because winit
/// divides coordinates by the launch monitor's scale factor.
///
/// On Windows with `CompensateSizeOnly`, position is applied directly and size is
/// compensated by `starting_scale / target_scale`. Phase 2 re-applies the exact size.
///
/// For fullscreen modes, we still move to the target monitor so the fullscreen mode
/// is applied on the correct monitor when `try_apply_restore` runs.
pub fn apply_initial_move(target: &TargetPosition, window: &mut Window) {
    /// Computed parameters for the initial window move to target monitor.
    #[derive(Debug)]
    struct MoveParams {
        position: IVec2,
        width:    u32,
        height:   u32,
    }

    // For fullscreen modes, just move to target monitor position (no 1x1 size)
    // The fullscreen mode will be applied later in try_apply_restore
    if target.mode.is_fullscreen() {
        if let Some(pos) = target.position {
            debug!(
                "[apply_initial_move] Moving to target position {:?} for fullscreen mode {:?}",
                pos, target.mode
            );
            window.position = WindowPosition::At(pos);
        } else {
            debug!(
                "[apply_initial_move] No position available (Wayland), fullscreen mode {:?}",
                target.mode
            );
        }
        return;
    }

    // Position may be None on Wayland - skip position setting if unavailable
    let Some(pos) = target.position else {
        debug!(
            "[apply_initial_move] No position available (Wayland), setting size only: {}x{}",
            target.width, target.height
        );
        debug!(
            "[apply_initial_move] BEFORE set_physical_resolution: physical={}x{} logical={}x{} scale={}",
            window.resolution.physical_width(),
            window.resolution.physical_height(),
            window.resolution.width(),
            window.resolution.height(),
            window.resolution.scale_factor()
        );
        window
            .resolution
            .set_physical_resolution(target.width, target.height);
        debug!(
            "[apply_initial_move] AFTER set_physical_resolution: physical={}x{} logical={}x{} scale={}",
            window.resolution.physical_width(),
            window.resolution.physical_height(),
            window.resolution.width(),
            window.resolution.height(),
            window.resolution.scale_factor()
        );
        return;
    };

    // Compute move parameters based on scale strategy
    let params = match target.monitor_scale_strategy {
        MonitorScaleStrategy::HigherToLower(_) => {
            // Compensate position because winit divides by launch scale
            let ratio = target.starting_scale / target.target_scale;
            let comp_x = (f64::from(pos.x) * ratio) as i32;
            let comp_y = (f64::from(pos.y) * ratio) as i32;
            debug!(
                "[apply_initial_move] HigherToLower: compensating position {:?} -> ({}, {}) (ratio={})",
                pos, comp_x, comp_y, ratio
            );
            MoveParams {
                position: IVec2::new(comp_x, comp_y),
                // Use actual target size to avoid macOS caching tiny size
                width:    target.width,
                height:   target.height,
            }
        },
        MonitorScaleStrategy::CompensateSizeOnly(_) => {
            // Position applied directly, size compensated to survive DPI transition.
            // Phase 2 will re-apply the exact target size after ScaleFactorChanged.
            let compensated = target.compensated_size();
            debug!(
                "[apply_initial_move] CompensateSizeOnly: position={:?} compensated_size={}x{} (ratio={})",
                pos,
                compensated.x,
                compensated.y,
                target.ratio()
            );
            MoveParams {
                position: pos,
                width:    compensated.x,
                height:   compensated.y,
            }
        },
        _ => MoveParams {
            position: pos,
            width:    target.width,
            height:   target.height,
        },
    };

    debug!(
        "[apply_initial_move] position={:?} size={}x{} visible={}",
        params.position, params.width, params.height, window.visible
    );

    window.position = WindowPosition::At(params.position);
    window
        .resolution
        .set_physical_resolution(params.width, params.height);
}

/// Cached window state for change detection comparison.
#[derive(Default)]
pub struct CachedWindowState {
    position:       Option<IVec2>,
    logical_width:  u32,
    logical_height: u32,
    mode:           Option<SavedWindowMode>,
    monitor_index:  Option<usize>,
}

/// Build state from all currently-active windows and write it to the state file.
///
/// Iterates every primary and managed window, captures position/size/monitor/mode,
/// and writes the full persisted state map in one shot. Used by the
/// `ActiveOnly` persistence mode so that the file always reflects exactly which
/// windows are open right now.
///
/// `exclude_entity` allows callers (e.g., `On<Remove>` observers) to skip an entity
/// whose component is still visible in the query but is being removed.
#[allow(clippy::type_complexity)]
pub fn save_active_window_state(
    config: &RestoreWindowConfig,
    monitors: &Monitors,
    all_windows: &Query<
        (
            Entity,
            &Window,
            Option<&CurrentMonitor>,
            Option<&crate::ManagedWindow>,
        ),
        Or<(With<PrimaryWindow>, With<crate::ManagedWindow>)>,
    >,
    primary_q: &Query<(), With<PrimaryWindow>>,
    exclude_entity: Option<Entity>,
) {
    if monitors.is_empty() {
        return;
    }

    let app_name = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_stem().and_then(|s| s.to_str()).map(String::from))
        .unwrap_or_default();

    let mut states = std::collections::HashMap::new();

    for (entity, window, existing_monitor, managed) in all_windows {
        if exclude_entity == Some(entity) {
            continue;
        }

        let key = if primary_q.get(entity).is_ok() {
            WindowKey::Primary
        } else if let Some(m) = managed {
            WindowKey::Managed(m.window_name.clone())
        } else {
            continue;
        };

        let pos = get_window_position(entity, window);

        let (monitor_index, monitor_scale) = existing_monitor.map_or_else(
            || {
                let p = monitors.first();
                (p.index, p.scale)
            },
            |m| (m.index, m.scale),
        );
        let mode: SavedWindowMode =
            existing_monitor.map_or_else(|| (&window.mode).into(), |m| (&m.effective_mode).into());
        let logical_position = pos.map(|p| {
            let lx = (p.x as f64 / monitor_scale).round() as i32;
            let ly = (p.y as f64 / monitor_scale).round() as i32;
            (lx, ly)
        });
        states.insert(
            key,
            WindowState {
                logical_position,
                logical_width: window.resolution.width() as u32,
                logical_height: window.resolution.height() as u32,
                monitor_scale,
                monitor_index,
                mode,
                app_name: app_name.clone(),
            },
        );
    }

    state::save_all_states(&config.path, &states);
}

/// Save window state when position, size, or mode changes. Runs only when not restoring.
///
/// Handles both the primary window and any `ManagedWindow` entities. Uses
/// `ManagedWindowPersistence` to decide whether closed windows keep their saved state.
#[allow(
    clippy::type_complexity,
    clippy::too_many_lines,
    clippy::too_many_arguments,
    clippy::option_if_let_else
)]
pub fn save_window_state(
    config: Res<RestoreWindowConfig>,
    monitors: Res<Monitors>,
    persistence: Res<crate::ManagedWindowPersistence>,
    windows: Query<
        (
            Entity,
            &Window,
            Option<&CurrentMonitor>,
            Option<&crate::ManagedWindow>,
        ),
        (
            Or<(With<PrimaryWindow>, With<crate::ManagedWindow>)>,
            Or<(Changed<Window>, Changed<CurrentMonitor>)>,
        ),
    >,
    all_windows: Query<
        (
            Entity,
            &Window,
            Option<&CurrentMonitor>,
            Option<&crate::ManagedWindow>,
        ),
        Or<(With<PrimaryWindow>, With<crate::ManagedWindow>)>,
    >,
    primary_q: Query<(), With<PrimaryWindow>>,
    mut cached: Local<std::collections::HashMap<Entity, CachedWindowState>>,
    _non_send: NonSendMarker,
) {
    // Can't save state if no monitors exist (e.g., laptop lid closed).
    if monitors.is_empty() {
        return;
    }

    let mut any_changed = false;

    for (window_entity, window, existing_monitor, managed) in &windows {
        // Determine the key for this window in the state file
        let key = if primary_q.get(window_entity).is_ok() {
            WindowKey::Primary
        } else if let Some(m) = managed {
            WindowKey::Managed(m.window_name.clone())
        } else {
            continue;
        };

        // Get window position for saving state.
        let pos = get_window_position(window_entity, window);

        let physical_w = window.resolution.physical_width();
        let physical_h = window.resolution.physical_height();
        let logical_w = window.resolution.width() as u32;
        let logical_h = window.resolution.height() as u32;
        let res_scale = window.resolution.scale_factor();

        // Read monitor and effective mode from `CurrentMonitor` (maintained by
        // `update_current_monitor`)
        let (monitor_index, monitor_scale) = existing_monitor.map_or_else(
            || {
                let p = monitors.first();
                (p.index, p.scale)
            },
            |m| (m.index, m.scale),
        );
        let mode: SavedWindowMode =
            existing_monitor.map_or_else(|| (&window.mode).into(), |m| (&m.effective_mode).into());

        let entry = cached.entry(window_entity).or_default();

        // Only save if position, size, or mode actually changed
        let position_changed = entry.position != pos;
        let size_changed = entry.logical_width != logical_w || entry.logical_height != logical_h;
        let mode_changed = entry.mode.as_ref() != Some(&mode);
        let monitor_changed = entry.monitor_index != Some(monitor_index);

        if !position_changed && !size_changed && !mode_changed && !monitor_changed {
            continue;
        }

        debug!(
            "[save_window_state] [{key}] SAVE DETAIL: pos={pos:?} physical={physical_w}x{physical_h} logical={logical_w}x{logical_h} res_scale={res_scale} monitor={monitor_index} mode={mode:?}",
        );

        // Log monitor transitions with detailed info
        if monitor_changed {
            let prev_scale = entry
                .monitor_index
                .and_then(|i| monitors.by_index(i))
                .map(|m| m.scale);
            debug!(
                "[save_window_state] [{key}] MONITOR CHANGE: {:?} (scale={:?}) -> {} (scale={})",
                entry.monitor_index, prev_scale, monitor_index, monitor_scale
            );
        }

        // Update cache
        entry.position = pos;
        entry.logical_width = logical_w;
        entry.logical_height = logical_h;
        entry.mode = Some(mode.clone());
        entry.monitor_index = Some(monitor_index);

        any_changed = true;

        debug!(
            "[save_window_state] [{key}] pos={pos:?} logical={logical_w}x{logical_h} physical={physical_w}x{physical_h} monitor={monitor_index} scale={monitor_scale} mode={mode:?}",
        );
    }

    if !any_changed {
        return;
    }

    match *persistence {
        crate::ManagedWindowPersistence::ActiveOnly => {
            // Build state from all active windows and write in one shot
            save_active_window_state(&config, &monitors, &all_windows, &primary_q, None);
        },
        crate::ManagedWindowPersistence::RememberAll => {
            // Load existing file first to preserve closed windows, then merge cache
            let app_name = std::env::current_exe()
                .ok()
                .and_then(|p| p.file_stem().and_then(|s| s.to_str()).map(String::from))
                .unwrap_or_default();

            let mut states = state::load_all_states(&config.path).unwrap_or_default();

            // Update with current window states from cache
            for (entity, entry) in &*cached {
                let key = if primary_q.get(*entity).is_ok() {
                    WindowKey::Primary
                } else if let Ok((_, _, _, Some(managed))) = all_windows.get(*entity) {
                    WindowKey::Managed(managed.window_name.clone())
                } else {
                    // Entity may have been despawned - skip stale cached entry
                    continue;
                };

                if let Some(mode) = &entry.mode {
                    let monitor_index = entry.monitor_index.unwrap_or(0);
                    let monitor_scale = monitors.by_index(monitor_index).map_or(1.0, |m| m.scale);
                    let logical_position = entry.position.map(|p| {
                        let lx = (p.x as f64 / monitor_scale).round() as i32;
                        let ly = (p.y as f64 / monitor_scale).round() as i32;
                        (lx, ly)
                    });
                    states.insert(
                        key,
                        WindowState {
                            logical_position,
                            logical_width: entry.logical_width,
                            logical_height: entry.logical_height,
                            monitor_scale,
                            monitor_index,
                            mode: mode.clone(),
                            app_name: app_name.clone(),
                        },
                    );
                }
            }

            state::save_all_states(&config.path, &states);
        },
    }
}

/// Apply pending window restore. Runs only when entities with `TargetPosition` exist.
/// Processes all windows with both `TargetPosition` and `X11FrameCompensated` components.
///
/// Requires `NonSendMarker` because we access `WINIT_WINDOWS` thread-local to gate
/// restore on the winit window existing. Without this gate, managed windows spawned
/// at runtime would have their physical size set by `set_physical_resolution()` and
/// then doubled by `create_windows` → `set_scale_factor_and_apply_to_physical_size()`
/// which runs between frames.
#[allow(clippy::too_many_arguments)]
pub fn restore_windows(
    mut scale_changed_messages: MessageReader<WindowScaleFactorChanged>,
    mut windows: Query<(Entity, &mut TargetPosition, &mut Window), With<X11FrameCompensated>>,
    _non_send: NonSendMarker,
    platform: Res<Platform>,
) {
    let scale_changed = scale_changed_messages.read().last().is_some();

    for (entity, mut target, mut window) in &mut windows {
        // Wait for the winit window to be created before applying restore.
        //
        // Primary window: `create_windows` runs during winit `init()` before any Bevy
        // schedules, so the winit window always exists when we get here.
        //
        // Managed windows: spawned at runtime, `create_windows` runs between frames via
        // `WinitUserEvent::WindowAdded`. If we apply `set_physical_resolution()` before
        // `create_windows` runs, `create_windows` will call
        // `set_scale_factor_and_apply_to_physical_size(scale)` which multiplies our
        // already-correct physical size by the scale factor (doubling it on 2x displays).
        // Already settling — skip restore logic, handled by check_restore_settling
        if target.settle_state.is_some() {
            continue;
        }

        let winit_window_exists = WINIT_WINDOWS.with(|ww| ww.borrow().get_window(entity).is_some());
        if !winit_window_exists {
            debug!("[restore_windows] Skipping entity {entity:?}: winit window not yet created");
            continue;
        }

        // Managed windows may be created on a different monitor than assumed.
        // `starting_scale` was computed from the primary window's current monitor,
        // but Windows OS places new windows on the OS primary display (which may differ).
        // Detect this and recalculate the scale strategy with the actual creation scale.
        if platform.needs_managed_scale_fixup() {
            let actual_scale = f64::from(window.resolution.base_scale_factor());
            if (actual_scale - target.starting_scale).abs() > SCALE_FACTOR_EPSILON {
                let old_strategy = target.monitor_scale_strategy;
                target.starting_scale = actual_scale;
                target.monitor_scale_strategy =
                    platform.scale_strategy(actual_scale, target.target_scale);
                debug!(
                    "[restore_windows] Corrected starting_scale for entity {entity:?}: \
                     strategy: {old_strategy:?} -> {:?} (actual_scale={actual_scale:.2})",
                    target.monitor_scale_strategy
                );
            }
        }

        // Two-phase restore for cross-DPI strategies (HigherToLower, CompensateSizeOnly).
        // Phase 1: apply_initial_move sets compensated position/size to trigger DPI change.
        // Phase 2: after ScaleFactorChanged, re-apply exact target size.
        if matches!(
            target.monitor_scale_strategy,
            MonitorScaleStrategy::HigherToLower(WindowRestoreState::NeedInitialMove)
                | MonitorScaleStrategy::CompensateSizeOnly(WindowRestoreState::NeedInitialMove)
        ) {
            apply_initial_move(&target, &mut window);
            target.monitor_scale_strategy = match target.monitor_scale_strategy {
                MonitorScaleStrategy::HigherToLower(_) => {
                    MonitorScaleStrategy::HigherToLower(WindowRestoreState::WaitingForScaleChange)
                },
                _ => MonitorScaleStrategy::CompensateSizeOnly(
                    WindowRestoreState::WaitingForScaleChange,
                ),
            };
            continue;
        }

        // Handle state transition on scale change for both strategies.
        // CompensateSizeOnly: also advance if no scale change arrives (e.g., hidden window
        // didn't trigger WM_DPICHANGED, or the app launched on the target monitor).
        match target.monitor_scale_strategy {
            MonitorScaleStrategy::HigherToLower(WindowRestoreState::WaitingForScaleChange)
                if scale_changed =>
            {
                debug!(
                    "[Restore] ScaleChanged received, transitioning to WindowRestoreState::ApplySize"
                );
                target.monitor_scale_strategy =
                    MonitorScaleStrategy::HigherToLower(WindowRestoreState::ApplySize);
            },
            MonitorScaleStrategy::CompensateSizeOnly(WindowRestoreState::WaitingForScaleChange) => {
                debug!(
                    "[Restore] CompensateSizeOnly: transitioning to ApplySize (scale_changed={scale_changed})"
                );
                target.monitor_scale_strategy =
                    MonitorScaleStrategy::CompensateSizeOnly(WindowRestoreState::ApplySize);
            },
            _ => {},
        }

        // Fullscreen phase state machine.
        // Phases: MoveToMonitor → WaitForMove → ApplyMode (Linux X11)
        //         WaitForSurface → ApplyMode (Windows DX12)
        //         ApplyMode (Wayland, macOS — direct apply)
        if let Some(fs_state) = target.fullscreen_restore_state {
            match fs_state {
                FullscreenRestoreState::MoveToMonitor => {
                    // Move window to target monitor so compositor knows where it belongs
                    if let Some(pos) = target.position {
                        debug!("[restore_windows] Fullscreen MoveToMonitor: position={pos:?}");
                        window.position = WindowPosition::At(pos);
                    }
                    target.fullscreen_restore_state = Some(FullscreenRestoreState::WaitForMove);
                    continue;
                },
                FullscreenRestoreState::WaitForMove => {
                    // Wait one frame for compositor to process the position change
                    debug!("[restore_windows] Fullscreen WaitForMove: waiting for compositor");
                    target.fullscreen_restore_state = Some(FullscreenRestoreState::ApplyMode);
                    continue;
                },
                FullscreenRestoreState::WaitForSurface => {
                    // Wait one frame for GPU surface creation (Windows DX12, winit #3124)
                    debug!("[restore_windows] Fullscreen WaitForSurface: waiting for GPU surface");
                    target.fullscreen_restore_state = Some(FullscreenRestoreState::ApplyMode);
                    continue;
                },
                FullscreenRestoreState::ApplyMode => {
                    // Fall through to try_apply_restore which applies the fullscreen mode
                },
            }
        }

        if matches!(
            try_apply_restore(&target, &mut window, *platform),
            RestoreStatus::Complete
        ) {
            // Restore applied — start settle timer to wait for compositor/winit to
            // deliver matching state before declaring success or mismatch.
            if target.settle_state.is_none() {
                info!(
                    "[restore_windows] Restore applied, starting settle (200ms stability / 1s timeout)"
                );
                target.settle_state = Some(SettleState::new());
            }
        }
    }
}

/// Build a [`SettleSnapshot`] from the current window state, returning the snapshot
/// and the actual scale factor (tracked separately since scale is informational).
fn build_actual_snapshot(
    window: &Window,
    current_monitor: Option<&CurrentMonitor>,
    platform: Platform,
) -> (SettleSnapshot, f64) {
    let position = if platform.position_available() {
        match window.position {
            bevy::window::WindowPosition::At(p) => Some(IVec2::new(p.x, p.y)),
            _ => None,
        }
    } else {
        None
    };
    let size = UVec2::new(
        window.resolution.physical_width(),
        window.resolution.physical_height(),
    );
    (
        SettleSnapshot {
            position,
            size,
            mode: window.mode,
            monitor: current_monitor.map_or(0, |cm| cm.monitor.index),
        },
        f64::from(window.resolution.scale_factor()),
    )
}

/// Check whether actual window state matches the target for settle purposes.
///
/// Fullscreen modes skip position and size comparison — the window fills the
/// monitor so the stored position/size are irrelevant. On macOS, borderless
/// fullscreen reports position offset by the menu bar height; on X11 (W6),
/// frame vs client coords differ. The physical size can also differ when
/// scales differ between backends (e.g. Wayland scale 1 vs `XWayland` scale 2).
fn check_settle_matches(
    target: &TargetPosition,
    target_position: Option<IVec2>,
    target_size: UVec2,
    target_mode: WindowMode,
    target_monitor: usize,
    actual: &SettleSnapshot,
    platform: Platform,
) -> (bool, bool, bool, bool) {
    let is_fullscreen = target.mode.is_fullscreen();
    let pos_match = if is_fullscreen {
        true
    } else if platform.position_reliable_for_settle() {
        target_position == actual.position
    } else {
        true // X11 W6: target is frame coords, actual is client area — skip
    };
    let size_match = is_fullscreen || target_size == actual.size;
    let mode_match = platform.modes_match(target_mode, actual.mode);
    let monitor_match = target_monitor == actual.monitor;
    (pos_match, size_match, mode_match, monitor_match)
}

/// Detect whether the settle snapshot changed from the previous frame and reset the
/// stability timer if so. Returns `true` if the caller should skip further checks
/// this frame (snapshot just changed and we haven't timed out yet).
fn detect_settle_change(
    settle: &mut SettleState,
    snapshot: SettleSnapshot,
    key: &WindowKey,
    total_elapsed_ms: f32,
    total_timed_out: bool,
) -> bool {
    let changed = settle.last_snapshot.as_ref() != Some(&snapshot);
    if changed {
        if settle.last_snapshot.is_some() {
            debug!(
                "[check_restore_settling] [{key}] {total_elapsed_ms:.0}ms: values changed, \
                 resetting stability timer"
            );
        }
        settle.stability_timer.reset();
        settle.last_snapshot = Some(snapshot);
        // Don't check stability this frame — we just reset
        !total_timed_out
    } else {
        false
    }
}

/// Resolve the [`WindowKey`] for an entity — `Primary` if it has the `PrimaryWindow`
/// marker, otherwise the `ManagedWindow` name (falling back to `Primary`).
fn resolve_window_key(
    entity: Entity,
    primary_q: &Query<(), With<PrimaryWindow>>,
    managed_q: &Query<&ManagedWindow>,
) -> WindowKey {
    if primary_q.get(entity).is_ok() {
        WindowKey::Primary
    } else if let Ok(managed) = managed_q.get(entity) {
        WindowKey::Managed(managed.window_name.clone())
    } else {
        WindowKey::Primary
    }
}

/// Check settling windows each frame using a two-timer approach.
///
/// - **Stability timer** (200ms): resets whenever any compared value changes. If values stay stable
///   for 200ms, fires `WindowRestored`.
/// - **Total timeout** (1s): hard deadline. Fires `WindowRestoreMismatch` if stability is never
///   reached.
///
/// Runs while `TargetPosition` entities exist (same gate as `restore_windows`).
/// Only processes entities that have a `settle_state` set.
pub fn check_restore_settling(
    mut commands: Commands,
    time: Res<Time>,
    mut windows: Query<
        (
            Entity,
            &mut TargetPosition,
            &Window,
            Option<&CurrentMonitor>,
        ),
        With<X11FrameCompensated>,
    >,
    primary_q: Query<(), With<PrimaryWindow>>,
    managed_q: Query<&crate::ManagedWindow>,
    platform: Res<Platform>,
) {
    for (entity, mut target, window, current_monitor) in &mut windows {
        // Read target fields before borrowing settle_state mutably
        let target_mode = target.mode.to_window_mode(target.target_monitor_index);
        let target_size = target.size();
        let target_logical_size = target.logical_size();
        let target_monitor = target.target_monitor_index;
        let expected_scale = target.target_scale;

        let target_position = platform
            .position_available()
            .then_some(target.position)
            .flatten();
        let key = resolve_window_key(entity, &primary_q, &managed_q);
        let (current_snapshot, actual_scale) =
            build_actual_snapshot(window, current_monitor, *platform);

        // Now borrow settle_state mutably for timer ticking and change detection
        let Some(settle) = target.settle_state.as_mut() else {
            continue;
        };
        settle.total_timeout.tick(time.delta());
        settle.stability_timer.tick(time.delta());

        let total_elapsed_ms = settle.total_timeout.elapsed_secs() * 1000.0;
        let stability_elapsed_ms = settle.stability_timer.elapsed_secs() * 1000.0;
        let total_timed_out = settle.total_timeout.is_finished();

        if detect_settle_change(
            settle,
            current_snapshot,
            &key,
            total_elapsed_ms,
            total_timed_out,
        ) {
            continue;
        }
        let stable = settle.stability_timer.is_finished();
        let (pos_match, size_match, mode_match, monitor_match) = check_settle_matches(
            &target,
            target_position,
            target_size,
            target_mode,
            target_monitor,
            &current_snapshot,
            *platform,
        );
        let all_match = pos_match && size_match && mode_match && monitor_match;
        debug!(
            "[check_restore_settling] [{key}] {total_elapsed_ms:.0}ms (stable: {stability_elapsed_ms:.0}ms): \
             pos={pos_match} size={size_match} mode={mode_match} monitor={monitor_match} | \
             size: {target_size} vs {}, \
             mode: {target_mode:?} vs {:?}, \
             monitor: {target_monitor} vs {}, \
             scale: {expected_scale} vs {actual_scale}",
            current_snapshot.size, current_snapshot.mode, current_snapshot.monitor,
        );

        if stable && all_match {
            info!(
                "[check_restore_settling] [{key}] Settled after {total_elapsed_ms:.0}ms \
                 (stable for {stability_elapsed_ms:.0}ms)"
            );
            commands
                .entity(entity)
                .trigger(|entity| WindowRestored {
                    entity,
                    window_id: key,
                    position: target_position,
                    size: target_size,
                    logical_size: target_logical_size,
                    mode: target_mode,
                    monitor_index: target_monitor,
                })
                .remove::<TargetPosition>()
                .remove::<X11FrameCompensated>();
        } else if total_timed_out {
            warn!(
                "[check_restore_settling] [{key}] Settle timeout after {total_elapsed_ms:.0}ms — \
                 mismatch remains: \
                 position: {target_position:?} vs {:?}, \
                 size: {target_size} vs {}, \
                 mode: {target_mode:?} vs {:?}, \
                 monitor: {target_monitor} vs {}, \
                 scale: {expected_scale} vs {actual_scale}",
                current_snapshot.position,
                current_snapshot.size,
                current_snapshot.mode,
                current_snapshot.monitor,
            );
            let actual_logical_size = UVec2::new(
                window.resolution.width() as u32,
                window.resolution.height() as u32,
            );
            commands
                .entity(entity)
                .trigger(|entity| WindowRestoreMismatch {
                    entity,
                    window_id: key,
                    expected_position: target_position,
                    actual_position: current_snapshot.position,
                    expected_size: target_size,
                    actual_size: current_snapshot.size,
                    expected_logical_size: target_logical_size,
                    actual_logical_size,
                    expected_mode: target_mode,
                    actual_mode: current_snapshot.mode,
                    expected_monitor: target_monitor,
                    actual_monitor: current_snapshot.monitor,
                    expected_scale,
                    actual_scale,
                })
                .remove::<TargetPosition>()
                .remove::<X11FrameCompensated>();
        }
    }
}

/// Result of attempting to apply a window restore.
enum RestoreStatus {
    /// Restore completed successfully.
    Complete,
    /// Waiting for conditions to be met (scale change, window position, etc.).
    Waiting,
}

/// Get window position, using winit's `outer_position` on Linux with W5 workaround.
#[allow(clippy::missing_const_for_fn)] // Linux branch uses WINIT_WINDOWS thread-local
fn get_window_position(entity: Entity, window: &Window) -> Option<IVec2> {
    #[cfg(all(target_os = "linux", feature = "workaround-winit-4443"))]
    {
        let _ = window;
        WINIT_WINDOWS.with(|ww| {
            let ww = ww.borrow();
            let winit_win = ww.get_window(entity)?;
            let outer_pos = winit_win.outer_position().ok()?;
            Some(IVec2::new(outer_pos.x, outer_pos.y))
        })
    }
    #[cfg(not(all(target_os = "linux", feature = "workaround-winit-4443")))]
    {
        let _ = entity;
        match window.position {
            bevy::window::WindowPosition::At(p) => Some(p),
            _ => None,
        }
    }
}

/// Unified monitor detection system. Maintains `CurrentMonitor` on all managed windows.
///
/// Detection priority:
/// 1. winit's `current_monitor()` — most reliable, works even before `window.position` is set
/// 2. Position-based center-point detection — uses `window.position` when available
/// 3. Existing `CurrentMonitor` value — preserves last-known monitor during transient states
/// 4. `monitors.first()` — last resort fallback
///
/// All platforms: computes `effective_mode` (handles macOS green button fullscreen)
#[allow(clippy::type_complexity)]
pub fn update_current_monitor(
    mut commands: Commands,
    windows: Query<
        (Entity, &Window, Option<&CurrentMonitor>),
        Or<(With<PrimaryWindow>, With<ManagedWindow>)>,
    >,
    monitors: Res<Monitors>,
    _non_send: NonSendMarker,
) {
    if monitors.is_empty() {
        return;
    }

    for (entity, window, existing) in &windows {
        let winit_result = winit_detect_monitor(entity, &monitors);
        let position_result = if winit_result.is_none() {
            position_detect_monitor(window, &monitors)
        } else {
            None
        };

        let (monitor_info, source) = match (winit_result, position_result, existing) {
            (Some(info), _, _) => (info, "winit"),
            (_, Some(info), _) => (info, "position"),
            (_, _, Some(cm)) => (cm.monitor, "existing"),
            _ => (*monitors.first(), "fallback"),
        };

        // Compute effective mode
        let effective_mode = compute_effective_mode(window, &monitor_info, &monitors);

        let new_current = CurrentMonitor {
            monitor: monitor_info,
            effective_mode,
        };

        // Only insert if changed to avoid unnecessary change detection triggers
        let changed = existing.is_none_or(|cm| {
            cm.monitor.index != new_current.monitor.index
                || cm.effective_mode != new_current.effective_mode
        });

        if changed {
            debug!(
                "[update_current_monitor] source={} index={} scale={} effective_mode={:?}",
                source, monitor_info.index, monitor_info.scale, effective_mode
            );
            commands.entity(entity).insert(new_current);
        }
    }
}

/// Detect monitor via winit's `current_monitor()`.
fn winit_detect_monitor(entity: Entity, monitors: &Monitors) -> Option<MonitorInfo> {
    WINIT_WINDOWS.with(|ww| {
        let ww = ww.borrow();
        ww.get_window(entity).and_then(|winit_window| {
            winit_window.current_monitor().and_then(|current_monitor| {
                let pos = current_monitor.position();
                monitors.at(pos.x, pos.y).copied()
            })
        })
    })
}

/// Detect monitor from `window.position` using center-point logic.
fn position_detect_monitor(window: &Window, monitors: &Monitors) -> Option<MonitorInfo> {
    if let bevy::window::WindowPosition::At(pos) = window.position {
        Some(*monitors.monitor_for_window(pos, window.physical_width(), window.physical_height()))
    } else {
        None
    }
}

/// Compute the effective window mode, including macOS green button detection.
///
/// On macOS, clicking the green "maximize" button fills the screen but `window.mode`
/// remains `Windowed`. This detects that case and returns `BorderlessFullscreen`.
fn compute_effective_mode(
    window: &Window,
    monitor_info: &MonitorInfo,
    monitors: &Monitors,
) -> WindowMode {
    // Trust exclusive fullscreen - OS manages this mode
    if matches!(window.mode, WindowMode::Fullscreen(_, _)) {
        return window.mode;
    }

    // Can't determine effective mode without monitors
    if monitors.is_empty() {
        return window.mode;
    }

    // On Wayland, position is unavailable so we can only trust self.mode
    let bevy::window::WindowPosition::At(pos) = window.position else {
        return window.mode;
    };

    // Check if window spans full width and reaches bottom of monitor
    let full_width = window.physical_width() == monitor_info.size.x;
    let left_aligned = pos.x == monitor_info.position.x;
    let reaches_bottom = pos.y + window.physical_height() as i32
        == monitor_info.position.y + monitor_info.size.y as i32;

    if full_width && left_aligned && reaches_bottom {
        WindowMode::BorderlessFullscreen(MonitorSelection::Index(monitor_info.index))
    } else {
        WindowMode::Windowed
    }
}

/// Apply fullscreen mode, handling Wayland limitations.
fn apply_fullscreen_restore(target: &TargetPosition, window: &mut Window, platform: Platform) {
    let monitor_index = target.target_monitor_index;

    // On Wayland, exclusive fullscreen is not supported by winit, so restore as
    // borderless fullscreen instead.
    let window_mode = if platform.exclusive_fullscreen_fallback()
        && matches!(target.mode, SavedWindowMode::Fullscreen { .. })
    {
        warn!(
            "Exclusive fullscreen is not supported on Wayland, restoring as BorderlessFullscreen"
        );
        WindowMode::BorderlessFullscreen(MonitorSelection::Index(monitor_index))
    } else {
        target.mode.to_window_mode(monitor_index)
    };

    debug!(
        "[Restore] Applying fullscreen mode {:?} on monitor {} -> WindowMode::{:?}",
        target.mode, monitor_index, window_mode
    );
    debug!(
        "[Restore] Current window state: position={:?} mode={:?}",
        window.position, window.mode
    );

    window.mode = window_mode;
}

/// Apply position and/or size to window with logging.
fn apply_window_geometry(
    window: &mut Window,
    position: Option<IVec2>,
    size: UVec2,
    strategy: &str,
    ratio: Option<f64>,
) {
    if let Some(pos) = position {
        if let Some(r) = ratio {
            debug!(
                "[try_apply_restore] position={:?} size={}x{} ({strategy}, ratio={r})",
                pos, size.x, size.y
            );
        } else {
            debug!(
                "[try_apply_restore] position={:?} size={}x{} ({strategy})",
                pos, size.x, size.y
            );
        }
        window.position = WindowPosition::At(pos);
    } else if let Some(r) = ratio {
        debug!(
            "[try_apply_restore] size={}x{} only ({strategy}, ratio={r}, no position)",
            size.x, size.y
        );
    } else {
        debug!(
            "[try_apply_restore] size={}x{} only ({strategy}, no position)",
            size.x, size.y
        );
    }
    window.resolution.set_physical_resolution(size.x, size.y);
}

/// Try to apply a pending window restore.
fn try_apply_restore(
    target: &TargetPosition,
    window: &mut Window,
    platform: Platform,
) -> RestoreStatus {
    // Handle fullscreen modes - use saved monitor index from TargetPosition
    if target.mode.is_fullscreen() {
        debug!(
            "[try_apply_restore] fullscreen: mode={:?} target_monitor={} current_physical={}x{} current_mode={:?} current_pos={:?}",
            target.mode,
            target.target_monitor_index,
            window.physical_width(),
            window.physical_height(),
            window.mode,
            window.position,
        );
        apply_fullscreen_restore(target, window, platform);

        window.visible = true;
        return RestoreStatus::Complete;
    }

    debug!(
        "[Restore] target_pos={:?} target_scale={} strategy={:?}",
        target.position, target.target_scale, target.monitor_scale_strategy
    );

    match target.monitor_scale_strategy {
        MonitorScaleStrategy::ApplyUnchanged => {
            apply_window_geometry(
                window,
                target.position(),
                target.size(),
                "ApplyUnchanged",
                None,
            );
        },
        MonitorScaleStrategy::CompensateSizeOnly(WindowRestoreState::ApplySize) => {
            let size = target.size();
            debug!(
                "[try_apply_restore] size={}x{} ONLY (CompensateSizeOnly::ApplySize, position already set)",
                size.x, size.y
            );
            window.resolution.set_physical_resolution(size.x, size.y);
        },
        MonitorScaleStrategy::CompensateSizeOnly(
            WindowRestoreState::NeedInitialMove | WindowRestoreState::WaitingForScaleChange,
        ) => {
            debug!(
                "[Restore] CompensateSizeOnly: waiting for initial move or ScaleChanged message"
            );
            return RestoreStatus::Waiting;
        },
        MonitorScaleStrategy::LowerToHigher => {
            apply_window_geometry(
                window,
                target.compensated_position(),
                target.compensated_size(),
                "LowerToHigher",
                Some(target.ratio()),
            );
        },
        MonitorScaleStrategy::HigherToLower(WindowRestoreState::ApplySize) => {
            let size = target.size();
            debug!(
                "[try_apply_restore] size={}x{} ONLY (HigherToLower::ApplySize, position already set)",
                size.x, size.y
            );
            window.resolution.set_physical_resolution(size.x, size.y);
        },
        MonitorScaleStrategy::HigherToLower(
            WindowRestoreState::NeedInitialMove | WindowRestoreState::WaitingForScaleChange,
        ) => {
            debug!("[Restore] HigherToLower: waiting for initial move or ScaleChanged message");
            return RestoreStatus::Waiting;
        },
    }

    // Show window now that restore is complete
    window.visible = true;
    RestoreStatus::Complete
}

#[cfg(test)]
mod tests {
    use bevy::window::MonitorSelection;
    use bevy::window::VideoModeSelection;
    use bevy::window::WindowMode;
    use bevy::window::WindowPosition;

    use super::*;

    fn monitor_0() -> MonitorInfo {
        MonitorInfo {
            index:    0,
            scale:    2.0,
            position: IVec2::ZERO,
            size:     UVec2::new(3456, 2234),
        }
    }

    fn monitors_with(info: MonitorInfo) -> Monitors { Monitors { list: vec![info] } }

    fn window_at(pos: IVec2, width: u32, height: u32) -> Window {
        let mut window = Window {
            position: WindowPosition::At(pos),
            mode: WindowMode::Windowed,
            ..Default::default()
        };
        window.resolution.set_physical_resolution(width, height);
        window
    }

    #[test]
    fn effective_mode_fullscreen_when_window_fills_monitor() {
        let mon = monitor_0();
        let monitors = monitors_with(mon);
        let window = window_at(mon.position, mon.size.x, mon.size.y);

        let mode = compute_effective_mode(&window, &mon, &monitors);
        assert_eq!(
            mode,
            WindowMode::BorderlessFullscreen(MonitorSelection::Index(0))
        );
    }

    #[test]
    fn effective_mode_windowed_when_window_smaller_than_monitor() {
        let mon = monitor_0();
        let monitors = monitors_with(mon);
        let window = window_at(IVec2::new(100, 100), 1600, 1200);

        let mode = compute_effective_mode(&window, &mon, &monitors);
        assert_eq!(mode, WindowMode::Windowed);
    }

    #[test]
    fn effective_mode_windowed_when_not_left_aligned() {
        let mon = monitor_0();
        let monitors = monitors_with(mon);
        // Full width + reaches bottom, but offset from left edge
        let window = window_at(IVec2::new(1, 0), mon.size.x, mon.size.y);

        let mode = compute_effective_mode(&window, &mon, &monitors);
        assert_eq!(mode, WindowMode::Windowed);
    }

    #[test]
    fn effective_mode_trusts_exclusive_fullscreen() {
        let mon = monitor_0();
        let monitors = monitors_with(mon);
        let mut window = window_at(IVec2::ZERO, 800, 600);
        window.mode =
            WindowMode::Fullscreen(MonitorSelection::Index(0), VideoModeSelection::Current);

        let mode = compute_effective_mode(&window, &mon, &monitors);
        assert!(matches!(mode, WindowMode::Fullscreen(_, _)));
    }

    #[test]
    fn effective_mode_returns_mode_when_no_position() {
        let mon = monitor_0();
        let monitors = monitors_with(mon);
        let mut window = Window::default();
        window
            .resolution
            .set_physical_resolution(mon.size.x, mon.size.y);
        // position is Automatic (no position available, like Wayland)

        let mode = compute_effective_mode(&window, &mon, &monitors);
        assert_eq!(mode, WindowMode::Windowed);
    }

    #[test]
    fn effective_mode_returns_mode_when_no_monitors() {
        let mon = monitor_0();
        let empty = Monitors { list: vec![] };
        let window = window_at(IVec2::ZERO, mon.size.x, mon.size.y);

        let mode = compute_effective_mode(&window, &mon, &empty);
        assert_eq!(mode, WindowMode::Windowed);
    }
}
