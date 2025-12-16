//! Interactive example for testing window restoration and fullscreen modes.
//!
//! Run with: `cargo run --example restore_window`
//!
//! This example displays the current window state and allows switching between modes:
//! - Press `1` for exclusive fullscreen (uses selected video mode) WARNING: Exclusive fullscreen on
//!   macOS may panic on exit due to winit bugs. See: <https://github.com/rust-windowing/winit/issues/3668>
//! - Press `2` for borderless fullscreen (recommended on macOS)
//! - Press `W` or `Escape` for windowed mode
//! - Press `Up`/`Down` to cycle through available video modes
//!
//! Move and resize the window to test state persistence across restarts.

// Monitor dimensions always fit in i32
#![allow(clippy::cast_possible_wrap)]

use std::collections::HashMap;

use bevy::ecs::system::NonSendMarker;
use bevy::prelude::*;
use bevy::window::Monitor;
use bevy::window::MonitorSelection;
use bevy::window::PrimaryWindow;
use bevy::window::VideoMode;
use bevy::window::VideoModeSelection;
use bevy::window::WindowMode;
use bevy::window::WindowPosition;
use bevy::winit::WINIT_WINDOWS;
use bevy_brp_extras::BrpExtrasPlugin;
use bevy_window_manager::Monitors;
use bevy_window_manager::WindowExt;
use bevy_window_manager::WindowManagerPlugin;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Window Restore Test".into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(WindowManagerPlugin)
        .add_plugins(BrpExtrasPlugin::default())
        .init_resource::<SelectedVideoModes>()
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                update_display,
                handle_input,
                debug_winit_monitor,
                debug_window_changed,
            ),
        )
        .run();
}

// --- Display UI ---

#[derive(Component)]
struct MainDisplay;

const MARGIN: Val = Val::Px(20.0);

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);

    let text_font = TextFont {
        font_size: 20.0,
        ..default()
    };

    // Single text display for everything
    commands.spawn((
        Text::new(""),
        text_font,
        Node {
            position_type: PositionType::Absolute,
            top: MARGIN,
            left: MARGIN,
            ..default()
        },
        MainDisplay,
    ));
}

fn update_display(
    mut main_text: Single<&mut Text, With<MainDisplay>>,
    window: Single<&Window, With<PrimaryWindow>>,
    monitors_res: Res<Monitors>,
    bevy_monitors: Query<(Entity, &Monitor)>,
    mut selected: ResMut<SelectedVideoModes>,
) {
    let monitor = window.monitor(&monitors_res);
    let effective_mode = window.effective_mode(&monitors_res);

    // Get video modes and refresh rate for current monitor by matching position
    let (video_modes, refresh_rate): (Vec<&VideoMode>, Option<u32>) = bevy_monitors
        .iter()
        .find(|(_, m)| m.physical_position == monitor.position)
        .map(|(_, m)| {
            (
                m.video_modes.iter().collect(),
                m.refresh_rate_millihertz.map(|r| r / 1000),
            )
        })
        .unwrap_or_default();

    // Show active refresh rate - from video mode if in exclusive fullscreen, otherwise from monitor
    let active_refresh = match &window.mode {
        WindowMode::Fullscreen(_, VideoModeSelection::Specific(mode)) => {
            Some(mode.refresh_rate_millihertz / 1000)
        },
        _ => refresh_rate,
    };
    let refresh_display = active_refresh.map_or_else(|| "N/A".into(), |hz| format!("{hz}Hz"));

    // Find the active video mode index if in exclusive fullscreen
    let active_mode_idx = match &window.mode {
        WindowMode::Fullscreen(_, VideoModeSelection::Specific(active)) => {
            video_modes.iter().position(|m| {
                m.physical_size == active.physical_size
                    && m.refresh_rate_millihertz == active.refresh_rate_millihertz
            })
        },
        _ => None,
    };

    // Sync selected index to active video mode only when mode changes (including startup)
    if let WindowMode::Fullscreen(_, VideoModeSelection::Specific(active)) = &window.mode {
        let current_mode = (active.physical_size, active.refresh_rate_millihertz);
        if selected.last_sync != Some(current_mode) {
            // Only mark as synced if we actually found the active mode
            if let Some(active_idx) = active_mode_idx {
                selected.set(monitor.index, active_idx);
                selected.last_sync = Some(current_mode);
            }
        }
    } else {
        selected.last_sync = None;
    }

    // Build video modes display (show 5 modes, centered appropriately)
    let selected_idx = selected.get(monitor.index);
    let video_modes_display =
        build_video_modes_display(&video_modes, selected_idx, active_mode_idx);

    // Row 1: Monitor info all on one line
    let primary_marker = if monitor.index == 0 {
        " Primary Monitor -"
    } else {
        " -"
    };
    let row1 = format!(
        "Monitor: {}{primary_marker} Scale: {} - Refresh Rate: {refresh_display}",
        monitor.index, monitor.scale,
    );

    // Format aligned position strings based on window position variant
    let (monitor_pos, window_pos) = match window.position {
        WindowPosition::At(pos) => format_aligned_pair(
            (monitor.position.x, monitor.position.y),
            (pos.x, pos.y),
            Delimiter::Parens,
        ),
        WindowPosition::Automatic => {
            let monitor_pos = format!("({}, {})", monitor.position.x, monitor.position.y);
            (monitor_pos, "Automatic".to_string())
        },
        WindowPosition::Centered(mon_sel) => {
            let monitor_pos = format!("({}, {})", monitor.position.x, monitor.position.y);
            (monitor_pos, format!("Centered({mon_sel:?})"))
        },
    };

    let (monitor_size, window_size) = format_aligned_pair(
        (monitor.size.x as i32, monitor.size.y as i32),
        (
            window.physical_width() as i32,
            window.physical_height() as i32,
        ),
        Delimiter::None,
    );

    // Row 2: Monitor position and size
    let row2 = format!("Monitor Position: {monitor_pos} - Size: {monitor_size}");

    // Row 3: Window position and size (aligned with row 2)
    let row3 = format!("Window  Position: {window_pos} - Size: {window_size}");

    // Update main display
    main_text.0 = format!(
        "{row1}\n\
         {row2}\n\
         {row3}\n\
         \n\
         Mode:           {:?} (set value only, not dynamically updated)\n\
         Effective Mode: {:?}\n\
         \n\
         Video Modes (Up/Down to select):\n\
         {video_modes_display}\n\
         \n\
         Controls:\n\
         [1] Exclusive Fullscreen (selected mode)\n\
         [2] Borderless Fullscreen\n\
         [W/Esc] Windowed\n\
         [Q] Quit",
        window.mode, effective_mode,
    );
}

// --- Input Handling ---

fn handle_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut window: Single<&mut Window, With<PrimaryWindow>>,
    bevy_monitors: Query<(Entity, &Monitor)>,
    monitors_res: Res<Monitors>,
    mut selected: ResMut<SelectedVideoModes>,
) {
    let monitor = window.monitor(&monitors_res);

    // Get video modes for current monitor by matching position
    let video_modes: Vec<VideoMode> = bevy_monitors
        .iter()
        .find(|(_, m)| m.physical_position == monitor.position)
        .map(|(_, m)| m.video_modes.clone())
        .unwrap_or_default();

    // Navigate video modes (per monitor)
    let current_idx = selected.get(monitor.index);
    if keys.just_pressed(KeyCode::ArrowUp) && current_idx > 0 {
        selected.set(monitor.index, current_idx - 1);
    }
    if keys.just_pressed(KeyCode::ArrowDown) && current_idx < video_modes.len().saturating_sub(1) {
        selected.set(monitor.index, current_idx + 1);
    }

    if keys.just_pressed(KeyCode::Digit1) {
        let selected_idx = selected
            .get(monitor.index)
            .min(video_modes.len().saturating_sub(1));
        let video_mode_selection = video_modes
            .get(selected_idx)
            .map_or(VideoModeSelection::Current, |mode| {
                VideoModeSelection::Specific(*mode)
            });

        window.mode =
            WindowMode::Fullscreen(MonitorSelection::Index(monitor.index), video_mode_selection);
    }
    if keys.just_pressed(KeyCode::Digit2) {
        window.mode = WindowMode::BorderlessFullscreen(MonitorSelection::Index(monitor.index));
    }
    if keys.just_pressed(KeyCode::KeyW) || keys.just_pressed(KeyCode::Escape) {
        window.mode = WindowMode::Windowed;
    }
    if keys.just_pressed(KeyCode::KeyQ) {
        std::process::exit(0);
    }
}

// --- Video Mode Selection State ---

/// Tracks the selected video mode index per monitor for exclusive fullscreen.
#[derive(Resource, Default)]
struct SelectedVideoModes {
    /// Selected index per monitor (keyed by monitor index).
    indices:   HashMap<usize, usize>,
    /// Track last synced mode to avoid overriding user selection.
    last_sync: Option<(UVec2, u32)>,
}

impl SelectedVideoModes {
    fn get(&self, monitor_index: usize) -> usize {
        self.indices.get(&monitor_index).copied().unwrap_or(0)
    }

    fn set(&mut self, monitor_index: usize, index: usize) {
        self.indices.insert(monitor_index, index);
    }
}

// --- Formatting Helpers ---

enum Delimiter {
    Parens,
    None,
}

/// Formats two pairs of values (monitor and window) with right-aligned numbers.
/// Returns (`monitor_str`, `window_str`) with matching widths.
fn format_aligned_pair(
    monitor_vals: (i32, i32),
    window_vals: (i32, i32),
    delimiter: Delimiter,
) -> (String, String) {
    let (m1, m2) = monitor_vals;
    let (w1, w2) = window_vals;

    // Find max width for each column
    let width1 = m1.to_string().len().max(w1.to_string().len());
    let width2 = m2.to_string().len().max(w2.to_string().len());

    match delimiter {
        Delimiter::Parens => (
            format!("({m1:>width1$}, {m2:>width2$})"),
            format!("({w1:>width1$}, {w2:>width2$})"),
        ),
        Delimiter::None => (
            format!("{m1:>width1$}x{m2:>width2$}"),
            format!("{w1:>width1$}x{w2:>width2$}"),
        ),
    }
}

/// Builds the video modes display string showing a scrollable window of modes.
fn build_video_modes_display(
    video_modes: &[&VideoMode],
    selected_idx: usize,
    active_mode_idx: Option<usize>,
) -> String {
    if video_modes.is_empty() {
        return "  (no video modes available)".into();
    }

    let selected_idx = selected_idx.min(video_modes.len().saturating_sub(1));
    let len = video_modes.len();

    // Determine the visible window start position
    let start = if len <= 5 {
        // Show all modes if 5 or fewer
        0
    } else {
        // Center on active mode (slot 3 of 5) if it exists, otherwise center on selected
        let center_target = active_mode_idx.unwrap_or(selected_idx);

        // But always ensure selected is visible by adjusting if needed
        let ideal_start = center_target.saturating_sub(2);
        let ideal_end = ideal_start + 5;

        // Check if selected would be outside the ideal window
        if selected_idx < ideal_start {
            // Selected is above the window, scroll up to show it
            selected_idx.saturating_sub(2)
        } else if selected_idx >= ideal_end {
            // Selected is below the window, scroll down to show it
            (selected_idx + 3).saturating_sub(5)
        } else {
            // Selected is visible, use the ideal centering on active
            ideal_start
        }
        .min(len.saturating_sub(5))
    };
    let end = (start + 5).min(len);

    video_modes[start..end]
        .iter()
        .enumerate()
        .map(|(i, mode)| {
            let actual_idx = start + i;
            let left_marker = if actual_idx == selected_idx { ">" } else { " " };
            let right_marker = if Some(actual_idx) == active_mode_idx {
                " <- active"
            } else {
                ""
            };
            format!(
                "  {left_marker} {}x{} @ {}Hz{right_marker}",
                mode.physical_size.x,
                mode.physical_size.y,
                mode.refresh_rate_millihertz / 1000
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// --- Debug: Winit Monitor Detection ---

/// Debug system that runs every frame and logs winit-detected monitor changes.
fn debug_winit_monitor(
    window: Single<Entity, With<PrimaryWindow>>,
    monitors: Res<Monitors>,
    mut cached_monitor: Local<Option<usize>>,
    _non_send: NonSendMarker,
) {
    let window_entity = *window;

    let winit_monitor_index: Option<usize> = WINIT_WINDOWS.with(|ww| {
        let ww = ww.borrow();
        ww.get_window(window_entity).and_then(|winit_window| {
            winit_window.current_monitor().and_then(|current_monitor| {
                let pos = current_monitor.position();
                monitors.at(pos.x, pos.y).map(|mon| mon.index)
            })
        })
    });

    if *cached_monitor != winit_monitor_index {
        info!(
            "[debug_winit_monitor] Monitor changed: {:?} -> {:?}",
            *cached_monitor, winit_monitor_index
        );
        *cached_monitor = winit_monitor_index;
    }
}

/// Cached state for detecting what changed in Window component.
#[derive(Default)]
struct CachedWindowDebug {
    position: Option<WindowPosition>,
    width:    u32,
    height:   u32,
    mode:     Option<WindowMode>,
    focused:  bool,
}

/// Debug system that logs when Changed<Window> fires and what changed.
fn debug_window_changed(
    window: Single<&Window, (With<PrimaryWindow>, Changed<Window>)>,
    mut cached: Local<CachedWindowDebug>,
) {
    let w = *window;

    let position_changed = cached.position.as_ref() != Some(&w.position);
    let size_changed = cached.width != w.physical_width() || cached.height != w.physical_height();
    let mode_changed = cached.mode.as_ref() != Some(&w.mode);
    let focused_changed = cached.focused != w.focused;

    let mut changes = Vec::new();
    if position_changed {
        changes.push(format!(
            "position: {:?} -> {:?}",
            cached.position, w.position
        ));
    }
    if size_changed {
        changes.push(format!(
            "size: {}x{} -> {}x{}",
            cached.width,
            cached.height,
            w.physical_width(),
            w.physical_height()
        ));
    }
    if mode_changed {
        changes.push(format!("mode: {:?} -> {:?}", cached.mode, w.mode));
    }
    if focused_changed {
        changes.push(format!("focused: {} -> {}", cached.focused, w.focused));
    }

    if !changes.is_empty() {
        info!("[debug_window_changed] {}", changes.join(", "));
    }

    // Update cache
    cached.position = Some(w.position.clone());
    cached.width = w.physical_width();
    cached.height = w.physical_height();
    cached.mode = Some(w.mode.clone());
    cached.focused = w.focused;
}
