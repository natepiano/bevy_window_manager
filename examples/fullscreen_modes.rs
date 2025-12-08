//! Interactive example for testing fullscreen mode detection and restoration.
//!
//! Run with: `cargo run --example fullscreen_modes`
//!
//! This example displays the current window mode and allows switching between modes:
//! - Press `1` for exclusive fullscreen (uses selected video mode) WARNING: Exclusive fullscreen on
//!   macOS may panic on exit due to winit bugs. See: https://github.com/rust-windowing/winit/issues/3668
//! - Press `2` for borderless fullscreen (recommended on macOS)
//! - Press `W` or `Escape` for windowed mode
//! - Press `Up`/`Down` to cycle through available video modes
//!
//! The detected mode (what would be saved) and Bevy's `window.mode` are displayed,
//! allowing you to verify the detection logic works correctly.

// Monitor dimensions always fit in i32
#![allow(clippy::cast_possible_wrap)]

use bevy::prelude::*;
use bevy::window::Monitor;
use bevy::window::MonitorSelection;
use bevy::window::PrimaryWindow;
use bevy::window::VideoMode;
use bevy::window::VideoModeSelection;
use bevy::window::WindowMode;
use bevy_restore_windows::Monitors;
use bevy_restore_windows::RestoreWindowsPlugin;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Fullscreen Modes Test".into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(RestoreWindowsPlugin)
        .init_resource::<SelectedVideoMode>()
        .add_systems(Startup, setup)
        .add_systems(Update, (update_display, handle_input))
        .run();
}

/// Tracks the selected video mode index for exclusive fullscreen.
#[derive(Resource, Default)]
struct SelectedVideoMode {
    index: usize,
}

#[derive(Component)]
struct ModeDisplay;

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
    commands.spawn((
        Text::new(""),
        TextFont {
            font_size: 20.0,
            ..default()
        },
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(20.0),
            left: Val::Px(20.0),
            ..default()
        },
        ModeDisplay,
    ));
}

fn update_display(
    mut query: Query<&mut Text, With<ModeDisplay>>,
    window: Single<&Window, With<PrimaryWindow>>,
    monitors_res: Res<Monitors>,
    bevy_monitors: Query<(Entity, &Monitor)>,
    selected: Res<SelectedVideoMode>,
) {
    let Ok(mut text) = query.single_mut() else {
        return;
    };

    // Get window position
    let pos = match window.position {
        bevy::window::WindowPosition::At(p) => p,
        _ => IVec2::ZERO,
    };

    // Determine current monitor - use MonitorSelection from fullscreen mode if available
    let current_monitor_index = get_current_monitor_index(&window, pos, &monitors_res);

    // Detect effective mode which may differ from Bevy's window.mode in the case of
    // BorderlessFullscreen
    let detected = monitors_res.detect_effective_mode(&window);

    // Get Bevy's window.mode
    let bevy_mode = format!("{:?}", window.mode);

    // Get monitor info
    let monitor_info = monitors_res.by_index(current_monitor_index).map_or_else(
        || "Unknown".into(),
        |m| {
            format!(
                "index={} size={}x{} scale={}",
                m.index, m.size.x, m.size.y, m.scale
            )
        },
    );

    // Get video modes and refresh rate for current monitor by matching position
    let current_monitor_pos = monitors_res
        .by_index(current_monitor_index)
        .map(|m| m.position);

    let (video_modes, refresh_rate): (Vec<&VideoMode>, Option<u32>) = current_monitor_pos
        .and_then(|target_pos| {
            bevy_monitors
                .iter()
                .find(|(_, m)| m.physical_position == target_pos)
                .map(|(_, m)| {
                    (
                        m.video_modes.iter().collect(),
                        m.refresh_rate_millihertz.map(|r| r / 1000),
                    )
                })
        })
        .unwrap_or_default();

    // Show active refresh rate - from video mode if in exclusive fullscreen, otherwise from monitor
    let active_refresh = match &window.mode {
        WindowMode::Fullscreen(_, VideoModeSelection::Specific(mode)) => {
            Some(mode.refresh_rate_millihertz / 1000)
        },
        _ => refresh_rate,
    };
    let refresh_info =
        active_refresh.map_or_else(String::new, |hz| format!("Refresh rate: {hz}Hz"));

    // Build video modes display (show a few around selected)
    let video_modes_display = if video_modes.is_empty() {
        "  (no video modes available)".into()
    } else {
        let selected_idx = selected.index.min(video_modes.len().saturating_sub(1));
        let start = selected_idx.saturating_sub(2);
        let end = (start + 5).min(video_modes.len());

        video_modes[start..end]
            .iter()
            .enumerate()
            .map(|(i, mode)| {
                let actual_idx = start + i;
                let marker = if actual_idx == selected_idx { ">" } else { " " };
                format!(
                    "  {marker} {}x{} @ {}Hz",
                    mode.physical_size.x,
                    mode.physical_size.y,
                    mode.refresh_rate_millihertz / 1000
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    text.0 = format!(
        "Detected mode: {detected:?}\n\
         Bevy window.mode: {bevy_mode}\n\
         \n\
         Window pos: ({}, {})\n\
         Window size: {}x{}\n\
         Monitor: {monitor_info}\n\
         {refresh_info}\n\
         \n\
         Video Modes (Up/Down to select):\n\
         {video_modes_display}\n\
         \n\
         Controls:\n\
         [1] Exclusive Fullscreen (selected mode)\n\
         [2] Borderless Fullscreen\n\
         [W/Esc] Windowed",
        pos.x,
        pos.y,
        window.physical_width(),
        window.physical_height(),
    );
}

fn handle_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut window: Single<&mut Window, With<PrimaryWindow>>,
    bevy_monitors: Query<(Entity, &Monitor)>,
    monitors_res: Res<Monitors>,
    mut selected: ResMut<SelectedVideoMode>,
) {
    // Get window position
    let pos = match window.position {
        bevy::window::WindowPosition::At(p) => p,
        _ => IVec2::ZERO,
    };

    // Determine current monitor - use MonitorSelection from fullscreen mode if available
    let current_monitor_index = get_current_monitor_index(&window, pos, &monitors_res);

    // Get video modes for current monitor by matching position
    let current_monitor_pos = monitors_res
        .by_index(current_monitor_index)
        .map(|m| m.position);

    let video_modes: Vec<VideoMode> = current_monitor_pos
        .and_then(|target_pos| {
            bevy_monitors
                .iter()
                .find(|(_, m)| m.physical_position == target_pos)
                .map(|(_, m)| m.video_modes.clone())
        })
        .unwrap_or_default();

    // Navigate video modes
    if keys.just_pressed(KeyCode::ArrowUp) && selected.index > 0 {
        selected.index -= 1;
    }
    if keys.just_pressed(KeyCode::ArrowDown) && selected.index < video_modes.len().saturating_sub(1)
    {
        selected.index += 1;
    }

    if keys.just_pressed(KeyCode::Digit1) {
        let selected_idx = selected.index.min(video_modes.len().saturating_sub(1));
        let video_mode_selection = video_modes
            .get(selected_idx)
            .map_or(VideoModeSelection::Current, |mode| {
                VideoModeSelection::Specific(*mode)
            });

        window.mode = WindowMode::Fullscreen(
            MonitorSelection::Index(current_monitor_index),
            video_mode_selection,
        );
    }
    if keys.just_pressed(KeyCode::Digit2) {
        window.mode =
            WindowMode::BorderlessFullscreen(MonitorSelection::Index(current_monitor_index));
    }
    if keys.just_pressed(KeyCode::KeyW) || keys.just_pressed(KeyCode::Escape) {
        window.mode = WindowMode::Windowed;
    }
}

/// Get the current monitor index, using the `MonitorSelection` from fullscreen mode if available.
fn get_current_monitor_index(window: &Window, pos: IVec2, monitors: &Monitors) -> usize {
    // If in fullscreen, extract the monitor index from the MonitorSelection
    match &window.mode {
        WindowMode::Fullscreen(selection, _) | WindowMode::BorderlessFullscreen(selection) => {
            match selection {
                MonitorSelection::Index(idx) => return *idx,
                MonitorSelection::Primary => return 0,
                MonitorSelection::Current | MonitorSelection::Entity(_) => {
                    // Fall through to position-based lookup
                },
            }
        },
        WindowMode::Windowed => {},
    }

    // Fall back to position-based lookup
    monitors
        .at(pos.x, pos.y)
        .map_or_else(|| monitors.infer_index(pos.x, pos.y), |m| m.index)
}
