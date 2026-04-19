use bevy::prelude::*;
use bevy_window_manager::WindowRestoreMismatch;
use bevy_window_manager::WindowRestored;

use super::state::CachedMismatchState;
use super::state::CachedRestoredState;
use super::state::MismatchStates;
use super::state::ModeMismatch;
use super::state::MonitorMismatch;
use super::state::PositionMismatch;
use super::state::RestoredStates;
use super::state::ScaleMismatch;
use super::state::SizeMismatch;
use super::state::WindowRestoreMismatchReceived;
use super::state::WindowRestoredReceived;
use super::state::WindowsSettledCount;

#[derive(Event, Reflect)]
#[reflect(Event)]
pub(crate) struct SpawnManagedWindow;

#[derive(Event, Reflect)]
#[reflect(Event)]
pub(crate) struct SetBorderlessFullscreen;

#[derive(Event, Reflect)]
#[reflect(Event)]
pub(crate) struct SetWindowed;

#[derive(Event, Reflect)]
#[reflect(Event)]
pub(crate) struct SetExclusiveFullscreen;

#[derive(Event, Reflect)]
#[reflect(Event)]
pub(crate) struct TogglePersistence;

#[derive(Event, Reflect)]
#[reflect(Event)]
pub(crate) struct ClearStateAndQuit;

#[derive(Event, Reflect)]
#[reflect(Event)]
pub(crate) struct QuitApp;

pub(crate) fn on_window_restored(
    trigger: On<WindowRestored>,
    mut commands: Commands,
    mut restored_states: ResMut<RestoredStates>,
    mut settled_count: ResMut<WindowsSettledCount>,
) {
    let event = trigger.event();
    info!(
        "[on_window_restored] Restore complete: window_id={} entity={:?} position={:?} physical={} logical={} mode={:?} monitor={}",
        event.window_id,
        event.entity,
        event.physical_position,
        event.logical_size,
        event.logical_size,
        event.mode,
        event.monitor_index
    );

    restored_states.states.insert(
        event.entity,
        CachedRestoredState {
            position: event.physical_position,
            size:     event.logical_size,
            logical:  event.logical_size,
            monitor:  event.monitor_index,
            mode:     event.mode,
        },
    );

    commands.insert_resource(WindowRestoredReceived {
        position: event.physical_position,
        size:     event.logical_size,
        mode:     event.mode,
        monitor:  event.monitor_index,
    });
    settled_count.count += 1;
}

pub(crate) fn on_window_restore_mismatch(
    trigger: On<WindowRestoreMismatch>,
    mut commands: Commands,
    mut restored_states: ResMut<RestoredStates>,
    mut mismatch_states: ResMut<MismatchStates>,
    mut settled_count: ResMut<WindowsSettledCount>,
) {
    let event = trigger.event();
    warn!(
        "[on_window_restore_mismatch] window_id={} entity={:?} \
         monitor: {} vs {}, size: {} vs {}, mode: {:?} vs {:?}",
        event.window_id,
        event.entity,
        event.expected_monitor,
        event.actual_monitor,
        event.expected_physical_size,
        event.actual_physical_size,
        event.expected_mode,
        event.actual_mode,
    );

    restored_states.states.insert(
        event.entity,
        CachedRestoredState {
            position: event.expected_physical_position,
            size:     event.expected_physical_size,
            logical:  event.expected_logical_size,
            monitor:  event.expected_monitor,
            mode:     event.expected_mode,
        },
    );

    mismatch_states.states.insert(
        event.entity,
        CachedMismatchState {
            position: PositionMismatch {
                expected: event.expected_physical_position,
                actual:   event.actual_physical_position,
            },
            size:     SizeMismatch {
                expected: event.expected_physical_size,
                actual:   event.actual_physical_size,
            },
            logical:  SizeMismatch {
                expected: event.expected_logical_size,
                actual:   event.actual_logical_size,
            },
            mode:     ModeMismatch {
                expected: event.expected_mode,
                actual:   event.actual_mode,
            },
            monitor:  MonitorMismatch {
                expected: event.expected_monitor,
                actual:   event.actual_monitor,
            },
            scale:    ScaleMismatch {
                expected: event.expected_scale,
                actual:   event.actual_scale,
            },
        },
    );

    commands.insert_resource(WindowRestoreMismatchReceived {
        monitor: MonitorMismatch {
            expected: event.expected_monitor,
            actual:   event.actual_monitor,
        },
        size:    SizeMismatch {
            expected: event.expected_physical_size,
            actual:   event.actual_physical_size,
        },
        mode:    ModeMismatch {
            expected: event.expected_mode,
            actual:   event.actual_mode,
        },
    });
    settled_count.count += 1;
}
