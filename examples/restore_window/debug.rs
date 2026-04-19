use bevy::ecs::system::NonSendMarker;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy::window::WindowMode;
use bevy::window::WindowPosition;
use bevy::window::WindowScaleFactorChanged;
use bevy::winit::WINIT_WINDOWS;
use bevy_window_manager::Monitors;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum FocusState {
    Focused,
    #[default]
    Unfocused,
}

impl From<bool> for FocusState {
    fn from(focused: bool) -> Self {
        if focused {
            Self::Focused
        } else {
            Self::Unfocused
        }
    }
}

pub(crate) fn debug_winit_monitor(
    window: Single<Entity, With<PrimaryWindow>>,
    monitors: Res<Monitors>,
    mut cached_monitor: Local<Option<usize>>,
    _non_send: NonSendMarker,
) {
    let window_entity = *window;

    let winit_monitor_index: Option<usize> = WINIT_WINDOWS.with(|ww| {
        let ww = ww.borrow();
        ww.get_window(window_entity).and_then(|winit_window| {
            winit_window.current_monitor().and_then(|current_monitor| {
                let position = current_monitor.position();
                monitors
                    .at(position.x, position.y)
                    .map(|monitor| monitor.index)
            })
        })
    });

    if *cached_monitor != winit_monitor_index {
        info!(
            "[debug_winit_monitor] Monitor changed: {:?} -> {:?}",
            *cached_monitor, winit_monitor_index
        );
        *cached_monitor = winit_monitor_index;
    }
}

#[derive(Default)]
pub(crate) struct CachedWindowDebug {
    position: Option<WindowPosition>,
    width:    u32,
    height:   u32,
    mode:     Option<WindowMode>,
    focus:    FocusState,
}

pub(crate) fn debug_window_changed(
    window: Single<&Window, (With<PrimaryWindow>, Changed<Window>)>,
    mut cached: Local<CachedWindowDebug>,
) {
    let window = *window;

    let position_changed = cached.position.as_ref() != Some(&window.position);
    let size_changed =
        cached.width != window.physical_width() || cached.height != window.physical_height();
    let mode_changed = cached.mode.as_ref() != Some(&window.mode);
    let focus = FocusState::from(window.focused);
    let focused_changed = cached.focus != focus;

    let mut changes = Vec::new();
    if position_changed {
        changes.push(format!(
            "position: {:?} -> {:?}",
            cached.position, window.position
        ));
    }
    if size_changed {
        changes.push(format!(
            "size: {}x{} -> {}x{}",
            cached.width,
            cached.height,
            window.physical_width(),
            window.physical_height()
        ));
    }
    if mode_changed {
        changes.push(format!("mode: {:?} -> {:?}", cached.mode, window.mode));
    }
    if focused_changed {
        changes.push(format!("focused: {:?} -> {:?}", cached.focus, focus));
    }

    if !changes.is_empty() {
        info!("[debug_window_changed] {}", changes.join(", "));
    }

    cached.position = Some(window.position);
    cached.width = window.physical_width();
    cached.height = window.physical_height();
    cached.mode = Some(window.mode);
    cached.focus = focus;
}

pub(crate) fn debug_scale_factor_changed(mut messages: MessageReader<WindowScaleFactorChanged>) {
    for message in messages.read() {
        info!(
            "[debug_scale_factor_changed] WindowScaleFactorChanged received: scale_factor={}",
            message.scale_factor
        );
    }
}
