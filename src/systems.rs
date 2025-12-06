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
use crate::types::TargetPosition;
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

    info!(
        "[Step1] Starting monitor={} scale={}, Target monitor={} scale={}",
        starting_monitor_index, starting_scale, target_monitor_index, target_info.scale
    );

    // Move window to center of target monitor at 1x1 size
    // This triggers the scale factor change before Step2 applies final size
    let center_x = target_info.position.x + (target_info.size.x as i32 / 2);
    let center_y = target_info.position.y + (target_info.size.y as i32 / 2);

    info!(
        "[Step1] Moving to monitor center ({}, {}) at 1x1 size",
        center_x, center_y
    );

    window.position = bevy::window::WindowPosition::At(IVec2::new(center_x, center_y));
    window.resolution.set_physical_resolution(1, 1);

    // Store target info for restore
    commands.insert_resource(TargetPosition {
        x: saved_x,
        y: saved_y,
        width: state.width as u32,
        height: state.height as u32,
        entity: window_entity,
        target_scale: target_info.scale as f32,
        starting_scale,
    });
}

/// Step 2 (Startup): Just logging - scale override was set in Step1.
pub fn step2_apply_exact(target: Option<Res<TargetPosition>>) {
    if target.is_some() {
        info!("[Step2] Scale override set, will apply final size in handle_window_events");
    } else {
        info!("[Step2] No target position");
    }
}

/// Handle window events - apply pending restore or save state.
#[expect(
    clippy::too_many_arguments,
    reason = "Bevy systems require many owned params"
)]
pub fn handle_window_events(
    mut commands: Commands,
    mut moved_events: MessageReader<WindowMoved>,
    mut resized_events: MessageReader<WindowResized>,
    mut scale_events: MessageReader<WindowScaleFactorChanged>,
    target: Option<Res<TargetPosition>>,
    winit_info: Option<Res<WinitInfo>>,
    config: Res<RestoreWindowConfig>,
    mut windows: Query<&mut Window>,
    monitors: Query<(Entity, &Monitor)>,
) {
    let decoration = winit_info.as_ref().map_or((0, 0), |w| {
        (w.window_decoration.width, w.window_decoration.height)
    });

    // Drain events to clear queue
    let last_scale = scale_events.read().last().cloned();
    let last_move = moved_events.read().last().cloned();
    let last_resize = resized_events.read().last().cloned();

    // If we have a pending restore, try to apply it
    if let Some(target) = target {
        let monitors_vec: Vec<_> = monitors.iter().collect();
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

    // Save state on events (only when not restoring)
    let Ok(window) = windows.single() else {
        return;
    };

    if let Some(event) = last_scale {
        info!("[Event:ScaleChanged] scale={}", event.scale_factor);
    }

    let monitors_vec: Vec<_> = monitors.iter().collect();
    let last_move_pos = last_move.as_ref().map(|m| m.position);

    if let Some(event) = last_move {
        save_on_move(&event, window, &monitors_vec, decoration, &config.path);
    }

    if let Some(event) = last_resize {
        save_on_resize(
            &event,
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
        "[Restore] pos=({}, {}) current_scale={} target_scale={}",
        pos.x, pos.y, current_scale, target.target_scale
    );

    // Not yet on target monitor
    if (current_scale - target.target_scale).abs() >= 0.01 {
        return true;
    }

    // Log target monitor info
    if let Some(mon) = monitor_at(monitors, target.x, target.y) {
        info!(
            "[Restore] Target monitor: pos=({}, {}) size={}x{} scale={}",
            mon.position.x, mon.position.y, mon.size.x, mon.size.y, mon.scale
        );
    }

    info!("[Restore] On target monitor, applying final position/size");

    let (decoration_width, decoration_height) = decoration;
    let inner_width = target.width.saturating_sub(decoration_width);
    let inner_height = target.height.saturating_sub(decoration_height);

    // Clamp position to keep window fully within target monitor
    let (final_x, final_y) = clamp_to_monitor(target, monitors);

    let Ok(mut window) = windows.get_mut(target.entity) else {
        return true;
    };

    // Only compensate for winit's internal scale conversion when crossing monitors
    // with different scale factors. Winit uses the keyboard focus monitor's scale
    // for coordinate math, so we multiply by starting_scale/target_scale.
    let (apply_x, apply_y, apply_width, apply_height) =
        if (target.starting_scale - target.target_scale).abs() > 0.01 {
            let winit_ratio = target.starting_scale / target.target_scale;
            let comp_x = (final_x as f32 * winit_ratio) as i32;
            let comp_y = (final_y as f32 * winit_ratio) as i32;
            let comp_width = (inner_width as f32 * winit_ratio) as u32;
            let comp_height = (inner_height as f32 * winit_ratio) as u32;

            info!(
                "[Restore] Applying: pos=({}, {}) size={}x{} with winit compensation (ratio={})",
                final_x, final_y, inner_width, inner_height, winit_ratio
            );
            info!(
                "[Restore] Compensated: pos=({}, {}) size={}x{}",
                comp_x, comp_y, comp_width, comp_height
            );

            (comp_x, comp_y, comp_width, comp_height)
        } else {
            info!(
                "[Restore] Applying: pos=({}, {}) size={}x{} (no compensation needed)",
                final_x, final_y, inner_width, inner_height
            );
            (final_x, final_y, inner_width, inner_height)
        };

    window.position = bevy::window::WindowPosition::At(IVec2::new(apply_x, apply_y));
    window
        .resolution
        .set_physical_resolution(apply_width, apply_height);

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

/// Save window state on move event.
#[expect(clippy::cast_precision_loss, reason = "window dimensions fit in f32")]
fn save_on_move(
    event: &WindowMoved,
    window: &Window,
    monitors: &[(Entity, &Monitor)],
    decoration: (u32, u32),
    path: &std::path::Path,
) {
    let (decoration_width, decoration_height) = decoration;
    let monitor_index = monitor_at(monitors, event.position.x, event.position.y)
        .map_or_else(|| infer_monitor_index(event.position.y), |m| m.index);

    let outer_width = window.resolution.physical_width() + decoration_width;
    let outer_height = window.resolution.physical_height() + decoration_height;

    info!(
        "[Event:Moved] pos=({}, {}) size={}x{} monitor={}",
        event.position.x, event.position.y, outer_width, outer_height, monitor_index
    );

    let state = WindowState {
        position: Some((event.position.x, event.position.y)),
        width: outer_width as f32,
        height: outer_height as f32,
        monitor_index,
    };
    state::save_state(path, &state);
}

/// Save window state on resize event.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "window dimensions are within safe ranges"
)]
fn save_on_resize(
    event: &WindowResized,
    window: &Window,
    monitors: &[(Entity, &Monitor)],
    decoration: (u32, u32),
    last_move_pos: Option<IVec2>,
    path: &std::path::Path,
) {
    let (decoration_width, decoration_height) = decoration;
    let scale = window.scale_factor();
    let physical_width = (event.width * scale) as u32 + decoration_width;
    let physical_height = (event.height * scale) as u32 + decoration_height;

    // Use position from move event if available, otherwise from window component
    let pos = last_move_pos.or(match window.position {
        bevy::window::WindowPosition::At(p) => Some(p),
        _ => None,
    });

    let Some(pos) = pos else {
        info!(
            "[Event:Resized] logical={}x{} - skipping save, position unknown",
            event.width, event.height
        );
        return;
    };

    let monitor_index =
        monitor_at(monitors, pos.x, pos.y).map_or_else(|| infer_monitor_index(pos.y), |m| m.index);

    info!(
        "[Event:Resized] logical={}x{} physical={}x{} pos=({}, {}) monitor={}",
        event.width, event.height, physical_width, physical_height, pos.x, pos.y, monitor_index
    );

    let state = WindowState {
        position: Some((pos.x, pos.y)),
        width: physical_width as f32,
        height: physical_height as f32,
        monitor_index,
    };
    state::save_state(path, &state);
}
