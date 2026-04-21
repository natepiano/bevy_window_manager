use std::collections::HashMap;

use bevy::prelude::*;

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
