//! Window restoration logic.
//!
//! Handles startup state loading, cross-DPI moves, and final restore application.

mod bootstrap;
mod cross_dpi;
mod restore;

pub use bootstrap::init_winit_info;
pub use bootstrap::load_target_position;
#[cfg(target_os = "linux")]
pub use bootstrap::move_to_target_monitor;
pub use restore::restore_windows;
