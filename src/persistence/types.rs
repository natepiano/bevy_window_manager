//! Saved/persistence types for window state serialization.

use bevy::prelude::*;
use bevy::window::MonitorSelection;
use bevy::window::VideoMode;
use bevy::window::VideoModeSelection;
use bevy::window::WindowMode;
use serde::Deserialize;
use serde::Serialize;

use crate::constants::DEFAULT_SCALE_FACTOR;

/// Saved video mode for exclusive fullscreen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Reflect)]
pub struct SavedVideoMode {
    pub physical_size:           UVec2,
    pub bit_depth:               u16,
    pub refresh_rate_millihertz: u32,
}

impl SavedVideoMode {
    /// Convert to Bevy's `VideoMode`.
    #[must_use]
    pub const fn to_video_mode(&self) -> VideoMode {
        VideoMode {
            physical_size:           self.physical_size,
            bit_depth:               self.bit_depth,
            refresh_rate_millihertz: self.refresh_rate_millihertz,
        }
    }
}

/// Serializable window mode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Reflect)]
pub enum SavedWindowMode {
    Windowed,
    BorderlessFullscreen,
    /// Exclusive fullscreen with optional specific video mode.
    Fullscreen {
        /// Video mode if explicitly set (`None` = use current display mode).
        video_mode: Option<SavedVideoMode>,
    },
}

impl SavedWindowMode {
    /// Convert to Bevy's `WindowMode` with the given monitor index.
    #[must_use]
    pub const fn to_window_mode(&self, monitor_index: usize) -> WindowMode {
        let selection = MonitorSelection::Index(monitor_index);
        match self {
            Self::Windowed => WindowMode::Windowed,
            Self::BorderlessFullscreen => WindowMode::BorderlessFullscreen(selection),
            Self::Fullscreen { video_mode: None } => {
                WindowMode::Fullscreen(selection, VideoModeSelection::Current)
            },
            Self::Fullscreen {
                video_mode: Some(saved),
            } => WindowMode::Fullscreen(
                selection,
                VideoModeSelection::Specific(saved.to_video_mode()),
            ),
        }
    }

    /// Check if this is a fullscreen mode (borderless or exclusive).
    #[must_use]
    pub const fn is_fullscreen(&self) -> bool { !matches!(self, Self::Windowed) }
}

impl From<&WindowMode> for SavedWindowMode {
    fn from(mode: &WindowMode) -> Self {
        match mode {
            WindowMode::Windowed => Self::Windowed,
            WindowMode::BorderlessFullscreen(_) => Self::BorderlessFullscreen,
            WindowMode::Fullscreen(_, video_mode_selection) => Self::Fullscreen {
                video_mode: match video_mode_selection {
                    VideoModeSelection::Current => None,
                    VideoModeSelection::Specific(mode) => Some(SavedVideoMode {
                        physical_size:           mode.physical_size,
                        bit_depth:               mode.bit_depth,
                        refresh_rate_millihertz: mode.refresh_rate_millihertz,
                    }),
                },
            },
        }
    }
}

/// Saved window state persisted to the RON file.
///
/// All spatial values are in **logical pixels** — they represent the user's visual intent
/// and are independent of scale factor. On restore, both position and size are converted
/// to physical pixels using the target monitor's scale factor in
/// [`compute_target_position`](crate::restore::plan::compute_target_position).
///
/// `monitor_scale` records the scale factor of the monitor at save time. It is informational
/// only — restore uses the target monitor's live scale factor, not this saved value.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowState {
    /// Top-left corner of the window content area in logical pixels.
    /// `None` on Wayland where clients cannot access window position.
    #[serde(alias = "position")]
    pub logical_position: Option<(i32, i32)>,
    /// Content area width in logical pixels (excludes window decoration).
    pub logical_width:    u32,
    /// Content area height in logical pixels (excludes window decoration).
    pub logical_height:   u32,
    /// Scale factor of the monitor at save time (informational, not used during restore).
    #[serde(default = "default_monitor_scale")]
    pub monitor_scale:    f64,
    pub monitor_index:    usize,
    pub mode:             SavedWindowMode,
    #[serde(default)]
    pub app_name:         String,
}

/// Default monitor scale for deserialization of legacy files missing the field.
const fn default_monitor_scale() -> f64 { DEFAULT_SCALE_FACTOR }
