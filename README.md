# bevy_window_manager

[![License](https://img.shields.io/badge/license-MIT%2FApache-blue.svg)](https://github.com/natemccoy/bevy_window_manager#license)
[![Crates.io](https://img.shields.io/crates/v/bevy_window_manager.svg)](https://crates.io/crates/bevy_window_manager)
[![Downloads](https://img.shields.io/crates/d/bevy_window_manager.svg)](https://crates.io/crates/bevy_window_manager)
[![CI](https://github.com/natepiano/bevy_window_manager/actions/workflows/ci.yml/badge.svg)](https://github.com/natepiano/bevy_window_manager/actions/workflows/ci.yml)


A Bevy plugin for window state persistence and multi-monitor utilities.

## Motivation

Originally created as a mechanism to restore the `PrimaryWindow` to its last known position when launching - the way you expect an app to work. I quickly discovered that on my `MacBook` Pro with Retina display (scale factor 2.0) and my external monitor (scale factor 1.0), there were numerous issues with saving/restoring positions across differently-scaled monitors. 

The first discovered issue is that winit uses the scale factor of the focused window from which you launch the application. And if the target monitor for the app has a different scale factor, then that will get factored into the size and position calculations resulting in something you definitely don't want.

`bevy_window_manager` plugin works around this issue by using winit directly to capture actual monitor position/size/scale and comparing it to the target position/size for the window and does the conversions correctly.

Windows has similar scale factor issues, plus additional quirks like invisible window borders that prevent precise placement. Linux X11 has its own quirks with window manager keyboard shortcuts not firing position events. This plugin now supports macOS, Windows, and Linux (X11 and Wayland) with workarounds for platform-specific issues (see [Platform Support](#platform-support) for details).

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

For a complete interactive example with fullscreen mode switching, run:

```bash
cargo run --example restore_window
```

## API

This crate exposes several types for working with monitors and windows beyond the plugin itself. See [docs.rs](https://docs.rs/bevy_window_manager) for full API documentation.

### `Monitors` Resource

Query available monitors sorted by position:

- `monitors.at(x, y)` – Find the monitor containing a position
- `monitors.by_index(index)` – Get monitor by sorted index
- `monitors.first()` – Get the first monitor (index 0)
- `monitors.closest_to(x, y)` – Find the closest monitor to a position

### `MonitorInfo`

Information about a single monitor: `index`, `scale`, `position`, and `size`.

### `CurrentMonitor` Component

Automatically maintained on all managed windows. Query it to get monitor info and effective mode:

```rust
fn my_system(q: Query<(&Window, &CurrentMonitor), With<PrimaryWindow>>) {
    let (window, monitor) = q.single();
    println!("Monitor {}, scale {}", monitor.index, monitor.scale);
    println!("Effective mode: {:?}", monitor.effective_mode);
}
```

- `monitor.index`, `monitor.scale`, `monitor.position`, `monitor.size` – Monitor info (via `Deref<Target = MonitorInfo>`)
- `monitor.effective_mode` – The actual window mode, even when `window.mode` is stale (e.g., macOS green button fullscreen reports `Windowed` but the window is actually fullscreen)

### Plugin Configuration

- `WindowManagerPlugin` – Uses executable name for config directory
- `WindowManagerPlugin::with_app_name("name")` – Custom app name
- `WindowManagerPlugin::with_path(path)` – Full control over state file path
- `WindowManagerPlugin::with_persistence(mode)` – Set persistence behavior for managed windows

### Multi-Window Support

Add `ManagedWindow` to any secondary window to opt it into save/restore:

```rust
use bevy_window_manager::ManagedWindow;

commands.spawn((
    Window {
        title: "Inspector".into(),
        ..default()
    },
    ManagedWindow {
        window_name: "inspector".to_string(),
    },
));
```

Each managed window gets the same restore treatment as the primary window — scale factor compensation, position clamping, and platform workarounds.

Control what happens when windows are closed with `ManagedWindowPersistence`:

- `RememberAll` (default) — closed windows keep their saved state for next launch
- `ActiveOnly` — only currently open windows are persisted

```rust
use bevy_window_manager::ManagedWindowPersistence;
use bevy_window_manager::WindowManagerPlugin;

app.add_plugins(WindowManagerPlugin::with_persistence(ManagedWindowPersistence::ActiveOnly));
```

See `examples/restore_window.rs` for a complete interactive example.

### State File Format

The state file uses a versioned v2 schema:

- `version: 2`
- `entries: [{ key, state }, ...]`

All spatial values (position, size) are stored in **logical pixels**, making them independent
of monitor scale factor. On restore, values are converted to physical pixels using the target
monitor's live scale factor.

`key` is typed (`Primary` or `Managed("<name>")`), so the primary window and a managed
window named `"primary"` are distinct and unambiguous.

Legacy state files (unversioned and v1) are still accepted on read and migrated to v2 on save.

## Version Compatibility

| bevy_window_manager | Bevy |
|---------------------|------|
| 0.19                | 0.18 |
| 0.18                | 0.18 |
| 0.17                | 0.17 |

## Platform Support

| Platform | Status | Notes |
|----------|--------|-------|
| macOS    | ✅ Tested | Native hardware with multiple monitors at different scales |
| Windows  | ✅ Tested | `VMware` VM with multi-monitor, different scale factors |
| Linux X11 | ✅ Tested | Position and size restoration with keyboard snap workaround |
| Linux Wayland | ✅ Tested | Size + fullscreen only (Wayland cannot query/set position) |


**Note on Windows testing**: Windows support has been tested in a `VMware` virtual machine with multiple monitors at different scale factors. Native Windows installations may behave differently - if you encounter issues, please open an issue with details about your monitor configuration.

**Note on Linux support**: Linux support has been tested on KDE Plasma (Asahi Linux on Fedora). X11 includes a workaround for keyboard snap shortcuts (Meta+Arrow) that don't fire position events ([winit #4443](https://github.com/rust-windowing/winit/issues/4443)). Wayland has an inherent limitation: clients cannot query or set window position, so only size and fullscreen state can be restored. If you encounter issues, please open an issue with details about your distribution, desktop environment, and monitor configuration.

## Feature Flags (Platform Workarounds)

This plugin includes workarounds for known issues in winit and Bevy. Each workaround is behind a feature flag, and **all are enabled by default**.

This design allows:
- **Easy testing of upstream fixes** - disable a workaround to verify an upstream fix works
- **Opt-out flexibility** - if a workaround doesn't suit your setup, you can exclude it
- **Minimal code when not needed** - platform-specific workarounds are compiled out on other platforms

### Available Feature Flags

| Feature | Platform | Issue | Description |
|---------|----------|-------|-------------|
| `workaround-winit-4341` | Windows | [winit #4041](https://github.com/rust-windowing/winit/issues/4041) | DPI drag bounce fix |
| `workaround-winit-3124` | Windows | [winit #3124](https://github.com/rust-windowing/winit/issues/3124) | DX12/DXGI fullscreen crash fix |
| `workaround-winit-4443` | Linux X11 | [winit #4443](https://github.com/rust-windowing/winit/issues/4443) | Keyboard snap position fix |
| `workaround-winit-4440` | Windows, macOS, Linux X11 | [winit #4440](https://github.com/rust-windowing/winit/issues/4440) | Multi-monitor scale factor compensation |

### Disabling Workarounds

To test without a specific workaround (e.g., to verify an upstream fix):

```bash
# Disable all workarounds
cargo run --example restore_window --no-default-features
```

In your `Cargo.toml`, you can selectively enable features:

```toml
[dependencies]
bevy_window_manager = { version = "0.19", default-features = false, features = ["workaround-winit-4341"] }
```

## License

`bevy_window_manager` is free, open source and permissively licensed!
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
