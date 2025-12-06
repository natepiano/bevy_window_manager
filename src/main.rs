//! Test app for window positioning workaround on multi-DPI macOS setups.
//!
//! # The Problem
//!
//! On macOS with multiple monitors that have different scale factors (e.g., a Retina display
//! at scale 2.0 and an external monitor at scale 1.0), Bevy's window positioning has issues:
//!
//! 1. **`Window.position` is unreliable at startup**: When a window is created, `Window.position`
//!    is `Automatic` (not `At(pos)`), even though winit has placed the window at a specific
//!    physical position. This means we cannot determine which monitor the window is actually on
//!    by reading the Bevy `Window` component.
//!
//! 2. **Scale factor conversion in `changed_windows`**: When you modify `Window.resolution`,
//!    Bevy's `changed_windows` system (in `bevy_winit/src/system.rs`) applies scale factor
//!    conversion if `scale_factor != cached_scale_factor`. It converts physical→logical using
//!    the OLD cached scale, then logical→physical using the NEW scale. This corrupts the size
//!    when moving windows between monitors with different scale factors.
//!
//! 3. **Timing of scale factor updates**: The `CachedWindow` is updated after winit events are
//!    processed, but our systems run before we receive the `ScaleFactorChanged` event from the
//!    actual monitor transition. This creates a window where we set values with one scale but
//!    they get processed with another.
//!
//! # The Workaround
//!
//! We use `WinitInfo` to capture the actual window position from winit at startup (via
//! `winit_window.outer_position()`), which tells us the real monitor the window is on.
//! This lets us calculate the correct `starting_scale` for compensation.
//!
//! When restoring a window to a different monitor:
//! - `starting_scale`: The scale factor of the monitor the window is ACTUALLY on (from winit)
//! - `target_scale`: The scale factor of the monitor we want to move to
//! - `scale_ratio = starting_scale / target_scale`
//!
//! We pre-multiply the size by `scale_ratio` to counteract the conversion that `changed_windows`
//! will apply. For example, going from scale 2→1, ratio=2, so we double the size knowing it
//! will be halved.
//!
//! # Future Bevy Improvements
//!
//! Ideally, Bevy should:
//! - Populate `Window.position` with the actual position from winit after window creation
//! - Provide a way to set physical resolution without scale factor conversion
//! - Or expose the actual monitor the window is on as a component/resource

use std::fs;
use std::path::PathBuf;

use bevy::ecs::system::NonSendMarker;
use bevy::prelude::*;
use bevy::window::Monitor;
use bevy::window::WindowMoved;
use bevy::window::WindowResized;
use bevy::window::WindowScaleFactorChanged;
use bevy::winit::WINIT_WINDOWS;
use serde::Deserialize;
use serde::Serialize;

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
                ..default()
            }),
            ..default()
        }))
        .add_systems(PreStartup, (init_winit_info, step1_move_to_monitor).chain())
        .add_systems(Startup, step2_apply_exact)
        .add_systems(Update, handle_window_events)
        .run();
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WindowState {
    position: Option<(i32, i32)>,
    width: f32,
    height: f32,
    monitor_index: usize,
}

/// Window decoration dimensions (title bar, borders)
struct WindowDecoration {
    width: u32,
    height: u32,
}

/// Information from winit captured at startup
#[derive(Resource)]
struct WinitInfo {
    starting_monitor_index: usize,
    window_decoration: WindowDecoration,
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
    /// Scale of the monitor the window is PHYSICALLY on at startup (before any moves)
    starting_scale: f32,
}

#[derive(Clone)]
struct MonitorInfo {
    index: usize,
    name: String,
    scale: f64,
    position: IVec2,
    size: UVec2,
}

impl MonitorInfo {
    fn format_with_window(&self, win_pos: IVec2, win_size: (u32, u32)) -> String {
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

fn format_info(mon: Option<MonitorInfo>, win_pos: IVec2, win_size: (u32, u32)) -> String {
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

/// Get sort key for monitor (primary at 0,0 first, then by position)
fn monitor_sort_key(mon: &Monitor) -> (bool, i32, i32) {
    let pos = mon.physical_position;
    let is_primary = pos.x == 0 && pos.y == 0;
    (!is_primary, pos.x, pos.y)
}

/// Helper to get monitor info at a position from Bevy Monitor components
fn monitor_at(monitors: &[(Entity, &Monitor)], x: i32, y: i32) -> Option<MonitorInfo> {
    // Sort monitors consistently (primary first)
    let mut sorted: Vec<_> = monitors.iter().collect();
    sorted.sort_by_key(|(_, mon)| monitor_sort_key(mon));

    sorted.iter().enumerate().find_map(|(idx, (_, mon))| {
        let pos = mon.physical_position;
        let size = mon.physical_size();
        if x >= pos.x && x < pos.x + size.x as i32 && y >= pos.y && y < pos.y + size.y as i32 {
            Some(MonitorInfo {
                index: idx,
                name: mon.name.clone().unwrap_or_else(|| "?".to_string()),
                scale: mon.scale_factor,
                position: pos,
                size,
            })
        } else {
            None
        }
    })
}

/// Helper to get monitor info by index from Bevy Monitor components
fn monitor_by_index(monitors: &[(Entity, &Monitor)], index: usize) -> Option<MonitorInfo> {
    let mut sorted: Vec<_> = monitors.iter().collect();
    sorted.sort_by_key(|(_, mon)| monitor_sort_key(mon));

    sorted.get(index).map(|(_, mon)| MonitorInfo {
        index,
        name: mon.name.clone().unwrap_or_else(|| "?".to_string()),
        scale: mon.scale_factor,
        position: mon.physical_position,
        size: mon.physical_size(),
    })
}

/// Infer monitor index when position is outside all monitor bounds.
/// Uses Y coordinate: negative Y = secondary monitor (index 1), else primary (index 0).
fn infer_monitor_index(y: i32) -> usize {
    if y < 0 { 1 } else { 0 }
}

/// Populate WinitInfo resource from winit (decoration and starting monitor)
fn init_winit_info(
    mut commands: Commands,
    windows: Query<Entity, With<Window>>,
    monitors: Query<(Entity, &Monitor)>,
    _non_send: NonSendMarker,
) {
    let Ok(entity) = windows.single() else {
        return;
    };

    let monitors_vec: Vec<_> = monitors.iter().collect();

    WINIT_WINDOWS.with(|ww| {
        let ww = ww.borrow();
        if let Some(winit_window) = ww.get_window(entity) {
            let outer = winit_window.outer_size();
            let inner = winit_window.inner_size();
            let decoration = WindowDecoration {
                width: outer.width.saturating_sub(inner.width),
                height: outer.height.saturating_sub(inner.height),
            };

            // Get actual position from winit to determine starting monitor
            let pos = winit_window
                .outer_position()
                .map(|p| IVec2::new(p.x, p.y))
                .unwrap_or(IVec2::ZERO);

            let starting_monitor_index = monitor_at(&monitors_vec, pos.x, pos.y)
                .map(|m| m.index)
                .unwrap_or(0);

            info!(
                "[Init] decoration={}x{} pos=({}, {}) starting_monitor={}",
                decoration.width, decoration.height, pos.x, pos.y, starting_monitor_index
            );

            commands.insert_resource(WinitInfo {
                starting_monitor_index,
                window_decoration: decoration,
            });
        }
    });
}

/// Step 1 (PreStartup): Move window to target monitor at 1x1 size
fn step1_move_to_monitor(
    mut commands: Commands,
    mut windows: Query<(Entity, &mut Window)>,
    monitors: Query<(Entity, &Monitor)>,
    winit_info: Option<Res<WinitInfo>>,
) {
    let Some(state) = load_state() else {
        info!("[Step1] No saved state");
        return;
    };

    let Some((saved_x, saved_y)) = state.position else {
        info!("[Step1] No saved position");
        return;
    };

    let target_monitor_index = state.monitor_index;

    let Ok((window_entity, mut window)) = windows.single_mut() else {
        info!("[Step1] No window");
        return;
    };

    let monitors_vec: Vec<_> = monitors.iter().collect();

    // Get starting monitor from WinitInfo (actual position from winit)
    let starting_monitor_index = winit_info
        .as_ref()
        .map(|w| w.starting_monitor_index)
        .unwrap_or(0);
    let starting_info = monitor_by_index(&monitors_vec, starting_monitor_index);
    let starting_scale = starting_info
        .as_ref()
        .map(|m| m.scale as f32)
        .unwrap_or(1.0);

    let Some(target_info) = monitor_by_index(&monitors_vec, target_monitor_index) else {
        info!("[Step1] Target monitor index {target_monitor_index} not found");
        return;
    };

    info!(
        "[Step1] Starting monitor={} scale={}, Target monitor={} scale={}",
        starting_monitor_index, starting_scale, target_monitor_index, target_info.scale
    );

    // Move window to center of target monitor at 1x1 size
    // This triggers the scale factor change before Step2 applies final size
    let center_x = target_info.position.x + (target_info.size.x as i32 / 2);
    let center_y = target_info.position.y + (target_info.size.y as i32 / 2);

    info!(
        "[Step1] Moving to monitor center ({}, {}) at 1x1 size",
        center_x, center_y
    );

    window.position = bevy::window::WindowPosition::At(IVec2::new(center_x, center_y));
    window.resolution.set_physical_resolution(1, 1);

    // Store target info for restore
    commands.insert_resource(TargetPosition {
        x: saved_x,
        y: saved_y,
        width: state.width as u32,
        height: state.height as u32,
        entity: window_entity,
        target_scale: target_info.scale as f32,
        starting_scale,
    });
}

/// Step 2 (Startup): No-op, actual positioning happens in handle_window_events
/// after window has moved to target monitor
fn step2_apply_exact(target: Option<Res<TargetPosition>>) {
    if target.is_some() {
        info!("[Step2] Target stored, will apply after move completes");
    } else {
        info!("[Step2] No target position");
    }
}

/// Handle window events - apply pending restore or save state
fn handle_window_events(
    mut commands: Commands,
    mut moved_events: MessageReader<WindowMoved>,
    mut resized_events: MessageReader<WindowResized>,
    mut scale_events: MessageReader<WindowScaleFactorChanged>,
    target: Option<Res<TargetPosition>>,
    winit_info: Option<Res<WinitInfo>>,
    mut windows: Query<&mut Window>,
    monitors: Query<(Entity, &Monitor)>,
) {
    let (decoration_width, decoration_height) = winit_info
        .as_ref()
        .map(|w| (w.window_decoration.width, w.window_decoration.height))
        .unwrap_or((0, 0));

    // Drain events to clear queue
    let last_scale: Option<_> = scale_events.read().last().cloned();
    let last_move: Option<_> = moved_events.read().last().cloned();
    let last_resize: Option<_> = resized_events.read().last().cloned();

    // If we have a pending restore, check if we can apply it
    if let Some(target) = target {
        let monitors_vec: Vec<_> = monitors.iter().collect();

        // Check current monitor via last move event or window position
        let current_pos = last_move.as_ref().map(|e| e.position).or_else(|| {
            windows
                .get(target.entity)
                .ok()
                .and_then(|w| match w.position {
                    bevy::window::WindowPosition::At(p) => Some(p),
                    _ => None,
                })
        });

        let Some(pos) = current_pos else {
            return;
        };

        let current_mon = monitor_at(&monitors_vec, pos.x, pos.y);
        let current_scale = current_mon.as_ref().map(|m| m.scale as f32).unwrap_or(1.0);

        info!(
            "[Restore] pos=({}, {}) current_scale={} target_scale={}",
            pos.x, pos.y, current_scale, target.target_scale
        );

        // If we're on the target monitor (scales match), apply final position/size
        if (current_scale - target.target_scale).abs() < 0.01 {
            info!("[Restore] On target monitor, applying final position/size");

            let inner_width = target.width.saturating_sub(decoration_width);
            let inner_height = target.height.saturating_sub(decoration_height);

            // Clamp position to keep window fully within target monitor
            // (programmatic positioning can't restore cross-monitor overlapping positions)
            let target_mon = monitor_at(&monitors_vec, target.x, target.y);
            let (final_x, final_y) = if let Some(ref mon) = target_mon {
                let mon_right = mon.position.x + mon.size.x as i32;
                let mon_bottom = mon.position.y + mon.size.y as i32;

                let mut x = target.x;
                let mut y = target.y;

                // Clamp right edge
                if x + target.width as i32 > mon_right {
                    x = mon_right - target.width as i32;
                }
                // Clamp bottom edge
                if y + target.height as i32 > mon_bottom {
                    y = mon_bottom - target.height as i32;
                }
                // Ensure we don't go past left/top edges after clamping
                x = x.max(mon.position.x);
                y = y.max(mon.position.y);

                if x != target.x || y != target.y {
                    info!(
                        "[Restore] Clamped position: ({}, {}) -> ({}, {}) to fit monitor",
                        target.x, target.y, x, y
                    );
                }

                (x, y)
            } else {
                (target.x, target.y)
            };

            if let Ok(mut window) = windows.get_mut(target.entity) {
                // Compensate for scale factor conversion in changed_windows
                // 2→1: ratio=2, size/pos doubled then halved back
                // 1→2: ratio=0.5, size/pos halved then doubled back
                // Same: ratio=1, no change
                let scale_ratio = target.starting_scale / target.target_scale;

                let comp_x = (final_x as f32 * scale_ratio) as i32;
                let comp_y = (final_y as f32 * scale_ratio) as i32;
                let comp_width = (inner_width as f32 * scale_ratio) as u32;
                let comp_height = (inner_height as f32 * scale_ratio) as u32;

                window.position = bevy::window::WindowPosition::At(IVec2::new(comp_x, comp_y));
                window
                    .resolution
                    .set_physical_resolution(comp_width, comp_height);

                info!(
                    "[Restore] Applied: pos=({}, {}) inner_size={}x{} (compensated from {}x{}, ratio={})",
                    final_x,
                    final_y,
                    comp_width,
                    comp_height,
                    inner_width,
                    inner_height,
                    scale_ratio
                );
            }

            commands.remove_resource::<TargetPosition>();
        }
        return; // Don't save during restore
    }

    // Save state on events (only when not restoring)
    let Ok(window) = windows.single() else {
        return;
    };

    if let Some(event) = last_scale {
        info!("[Event:ScaleChanged] scale={}", event.scale_factor);
    }

    let last_move_pos = last_move.as_ref().map(|m| m.position);

    if let Some(event) = last_move {
        let monitors_vec: Vec<_> = monitors.iter().collect();
        let monitor_index = monitor_at(&monitors_vec, event.position.x, event.position.y)
            .map(|m| m.index)
            .unwrap_or_else(|| infer_monitor_index(event.position.y));

        let outer_width = window.resolution.physical_width() + decoration_width;
        let outer_height = window.resolution.physical_height() + decoration_height;

        info!(
            "[Event:Moved] pos=({}, {}) size={}x{} monitor={}",
            event.position.x, event.position.y, outer_width, outer_height, monitor_index
        );

        let state = WindowState {
            position: Some((event.position.x, event.position.y)),
            width: outer_width as f32,
            height: outer_height as f32,
            monitor_index,
        };
        save_state(&state);
    }

    if let Some(event) = last_resize {
        let scale = window.scale_factor();
        let physical_width = (event.width * scale) as u32 + decoration_width;
        let physical_height = (event.height * scale) as u32 + decoration_height;

        // Use position from move event if available, otherwise from window component
        let pos = last_move_pos.or_else(|| match window.position {
            bevy::window::WindowPosition::At(p) => Some(p),
            _ => None,
        });

        let Some(pos) = pos else {
            // Can't determine position, skip saving
            info!(
                "[Event:Resized] logical={}x{} - skipping save, position unknown",
                event.width, event.height
            );
            return;
        };

        let monitors_vec: Vec<_> = monitors.iter().collect();
        let monitor_index = monitor_at(&monitors_vec, pos.x, pos.y)
            .map(|m| m.index)
            .unwrap_or_else(|| infer_monitor_index(pos.y));

        info!(
            "[Event:Resized] logical={}x{} physical_outer={}x{} pos=({}, {}) monitor={} scale={}",
            event.width,
            event.height,
            physical_width,
            physical_height,
            pos.x,
            pos.y,
            monitor_index,
            scale
        );

        let state = WindowState {
            position: Some((pos.x, pos.y)),
            width: physical_width as f32,
            height: physical_height as f32,
            monitor_index,
        };
        save_state(&state);
    }
}
