//! Simple example demonstrating the `RestoreWindowPlugin`.
//!
//! Run with: `cargo run --example simple`
//!
//! This creates a primary window that remembers its position and size across sessions.
//! Try moving/resizing the window, closing it, and running again - it will
//! restore to the same position and size.

use bevy::prelude::*;
use bevy_restore_window::RestoreWindowPlugin;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Window Restore Example".to_string(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(RestoreWindowPlugin::new("bevy_restore_window_example"))
        .run();
}
