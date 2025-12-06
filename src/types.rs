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

/// Holds the target window position/size during a two-phase restore process.
///
/// Window restoration requires two phases because Bevy's `changed_windows` system
/// applies scale factor conversion using cached values that may not match the target
/// monitor's scale. By first moving the window to the target monitor (Step 1), we
/// trigger a scale factor update. Then in `handle_window_events`, once the window
/// is on the correct monitor, we apply the final position/size with proper scale
/// compensation.
#[derive(Resource)]
pub struct TargetPosition {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub entity: Entity,
    pub target_scale: f32,
    /// Scale of the monitor the window is PHYSICALLY on at startup (before any moves).
    pub starting_scale: f32,
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
