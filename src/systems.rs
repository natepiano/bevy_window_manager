//! Systems for window restoration.

use bevy::ecs::system::NonSendMarker;
use bevy::prelude::*;
use bevy::window::Monitor;
use bevy::window::PrimaryWindow;
use bevy::window::WindowMoved;
use bevy::window::WindowResized;
use bevy::window::WindowScaleFactorChanged;
use bevy::winit::WINIT_WINDOWS;

use super::state;
use super::types::MonitorInfo;
use super::types::RestoreWindowConfig;
use super::types::WindowState;
use crate::types::MonitorScaleStrategy;
use crate::types::TargetPosition;
use crate::types::WindowDecoration;
use crate::types::WindowRestoreState;
use crate::types::WinitInfo;

/// Get sort key for monitor (primary at 0,0 first, then by position).
const fn monitor_sort_key(mon: &Monitor) -> (bool, i32, i32) {
    let pos = mon.physical_position;
    let is_primary = pos.x == 0 && pos.y == 0;
    (!is_primary, pos.x, pos.y)
}

/// Create a sorted monitor list (primary first, then by position).
fn sorted_monitors<'a>(monitors: impl Iterator<Item = &'a Monitor>) -> Vec<&'a Monitor> {
    let mut sorted: Vec<_> = monitors.collect();
    sorted.sort_by_key(|mon| monitor_sort_key(mon));
    sorted
}

/// Get monitor info at a position (assumes pre-sorted monitors).
#[expect(
    clippy::cast_possible_wrap,
    reason = "monitor dimensions are always within i32 range"
)]
fn monitor_at(monitors: &[&Monitor], x: i32, y: i32) -> Option<MonitorInfo> {
    monitors.iter().enumerate().find_map(|(idx, mon)| {
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

/// Get monitor info by index (assumes pre-sorted monitors).
fn monitor_by_index(monitors: &[&Monitor], index: usize) -> Option<MonitorInfo> {
    monitors.get(index).map(|mon| MonitorInfo {
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
    window_entity: Single<Entity, With<PrimaryWindow>>,
    monitors: Query<(Entity, &Monitor)>,
    _non_send: NonSendMarker,
) {
    let monitors = sorted_monitors(monitors.iter().map(|(_, m)| m));

    WINIT_WINDOWS.with(|ww| {
        let ww = ww.borrow();
        if let Some(winit_window) = ww.get_window(*window_entity) {
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

            let starting_monitor_index = monitor_at(&monitors, pos.x, pos.y).map_or(0, |m| m.index);

            info!(
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
/// All type conversions (f32→u32, etc.) happen here in one place.
/// Runs after `init_winit_info` so we have access to starting monitor info.
#[expect(
    clippy::cast_possible_wrap,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::needless_pass_by_value,
    reason = "window dimensions safe; Bevy systems require owned Res types"
)]
pub fn load_target_position(
    mut commands: Commands,
    window_entity: Single<Entity, With<PrimaryWindow>>,
    monitors: Query<(Entity, &Monitor)>,
    winit_info: Option<Res<WinitInfo>>,
    config: Res<RestoreWindowConfig>,
) {
    let Some(state) = state::load_state(&config.path) else {
        debug!("[load_target_position] No saved bevy_restore_window state");
        return;
    };

    let Some((saved_x, saved_y)) = state.position else {
        debug!("[load_target_position] No saved bevy_restore_window position");
        return;
    };

    let monitors = sorted_monitors(monitors.iter().map(|(_, m)| m));

    // Get starting monitor from WinitInfo
    let starting_monitor_index = winit_info.as_ref().map_or(0, |w| w.starting_monitor_index);
    let starting_info = monitor_by_index(&monitors, starting_monitor_index);
    let starting_scale = starting_info.as_ref().map_or(1.0, |m| m.scale as f32);

    let Some(target_info) = monitor_by_index(&monitors, state.monitor_index) else {
        info!(
            "[load_target_position] Target monitor index {} not found",
            state.monitor_index
        );
        return;
    };

    let target_scale = target_info.scale as f32;

    // Convert saved dimensions (all casting happens here)
    let width = state.width as u32;
    let height = state.height as u32;

    // Determine monitor scale strategy based on scale relationship
    let strategy = if (starting_scale - target_scale).abs() < 0.01 {
        MonitorScaleStrategy::ApplyUnchanged
    } else if starting_scale < target_scale {
        MonitorScaleStrategy::LowerToHigher
    } else {
        MonitorScaleStrategy::HigherToLower(WindowRestoreState::WaitingForScaleChange)
    };

    // Calculate final clamped position
    let mon_right = target_info.position.x + target_info.size.x as i32;
    let mon_bottom = target_info.position.y + target_info.size.y as i32;

    let mut x = saved_x;
    let mut y = saved_y;

    // Clamp to fit within monitor bounds
    if x + width as i32 > mon_right {
        x = mon_right - width as i32;
    }
    if y + height as i32 > mon_bottom {
        y = mon_bottom - height as i32;
    }
    x = x.max(target_info.position.x);
    y = y.max(target_info.position.y);

    if x != saved_x || y != saved_y {
        info!(
            "[load_target_position] Clamped position: ({}, {}) -> ({}, {}) for size {}x{}",
            saved_x, saved_y, x, y, width, height
        );
    }

    info!(
        "[load_target_position] Starting monitor={} scale={}, Target monitor={} scale={}, strategy={:?}",
        starting_monitor_index, starting_scale, state.monitor_index, target_scale, strategy
    );

    commands.insert_resource(TargetPosition {
        x,
        y,
        width,
        height,
        entity: *window_entity,
        target_scale,
        starting_scale,
        monitor_scale_strategy: strategy,
    });
}

/// Move window to target monitor at 1x1 size (`PreStartup`).
///
/// Uses pre-computed `TargetPosition` to move the window. For `HigherToLower` strategy,
/// the position is compensated because winit divides by launch scale.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    reason = "position values fit in f32 and i32"
)]
pub fn move_to_target_monitor(
    mut window: Single<&mut Window, With<PrimaryWindow>>,
    target: Res<TargetPosition>,
) {
    // For HigherToLower, compensate position because winit divides by launch scale
    let (move_x, move_y) = if matches!(
        target.monitor_scale_strategy,
        MonitorScaleStrategy::HigherToLower(_)
    ) {
        let ratio = target.starting_scale / target.target_scale;
        let comp_x = (target.x as f32 * ratio) as i32;
        let comp_y = (target.y as f32 * ratio) as i32;
        info!(
            "[move_to_target_monitor] HigherToLower: compensating position ({}, {}) -> ({}, {}) (ratio={})",
            target.x, target.y, comp_x, comp_y, ratio
        );
        (comp_x, comp_y)
    } else {
        (target.x, target.y)
    };

    info!(
        "[move_to_target_monitor] position=({}, {}) size=1x1",
        move_x, move_y
    );

    window.position = bevy::window::WindowPosition::At(IVec2::new(move_x, move_y));
    window.resolution.set_physical_resolution(1, 1);
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
        let monitors = sorted_monitors(monitors.iter().map(|(_, m)| m));

        // Check for HigherToLower state transitions
        if target.monitor_scale_strategy
            == MonitorScaleStrategy::HigherToLower(WindowRestoreState::WaitingForScaleChange)
            && scale_changed.is_some()
        {
            info!(
                "[Restore] ScaleChanged received, transitioning to WindowRestoreState::ApplySize"
            );
            target.monitor_scale_strategy =
                MonitorScaleStrategy::HigherToLower(WindowRestoreState::ApplySize);
        }

        if try_apply_restore(&target, &monitors, &mut windows, decoration, &mut commands) {
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

    let monitors = sorted_monitors(monitors.iter().map(|(_, m)| m));
    let last_move_pos = last_move.as_ref().map(|m| m.position);

    if let Some(message) = last_move {
        save_on_move(&message, window, &monitors, decoration, &config.path);
    }

    if let Some(message) = last_resize {
        save_on_resize(
            &message,
            window,
            &monitors,
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
    monitors: &[&Monitor],
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
        "[Restore] pos=({}, {}) current_scale={} target_scale={} strategy={:?}",
        pos.x, pos.y, current_scale, target.target_scale, target.monitor_scale_strategy
    );

    // For HigherToLower(WaitingForScaleChange), just wait - don't apply anything yet
    if target.monitor_scale_strategy
        == MonitorScaleStrategy::HigherToLower(WindowRestoreState::WaitingForScaleChange)
    {
        info!("[Restore] HigherToLower: waiting for ScaleChanged message");
        return true;
    }

    // Not yet on target monitor (for ApplyUnchanged/LowerToHigher strategies)
    if !matches!(
        target.monitor_scale_strategy,
        MonitorScaleStrategy::HigherToLower(WindowRestoreState::ApplySize)
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

    // Position is already clamped in TargetPosition
    let Ok(mut window) = windows.get_mut(target.entity) else {
        return true;
    };

    match target.monitor_scale_strategy {
        MonitorScaleStrategy::ApplyUnchanged => {
            info!(
                "[try_apply_restore] position=({}, {}) size={}x{} (ApplyUnchanged)",
                target.x, target.y, inner_width, inner_height
            );
            window.position = bevy::window::WindowPosition::At(IVec2::new(target.x, target.y));
            window
                .resolution
                .set_physical_resolution(inner_width, inner_height);
        }
        MonitorScaleStrategy::LowerToHigher => {
            // Low→High DPI: compensate with ratio < 1
            let ratio = target.starting_scale / target.target_scale;
            let comp_x = (target.x as f32 * ratio) as i32;
            let comp_y = (target.y as f32 * ratio) as i32;
            let comp_width = (inner_width as f32 * ratio) as u32;
            let comp_height = (inner_height as f32 * ratio) as u32;

            info!(
                "[try_apply_restore] position=({}, {}) size={}x{} (LowerToHigher, ratio={})",
                comp_x, comp_y, comp_width, comp_height, ratio
            );
            window.position = bevy::window::WindowPosition::At(IVec2::new(comp_x, comp_y));
            window
                .resolution
                .set_physical_resolution(comp_width, comp_height);
        }
        MonitorScaleStrategy::HigherToLower(WindowRestoreState::ApplySize) => {
            // HigherToLower after scale changed: ONLY set size, position was set earlier
            info!(
                "[try_apply_restore] size={}x{} ONLY (HigherToLower::ApplySize, position already set)",
                inner_width, inner_height
            );
            window
                .resolution
                .set_physical_resolution(inner_width, inner_height);
        }
        MonitorScaleStrategy::HigherToLower(WindowRestoreState::WaitingForScaleChange) => {
            // Still waiting, don't apply yet
            return true;
        }
    }

    commands.remove_resource::<TargetPosition>();
    true
}

/// Save window state on move message.
#[expect(clippy::cast_precision_loss, reason = "window dimensions fit in f32")]
fn save_on_move(
    message: &WindowMoved,
    window: &Window,
    monitors: &[&Monitor],
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
    monitors: &[&Monitor],
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
