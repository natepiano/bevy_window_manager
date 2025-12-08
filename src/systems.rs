//! Systems for window restoration.

use bevy::ecs::system::NonSendMarker;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy::window::WindowMoved;
use bevy::window::WindowResized;
use bevy::window::WindowScaleFactorChanged;
use bevy::winit::WINIT_WINDOWS;

use super::state;
use super::types::RestoreWindowConfig;
use super::types::WindowState;
use crate::monitors::Monitors;
use crate::types::MonitorScaleStrategy;
use crate::types::SavedWindowMode;
use crate::types::TargetPosition;
use crate::types::WindowDecoration;
use crate::types::WindowRestoreState;
use crate::types::WinitInfo;

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

            let starting_monitor_index = monitors.at(pos.x, pos.y).map_or(0, |m| m.index);

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
/// Runs after `init_winit_info` so we have access to starting monitor info.
#[expect(
    clippy::cast_possible_wrap,
    reason = "window dimensions never exceed i32::MAX"
)]
pub fn load_target_position(
    mut commands: Commands,
    window_entity: Single<Entity, With<PrimaryWindow>>,
    monitors: Res<Monitors>,
    winit_info: Res<WinitInfo>,
    config: Res<RestoreWindowConfig>,
) {
    let Some(state) = state::load_state(&config.path) else {
        debug!("[load_target_position] No saved bevy_restore_windows state");
        return;
    };

    let Some((saved_x, saved_y)) = state.position else {
        debug!("[load_target_position] No saved bevy_restore_windows position");
        return;
    };

    // Get starting monitor from WinitInfo
    let starting_monitor_index = winit_info.starting_monitor_index;
    let starting_info = monitors.by_index(starting_monitor_index);
    let starting_scale = starting_info.map_or(1.0, |m| m.scale);

    let Some(target_info) = monitors.by_index(state.monitor_index) else {
        info!(
            "[load_target_position] Target monitor index {} not found",
            state.monitor_index
        );
        return;
    };

    let target_scale = target_info.scale;
    let width = state.width;
    let height = state.height;

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
        mode: state.mode,
    });
}

/// Move window to target monitor at 1x1 size (`PreStartup`).
///
/// Uses pre-computed `TargetPosition` to move the window. For `HigherToLower` strategy,
/// the position is compensated because winit divides by launch scale.
///
/// Skipped for fullscreen modes - they don't need position/size setup.
#[expect(
    clippy::cast_possible_truncation,
    reason = "position values fit in i32"
)]
pub fn move_to_target_monitor(
    mut window: Single<&mut Window, With<PrimaryWindow>>,
    target: Res<TargetPosition>,
) {
    // Skip for fullscreen modes - they don't need the 1x1 positioning trick
    if !matches!(target.mode, SavedWindowMode::Windowed) {
        info!(
            "[move_to_target_monitor] Skipping for fullscreen mode {:?}",
            target.mode
        );
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

/// Apply pending window restore. Runs only when `TargetPosition` exists.
///
/// Drains all window messages during restore to prevent saving.
#[expect(
    clippy::too_many_arguments,
    reason = "Bevy system requires many params"
)]
pub fn apply_restore(
    mut commands: Commands,
    mut moved_messages: MessageReader<WindowMoved>,
    mut resized_messages: MessageReader<WindowResized>,
    mut scale_changed_messages: MessageReader<WindowScaleFactorChanged>,
    mut target: ResMut<TargetPosition>,
    winit_info: Res<WinitInfo>,
    mut windows: Query<&mut Window>,
    monitors: Res<Monitors>,
) {
    let decoration = (
        winit_info.window_decoration.width,
        winit_info.window_decoration.height,
    );

    // Drain all messages (don't save during restore)
    let scale_changed = scale_changed_messages.read().last().is_some();
    let _ = moved_messages.read().count();
    let _ = resized_messages.read().count();

    // Handle HigherToLower state transition on scale change
    if target.monitor_scale_strategy
        == MonitorScaleStrategy::HigherToLower(WindowRestoreState::WaitingForScaleChange)
        && scale_changed
    {
        info!("[Restore] ScaleChanged received, transitioning to WindowRestoreState::ApplySize");
        target.monitor_scale_strategy =
            MonitorScaleStrategy::HigherToLower(WindowRestoreState::ApplySize);
    }

    if try_apply_restore(&target, &monitors, &mut windows, decoration) {
        commands.remove_resource::<TargetPosition>();
    }
}

/// Save window state on move/resize. Runs only when not restoring.
pub fn save_window_state(
    mut moved_messages: MessageReader<WindowMoved>,
    mut resized_messages: MessageReader<WindowResized>,
    winit_info: Res<WinitInfo>,
    config: Res<RestoreWindowConfig>,
    monitors: Res<Monitors>,
    windows: Query<&Window>,
) {
    // Check if any relevant messages arrived
    let moved = moved_messages.read().last().is_some();
    let resized = resized_messages.read().last().is_some();

    if !moved && !resized {
        return;
    }

    let Ok(window) = windows.single() else {
        return;
    };

    let Some(pos) = (match window.position {
        bevy::window::WindowPosition::At(p) => Some(p),
        _ => None,
    }) else {
        return;
    };

    let decoration = (
        winit_info.window_decoration.width,
        winit_info.window_decoration.height,
    );

    let outer_width = window.resolution.physical_width() + decoration.0;
    let outer_height = window.resolution.physical_height() + decoration.1;

    let monitor_index = monitors
        .at(pos.x, pos.y)
        .map_or_else(|| monitors.infer_index(pos.x, pos.y), |m| m.index);

    let mode = monitors.detect_effective_mode(window);

    info!(
        "[save_window_state] pos=({}, {}) size={}x{} monitor={} mode={:?}",
        pos.x, pos.y, outer_width, outer_height, monitor_index, mode
    );

    let state = WindowState {
        position: Some((pos.x, pos.y)),
        width: outer_width,
        height: outer_height,
        monitor_index,
        mode,
    };
    state::save_state(&config.path, &state);
}

/// Try to apply a pending window restore. Returns `true` when restore is complete.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "window dimensions and scale factors are within safe ranges"
)]
fn try_apply_restore(
    target: &TargetPosition,
    monitors: &Monitors,
    windows: &mut Query<&mut Window>,
    decoration: (u32, u32),
) -> bool {
    // Handle fullscreen modes - just set the mode and we're done
    if !matches!(target.mode, SavedWindowMode::Windowed) {
        let Ok(mut window) = windows.get_mut(target.entity) else {
            return false;
        };

        // Find the target monitor index from the saved position
        let monitor_index = monitors.at(target.x, target.y).map_or(0, |m| m.index);

        info!(
            "[Restore] Applying fullscreen mode {:?} on monitor {}",
            target.mode, monitor_index
        );

        window.mode = target.mode.to_window_mode(monitor_index);
        return true;
    }

    // Check current monitor via window position
    let current_pos = windows
        .get(target.entity)
        .ok()
        .and_then(|w| match w.position {
            bevy::window::WindowPosition::At(p) => Some(p),
            _ => None,
        });

    let Some(pos) = current_pos else {
        return false;
    };

    let current_scale = monitors.at(pos.x, pos.y).map_or(1.0, |m| m.scale);

    info!(
        "[Restore] pos=({}, {}) current_scale={} target_scale={} strategy={:?}",
        pos.x, pos.y, current_scale, target.target_scale, target.monitor_scale_strategy
    );

    // For HigherToLower(WaitingForScaleChange), just wait - don't apply anything yet
    if target.monitor_scale_strategy
        == MonitorScaleStrategy::HigherToLower(WindowRestoreState::WaitingForScaleChange)
    {
        info!("[Restore] HigherToLower: waiting for ScaleChanged message");
        return false;
    }

    // Not yet on target monitor (for ApplyUnchanged/LowerToHigher strategies)
    if !matches!(
        target.monitor_scale_strategy,
        MonitorScaleStrategy::HigherToLower(WindowRestoreState::ApplySize)
    ) && (current_scale - target.target_scale).abs() >= 0.01
    {
        return false;
    }

    // Log target monitor info
    if let Some(mon) = monitors.at(target.x, target.y) {
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
        return false;
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
        },
        MonitorScaleStrategy::LowerToHigher => {
            // Low→High DPI: compensate with ratio < 1
            let ratio = target.starting_scale / target.target_scale;
            let comp_x = (f64::from(target.x) * ratio) as i32;
            let comp_y = (f64::from(target.y) * ratio) as i32;
            let comp_width = (f64::from(inner_width) * ratio) as u32;
            let comp_height = (f64::from(inner_height) * ratio) as u32;

            info!(
                "[try_apply_restore] position=({}, {}) size={}x{} (LowerToHigher, ratio={})",
                comp_x, comp_y, comp_width, comp_height, ratio
            );
            window.position = bevy::window::WindowPosition::At(IVec2::new(comp_x, comp_y));
            window
                .resolution
                .set_physical_resolution(comp_width, comp_height);
        },
        MonitorScaleStrategy::HigherToLower(WindowRestoreState::ApplySize) => {
            // HigherToLower after scale changed: ONLY set size, position was set earlier
            info!(
                "[try_apply_restore] size={}x{} ONLY (HigherToLower::ApplySize, position already set)",
                inner_width, inner_height
            );
            window
                .resolution
                .set_physical_resolution(inner_width, inner_height);
        },
        MonitorScaleStrategy::HigherToLower(WindowRestoreState::WaitingForScaleChange) => {
            // Still waiting, don't apply yet
            return false;
        },
    }

    true
}
