//! Type definitions for window restoration.

use std::path::PathBuf;

use bevy::prelude::*;
use bevy::window::MonitorSelection;
use bevy::window::VideoMode;
use bevy::window::VideoModeSelection;
use bevy::window::WindowMode;
use serde::Deserialize;
use serde::Serialize;

/// Saved video mode for exclusive fullscreen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

/// Window decoration dimensions (title bar, borders).
pub struct WindowDecoration {
    pub width:  u32,
    pub height: u32,
}

/// Information from winit captured at startup.
#[derive(Resource)]
pub struct WinitInfo {
    pub starting_monitor_index: usize,
    pub window_decoration:      WindowDecoration,
}

impl WinitInfo {
    /// Get window decoration dimensions as a `UVec2`.
    #[must_use]
    pub const fn decoration(&self) -> UVec2 {
        UVec2::new(self.window_decoration.width, self.window_decoration.height)
    }
}

/// State for `MonitorScaleStrategy::HigherToLower` (high→low DPI restore).
///
/// When restoring from a high-DPI to low-DPI monitor, we must set position BEFORE size
/// because Bevy's `changed_windows` system processes size changes before position changes.
/// If we set both together, the window resizes first while still at the old position,
/// temporarily extending into the wrong monitor and triggering a scale factor bounce from macOS.
///
/// By moving a 1x1 window to the final position first, we ensure the window is already
/// at the correct location when we later apply size in `ApplySize`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowRestoreState {
    /// Position applied with compensation, waiting for `ScaleChanged` message.
    WaitingForScaleChange,
    /// Scale changed, ready to apply final size (position already set in phase 1).
    ApplySize,
}

/// State for fullscreen restore on Windows (DX12/DXGI workaround).
///
/// Exclusive fullscreen crashes on startup with DX12 due to DXGI flip model
/// limitations (see <https://github.com/rust-windowing/winit/issues/3124>).
/// We wait one frame for `create_surfaces` to create a windowed surface first,
/// then switch to fullscreen.
#[cfg(all(target_os = "windows", feature = "workaround-winit-3124"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FullscreenRestoreState {
    /// Waiting for first frame to render (surface creation).
    WaitingForSurface,
    /// Surface created, ready to apply fullscreen mode.
    ApplyFullscreen,
}

/// Restore strategy based on scale factor relationship between launch and target monitors.
///
/// # Platform Differences
///
/// Window coordinate handling differs significantly between platforms due to how winit
/// interacts with each OS's DPI system:
///
/// ## macOS
///
/// Winit's coordinate handling on macOS is fundamentally broken for multi-monitor setups
/// with different scale factors. Desktop/window coordinates are internally closer to
/// logical positions, but winit converts them using the *current* monitor's scale factor,
/// incorrectly assuming all monitors have the same scale.
///
/// This means:
/// - When setting position/size, values are divided by the launch monitor's scale
/// - We must compensate when scales differ between launch and target monitors
/// - The `LowerToHigher` and `HigherToLower` strategies exist to work around this
///
/// See: <https://github.com/rust-windowing/winit/issues/2645>
///
/// ## Windows
///
/// Windows handles DPI correctly at the OS level. Winit uses physical coordinates
/// directly without applying incorrect scale factor conversions. No compensation
/// is needed when moving windows between monitors with different scales.
///
/// On Windows, we always use `ApplyUnchanged` regardless of scale factor differences.
///
/// Note: Windows does have a separate issue where `GetWindowRect` includes an invisible
/// resize border (~7-11 pixels), causing reported positions to be slightly negative
/// when windows are snapped to screen edges. This is tracked in:
/// <https://github.com/rust-windowing/winit/issues/4107>
///
/// # Variants
///
/// - **`ApplyUnchanged`**: Apply position and size directly without compensation. Used on macOS
///   when scales match.
///
/// - **`CompensateSizeOnly`**: Windows only. Apply position directly, but compensate size by
///   multiplying by `starting_scale / target_scale`. This is needed because Bevy's
///   `set_physical_resolution` still applies scale conversion based on the current monitor, even
///   though Windows handles position coordinates correctly.
///
/// - **`LowerToHigher`**: macOS only. Low→High DPI (1x→2x, ratio < 1). Multiply both position and
///   size by ratio before applying so that after winit divides by launch scale, we get the correct
///   result.
///
/// - **`HigherToLower`**: macOS only. High→Low DPI (2x→1x, ratio > 1). Cannot use simple
///   compensation because the compensated size would exceed monitor bounds and get clamped by
///   macOS. Instead uses a two-phase approach via `WindowRestoreState`:
///   1. Move a 1x1 window to the final position (compensated) to trigger scale change
///   2. After scale changes, apply size without compensation (position already correct)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MonitorScaleStrategy {
    /// Same scale - apply position and size directly.
    ApplyUnchanged,
    /// Windows only: apply position directly, compensate size only.
    #[cfg(target_os = "windows")]
    CompensateSizeOnly,
    /// Low→High DPI (1x→2x) - apply with compensation (ratio < 1). macOS only.
    #[cfg(all(
        not(target_os = "windows"),
        feature = "workaround-macos-scale-compensation"
    ))]
    LowerToHigher,
    /// High→Low DPI (2x→1x) - requires two phases (see enum docs). macOS only.
    HigherToLower(WindowRestoreState),
}

/// Holds the target window state during the restore process.
///
/// All values are pre-computed with proper types. Casting from saved state
/// happens once during loading, not scattered throughout the restore logic.
///
/// Dimensions stored here are **inner** (content area only), matching what
/// Bevy's `Window.resolution` represents and what we save to the state file.
/// Outer dimensions (including title bar) are only used during loading for
/// clamping calculations.
#[derive(Resource)]
pub struct TargetPosition {
    /// Final clamped position (adjusted to fit within target monitor).
    /// None on Wayland where clients can't access window position.
    pub position:                 Option<IVec2>,
    /// Target width (content area, excluding window decoration).
    pub width:                    u32,
    /// Target height (content area, excluding window decoration).
    pub height:                   u32,
    /// Scale factor of the target monitor.
    pub target_scale:             f64,
    /// Scale factor of the monitor where the window starts (keyboard focus monitor).
    pub starting_scale:           f64,
    /// Strategy for handling scale factor differences between monitors.
    pub monitor_scale_strategy:   MonitorScaleStrategy,
    /// Window mode to restore.
    pub mode:                     SavedWindowMode,
    /// Fullscreen restore state (Windows only, DX12/DXGI workaround).
    #[cfg(all(target_os = "windows", feature = "workaround-winit-3124"))]
    pub fullscreen_restore_state: FullscreenRestoreState,
}

impl TargetPosition {
    /// Get the target position as an `IVec2`, if available.
    #[must_use]
    pub const fn position(&self) -> Option<IVec2> { self.position }

    /// Get the target size as a `UVec2`.
    #[must_use]
    pub const fn size(&self) -> UVec2 { UVec2::new(self.width, self.height) }

    /// Scale ratio between starting and target monitors.
    #[cfg(any(target_os = "windows", feature = "workaround-macos-scale-compensation"))]
    #[must_use]
    pub fn ratio(&self) -> f64 { self.starting_scale / self.target_scale }

    /// Position compensated for scale factor differences.
    ///
    /// Multiplies position by the ratio to account for winit dividing by launch scale.
    /// Returns None if position is not available (Wayland).
    #[cfg(all(
        not(target_os = "windows"),
        feature = "workaround-macos-scale-compensation"
    ))]
    #[must_use]
    pub fn compensated_position(&self) -> Option<IVec2> {
        let ratio = self.ratio();
        self.position.map(|pos| {
            IVec2::new(
                (f64::from(pos.x) * ratio) as i32,
                (f64::from(pos.y) * ratio) as i32,
            )
        })
    }

    /// Size compensated for scale factor differences.
    ///
    /// Multiplies size by the ratio to account for winit dividing by launch scale.
    #[cfg(any(target_os = "windows", feature = "workaround-macos-scale-compensation"))]
    #[must_use]
    pub fn compensated_size(&self) -> UVec2 {
        let ratio = self.ratio();
        UVec2::new(
            (f64::from(self.width) * ratio) as u32,
            (f64::from(self.height) * ratio) as u32,
        )
    }
}

/// Configuration for the `RestoreWindowPlugin`.
#[derive(Resource, Clone)]
pub struct RestoreWindowConfig {
    /// Full path to the state file.
    pub path: PathBuf,
}

/// Saved window state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowState {
    pub position:      Option<(i32, i32)>,
    pub width:         u32,
    pub height:        u32,
    pub monitor_index: usize,
    pub mode:          SavedWindowMode,
    #[serde(default)]
    pub app_name:      String,
}
