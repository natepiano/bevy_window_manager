//! Type definitions for window restoration.

use std::path::PathBuf;

use bevy::prelude::*;
use bevy::window::MonitorSelection;
use bevy::window::VideoMode;
use bevy::window::VideoModeSelection;
use bevy::window::WindowMode;
use serde::Deserialize;
use serde::Serialize;

use crate::WindowKey;

/// Event fired when a window restore completes and the window becomes visible.
///
/// This is an [`EntityEvent`] triggered on the window entity at the end of the restore
/// process, after position, size, and mode have been applied. Dependent crates can
/// observe this event to know the final restored window state.
///
/// Use an observer to receive this event:
/// ```ignore
/// // For all windows
/// app.add_observer(|trigger: On<WindowRestored>| {
///     let event = trigger.event();
///     // Use event.entity, event.size, event.mode, etc.
/// });
///
/// // For primary window only - check event.entity against PrimaryWindow query
/// fn on_window_restored(
///     trigger: On<WindowRestored>,
///     primary_window: Query<(), With<PrimaryWindow>>,
/// ) {
///     let event = trigger.event();
///     if primary_window.get(event.entity).is_ok() {
///         // Handle primary window only
///     }
/// }
/// ```
#[derive(EntityEvent, Debug, Clone, Reflect)]
pub struct WindowRestored {
    /// The window entity this event targets.
    pub entity:        Entity,
    /// Identifier for this window (primary or managed name).
    pub window_id:     WindowKey,
    /// Target position that was applied (None on Wayland).
    pub position:      Option<IVec2>,
    /// Target size that was applied (content area).
    pub size:          UVec2,
    /// Window mode that was applied.
    pub mode:          WindowMode,
    /// Monitor index the window was restored to.
    pub monitor_index: usize,
}

/// Threshold for considering two scale factors equal.
///
/// Accounts for floating-point imprecision when comparing scale factors.
/// A difference less than this epsilon is considered negligible.
pub const SCALE_FACTOR_EPSILON: f64 = 0.01;

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

/// Token indicating X11 frame extent compensation is complete (W6 workaround).
///
/// This component gates `restore_windows` - the restore system cannot process
/// a window until this token exists on the entity. On Linux X11 with W6 workaround
/// enabled, this ensures frame extents are queried and position is compensated
/// before restore proceeds. On other platforms/configurations, the token is
/// inserted immediately during `load_target_position` since no compensation is needed.
#[derive(Component)]
pub struct X11FrameCompensated;

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
    /// Initial state: window needs to be moved to the target monitor to trigger a scale change.
    /// Handled by `restore_windows` which calls `apply_initial_move` and transitions to
    /// `WaitingForScaleChange`. This unified entry point replaces the old separate paths
    /// (PreStartup `move_to_target_monitor` for primary, inline guard for managed).
    NeedInitialMove,
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
/// # The Problem
///
/// Winit's `request_inner_size` and `set_outer_position` use the current window's scale factor
/// when interpreting coordinates, rather than the target monitor's scale factor. This causes
/// incorrect sizing/positioning when restoring windows across monitors with different DPIs.
///
/// See: <https://github.com/rust-windowing/winit/issues/4440>
///
/// # Platform Differences
///
/// ## Windows
///
/// - **Position**: Winit uses physical coordinates directly - no compensation needed
/// - **Size**: Winit applies scale conversion using current monitor's scale - needs compensation
/// - Strategy: `CompensateSizeOnly` when scales differ
///
/// Note: Windows has a separate issue where `GetWindowRect` includes an invisible
/// resize border (~7-11 pixels). See: <https://github.com/rust-windowing/winit/issues/4107>
///
/// ## macOS / Linux X11
///
/// - **Position**: Winit converts using current monitor's scale - needs compensation
/// - **Size**: Winit converts using current monitor's scale - needs compensation
/// - Strategy: `LowerToHigher` or `HigherToLower` depending on scale relationship
///
/// ## Linux Wayland
///
/// Cannot detect starting monitor or set position, so no compensation is applied.
///
/// # Variants
///
/// - **`ApplyUnchanged`**: Apply position and size directly without compensation.
///
/// - **`CompensateSizeOnly`**: Windows only. Apply position directly, compensate size by
///   multiplying by `starting_scale / target_scale`.
///
/// - **`LowerToHigher`**: macOS/Linux X11. Low→High DPI (1x→2x, ratio < 1). Multiply both position
///   and size by ratio.
///
/// - **`HigherToLower`**: macOS/Linux X11. High→Low DPI (2x→1x, ratio > 1). Uses two-phase approach
///   via `WindowRestoreState` to avoid size clamping:
///   1. Move a 1x1 window to final position (compensated) to trigger scale change
///   2. After scale changes, apply size without compensation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MonitorScaleStrategy {
    /// Same scale - apply position and size directly.
    ApplyUnchanged,
    /// Windows only: apply position directly, compensate size only.
    #[cfg(all(target_os = "windows", feature = "workaround-winit-4440"))]
    CompensateSizeOnly,
    /// Low→High DPI (1x→2x) - apply with compensation (ratio < 1). macOS only.
    #[cfg(all(not(target_os = "windows"), feature = "workaround-winit-4440"))]
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
#[derive(Component)]
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
    /// Target monitor index for fullscreen restore.
    /// On non-Wayland platforms, this could be derived from position, but Wayland
    /// doesn't provide window position, so we store it explicitly.
    pub target_monitor_index:     usize,
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
    #[cfg(feature = "workaround-winit-4440")]
    #[must_use]
    pub fn ratio(&self) -> f64 { self.starting_scale / self.target_scale }

    /// Position compensated for scale factor differences.
    ///
    /// Multiplies position by the ratio to account for winit dividing by launch scale.
    /// Returns None if position is not available (Wayland).
    #[cfg(all(not(target_os = "windows"), feature = "workaround-winit-4440"))]
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
    #[cfg(feature = "workaround-winit-4440")]
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
    pub path:          PathBuf,
    /// Snapshot of window states as loaded from the file at startup.
    /// Populated during restore so downstream code can compare intended vs actual state.
    /// Entries persist as a read-only snapshot for the example's File column.
    pub loaded_states: std::collections::HashMap<WindowKey, WindowState>,
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

/// Marks a window entity as managed by the window manager plugin.
///
/// Add this component to any secondary window entity to opt into automatic
/// save/restore behavior. The primary window is always managed automatically
/// using the key `"primary"` in the state file.
///
/// Each managed window must have a unique `window_name`. Duplicate names
/// will cause a panic.
///
/// # Example
///
/// ```ignore
/// commands.spawn((
///     Window { title: "Inspector".into(), ..default() },
///     ManagedWindow { window_name: "inspector".into() },
/// ));
/// ```
#[derive(Component, Clone, Reflect)]
#[reflect(Component)]
pub struct ManagedWindow {
    /// Unique name used as the key in the state file.
    pub window_name: String,
}

/// Controls what happens to saved state when a managed window is despawned.
///
/// Set as a resource on the app to control persistence behavior for all windows.
#[derive(Resource, Default, Clone, Debug, PartialEq, Eq, Reflect)]
#[reflect(Resource)]
pub enum ManagedWindowPersistence {
    /// Default: saved state persists even if window is closed during the session.
    /// All windows ever opened are remembered in the state file.
    #[default]
    RememberAll,
    /// Only windows open at time of save are persisted.
    /// Closing a window removes its entry from the state file.
    ActiveOnly,
}

/// Internal registry to track managed window names and detect duplicates.
#[derive(Resource, Default)]
pub struct ManagedWindowRegistry {
    /// Set of registered window names (for duplicate detection).
    pub names:    std::collections::HashSet<String>,
    /// Map from entity to window name (for cleanup on removal).
    pub entities: std::collections::HashMap<Entity, String>,
}
