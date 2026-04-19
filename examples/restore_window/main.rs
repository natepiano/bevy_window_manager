//! Interactive example for testing window restoration, fullscreen modes, and multi-window
//! management.
//!
//! Run with: `cargo run --example restore_window`
//!
//! Controls (all windows):
//! - Press `Enter` for exclusive fullscreen (uses selected video mode)
//! - Press `B` for borderless fullscreen
//! - Press `W` for windowed mode
//! - Press `Up`/`Down` to cycle through available video modes
//! - Press `Space` to spawn a new managed window
//! - Press `P` to toggle persistence mode (`RememberAll` / `ActiveOnly`)
//! - Press `Ctrl+Shift+Backspace` to clear saved state and quit
//! - Press `Q` to quit

mod debug;
mod display;
mod events;
mod input;
mod mode_observers;
mod setup;
mod state;

use bevy::prelude::*;
use bevy_brp_extras::BrpExtrasPlugin;
use bevy_window_manager::WindowManagerPlugin;
use state::KeyboardInputMode;
use state::MismatchStates;
use state::RestoredStates;
use state::SelectedVideoModes;
use state::TEST_MODE_ENV_VAR;
use state::WindowCounter;
use state::WindowsSettledCount;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Window Restore - Primary Window".into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(WindowManagerPlugin)
        .add_plugins(BrpExtrasPlugin::default())
        .add_observer(setup::on_spawn_managed_window)
        .add_observer(events::on_window_restored)
        .add_observer(events::on_window_restore_mismatch)
        .add_observer(setup::on_secondary_window_added)
        .add_observer(setup::on_secondary_window_removed)
        .add_observer(mode_observers::on_set_borderless_fullscreen)
        .add_observer(mode_observers::on_set_windowed)
        .add_observer(mode_observers::on_set_exclusive_fullscreen)
        .add_observer(mode_observers::on_toggle_persistence)
        .add_observer(mode_observers::on_clear_state_and_quit)
        .add_observer(mode_observers::on_quit_app)
        .insert_resource(KeyboardInputMode::from(
            std::env::var(TEST_MODE_ENV_VAR).is_err(),
        ))
        .init_resource::<SelectedVideoModes>()
        .init_resource::<WindowCounter>()
        .init_resource::<RestoredStates>()
        .init_resource::<MismatchStates>()
        .init_resource::<WindowsSettledCount>()
        .add_systems(Startup, setup::setup)
        .add_systems(
            Update,
            (
                display::update_primary_display,
                display::update_secondary_displays,
                input::handle_global_input.run_if(state::keyboard_enabled),
                input::handle_window_mode_input.run_if(state::keyboard_enabled),
                debug::debug_winit_monitor,
                debug::debug_window_changed,
                debug::debug_scale_factor_changed,
            ),
        )
        .run();
}
