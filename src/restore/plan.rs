//! Shared restore target planning for primary and managed windows.

use bevy::prelude::*;
use bevy_kana::ToI32;
use bevy_kana::ToU32;

use super::target::TargetPosition;
use crate::Platform;
use crate::monitors::MonitorInfo;
use crate::monitors::Monitors;
use crate::persistence::WindowState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MonitorResolutionSource {
    Requested,
    FallbackToPrimary,
}

pub struct ResolvedMonitor<'a> {
    pub info:             &'a MonitorInfo,
    pub logical_position: Option<(i32, i32)>,
    pub source:           MonitorResolutionSource,
}

/// Resolve the target monitor from saved state and return an adjusted saved position.
#[must_use]
pub fn resolve_target_monitor_and_position(
    saved_monitor_index: usize,
    saved_position: Option<(i32, i32)>,
    monitors: &Monitors,
) -> ResolvedMonitor<'_> {
    monitors.by_index(saved_monitor_index).map_or_else(
        || ResolvedMonitor {
            info:             monitors.first(),
            logical_position: None,
            source:           MonitorResolutionSource::FallbackToPrimary,
        },
        |info| ResolvedMonitor {
            info,
            logical_position: saved_position,
            source: MonitorResolutionSource::Requested,
        },
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
    let target_scale = target_info.scale;

    // Convert logical → physical using the target monitor's scale factor.
    // This is the single conversion point for size values.
    let width = (f64::from(saved_state.logical_width) * target_scale).to_u32();
    let height = (f64::from(saved_state.logical_height) * target_scale).to_u32();

    let outer_width = width + decoration.x;
    let outer_height = height + decoration.y;
    let position = fallback_position.map(|(x, y)| {
        // Convert logical position to physical using the target monitor's scale factor.
        let physical_x = (f64::from(x) * target_scale).round().to_i32();
        let physical_y = (f64::from(y) * target_scale).round().to_i32();
        clamp_position_to_monitor(
            physical_x,
            physical_y,
            target_info,
            outer_width,
            outer_height,
            platform,
        )
    });

    TargetPosition {
        physical_position: position,
        logical_position: fallback_position.map(|(x, y)| IVec2::new(x, y)),
        physical_size: UVec2::new(width, height),
        logical_size: UVec2::new(saved_state.logical_width, saved_state.logical_height),
        target_scale,
        starting_scale,
        scale_strategy: platform.scale_strategy(starting_scale, target_scale),
        mode: saved_state.mode.clone(),
        monitor_index: target_info.index,
        fullscreen_state: saved_state
            .mode
            .is_fullscreen()
            .then_some(platform.fullscreen_restore_state()),
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
        let monitor_right = target_info.physical_position.x + target_info.physical_size.x.to_i32();
        let monitor_bottom = target_info.physical_position.y + target_info.physical_size.y.to_i32();

        let mut x = saved_x;
        let mut y = saved_y;

        if x + outer_width.to_i32() > monitor_right {
            x = monitor_right - outer_width.to_i32();
        }
        if y + outer_height.to_i32() > monitor_bottom {
            y = monitor_bottom - outer_height.to_i32();
        }
        x = x.max(target_info.physical_position.x);
        y = y.max(target_info.physical_position.y);

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
