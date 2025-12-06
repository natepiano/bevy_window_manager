//! Example demonstrating explicit path configuration with `RestoreWindowPlugin`.
//!
//! Run with: `cargo run --example custom_path`
//!
//! This shows how to manually construct a cross-platform config path using `dirs`,
//! giving you full control over the app name and filename. Of course you can put it anywhere you want,
//! we're just using `dirs` for convenience in this example.
//!
//! Window state is saved to:
//! - macOS: `~/Library/Application Support/my_custom_app/window_state.ron`
//! - Linux: `~/.config/my_custom_app/window_state.ron`
//! - Windows: `C:\Users\{user}\AppData\Roaming\my_custom_app\window_state.ron`

use bevy::prelude::*;
use bevy_restore_window::RestoreWindowPlugin;

#[expect(
    clippy::expect_used,
    reason = "example code - panicking on missing config dir is acceptable"
)]
fn main() {
    // Construct a cross-platform config path manually
    let config_path = dirs::config_dir()
        .expect("Could not find config directory")
        .join("my_custom_app")
        .join("window_state.ron");

    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Custom Path Example".to_string(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(RestoreWindowPlugin::with_path(config_path))
        .run();
}
