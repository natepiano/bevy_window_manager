//! Simple example demonstrating `WindowManagerPlugin`.
//!
//! Run with: `cargo run --example simple_restore`
//!
//! This creates a window that remembers its position and size across sessions.
//! Try moving/resizing the window, closing it, and running again - it will
//! restore to the same position and size.
//!
//! Window state is saved to (using `dirs::config_dir()` and the executable name):
//! - macOS: `~/Library/Application Support/simple_restore/windows.ron`
//! - Linux: `~/.config/simple_restore/windows.ron`
//! - Windows: `C:\Users\{user}\AppData\Roaming\simple_restore\windows.ron`

use bevy::prelude::*;
use bevy::window::Monitor;
use bevy::window::PrimaryWindow;
use bevy::ecs::message::MessageReader;
use bevy::window::WindowScaleFactorChanged;
use bevy_window_manager::Monitors;
use bevy_window_manager::WindowExt;
use bevy_window_manager::WindowManagerPlugin;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Simple Window Restore Example".to_string(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(WindowManagerPlugin)
        .add_systems(Startup, setup)
        .add_systems(Update, (update_info_text, log_scale_changes))
        .run();
}

fn log_scale_changes(mut events: MessageReader<WindowScaleFactorChanged>) {
    for event in events.read() {
        info!(
            "[ScaleFactorChanged] window={:?} scale={}",
            event.window, event.scale_factor
        );
    }
}

#[derive(Component)]
struct InfoText;

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);

    commands.spawn((
        InfoText,
        Text::default(),
        TextFont {
            font_size: 20.0,
            ..default()
        },
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(10.0),
            left: Val::Px(10.0),
            ..default()
        },
    ));
}

fn update_info_text(
    window: Single<&Window, With<PrimaryWindow>>,
    monitors: Res<Monitors>,
    bevy_monitors: Query<&Monitor>,
    mut text: Single<&mut Text, With<InfoText>>,
) {
    let monitor = window.monitor(&monitors);
    let effective_mode = window.effective_mode(&monitors);

    // Find refresh rate from Bevy's Monitor by matching position
    let refresh_rate = bevy_monitors
        .iter()
        .find(|m| m.physical_position == monitor.position)
        .and_then(|m| m.refresh_rate_millihertz)
        .map(|r| r / 1000);

    let refresh_display = refresh_rate.map_or_else(|| "N/A".into(), |hz| format!("{hz}Hz"));

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
         Refresh Rate: {}",
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
