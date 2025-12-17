//! Query X11 `_NET_FRAME_EXTENTS` to work around winit #4445.
//!
//! On X11, winit's `outer_position()` returns the client area position instead of
//! the frame position. This is because winit's heuristic incorrectly zeros out
//! `_NET_FRAME_EXTENTS` when the window is detected as "not nested".
//!
//! This module queries `_NET_FRAME_EXTENTS` directly via x11rb so we can
//! compensate for the title bar height when saving window positions.
//!
//! See: <https://github.com/rust-windowing/winit/issues/4445>

use bevy::prelude::*;
use raw_window_handle::HasWindowHandle;
use raw_window_handle::RawWindowHandle;
use x11rb::protocol::xproto::AtomEnum;
use x11rb::protocol::xproto::ConnectionExt;
use x11rb::xcb_ffi::XCBConnection;

/// Frame extents (left, right, top, bottom) from `_NET_FRAME_EXTENTS`.
#[derive(Debug, Clone, Copy, Default)]
pub struct FrameExtents {
    pub left:   u32,
    pub right:  u32,
    pub top:    u32,
    pub bottom: u32,
}

/// Query `_NET_FRAME_EXTENTS` for an X11 window ID.
///
/// Returns `None` if the connection fails or the property is not set.
fn query_frame_extents(window_id: u32) -> Option<FrameExtents> {
    // Connect to the X server
    let (conn, _screen_num) = XCBConnection::connect(None).ok()?;

    // Get the _NET_FRAME_EXTENTS atom
    let atom_cookie = conn.intern_atom(false, b"_NET_FRAME_EXTENTS").ok()?;
    let atom = atom_cookie.reply().ok()?.atom;

    // Query the property
    let property_cookie = conn
        .get_property(false, window_id, atom, AtomEnum::CARDINAL, 0, 4)
        .ok()?;
    let property = property_cookie.reply().ok()?;

    // Parse the 4 u32 values: left, right, top, bottom
    let values: Vec<u32> = property.value32()?.collect();
    if values.len() >= 4 {
        Some(FrameExtents {
            left:   values[0],
            right:  values[1],
            top:    values[2],
            bottom: values[3],
        })
    } else {
        None
    }
}

/// Get the X11 window ID from a window that implements `HasWindowHandle`.
pub fn get_x11_window_id<W: HasWindowHandle>(window: &W) -> Option<u32> {
    let handle = window.window_handle().ok()?;
    match handle.as_raw() {
        RawWindowHandle::Xlib(h) => Some(h.window as u32),
        RawWindowHandle::Xcb(h) => Some(h.window.get()),
        _ => None,
    }
}

/// Get frame extents for a window that implements `HasWindowHandle`.
pub fn get_frame_extents<W: HasWindowHandle>(window: &W) -> Option<FrameExtents> {
    let window_id = get_x11_window_id(window)?;
    query_frame_extents(window_id)
}

/// Compensate a position by subtracting frame extents.
///
/// When workaround-winit-4445 is enabled, this subtracts the frame extents
/// from the position returned by `outer_position()` to get the true frame position.
pub fn compensate_position<W: HasWindowHandle>(window: &W, pos: IVec2) -> IVec2 {
    get_frame_extents(window).map_or_else(
        || {
            debug!("[x11_frame_extents] Could not get frame extents, using raw position");
            pos
        },
        |extents| {
            debug!(
                "[x11_frame_extents] _NET_FRAME_EXTENTS: left={} right={} top={} bottom={}",
                extents.left, extents.right, extents.top, extents.bottom
            );
            let compensated = IVec2::new(pos.x - extents.left as i32, pos.y - extents.top as i32);
            debug!(
                "[x11_frame_extents] Compensated position: {:?} -> {:?}",
                pos, compensated
            );
            compensated
        },
    )
}
