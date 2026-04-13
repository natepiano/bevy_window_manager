//! Window restoration logic.
//!
//! Handles loading saved state, applying initial moves, cross-DPI strategies,
//! and fullscreen restoration.

use bevy::ecs::system::NonSendMarker;
use bevy::prelude::*;
use bevy::window::MonitorSelection;
use bevy::window::PrimaryWindow;
use bevy::window::WindowMode;
use bevy::window::WindowScaleFactorChanged;
use bevy::winit::WINIT_WINDOWS;
use bevy_kana::ToI32;
use bevy_kana::ToU32;

use crate::Platform;
use crate::WindowKey;
use crate::config::RestoreWindowConfig;
use crate::constants::DEFAULT_SCALE_FACTOR;
use crate::constants::SCALE_FACTOR_EPSILON;
use crate::monitors::Monitors;
use crate::persistence;
use crate::persistence::SavedWindowMode;
use crate::restore_plan;
use crate::restore_target::FullscreenRestoreState;
use crate::restore_target::MonitorScaleStrategy;
use crate::restore_target::SettleState;
use crate::restore_target::TargetPosition;
use crate::restore_target::WindowDecoration;
use crate::restore_target::WindowRestoreState;
use crate::restore_target::WinitInfo;
use crate::restore_target::X11FrameCompensated;

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

    WINIT_WINDOWS.with(|winit_windows| {
        let winit_windows = winit_windows.borrow();
        if let Some(winit_window) = winit_windows.get_window(*window_entity) {
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
                    let monitor_position = cm.position();
                    let info = monitors.at(monitor_position.x, monitor_position.y);
                    debug!(
                        "[init_winit_info] current_monitor() position=({}, {}) -> index={:?}",
                        monitor_position.x, monitor_position.y, info.map(|m| m.index)
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
                "[init_winit_info] decoration={}x{} pos=({}, {}) starting_monitor={starting_monitor_index}",
                decoration.width, decoration.height, pos.x, pos.y,
            );

            // Insert initial CurrentMonitor component on window entity
            commands
                .entity(*window_entity)
                .insert(crate::monitors::CurrentMonitor {
                    monitor:        starting_monitor,
                    effective_mode: WindowMode::Windowed,
                });

            commands.insert_resource(WinitInfo {
                starting_monitor_index,
                decoration,
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
    if let Some(all_states) = persistence::load_all_states(&config.path) {
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
    let starting_scale = starting_info.map_or(DEFAULT_SCALE_FACTOR, |m| m.scale);

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
        "[load_target_position] Starting monitor={starting_monitor_index} scale={starting_scale}, Target monitor={} scale={}, strategy={:?}, position={:?}",
        target.target_monitor_index, target.target_scale, target.scale_strategy, target.position
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
    // screen so frame extents are irrelevant, and delaying `restore_windows` by extra frames
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
fn apply_initial_move(target: &TargetPosition, window: &mut Window) {
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
    let params = match target.scale_strategy {
        MonitorScaleStrategy::HigherToLower(_) => {
            // Compensate position because winit divides by launch scale
            let ratio = target.starting_scale / target.target_scale;
            let compensated_x = (f64::from(pos.x) * ratio).to_i32();
            let compensated_y = (f64::from(pos.y) * ratio).to_i32();
            debug!(
                "[apply_initial_move] HigherToLower: compensating position {pos:?} -> ({compensated_x}, {compensated_y}) (ratio={ratio})",
            );
            MoveParams {
                position: IVec2::new(compensated_x, compensated_y),
                // Use actual target size to avoid macOS caching tiny size
                width:    target.width,
                height:   target.height,
            }
        },
        MonitorScaleStrategy::CompensateSizeOnly(_) => {
            // Position applied directly, size compensated to survive DPI transition.
            // Phase 2 will re-apply the exact target size after `ScaleFactorChanged`.
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

/// Apply pending window restore. Runs only when entities with `TargetPosition` exist.
/// Processes all windows with both `TargetPosition` and `X11FrameCompensated` components.
///
/// Requires `NonSendMarker` because we access `WINIT_WINDOWS` thread-local to gate
/// restore on the winit window existing. Without this gate, managed windows spawned
/// at runtime would have their physical size set by `set_physical_resolution()` and
/// then doubled by `create_windows` → `set_scale_factor_and_apply_to_physical_size()`
/// which runs between frames.
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

        let winit_window_exists =
            WINIT_WINDOWS.with(|winit_windows| winit_windows.borrow().get_window(entity).is_some());
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
                let old_strategy = target.scale_strategy;
                target.starting_scale = actual_scale;
                target.scale_strategy = platform.scale_strategy(actual_scale, target.target_scale);
                debug!(
                    "[restore_windows] Corrected starting_scale for entity {entity:?}: \
                     strategy: {old_strategy:?} -> {:?} (actual_scale={actual_scale:.2})",
                    target.scale_strategy
                );
            }
        }

        // Two-phase restore for cross-DPI strategies (`HigherToLower`, `CompensateSizeOnly`).
        // Phase 1: `apply_initial_move` sets compensated position/size to trigger DPI change.
        // Phase 2: after `ScaleFactorChanged`, re-apply exact target size.
        if matches!(
            target.scale_strategy,
            MonitorScaleStrategy::HigherToLower(WindowRestoreState::NeedInitialMove)
                | MonitorScaleStrategy::CompensateSizeOnly(WindowRestoreState::NeedInitialMove)
        ) && try_cross_dpi_initial_move(&mut target, &mut window)
        {
            continue;
        }

        // Handle state transition on scale change for both strategies.
        // `CompensateSizeOnly`: also advance if no scale change arrives (e.g., hidden window
        // didn't trigger `WM_DPICHANGED`, or the app launched on the target monitor).
        match target.scale_strategy {
            MonitorScaleStrategy::HigherToLower(WindowRestoreState::WaitingForScaleChange)
                if scale_changed =>
            {
                debug!(
                    "[Restore] ScaleChanged received, transitioning to WindowRestoreState::ApplySize"
                );
                target.scale_strategy =
                    MonitorScaleStrategy::HigherToLower(WindowRestoreState::ApplySize);
            },
            MonitorScaleStrategy::CompensateSizeOnly(WindowRestoreState::WaitingForScaleChange) => {
                debug!(
                    "[Restore] CompensateSizeOnly: transitioning to ApplySize (scale_changed={scale_changed})"
                );
                target.scale_strategy =
                    MonitorScaleStrategy::CompensateSizeOnly(WindowRestoreState::ApplySize);
            },
            _ => {},
        }

        // Fullscreen phase state machine.
        // Phases: MoveToMonitor → WaitForMove → ApplyMode (Linux X11)
        //         WaitForSurface → ApplyMode (Windows DX12)
        //         ApplyMode (Wayland, macOS — direct apply)
        if let Some(fs_state) = target.fullscreen_state {
            match fs_state {
                FullscreenRestoreState::MoveToMonitor => {
                    // Move window to target monitor so compositor knows where it belongs
                    if let Some(pos) = target.position {
                        debug!("[restore_windows] Fullscreen MoveToMonitor: position={pos:?}");
                        window.position = WindowPosition::At(pos);
                    }
                    target.fullscreen_state = Some(FullscreenRestoreState::WaitForMove);
                    continue;
                },
                FullscreenRestoreState::WaitForMove => {
                    // Wait one frame for compositor to process the position change
                    debug!("[restore_windows] Fullscreen WaitForMove: waiting for compositor");
                    target.fullscreen_state = Some(FullscreenRestoreState::ApplyMode);
                    continue;
                },
                FullscreenRestoreState::WaitForSurface => {
                    // Wait one frame for GPU surface creation (Windows DX12, winit #3124)
                    debug!("[restore_windows] Fullscreen WaitForSurface: waiting for GPU surface");
                    target.fullscreen_state = Some(FullscreenRestoreState::ApplyMode);
                    continue;
                },
                FullscreenRestoreState::ApplyMode => {
                    // Fall through to `try_apply_restore` which applies the fullscreen mode
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

/// Result of attempting to apply a window restore.
enum RestoreStatus {
    /// Restore completed successfully.
    Complete,
    /// Waiting for conditions to be met (scale change, window position, etc.).
    Waiting,
}

/// Handle the initial move for cross-DPI strategies (`HigherToLower`, `CompensateSizeOnly`).
///
/// When position is available, starts the two-phase dance: move to trigger DPI change,
/// then wait for `ScaleFactorChanged` to apply final size.
///
/// When position is `None` (e.g., macOS first launch where `Window.position` stays
/// `Automatic`), the window can't move to the target monitor, so the two-phase dance
/// would wait for a `ScaleFactorChanged` that never arrives. Instead, recompute the
/// physical size for the starting monitor's scale and apply directly.
///
/// Returns `true` if the caller should `continue` (skip to next entity).
fn try_cross_dpi_initial_move(target: &mut TargetPosition, window: &mut Window) -> bool {
    if target.position.is_none() {
        let width = (f64::from(target.logical_width) * target.starting_scale).to_u32();
        let height = (f64::from(target.logical_height) * target.starting_scale).to_u32();
        debug!(
            "[restore_windows] No position for cross-DPI restore, applying logical size \
             {}x{} at starting_scale={} (physical {}x{}) instead of two-phase dance",
            target.logical_width, target.logical_height, target.starting_scale, width, height
        );
        window.resolution.set_physical_resolution(width, height);
        window.visible = true;
        target.settle_state = Some(SettleState::new());
        return true;
    }
    apply_initial_move(target, window);
    target.scale_strategy = match target.scale_strategy {
        MonitorScaleStrategy::HigherToLower(_) => {
            MonitorScaleStrategy::HigherToLower(WindowRestoreState::WaitingForScaleChange)
        },
        _ => MonitorScaleStrategy::CompensateSizeOnly(WindowRestoreState::WaitingForScaleChange),
    };
    true
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
        target.position, target.target_scale, target.scale_strategy
    );

    match target.scale_strategy {
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
