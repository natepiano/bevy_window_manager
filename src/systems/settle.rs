//! Settle checking logic.
//!
//! After a window restore is applied, monitors the actual window state each frame
//! to confirm the compositor delivered matching values (or detect mismatches).

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy::window::WindowMode;
use bevy_kana::ToU32;

use crate::ManagedWindow;
use crate::Platform;
use crate::WindowKey;
use crate::monitors::CurrentMonitor;
use crate::types::SettleSnapshot;
use crate::types::SettleState;
use crate::types::TargetPosition;
use crate::types::WindowRestoreMismatch;
use crate::types::WindowRestored;
use crate::types::X11FrameCompensated;

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
            position,
            size,
            mode: window.mode,
            monitor: current_monitor.map_or(0, |cm| cm.monitor.index),
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
    target_position: Option<IVec2>,
    target_size: UVec2,
    target_mode: WindowMode,
    target_monitor: usize,
    actual: &SettleSnapshot,
    platform: Platform,
) -> (bool, bool, bool, bool) {
    let is_fullscreen = target.mode.is_fullscreen();
    let position_matches = if is_fullscreen {
        true
    } else if platform.position_reliable_for_settle() {
        target_position == actual.position
    } else {
        true // X11 W6: target is frame coords, actual is client area — skip
    };
    let size_match = is_fullscreen || target_size == actual.size;
    let mode_match = platform.modes_match(target_mode, actual.mode);
    let monitor_match = target_monitor == actual.monitor;
    (position_matches, size_match, mode_match, monitor_match)
}

/// Detect whether the settle snapshot changed from the previous frame and reset the
/// stability timer if so. Returns `true` if the caller should skip further checks
/// this frame (snapshot just changed and we haven't timed out yet).
fn detect_settle_change(
    settle: &mut SettleState,
    snapshot: SettleSnapshot,
    key: &WindowKey,
    total_elapsed_ms: f32,
    total_timed_out: bool,
) -> bool {
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
        // Don't check stability this frame — we just reset
        !total_timed_out
    } else {
        false
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
pub(crate) fn check_restore_settling(
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
        // Read target fields before borrowing settle_state mutably
        let target_mode = target.mode.to_window_mode(target.target_monitor_index);
        let target_size = target.size();
        let target_logical_size = target.logical_size();
        let target_monitor = target.target_monitor_index;
        let expected_scale = target.target_scale;

        let target_position = platform
            .position_available()
            .then_some(target.position)
            .flatten();
        let key = resolve_window_key(entity, &primary_q, &managed_q);
        let (current_snapshot, actual_scale) =
            build_actual_snapshot(window, current_monitor, *platform);

        // Now borrow settle_state mutably for timer ticking and change detection
        let Some(settle) = target.settle_state.as_mut() else {
            continue;
        };
        settle.total_timeout.tick(time.delta());
        settle.stability_timer.tick(time.delta());

        let total_elapsed_ms = settle.total_timeout.elapsed_secs() * 1000.0;
        let stability_elapsed_ms = settle.stability_timer.elapsed_secs() * 1000.0;
        let total_timed_out = settle.total_timeout.is_finished();

        if detect_settle_change(
            settle,
            current_snapshot,
            &key,
            total_elapsed_ms,
            total_timed_out,
        ) {
            continue;
        }
        let stable = settle.stability_timer.is_finished();
        let (position_matches, size_match, mode_match, monitor_match) = check_settle_matches(
            &target,
            target_position,
            target_size,
            target_mode,
            target_monitor,
            &current_snapshot,
            *platform,
        );
        let all_match = position_matches && size_match && mode_match && monitor_match;
        debug!(
            "[check_restore_settling] [{key}] {total_elapsed_ms:.0}ms (stable: {stability_elapsed_ms:.0}ms): \
             pos={position_matches} size={size_match} mode={mode_match} monitor={monitor_match} | \
             size: {target_size} vs {}, \
             mode: {target_mode:?} vs {:?}, \
             monitor: {target_monitor} vs {}, \
             scale: {expected_scale} vs {actual_scale}",
            current_snapshot.size, current_snapshot.mode, current_snapshot.monitor,
        );

        let settle_target = SettleTarget {
            position:     target_position,
            size:         target_size,
            logical_size: target_logical_size,
            mode:         target_mode,
            monitor:      target_monitor,
            scale:        expected_scale,
        };
        if stable && all_match {
            emit_settle_success(
                &mut commands,
                entity,
                key,
                &settle_target,
                total_elapsed_ms,
                stability_elapsed_ms,
            );
        } else if total_timed_out {
            let settle_actual = SettleActual {
                snapshot:     current_snapshot,
                scale:        actual_scale,
                logical_size: UVec2::new(
                    window.resolution.width().to_u32(),
                    window.resolution.height().to_u32(),
                ),
            };
            emit_settle_mismatch(
                &mut commands,
                entity,
                key,
                &settle_target,
                &settle_actual,
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

/// Extracted target values for settle resolution, avoiding too-many-arguments.
struct SettleTarget {
    position:     Option<IVec2>,
    size:         UVec2,
    logical_size: UVec2,
    mode:         WindowMode,
    monitor:      usize,
    scale:        f64,
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
            position: target.position,
            size: target.size,
            logical_size: target.logical_size,
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
        target.position,
        actual.snapshot.position,
        target.size,
        actual.snapshot.size,
        target.mode,
        actual.snapshot.mode,
        target.monitor,
        actual.snapshot.monitor,
        target.scale,
        actual.scale,
    );
    commands
        .entity(entity)
        .trigger(|entity| WindowRestoreMismatch {
            entity,
            window_id: key,
            expected_position: target.position,
            actual_position: actual.snapshot.position,
            expected_size: target.size,
            actual_size: actual.snapshot.size,
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
