//! Simple example demonstrating the `RestoreWindowPlugin`.
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
use bevy_restore_windows::RestoreWindowsPlugin;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Simple Window Restore Example".to_string(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(RestoreWindowsPlugin::default())
        .run();
}
