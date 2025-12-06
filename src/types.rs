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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TwoPhaseState {
    /// Position applied with compensation, waiting for `ScaleChanged` event.
    WaitingForScaleChange,
    /// Scale changed, waiting one more frame for events to settle.
    WaitingForSettle,
    /// Events settled, ready to apply final size without compensation.
    ReadyToApplySize,
}

/// Restore strategy based on scale factor relationship.
///
/// - `Direct`: Same scale on both monitors - apply position and size directly.
/// - `Compensate`: Low→High DPI (1x→2x) - apply with compensation (ratio < 1).
/// - `TwoPhase`: High→Low DPI (2x→1x) - requires two phases because compensated
///   size would exceed monitor bounds and get clamped by the OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestorePhase {
    /// Same scale - apply position and size directly.
    Direct,
    /// Low→High DPI (1x→2x) - apply with compensation (ratio < 1).
    Compensate,
    /// High→Low DPI (2x→1x) - requires two phases.
    TwoPhase(TwoPhaseState),
}

/// Holds the target window position/size during the restore process.
#[derive(Resource)]
pub struct TargetPosition {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub entity: Entity,
    pub target_scale: f32,
    /// Scale of the keyboard focus monitor at startup.
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
