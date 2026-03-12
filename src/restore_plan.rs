//! Shared restore target planning for primary and managed windows.

use bevy::prelude::*;

use crate::monitors::MonitorInfo;
use crate::monitors::Monitors;
use crate::types::FullscreenRestoreState;
use crate::types::MonitorScaleStrategy;
use crate::types::SCALE_FACTOR_EPSILON;
use crate::types::TargetPosition;
use crate::types::WindowRestoreState;
use crate::types::WindowState;

/// Resolve the target monitor from saved state and return an adjusted saved position.
///
/// If the saved monitor no longer exists, falls back to monitor 0 and drops saved position
/// because the coordinates referred to the missing monitor.
#[must_use]
pub(crate) fn resolve_target_monitor_and_position<'a>(
    saved_monitor_index: usize,
    saved_position: Option<(i32, i32)>,
    monitors: &'a Monitors,
) -> (&'a MonitorInfo, Option<(i32, i32)>, bool) {
    if let Some(info) = monitors.by_index(saved_monitor_index) {
        (info, saved_position, false)
    } else {
        (monitors.first(), None, true)
    }
}

/// Compute a `TargetPosition` from saved state and a resolved target monitor.
#[must_use]
pub(crate) fn compute_target_position(
    saved_state: &WindowState,
    target_info: &MonitorInfo,
    fallback_position: Option<(i32, i32)>,
    decoration: UVec2,
    starting_scale: f64,
) -> TargetPosition {
    let width = saved_state.width;
    let height = saved_state.height;
    let target_scale = target_info.scale;

    let outer_width = width + decoration.x;
    let outer_height = height + decoration.y;
    let position = fallback_position
        .map(|(x, y)| clamp_position_to_monitor(x, y, target_info, outer_width, outer_height));

    TargetPosition {
        position,
        width,
        height,
        target_scale,
        starting_scale,
        monitor_scale_strategy: determine_scale_strategy(starting_scale, target_scale),
        mode: saved_state.mode.clone(),
        target_monitor_index: target_info.index,
        fullscreen_restore_state: if saved_state.mode.is_fullscreen() {
            if cfg!(all(
                target_os = "windows",
                feature = "workaround-winit-3124"
            )) {
                Some(FullscreenRestoreState::WaitForSurface)
            } else if cfg!(target_os = "linux") && !crate::systems::is_wayland() {
                // X11: use MoveToMonitor state machine so compositor has time to process
                // the position change before fullscreen mode is applied.
                Some(FullscreenRestoreState::MoveToMonitor)
            } else {
                // macOS/Wayland: apply mode directly.
                Some(FullscreenRestoreState::ApplyMode)
            }
        } else {
            None
        },
        settle_state: None,
    }
}

/// Calculate restored window position, with optional clamping for macOS.
///
/// On macOS, clamps to monitor bounds because macOS may resize/reposition windows
/// that extend beyond the screen. macOS does not allow windows to span monitors.
///
/// On Windows and Linux X11, windows can legitimately span multiple monitors,
/// so we preserve the exact saved position without clamping.
#[must_use]
fn clamp_position_to_monitor(
    saved_x: i32,
    saved_y: i32,
    target_info: &MonitorInfo,
    outer_width: u32,
    outer_height: u32,
) -> IVec2 {
    if cfg!(target_os = "macos") {
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
                "[clamp_position_to_monitor] Clamped: ({saved_x}, {saved_y}) -> ({x}, {y}) for outer size {outer_width}x{outer_height}"
            );
        }

        IVec2::new(x, y)
    } else {
        IVec2::new(saved_x, saved_y)
    }
}

/// Determine the monitor scale strategy based on platform and scale factors.
///
/// The strategy depends on how each platform's winit backend interprets coordinates
/// passed to `request_inner_size` and `set_outer_position`:
///
/// - **Feature gate**: Without `workaround-winit-4440`, always returns `ApplyUnchanged`.
/// - **Wayland**: Handles DPI natively; no compensation needed → `ApplyUnchanged`.
/// - **Same scale**: `starting ≈ target` → `ApplyUnchanged` (no cross-DPI issue).
/// - **Windows** (different scales): Position is physical and unaffected, but size goes through a
///   scale conversion using the current monitor's scale → `CompensateSizeOnly`. Uses a two-phase
///   approach: Phase 1 applies compensated size + position to move the window to the target
///   monitor, Phase 2 re-applies the exact target size after the DPI transition to eliminate
///   rounding errors from fractional scale factors (e.g. 1.5×).
/// - **macOS / Linux X11** (different scales): Both position and size are affected by winit's scale
///   conversion → `LowerToHigher` (ratio < 1) or `HigherToLower` (ratio > 1, two-phase to avoid
///   size clamping from macOS).
pub fn determine_scale_strategy(starting_scale: f64, target_scale: f64) -> MonitorScaleStrategy {
    if !cfg!(feature = "workaround-winit-4440") {
        return MonitorScaleStrategy::ApplyUnchanged;
    }

    if cfg!(target_os = "linux") && crate::systems::is_wayland() {
        return MonitorScaleStrategy::ApplyUnchanged;
    }

    if (starting_scale - target_scale).abs() < SCALE_FACTOR_EPSILON {
        MonitorScaleStrategy::ApplyUnchanged
    } else if cfg!(target_os = "windows") {
        MonitorScaleStrategy::CompensateSizeOnly(WindowRestoreState::NeedInitialMove)
    } else if starting_scale < target_scale {
        MonitorScaleStrategy::LowerToHigher
    } else {
        MonitorScaleStrategy::HigherToLower(WindowRestoreState::NeedInitialMove)
    }
}
