//! Type definitions for window restoration.

use bevy::prelude::*;
use serde::Deserialize;
use serde::Serialize;
use std::path::PathBuf;

/// Window decoration dimensions (title bar, borders).
pub struct WindowDecoration {
    pub width: u32,
    pub height: u32,
}

/// Information from winit captured at startup.
#[derive(Resource)]
pub struct WinitInfo {
    pub starting_monitor_index: usize,
    pub window_decoration: WindowDecoration,
}

/// State for the two-phase restore process (high→low DPI only).
///
/// In `TwoPhase`, we must set position BEFORE size because Bevy's `changed_windows`
/// system processes size changes before position changes. If we set both together,
/// the window resizes first while still at the old position, temporarily extending
/// into the wrong monitor and triggering a scale factor bounce from macOS.
///
/// By setting the final position in Step1 (with a 1x1 window), we ensure the window
/// is already at the correct location when we later apply size in `ReadyToApplySize`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TwoPhaseState {
    /// Position applied with compensation, waiting for `ScaleChanged` message.
    WaitingForScaleChange,
    /// Scale changed, ready to apply final size (position already set in Step1).
    ReadyToApplySize,
}

/// Restore strategy based on scale factor relationship between starting and target monitors.
///
/// The strategy depends on how winit/macOS handle coordinate scaling:
/// - Winit uses the "keyboard focus monitor's" scale factor for coordinate math
/// - When setting position/size, values are divided by the starting monitor's scale
/// - This means we must compensate when scales differ
///
/// # Variants
///
/// - **Direct**: Same scale on both monitors (1→1 or 2→2). Apply position and size directly.
///
/// - **Compensate**: Low→High DPI (1x→2x, ratio < 1). Multiply values by ratio before
///   applying so that after winit divides by starting scale, we get the correct result.
///
/// - **`TwoPhase`**: High→Low DPI (2x→1x, ratio > 1). Cannot use simple compensation because
///   the compensated size would exceed monitor bounds and get clamped by macOS. Instead:
///   1. Move a 1x1 window to the final position (compensated) to trigger scale change
///   2. After scale changes, apply size without compensation (position already correct)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestorePhase {
    /// Same scale - apply position and size directly.
    Direct,
    /// Low→High DPI (1x→2x) - apply with compensation (ratio < 1).
    Compensate,
    /// High→Low DPI (2x→1x) - requires two phases (see enum docs).
    TwoPhase(TwoPhaseState),
}

/// Holds the target window state during the restore process.
///
/// All values are pre-computed with proper types. Casting from saved state
/// happens once during loading, not scattered throughout the restore logic.
#[derive(Resource)]
pub struct TargetPosition {
    /// Final clamped position (adjusted to fit within target monitor).
    pub x: i32,
    pub y: i32,
    /// Target outer size (including window decoration).
    pub width: u32,
    pub height: u32,
    /// Window entity being restored.
    pub entity: Entity,
    /// Scale factor of the target monitor.
    pub target_scale: f32,
    /// Scale factor of the monitor where the window starts (keyboard focus monitor).
    pub starting_scale: f32,
    /// Current restore phase/strategy.
    pub phase: RestorePhase,
}

/// Information about a monitor.
#[derive(Clone)]
pub struct MonitorInfo {
    pub index: usize,
    pub scale: f64,
    pub position: IVec2,
    pub size: UVec2,
}

/// Configuration for the `RestoreWindowPlugin`.
#[derive(Resource, Clone)]
pub struct RestoreWindowConfig {
    /// Full path to the state file.
    pub path: PathBuf,
}

/// Saved window state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowState {
    pub position: Option<(i32, i32)>,
    pub width: f32,
    pub height: f32,
    pub monitor_index: usize,
}
