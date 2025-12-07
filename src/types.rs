//! Type definitions for window restoration.

use std::path::PathBuf;

use bevy::prelude::*;
use serde::Deserialize;
use serde::Serialize;

/// Window decoration dimensions (title bar, borders).
pub struct WindowDecoration {
    pub width:  u32,
    pub height: u32,
}

/// Information from winit captured at startup.
#[derive(Resource)]
pub struct WinitInfo {
    pub starting_monitor_index: usize,
    pub window_decoration:      WindowDecoration,
}

/// State for `MonitorScaleStrategy::HigherToLower` (high→low DPI restore).
///
/// When restoring from a high-DPI to low-DPI monitor, we must set position BEFORE size
/// because Bevy's `changed_windows` system processes size changes before position changes.
/// If we set both together, the window resizes first while still at the old position,
/// temporarily extending into the wrong monitor and triggering a scale factor bounce from macOS.
///
/// By moving a 1x1 window to the final position first, we ensure the window is already
/// at the correct location when we later apply size in `ApplySize`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowRestoreState {
    /// Position applied with compensation, waiting for `ScaleChanged` message.
    WaitingForScaleChange,
    /// Scale changed, ready to apply final size (position already set in phase 1).
    ApplySize,
}

/// Restore strategy based on scale factor relationship between launch and target monitors.
///
/// The strategy depends on how winit/macOS handle coordinate scaling:
/// - Winit uses the "keyboard focus monitor's" scale factor for coordinate math
/// - When setting position/size, values are divided by the launch monitor's scale
/// - This means we must compensate when scales differ
///
/// # Variants
///
/// - **`ApplyUnchanged`**: Same scale on both monitors (1→1 or 2→2). Apply position and size
///   directly without compensation.
///
/// - **`LowerToHigher`**: Low→High DPI (1x→2x, ratio < 1). Multiply values by ratio before applying
///   so that after winit divides by launch scale, we get the correct result.
///
/// - **`HigherToLower`**: High→Low DPI (2x→1x, ratio > 1). Cannot use simple compensation because
///   the compensated size would exceed monitor bounds and get clamped by macOS. Instead uses a
///   two-phase approach via `WindowRestoreState`:
///   1. Move a 1x1 window to the final position (compensated) to trigger scale change
///   2. After scale changes, apply size without compensation (position already correct)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MonitorScaleStrategy {
    /// Same scale - apply position and size directly.
    ApplyUnchanged,
    /// Low→High DPI (1x→2x) - apply with compensation (ratio < 1).
    LowerToHigher,
    /// High→Low DPI (2x→1x) - requires two phases (see enum docs).
    HigherToLower(WindowRestoreState),
}

/// Holds the target window state during the restore process.
///
/// All values are pre-computed with proper types. Casting from saved state
/// happens once during loading, not scattered throughout the restore logic.
#[derive(Resource)]
pub struct TargetPosition {
    /// Final clamped position (adjusted to fit within target monitor).
    pub x:                      i32,
    pub y:                      i32,
    /// Target outer size (including window decoration).
    pub width:                  u32,
    pub height:                 u32,
    /// Window entity being restored.
    pub entity:                 Entity,
    /// Scale factor of the target monitor.
    pub target_scale:           f64,
    /// Scale factor of the monitor where the window starts (keyboard focus monitor).
    pub starting_scale:         f64,
    /// Strategy for handling scale factor differences between monitors.
    pub monitor_scale_strategy: MonitorScaleStrategy,
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
    pub position:      Option<(i32, i32)>,
    pub width:         u32,
    pub height:        u32,
    pub monitor_index: usize,
}
