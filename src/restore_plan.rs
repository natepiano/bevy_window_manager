//! Shared restore target planning for primary and managed windows.

use bevy::prelude::*;

use crate::Platform;
use crate::monitors::MonitorInfo;
use crate::monitors::Monitors;
use crate::types::TargetPosition;
use crate::types::WindowState;

/// Resolve the target monitor from saved state and return an adjusted saved position.
///
/// If the saved monitor no longer exists, falls back to monitor 0 and drops saved position
/// because the coordinates referred to the missing monitor.
#[must_use]
pub fn resolve_target_monitor_and_position(
    saved_monitor_index: usize,
    saved_position: Option<(i32, i32)>,
    monitors: &Monitors,
) -> (&MonitorInfo, Option<(i32, i32)>, bool) {
    monitors.by_index(saved_monitor_index).map_or_else(
        || (monitors.first(), None, true),
        |info| (info, saved_position, false),
    )
}

/// Compute a `TargetPosition` from saved state and a resolved target monitor.
#[must_use]
pub fn compute_target_position(
    saved_state: &WindowState,
    target_info: &MonitorInfo,
    fallback_position: Option<(i32, i32)>,
    decoration: UVec2,
    starting_scale: f64,
    platform: Platform,
) -> TargetPosition {
    let width = saved_state.width;
    let height = saved_state.height;
    let target_scale = target_info.scale;

    let outer_width = width + decoration.x;
    let outer_height = height + decoration.y;
    let position = fallback_position.map(|(x, y)| {
        clamp_position_to_monitor(x, y, target_info, outer_width, outer_height, platform)
    });

    TargetPosition {
        position,
        width,
        height,
        target_scale,
        starting_scale,
        monitor_scale_strategy: platform.scale_strategy(starting_scale, target_scale),
        mode: saved_state.mode.clone(),
        target_monitor_index: target_info.index,
        fullscreen_restore_state: if saved_state.mode.is_fullscreen() {
            Some(platform.fullscreen_restore_state())
        } else {
            None
        },
        settle_state: None,
    }
}

/// Calculate restored window position, with optional clamping.
///
/// On macOS, clamps to monitor bounds because macOS may resize/reposition windows
/// that extend beyond the screen. macOS does not allow windows to span monitors.
///
/// On Windows and Linux, windows can legitimately span multiple monitors,
/// so we preserve the exact saved position without clamping.
#[must_use]
fn clamp_position_to_monitor(
    saved_x: i32,
    saved_y: i32,
    target_info: &MonitorInfo,
    outer_width: u32,
    outer_height: u32,
    platform: Platform,
) -> IVec2 {
    if platform.should_clamp_position() {
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
