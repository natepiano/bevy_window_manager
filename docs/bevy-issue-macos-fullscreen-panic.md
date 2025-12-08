# macOS: Panic on exit from exclusive fullscreen mode

## Summary

On macOS, exiting an app while in exclusive fullscreen mode causes a panic due to Thread Local Storage (TLS) being accessed during its destruction.

## Reproduction

Minimal example:

```rust
use bevy::prelude::*;
use bevy::window::{MonitorSelection, VideoModeSelection, WindowMode};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Exclusive Fullscreen Test".into(),
                mode: WindowMode::Fullscreen(
                    MonitorSelection::Primary,
                    VideoModeSelection::Current,
                ),
                ..default()
            }),
            ..default()
        }))
        .run();
}
```

1. Run the app (enters exclusive fullscreen)
2. Press Cmd+Q to quit
3. Panic occurs

## Environment

- macOS 26.1 (Sequoia)
- Bevy 0.17
- winit 0.30.12

## Error

```
thread 'main' panicked at .../std/src/thread/local.rs:281:25:
cannot access a Thread Local Storage value during or after destruction: AccessError
```

## Root Cause Analysis

The panic occurs because `WINIT_WINDOWS` is stored in TLS, and windows are dropped during TLS destruction rather than during the event loop's `exiting` callback.

### Sequence of events:

1. `fn exiting` runs → `world.clear_all()` clears ECS resources
2. `winit_runner` returns from `event_loop.run_app()`
3. TLS destructors run as the thread cleans up
4. `WINIT_WINDOWS` TLS is destroyed, dropping all windows
5. winit's `Window::drop` calls `set_fullscreen(None)` for exclusive fullscreen
6. macOS sends a frame change callback
7. The callback tries to access TLS → **panic**

### Stack trace with annotations:

```
# TLS destructors running (thread cleanup phase)
56: std::sys::thread_local::destructors::list::run
55: std::sys::thread_local::native::eager::destroy

# WINIT_WINDOWS HashMap being dropped during TLS destruction
54: <hashbrown::raw::RawTable as Drop>::drop
53: hashbrown::raw::RawTableInner::drop_inner_table
52: hashbrown::raw::RawTableInner::drop_elements
51: hashbrown::raw::Bucket<T>::drop

# WindowWrapper<WinitWindow> being dropped
48: drop_in_place<bevy_window::raw_handle::WindowWrapper<winit::window::Window>>

# winit Window::drop calling set_fullscreen(None)
43: drop_in_place<winit::window::Window>
38-39: <winit::window::Window as Drop>::drop::{{closure}}
38: WindowDelegate::set_fullscreen

# macOS callback fires, tries to access TLS that's being destroyed
20: WinitView::frame_did_change

# PANIC: TLS already in destruction
```

### Evidence from winit source (v0.30.12)

winit's `Window::drop` explicitly exits fullscreen:

```rust
// src/window.rs
impl Drop for Window {
    fn drop(&mut self) {
        self.window.maybe_wait_on_main(|w| {
            // If the window is in exclusive fullscreen, we must restore the desktop
            // video mode (generally this would be done on application exit, but
            // closing the window doesn't necessarily always mean application exit,
            // such as when there are multiple windows)
            if let Some(Fullscreen::Exclusive(_)) = w.fullscreen().map(|f| f.into()) {
                w.set_fullscreen(None);
            }
        })
    }
}
```

## Suggested Fix

Drop windows from TLS in `fn exiting` before the event loop returns, while the event loop is still active:

```rust
// bevy_winit/src/state.rs
fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
    // Drop windows while event loop is still active, before TLS destruction
    WINIT_WINDOWS.with(|ww| ww.borrow_mut().windows.clear());
    self.world_mut().clear_all();
}
```

This ensures windows are dropped in a controlled context where:
- The event loop is still running
- TLS is not being destroyed
- winit's `set_fullscreen(None)` can complete successfully

## Current Workaround

Create a resource that exits fullscreen in its `Drop` impl (runs during `world.clear_all()`):

```rust
use std::ops::Deref;
use bevy::prelude::*;
use bevy::winit::WINIT_WINDOWS;

#[derive(Resource)]
struct FullscreenExitGuard;

impl Drop for FullscreenExitGuard {
    fn drop(&mut self) {
        WINIT_WINDOWS.with(|ww| {
            for (_, window) in ww.borrow().windows.iter() {
                window.deref().set_fullscreen(None);
            }
        });
    }
}

// Insert in Startup system:
commands.insert_resource(FullscreenExitGuard);
```

This works because after exiting fullscreen, winit's `Window::drop` check (`if let Some(Fullscreen::Exclusive(_))`) fails, so it never calls `set_fullscreen(None)` during TLS destruction.
