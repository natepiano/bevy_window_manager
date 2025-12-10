# bevy_window_manager

[![License](https://img.shields.io/badge/license-MIT%2FApache-blue.svg)](https://github.com/natemccoy/bevy_window_manager#license)
[![Crates.io](https://img.shields.io/crates/v/bevy_window_manager.svg)](https://crates.io/crates/bevy_window_manager)
[![Downloads](https://img.shields.io/crates/d/bevy_window_manager.svg)](https://crates.io/crates/bevy_window_manager)


A Bevy plugin for window state persistence and multi-monitor utilities.

## Motivation

Originally created as a mechanism to restore a window to its last known position when launching. I quickly discovered that on my MacBook Pro with Retina display (scale factor 2.0) and my external monitor with a scale factor 1.0, there were numerous issues with saving. Especially painful is the situation that winit uses the scale factor of where you're typing (launching) the application so that when it lands on a different monitor, you cannot reliably restore the window to its last known position when launching from a monitor with a different scale factor than the application window's target monitor.  Painful.

`bevy_window_manager` plugin works around these issues by using winit directly to capture actual window positions and compensate for scale factor conversions. See the documentation in [`src/lib.rs`](src/lib.rs) for technical details.

I wrote `bevy_window_manager` and released it for MacOS and left a note (below) that it's not yet tested on Windows and Linux. PRs Welcome. But it bothered me enough that i decided to install Windows in a virtual machine on my Mac and discovered that scale issue also plague windows. And there are even other issues with winit where there is an invisible border around a window, preventing precise placement.  

If you're trying this out with Windows from 0.17.0 published on crates.io, and you have differently scaled monitors (something Windows gives you far more control over than MacOS), then you'll experience the same issues - and in fact **0.17.0 of this plugin is broken for Windows** when restoring the primary window across differently scaled monitors so don't use this if that's your setup! I don't yet know about Linux. 

With that said, I'vemade a bunch of fixes for Windows and I'm testing it here on 0.17.1 - I _think_ this version will work for you on Windows if you want to refer to it on github directly in your Cargo.toml. Once I finish testing it I will publish 0.17.1.  

Then if I can get Linux running in a VM that will work with my monitors I will test it there also.

Future directions include comprehensive multi-monitor lifecycle support.

## Usage

```rust
use bevy::prelude::*;
use bevy_window_manager::WindowManagerPlugin;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(WindowManagerPlugin)
        .run();
}
```

## API

This crate exposes several types for working with monitors and windows beyond the plugin itself. See [docs.rs](https://docs.rs/bevy_window_manager) for full API documentation.

### `Monitors` Resource

Query available monitors sorted by position:

- `monitors.at(x, y)` – Find the monitor containing a position
- `monitors.by_index(index)` – Get monitor by sorted index
- `monitors.primary()` – Get the primary monitor (index 0)
- `monitors.closest_to(x, y)` – Find the closest monitor to a position

### `MonitorInfo`

Information about a single monitor: `index`, `scale`, `position`, and `size`.

### `WindowExt` Extension Trait

Requires `use bevy_window_manager::WindowExt`:

- `window.monitor(&monitors)` – Get the monitor this window is currently on
- `window.effective_mode(&monitors)` – Detect effective window mode (handles macOS green button fullscreen)
- `window.set_position_and_size(position, size)` – Set both in one call

### Plugin Configuration

- `WindowManagerPlugin` – Uses executable name for config directory
- `WindowManagerPlugin::with_app_name("name")` – Custom app name
- `WindowManagerPlugin::with_path(path)` – Full control over state file path

## Version Compatibility

| bevy_window_manager | Bevy |
|---------------------|------|
| 0.17                | 0.17 |

## Platform Support

| Platform | Status |
|----------|--------|
| macOS    | Tested |
| Windows  | Tested |
| Linux    | Untested |

This plugin was originally created to handle a MacBook Pro with external monitors at different scale factors, which caused window position/size corruption. Windows testing revealed additional issues with multi-monitor and multi-scale setups that have been fixed (see [CHANGELOG](CHANGELOG.md)). Linux support is untested - if you try it, please let me know how it goes. PRs welcome!

## macOS Fullscreen Crash Fix

This plugin includes a workaround for a Bevy bug on macOS where quitting the application while in exclusive fullscreen mode causes a panic. The crash occurs because Bevy stores windows in thread-local storage (TLS), and when windows are dropped during TLS destruction, winit's cleanup code triggers a macOS callback that tries to access already-destroyed TLS.

This plugin prevents the crash by exiting fullscreen before TLS destruction begins. See [docs/bevy-issue-macos-fullscreen-panic.md](docs/bevy-issue-macos-fullscreen-panic.md) for full technical details.

This issue will be fixed upstream in Bevy: https://github.com/bevyengine/bevy/pull/22060 - currently merged in bevy 0.18.0-dev.

## Windows DPI Drag Fix

On Windows with multiple monitors that have **different scale factors** (e.g., a 4K monitor at 200% and a 1440p monitor at 175%), winit has a bug where dragging a window between monitors causes the window to bounce back or resize incorrectly. This is particularly noticeable on Windows 11 24H2.

This plugin automatically installs a window subclass that intercepts `WM_DPICHANGED` messages and handles them using Microsoft's recommended approach, allowing smooth window dragging between monitors with different DPI scales.

This issue is tracked in [winit #4041](https://github.com/rust-windowing/winit/issues/4041) and fixed in [PR #4341](https://github.com/rust-windowing/winit/pull/4341) (merged but not yet released). This workaround will be removed when winit releases the fix.

## License

bevy_window_manager is free, open source and permissively licensed!
Except where noted (below and/or in individual files), all code in this repository is dual-licensed under either:

* MIT License ([LICENSE-MIT](LICENSE-MIT) or [http://opensource.org/licenses/MIT](http://opensource.org/licenses/MIT))
* Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or [http://www.apache.org/licenses/LICENSE-2.0](http://www.apache.org/licenses/LICENSE-2.0))

at your option.

### Your contributions

Unless you explicitly state otherwise,
any contribution intentionally submitted for inclusion in the work by you,
as defined in the Apache-2.0 license,
shall be dual licensed as above,
without any additional terms or conditions.
