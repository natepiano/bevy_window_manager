# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

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

[Unreleased]: https://github.com/natepiano/bevy_window_manager/compare/v0.17.0...HEAD
[0.17.0]: https://github.com/natepiano/bevy_window_manager/releases/tag/v0.17.0
