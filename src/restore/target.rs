//! Restore target types: target position, scale strategies, winit info.

use bevy::prelude::*;
use bevy_kana::ToI32;
use bevy_kana::ToU32;

use super::settle::SettleState;
use crate::persistence::SavedWindowMode;

/// Window decoration dimensions (title bar, borders).
pub(crate) struct WindowDecoration {
    pub width:  u32,
    pub height: u32,
}

/// Information from winit captured at startup.
#[derive(Resource)]
pub(crate) struct WinitInfo {
    pub starting_monitor_index: usize,
    pub decoration:             WindowDecoration,
}

impl WinitInfo {
    /// Get window decoration dimensions as a `UVec2`.
    #[must_use]
    pub(crate) const fn decoration(&self) -> UVec2 {
        UVec2::new(self.decoration.width, self.decoration.height)
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
pub(crate) struct X11FrameCompensated;

/// State for `MonitorScaleStrategy::HigherToLower` (high→low DPI restore).
///
/// When restoring from a high-DPI to low-DPI monitor, we must set position BEFORE size
/// because Bevy's `changed_windows` system processes size changes before position changes.
/// If we set both together, the window resizes first while still at the old position,
/// temporarily extending into the wrong monitor and triggering a scale factor bounce from macOS.
///
/// By moving a 1x1 window to the final position first, we ensure the window is already
/// at the correct location when we later apply size in `ApplySize`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect)]
pub(crate) enum WindowRestoreState {
    /// Initial state: window needs to be moved to the target monitor to trigger a scale change.
    /// Handled by `restore_windows` which calls `apply_initial_move` and transitions to
    /// `WaitingForScaleChange`. This unified entry point replaces the old separate paths
    /// (`PreStartup` `move_to_target_monitor` for primary, inline guard for managed).
    NeedInitialMove,
    /// Position applied with compensation, waiting for `ScaleChanged` message.
    WaitingForScaleChange,
    /// Scale changed, ready to apply final size (position already set in phase 1).
    ApplySize,
}

/// Phase-based fullscreen restore state machine.
///
/// Fullscreen restore requires platform-specific sequencing:
///
/// - **Linux X11**: Move to target monitor first, wait a frame for the compositor to process, then
///   apply fullscreen mode as a fresh change.
/// - **Linux Wayland**: Apply mode directly (no position available).
/// - **Windows (DX12)**: Wait for surface creation before applying fullscreen (see <https://github.com/rust-windowing/winit/issues/3124>).
/// - **macOS**: Apply mode directly.
///
/// The key insight: on X11, if fullscreen mode is set in the same frame as
/// position, the compositor may briefly honor it then revert. Splitting into
/// separate frames ensures each change is processed independently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect)]
pub(crate) enum FullscreenRestoreState {
    /// Move window to target monitor position. Skipped on Wayland (no position).
    MoveToMonitor,
    /// Wait for compositor to process the position change (1 frame).
    WaitForMove,
    /// Wait for GPU surface creation (Windows DX12 workaround, winit #3124).
    WaitForSurface,
    /// Apply the fullscreen mode.
    ApplyMode,
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
/// - **`CompensateSizeOnly`**: Windows only. Uses two-phase approach via `WindowRestoreState`:
///   1. Apply position directly + compensated size (triggers `WM_DPICHANGED`)
///   2. After scale changes, re-apply exact target size to eliminate rounding errors
///
/// - **`LowerToHigher`**: macOS/Linux X11. Low→High DPI (1x→2x, ratio < 1). Multiply both position
///   and size by ratio.
///
/// - **`HigherToLower`**: macOS/Linux X11. High→Low DPI (2x→1x, ratio > 1). Uses two-phase approach
///   via `WindowRestoreState` to avoid size clamping:
///   1. Move a 1x1 window to final position (compensated) to trigger scale change
///   2. After scale changes, apply size without compensation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect)]
pub(crate) enum MonitorScaleStrategy {
    /// Same scale - apply position and size directly.
    ApplyUnchanged,
    /// Windows cross-DPI: position direct, size in two phases.
    CompensateSizeOnly(WindowRestoreState),
    /// Low→High DPI (1x→2x) - apply with compensation (ratio < 1).
    LowerToHigher,
    /// High→Low DPI (2x→1x) - requires two phases (see enum docs).
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
#[derive(Component, Reflect)]
#[reflect(Component)]
pub(crate) struct TargetPosition {
    /// Final clamped position (adjusted to fit within target monitor).
    /// None on Wayland where clients can't access window position.
    pub position:             Option<IVec2>,
    /// Target width in physical pixels (content area, excluding window decoration).
    /// Computed from `logical_width * target_scale`. Applied via `set_physical_resolution()`.
    pub width:                u32,
    /// Target height in physical pixels (content area, excluding window decoration).
    /// Computed from `logical_height * target_scale`. Applied via `set_physical_resolution()`.
    pub height:               u32,
    /// Target width in logical pixels, from `WindowState.logical_width`.
    /// Used for event reporting.
    pub logical_width:        u32,
    /// Target height in logical pixels, from `WindowState.logical_height`.
    /// Used for event reporting.
    pub logical_height:       u32,
    /// Scale factor of the target monitor.
    pub target_scale:         f64,
    /// Scale factor of the monitor where the window starts (keyboard focus monitor).
    pub starting_scale:       f64,
    /// Strategy for handling scale factor differences between monitors.
    pub scale_strategy:       MonitorScaleStrategy,
    /// Window mode to restore.
    pub mode:                 SavedWindowMode,
    /// Target monitor index for fullscreen restore.
    /// On non-Wayland platforms, this could be derived from position, but Wayland
    /// doesn't provide window position, so we store it explicitly.
    pub target_monitor_index: usize,
    /// Fullscreen restore state (DX12/DXGI workaround).
    pub fullscreen_state:     Option<FullscreenRestoreState>,
    /// Settling state. When set, `try_apply_restore` has completed and we're waiting
    /// for the compositor/winit to deliver stable, matching state.
    ///
    /// Uses a two-timer approach:
    /// - **Stability timer** (200ms): resets whenever any compared value changes between frames.
    ///   Fires `WindowRestored` when all values have been stable for 200ms.
    /// - **Total timeout** (1s): hard deadline. If values never stabilize for 200ms continuously,
    ///   fires `WindowRestoreMismatch` with whatever state exists at timeout.
    ///
    /// This handles compositor artifacts like Wayland `wl_surface.enter`/`leave` bounces
    /// where `current_monitor()` transiently reports the wrong monitor during fullscreen
    /// transitions.
    pub settle_state:         Option<SettleState>,
}

impl TargetPosition {
    /// Get the target position as an `IVec2`, if available.
    #[must_use]
    pub(crate) const fn position(&self) -> Option<IVec2> { self.position }

    /// Get the target physical size as a `UVec2`.
    #[must_use]
    pub(crate) const fn size(&self) -> UVec2 { UVec2::new(self.width, self.height) }

    /// Get the target logical size as a `UVec2`.
    #[must_use]
    pub(crate) const fn logical_size(&self) -> UVec2 {
        UVec2::new(self.logical_width, self.logical_height)
    }

    /// Scale ratio between starting and target monitors.
    #[must_use]
    pub(crate) fn ratio(&self) -> f64 { self.starting_scale / self.target_scale }

    /// Position compensated for scale factor differences.
    ///
    /// Multiplies position by the ratio to account for winit dividing by launch scale.
    /// Returns None if position is not available (Wayland).
    #[must_use]
    pub(crate) fn compensated_position(&self) -> Option<IVec2> {
        let ratio = self.ratio();
        self.position.map(|pos| {
            IVec2::new(
                (f64::from(pos.x) * ratio).to_i32(),
                (f64::from(pos.y) * ratio).to_i32(),
            )
        })
    }

    /// Size compensated for scale factor differences.
    ///
    /// Multiplies size by the ratio to account for winit dividing by launch scale.
    #[must_use]
    pub(crate) fn compensated_size(&self) -> UVec2 {
        let ratio = self.ratio();
        UVec2::new(
            (f64::from(self.width) * ratio).to_u32(),
            (f64::from(self.height) * ratio).to_u32(),
        )
    }
}
