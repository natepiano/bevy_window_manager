//! Cross-module constants.

// Persistence
pub(crate) const CURRENT_STATE_VERSION: u8 = 2;
pub(crate) const PRIMARY_WINDOW_KEY: &str = "primary";
pub(crate) const STATE_FILE: &str = "windows.ron";

// Scale factor
/// Fallback scale factor when the monitor cannot be determined.
pub(crate) const DEFAULT_SCALE_FACTOR: f64 = 1.0;
/// Threshold for considering two scale factors equal.
///
/// Accounts for floating-point imprecision when comparing scale factors.
/// A difference less than this epsilon is considered negligible.
pub(crate) const SCALE_FACTOR_EPSILON: f64 = 0.01;

// State format
/// Header comment prepended to the RON file to document the coordinate contract.
pub(crate) const RON_HEADER: &str = "\
// All spatial values (position, size) are in logical pixels.
// monitor_scale: scale factor at save time (informational, not used during restore).
";

// Windows DPI fix
/// Win32 subclass identifier for DPI-change interception.
#[cfg(all(target_os = "windows", feature = "workaround-winit-4341"))]
pub(crate) const SUBCLASS_ID: usize = 1;

// X11 frame extents (`_NET_FRAME_EXTENTS`: left, right, top, bottom)
/// Number of values in `_NET_FRAME_EXTENTS` (left, right, top, bottom).
#[cfg(all(target_os = "linux", feature = "workaround-winit-4445"))]
pub(crate) const FRAME_EXTENT_COUNT: usize = 4;
/// Index of the "top" extent in `_NET_FRAME_EXTENTS`.
#[cfg(all(target_os = "linux", feature = "workaround-winit-4445"))]
pub(crate) const FRAME_EXTENT_TOP_INDEX: usize = 2;

// Settle timing
/// Duration (in seconds) that all values must remain stable before declaring success.
pub(crate) const SETTLE_STABILITY_SECS: f32 = 0.2;
/// Maximum total duration (in seconds) to wait for values to stabilize.
pub(crate) const SETTLE_TIMEOUT_SECS: f32 = 1.0;
