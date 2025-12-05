//! Test app for window positioning workaround on multi-DPI macOS setups.
//!
//! Saves/restores window position to RON, compensating for winit's scale factor bug.

use std::fs;
use std::path::PathBuf;

use bevy::ecs::system::NonSendMarker;
use bevy::prelude::*;
use bevy::winit::WINIT_WINDOWS;
use serde::Deserialize;
use serde::Serialize;
use winit::dpi::PhysicalPosition;

const STATE_FILE: &str = "windows.ron";

fn main() {
    // Load saved state for initial window size
    let (width, height) = load_state()
        .map(|s| (s.width, s.height))
        .unwrap_or((400.0, 300.0));

    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Monitor Position Test".to_string(),
                resolution: (width as u32, height as u32).into(),
                // Don't set position - we'll fix it in apply_saved_position
                ..default()
            }),
            ..default()
        }))
        .add_systems(PreStartup, step1_move_to_monitor)
        .add_systems(Startup, step2_apply_exact)
        .add_systems(Update, save_on_change)
        .init_resource::<PositionTracker>()
        .run();
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WindowState {
    position: Option<(i32, i32)>,
    width: f32,
    height: f32,
    monitor_index: Option<usize>,
}

#[derive(Resource, Default)]
struct PositionTracker {
    last_position: Option<IVec2>,
    last_size: Option<(f32, f32)>,
}

fn get_state_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("nateroids").join(STATE_FILE))
}

fn load_state() -> Option<WindowState> {
    let path = get_state_path()?;
    let contents = fs::read_to_string(&path).ok()?;
    ron::from_str(&contents).ok()
}

fn save_state(state: &WindowState) {
    let Some(path) = get_state_path() else { return };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(contents) = ron::ser::to_string_pretty(state, ron::ser::PrettyConfig::default()) {
        let _ = fs::write(&path, contents);
    }
}

/// Resource to pass target position from PreStartup to Startup
#[derive(Resource)]
struct TargetPosition {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    entity: Entity,
}

struct MonitorInfo {
    index: usize,
    name: String,
    scale: f64,
    position: PhysicalPosition<i32>,
}

impl MonitorInfo {
    fn format_with_window(&self, win_pos: PhysicalPosition<i32>, win_size: (u32, u32)) -> String {
        format!(
            "(mon {} {} scale={} pos=({}, {})) win_pos=({}, {}) win_size={}x{}",
            self.index, self.name, self.scale, self.position.x, self.position.y,
            win_pos.x, win_pos.y, win_size.0, win_size.1
        )
    }
}

fn format_info(mon: Option<MonitorInfo>, win_pos: PhysicalPosition<i32>, win_size: (u32, u32)) -> String {
    mon.map_or_else(
        || format!("(mon ?) win_pos=({}, {}) win_size={}x{}", win_pos.x, win_pos.y, win_size.0, win_size.1),
        |m| m.format_with_window(win_pos, win_size),
    )
}

/// Helper to get monitor info at a position
fn monitor_at(
    monitors: impl Iterator<Item = winit::monitor::MonitorHandle>,
    x: i32,
    y: i32,
) -> Option<MonitorInfo> {
    monitors.enumerate().find_map(|(idx, mon)| {
        let pos = mon.position();
        let size = mon.size();
        if x >= pos.x && x < pos.x + size.width as i32 && y >= pos.y && y < pos.y + size.height as i32 {
            Some(MonitorInfo {
                index: idx,
                name: mon.name().unwrap_or_else(|| "?".to_string()),
                scale: mon.scale_factor(),
                position: pos,
            })
        } else {
            None
        }
    })
}

/// Helper to get monitor info by index
fn monitor_by_index(
    monitors: impl Iterator<Item = winit::monitor::MonitorHandle>,
    index: usize,
) -> Option<MonitorInfo> {
    monitors.enumerate().find_map(|(idx, mon)| {
        if idx == index {
            Some(MonitorInfo {
                index: idx,
                name: mon.name().unwrap_or_else(|| "?".to_string()),
                scale: mon.scale_factor(),
                position: mon.position(),
            })
        } else {
            None
        }
    })
}

/// Step 1 (PreStartup): Move window onto target monitor (just need to land on it)
fn step1_move_to_monitor(
    mut commands: Commands,
    windows: Query<Entity, With<Window>>,
    _non_send: NonSendMarker,
) {
    let Some(state) = load_state() else {
        info!("[Step1] No saved state");
        return;
    };

    let Some((saved_x, saved_y)) = state.position else {
        info!("[Step1] No saved position");
        return;
    };

    let Some(target_monitor_index) = state.monitor_index else {
        info!("[Step1] No saved monitor index");
        return;
    };

    let window_entity = windows.single().unwrap();

    WINIT_WINDOWS.with(|ww| {
        let ww = ww.borrow();
        let Some(winit_window) = ww.get_window(window_entity) else {
            info!("[Step1] No winit window");
            return;
        };

        let cur_pos = winit_window.outer_position().unwrap_or(PhysicalPosition::new(0, 0));
        let win_size = winit_window.inner_size();
        let current = monitor_at(winit_window.available_monitors(), cur_pos.x, cur_pos.y);
        let Some(target_info) = monitor_by_index(winit_window.available_monitors(), target_monitor_index) else {
            info!("[Step1] Target monitor index {target_monitor_index} not found");
            return;
        };

        // Need monitor size for center calculation
        let target_mon = winit_window.available_monitors().nth(target_monitor_index).unwrap();
        let mon_size = target_mon.size();
        let center_x = target_info.position.x + (mon_size.width as i32 / 2);
        let center_y = target_info.position.y + (mon_size.height as i32 / 2);

        info!("[Step1] Current: {}", format_info(current, cur_pos, (win_size.width, win_size.height)));
        info!("[Step1] Target:  {}", format_info(Some(target_info), PhysicalPosition::new(saved_x, saved_y), (win_size.width, win_size.height)));

        // Minimize window size before moving to avoid off-screen constraints
        info!("[Step1] Minimizing window to 1x1 before move");
        let _ = winit_window.request_inner_size(winit::dpi::PhysicalSize::new(1u32, 1u32));

        info!("[Step1] Sending: ({center_x}, {center_y}) [target monitor center]");
        winit_window.set_outer_position(PhysicalPosition::new(center_x, center_y));

        // Store target size from RON file (not scaled inner_size)
        commands.insert_resource(TargetPosition {
            x: saved_x,
            y: saved_y,
            width: state.width as u32,
            height: state.height as u32,
            entity: window_entity,
        });
    });
}

/// Step 2 (Startup): Apply exact position now that window is on correct monitor
fn step2_apply_exact(target: Option<Res<TargetPosition>>, _non_send: NonSendMarker) {
    let Some(target) = target else {
        info!("[Step2] No target position");
        return;
    };

    WINIT_WINDOWS.with(|ww| {
        let ww = ww.borrow();
        let Some(winit_window) = ww.get_window(target.entity) else {
            info!("[Step2] No winit window");
            return;
        };

        let cur_pos = winit_window.outer_position().unwrap_or(PhysicalPosition::new(0, 0));
        let win_size = winit_window.inner_size();
        let current = monitor_at(winit_window.available_monitors(), cur_pos.x, cur_pos.y);
        let target_mon = monitor_at(winit_window.available_monitors(), target.x, target.y);

        // Get what winit thinks the scale is vs what we know it actually is
        let winit_scale = winit_window.scale_factor();
        let actual_scale = current.as_ref().map(|m| m.scale).unwrap_or(winit_scale);

        // Compensate if they differ
        let compensation = winit_scale / actual_scale;
        let send_x = (target.x as f64 * compensation).round() as i32;
        let send_y = (target.y as f64 * compensation).round() as i32;

        info!("[Step2] Current: {}", format_info(current, cur_pos, (win_size.width, win_size.height)));
        info!("[Step2] Target:  {}", format_info(target_mon, PhysicalPosition::new(target.x, target.y), (target.width, target.height)));
        info!("[Step2] winit_scale={winit_scale}, actual_scale={actual_scale}, compensation={compensation}");
        info!("[Step2] Sending: ({send_x}, {send_y}) [compensated from ({}, {})]", target.x, target.y);
        winit_window.set_outer_position(PhysicalPosition::new(send_x, send_y));

        // Restore original size
        info!("[Step2] Restoring size to {}x{}", target.width, target.height);
        let _ = winit_window.request_inner_size(winit::dpi::PhysicalSize::new(target.width, target.height));
    });
}


/// Save window state when position or size changes
fn save_on_change(
    mut tracker: ResMut<PositionTracker>,
    windows: Query<(Entity, &Window)>,
    _non_send: NonSendMarker,
) {
    let Ok((entity, window)) = windows.single() else {
        return;
    };

    let current_pos = match window.position {
        bevy::window::WindowPosition::At(p) => Some(p),
        _ => None,
    };
    let current_size = (window.width(), window.height());

    let changed = tracker.last_position != current_pos || tracker.last_size != Some(current_size);
    if !changed {
        return;
    }

    tracker.last_position = current_pos;
    tracker.last_size = Some(current_size);

    // Don't save if position unknown yet
    let Some(pos) = current_pos else { return };

    // Find monitor index using winit's enumeration (matches MonitorSelection::Index)
    let monitor_index = WINIT_WINDOWS.with(|ww| {
        let ww = ww.borrow();
        let winit_window = ww.get_window(entity)?;
        winit_window
            .available_monitors()
            .enumerate()
            .find_map(|(i, mon)| {
                let mon_pos = mon.position();
                let mon_size = mon.size();
                if pos.x >= mon_pos.x
                    && pos.x < mon_pos.x + mon_size.width as i32
                    && pos.y >= mon_pos.y
                    && pos.y < mon_pos.y + mon_size.height as i32
                {
                    Some(i)
                } else {
                    None
                }
            })
    });

    let state = WindowState {
        position: Some((pos.x, pos.y)),
        width: current_size.0,
        height: current_size.1,
        monitor_index,
    };

    save_state(&state);
    info!(
        "Saved: pos=({}, {}) size={}x{} monitor={:?}",
        pos.x, pos.y, current_size.0, current_size.1, monitor_index
    );
}
