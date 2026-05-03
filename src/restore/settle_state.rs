//! Settle checking logic.
//!
//! After a window restore is applied, monitors the actual window state each frame
//! to confirm the compositor delivered matching values (or detect mismatches).

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy::window::WindowMode;
use bevy_kana::ToI32;
use bevy_kana::ToU32;

use super::target_position::TargetPosition;
use super::winit_info::X11FrameCompensated;
use crate::ManagedWindow;
use crate::Platform;
use crate::WindowKey;
use crate::constants::SETTLE_STABILITY_SECS;
use crate::constants::SETTLE_TIMEOUT_SECS;
use crate::events::WindowRestoreMismatch;
use crate::events::WindowRestored;
use crate::monitors::CurrentMonitor;

/// Tracks the two-timer settling state after restore completes.
#[derive(Debug, Clone, Reflect)]
pub(super) struct SettleState {
    /// Hard deadline timer — fires mismatch if stability is never reached.
    total_timeout:   Timer,
    /// Resets whenever any compared value changes between frames.
    stability_timer: Timer,
    /// Snapshot of last frame's compared values, used to detect changes.
    last_snapshot:   Option<SettleSnapshot>,
}

impl SettleState {
    /// Create a new settle state with default durations.
    #[must_use]
    pub fn new() -> Self {
        Self {
            total_timeout:   Timer::from_seconds(SETTLE_TIMEOUT_SECS, TimerMode::Once),
            stability_timer: Timer::from_seconds(SETTLE_STABILITY_SECS, TimerMode::Once),
            last_snapshot:   None,
        }
    }
}

/// Snapshot of compared values for change detection between frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect)]
struct SettleSnapshot {
    physical_position: Option<IVec2>,
    physical_size:     UVec2,
    mode:              WindowMode,
    monitor:           usize,
}

/// Build a [`SettleSnapshot`] from the current window state, returning the snapshot
/// and the actual scale factor (tracked separately since scale is informational).
fn build_actual_snapshot(
    window: &Window,
    current_monitor: Option<&CurrentMonitor>,
    platform: Platform,
) -> (SettleSnapshot, f64) {
    let position = if platform.position_available() {
        match window.position {
            WindowPosition::At(p) => Some(IVec2::new(p.x, p.y)),
            _ => None,
        }
    } else {
        None
    };
    let size = UVec2::new(
        window.resolution.physical_width(),
        window.resolution.physical_height(),
    );
    (
        SettleSnapshot {
            physical_position: position,
            physical_size:     size,
            mode:              window.mode,
            monitor:           current_monitor.map_or(0, |cm| cm.monitor.index),
        },
        f64::from(window.resolution.scale_factor()),
    )
}

/// Check whether actual window state matches the target for settle purposes.
///
/// Fullscreen modes skip position and size comparison — the window fills the
/// monitor so the stored position/size are irrelevant. On macOS, borderless
/// fullscreen reports position offset by the menu bar height; on X11 (W6),
/// frame vs client coords differ. The physical size can also differ when
/// scales differ between backends (e.g. Wayland scale 1 vs `XWayland` scale 2).
fn check_settle_matches(
    target: &TargetPosition,
    target_physical_position: Option<IVec2>,
    target_physical_size: UVec2,
    target_mode: WindowMode,
    target_monitor: usize,
    actual: &SettleSnapshot,
    platform: Platform,
) -> SettleComparison {
    let is_fullscreen = target.mode.is_fullscreen();
    // Skip position comparison when:
    // - fullscreen (window fills monitor; saved position is irrelevant)
    // - no saved position (window was anchored via `WindowPosition::Centered`; the resulting `At`
    //   position is OS-chosen and not part of the comparison)
    // - X11 W6 frame-vs-client coordinate mismatch
    let skip_position = is_fullscreen
        || target_physical_position.is_none()
        || !platform.position_reliable_for_settle();
    let position_matches = skip_position || target_physical_position == actual.physical_position;
    let size_match = is_fullscreen || target_physical_size == actual.physical_size;
    let mode_match = platform.modes_match(target_mode, actual.mode);
    let monitor_match = target_monitor == actual.monitor;
    SettleComparison {
        position: position_matches.into(),
        size:     size_match.into(),
        mode:     mode_match.into(),
        monitor:  monitor_match.into(),
    }
}

#[derive(Clone, Copy)]
enum MatchState {
    Match,
    Mismatch,
}

impl From<bool> for MatchState {
    fn from(matches: bool) -> Self { if matches { Self::Match } else { Self::Mismatch } }
}

impl MatchState {
    const fn is_match(self) -> bool { matches!(self, Self::Match) }
}

struct SettleComparison {
    position: MatchState,
    size:     MatchState,
    mode:     MatchState,
    monitor:  MatchState,
}

impl SettleComparison {
    const fn all_match(&self) -> bool {
        self.position.is_match()
            && self.size.is_match()
            && self.mode.is_match()
            && self.monitor.is_match()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TimeoutState {
    Active,
    TimedOut,
}

#[derive(Clone, Copy)]
enum ChangeHandling {
    Skip,
    Continue,
}

/// Detect whether the settle snapshot changed from the previous frame and reset the
/// stability timer if so.
fn detect_settle_change(
    settle: &mut SettleState,
    snapshot: SettleSnapshot,
    key: &WindowKey,
    total_elapsed_ms: f32,
    timeout_state: TimeoutState,
) -> ChangeHandling {
    let changed = settle.last_snapshot.as_ref() != Some(&snapshot);
    if changed {
        if settle.last_snapshot.is_some() {
            debug!(
                "[check_restore_settling] [{key}] {total_elapsed_ms:.0}ms: values changed, \
                 resetting stability timer"
            );
        }
        settle.stability_timer.reset();
        settle.last_snapshot = Some(snapshot);
        match timeout_state {
            TimeoutState::TimedOut => ChangeHandling::Continue,
            TimeoutState::Active => ChangeHandling::Skip,
        }
    } else {
        ChangeHandling::Continue
    }
}

/// Resolve the [`WindowKey`] for an entity — `Primary` if it has the `PrimaryWindow`
/// marker, otherwise the `ManagedWindow` name (falling back to `Primary`).
fn resolve_window_key(
    entity: Entity,
    primary_q: &Query<(), With<PrimaryWindow>>,
    managed_q: &Query<&ManagedWindow>,
) -> WindowKey {
    if primary_q.get(entity).is_ok() {
        WindowKey::Primary
    } else if let Ok(managed) = managed_q.get(entity) {
        WindowKey::Managed(managed.name.clone())
    } else {
        WindowKey::Primary
    }
}

/// Check settling windows each frame using a two-timer approach.
///
/// - **Stability timer** (200ms): resets whenever any compared value changes. If values stay stable
///   for 200ms, fires `WindowRestored`.
/// - **Total timeout** (1s): hard deadline. Fires `WindowRestoreMismatch` if stability is never
///   reached.
///
/// Runs while `TargetPosition` entities exist (same gate as `restore_windows`).
/// Only processes entities that have a `settle_state` set.
pub fn check_restore_settling(
    mut commands: Commands,
    time: Res<Time>,
    mut windows: Query<
        (
            Entity,
            &mut TargetPosition,
            &Window,
            Option<&CurrentMonitor>,
        ),
        With<X11FrameCompensated>,
    >,
    primary_q: Query<(), With<PrimaryWindow>>,
    managed_q: Query<&ManagedWindow>,
    platform: Res<Platform>,
) {
    for (entity, mut target, window, current_monitor) in &mut windows {
        let target_mode = target.mode.to_window_mode(target.monitor_index);
        let target_physical_size = target.physical_size;
        let target_logical_size = target.logical_size;
        let target_monitor = target.monitor_index;
        let expected_scale = target.target_scale;

        let target_physical_position = platform
            .position_available()
            .then_some(target.physical_position)
            .flatten();
        let target_logical_position = platform
            .position_available()
            .then_some(target.logical_position)
            .flatten();
        let key = resolve_window_key(entity, &primary_q, &managed_q);
        let (current_snapshot, actual_scale) =
            build_actual_snapshot(window, current_monitor, *platform);

        let Some(settle) = target.settle_state.as_mut() else {
            continue;
        };
        settle.total_timeout.tick(time.delta());
        settle.stability_timer.tick(time.delta());

        let total_elapsed_ms = settle.total_timeout.elapsed_secs() * 1000.0;
        let stability_elapsed_ms = settle.stability_timer.elapsed_secs() * 1000.0;
        let timeout_state = if settle.total_timeout.is_finished() {
            TimeoutState::TimedOut
        } else {
            TimeoutState::Active
        };

        if matches!(
            detect_settle_change(
                settle,
                current_snapshot,
                &key,
                total_elapsed_ms,
                timeout_state,
            ),
            ChangeHandling::Skip
        ) {
            continue;
        }
        let stable = settle.stability_timer.is_finished();
        let comparison = check_settle_matches(
            &target,
            target_physical_position,
            target_physical_size,
            target_mode,
            target_monitor,
            &current_snapshot,
            *platform,
        );
        debug!(
            "[check_restore_settling] [{key}] {total_elapsed_ms:.0}ms (stable: {stability_elapsed_ms:.0}ms): \
             pos={} size={} mode={} monitor={} | \
             size: {target_physical_size} vs {}, \
             mode: {target_mode:?} vs {:?}, \
             monitor: {target_monitor} vs {}, \
             scale: {expected_scale} vs {actual_scale}",
            comparison.position.is_match(),
            comparison.size.is_match(),
            comparison.mode.is_match(),
            comparison.monitor.is_match(),
            current_snapshot.physical_size,
            current_snapshot.mode,
            current_snapshot.monitor,
        );

        let settle_target = SettleTarget {
            physical_position: target_physical_position,
            logical_position:  target_logical_position,
            physical_size:     target_physical_size,
            logical_size:      target_logical_size,
            mode:              target_mode,
            monitor:           target_monitor,
            scale:             expected_scale,
        };
        if stable && comparison.all_match() {
            emit_settle_success(
                &mut commands,
                entity,
                key,
                &settle_target,
                total_elapsed_ms,
                stability_elapsed_ms,
            );
        } else if timeout_state == TimeoutState::TimedOut {
            emit_settle_mismatch(
                &mut commands,
                entity,
                key,
                &settle_target,
                &build_settle_actual(window, current_snapshot, actual_scale),
                total_elapsed_ms,
            );
        }
    }
}

/// Bundled actual values for settle mismatch reporting.
struct SettleActual {
    snapshot:     SettleSnapshot,
    scale:        f64,
    logical_size: UVec2,
}

fn build_settle_actual(window: &Window, snapshot: SettleSnapshot, scale: f64) -> SettleActual {
    SettleActual {
        snapshot,
        scale,
        logical_size: UVec2::new(
            window.resolution.width().to_u32(),
            window.resolution.height().to_u32(),
        ),
    }
}

/// Extracted target values for settle resolution, avoiding too-many-arguments.
struct SettleTarget {
    physical_position: Option<IVec2>,
    logical_position:  Option<IVec2>,
    logical_size:      UVec2,
    physical_size:     UVec2,
    mode:              WindowMode,
    monitor:           usize,
    scale:             f64,
}

/// Emit `WindowRestored` and clean up `TargetPosition` when settle succeeds.
fn emit_settle_success(
    commands: &mut Commands,
    entity: Entity,
    key: WindowKey,
    target: &SettleTarget,
    total_elapsed_ms: f32,
    stability_elapsed_ms: f32,
) {
    info!(
        "[check_restore_settling] [{key}] Settled after {total_elapsed_ms:.0}ms \
         (stable for {stability_elapsed_ms:.0}ms)"
    );
    commands
        .entity(entity)
        .trigger(|entity| WindowRestored {
            entity,
            window_id: key,
            physical_position: target.physical_position,
            logical_position: target.logical_position,
            logical_size: target.logical_size,
            physical_size: target.physical_size,
            mode: target.mode,
            monitor_index: target.monitor,
        })
        .remove::<TargetPosition>()
        .remove::<X11FrameCompensated>();
}

/// Emit `WindowRestoreMismatch` and clean up `TargetPosition` when settle times out.
fn emit_settle_mismatch(
    commands: &mut Commands,
    entity: Entity,
    key: WindowKey,
    target: &SettleTarget,
    actual: &SettleActual,
    total_elapsed_ms: f32,
) {
    warn!(
        "[check_restore_settling] [{key}] Settle timeout after {total_elapsed_ms:.0}ms — \
        mismatch remains: \
         position: {:?} vs {:?}, \
         size: {} vs {}, \
         mode: {:?} vs {:?}, \
         monitor: {} vs {}, \
         scale: {} vs {}",
        target.physical_position,
        actual.snapshot.physical_position,
        target.physical_size,
        actual.snapshot.physical_size,
        target.mode,
        actual.snapshot.mode,
        target.monitor,
        actual.snapshot.monitor,
        target.scale,
        actual.scale,
    );
    let actual_logical_position = actual.snapshot.physical_position.map(|pos| {
        IVec2::new(
            (f64::from(pos.x) / actual.scale).round().to_i32(),
            (f64::from(pos.y) / actual.scale).round().to_i32(),
        )
    });
    commands
        .entity(entity)
        .trigger(|entity| WindowRestoreMismatch {
            entity,
            window_id: key,
            expected_physical_position: target.physical_position,
            actual_physical_position: actual.snapshot.physical_position,
            expected_logical_position: target.logical_position,
            actual_logical_position,
            expected_physical_size: target.physical_size,
            actual_physical_size: actual.snapshot.physical_size,
            expected_logical_size: target.logical_size,
            actual_logical_size: actual.logical_size,
            expected_mode: target.mode,
            actual_mode: actual.snapshot.mode,
            expected_monitor: target.monitor,
            actual_monitor: actual.snapshot.monitor,
            expected_scale: target.scale,
            actual_scale: actual.scale,
        })
        .remove::<TargetPosition>()
        .remove::<X11FrameCompensated>();
}
