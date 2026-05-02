//! Workaround for winit #4445: fix X11 saved-position drift by querying
//! `_NET_FRAME_EXTENTS` and subtracting the title bar height before restore.
//!
//! On X11, winit's `outer_position()` returns the client-area position instead of
//! the frame position, so a save/restore cycle drifts the window down by the title
//! bar height each time. This module queries the X11 frame extents and rewrites
//! `TargetPosition` before `restore_windows` runs.
//!
//! See: <https://github.com/rust-windowing/winit/issues/4445>

use bevy::ecs::system::NonSendMarker;
use bevy::prelude::*;
use bevy::winit::WINIT_WINDOWS;
use bevy_kana::ToI32;
use bevy_kana::ToU32;
use bevy_kana::ToUsize;
use raw_window_handle::HasWindowHandle;
use raw_window_handle::RawWindowHandle;
use x11rb::protocol::xproto::AtomEnum;
use x11rb::protocol::xproto::ConnectionExt;
use x11rb::xcb_ffi::XCBConnection;

use crate::constants::FRAME_EXTENT_COUNT;
use crate::constants::FRAME_EXTENT_TOP_INDEX;
use crate::restore::TargetPosition;
use crate::restore::X11FrameCompensated;

/// Subtract the X11 title bar height from `TargetPosition.physical_position`.
///
/// Inserts `X11FrameCompensated` once frame extents are available; this gates
/// `restore_windows`. If `_NET_FRAME_EXTENTS` is not yet set by the WM, returns
/// silently and retries next frame.
pub(crate) fn compensate_target_position(
    mut commands: Commands,
    mut windows: Query<(Entity, &mut TargetPosition), Without<X11FrameCompensated>>,
    _: NonSendMarker,
) {
    for (entity, mut target) in &mut windows {
        let Some(position) = target.physical_position else {
            commands.entity(entity).insert(X11FrameCompensated);
            continue;
        };

        let Some(frame_top) = query_frame_top_for_entity(entity) else {
            continue;
        };

        let compensated = IVec2::new(position.x, position.y - frame_top);
        info!(
            "[W6] Compensating position: {position:?} -> {compensated:?} (frame_top={frame_top})"
        );
        target.physical_position = Some(compensated);
        commands.entity(entity).insert(X11FrameCompensated);
    }
}

fn query_frame_top_for_entity(entity: Entity) -> Option<i32> {
    WINIT_WINDOWS.with(|winit_windows| {
        let winit_windows = winit_windows.borrow();
        winit_windows.get_window(entity).and_then(|winit_window| {
            let window_id = get_x11_window_id(&**winit_window)?;
            query_frame_top(window_id)
        })
    })
}

fn query_frame_top(window_id: u32) -> Option<i32> {
    let (conn, _) = XCBConnection::connect(None).ok()?;

    let atom_cookie = conn.intern_atom(false, b"_NET_FRAME_EXTENTS").ok()?;
    let atom = atom_cookie.reply().ok()?.atom;

    let property_cookie = conn
        .get_property(
            false,
            window_id,
            atom,
            AtomEnum::CARDINAL,
            0,
            FRAME_EXTENT_COUNT,
        )
        .ok()?;
    let property = property_cookie.reply().ok()?;

    let values: Vec<u32> = property.value32()?.collect();
    if values.len() >= FRAME_EXTENT_COUNT.to_usize() {
        Some(values[FRAME_EXTENT_TOP_INDEX].to_i32())
    } else {
        None
    }
}

fn get_x11_window_id<W: HasWindowHandle>(window: &W) -> Option<u32> {
    let handle = window.window_handle().ok()?;
    match handle.as_raw() {
        RawWindowHandle::Xlib(h) => Some(h.window.to_u32()),
        RawWindowHandle::Xcb(h) => Some(h.window.get()),
        _ => None,
    }
}
