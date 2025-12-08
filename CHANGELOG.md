# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added

- Fullscreen mode detection and restoration (windowed, borderless, exclusive with video mode)
- macOS crash fix: workaround for panic when quitting from exclusive fullscreen mode (will be fixed upstream in https://github.com/bevyengine/bevy/pull/22060)

## [0.17.0] - 2025-12-06

Initial release.

### Added

- `RestoreWindowsPlugin` for saving and restoring window position and size across sessions
- Multi-monitor support with proper scale factor handling
- Automatic state persistence to platform-specific config directories
- Simple example demonstrating plugin usage

[Unreleased]: https://github.com/natemccoy/bevy_restore_windows/compare/v0.17.0...HEAD
[0.17.0]: https://github.com/natemccoy/bevy_restore_windows/releases/tag/v0.17.0
