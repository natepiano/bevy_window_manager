//! Restore target planning, state transitions, and restore application.

use bevy::ecs::system::NonSendMarker;
use bevy::prelude::*;
use bevy::window::MonitorSelection;
use bevy::window::WindowMode;
use bevy::window::WindowPosition;
use bevy::window::WindowScaleFactorChanged;
use bevy::winit::WINIT_WINDOWS;
use bevy_kana::ToI32;
use bevy_kana::ToU32;

use super::settle_state::SettleState;
use super::winit_info::X11FrameCompensated;
use crate::Platform;
use crate::constants::SCALE_FACTOR_EPSILON;
use crate::monitors::MonitorInfo;
use crate::monitors::Monitors;
use crate::persistence::SavedWindowMode;
use crate::persistence::WindowState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MonitorResolutionSource {
    Requested,
    FallbackToPrimary,
}

pub struct ResolvedMonitor<'a> {
    pub info:             &'a MonitorInfo,
    pub logical_position: Option<(i32, i32)>,
    pub source:           MonitorResolutionSource,
}

/// Resolve the target monitor from saved state and return an adjusted saved position.
#[must_use]
pub fn resolve_target_monitor_and_position(
    saved_monitor_index: usize,
    saved_position: Option<(i32, i32)>,
    monitors: &Monitors,
) -> ResolvedMonitor<'_> {
    monitors.by_index(saved_monitor_index).map_or_else(
        || ResolvedMonitor {
            info:             monitors.first(),
            logical_position: None,
            source:           MonitorResolutionSource::FallbackToPrimary,
        },
        |info| ResolvedMonitor {
            info,
            logical_position: saved_position,
            source: MonitorResolutionSource::Requested,
        },
    )
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect)]
pub enum WindowRestoreState {
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
pub enum FullscreenRestoreState {
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
pub enum MonitorScaleStrategy {
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
pub struct TargetPosition {
    /// Final clamped position (adjusted to fit within target monitor).
    /// None on Wayland where clients can't access window position.
    pub physical_position:   Option<IVec2>,
    /// Pre-scale position from the saved state, preserved for event reporting.
    /// None on Wayland (no position was ever saved) or when the saved state had none.
    pub logical_position:    Option<IVec2>,
    /// Target size in physical pixels (content area, excluding window decoration).
    pub physical_size:       UVec2,
    /// Target size in logical pixels from the saved state.
    pub logical_size:        UVec2,
    /// Scale factor of the target monitor.
    pub target_scale:        f64,
    /// Scale factor of the monitor where the window starts (keyboard focus monitor).
    pub starting_scale:      f64,
    /// Strategy for handling scale factor differences between monitors.
    pub scale_strategy:      MonitorScaleStrategy,
    /// Window mode to restore.
    pub mode:                SavedWindowMode,
    /// Target monitor index for fullscreen restore.
    /// On non-Wayland platforms, this could be derived from position, but Wayland
    /// doesn't provide window position, so we store it explicitly.
    pub monitor_index:       usize,
    /// Fullscreen restore state (DX12/DXGI workaround).
    pub fullscreen_state:    Option<FullscreenRestoreState>,
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
    pub(super) settle_state: Option<SettleState>,
}

impl TargetPosition {
    /// Scale ratio between starting and target monitors.
    #[must_use]
    pub const fn ratio(&self) -> f64 { self.starting_scale / self.target_scale }

    /// Position compensated for scale factor differences.
    ///
    /// Multiplies physical position by the ratio to account for winit dividing by launch scale.
    /// Returns None if position is not available (Wayland).
    #[must_use]
    pub fn compensated_position(&self) -> Option<IVec2> {
        let ratio = self.ratio();
        self.physical_position.map(|pos| {
            IVec2::new(
                (f64::from(pos.x) * ratio).to_i32(),
                (f64::from(pos.y) * ratio).to_i32(),
            )
        })
    }

    /// Size compensated for scale factor differences.
    ///
    /// Multiplies physical size by the ratio to account for winit dividing by launch scale.
    #[must_use]
    pub fn compensated_size(&self) -> UVec2 {
        let ratio = self.ratio();
        UVec2::new(
            (f64::from(self.physical_size.x) * ratio).to_u32(),
            (f64::from(self.physical_size.y) * ratio).to_u32(),
        )
    }
}

/// Compute a `TargetPosition` from saved state and a resolved target monitor.
#[must_use]
pub fn compute_target_position(
    saved_state: &WindowState,
    target_info: &MonitorInfo,
    fallback_position: Option<(i32, i32)>,
    decoration: UVec2,
    starting_scale: f64,
    platform: Platform,
) -> TargetPosition {
    let target_scale = target_info.scale;

    // Convert logical → physical using the target monitor's scale factor.
    // This is the single conversion point for size values.
    let physical_width = (f64::from(saved_state.logical_width) * target_scale).to_u32();
    let physical_height = (f64::from(saved_state.logical_height) * target_scale).to_u32();

    let physical_outer_width = physical_width + decoration.x;
    let physical_outer_height = physical_height + decoration.y;
    let physical_position = fallback_position.map(|(x, y)| {
        // Convert logical position to physical using the target monitor's scale factor.
        let physical_x = (f64::from(x) * target_scale).round().to_i32();
        let physical_y = (f64::from(y) * target_scale).round().to_i32();
        clamp_position_to_monitor(
            physical_x,
            physical_y,
            target_info,
            physical_outer_width,
            physical_outer_height,
            platform,
        )
    });

    TargetPosition {
        physical_position,
        logical_position: fallback_position.map(|(x, y)| IVec2::new(x, y)),
        physical_size: UVec2::new(physical_width, physical_height),
        logical_size: UVec2::new(saved_state.logical_width, saved_state.logical_height),
        target_scale,
        starting_scale,
        scale_strategy: platform.scale_strategy(starting_scale, target_scale),
        mode: saved_state.mode.clone(),
        monitor_index: target_info.index,
        fullscreen_state: saved_state
            .mode
            .is_fullscreen()
            .then_some(platform.fullscreen_restore_state()),
        settle_state: None,
    }
}

/// Calculate restored window position, with optional clamping.
///
/// On macOS, clamps to monitor bounds because macOS may resize/reposition windows
/// that extend beyond the screen. macOS does not allow windows to span monitors.
///
/// On Windows and Linux, windows can legitimately span multiple monitors,
/// so we preserve the exact saved position without clamping.
#[must_use]
fn clamp_position_to_monitor(
    physical_saved_x: i32,
    physical_saved_y: i32,
    target_info: &MonitorInfo,
    physical_outer_width: u32,
    physical_outer_height: u32,
    platform: Platform,
) -> IVec2 {
    if platform.should_clamp_position() {
        let monitor_right = target_info.physical_position.x + target_info.physical_size.x.to_i32();
        let monitor_bottom = target_info.physical_position.y + target_info.physical_size.y.to_i32();

        let mut x = physical_saved_x;
        let mut y = physical_saved_y;

        if x + physical_outer_width.to_i32() > monitor_right {
            x = monitor_right - physical_outer_width.to_i32();
        }
        if y + physical_outer_height.to_i32() > monitor_bottom {
            y = monitor_bottom - physical_outer_height.to_i32();
        }
        x = x.max(target_info.physical_position.x);
        y = y.max(target_info.physical_position.y);

        if x != physical_saved_x || y != physical_saved_y {
            debug!(
                "[clamp_position_to_monitor] Clamped: ({physical_saved_x}, {physical_saved_y}) -> ({x}, {y}) for outer size {physical_outer_width}x{physical_outer_height}"
            );
        }

        IVec2::new(x, y)
    } else {
        IVec2::new(physical_saved_x, physical_saved_y)
    }
}

/// Apply the initial window move to the target monitor.
fn apply_initial_move(target: &TargetPosition, window: &mut Window) {
    if target.mode.is_fullscreen() {
        if let Some(position) = target.physical_position {
            debug!(
                "[apply_initial_move] Moving to target position {:?} for fullscreen mode {:?}",
                position, target.mode
            );
            window.position = WindowPosition::At(position);
        } else {
            debug!(
                "[apply_initial_move] No saved position, fullscreen mode {:?} targets monitor {} via WindowMode",
                target.mode, target.monitor_index
            );
        }
        return;
    }

    let Some(position) = target.physical_position else {
        debug!(
            "[apply_initial_move] No saved position, centering on monitor {}",
            target.monitor_index
        );
        window.position = WindowPosition::Centered(MonitorSelection::Index(target.monitor_index));
        return;
    };

    let (physical_move_position, physical_move_size) = match target.scale_strategy {
        MonitorScaleStrategy::HigherToLower(_) => {
            let ratio = target.ratio();
            let physical_compensated_x = (f64::from(position.x) * ratio).to_i32();
            let physical_compensated_y = (f64::from(position.y) * ratio).to_i32();
            debug!(
                "[apply_initial_move] HigherToLower: compensating position {position:?} -> ({physical_compensated_x}, {physical_compensated_y}) (ratio={ratio})",
            );
            (
                IVec2::new(physical_compensated_x, physical_compensated_y),
                target.physical_size,
            )
        },
        MonitorScaleStrategy::CompensateSizeOnly(_) => {
            let compensated_size = target.compensated_size();
            debug!(
                "[apply_initial_move] CompensateSizeOnly: position={:?} compensated_size={}x{} (ratio={})",
                position,
                compensated_size.x,
                compensated_size.y,
                target.ratio()
            );
            (position, compensated_size)
        },
        _ => (position, target.physical_size),
    };

    debug!(
        "[apply_initial_move] position={physical_move_position:?} size={}x{} visible={}",
        physical_move_size.x, physical_move_size.y, window.visible
    );

    window.position = WindowPosition::At(physical_move_position);
    window
        .resolution
        .set_physical_resolution(physical_move_size.x, physical_move_size.y);
}

/// Handle the initial move for cross-DPI strategies.
///
/// With a saved position, we apply a compensated position+size on the starting monitor,
/// then transition to `WaitingForScaleChange` so winit's `WindowScaleFactorChanged`
/// triggers the final `ApplySize` phase at `target_scale`.
///
/// With no saved position, we anchor the window on the saved monitor via
/// `WindowPosition::Centered` and size at `starting_scale` (so the stored logical size
/// resolves to `target.physical_size` once the window lands on the target monitor).
/// The two-phase scale-change dance is skipped because macOS does not fire
/// `WindowScaleFactorChanged` for windows that are still hidden; waiting for it would
/// deadlock. Settle starts immediately and verifies the resulting state.
fn begin_cross_dpi_restore(target: &mut TargetPosition, window: &mut Window) {
    if target.physical_position.is_none() {
        // Size at `starting_scale`: `set_physical_resolution` is interpreted at the
        // window's current scale factor, which is `starting_scale` until the move
        // completes. Storing logical = `starting_size / starting_scale = logical_size`
        // means the post-move physical size resolves to `logical_size * target_scale`,
        // matching `target.physical_size` for settle.
        let physical_width = (f64::from(target.logical_size.x) * target.starting_scale).to_u32();
        let physical_height = (f64::from(target.logical_size.y) * target.starting_scale).to_u32();
        debug!(
            "[begin_cross_dpi_restore] no saved position, centering on monitor {} at \
             starting_scale={} (physical {}x{} → logical {}x{} after move to target_scale={})",
            target.monitor_index,
            target.starting_scale,
            physical_width,
            physical_height,
            target.logical_size.x,
            target.logical_size.y,
            target.target_scale
        );
        window.position = WindowPosition::Centered(MonitorSelection::Index(target.monitor_index));
        window
            .resolution
            .set_physical_resolution(physical_width, physical_height);
        window.visible = true;
        target.settle_state = Some(SettleState::new());
        return;
    }

    apply_initial_move(target, window);
    target.scale_strategy = match target.scale_strategy {
        MonitorScaleStrategy::HigherToLower(_) => {
            MonitorScaleStrategy::HigherToLower(WindowRestoreState::WaitingForScaleChange)
        },
        _ => MonitorScaleStrategy::CompensateSizeOnly(WindowRestoreState::WaitingForScaleChange),
    };
}

/// Apply pending window restore. Runs only when entities with `TargetPosition` exist.
pub fn restore_windows(
    mut scale_changed_messages: MessageReader<WindowScaleFactorChanged>,
    mut windows: Query<(Entity, &mut TargetPosition, &mut Window), With<X11FrameCompensated>>,
    _: NonSendMarker,
    platform: Res<Platform>,
) {
    let scale_changed = scale_changed_messages.read().last().is_some();

    for (entity, mut target, mut window) in &mut windows {
        if target.settle_state.is_some() {
            continue;
        }

        let winit_window_exists =
            WINIT_WINDOWS.with(|winit_windows| winit_windows.borrow().get_window(entity).is_some());
        if !winit_window_exists {
            debug!("[restore_windows] Skipping entity {entity:?}: winit window not yet created");
            continue;
        }

        if platform.needs_managed_scale_fixup() {
            let actual_scale = f64::from(window.resolution.base_scale_factor());
            if (actual_scale - target.starting_scale).abs() > SCALE_FACTOR_EPSILON {
                let old_strategy = target.scale_strategy;
                target.starting_scale = actual_scale;
                target.scale_strategy = platform.scale_strategy(actual_scale, target.target_scale);
                debug!(
                    "[restore_windows] Corrected starting_scale for entity {entity:?}: \
                     strategy: {old_strategy:?} -> {:?} (actual_scale={actual_scale:.2})",
                    target.scale_strategy
                );
            }
        }

        if matches!(
            target.scale_strategy,
            MonitorScaleStrategy::HigherToLower(WindowRestoreState::NeedInitialMove)
                | MonitorScaleStrategy::CompensateSizeOnly(WindowRestoreState::NeedInitialMove)
        ) {
            begin_cross_dpi_restore(&mut target, &mut window);
            continue;
        }

        match target.scale_strategy {
            MonitorScaleStrategy::HigherToLower(WindowRestoreState::WaitingForScaleChange)
                if scale_changed =>
            {
                debug!(
                    "[Restore] ScaleChanged received, transitioning to WindowRestoreState::ApplySize"
                );
                target.scale_strategy =
                    MonitorScaleStrategy::HigherToLower(WindowRestoreState::ApplySize);
            },
            MonitorScaleStrategy::CompensateSizeOnly(WindowRestoreState::WaitingForScaleChange) => {
                debug!(
                    "[Restore] CompensateSizeOnly: transitioning to ApplySize (scale_changed={scale_changed})"
                );
                target.scale_strategy =
                    MonitorScaleStrategy::CompensateSizeOnly(WindowRestoreState::ApplySize);
            },
            _ => {},
        }

        if let Some(fullscreen_state) = target.fullscreen_state {
            match fullscreen_state {
                FullscreenRestoreState::MoveToMonitor => {
                    if let Some(position) = target.physical_position {
                        debug!("[restore_windows] Fullscreen MoveToMonitor: position={position:?}");
                        window.position = WindowPosition::At(position);
                    }
                    target.fullscreen_state = Some(FullscreenRestoreState::WaitForMove);
                    continue;
                },
                FullscreenRestoreState::WaitForMove => {
                    debug!("[restore_windows] Fullscreen WaitForMove: waiting for compositor");
                    target.fullscreen_state = Some(FullscreenRestoreState::ApplyMode);
                    continue;
                },
                FullscreenRestoreState::WaitForSurface => {
                    debug!("[restore_windows] Fullscreen WaitForSurface: waiting for GPU surface");
                    target.fullscreen_state = Some(FullscreenRestoreState::ApplyMode);
                    continue;
                },
                FullscreenRestoreState::ApplyMode => {},
            }
        }

        if matches!(
            try_apply_restore(&target, &mut window, *platform),
            RestoreStatus::Complete
        ) && target.settle_state.is_none()
        {
            info!(
                "[restore_windows] Restore applied, starting settle (200ms stability / 1s timeout)"
            );
            target.settle_state = Some(SettleState::new());
        }
    }
}

enum RestoreStatus {
    Complete,
    Waiting,
}

fn apply_window_geometry(
    window: &mut Window,
    physical_position: Option<IVec2>,
    physical_size: UVec2,
    strategy: &str,
    ratio: Option<f64>,
    monitor_index: usize,
) {
    if let Some(physical_position) = physical_position {
        if let Some(ratio) = ratio {
            debug!(
                "[try_apply_restore] position={:?} size={}x{} ({strategy}, ratio={ratio})",
                physical_position, physical_size.x, physical_size.y
            );
        } else {
            debug!(
                "[try_apply_restore] position={:?} size={}x{} ({strategy})",
                physical_position, physical_size.x, physical_size.y
            );
        }
        window.position = WindowPosition::At(physical_position);
    } else {
        if let Some(ratio) = ratio {
            debug!(
                "[try_apply_restore] size={}x{} centered on monitor {monitor_index} ({strategy}, ratio={ratio}, no saved position)",
                physical_size.x, physical_size.y
            );
        } else {
            debug!(
                "[try_apply_restore] size={}x{} centered on monitor {monitor_index} ({strategy}, no saved position)",
                physical_size.x, physical_size.y
            );
        }
        window.position = WindowPosition::Centered(MonitorSelection::Index(monitor_index));
    }
    window
        .resolution
        .set_physical_resolution(physical_size.x, physical_size.y);
}

fn apply_fullscreen_restore(target: &TargetPosition, window: &mut Window, platform: Platform) {
    let monitor_index = target.monitor_index;

    let window_mode = if platform.exclusive_fullscreen_fallback()
        && matches!(target.mode, SavedWindowMode::Fullscreen { .. })
    {
        warn!(
            "Exclusive fullscreen is not supported on Wayland, restoring as BorderlessFullscreen"
        );
        WindowMode::BorderlessFullscreen(MonitorSelection::Index(monitor_index))
    } else {
        target.mode.to_window_mode(monitor_index)
    };

    debug!(
        "[Restore] Applying fullscreen mode {:?} on monitor {} -> WindowMode::{:?}",
        target.mode, monitor_index, window_mode
    );
    debug!(
        "[Restore] Current window state: position={:?} mode={:?}",
        window.position, window.mode
    );

    window.mode = window_mode;
}

fn try_apply_restore(
    target: &TargetPosition,
    window: &mut Window,
    platform: Platform,
) -> RestoreStatus {
    if target.mode.is_fullscreen() {
        debug!(
            "[try_apply_restore] fullscreen: mode={:?} target_monitor={} current_physical={}x{} current_mode={:?} current_pos={:?}",
            target.mode,
            target.monitor_index,
            window.physical_width(),
            window.physical_height(),
            window.mode,
            window.position,
        );
        apply_fullscreen_restore(target, window, platform);
        window.visible = true;
        return RestoreStatus::Complete;
    }

    debug!(
        "[Restore] target_pos={:?} target_scale={} strategy={:?}",
        target.physical_position, target.target_scale, target.scale_strategy
    );

    match target.scale_strategy {
        MonitorScaleStrategy::ApplyUnchanged => {
            apply_window_geometry(
                window,
                target.physical_position,
                target.physical_size,
                "ApplyUnchanged",
                None,
                target.monitor_index,
            );
        },
        MonitorScaleStrategy::CompensateSizeOnly(WindowRestoreState::ApplySize) => {
            debug!(
                "[try_apply_restore] size={}x{} ONLY (CompensateSizeOnly::ApplySize, position already set)",
                target.physical_size.x, target.physical_size.y
            );
            window
                .resolution
                .set_physical_resolution(target.physical_size.x, target.physical_size.y);
        },
        MonitorScaleStrategy::CompensateSizeOnly(
            WindowRestoreState::NeedInitialMove | WindowRestoreState::WaitingForScaleChange,
        ) => {
            debug!(
                "[Restore] CompensateSizeOnly: waiting for initial move or ScaleChanged message"
            );
            return RestoreStatus::Waiting;
        },
        MonitorScaleStrategy::LowerToHigher => {
            apply_window_geometry(
                window,
                target.compensated_position(),
                target.compensated_size(),
                "LowerToHigher",
                Some(target.ratio()),
                target.monitor_index,
            );
        },
        MonitorScaleStrategy::HigherToLower(WindowRestoreState::ApplySize) => {
            debug!(
                "[try_apply_restore] size={}x{} ONLY (HigherToLower::ApplySize, position already set)",
                target.physical_size.x, target.physical_size.y
            );
            window
                .resolution
                .set_physical_resolution(target.physical_size.x, target.physical_size.y);
        },
        MonitorScaleStrategy::HigherToLower(
            WindowRestoreState::NeedInitialMove | WindowRestoreState::WaitingForScaleChange,
        ) => {
            debug!("[Restore] HigherToLower: waiting for initial move or ScaleChanged message");
            return RestoreStatus::Waiting;
        },
    }

    window.visible = true;
    RestoreStatus::Complete
}
