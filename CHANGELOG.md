# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Removed

- Internal `FullscreenExitGuard` workaround for macOS exclusive fullscreen crash - now fixed upstream in Bevy 0.18 ([bevy #22060](https://github.com/bevyengine/bevy/pull/22060))

## [0.17.2] - 2025-12-20

### Added

- Linux X11 support with position and size restoration
- Linux Wayland support with size and fullscreen restoration (position not available on Wayland)
- X11 keyboard snap position fix: workaround for missing `Moved` events when window manager moves window via keyboard shortcuts like Meta+Arrow ([winit #4443](https://github.com/rust-windowing/winit/issues/4443), related [bevy #17576](https://github.com/bevyengine/bevy/issues/17576)). Controlled by `workaround-winit-4443` feature flag.

## [0.17.1] - 2025-12-15

### Added

- Windows platform support with proper multi-monitor window restore
- Windows DPI drag fix: workaround for window bouncing/resizing bug when dragging between monitors with different scale factors ([winit #4041](https://github.com/rust-windowing/winit/issues/4041), fix in [PR #4341](https://github.com/rust-windowing/winit/pull/4341) not yet released)
- macOS drag-back size fix: workaround for window size resetting when dragging back to launch monitor after cross-DPI restore ([winit #4441](https://github.com/rust-windowing/winit/issues/4441)). **Trade-off:** Causes a brief visual flash in this rare scenario (launching from high-scale, restoring to low-scale, then dragging back to high-scale). Controlled by `workaround-macos-drag-back-reset` feature flag.
- `app_name` field in `WindowState` to track which application saved the state file

### Fixed

- macOS high→low DPI restore no longer flashes incorrect size on first frame. Window is hidden during two-phase restore and shown after correct size is applied. **Note:** When restoring from high-DPI to low-DPI monitor, the first frame will not be visible.
- Window state now saves when video mode refresh rate changes (e.g., switching from 75Hz to 60Hz at same resolution)
- Monitor detection for maximized/snapped windows now uses window center instead of top-left, which could fall outside visible monitor bounds due to Windows invisible border offset ([winit #4296](https://github.com/rust-windowing/winit/issues/4296))
- Windows position restoration accounts for invisible border offset (workaround for [winit #4107](https://github.com/rust-windowing/winit/issues/4107))
- Fullscreen windows now correctly restore to the saved target monitor on all platforms
- Windows exclusive fullscreen restore now waits one frame for surface creation (workaround for [winit #3124](https://github.com/rust-windowing/winit/issues/3124), [bevy #5485](https://github.com/bevyengine/bevy/issues/5485))

## [0.17.0] - 2025-12-08

### Added

- `WindowManagerPlugin` for saving and restoring window position and size across sessions
- Multi-monitor support with proper scale factor handling
- Automatic state persistence to platform-specific config directories
- Fullscreen mode detection and restoration (windowed, borderless, exclusive with video mode)
- macOS crash fix: workaround for panic when quitting from exclusive fullscreen mode (will be fixed upstream in https://github.com/bevyengine/bevy/pull/22060)
- `Monitors` resource for querying available monitors by position or index
- `MonitorInfo` struct exposing monitor scale, position, and size
- `WindowExt` extension trait for window-to-monitor queries and effective mode detection

[Unreleased]: https://github.com/natepiano/bevy_window_manager/compare/v0.17.2...HEAD
[0.17.2]: https://github.com/natepiano/bevy_window_manager/compare/v0.17.1...v0.17.2
[0.17.1]: https://github.com/natepiano/bevy_window_manager/compare/v0.17.0...v0.17.1
[0.17.0]: https://github.com/natepiano/bevy_window_manager/releases/tag/v0.17.0
