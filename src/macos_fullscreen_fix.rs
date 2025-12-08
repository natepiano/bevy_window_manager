//! Workaround for macOS crash when exiting from exclusive fullscreen.
//!
//! On macOS, when the app exits while in exclusive fullscreen, Bevy's windows
//! (stored in TLS) are dropped during TLS destruction. winit's `Window::drop`
//! calls `set_fullscreen(None)`, which triggers a macOS callback that tries to
//! access TLS - causing a panic.
//!
//! This module provides a resource that exits fullscreen in its `Drop` impl,
//! which runs during `world.clear_all()` before TLS destruction, avoiding the panic.
//!
//! **This workaround will be removed when Bevy 0.18 is released**, which includes
//! the fix from <https://github.com/bevyengine/bevy/pull/22060>.

use std::ops::Deref;

use bevy::prelude::*;
use bevy::winit::WINIT_WINDOWS;

/// Guard resource that exits fullscreen on drop to prevent macOS TLS panic.
#[derive(Resource)]
pub struct FullscreenExitGuard;

impl Drop for FullscreenExitGuard {
    fn drop(&mut self) {
        WINIT_WINDOWS.with(|ww| {
            for (_, window) in &ww.borrow().windows {
                window.deref().set_fullscreen(None);
            }
        });
        // Give macOS time to process the fullscreen exit
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

/// Insert the fullscreen exit guard into the app.
pub fn init(app: &mut App) { app.insert_resource(FullscreenExitGuard); }
