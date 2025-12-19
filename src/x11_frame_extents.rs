//! Query X11 `_NET_FRAME_EXTENTS` to work around winit #4445.
//!
//! On X11, winit's `outer_position()` returns the client area position instead of
//! the frame position. This causes window position to drift by the title bar height
//! on each save/restore cycle.
//!
//! This module queries `_NET_FRAME_EXTENTS` directly via x11rb and compensates
//! the saved position before restoring.
//!
//! See: <https://github.com/rust-windowing/winit/issues/4445>

use bevy::ecs::system::NonSendMarker;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy::winit::WINIT_WINDOWS;
use raw_window_handle::HasWindowHandle;
use raw_window_handle::RawWindowHandle;
use x11rb::protocol::xproto::AtomEnum;
use x11rb::protocol::xproto::ConnectionExt;
use x11rb::xcb_ffi::XCBConnection;

use crate::types::TargetPosition;
use crate::types::X11FrameCompensated;

/// Query `_NET_FRAME_EXTENTS` for an X11 window ID.
///
/// Returns the top extent (title bar height), or `None` if not yet available.
/// Does not block - returns immediately if the WM hasn't set the property yet.
fn query_frame_top(window_id: u32) -> Option<i32> {
    let (conn, _screen_num) = XCBConnection::connect(None).ok()?;

    let atom_cookie = conn.intern_atom(false, b"_NET_FRAME_EXTENTS").ok()?;
    let atom = atom_cookie.reply().ok()?.atom;

    let property_cookie = conn
        .get_property(false, window_id, atom, AtomEnum::CARDINAL, 0, 4)
        .ok()?;
    let property = property_cookie.reply().ok()?;

    let values: Vec<u32> = property.value32()?.collect();
    if values.len() >= 4 {
        Some(values[2] as i32) // top extent
    } else {
        None
    }
}

/// Get the X11 window ID from a window handle.
fn get_x11_window_id<W: HasWindowHandle>(window: &W) -> Option<u32> {
    let handle = window.window_handle().ok()?;
    match handle.as_raw() {
        RawWindowHandle::Xlib(h) => Some(h.window as u32),
        RawWindowHandle::Xcb(h) => Some(h.window.get()),
        _ => None,
    }
}

/// System to compensate `TargetPosition` for X11 frame extents (W6 workaround).
///
/// The saved position is the client area position (due to winit bug #4445),
/// but `set_outer_position` expects the frame position. We subtract the title
/// bar height to get the correct frame position.
///
/// Inserts `X11FrameCompensated` token when successful, which gates
/// `restore_primary_window`. If frame extents aren't available yet,
/// returns silently and retries next frame.
pub fn compensate_target_position(
    mut commands: Commands,
    window_entity: Single<Entity, With<PrimaryWindow>>,
    mut target: ResMut<TargetPosition>,
    _non_send: NonSendMarker,
) {
    let Some(pos) = target.position else {
        // No position to compensate (Wayland) - mark as done
        commands.insert_resource(X11FrameCompensated);
        return;
    };

    let frame_top = WINIT_WINDOWS.with(|ww| {
        let ww = ww.borrow();
        ww.get_window(*window_entity).and_then(|winit_window| {
            let window_id = get_x11_window_id(&**winit_window)?;
            query_frame_top(window_id)
        })
    });

    let Some(frame_top) = frame_top else {
        // WM hasn't set frame extents yet - try again next frame
        return;
    };

    let compensated = IVec2::new(pos.x, pos.y - frame_top);
    info!(
        "[W6] Compensating position: {:?} -> {:?} (frame_top={})",
        pos, compensated, frame_top
    );
    target.position = Some(compensated);
    commands.insert_resource(X11FrameCompensated);
}
