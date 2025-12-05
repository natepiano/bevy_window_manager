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
        .add_systems(
            PreStartup,
            (init_window_decoration, step1_move_to_monitor).chain(),
        )
        .add_systems(Startup, step2_apply_exact)
        .add_systems(Update, save_on_change)
        .init_resource::<WindowTracker>()
        .run();
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WindowState {
    position: Option<(i32, i32)>,
    width: f32,
    height: f32,
    monitor_index: Option<usize>,
}

/// Tracks window position/size changes for saving
#[derive(Resource, Default)]
struct WindowTracker {
    last_position: Option<IVec2>,
    last_size: Option<(f32, f32)>,
}

/// Window decoration dimensions (title bar, borders) - populated once from winit
#[derive(Resource)]
struct WindowDecoration {
    width: u32,
    height: u32,
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
    target_scale: f32,
}

#[derive(Clone)]
struct MonitorInfo {
    index: usize,
    name: String,
    scale: f64,
    position: PhysicalPosition<i32>,
}

impl MonitorInfo {
    fn format_with_window(&self, win_pos: PhysicalPosition<i32>, win_size: (u32, u32)) -> String {
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

fn format_info(
    mon: Option<MonitorInfo>,
    win_pos: PhysicalPosition<i32>,
    win_size: (u32, u32),
) -> String {
    mon.map_or_else(
        || {
            format!(
                "(mon ?) win_pos=({}, {}) win_size={}x{}",
                win_pos.x, win_pos.y, win_size.0, win_size.1
            )
        },
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
        if x >= pos.x
            && x < pos.x + size.width as i32
            && y >= pos.y
            && y < pos.y + size.height as i32
        {
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

/// Populate WindowDecoration resource from winit (only winit call for decoration info)
fn init_window_decoration(
    mut commands: Commands,
    windows: Query<Entity, With<Window>>,
    _non_send: NonSendMarker,
) {
    let Ok(entity) = windows.single() else {
        return;
    };

    WINIT_WINDOWS.with(|ww| {
        let ww = ww.borrow();
        if let Some(winit_window) = ww.get_window(entity) {
            let outer = winit_window.outer_size();
            let inner = winit_window.inner_size();
            let decoration = WindowDecoration {
                width: outer.width.saturating_sub(inner.width),
                height: outer.height.saturating_sub(inner.height),
            };
            info!(
                "[Init] Window decoration: {}x{}",
                decoration.width, decoration.height
            );
            commands.insert_resource(decoration);
        }
    });
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

        let cur_pos = winit_window
            .outer_position()
            .unwrap_or(PhysicalPosition::new(0, 0));
        let win_size = winit_window.inner_size();
        let current = monitor_at(winit_window.available_monitors(), cur_pos.x, cur_pos.y);
        let Some(target_info) =
            monitor_by_index(winit_window.available_monitors(), target_monitor_index)
        else {
            info!("[Step1] Target monitor index {target_monitor_index} not found");
            return;
        };

        info!(
            "[Step1] Current: {}",
            format_info(current, cur_pos, (win_size.width, win_size.height))
        );
        info!(
            "[Step1] Target:  {}",
            format_info(
                Some(target_info.clone()),
                PhysicalPosition::new(saved_x, saved_y),
                (win_size.width, win_size.height)
            )
        );

        // Skip direct winit calls - let Step2 handle everything via Bevy API with scale_factor_override
        info!("[Step1] Skipping winit calls, Step2 will handle via Bevy API");

        // Store target info including scale factor
        commands.insert_resource(TargetPosition {
            x: saved_x,
            y: saved_y,
            width: state.width as u32,
            height: state.height as u32,
            entity: window_entity,
            target_scale: target_info.scale as f32,
        });
    });
}

/// Step 2 (Startup): Apply exact position now that window is on correct monitor
fn step2_apply_exact(
    target: Option<Res<TargetPosition>>,
    decoration: Option<Res<WindowDecoration>>,
    mut windows: Query<&mut Window>,
    _non_send: NonSendMarker,
) {
    let Some(target) = target else {
        info!("[Step2] No target position");
        return;
    };

    let final_pos = WINIT_WINDOWS.with(|ww| {
        let ww = ww.borrow();
        let Some(winit_window) = ww.get_window(target.entity) else {
            info!("[Step2] No winit window");
            return None;
        };

        let cur_pos = winit_window
            .outer_position()
            .unwrap_or(PhysicalPosition::new(0, 0));
        let win_size = winit_window.inner_size();
        let current = monitor_at(winit_window.available_monitors(), cur_pos.x, cur_pos.y);
        let target_mon = monitor_at(winit_window.available_monitors(), target.x, target.y);

        info!(
            "[Step2] Current: {}",
            format_info(current, cur_pos, (win_size.width, win_size.height))
        );
        info!(
            "[Step2] Target:  {}",
            format_info(
                target_mon.clone(),
                PhysicalPosition::new(target.x, target.y),
                (target.width, target.height)
            )
        );

        // Clamp position so window stays entirely within ONE monitor
        // (prevents issues when window would span monitors with different scale factors)
        let (final_x, final_y) = if let Some(ref mon) = target_mon {
            let win_right = target.x + target.width as i32;
            let win_bottom = target.y + target.height as i32;

            // Check if window edges would land on a different monitor (or outside all monitors)
            // Use the actual edge coordinates, not edge-1, to catch boundary cases
            let right_mon = monitor_at(winit_window.available_monitors(), win_right, target.y);
            let bottom_mon = monitor_at(winit_window.available_monitors(), target.x, win_bottom);

            let mut x = target.x;
            let mut y = target.y;

            // If right edge is on different monitor (or no monitor), move left
            if right_mon.as_ref().map(|m| m.index) != Some(mon.index) {
                if let Some(mon_handle) = winit_window.available_monitors().nth(mon.index) {
                    let mon_right = mon.position.x + mon_handle.size().width as i32;
                    x = (mon_right - target.width as i32).max(mon.position.x);
                }
            }

            // If bottom edge is on different monitor (or no monitor), move up
            if bottom_mon.as_ref().map(|m| m.index) != Some(mon.index) {
                if let Some(mon_handle) = winit_window.available_monitors().nth(mon.index) {
                    let mon_bottom = mon.position.y + mon_handle.size().height as i32;
                    y = (mon_bottom - target.height as i32).max(mon.position.y);
                }
            }

            (x, y)
        } else {
            (target.x, target.y)
        };

        if final_x != target.x || final_y != target.y {
            info!(
                "[Step2] Clamped position: ({}, {}) -> ({}, {})",
                target.x, target.y, final_x, final_y
            );
        }

        // Use decoration from resource (populated once at startup from winit)
        let (decoration_width, decoration_height) = decoration
            .as_ref()
            .map(|d| (d.width, d.height))
            .unwrap_or((0, 0));

        // target.width/height are OUTER size, convert to inner for Bevy resolution
        let inner_width = (target.width as u32).saturating_sub(decoration_width);
        let inner_height = (target.height as u32).saturating_sub(decoration_height);

        info!(
            "[Step2] Restoring size: outer={}x{}, decoration={}x{}, inner={}x{}, target_scale={}",
            target.width,
            target.height,
            decoration_width,
            decoration_height,
            inner_width,
            inner_height,
            target.target_scale
        );

        Some((final_x, final_y, inner_width, inner_height))
    });

    // Apply position and size via Bevy's Window component (no direct winit calls)
    if let Some((x, y, width, height)) = final_pos {
        if let Ok(mut window) = windows.get_mut(target.entity) {
            info!(
                "[Step2] Sending via Bevy: pos=({x}, {y}) size={width}x{height} scale_override={}",
                target.target_scale
            );
            // Set scale factor override to target monitor's scale BEFORE setting resolution
            window
                .resolution
                .set_scale_factor_override(Some(target.target_scale));
            window.resolution.set_physical_resolution(width, height);
            window.position = bevy::window::WindowPosition::At(IVec2::new(x, y));
        }
    }
}

/// Save window state when position or size changes
fn save_on_change(
    mut tracker: ResMut<WindowTracker>,
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

    // Get physical OUTER size from winit (includes title bar) for accurate boundary checks
    let physical_size = WINIT_WINDOWS.with(|ww| {
        let ww = ww.borrow();
        ww.get_window(entity).map(|w| w.outer_size())
    });
    let Some(size) = physical_size else { return };
    let current_size = (size.width as f32, size.height as f32);

    let changed = tracker.last_position != current_pos || tracker.last_size != Some(current_size);
    if !changed {
        return;
    }

    tracker.last_position = current_pos;
    tracker.last_size = Some(current_size);

    // Get position from winit if Bevy doesn't have it
    let pos = current_pos.unwrap_or_else(|| {
        WINIT_WINDOWS.with(|ww| {
            let ww = ww.borrow();
            ww.get_window(entity)
                .and_then(|w| w.outer_position().ok())
                .map(|p| IVec2::new(p.x, p.y))
                .unwrap_or(IVec2::ZERO)
        })
    });

    // Find monitor index
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
