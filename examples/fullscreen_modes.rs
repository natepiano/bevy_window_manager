//! Interactive example for testing fullscreen mode detection and restoration.
//!
//! Run with: `cargo run --example fullscreen_modes`
//!
//! This example displays the current window mode and allows switching between modes:
//! - Press `1` for exclusive fullscreen (uses selected video mode) WARNING: Exclusive fullscreen on
//!   macOS may panic on exit due to winit bugs. See: <https://github.com/rust-windowing/winit/issues/3668>
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
use bevy_window_manager::Monitors;
use bevy_window_manager::WindowExt;
use bevy_window_manager::WindowManagerPlugin;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Fullscreen Modes Test".into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(WindowManagerPlugin)
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
        "Window Position: {:?}\n\
         Window Size: {}x{}\n\
         Mode: {:?} (set value only, not dynamically updated)\n\
         Effective Mode: {:?}\n\
         \n\
         Monitor {}\n\
         Position: ({}, {})\n\
         Size: {}x{}\n\
         Scale: {}\n\
         Refresh Rate: {}\n\
         \n\
         Video Modes (Up/Down to select):\n\
         {video_modes_display}\n\
         \n\
         Controls:\n\
         [1] Exclusive Fullscreen (selected mode)\n\
         [2] Borderless Fullscreen\n\
         [W/Esc] Windowed",
        window.position,
        window.physical_width(),
        window.physical_height(),
        window.mode,
        effective_mode,
        monitor.index,
        monitor.position.x,
        monitor.position.y,
        monitor.size.x,
        monitor.size.y,
        monitor.scale,
        refresh_display
    );
}

fn handle_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut window: Single<&mut Window, With<PrimaryWindow>>,
    bevy_monitors: Query<(Entity, &Monitor)>,
    monitors_res: Res<Monitors>,
    mut selected: ResMut<SelectedVideoMode>,
) {
    let monitor = window.monitor(&monitors_res);

    // Get video modes for current monitor by matching position
    let video_modes: Vec<VideoMode> = bevy_monitors
        .iter()
        .find(|(_, m)| m.physical_position == monitor.position)
        .map(|(_, m)| m.video_modes.clone())
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

        window.mode =
            WindowMode::Fullscreen(MonitorSelection::Index(monitor.index), video_mode_selection);
    }
    if keys.just_pressed(KeyCode::Digit2) {
        window.mode = WindowMode::BorderlessFullscreen(MonitorSelection::Index(monitor.index));
    }
    if keys.just_pressed(KeyCode::KeyW) || keys.just_pressed(KeyCode::Escape) {
        window.mode = WindowMode::Windowed;
    }
}
