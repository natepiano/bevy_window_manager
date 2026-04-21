use bevy::ecs::system::NonSendMarker;
use bevy::prelude::*;
use bevy::window::MonitorSelection;
use bevy::window::WindowMode;
use bevy::window::WindowPosition;
use bevy::window::WindowScaleFactorChanged;
use bevy::winit::WINIT_WINDOWS;

use super::cross_dpi;
use crate::Platform;
use crate::constants::SCALE_FACTOR_EPSILON;
use crate::persistence::SavedWindowMode;
use crate::restore::settle::SettleState;
use crate::restore::target::FullscreenRestoreState;
use crate::restore::target::MonitorScaleStrategy;
use crate::restore::target::TargetPosition;
use crate::restore::target::WindowRestoreState;
use crate::restore::target::X11FrameCompensated;

/// Apply pending window restore. Runs only when entities with `TargetPosition` exist.
pub fn restore_windows(
    mut scale_changed_messages: MessageReader<WindowScaleFactorChanged>,
    mut windows: Query<(Entity, &mut TargetPosition, &mut Window), With<X11FrameCompensated>>,
    _: NonSendMarker,
    platform: Res<Platform>,
) {
    let scale_changed = scale_changed_messages.read().last().is_some();

    for (entity, mut target, mut window) in &mut windows {
        if target.settle_state.is_some() {
            continue;
        }

        let winit_window_exists =
            WINIT_WINDOWS.with(|winit_windows| winit_windows.borrow().get_window(entity).is_some());
        if !winit_window_exists {
            debug!("[restore_windows] Skipping entity {entity:?}: winit window not yet created");
            continue;
        }

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

        if matches!(
            target.scale_strategy,
            MonitorScaleStrategy::HigherToLower(WindowRestoreState::NeedInitialMove)
                | MonitorScaleStrategy::CompensateSizeOnly(WindowRestoreState::NeedInitialMove)
        ) {
            cross_dpi::begin_cross_dpi_restore(&mut target, &mut window);
            continue;
        }

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

        if let Some(fullscreen_state) = target.fullscreen_state {
            match fullscreen_state {
                FullscreenRestoreState::MoveToMonitor => {
                    if let Some(position) = target.physical_position {
                        debug!("[restore_windows] Fullscreen MoveToMonitor: position={position:?}");
                        window.position = WindowPosition::At(position);
                    }
                    target.fullscreen_state = Some(FullscreenRestoreState::WaitForMove);
                    continue;
                },
                FullscreenRestoreState::WaitForMove => {
                    debug!("[restore_windows] Fullscreen WaitForMove: waiting for compositor");
                    target.fullscreen_state = Some(FullscreenRestoreState::ApplyMode);
                    continue;
                },
                FullscreenRestoreState::WaitForSurface => {
                    debug!("[restore_windows] Fullscreen WaitForSurface: waiting for GPU surface");
                    target.fullscreen_state = Some(FullscreenRestoreState::ApplyMode);
                    continue;
                },
                FullscreenRestoreState::ApplyMode => {},
            }
        }

        if matches!(
            try_apply_restore(&target, &mut window, *platform),
            RestoreStatus::Complete
        ) && target.settle_state.is_none()
        {
            info!(
                "[restore_windows] Restore applied, starting settle (200ms stability / 1s timeout)"
            );
            target.settle_state = Some(SettleState::new());
        }
    }
}

enum RestoreStatus {
    Complete,
    Waiting,
}

fn apply_window_geometry(
    window: &mut Window,
    position: Option<IVec2>,
    size: UVec2,
    strategy: &str,
    ratio: Option<f64>,
) {
    if let Some(position) = position {
        if let Some(ratio) = ratio {
            debug!(
                "[try_apply_restore] position={:?} size={}x{} ({strategy}, ratio={ratio})",
                position, size.x, size.y
            );
        } else {
            debug!(
                "[try_apply_restore] position={:?} size={}x{} ({strategy})",
                position, size.x, size.y
            );
        }
        window.position = WindowPosition::At(position);
    } else if let Some(ratio) = ratio {
        debug!(
            "[try_apply_restore] size={}x{} only ({strategy}, ratio={ratio}, no position)",
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

fn apply_fullscreen_restore(target: &TargetPosition, window: &mut Window, platform: Platform) {
    let monitor_index = target.monitor_index;

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

fn try_apply_restore(
    target: &TargetPosition,
    window: &mut Window,
    platform: Platform,
) -> RestoreStatus {
    if target.mode.is_fullscreen() {
        debug!(
            "[try_apply_restore] fullscreen: mode={:?} target_monitor={} current_physical={}x{} current_mode={:?} current_pos={:?}",
            target.mode,
            target.monitor_index,
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
        target.physical_position, target.target_scale, target.scale_strategy
    );

    match target.scale_strategy {
        MonitorScaleStrategy::ApplyUnchanged => {
            apply_window_geometry(
                window,
                target.physical_position,
                target.physical_size,
                "ApplyUnchanged",
                None,
            );
        },
        MonitorScaleStrategy::CompensateSizeOnly(WindowRestoreState::ApplySize) => {
            debug!(
                "[try_apply_restore] size={}x{} ONLY (CompensateSizeOnly::ApplySize, position already set)",
                target.physical_size.x, target.physical_size.y
            );
            window
                .resolution
                .set_physical_resolution(target.physical_size.x, target.physical_size.y);
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
            debug!(
                "[try_apply_restore] size={}x{} ONLY (HigherToLower::ApplySize, position already set)",
                target.physical_size.x, target.physical_size.y
            );
            window
                .resolution
                .set_physical_resolution(target.physical_size.x, target.physical_size.y);
        },
        MonitorScaleStrategy::HigherToLower(
            WindowRestoreState::NeedInitialMove | WindowRestoreState::WaitingForScaleChange,
        ) => {
            debug!("[Restore] HigherToLower: waiting for initial move or ScaleChanged message");
            return RestoreStatus::Waiting;
        },
    }

    window.visible = true;
    RestoreStatus::Complete
}
