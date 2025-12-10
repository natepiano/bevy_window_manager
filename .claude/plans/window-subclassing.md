# Windows DPI Change Workaround via Window Subclassing

## Feature Flag

This workaround will be behind an optional feature flag since:
- It only applies to a subset of Windows scenarios (Windows 11 24H2 with mixed-DPI monitors)
- It may have side effects (bypasses winit's DPI handling, may affect scale factor events)
- Users on unaffected Windows versions don't need it

```toml
[features]
default = []
windows-dpi-fix = []
```

Usage:
```toml
[dependencies]
bevy_window_manager = { version = "0.17", features = ["windows-dpi-fix"] }
```

The feature will be documented in README.md with guidance on when to enable it.

## Problem

On Windows 11 (particularly 24H2) with mixed-DPI monitors, winit has a bug where dragging a window between monitors causes the window to bounce back or resize incorrectly. This is tracked in:
- https://github.com/rust-windowing/winit/issues/4041
- Fixed in PR https://github.com/rust-windowing/winit/pull/4341 (merged but not yet released)

The bug is in winit's `WM_DPICHANGED` handler, which does incorrect calculations when the window crosses monitor boundaries.

## Proposed Solution

Subclass the Bevy/winit window to intercept `WM_DPICHANGED` messages and handle them using Microsoft's recommended simple approach, bypassing winit's buggy handler.

## Implementation Plan

### 1. Add Windows Dependencies

Add the `windows` crate to `Cargo.toml` with the required features:

```toml
[target.'cfg(windows)'.dependencies]
windows = { version = "0.58", features = [
    "Win32_Foundation",
    "Win32_UI_WindowsAndMessaging",
    "Win32_UI_Shell",
    "Win32_Graphics_Gdi",
] }
```

### 2. Create New Module `src/windows_dpi_fix.rs`

Structure similar to `macos_fullscreen_fix.rs`:

```rust
//! Workaround for Windows DPI change bug when dragging between mixed-DPI monitors.
//!
//! On Windows 11 with monitors of different DPI scales, winit's WM_DPICHANGED
//! handler has a bug that causes windows to bounce back or resize incorrectly
//! when dragged between monitors.
//!
//! This module subclasses the window to intercept WM_DPICHANGED and handle it
//! using Microsoft's recommended simple approach.
//!
//! See: https://github.com/rust-windowing/winit/issues/4041
//!
//! **This workaround can be removed when winit releases a version with the fix
//! from https://github.com/rust-windowing/winit/pull/4341**
```

### 3. Core Implementation Steps

#### 3.1 Get HWND from Bevy Window

```rust
use bevy::winit::WINIT_WINDOWS;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

fn get_hwnd(window_entity: Entity) -> Option<HWND> {
    WINIT_WINDOWS.with(|ww| {
        let ww = ww.borrow();
        let winit_window = ww.get_window(window_entity)?;
        match winit_window.window_handle().ok()?.as_raw() {
            RawWindowHandle::Win32(handle) => {
                Some(HWND(handle.hwnd.get() as isize))
            }
            _ => None,
        }
    })
}
```

#### 3.2 Subclass the Window

Use `SetWindowSubclass` to install our custom window procedure:

```rust
use windows::Win32::UI::Shell::SetWindowSubclass;
use windows::Win32::UI::WindowsAndMessaging::*;

const SUBCLASS_ID: usize = 1;

unsafe extern "system" fn subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _uidsubclass: usize,
    _dwrefdata: usize,
) -> LRESULT {
    if msg == WM_DPICHANGED {
        // Handle DPI change using Microsoft's recommended approach
        return handle_dpi_changed(hwnd, wparam, lparam);
    }

    // Pass all other messages to the original window procedure
    DefSubclassProc(hwnd, msg, wparam, lparam)
}

fn handle_dpi_changed(hwnd: HWND, _wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // lparam contains a pointer to a RECT with the suggested new size/position
    let suggested_rect = unsafe { *(lparam.0 as *const RECT) };

    // Use SetWindowPos with the suggested rectangle (Microsoft's recommended approach)
    unsafe {
        SetWindowPos(
            hwnd,
            None,
            suggested_rect.left,
            suggested_rect.top,
            suggested_rect.right - suggested_rect.left,
            suggested_rect.bottom - suggested_rect.top,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }

    LRESULT(0)
}
```

#### 3.3 Install Subclass at Startup

Create a system that runs in `PreStartup` after the window is created:

```rust
pub fn install_dpi_fix(
    window_entity: Single<Entity, With<PrimaryWindow>>,
    _non_send: NonSendMarker,
) {
    if let Some(hwnd) = get_hwnd(*window_entity) {
        unsafe {
            SetWindowSubclass(
                hwnd,
                Some(subclass_proc),
                SUBCLASS_ID,
                0,
            );
        }
        debug!("[windows_dpi_fix] Installed DPI change workaround");
    }
}
```

#### 3.4 Clean Up on Exit

Remove the subclass when the app exits (optional but good practice):

```rust
use windows::Win32::UI::Shell::RemoveWindowSubclass;

#[derive(Resource)]
pub struct DpiFixGuard {
    hwnd: HWND,
}

impl Drop for DpiFixGuard {
    fn drop(&mut self) {
        unsafe {
            RemoveWindowSubclass(self.hwnd, Some(subclass_proc), SUBCLASS_ID);
        }
    }
}
```

### 4. Integrate into Plugin

In `lib.rs`:

```rust
#[cfg(all(target_os = "windows", feature = "windows-dpi-fix"))]
mod windows_dpi_fix;

fn build_plugin(app: &mut App, path: PathBuf) {
    #[cfg(target_os = "macos")]
    macos_fullscreen_fix::init(app);

    #[cfg(all(target_os = "windows", feature = "windows-dpi-fix"))]
    windows_dpi_fix::init(app);

    // ... rest of plugin setup
}
```

In `Cargo.toml`:

```toml
[features]
default = []
windows-dpi-fix = ["windows"]

[target.'cfg(windows)'.dependencies]
windows = { version = "0.58", features = [
    "Win32_Foundation",
    "Win32_UI_WindowsAndMessaging",
    "Win32_UI_Shell",
    "Win32_Graphics_Gdi",
], optional = true }
```

### 5. Testing Plan

1. Build on Windows with mixed-DPI monitors
2. Verify window can be dragged from monitor 1 (scale 1.75) to monitor 0 (scale 2.0)
3. Verify window resizes correctly during the drag
4. Verify window position is stable after the drag
5. Test in both directions (high→low DPI and low→high DPI)
6. Test that normal window operations still work (resize, minimize, maximize, close)

## Risks and Considerations

1. **Interaction with winit**: Our subclass proc intercepts `WM_DPICHANGED` before winit sees it. This should be safe since we're handling it correctly, but winit won't know the DPI changed. This might affect winit's internal state.

2. **Bevy scale factor events**: Bevy may not receive `ScaleFactorChanged` events since we're bypassing winit's handler. We may need to manually trigger these or handle them ourselves.

3. **Future winit updates**: When winit releases the fix, we should test removing this workaround to avoid conflicts.

4. **Thread safety**: Window procedures run on the window's thread. The subclass proc must be careful about thread safety.

## Alternative Approaches Considered

1. **Disable Per-Monitor DPI Awareness**: Simpler but results in blurry rendering when moving between monitors.

2. **Patch winit directly**: Would require forking winit, adding maintenance burden.

3. **Wait for upstream fix**: The fix is merged but not released. Could be weeks/months.

## References

- Microsoft WM_DPICHANGED documentation: https://learn.microsoft.com/en-us/windows/win32/hidpi/wm-dpichanged
- winit issue: https://github.com/rust-windowing/winit/issues/4041
- winit fix PR: https://github.com/rust-windowing/winit/pull/4341
- SetWindowSubclass: https://learn.microsoft.com/en-us/windows/win32/api/commctrl/nf-commctrl-setwindowsubclass
