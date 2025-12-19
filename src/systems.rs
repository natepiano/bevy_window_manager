//! Systems for window restoration and state management.
//!
//! # Wayland Monitor Detection
//!
//! On Wayland, `window.position` always returns `(0,0)` for security/privacy reasons.
//! Position-based monitor detection (which works on X11/macOS/Windows) therefore always
//! returns Monitor 0.
//!
//! ## Solution: The `CurrentMonitor` Component
//!
//! We use a `CurrentMonitor` component on the Window entity:
//!
//! - **Non-Wayland**: [`save_window_state`] detects via position and updates the component
//! - **Wayland**: [`update_wayland_monitor`] polls winit's `current_monitor()` each frame
//!
//! ## Why focus matters
//!
//! On Wayland, [`save_window_state`] runs on `Changed<Window>` which fires on focus changes,
//! cursor movement, etc. Since position-based detection always returns Monitor 0, we must
//! preserve the existing `CurrentMonitor` component value rather than overwriting it.
//!
//! We only trust `current_monitor()` when the window has focus because testing showed
//! incorrect values when unfocused (possibly our own bug, possibly winit behavior).

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
#[cfg(all(target_os = "macos", feature = "workaround-macos-drag-back-reset"))]
use crate::macos_drag_back_fix::DragBackSizeProtection;
use crate::monitors::CurrentMonitor;
use crate::monitors::Monitors;
#[cfg(all(target_os = "windows", feature = "workaround-winit-3124"))]
use crate::types::FullscreenRestoreState;
use crate::types::MonitorScaleStrategy;
use crate::types::SCALE_FACTOR_EPSILON;
use crate::types::SavedWindowMode;
use crate::types::TargetPosition;
use crate::types::WindowDecoration;
use crate::types::WindowRestoreState;
use crate::types::WinitInfo;
use crate::types::X11FrameCompensated;
use crate::window_ext::WindowExt;

/// Populate `WinitInfo` resource from winit (decoration and starting monitor).
pub fn init_winit_info(
    mut commands: Commands,
    window_entity: Single<Entity, With<PrimaryWindow>>,
    monitors: Res<Monitors>,
    _non_send: NonSendMarker,
) {
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
                "[init_winit_info] outer_position={:?} is_wayland={}",
                pos,
                is_wayland()
            );

            // Log what current_monitor() returns for comparison
            let current_monitor_index = winit_window.current_monitor().and_then(|cm| {
                let cm_pos = cm.position();
                let idx = monitors.at(cm_pos.x, cm_pos.y).map(|m| m.index);
                debug!(
                    "[init_winit_info] current_monitor() position=({}, {}) -> index={:?}",
                    cm_pos.x, cm_pos.y, idx
                );
                idx
            });
            debug!(
                "[init_winit_info] current_monitor_index={:?}",
                current_monitor_index
            );

            let starting_monitor = monitors.closest_to(pos.x, pos.y);
            let starting_monitor_index = starting_monitor.index;

            debug!(
                "[init_winit_info] decoration={}x{} pos=({}, {}) starting_monitor={}",
                decoration.width, decoration.height, pos.x, pos.y, starting_monitor_index
            );

            // Insert initial CurrentMonitor component on window entity
            commands
                .entity(*window_entity)
                .insert(CurrentMonitor(*starting_monitor));

            commands.insert_resource(WinitInfo {
                starting_monitor_index,
                window_decoration: decoration,
            });
        }
    });
}

/// Load saved window state and create `TargetPosition` resource.
///
/// Runs after `init_winit_info` so we have access to starting monitor info.
pub fn load_target_position(
    mut commands: Commands,
    monitors: Res<Monitors>,
    winit_info: Res<WinitInfo>,
    config: Res<RestoreWindowConfig>,
) {
    let Some(state) = state::load_state(&config.path) else {
        debug!("[load_target_position] No saved bevy_window_manager state");
        return;
    };

    debug!(
        "[load_target_position] Loaded state: position={:?} size={}x{} monitor_index={} mode={:?}",
        state.position, state.width, state.height, state.monitor_index, state.mode
    );

    // Position may be None on Wayland where clients can't access window position.
    // We can still restore fullscreen mode, size, and monitor.
    let saved_pos = state.position;

    // Get starting monitor from WinitInfo
    let starting_monitor_index = winit_info.starting_monitor_index;
    let starting_info = monitors.by_index(starting_monitor_index);
    let starting_scale = starting_info.map_or(1.0, |m| m.scale);

    let Some(target_info) = monitors.by_index(state.monitor_index) else {
        debug!(
            "[load_target_position] Target monitor index {} not found",
            state.monitor_index
        );
        return;
    };

    let target_scale = target_info.scale;

    // File stores inner dimensions (content area)
    let width = state.width;
    let height = state.height;

    // Calculate outer dimensions for clamping (inner + decoration)
    let decoration = winit_info.decoration();
    let outer_width = width + decoration.x;
    let outer_height = height + decoration.y;

    // Determine monitor scale strategy based on scale relationship and platform.
    //
    // On Windows, winit handles position coordinates correctly, but Bevy's
    // set_physical_resolution still applies scale conversion. We use CompensateSizeOnly
    // when scales differ, or ApplyUnchanged when they match.
    //
    // On macOS, winit's coordinate handling is broken for multi-monitor setups with
    // different scale factors. Bevy processes size before position, so winit's
    // request_inner_size uses the launch monitor's scale factor instead of the target's.
    // We must compensate both position and size based on the scale factor relationship.
    //
    // The macOS compensation can be disabled via feature flag to test if upstream
    // fixes (e.g., Bevy processing position before size) resolve the issue.
    let strategy = determine_scale_strategy(starting_scale, target_scale);

    // Calculate final position, with optional clamping.
    // Position may be None on Wayland where clients can't access window position.
    //
    // On macOS, we clamp to monitor bounds because macOS may resize/reposition windows
    // that extend beyond the screen.
    //
    // On Windows, users can legitimately position windows partially off-screen,
    // and the invisible border offset means saved positions may be slightly outside
    // monitor bounds. We skip clamping to preserve the exact saved position.
    let position = saved_pos.map(|(saved_x, saved_y)| {
        if cfg!(target_os = "windows") {
            // Windows: use saved position directly, no clamping
            IVec2::new(saved_x, saved_y)
        } else {
            // macOS/Linux: clamp to monitor bounds (using outer dimensions for accurate bounds)
            let mon_right = target_info.position.x + target_info.size.x as i32;
            let mon_bottom = target_info.position.y + target_info.size.y as i32;

            let mut x = saved_x;
            let mut y = saved_y;

            if x + outer_width as i32 > mon_right {
                x = mon_right - outer_width as i32;
            }
            if y + outer_height as i32 > mon_bottom {
                y = mon_bottom - outer_height as i32;
            }
            x = x.max(target_info.position.x);
            y = y.max(target_info.position.y);

            if x != saved_x || y != saved_y {
                debug!(
                    "[load_target_position] Clamped position: ({}, {}) -> ({}, {}) for outer size {}x{}",
                    saved_x, saved_y, x, y, outer_width, outer_height
                );
            }

            IVec2::new(x, y)
        }
    });

    debug!(
        "[load_target_position] Starting monitor={} scale={}, Target monitor={} scale={}, strategy={:?}, position={:?}",
        starting_monitor_index,
        starting_scale,
        state.monitor_index,
        target_scale,
        strategy,
        position
    );

    // Store inner dimensions - decoration is only needed for clamping above
    commands.insert_resource(TargetPosition {
        position,
        width,
        height,
        target_scale,
        starting_scale,
        monitor_scale_strategy: strategy,
        mode: state.mode,
        target_monitor_index: state.monitor_index,
        #[cfg(all(target_os = "windows", feature = "workaround-winit-3124"))]
        fullscreen_restore_state: FullscreenRestoreState::WaitingForSurface,
    });

    // Insert X11FrameCompensated token for platforms that don't need compensation.
    // On Linux + W6 + X11, the compensation system inserts this token after adjusting position.
    #[cfg(not(all(target_os = "linux", feature = "workaround-winit-4445")))]
    commands.insert_resource(X11FrameCompensated);

    #[cfg(all(target_os = "linux", feature = "workaround-winit-4445"))]
    if is_wayland() {
        commands.insert_resource(X11FrameCompensated);
    }
}

/// Move window to target monitor at 1x1 size (`PreStartup`).
///
/// Uses pre-computed `TargetPosition` to move the window.
///
/// On macOS with `HigherToLower` strategy, the position is compensated because winit
/// divides coordinates by the launch monitor's scale factor.
///
/// On Windows, this compensation is never needed (strategy is always `ApplyUnchanged`).
///
/// For fullscreen modes, we still move to the target monitor so the fullscreen mode
/// is applied on the correct monitor when `try_apply_restore` runs.
pub fn move_to_target_monitor(
    mut window: Single<&mut Window, With<PrimaryWindow>>,
    target: Res<TargetPosition>,
) {
    // For fullscreen modes, just move to target monitor position (no 1x1 size)
    // The fullscreen mode will be applied later in try_apply_restore
    if target.mode.is_fullscreen() {
        if let Some(pos) = target.position {
            debug!(
                "[move_to_target_monitor] Moving to target position {:?} for fullscreen mode {:?}",
                pos, target.mode
            );
            window.position = WindowPosition::At(pos);
        } else {
            debug!(
                "[move_to_target_monitor] No position available (Wayland), fullscreen mode {:?}",
                target.mode
            );
        }
        return;
    }

    // Position may be None on Wayland - skip position setting if unavailable
    let Some(pos) = target.position else {
        debug!(
            "[move_to_target_monitor] No position available (Wayland), setting size only: {}x{}",
            target.width, target.height
        );
        debug!(
            "[move_to_target_monitor] BEFORE set_physical_resolution: physical={}x{} logical={}x{} scale={}",
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
            "[move_to_target_monitor] AFTER set_physical_resolution: physical={}x{} logical={}x{} scale={}",
            window.resolution.physical_width(),
            window.resolution.physical_height(),
            window.resolution.width(),
            window.resolution.height(),
            window.resolution.scale_factor()
        );
        return;
    };

    /// Visibility state for window during move operation.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum InitialVisibility {
        /// Window remains visible during move.
        Visible,
        /// Window is hidden to avoid visual flash (HigherToLower two-phase restore).
        Hidden,
    }

    /// Computed parameters for the initial window move to target monitor.
    #[derive(Debug)]
    struct MoveParams {
        position:   IVec2,
        width:      u32,
        height:     u32,
        visibility: InitialVisibility,
    }

    // Compute move parameters based on scale strategy
    let params = match target.monitor_scale_strategy {
        MonitorScaleStrategy::HigherToLower(_) => {
            // Compensate position because winit divides by launch scale
            let ratio = target.starting_scale / target.target_scale;
            let comp_x = (f64::from(pos.x) * ratio) as i32;
            let comp_y = (f64::from(pos.y) * ratio) as i32;
            debug!(
                "[move_to_target_monitor] HigherToLower: compensating position {:?} -> ({}, {}) (ratio={})",
                pos, comp_x, comp_y, ratio
            );
            MoveParams {
                position:   IVec2::new(comp_x, comp_y),
                // Use actual target size to avoid macOS caching tiny size
                width:      target.width,
                height:     target.height,
                // Hide to avoid visual flash during two-phase restore
                visibility: InitialVisibility::Hidden,
            }
        },
        _ => MoveParams {
            position:   pos,
            width:      1,
            height:     1,
            visibility: InitialVisibility::Visible,
        },
    };

    if params.visibility == InitialVisibility::Hidden {
        window.visible = false;
    }

    debug!(
        "[move_to_target_monitor] position={:?} size={}x{} visible={}",
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
    position:      Option<IVec2>,
    width:         u32,
    height:        u32,
    mode:          Option<SavedWindowMode>,
    monitor_index: Option<usize>,
}

/// Save window state when position, size, or mode changes. Runs only when not restoring.
#[allow(
    clippy::type_complexity,
    clippy::too_many_lines,
    clippy::option_if_let_else
)]
pub fn save_window_state(
    mut commands: Commands,
    config: Res<RestoreWindowConfig>,
    monitors: Res<Monitors>,
    window: Single<
        (Entity, &Window, Option<&CurrentMonitor>),
        (With<PrimaryWindow>, Changed<Window>),
    >,
    mut cached: Local<CachedWindowState>,
    _non_send: NonSendMarker,
) {
    let (window_entity, window, existing_monitor) = *window;

    // Get window position for saving state.
    //
    // On X11, bevy's cached window.position doesn't update when the window manager
    // moves the window via keyboard shortcuts (winit #4443). When the workaround is
    // enabled, we query winit's outer_position() directly which always returns the
    // correct position.
    //
    // When the workaround is disabled, we use bevy's cached position which may be
    // stale on X11 after keyboard snap operations. This allows testing whether winit
    // has fixed the bug.
    // Workaround W5 (winit #4443): Query outer_position() directly because keyboard
    // snap doesn't emit Moved events.
    // Note: W6 compensation moved to restore-side (load_target_position) so that
    // saved position matches Window.position for user clarity.
    #[cfg(all(target_os = "linux", feature = "workaround-winit-4443"))]
    let pos = WINIT_WINDOWS.with(|ww| {
        let ww = ww.borrow();
        let winit_win = ww.get_window(window_entity)?;

        let outer_pos = winit_win.outer_position().ok()?;
        Some(IVec2::new(outer_pos.x, outer_pos.y))
    });

    #[cfg(not(all(target_os = "linux", feature = "workaround-winit-4443")))]
    let pos = match window.position {
        bevy::window::WindowPosition::At(p) => Some(p),
        _ => None,
    };

    let width = window.resolution.physical_width();
    let height = window.resolution.physical_height();
    let mode: SavedWindowMode = (&window.effective_mode(&monitors)).into();

    // Get monitor info. See module docs for Wayland monitor detection details.
    let (monitor_index, monitor_scale) = if is_wayland() {
        // Wayland: read existing component (managed by update_wayland_monitor)
        existing_monitor.map_or_else(
            || {
                let p = monitors.first();
                (p.index, p.scale)
            },
            |m| (m.index, m.scale),
        )
    } else {
        // Non-Wayland: detect via position and update component
        let info = window.monitor(&monitors);
        commands.entity(window_entity).insert(CurrentMonitor(*info));
        (info.index, info.scale)
    };

    // Log monitor transitions with detailed info
    let monitor_changed = cached.monitor_index != Some(monitor_index);
    if monitor_changed {
        let prev_scale = cached
            .monitor_index
            .and_then(|i| monitors.by_index(i))
            .map(|m| m.scale);
        debug!(
            "[save_window_state] MONITOR CHANGE: {:?} (scale={:?}) -> {} (scale={})",
            cached.monitor_index, prev_scale, monitor_index, monitor_scale
        );
        debug!(
            "[save_window_state]   physical: {}x{}, logical: {}x{}, scale_factor: {}",
            width,
            height,
            window.resolution.width(),
            window.resolution.height(),
            window.resolution.scale_factor()
        );
        debug!(
            "[save_window_state]   cached size was: {}x{}",
            cached.width, cached.height
        );
    }

    // Only save if position, size, or mode actually changed
    let position_changed = cached.position != pos;
    let size_changed = cached.width != width || cached.height != height;
    let mode_changed = cached.mode.as_ref() != Some(&mode);

    if !position_changed && !size_changed && !mode_changed {
        cached.monitor_index = Some(monitor_index);
        return;
    }

    // Update cache
    cached.position = pos;
    cached.width = width;
    cached.height = height;
    cached.mode = Some(mode.clone());
    cached.monitor_index = Some(monitor_index);

    debug!(
        "[save_window_state] pos={:?} size={}x{} monitor={} scale={} mode={:?}",
        pos, width, height, monitor_index, monitor_scale, mode
    );

    let app_name = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_stem().and_then(|s| s.to_str()).map(String::from))
        .unwrap_or_default();

    let state = WindowState {
        position: pos.map(|p| (p.x, p.y)),
        width,
        height,
        monitor_index,
        mode,
        app_name,
    };
    state::save_state(&config.path, &state);
}

/// Apply pending window restore. Runs only when `TargetPosition` exists.
pub fn restore_primary_window(
    mut commands: Commands,
    mut scale_changed_messages: MessageReader<WindowScaleFactorChanged>,
    mut target: ResMut<TargetPosition>,
    mut primary_window: Single<&mut Window, With<PrimaryWindow>>,
) {
    let scale_changed = scale_changed_messages.read().last().is_some();

    // Handle HigherToLower state transition on scale change
    if target.monitor_scale_strategy
        == MonitorScaleStrategy::HigherToLower(WindowRestoreState::WaitingForScaleChange)
        && scale_changed
    {
        debug!("[Restore] ScaleChanged received, transitioning to WindowRestoreState::ApplySize");
        target.monitor_scale_strategy =
            MonitorScaleStrategy::HigherToLower(WindowRestoreState::ApplySize);
    }

    // Windows: transition fullscreen state after first frame (DX12/DXGI workaround)
    #[cfg(all(target_os = "windows", feature = "workaround-winit-3124"))]
    if target.mode.is_fullscreen()
        && target.fullscreen_restore_state == FullscreenRestoreState::WaitingForSurface
    {
        debug!("[Restore] First frame passed, transitioning to ApplyFullscreen");
        target.fullscreen_restore_state = FullscreenRestoreState::ApplyFullscreen;
        return; // Wait one more frame for the state change to take effect
    }

    // Check if this is a HigherToLower restore about to complete (for W4 protection)
    #[cfg(all(target_os = "macos", feature = "workaround-macos-drag-back-reset"))]
    let was_higher_to_lower = matches!(
        target.monitor_scale_strategy,
        MonitorScaleStrategy::HigherToLower(WindowRestoreState::ApplySize)
    );

    if matches!(
        try_apply_restore(&target, &mut primary_window),
        RestoreStatus::Complete
    ) {
        // Insert W4 drag-back protection for HigherToLower restores
        #[cfg(all(target_os = "macos", feature = "workaround-macos-drag-back-reset"))]
        if was_higher_to_lower {
            debug!(
                "[Restore] Inserting DragBackSizeProtection: size={}x{} launch_scale={} restored_scale={}",
                target.width, target.height, target.starting_scale, target.target_scale
            );
            // Phase 1 cached size is the physical size we set at launch scale before moving.
            // This is what AppKit will cache and restore when dragging back (W4 behavior).
            let phase1_cached_size = UVec2::new(target.width, target.height);
            commands.insert_resource(DragBackSizeProtection {
                expected_physical_size: UVec2::new(target.width, target.height),
                launch_scale: target.starting_scale,
                restored_scale: target.target_scale,
                phase1_cached_size,
                state: crate::macos_drag_back_fix::CorrectionState::WaitingForDragBack,
            });
        }

        commands.remove_resource::<TargetPosition>();
    }
}

/// Result of attempting to apply a window restore.
enum RestoreStatus {
    /// Restore completed successfully.
    Complete,
    /// Waiting for conditions to be met (scale change, window position, etc.).
    Waiting,
}

/// Run condition: returns true if running on Wayland.
pub fn is_wayland() -> bool {
    cfg!(target_os = "linux")
        && std::env::var("WAYLAND_DISPLAY")
            .map(|v| !v.is_empty())
            .unwrap_or(false)
}

/// Polls winit's `current_monitor()` on Wayland to update `CurrentMonitor`.
/// Only runs on Wayland; only updates when window has focus.
/// See module docs for Wayland monitor detection details.
#[cfg(target_os = "linux")]
pub fn update_wayland_monitor(
    mut commands: Commands,
    window: Single<(Entity, &Window), With<PrimaryWindow>>,
    monitors: Res<Monitors>,
    mut cached_index: Local<Option<usize>>,
    _non_send: NonSendMarker,
) {
    let (window_entity, window) = *window;

    // Only trust current_monitor() when window has focus - winit returns
    // the focused monitor, not the window's monitor, when unfocused
    if !window.focused {
        return;
    }

    let detected_index: Option<usize> = WINIT_WINDOWS.with(|ww| {
        let ww = ww.borrow();
        ww.get_window(window_entity).and_then(|winit_window| {
            winit_window.current_monitor().and_then(|current_monitor| {
                let pos = current_monitor.position();
                monitors.at(pos.x, pos.y).map(|mon| mon.index)
            })
        })
    });

    // Only update if monitor changed
    if *cached_index != detected_index {
        if let Some(idx) = detected_index
            && let Some(info) = monitors.by_index(idx)
        {
            debug!(
                "[update_wayland_monitor] Monitor changed: {:?} -> {}",
                *cached_index, idx
            );
            commands.entity(window_entity).insert(CurrentMonitor(*info));
        }
        *cached_index = detected_index;
    }
}

/// Apply fullscreen mode, handling Wayland limitations.
fn apply_fullscreen_restore(
    target: &TargetPosition,
    primary_window: &mut Window,
    monitor_index: usize,
) {
    // On Wayland, exclusive fullscreen is ignored by winit, so we restore it as
    // borderless fullscreen instead.
    let window_mode = if is_wayland() && matches!(target.mode, SavedWindowMode::Fullscreen { .. }) {
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
        primary_window.position, primary_window.mode
    );

    primary_window.mode = window_mode;
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
        window.set_position_and_size(pos, size);
    } else {
        if let Some(r) = ratio {
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
}

/// Try to apply a pending window restore.
fn try_apply_restore(target: &TargetPosition, primary_window: &mut Window) -> RestoreStatus {
    // Handle fullscreen modes - use saved monitor index from TargetPosition
    if target.mode.is_fullscreen() {
        apply_fullscreen_restore(target, primary_window, target.target_monitor_index);
        return RestoreStatus::Complete;
    }

    debug!(
        "[Restore] target_pos={:?} target_scale={} strategy={:?}",
        target.position, target.target_scale, target.monitor_scale_strategy
    );

    match target.monitor_scale_strategy {
        MonitorScaleStrategy::ApplyUnchanged => {
            apply_window_geometry(
                primary_window,
                target.position(),
                target.size(),
                "ApplyUnchanged",
                None,
            );
        },
        #[cfg(target_os = "windows")]
        MonitorScaleStrategy::CompensateSizeOnly => {
            apply_window_geometry(
                primary_window,
                target.position(),
                target.compensated_size(),
                "CompensateSizeOnly",
                Some(target.ratio()),
            );
        },
        #[cfg(all(
            not(target_os = "windows"),
            feature = "workaround-macos-scale-compensation"
        ))]
        MonitorScaleStrategy::LowerToHigher => {
            apply_window_geometry(
                primary_window,
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
            primary_window
                .resolution
                .set_physical_resolution(size.x, size.y);
            primary_window.visible = true;
        },
        MonitorScaleStrategy::HigherToLower(WindowRestoreState::WaitingForScaleChange) => {
            debug!("[Restore] HigherToLower: waiting for ScaleChanged message");
            return RestoreStatus::Waiting;
        },
    }

    RestoreStatus::Complete
}

/// Determine the monitor scale strategy based on platform and scale factors.
/// Windows: compensate size only when scales differ.
#[cfg(target_os = "windows")]
fn determine_scale_strategy(starting_scale: f64, target_scale: f64) -> MonitorScaleStrategy {
    if (starting_scale - target_scale).abs() < SCALE_FACTOR_EPSILON {
        MonitorScaleStrategy::ApplyUnchanged
    } else {
        MonitorScaleStrategy::CompensateSizeOnly
    }
}

/// Determine the monitor scale strategy based on platform and scale factors.
/// macOS with workaround: compensate position and size based on scale relationship.
/// Wayland: always use `ApplyUnchanged` (can't detect starting monitor, can't set position).
#[cfg(all(
    not(target_os = "windows"),
    feature = "workaround-macos-scale-compensation"
))]
fn determine_scale_strategy(starting_scale: f64, target_scale: f64) -> MonitorScaleStrategy {
    // On Wayland, we can't reliably detect the starting monitor (outer_position returns 0,0
    // and current_monitor/primary_monitor return None at init). Since we also can't set
    // position on Wayland, skip scale compensation entirely.
    if is_wayland() {
        return MonitorScaleStrategy::ApplyUnchanged;
    }

    if (starting_scale - target_scale).abs() < SCALE_FACTOR_EPSILON {
        MonitorScaleStrategy::ApplyUnchanged
    } else if starting_scale < target_scale {
        // Low DPI -> high DPI
        MonitorScaleStrategy::LowerToHigher
    } else {
        // High DPI -> low DPI
        MonitorScaleStrategy::HigherToLower(WindowRestoreState::WaitingForScaleChange)
    }
}

/// Determine the monitor scale strategy based on platform and scale factors.
/// macOS without workaround: always use `ApplyUnchanged`.
#[cfg(all(
    not(target_os = "windows"),
    not(feature = "workaround-macos-scale-compensation")
))]
fn determine_scale_strategy(_starting_scale: f64, _target_scale: f64) -> MonitorScaleStrategy {
    // Without workaround, assume upstream fixes handle scale factor correctly
    MonitorScaleStrategy::ApplyUnchanged
}
