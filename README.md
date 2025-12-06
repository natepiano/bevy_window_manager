# bevy_restore_window

[![License](https://img.shields.io/badge/license-MIT%2FApache-blue.svg)](https://github.com/natemccoy/bevy_restore_window#license)
[![Crates.io](https://img.shields.io/crates/v/bevy_restore_window.svg)](https://crates.io/crates/bevy_restore_window)
[![Downloads](https://img.shields.io/crates/d/bevy_restore_window.svg)](https://crates.io/crates/bevy_restore_window)


A Bevy plugin that saves and restores the primary window position and size across application sessions.

## Motivation

On macOS with multiple monitors that have different scale factors (e.g., a MacBook Pro Retina display at scale 2.0 and an external monitor at scale 1.0), Bevy's window positioning has issues with scale factor conversion that corrupt window size and position when moving between monitors.

This plugin works around those issues by using winit directly to capture actual window positions and compensate for scale factor conversions.

See the documentation in [`src/lib.rs`](src/lib.rs) for technical details.

## Usage

```rust
use bevy::prelude::*;
use bevy_restore_window::RestoreWindowPlugin;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(RestoreWindowPlugin::new("my_app"))
        .run();
}
```

## Version Compatibility

| bevy_restore_window | Bevy |
|---------------------|------|
| 0.17                | 0.17 |

## Platform Support

| Platform | Status |
|----------|--------|
| macOS    | Tested |
| Windows  | Untested |
| Linux    | Untested |

**Warning:** This plugin was developed for and tested on macOS only. It may work on Windows and Linux, but there are no guarantees as I don't have a setup to test on those platforms. PR's welcome!

## License

bevy_restore_window is free, open source and permissively licensed!
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
