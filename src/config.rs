//! Restore configuration.

use std::path::PathBuf;

use bevy::prelude::*;

use crate::WindowKey;
use crate::saved::WindowState;

/// Configuration for the `RestoreWindowPlugin`.
#[derive(Resource, Clone)]
pub(crate) struct RestoreWindowConfig {
    /// Full path to the state file.
    pub path:          PathBuf,
    /// Snapshot of window states as loaded from the file at startup.
    /// Populated during restore so downstream code can compare intended vs actual state.
    /// Entries persist as a read-only snapshot for the example's File column.
    pub loaded_states: std::collections::HashMap<WindowKey, WindowState>,
}
