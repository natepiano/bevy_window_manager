//! Disable macOS window tabbing on all managed windows.
//!
//! When two windows from the same app both go `BorderlessFullscreen`, macOS tabs them
//! into the same fullscreen space. Setting `NSWindow.tabbingMode = .disallowed` on every
//! window prevents this at the AppKit level.
//!
//! The primary window gets the fix at `Startup` (after winit creates the OS window).
//! Secondary managed windows get it via an `Update` query on `Added<ManagedWindow>`.

use bevy::ecs::system::NonSendMarker;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy::winit::WINIT_WINDOWS;
use objc2::rc::Retained;
use objc2_app_kit::NSView;
use objc2_app_kit::NSWindow;
use objc2_app_kit::NSWindowTabbingMode;
use raw_window_handle::HasWindowHandle;
use raw_window_handle::RawWindowHandle;

use crate::ManagedWindow;
use crate::systems;

/// Get the `NSWindow` for a Bevy window entity.
fn get_ns_window(entity: Entity) -> Option<Retained<NSWindow>> {
    WINIT_WINDOWS.with(|ww| {
        let ww = ww.borrow();
        let winit_window = ww.get_window(entity)?;
        let handle = winit_window.window_handle().ok()?;
        let RawWindowHandle::AppKit(appkit_handle) = handle.as_raw() else {
            return None;
        };
        // SAFETY: `ns_view` is a valid `NSView` pointer from winit's window handle.
        let ns_view: &NSView = unsafe { appkit_handle.ns_view.cast().as_ref() };
        ns_view.window()
    })
}

/// Disable tabbing on the primary window at startup.
///
/// This prevents macOS from pulling newly created windows into the primary's
/// fullscreen tab group. Without this, any window spawned while the primary is
/// fullscreen gets auto-tabbed before our `Update` systems can intervene.
fn disable_tabbing_on_primary(
    window_entity: Single<Entity, With<PrimaryWindow>>,
    _non_send: NonSendMarker,
) {
    let Some(ns_window) = get_ns_window(*window_entity) else {
        warn!("[macos_tabbing_fix] Could not get NSWindow for primary window");
        return;
    };

    ns_window.setTabbingMode(NSWindowTabbingMode::Disallowed);
    debug!("[macos_tabbing_fix] Disabled tabbing on primary window");
}

/// Disable tabbing on newly added `ManagedWindow` entities.
fn disable_tabbing_on_managed(
    new_windows: Query<Entity, Added<ManagedWindow>>,
    _non_send: NonSendMarker,
) {
    for entity in &new_windows {
        let Some(ns_window) = get_ns_window(entity) else {
            debug!("[macos_tabbing_fix] Could not get NSWindow for managed window {entity:?}");
            continue;
        };

        ns_window.setTabbingMode(NSWindowTabbingMode::Disallowed);
        debug!("[macos_tabbing_fix] Disabled tabbing on managed window {entity:?}");
    }
}

/// Register the tabbing fix systems.
///
/// The primary window fix runs at `Startup` so it's in place before any managed
/// windows are created. The managed window fix runs in `Update` before
/// `restore_windows` so `tabbingMode` is set before fullscreen is applied.
pub fn init(app: &mut App) {
    app.add_systems(Startup, disable_tabbing_on_primary);
    app.add_systems(
        Update,
        disable_tabbing_on_managed.before(systems::restore_windows),
    );
}
