//! Example demonstrating custom app name with `WindowManagerPlugin`.
//!
//! Run with: `cargo run --example custom_app_name`
//!
//! This shows how to specify a custom app name for the config directory
//! while using the default config location and filename.
//!
//! Window state is saved to:
//! - macOS: `~/Library/Application Support/my_awesome_game/windows.ron`
//! - Linux: `~/.config/my_awesome_game/windows.ron`
//! - Windows: `C:\Users\{user}\AppData\Roaming\my_awesome_game\windows.ron`
//!
//! For full control over file placement, use `WindowManagerPlugin::with_path()` instead.
//! See the `custom_path` example for details.

use bevy::prelude::*;
use bevy::window::Monitor;
use bevy::window::PrimaryWindow;
use bevy_window_manager::CurrentMonitor;
use bevy_window_manager::Monitors;
use bevy_window_manager::WindowExt;
use bevy_window_manager::WindowManagerPlugin;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Custom App Name Example".to_string(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(WindowManagerPlugin::with_app_name("my_awesome_game"))
        .add_systems(Startup, setup)
        .add_systems(Update, update_info_text)
        .run();
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
    window_query: Single<(&Window, &CurrentMonitor), With<PrimaryWindow>>,
    monitors: Res<Monitors>,
    bevy_monitors: Query<&Monitor>,
    mut text: Single<&mut Text, With<InfoText>>,
) {
    let (window, monitor) = *window_query;
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
