//! Cross-module constants.

/// Threshold for considering two scale factors equal.
///
/// Accounts for floating-point imprecision when comparing scale factors.
/// A difference less than this epsilon is considered negligible.
pub(super) const SCALE_FACTOR_EPSILON: f64 = 0.01;

/// Key used for the primary window in the state file.
pub(super) const PRIMARY_WINDOW_KEY: &str = "primary";

/// Current persisted state format version.
pub(super) const CURRENT_STATE_VERSION: u8 = 2;

/// Duration (in seconds) that all values must remain stable before declaring success.
pub(super) const SETTLE_STABILITY_SECS: f32 = 0.2;

/// Maximum total duration (in seconds) to wait for values to stabilize.
pub(super) const SETTLE_TIMEOUT_SECS: f32 = 1.0;

/// Default state file name for window persistence.
pub(super) const STATE_FILE: &str = "windows.ron";
