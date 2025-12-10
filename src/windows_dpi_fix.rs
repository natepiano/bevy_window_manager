//! Workaround for Windows DPI change bug when dragging between mixed-DPI monitors.
//!
//! On Windows 11 with monitors of different DPI scales, winit's `WM_DPICHANGED`
//! handler has a bug that causes windows to bounce back or resize incorrectly
//! when dragged between monitors.
//!
//! This module subclasses the window to intercept `WM_DPICHANGED` and handle it
//! using Microsoft's recommended simple approach: use the suggested RECT from lparam.
//!
//! See: <https://github.com/rust-windowing/winit/issues/4041>
//!
//! **This workaround can be removed when winit releases a version with the fix
//! from <https://github.com/rust-windowing/winit/pull/4341>**

use bevy::ecs::system::NonSendMarker;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy::winit::WINIT_WINDOWS;
use raw_window_handle::HasWindowHandle;
use raw_window_handle::RawWindowHandle;
use windows::Win32::Foundation::HWND;
use windows::Win32::Foundation::LPARAM;
use windows::Win32::Foundation::LRESULT;
use windows::Win32::Foundation::RECT;
use windows::Win32::Foundation::WPARAM;
use windows::Win32::UI::Shell::DefSubclassProc;
use windows::Win32::UI::Shell::RemoveWindowSubclass;
use windows::Win32::UI::Shell::SetWindowSubclass;
use windows::Win32::UI::WindowsAndMessaging::SetWindowPos;
use windows::Win32::UI::WindowsAndMessaging::SWP_NOACTIVATE;
use windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER;
use windows::Win32::UI::WindowsAndMessaging::WM_DPICHANGED;

const SUBCLASS_ID: usize = 1;

/// Wrapper around HWND that implements Send + Sync.
///
/// # Safety
///
/// HWND is a handle to a window created on the main thread. Window handles
/// are valid to use from any thread for read-only operations like removing
/// a subclass. The guard only stores the handle and removes the subclass on drop.
struct SendSyncHwnd(HWND);

// SAFETY: HWND is just a pointer/handle that can be sent between threads.
// The actual window operations are thread-safe when using proper Win32 APIs.
unsafe impl Send for SendSyncHwnd {}
unsafe impl Sync for SendSyncHwnd {}

/// Get the HWND from a Bevy window entity.
fn get_hwnd(window_entity: Entity) -> Option<HWND> {
    WINIT_WINDOWS.with(|ww| {
        let ww = ww.borrow();
        let winit_window = ww.get_window(window_entity)?;
        match winit_window.window_handle().ok()?.as_raw() {
            RawWindowHandle::Win32(handle) => Some(HWND(handle.hwnd.get() as *mut _)),
            _ => None,
        }
    })
}

/// Handle `WM_DPICHANGED` using Microsoft's recommended simple approach.
///
/// The lparam contains a pointer to a RECT with the suggested new size/position.
/// We simply apply it using `SetWindowPos`.
fn handle_dpi_changed(hwnd: HWND, lparam: LPARAM) -> LRESULT {
    // SAFETY: lparam is a valid pointer to RECT per WM_DPICHANGED contract
    let suggested_rect = unsafe { &*(lparam.0 as *const RECT) };

    // SAFETY: SetWindowPos is safe with valid HWND and dimensions
    let result = unsafe {
        SetWindowPos(
            hwnd,
            None,
            suggested_rect.left,
            suggested_rect.top,
            suggested_rect.right - suggested_rect.left,
            suggested_rect.bottom - suggested_rect.top,
            SWP_NOZORDER | SWP_NOACTIVATE,
        )
    };

    if result.is_err() {
        warn!("[windows_dpi_fix] SetWindowPos failed: {:?}", result);
    }

    LRESULT(0)
}

/// Subclass window procedure that intercepts `WM_DPICHANGED`.
///
/// # Safety
///
/// This is a Windows callback. It must be called only by Windows with valid parameters.
unsafe extern "system" fn subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _uidsubclass: usize,
    _dwrefdata: usize,
) -> LRESULT {
    if msg == WM_DPICHANGED {
        debug!("[windows_dpi_fix] Intercepted WM_DPICHANGED");
        return handle_dpi_changed(hwnd, lparam);
    }

    // Pass all other messages to the original window procedure
    // SAFETY: DefSubclassProc is safe when called from a subclass proc
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

/// Guard resource that removes the window subclass on drop.
#[derive(Resource)]
pub struct DpiFixGuard {
    hwnd: SendSyncHwnd,
}

impl Drop for DpiFixGuard {
    fn drop(&mut self) {
        // SAFETY: RemoveWindowSubclass is safe with valid HWND and matching subclass ID
        let result =
            unsafe { RemoveWindowSubclass(self.hwnd.0, Some(subclass_proc), SUBCLASS_ID) };
        if result.as_bool() {
            debug!("[windows_dpi_fix] Removed DPI fix subclass");
        }
    }
}

/// System to install the DPI fix subclass on the primary window.
pub fn install_dpi_fix(
    mut commands: Commands,
    window_entity: Single<Entity, With<PrimaryWindow>>,
    _non_send: NonSendMarker,
) {
    let Some(hwnd) = get_hwnd(*window_entity) else {
        warn!("[windows_dpi_fix] Could not get HWND for primary window");
        return;
    };

    // SAFETY: SetWindowSubclass is safe with valid HWND
    let result = unsafe { SetWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID, 0) };

    if result.as_bool() {
        debug!("[windows_dpi_fix] Installed DPI change workaround");
        commands.insert_resource(DpiFixGuard { hwnd: SendSyncHwnd(hwnd) });
    } else {
        warn!("[windows_dpi_fix] Failed to install subclass");
    }
}

/// Initialize the Windows DPI fix.
pub fn init(app: &mut App) {
    app.add_systems(Startup, install_dpi_fix);
}
