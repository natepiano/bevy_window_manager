//! Workaround for macOS W4 (winit #4441): Window size resets when dragging back to launch monitor.
//!
//! When restoring a window from High DPI (scale=2) to Low DPI (scale=1), our two-phase workaround
//! correctly sizes the window. However, when the user drags the window back to the High DPI
//! monitor, `AppKit`'s per-scale-factor size tracking resets the window to the cached wrong size
//! from Phase 1.
//!
//! This module detects the drag-back and re-applies the correct size.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy::window::WindowResized;
use bevy::window::WindowScaleFactorChanged;

use crate::types::SCALE_FACTOR_EPSILON;

/// State of the W4 drag-back correction process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CorrectionState {
    /// Waiting for user to drag back to launch monitor
    WaitingForDragBack,
    /// Drag-back detected, waiting for `AppKit`'s live resize to apply wrong size
    PendingCorrection {
        /// The correct size to apply (scaled up from `restored_scale` to `launch_scale`)
        corrected_size:    UVec2,
        /// The wrong size that W4 will apply (the Phase 1 cached size)
        wrong_cached_size: UVec2,
    },
}

/// Tracks the expected size for W4 protection after a High→Low DPI restore.
///
/// Inserted after Phase 2 of `HigherToLower` restore completes. Removed when:
/// - User drags back to launch monitor (scale change triggers correction)
/// - User manually resizes the window (they've taken control)
#[derive(Resource)]
pub struct DragBackSizeProtection {
    /// The correct physical size at the restored scale (Phase 2 size)
    pub expected_physical_size: UVec2,
    /// The scale factor of the launch monitor (where W4 caches occur)
    pub launch_scale:           f64,
    /// The scale factor we restored to (target monitor)
    pub restored_scale:         f64,
    /// The Phase 1 cached size at `launch_scale` (what W4 will reset to)
    pub phase1_cached_size:     UVec2,
    /// Current state of the correction process
    pub state:                  CorrectionState,
}

/// Initialize the W4 drag-back fix systems.
pub fn init(app: &mut App) {
    app.add_systems(
        Update,
        (
            detect_user_resize,
            handle_drag_back_scale_change,
            apply_pending_correction,
        )
            .chain()
            .run_if(resource_exists::<DragBackSizeProtection>),
    );
}

/// Detect user manual resize and remove protection resource.
///
/// If the user resizes the window while on the restored monitor, they've taken control
/// and we should not interfere with subsequent drag-backs.
fn detect_user_resize(
    mut commands: Commands,
    protection: Res<DragBackSizeProtection>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut resize_messages: MessageReader<WindowResized>,
) {
    // Only check in WaitingForDragBack state
    if protection.state != CorrectionState::WaitingForDragBack {
        return;
    }

    // Only check if we received a resize message
    if resize_messages.read().last().is_none() {
        return;
    }

    let current_scale = f64::from(window.resolution.scale_factor());

    // Only consider it a user resize if we're still on the restored monitor
    if (current_scale - protection.restored_scale).abs() > SCALE_FACTOR_EPSILON {
        return;
    }

    let current_size = UVec2::new(
        window.resolution.physical_width(),
        window.resolution.physical_height(),
    );

    // If size changed from expected while on restored scale, user resized
    if current_size != protection.expected_physical_size {
        debug!(
            "[W4 fix] User resize detected: {}x{} -> {}x{}, removing protection",
            protection.expected_physical_size.x,
            protection.expected_physical_size.y,
            current_size.x,
            current_size.y
        );
        commands.remove_resource::<DragBackSizeProtection>();
    }
}

/// Handle scale change when dragging back to launch monitor.
///
/// When the window is dragged back to the launch monitor (scale changes to `launch_scale`),
/// transition to `PendingCorrection` state. We don't apply immediately because `AppKit`'s
/// live resize will overwrite our correction - we need to wait for the resize to complete.
fn handle_drag_back_scale_change(
    mut protection: ResMut<DragBackSizeProtection>,
    mut scale_changed_messages: MessageReader<WindowScaleFactorChanged>,
) {
    // Only act in WaitingForDragBack state
    if protection.state != CorrectionState::WaitingForDragBack {
        return;
    }

    // Only act on scale change events
    let Some(scale_event) = scale_changed_messages.read().last() else {
        return;
    };

    let new_scale = scale_event.scale_factor;

    // Check if we're transitioning to the launch monitor
    if (new_scale - protection.launch_scale).abs() > SCALE_FACTOR_EPSILON {
        debug!(
            "[W4 fix] Scale changed to {} (not launch_scale {}), ignoring",
            new_scale, protection.launch_scale
        );
        return;
    }

    // Calculate the correct physical size at launch scale
    let ratio = protection.launch_scale / protection.restored_scale;
    let corrected_width = (f64::from(protection.expected_physical_size.x) * ratio) as u32;
    let corrected_height = (f64::from(protection.expected_physical_size.y) * ratio) as u32;
    let corrected_size = UVec2::new(corrected_width, corrected_height);

    debug!(
        "[W4 fix] Drag-back detected: scale {} -> {}, queueing correction {}x{} -> {}x{} (waiting for wrong size {}x{})",
        protection.restored_scale,
        protection.launch_scale,
        protection.expected_physical_size.x,
        protection.expected_physical_size.y,
        corrected_width,
        corrected_height,
        protection.phase1_cached_size.x,
        protection.phase1_cached_size.y,
    );

    protection.state = CorrectionState::PendingCorrection {
        corrected_size,
        wrong_cached_size: protection.phase1_cached_size,
    };
}

/// Apply pending correction after `AppKit`'s live resize applies the wrong cached size.
///
/// When in `PendingCorrection` state and we detect the window has been resized to the
/// wrong cached size (W4 behavior), apply our correction.
fn apply_pending_correction(
    mut commands: Commands,
    protection: Res<DragBackSizeProtection>,
    mut window: Single<&mut Window, With<PrimaryWindow>>,
    mut resize_messages: MessageReader<WindowResized>,
) {
    let CorrectionState::PendingCorrection {
        corrected_size,
        wrong_cached_size,
    } = protection.state
    else {
        return;
    };

    // Wait for a resize event
    if resize_messages.read().last().is_none() {
        return;
    }

    let current_size = UVec2::new(
        window.resolution.physical_width(),
        window.resolution.physical_height(),
    );

    // Only apply correction when we see the wrong cached size (W4 has triggered)
    // Use tolerance of 2 pixels due to rounding (AppKit rounds fractional logical sizes)
    let size_matches = current_size.x.abs_diff(wrong_cached_size.x) <= 2
        && current_size.y.abs_diff(wrong_cached_size.y) <= 2;

    if !size_matches {
        debug!(
            "[W4 fix] Resize to {}x{}, waiting for wrong size ~{}x{}",
            current_size.x, current_size.y, wrong_cached_size.x, wrong_cached_size.y
        );
        return;
    }

    debug!(
        "[W4 fix] W4 detected (size={}x{}), applying correction: {}x{}, removing protection",
        current_size.x, current_size.y, corrected_size.x, corrected_size.y
    );

    window
        .resolution
        .set_physical_resolution(corrected_size.x, corrected_size.y);

    commands.remove_resource::<DragBackSizeProtection>();
}
