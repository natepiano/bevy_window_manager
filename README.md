# bevy_window_manager

[![License](https://img.shields.io/badge/license-MIT%2FApache-blue.svg)](https://github.com/natemccoy/bevy_window_manager#license)
[![Crates.io](https://img.shields.io/crates/v/bevy_window_manager.svg)](https://crates.io/crates/bevy_window_manager)
[![Downloads](https://img.shields.io/crates/d/bevy_window_manager.svg)](https://crates.io/crates/bevy_window_manager)


A Bevy plugin for window state persistence and multi-monitor utilities.

## Motivation

On macOS with multiple monitors that have different scale factors (e.g., a MacBook Pro Retina display at scale 2.0 and an external monitor at scale 1.0), Bevy's window positioning has issues with scale factor conversion that corrupt window size and position when attempting to restore a window to its last known position when launching from a monitor with a different scale factor.

This plugin works around those issues by using winit directly to capture actual window positions and compensate for scale factor conversions.

See the documentation in [`src/lib.rs`](src/lib.rs) for technical details.

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
| Windows  | Untested |
| Linux    | Untested |

**Warning:** This plugin was developed for and tested on macOS. It may work on Windows and Linux, but there are no guarantees as I don't yet have a setup to test on those platforms. If you try it and it works for you, please let me know. If it doesn't work, PR's welcome!

## macOS Fullscreen Crash Fix

This plugin includes a workaround for a Bevy bug on macOS where quitting the application while in exclusive fullscreen mode causes a panic. The crash occurs because Bevy stores windows in thread-local storage (TLS), and when windows are dropped during TLS destruction, winit's cleanup code triggers a macOS callback that tries to access already-destroyed TLS.

This plugin prevents the crash by exiting fullscreen before TLS destruction begins. See [docs/bevy-issue-macos-fullscreen-panic.md](docs/bevy-issue-macos-fullscreen-panic.md) for full technical details.

This issue will be fixed upstream in Bevy: https://github.com/bevyengine/bevy/pull/22060

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
