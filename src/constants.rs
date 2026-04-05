//! Cross-module constants.

/// Current persisted state format version.
pub(crate) const CURRENT_STATE_VERSION: u8 = 2;

/// Key used for the primary window in the state file.
pub(crate) const PRIMARY_WINDOW_KEY: &str = "primary";

/// Threshold for considering two scale factors equal.
///
/// Accounts for floating-point imprecision when comparing scale factors.
/// A difference less than this epsilon is considered negligible.
pub(crate) const SCALE_FACTOR_EPSILON: f64 = 0.01;

/// Duration (in seconds) that all values must remain stable before declaring success.
pub(crate) const SETTLE_STABILITY_SECS: f32 = 0.2;

/// Maximum total duration (in seconds) to wait for values to stabilize.
pub(crate) const SETTLE_TIMEOUT_SECS: f32 = 1.0;

/// Default state file name for window persistence.
pub(crate) const STATE_FILE: &str = "windows.ron";
