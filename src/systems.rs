//! Systems for window restoration.

use bevy::ecs::system::NonSendMarker;
use bevy::prelude::*;
use bevy::window::Monitor;
use bevy::window::WindowMoved;
use bevy::window::WindowResized;
use bevy::window::WindowScaleFactorChanged;
use bevy::winit::WINIT_WINDOWS;

use super::state;
use super::types::MonitorInfo;
use super::types::RestoreWindowConfig;
use super::types::WindowState;
use crate::types::RestorePhase;
use crate::types::TargetPosition;
use crate::types::TwoPhaseState;
use crate::types::WindowDecoration;
use crate::types::WinitInfo;

/// Get sort key for monitor (primary at 0,0 first, then by position).
const fn monitor_sort_key(mon: &Monitor) -> (bool, i32, i32) {
    let pos = mon.physical_position;
    let is_primary = pos.x == 0 && pos.y == 0;
    (!is_primary, pos.x, pos.y)
}

/// Helper to get monitor info at a position from Bevy `Monitor` components.
#[expect(
    clippy::cast_possible_wrap,
    reason = "monitor dimensions are always within i32 range"
)]
fn monitor_at(monitors: &[(Entity, &Monitor)], x: i32, y: i32) -> Option<MonitorInfo> {
    let mut sorted: Vec<_> = monitors.iter().collect();
    sorted.sort_by_key(|(_, mon)| monitor_sort_key(mon));

    sorted.iter().enumerate().find_map(|(idx, (_, mon))| {
        let pos = mon.physical_position;
        let size = mon.physical_size();
        if x >= pos.x && x < pos.x + size.x as i32 && y >= pos.y && y < pos.y + size.y as i32 {
            Some(MonitorInfo {
                index: idx,
                scale: mon.scale_factor,
                position: pos,
                size,
            })
        } else {
            None
        }
    })
}

/// Helper to get monitor info by index from Bevy `Monitor` components.
fn monitor_by_index(monitors: &[(Entity, &Monitor)], index: usize) -> Option<MonitorInfo> {
    let mut sorted: Vec<_> = monitors.iter().collect();
    sorted.sort_by_key(|(_, mon)| monitor_sort_key(mon));

    sorted.get(index).map(|(_, mon)| MonitorInfo {
        index,
        scale: mon.scale_factor,
        position: mon.physical_position,
        size: mon.physical_size(),
    })
}

/// Infer monitor index when position is outside all monitor bounds.
/// Uses Y coordinate: negative Y = secondary monitor (index 1), else primary (index 0).
const fn infer_monitor_index(y: i32) -> usize {
    (y < 0) as usize
}

/// Populate `WinitInfo` resource from winit (decoration and starting monitor).
pub fn init_winit_info(
    mut commands: Commands,
    windows: Query<Entity, With<Window>>,
    monitors: Query<(Entity, &Monitor)>,
    _non_send: NonSendMarker,
) {
    let Ok(entity) = windows.single() else {
        return;
    };

    let monitors_vec: Vec<_> = monitors.iter().collect();

    WINIT_WINDOWS.with(|ww| {
        let ww = ww.borrow();
        if let Some(winit_window) = ww.get_window(entity) {
            let outer = winit_window.outer_size();
            let inner = winit_window.inner_size();
            let decoration = WindowDecoration {
                width: outer.width.saturating_sub(inner.width),
                height: outer.height.saturating_sub(inner.height),
            };

            // Get actual position from winit to determine starting monitor
            let pos = winit_window
                .outer_position()
                .map(|p| IVec2::new(p.x, p.y))
                .unwrap_or(IVec2::ZERO);

            let starting_monitor_index =
                monitor_at(&monitors_vec, pos.x, pos.y).map_or(0, |m| m.index);

            info!(
                "[Init] decoration={}x{} pos=({}, {}) starting_monitor={}",
                decoration.width, decoration.height, pos.x, pos.y, starting_monitor_index
            );

            commands.insert_resource(WinitInfo {
                starting_monitor_index,
                window_decoration: decoration,
            });
        }
    });
}

/// Step 1 (PreStartup): Move window to target monitor at 1x1 size.
#[expect(
    clippy::cast_possible_wrap,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::needless_pass_by_value,
    reason = "window dimensions safe; Bevy systems require owned Res types"
)]
pub fn step1_move_to_monitor(
    mut commands: Commands,
    mut windows: Query<(Entity, &mut Window)>,
    monitors: Query<(Entity, &Monitor)>,
    winit_info: Option<Res<WinitInfo>>,
    config: Res<RestoreWindowConfig>,
) {
    let Some(state) = state::load_state(&config.path) else {
        info!("[Step1] No saved state");
        return;
    };

    let Some((saved_x, saved_y)) = state.position else {
        info!("[Step1] No saved position");
        return;
    };

    let target_monitor_index = state.monitor_index;

    let Ok((window_entity, mut window)) = windows.single_mut() else {
        info!("[Step1] No window");
        return;
    };

    let monitors_vec: Vec<_> = monitors.iter().collect();

    // Get starting monitor from `WinitInfo` (actual position from winit)
    let starting_monitor_index = winit_info.as_ref().map_or(0, |w| w.starting_monitor_index);
    let starting_info = monitor_by_index(&monitors_vec, starting_monitor_index);
    let starting_scale = starting_info.as_ref().map_or(1.0, |m| m.scale as f32);

    let Some(target_info) = monitor_by_index(&monitors_vec, target_monitor_index) else {
        info!("[Step1] Target monitor index {target_monitor_index} not found");
        return;
    };

    let target_scale = target_info.scale as f32;

    // Determine restore phase based on scale relationship
    let phase = if (starting_scale - target_scale).abs() < 0.01 {
        RestorePhase::Direct
    } else if starting_scale < target_scale {
        RestorePhase::Compensate
    } else {
        RestorePhase::TwoPhase(TwoPhaseState::WaitingForScaleChange)
    };

    info!(
        "[Step1] Starting monitor={} scale={}, Target monitor={} scale={}, phase={:?}",
        starting_monitor_index, starting_scale, target_monitor_index, target_scale, phase
    );

    // Calculate final position with clamping based on target size
    let target_width = state.width as u32;
    let target_height = state.height as u32;

    // Clamp position to fit target size within monitor
    let mon_right = target_info.position.x + target_info.size.x as i32;
    let mon_bottom = target_info.position.y + target_info.size.y as i32;

    let mut final_x = saved_x;
    let mut final_y = saved_y;

    // Clamp to right/bottom edges based on target size
    if final_x + target_width as i32 > mon_right {
        final_x = mon_right - target_width as i32;
    }
    if final_y + target_height as i32 > mon_bottom {
        final_y = mon_bottom - target_height as i32;
    }

    // Ensure we don't go past left/top edges
    final_x = final_x.max(target_info.position.x);
    final_y = final_y.max(target_info.position.y);

    info!(
        "[Step1] Final position: ({}, {}) -> clamped ({}, {}) for size {}x{}",
        saved_x, saved_y, final_x, final_y, target_width, target_height
    );

    // For TwoPhase, compensate the position because winit will divide by starting_scale
    let (move_x, move_y) = if matches!(phase, RestorePhase::TwoPhase(_)) {
        let ratio = starting_scale / target_scale;
        let comp_x = (final_x as f32 * ratio) as i32;
        let comp_y = (final_y as f32 * ratio) as i32;
        info!(
            "[Step1] TwoPhase: compensating position ({}, {}) -> ({}, {}) (ratio={})",
            final_x, final_y, comp_x, comp_y, ratio
        );
        (comp_x, comp_y)
    } else {
        (final_x, final_y)
    };

    info!("[WE SET] position=({}, {}) size=1x1", move_x, move_y);

    window.position = bevy::window::WindowPosition::At(IVec2::new(move_x, move_y));
    window.resolution.set_physical_resolution(1, 1);

    // Store target info for restore
    commands.insert_resource(TargetPosition {
        x: saved_x,
        y: saved_y,
        width: state.width as u32,
        height: state.height as u32,
        entity: window_entity,
        target_scale,
        starting_scale,
        phase,
    });
}

/// Step 2 (Startup): Just logging.
pub fn step2_apply_exact(target: Option<Res<TargetPosition>>) {
    if let Some(t) = target {
        info!(
            "[Step2] phase={:?}, will apply in handle_window_messages",
            t.phase
        );
    } else {
        info!("[Step2] No target position");
    }
}

/// Handle window messages - apply pending restore or save state.
#[expect(
    clippy::too_many_arguments,
    reason = "Bevy systems require many owned params"
)]
pub fn handle_window_messages(
    mut commands: Commands,
    mut moved_messages: MessageReader<WindowMoved>,
    mut resized_messages: MessageReader<WindowResized>,
    mut scale_factor_changed_messages: MessageReader<WindowScaleFactorChanged>,
    target: Option<ResMut<TargetPosition>>,
    winit_info: Option<Res<WinitInfo>>,
    config: Res<RestoreWindowConfig>,
    mut windows: Query<&mut Window>,
    monitors: Query<(Entity, &Monitor)>,
) {
    let decoration = winit_info.as_ref().map_or((0, 0), |w| {
        (w.window_decoration.width, w.window_decoration.height)
    });

    // Drain messages to clear queue
    let scale_changed = scale_factor_changed_messages.read().last().cloned();
    let last_move = moved_messages.read().last().cloned();
    let last_resize = resized_messages.read().last().cloned();

    // If we have a pending restore, try to apply it
    if let Some(mut target) = target {
        let monitors_vec: Vec<_> = monitors.iter().collect();

        // Check for phase transitions
        match target.phase {
            RestorePhase::TwoPhase(TwoPhaseState::WaitingForScaleChange) => {
                if scale_changed.is_some() {
                    info!("[Restore] ScaleChanged received, transitioning to WaitingForSettle");
                    target.phase = RestorePhase::TwoPhase(TwoPhaseState::WaitingForSettle);
                }
            }
            RestorePhase::TwoPhase(TwoPhaseState::WaitingForSettle) => {
                // Wait one frame for messages to settle, then transition
                info!("[Restore] Settling complete, transitioning to ReadyToApplySize");
                target.phase = RestorePhase::TwoPhase(TwoPhaseState::ReadyToApplySize);
            }
            _ => {}
        }

        if try_apply_restore(
            &target,
            &monitors_vec,
            &mut windows,
            decoration,
            &mut commands,
        ) {
            return; // Don't save during restore
        }
    }

    // Save state on messages (only when not restoring)
    let Ok(window) = windows.single() else {
        return;
    };

    if let Some(message) = scale_changed {
        info!("[Message:ScaleChanged] scale={}", message.scale_factor);
    }

    let monitors_vec: Vec<_> = monitors.iter().collect();
    let last_move_pos = last_move.as_ref().map(|m| m.position);

    if let Some(message) = last_move {
        save_on_move(&message, window, &monitors_vec, decoration, &config.path);
    }

    if let Some(message) = last_resize {
        save_on_resize(
            &message,
            window,
            &monitors_vec,
            decoration,
            last_move_pos,
            &config.path,
        );
    }
}

/// Try to apply a pending window restore. Returns `true` if restore is still in progress.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "window dimensions and scale factors are within safe ranges"
)]
fn try_apply_restore(
    target: &TargetPosition,
    monitors: &[(Entity, &Monitor)],
    windows: &mut Query<&mut Window>,
    decoration: (u32, u32),
    commands: &mut Commands,
) -> bool {
    // Check current monitor via window position
    let current_pos = windows
        .get(target.entity)
        .ok()
        .and_then(|w| match w.position {
            bevy::window::WindowPosition::At(p) => Some(p),
            _ => None,
        });

    let Some(pos) = current_pos else {
        return true;
    };

    let current_scale = monitor_at(monitors, pos.x, pos.y)
        .as_ref()
        .map_or(1.0, |m| m.scale as f32);

    info!(
        "[Restore] pos=({}, {}) current_scale={} target_scale={} phase={:?}",
        pos.x, pos.y, current_scale, target.target_scale, target.phase
    );

    // For TwoPhase(WaitingForScaleChange), just wait - don't apply anything yet
    if target.phase == RestorePhase::TwoPhase(TwoPhaseState::WaitingForScaleChange) {
        info!("[Restore] TwoPhase: waiting for ScaleChanged message");
        return true;
    }

    // Not yet on target monitor (for Direct/Compensate phases)
    if !matches!(
        target.phase,
        RestorePhase::TwoPhase(TwoPhaseState::ReadyToApplySize)
    ) && (current_scale - target.target_scale).abs() >= 0.01
    {
        return true;
    }

    // Log target monitor info
    if let Some(mon) = monitor_at(monitors, target.x, target.y) {
        info!(
            "[Restore] Target monitor: pos=({}, {}) size={}x{} scale={}",
            mon.position.x, mon.position.y, mon.size.x, mon.size.y, mon.scale
        );
    }

    let (decoration_width, decoration_height) = decoration;
    let inner_width = target.width.saturating_sub(decoration_width);
    let inner_height = target.height.saturating_sub(decoration_height);

    // Clamp position to keep window fully within target monitor
    let (final_x, final_y) = clamp_to_monitor(target, monitors);

    let Ok(mut window) = windows.get_mut(target.entity) else {
        return true;
    };

    match target.phase {
        RestorePhase::Direct => {
            info!(
                "[WE SET] position=({}, {}) size={}x{} (Direct)",
                final_x, final_y, inner_width, inner_height
            );
            window.position = bevy::window::WindowPosition::At(IVec2::new(final_x, final_y));
            window
                .resolution
                .set_physical_resolution(inner_width, inner_height);
        }
        RestorePhase::Compensate => {
            // Low→High DPI: compensate with ratio < 1
            let ratio = target.starting_scale / target.target_scale;
            let comp_x = (final_x as f32 * ratio) as i32;
            let comp_y = (final_y as f32 * ratio) as i32;
            let comp_width = (inner_width as f32 * ratio) as u32;
            let comp_height = (inner_height as f32 * ratio) as u32;

            info!(
                "[WE SET] position=({}, {}) size={}x{} (Compensate, ratio={})",
                comp_x, comp_y, comp_width, comp_height, ratio
            );
            window.position = bevy::window::WindowPosition::At(IVec2::new(comp_x, comp_y));
            window
                .resolution
                .set_physical_resolution(comp_width, comp_height);
        }
        RestorePhase::TwoPhase(TwoPhaseState::ReadyToApplySize) => {
            // High→Low DPI after scale changed: ONLY set size, position was set in Step1
            info!(
                "[WE SET] size={}x{} ONLY (TwoPhase final, position already set)",
                inner_width, inner_height
            );
            window
                .resolution
                .set_physical_resolution(inner_width, inner_height);
        }
        RestorePhase::TwoPhase(
            TwoPhaseState::WaitingForScaleChange | TwoPhaseState::WaitingForSettle,
        ) => {
            // Still waiting, don't apply yet
            return true;
        }
    }

    commands.remove_resource::<TargetPosition>();
    true
}

/// Clamp window position to fit within target monitor bounds.
#[expect(
    clippy::cast_possible_wrap,
    reason = "window dimensions are within safe ranges"
)]
fn clamp_to_monitor(target: &TargetPosition, monitors: &[(Entity, &Monitor)]) -> (i32, i32) {
    let Some(mon) = monitor_at(monitors, target.x, target.y) else {
        return (target.x, target.y);
    };

    let mon_right = mon.position.x + mon.size.x as i32;
    let mon_bottom = mon.position.y + mon.size.y as i32;

    let mut x = target.x;
    let mut y = target.y;

    // Clamp to right/bottom edges
    if x + target.width as i32 > mon_right {
        x = mon_right - target.width as i32;
    }
    if y + target.height as i32 > mon_bottom {
        y = mon_bottom - target.height as i32;
    }

    // Ensure we don't go past left/top edges
    x = x.max(mon.position.x);
    y = y.max(mon.position.y);

    if x != target.x || y != target.y {
        info!(
            "[Restore] Clamped position: ({}, {}) -> ({}, {})",
            target.x, target.y, x, y
        );
    }

    (x, y)
}

/// Save window state on move message.
#[expect(clippy::cast_precision_loss, reason = "window dimensions fit in f32")]
fn save_on_move(
    message: &WindowMoved,
    window: &Window,
    monitors: &[(Entity, &Monitor)],
    decoration: (u32, u32),
    path: &std::path::Path,
) {
    let (decoration_width, decoration_height) = decoration;
    let monitor_index = monitor_at(monitors, message.position.x, message.position.y)
        .map_or_else(|| infer_monitor_index(message.position.y), |m| m.index);

    let outer_width = window.resolution.physical_width() + decoration_width;
    let outer_height = window.resolution.physical_height() + decoration_height;

    info!(
        "[Message:Moved] pos=({}, {}) size={}x{} monitor={}",
        message.position.x, message.position.y, outer_width, outer_height, monitor_index
    );

    let state = WindowState {
        position: Some((message.position.x, message.position.y)),
        width: outer_width as f32,
        height: outer_height as f32,
        monitor_index,
    };
    state::save_state(path, &state);
}

/// Save window state on resize message.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "window dimensions are within safe ranges"
)]
fn save_on_resize(
    message: &WindowResized,
    window: &Window,
    monitors: &[(Entity, &Monitor)],
    decoration: (u32, u32),
    last_move_pos: Option<IVec2>,
    path: &std::path::Path,
) {
    let (decoration_width, decoration_height) = decoration;
    let scale = window.scale_factor();
    let physical_width = (message.width * scale) as u32 + decoration_width;
    let physical_height = (message.height * scale) as u32 + decoration_height;

    // Use position from move message if available, otherwise from window component
    let pos = last_move_pos.or(match window.position {
        bevy::window::WindowPosition::At(p) => Some(p),
        _ => None,
    });

    let Some(pos) = pos else {
        info!(
            "[Message:Resized] logical={}x{} - skipping save, position unknown",
            message.width, message.height
        );
        return;
    };

    let monitor_index =
        monitor_at(monitors, pos.x, pos.y).map_or_else(|| infer_monitor_index(pos.y), |m| m.index);

    info!(
        "[Message:Resized] logical={}x{} physical={}x{} pos=({}, {}) monitor={}",
        message.width, message.height, physical_width, physical_height, pos.x, pos.y, monitor_index
    );

    let state = WindowState {
        position: Some((pos.x, pos.y)),
        width: physical_width as f32,
        height: physical_height as f32,
        monitor_index,
    };
    state::save_state(path, &state);
}
