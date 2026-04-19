use std::collections::HashMap;

use bevy::prelude::*;
use bevy::window::WindowMode;

pub(crate) const TEST_MODE_ENV_VAR: &str = "BWM_TEST_MODE";
pub(crate) const MARGIN: Val = Val::Px(20.0);
pub(crate) const FONT_SIZE: f32 = 14.0;
pub(crate) const SECONDARY_WINDOW_WIDTH: u32 = 600;
pub(crate) const SECONDARY_WINDOW_HEIGHT: u32 = 400;
pub(crate) const MISMATCH_COLOR: Color = Color::linear_rgb(1.0, 0.3, 0.3);
pub(crate) const MISMATCH_WARN_COLOR: Color = Color::linear_rgb(1.0, 0.7, 0.2);
pub(crate) const DEFAULT_COLOR: Color = Color::WHITE;
pub(crate) const LABEL_WIDTH: usize = 18;

#[derive(Resource, Clone, Copy)]
pub(crate) enum KeyboardInputMode {
    Enabled,
    Disabled,
}

impl From<bool> for KeyboardInputMode {
    fn from(enabled: bool) -> Self {
        if enabled {
            Self::Enabled
        } else {
            Self::Disabled
        }
    }
}

pub(crate) fn keyboard_enabled(input_mode: Res<KeyboardInputMode>) -> bool {
    matches!(*input_mode, KeyboardInputMode::Enabled)
}

#[derive(Resource, Default)]
pub(crate) struct WindowCounter {
    pub(crate) next: usize,
}

#[derive(Resource, Default)]
pub(crate) struct SelectedVideoModes {
    indices:              HashMap<usize, usize>,
    pub(crate) last_sync: Option<(UVec2, u32)>,
}

impl SelectedVideoModes {
    pub(crate) fn get(&self, monitor_index: usize) -> usize {
        self.indices.get(&monitor_index).copied().unwrap_or(0)
    }

    pub(crate) fn set(&mut self, monitor_index: usize, index: usize) {
        self.indices.insert(monitor_index, index);
    }
}

#[derive(Component)]
pub(crate) struct PrimaryDisplay;

#[derive(Component)]
pub(crate) struct SecondaryDisplay(pub(crate) Entity);

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
    pub(crate) position: Option<IVec2>,
    pub(crate) size:     UVec2,
    pub(crate) mode:     WindowMode,
    pub(crate) monitor:  usize,
}

/// Adapts the flat `expected_*` / `actual_*` shape of `WindowRestoreMismatch` into
/// nested comparison structs for BRP inspection. If the public event's field layout
/// changes, this resource's unpacking (in `events::on_window_restore_mismatch`) must
/// change with it.
#[derive(Resource, Debug, Clone, Reflect)]
#[reflect(Resource)]
pub(crate) struct WindowRestoreMismatchReceived {
    pub(crate) monitor: MonitorMismatch,
    pub(crate) size:    SizeMismatch,
    pub(crate) mode:    ModeMismatch,
}

#[derive(Resource, Debug, Default, Reflect)]
#[reflect(Resource)]
pub(crate) struct WindowsSettledCount {
    pub(crate) count: usize,
}

#[derive(Clone)]
pub(crate) struct CachedMismatchState {
    pub(crate) position: PositionMismatch,
    pub(crate) size:     SizeMismatch,
    pub(crate) logical:  SizeMismatch,
    pub(crate) mode:     ModeMismatch,
    pub(crate) monitor:  MonitorMismatch,
    pub(crate) scale:    ScaleMismatch,
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
    pub(crate) position: Option<IVec2>,
    pub(crate) size:     UVec2,
    pub(crate) logical:  UVec2,
    pub(crate) monitor:  usize,
    pub(crate) mode:     WindowMode,
}
