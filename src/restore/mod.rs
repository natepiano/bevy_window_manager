//! Window restore pipeline: target definition, planning, execution, and verification.

mod apply;
mod plan;
mod settle;
mod target;

pub(crate) use apply::init_winit_info;
pub(crate) use apply::load_target_position;
#[cfg(target_os = "linux")]
pub(crate) use apply::move_to_target_monitor;
pub(crate) use apply::restore_windows;
pub(crate) use plan::MonitorResolutionSource;
pub(crate) use plan::compute_target_position;
pub(crate) use plan::resolve_target_monitor_and_position;
pub(crate) use settle::check_restore_settling;
pub(crate) use target::FullscreenRestoreState;
pub(crate) use target::MonitorScaleStrategy;
pub(crate) use target::TargetPosition;
pub(crate) use target::WindowRestoreState;
pub(crate) use target::WinitInfo;
pub(crate) use target::X11FrameCompensated;
