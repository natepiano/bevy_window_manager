//! Demonstrates the macOS fullscreen crash bug (without the workaround).
//!
//! Run with: `cargo run --example fullscreen_crash_on_exit`
//!
//! On macOS, pressing Cmd+Q while in exclusive fullscreen will panic.
//! This example does NOT use RestoreWindowsPlugin, which contains the workaround.

use bevy::prelude::*;
use bevy::window::MonitorSelection;
use bevy::window::VideoModeSelection;
use bevy::window::WindowMode;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Fullscreen Crash Test (no workaround)".into(),
                mode: WindowMode::Fullscreen(
                    MonitorSelection::Primary,
                    VideoModeSelection::Current,
                ),
                ..default()
            }),
            ..default()
        }))
        .add_systems(Startup, setup)
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
    commands.spawn((
        Text::new("Exclusive Fullscreen Mode (NO WORKAROUND)\n\nPress Cmd+Q to trigger crash"),
        TextFont {
            font_size: 40.0,
            ..default()
        },
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(100.0),
            left: Val::Px(100.0),
            ..default()
        },
    ));
}
