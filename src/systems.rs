//! Systems for window restoration.

use bevy::ecs::system::NonSendMarker;
use bevy::prelude::*;
use bevy::window::Monitor;
use bevy::window::WindowMoved;
use bevy::window::WindowResized;
use bevy::window::WindowScaleFactorChanged;
use bevy::winit::WINIT_WINDOWS;

use crate::state::RestoreWindowConfig;
use crate::state::WindowState;
use crate::state::load_state;
use crate::state::save_state;
use crate::types::MonitorInfo;
use crate::types::TargetPosition;
use crate::types::WindowDecoration;
use crate::types::WinitInfo;

/// Get sort key for monitor (primary at 0,0 first, then by position).
fn monitor_sort_key(mon: &Monitor) -> (bool, i32, i32) {
    let pos = mon.physical_position;
    let is_primary = pos.x == 0 && pos.y == 0;
    (!is_primary, pos.x, pos.y)
}

/// Helper to get monitor info at a position from Bevy `Monitor` components.
fn monitor_at(monitors: &[(Entity, &Monitor)], x: i32, y: i32) -> Option<MonitorInfo> {
    let mut sorted: Vec<_> = monitors.iter().collect();
    sorted.sort_by_key(|(_, mon)| monitor_sort_key(mon));

    sorted.iter().enumerate().find_map(|(idx, (_, mon))| {
        let pos = mon.physical_position;
        let size = mon.physical_size();
        if x >= pos.x && x < pos.x + size.x as i32 && y >= pos.y && y < pos.y + size.y as i32 {
            Some(MonitorInfo {
                index: idx,
                name: mon.name.clone().unwrap_or_else(|| "?".to_string()),
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
        name: mon.name.clone().unwrap_or_else(|| "?".to_string()),
        scale: mon.scale_factor,
        position: mon.physical_position,
        size: mon.physical_size(),
    })
}

/// Infer monitor index when position is outside all monitor bounds.
/// Uses Y coordinate: negative Y = secondary monitor (index 1), else primary (index 0).
fn infer_monitor_index(y: i32) -> usize {
    if y < 0 { 1 } else { 0 }
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

            let starting_monitor_index = monitor_at(&monitors_vec, pos.x, pos.y)
                .map(|m| m.index)
                .unwrap_or(0);

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
pub fn step1_move_to_monitor(
    mut commands: Commands,
    mut windows: Query<(Entity, &mut Window)>,
    monitors: Query<(Entity, &Monitor)>,
    winit_info: Option<Res<WinitInfo>>,
    config: Res<RestoreWindowConfig>,
) {
    let Some(state) = load_state(&config.app_name) else {
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
    let starting_monitor_index = winit_info
        .as_ref()
        .map(|w| w.starting_monitor_index)
        .unwrap_or(0);
    let starting_info = monitor_by_index(&monitors_vec, starting_monitor_index);
    let starting_scale = starting_info
        .as_ref()
        .map(|m| m.scale as f32)
        .unwrap_or(1.0);

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

/// Step 2 (Startup): Logging only, actual positioning happens in `handle_window_events`
/// after window has moved to target monitor.
pub fn step2_apply_exact(target: Option<Res<TargetPosition>>) {
    if target.is_some() {
        info!("[Step2] Target stored, will apply after move completes");
    } else {
        info!("[Step2] No target position");
    }
}

/// Handle window events - apply pending restore or save state.
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
    let (decoration_width, decoration_height) = winit_info
        .as_ref()
        .map(|w| (w.window_decoration.width, w.window_decoration.height))
        .unwrap_or((0, 0));

    // Drain events to clear queue
    let last_scale: Option<_> = scale_events.read().last().cloned();
    let last_move: Option<_> = moved_events.read().last().cloned();
    let last_resize: Option<_> = resized_events.read().last().cloned();

    // If we have a pending restore, check if we can apply it
    if let Some(target) = target {
        let monitors_vec: Vec<_> = monitors.iter().collect();

        // Check current monitor via last move event or window position
        let current_pos = last_move.as_ref().map(|e| e.position).or_else(|| {
            windows
                .get(target.entity)
                .ok()
                .and_then(|w| match w.position {
                    bevy::window::WindowPosition::At(p) => Some(p),
                    _ => None,
                })
        });

        let Some(pos) = current_pos else {
            return;
        };

        let current_mon = monitor_at(&monitors_vec, pos.x, pos.y);
        let current_scale = current_mon.as_ref().map(|m| m.scale as f32).unwrap_or(1.0);

        info!(
            "[Restore] pos=({}, {}) current_scale={} target_scale={}",
            pos.x, pos.y, current_scale, target.target_scale
        );

        // If we're on the target monitor (scales match), apply final position/size
        if (current_scale - target.target_scale).abs() < 0.01 {
            info!("[Restore] On target monitor, applying final position/size");

            let inner_width = target.width.saturating_sub(decoration_width);
            let inner_height = target.height.saturating_sub(decoration_height);

            // Clamp position to keep window fully within target monitor
            // (programmatic positioning can't restore cross-monitor overlapping positions)
            let target_mon = monitor_at(&monitors_vec, target.x, target.y);
            let (final_x, final_y) = if let Some(ref mon) = target_mon {
                let mon_right = mon.position.x + mon.size.x as i32;
                let mon_bottom = mon.position.y + mon.size.y as i32;

                let mut x = target.x;
                let mut y = target.y;

                // Clamp right edge
                if x + target.width as i32 > mon_right {
                    x = mon_right - target.width as i32;
                }
                // Clamp bottom edge
                if y + target.height as i32 > mon_bottom {
                    y = mon_bottom - target.height as i32;
                }
                // Ensure we don't go past left/top edges after clamping
                x = x.max(mon.position.x);
                y = y.max(mon.position.y);

                if x != target.x || y != target.y {
                    info!(
                        "[Restore] Clamped position: ({}, {}) -> ({}, {}) to fit monitor",
                        target.x, target.y, x, y
                    );
                }

                (x, y)
            } else {
                (target.x, target.y)
            };

            if let Ok(mut window) = windows.get_mut(target.entity) {
                // Compensate for scale factor conversion in `changed_windows`
                // 2→1: ratio=2, size/pos doubled then halved back
                // 1→2: ratio=0.5, size/pos halved then doubled back
                // Same: ratio=1, no change
                let scale_ratio = target.starting_scale / target.target_scale;

                let comp_x = (final_x as f32 * scale_ratio) as i32;
                let comp_y = (final_y as f32 * scale_ratio) as i32;
                let comp_width = (inner_width as f32 * scale_ratio) as u32;
                let comp_height = (inner_height as f32 * scale_ratio) as u32;

                window.position = bevy::window::WindowPosition::At(IVec2::new(comp_x, comp_y));
                window
                    .resolution
                    .set_physical_resolution(comp_width, comp_height);

                info!(
                    "[Restore] Applied: pos=({}, {}) inner_size={}x{} (compensated from {}x{}, ratio={})",
                    final_x,
                    final_y,
                    comp_width,
                    comp_height,
                    inner_width,
                    inner_height,
                    scale_ratio
                );
            }

            commands.remove_resource::<TargetPosition>();
        }
        return; // Don't save during restore
    }

    // Save state on events (only when not restoring)
    let Ok(window) = windows.single() else {
        return;
    };

    if let Some(event) = last_scale {
        info!("[Event:ScaleChanged] scale={}", event.scale_factor);
    }

    let last_move_pos = last_move.as_ref().map(|m| m.position);

    if let Some(event) = last_move {
        let monitors_vec: Vec<_> = monitors.iter().collect();
        let monitor_index = monitor_at(&monitors_vec, event.position.x, event.position.y)
            .map(|m| m.index)
            .unwrap_or_else(|| infer_monitor_index(event.position.y));

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
        save_state(&config.app_name, &state);
    }

    if let Some(event) = last_resize {
        let scale = window.scale_factor();
        let physical_width = (event.width * scale) as u32 + decoration_width;
        let physical_height = (event.height * scale) as u32 + decoration_height;

        // Use position from move event if available, otherwise from window component
        let pos = last_move_pos.or_else(|| match window.position {
            bevy::window::WindowPosition::At(p) => Some(p),
            _ => None,
        });

        let Some(pos) = pos else {
            // Can't determine position, skip saving
            info!(
                "[Event:Resized] logical={}x{} - skipping save, position unknown",
                event.width, event.height
            );
            return;
        };

        let monitors_vec: Vec<_> = monitors.iter().collect();
        let monitor_index = monitor_at(&monitors_vec, pos.x, pos.y)
            .map(|m| m.index)
            .unwrap_or_else(|| infer_monitor_index(pos.y));

        info!(
            "[Event:Resized] logical={}x{} physical_outer={}x{} pos=({}, {}) monitor={} scale={}",
            event.width,
            event.height,
            physical_width,
            physical_height,
            pos.x,
            pos.y,
            monitor_index,
            scale
        );

        let state = WindowState {
            position: Some((pos.x, pos.y)),
            width: physical_width as f32,
            height: physical_height as f32,
            monitor_index,
        };
        save_state(&config.app_name, &state);
    }
}
