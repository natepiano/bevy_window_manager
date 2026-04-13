//! Systems for window restoration and state management.
//!
//! # Monitor Detection
//!
//! [`update_current_monitor`] is the unified system that maintains `CurrentMonitor` on all
//! managed windows. It uses winit's `current_monitor()` as the primary detection method,
//! with position-based center-point detection as a fallback. This ensures correct monitor
//! identification even for newly spawned windows whose `window.position` is still `Automatic`.
//!
//! On Wayland, `window.position` always returns `(0,0)` for security/privacy reasons, making
//! winit's `current_monitor()` the only viable detection method on that platform.

mod monitor;
mod restore;
mod settle;

pub(crate) use monitor::update_current_monitor;
pub(crate) use restore::init_winit_info;
pub(crate) use restore::load_target_position;
#[cfg(target_os = "linux")]
pub(crate) use restore::move_to_target_monitor;
pub(crate) use restore::restore_windows;
pub(crate) use settle::check_restore_settling;
