use std::collections::HashMap;

use bevy::prelude::*;
use bevy::window::WindowMode;
use bevy_window_manager::WindowRestoreMismatch;
use bevy_window_manager::WindowRestored;

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

#[derive(Debug, Clone, Reflect)]
pub(crate) struct MonitorMismatch {
    pub(crate) expected: usize,
    pub(crate) actual:   usize,
}

#[derive(Debug, Clone, Reflect)]
pub(crate) struct ModeMismatch {
    pub(crate) expected: WindowMode,
    pub(crate) actual:   WindowMode,
}

#[derive(Debug, Clone, Reflect)]
pub(crate) struct PositionMismatch {
    pub(crate) expected: Option<IVec2>,
    pub(crate) actual:   Option<IVec2>,
}

#[derive(Debug, Clone, Reflect)]
pub(crate) struct SizeMismatch {
    pub(crate) expected: UVec2,
    pub(crate) actual:   UVec2,
}

#[derive(Debug, Clone, Reflect)]
pub(crate) struct ScaleMismatch {
    pub(crate) expected: f64,
    pub(crate) actual:   f64,
}

#[derive(Resource, Debug, Clone, Reflect)]
#[reflect(Resource)]
pub(crate) struct WindowRestoredReceived {
    pub(crate) physical_position: Option<IVec2>,
    pub(crate) physical_size:     UVec2,
    pub(crate) mode:              WindowMode,
    pub(crate) monitor:           usize,
}

/// Adapts the flat `expected_*` / `actual_*` shape of `WindowRestoreMismatch` into
/// nested comparison structs for BRP inspection. If the public event's field layout
/// changes, this resource's unpacking (in `on_window_restore_mismatch`) must change with it.
#[derive(Resource, Debug, Clone, Reflect)]
#[reflect(Resource)]
pub(crate) struct WindowRestoreMismatchReceived {
    pub(crate) monitor:       MonitorMismatch,
    pub(crate) physical_size: SizeMismatch,
    pub(crate) mode:          ModeMismatch,
}

#[derive(Resource, Debug, Default, Reflect)]
#[reflect(Resource)]
pub(crate) struct WindowsSettledCount {
    pub(crate) count: usize,
}

#[derive(Clone)]
pub(crate) struct CachedMismatchState {
    pub(crate) physical_position: PositionMismatch,
    pub(crate) logical_position:  PositionMismatch,
    pub(crate) physical_size:     SizeMismatch,
    pub(crate) logical_size:      SizeMismatch,
    pub(crate) mode:              ModeMismatch,
    pub(crate) monitor:           MonitorMismatch,
    pub(crate) scale:             ScaleMismatch,
}

#[derive(Resource, Default)]
pub(crate) struct MismatchStates {
    pub(crate) states: HashMap<Entity, CachedMismatchState>,
}

#[derive(Resource, Default)]
pub(crate) struct RestoredStates {
    pub(crate) states: HashMap<Entity, CachedRestoredState>,
}

pub(crate) struct CachedRestoredState {
    pub(crate) physical_position: Option<IVec2>,
    pub(crate) logical_position:  Option<IVec2>,
    pub(crate) physical_size:     UVec2,
    pub(crate) logical_size:      UVec2,
    pub(crate) monitor:           usize,
    pub(crate) mode:              WindowMode,
}

pub(crate) fn on_window_restored(
    trigger: On<WindowRestored>,
    mut commands: Commands,
    mut restored_states: ResMut<RestoredStates>,
    mut settled_count: ResMut<WindowsSettledCount>,
) {
    let event = trigger.event();
    info!(
        "[on_window_restored] Restore complete: window_id={} entity={:?} physical_position={:?} logical_position={:?} physical_size={} logical_size={} mode={:?} monitor={}",
        event.window_id,
        event.entity,
        event.physical_position,
        event.logical_position,
        event.physical_size,
        event.logical_size,
        event.mode,
        event.monitor_index
    );

    restored_states.states.insert(
        event.entity,
        CachedRestoredState {
            physical_position: event.physical_position,
            logical_position:  event.logical_position,
            physical_size:     event.physical_size,
            logical_size:      event.logical_size,
            monitor:           event.monitor_index,
            mode:              event.mode,
        },
    );

    commands.insert_resource(WindowRestoredReceived {
        physical_position: event.physical_position,
        physical_size:     event.physical_size,
        mode:              event.mode,
        monitor:           event.monitor_index,
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
            physical_position: event.expected_physical_position,
            logical_position:  event.expected_logical_position,
            physical_size:     event.expected_physical_size,
            logical_size:      event.expected_logical_size,
            monitor:           event.expected_monitor,
            mode:              event.expected_mode,
        },
    );

    mismatch_states.states.insert(
        event.entity,
        CachedMismatchState {
            physical_position: PositionMismatch {
                expected: event.expected_physical_position,
                actual:   event.actual_physical_position,
            },
            logical_position:  PositionMismatch {
                expected: event.expected_logical_position,
                actual:   event.actual_logical_position,
            },
            physical_size:     SizeMismatch {
                expected: event.expected_physical_size,
                actual:   event.actual_physical_size,
            },
            logical_size:      SizeMismatch {
                expected: event.expected_logical_size,
                actual:   event.actual_logical_size,
            },
            mode:              ModeMismatch {
                expected: event.expected_mode,
                actual:   event.actual_mode,
            },
            monitor:           MonitorMismatch {
                expected: event.expected_monitor,
                actual:   event.actual_monitor,
            },
            scale:             ScaleMismatch {
                expected: event.expected_scale,
                actual:   event.actual_scale,
            },
        },
    );

    commands.insert_resource(WindowRestoreMismatchReceived {
        monitor:       MonitorMismatch {
            expected: event.expected_monitor,
            actual:   event.actual_monitor,
        },
        physical_size: SizeMismatch {
            expected: event.expected_physical_size,
            actual:   event.actual_physical_size,
        },
        mode:          ModeMismatch {
            expected: event.expected_mode,
            actual:   event.actual_mode,
        },
    });
    settled_count.count += 1;
}
