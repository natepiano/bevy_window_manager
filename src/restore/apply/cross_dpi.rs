use bevy::prelude::*;
use bevy::window::WindowPosition;
use bevy_kana::ToI32;
use bevy_kana::ToU32;

use crate::restore::settle::SettleState;
use crate::restore::target::MonitorScaleStrategy;
use crate::restore::target::TargetPosition;
use crate::restore::target::WindowRestoreState;

/// Apply the initial window move to the target monitor.
pub(super) fn apply_initial_move(target: &TargetPosition, window: &mut Window) {
    #[derive(Debug)]
    struct MoveParams {
        position: IVec2,
        width:    u32,
        height:   u32,
    }

    if target.mode.is_fullscreen() {
        if let Some(position) = target.physical_position {
            debug!(
                "[apply_initial_move] Moving to target position {:?} for fullscreen mode {:?}",
                position, target.mode
            );
            window.position = WindowPosition::At(position);
        } else {
            debug!(
                "[apply_initial_move] No position available (Wayland), fullscreen mode {:?}",
                target.mode
            );
        }
        return;
    }

    let Some(position) = target.physical_position else {
        debug!(
            "[apply_initial_move] No position available (Wayland), setting size only: {}x{}",
            target.physical_size.x, target.physical_size.y
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
            .set_physical_resolution(target.physical_size.x, target.physical_size.y);
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

    let params = match target.scale_strategy {
        MonitorScaleStrategy::HigherToLower(_) => {
            let ratio = target.starting_scale / target.target_scale;
            let compensated_x = (f64::from(position.x) * ratio).to_i32();
            let compensated_y = (f64::from(position.y) * ratio).to_i32();
            debug!(
                "[apply_initial_move] HigherToLower: compensating position {position:?} -> ({compensated_x}, {compensated_y}) (ratio={ratio})",
            );
            MoveParams {
                position: IVec2::new(compensated_x, compensated_y),
                width:    target.physical_size.x,
                height:   target.physical_size.y,
            }
        },
        MonitorScaleStrategy::CompensateSizeOnly(_) => {
            let compensated = target.compensated_size();
            debug!(
                "[apply_initial_move] CompensateSizeOnly: position={:?} compensated_size={}x{} (ratio={})",
                position,
                compensated.x,
                compensated.y,
                target.ratio()
            );
            MoveParams {
                position,
                width: compensated.x,
                height: compensated.y,
            }
        },
        _ => MoveParams {
            position,
            width: target.physical_size.x,
            height: target.physical_size.y,
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

/// Handle the initial move for cross-DPI strategies.
pub(super) fn begin_cross_dpi_restore(target: &mut TargetPosition, window: &mut Window) {
    if target.physical_position.is_none() {
        let width = (f64::from(target.logical_size.x) * target.starting_scale).to_u32();
        let height = (f64::from(target.logical_size.y) * target.starting_scale).to_u32();
        debug!(
            "[restore_windows] No position for cross-DPI restore, applying logical size \
             {}x{} at starting_scale={} (physical {}x{}) instead of two-phase dance",
            target.logical_size.x, target.logical_size.y, target.starting_scale, width, height
        );
        window.resolution.set_physical_resolution(width, height);
        window.visible = true;
        target.settle_state = Some(SettleState::new());
        return;
    }

    apply_initial_move(target, window);
    target.scale_strategy = match target.scale_strategy {
        MonitorScaleStrategy::HigherToLower(_) => {
            MonitorScaleStrategy::HigherToLower(WindowRestoreState::WaitingForScaleChange)
        },
        _ => MonitorScaleStrategy::CompensateSizeOnly(WindowRestoreState::WaitingForScaleChange),
    };
}
