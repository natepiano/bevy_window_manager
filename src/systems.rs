//! Systems for window restoration.

use std::ffi::OsStr;

use bevy::ecs::system::NonSendMarker;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy::window::WindowScaleFactorChanged;
use bevy::winit::WINIT_WINDOWS;

use super::state;
use super::types::RestoreWindowConfig;
use super::types::WindowState;
#[cfg(all(target_os = "macos", feature = "workaround-macos-drag-back-reset"))]
use crate::macos_drag_back_fix::DragBackSizeProtection;
use crate::monitors::Monitors;
#[cfg(all(target_os = "windows", feature = "workaround-winit-3124"))]
use crate::types::FullscreenRestoreState;
use crate::types::MonitorScaleStrategy;
use crate::types::SavedWindowMode;
use crate::types::TargetPosition;
use crate::types::WindowDecoration;
use crate::types::WindowRestoreState;
use crate::types::WinitInfo;
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

            let starting_monitor_index = monitors.closest_to(pos.x, pos.y).index;

            debug!(
                "[init_winit_info] decoration={}x{} pos=({}, {}) starting_monitor={}",
                decoration.width, decoration.height, pos.x, pos.y, starting_monitor_index
            );

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

    let Some((saved_x, saved_y)) = state.position else {
        debug!("[load_target_position] No saved bevy_window_manager position");
        return;
    };

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
    //
    // On macOS, we clamp to monitor bounds because macOS may resize/reposition windows
    // that extend beyond the screen.
    //
    // On Windows, users can legitimately position windows partially off-screen,
    // and the invisible border offset means saved positions may be slightly outside
    // monitor bounds. We skip clamping to preserve the exact saved position.
    let (x, y) = if cfg!(target_os = "windows") {
        // Windows: use saved position directly, no clamping
        (saved_x, saved_y)
    } else {
        // macOS: clamp to monitor bounds (using outer dimensions for accurate bounds)
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

        (x, y)
    };

    debug!(
        "[load_target_position] Starting monitor={} scale={}, Target monitor={} scale={}, strategy={:?}",
        starting_monitor_index, starting_scale, state.monitor_index, target_scale, strategy
    );

    // Store inner dimensions - decoration is only needed for clamping above
    commands.insert_resource(TargetPosition {
        x,
        y,
        width,
        height,
        target_scale,
        starting_scale,
        monitor_scale_strategy: strategy,
        mode: state.mode,
        #[cfg(all(target_os = "windows", feature = "workaround-winit-3124"))]
        fullscreen_restore_state: FullscreenRestoreState::WaitingForSurface,
    });
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
        debug!(
            "[move_to_target_monitor] Moving to target position ({}, {}) for fullscreen mode {:?}",
            target.x, target.y, target.mode
        );
        window.position = WindowPosition::At(IVec2::new(target.x, target.y));
        return;
    }

    // For HigherToLower, compensate position because winit divides by launch scale
    let (move_x, move_y) = if matches!(
        target.monitor_scale_strategy,
        MonitorScaleStrategy::HigherToLower(_)
    ) {
        let ratio = target.starting_scale / target.target_scale;
        let comp_x = (f64::from(target.x) * ratio) as i32;
        let comp_y = (f64::from(target.y) * ratio) as i32;
        debug!(
            "[move_to_target_monitor] HigherToLower: compensating position ({}, {}) -> ({}, {}) (ratio={})",
            target.x, target.y, comp_x, comp_y, ratio
        );
        (comp_x, comp_y)
    } else {
        (target.x, target.y)
    };

    // For HigherToLower, set the actual target size instead of 1x1 to avoid
    // macOS caching the tiny size and applying it later during monitor transitions.
    let (width, height) = if matches!(
        target.monitor_scale_strategy,
        MonitorScaleStrategy::HigherToLower(_)
    ) {
        (target.width, target.height)
    } else {
        (1, 1)
    };

    // Hide window during HigherToLower two-phase restore to avoid visual flash
    if matches!(
        target.monitor_scale_strategy,
        MonitorScaleStrategy::HigherToLower(_)
    ) {
        window.visible = false;
    }

    debug!(
        "[move_to_target_monitor] position=({}, {}) size={}x{} visible={}",
        move_x, move_y, width, height, window.visible
    );

    window.position = bevy::window::WindowPosition::At(IVec2::new(move_x, move_y));
    window.resolution.set_physical_resolution(width, height);
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
pub fn save_window_state(
    config: Res<RestoreWindowConfig>,
    monitors: Res<Monitors>,
    window: Single<&Window, (With<PrimaryWindow>, Changed<Window>)>,
    mut cached: Local<CachedWindowState>,
) {
    let Some(pos) = (match window.position {
        bevy::window::WindowPosition::At(p) => Some(p),
        _ => None,
    }) else {
        return;
    };

    let width = window.resolution.physical_width();
    let height = window.resolution.physical_height();
    let mode: SavedWindowMode = (&window.effective_mode(&monitors)).into();
    let monitor_info = window.monitor(&monitors);
    let monitor_index = monitor_info.index;
    let monitor_scale = monitor_info.scale;

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
    let position_changed = cached.position != Some(pos);
    let size_changed = cached.width != width || cached.height != height;
    let mode_changed = cached.mode.as_ref() != Some(&mode);

    if !position_changed && !size_changed && !mode_changed {
        cached.monitor_index = Some(monitor_index);
        return;
    }

    // Update cache
    cached.position = Some(pos);
    cached.width = width;
    cached.height = height;
    cached.mode = Some(mode.clone());
    cached.monitor_index = Some(monitor_index);

    debug!(
        "[save_window_state] pos=({}, {}) size={}x{} monitor={} scale={} mode={:?}",
        pos.x, pos.y, width, height, monitor_index, monitor_scale, mode
    );

    let app_name = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_stem().map(OsStr::to_owned))
        .and_then(|s| s.to_str().map(|s| (*s).to_string()))
        .unwrap_or_default();

    let state = WindowState {
        position: Some((pos.x, pos.y)),
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
    monitors: Res<Monitors>,
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
        try_apply_restore(&target, &monitors, &mut primary_window),
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

/// Try to apply a pending window restore.
fn try_apply_restore(
    target: &TargetPosition,
    monitors: &Monitors,
    primary_window: &mut Window,
) -> RestoreStatus {
    let target_mon = monitors.closest_to(target.x, target.y);

    // Handle fullscreen modes - just set the mode and we're done
    if target.mode.is_fullscreen() {
        let window_mode = target.mode.to_window_mode(target_mon.index);
        debug!(
            "[Restore] Applying fullscreen mode {:?} on monitor {} -> WindowMode::{:?}",
            target.mode, target_mon.index, window_mode
        );
        debug!(
            "[Restore] Current window state: position={:?} mode={:?}",
            primary_window.position, primary_window.mode
        );

        primary_window.mode = window_mode;
        return RestoreStatus::Complete;
    }

    // the primary window may be on a monitor that is different than the target
    // so the scale could be different - this difference is why we need this crate
    let current_scale = primary_window.monitor(monitors).scale;

    debug!(
        "[Restore] target_pos=({}, {}) current_scale={} target_scale={} strategy={:?}",
        target.x, target.y, current_scale, target.target_scale, target.monitor_scale_strategy
    );

    // Not yet on target monitor (for ApplyUnchanged/LowerToHigher strategies)
    // if !matches!(
    //     target.monitor_scale_strategy,
    //     MonitorScaleStrategy::HigherToLower(WindowRestoreState::ApplySize)
    // ) && (current_scale - target.target_scale).abs() >= 0.01
    // {
    //     return RestoreStatus::Waiting;
    // }

    match target.monitor_scale_strategy {
        MonitorScaleStrategy::ApplyUnchanged => {
            let size = target.size();
            debug!(
                "[try_apply_restore] position=({}, {}) size={}x{} (ApplyUnchanged)",
                target.x, target.y, size.x, size.y
            );
            primary_window.set_position_and_size(target.position(), size);
        },
        #[cfg(target_os = "windows")]
        MonitorScaleStrategy::CompensateSizeOnly => {
            // Windows: position is correct, but size needs compensation
            let position = target.position();
            let size = target.compensated_size();
            debug!(
                "[try_apply_restore] position=({}, {}) size={}x{} (CompensateSizeOnly, ratio={})",
                position.x,
                position.y,
                size.x,
                size.y,
                target.ratio()
            );
            primary_window.set_position_and_size(position, size);
        },
        MonitorScaleStrategy::LowerToHigher => {
            let position = target.compensated_position();
            let size = target.compensated_size();
            debug!(
                "[try_apply_restore] position=({}, {}) size={}x{} (LowerToHigher, ratio={})",
                position.x,
                position.y,
                size.x,
                size.y,
                target.ratio()
            );
            primary_window.set_position_and_size(position, size);
        },
        MonitorScaleStrategy::HigherToLower(WindowRestoreState::ApplySize) => {
            // HigherToLower after scale changed: set size and show window
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
        // in this case we haven't yet received the ScaleChanged message so we can't apply the size
        // yet - early return
        MonitorScaleStrategy::HigherToLower(WindowRestoreState::WaitingForScaleChange) => {
            debug!("[Restore] HigherToLower: waiting for ScaleChanged message");
            return RestoreStatus::Waiting;
        },
    }

    // if we're here then we're good to go
    RestoreStatus::Complete
}

/// Determine the monitor scale strategy based on platform and scale factors.
/// Windows: compensate size only when scales differ.
#[cfg(target_os = "windows")]
fn determine_scale_strategy(starting_scale: f64, target_scale: f64) -> MonitorScaleStrategy {
    if (starting_scale - target_scale).abs() < 0.01 {
        MonitorScaleStrategy::ApplyUnchanged
    } else {
        MonitorScaleStrategy::CompensateSizeOnly
    }
}

/// Determine the monitor scale strategy based on platform and scale factors.
/// macOS with workaround: compensate position and size based on scale relationship.
#[cfg(all(
    not(target_os = "windows"),
    feature = "workaround-macos-scale-compensation"
))]
fn determine_scale_strategy(starting_scale: f64, target_scale: f64) -> MonitorScaleStrategy {
    if (starting_scale - target_scale).abs() < 0.01 {
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
