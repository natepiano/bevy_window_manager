//! Example demonstrating custom app name with `RestoreWindowPlugin`.
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
//! For full control over file placement, use `RestoreWindowPlugin::with_path()` instead.
//! See the `custom_path` example for details.

use bevy::prelude::*;
use bevy_restore_window::RestoreWindowPlugin;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Custom App Name Example".to_string(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(RestoreWindowPlugin::with_app_name("my_awesome_game"))
        .run();
}
