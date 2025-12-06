//! Type definitions for window restoration.

use bevy::prelude::*;

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

/// Resource to pass target position from PreStartup to Startup.
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
    pub name: String,
    pub scale: f64,
    pub position: IVec2,
    pub size: UVec2,
}

impl MonitorInfo {
    /// Format monitor info with window position and size for logging.
    pub fn format_with_window(&self, win_pos: IVec2, win_size: (u32, u32)) -> String {
        format!(
            "({} index={} scale={} pos=({}, {})) win_pos=({}, {}) win_size={}x{}",
            self.name,
            self.index,
            self.scale,
            self.position.x,
            self.position.y,
            win_pos.x,
            win_pos.y,
            win_size.0,
            win_size.1
        )
    }
}
