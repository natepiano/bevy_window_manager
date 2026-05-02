//! Window restore startup, target state, and settle verification.

mod settle_state;
mod target_position;
mod winit_info;

pub(crate) use settle_state::check_restore_settling;
pub(crate) use target_position::FullscreenRestoreState;
pub(crate) use target_position::MonitorResolutionSource;
pub(crate) use target_position::MonitorScaleStrategy;
pub(crate) use target_position::TargetPosition;
pub(crate) use target_position::WindowRestoreState;
pub(crate) use target_position::compute_target_position;
pub(crate) use target_position::resolve_target_monitor_and_position;
pub(crate) use target_position::restore_windows;
pub(crate) use winit_info::WinitInfo;
pub(crate) use winit_info::X11FrameCompensated;
pub(crate) use winit_info::init_winit_info;
pub(crate) use winit_info::load_target_position;
pub(crate) use winit_info::move_to_target_monitor;
