use bevy::prelude::*;
use bevy::window::Monitor;
use bevy::window::MonitorSelection;
use bevy::window::VideoMode;
use bevy::window::VideoModeSelection;
use bevy::window::WindowMode;
use bevy_window_manager::CurrentMonitor;
use bevy_window_manager::ManagedWindow;
use bevy_window_manager::ManagedWindowPersistence;
use bevy_window_manager::Monitors;

use super::events::ClearStateAndQuit;
use super::events::QuitApp;
use super::events::SetBorderlessFullscreen;
use super::events::SetExclusiveFullscreen;
use super::events::SetWindowed;
use super::events::TogglePersistence;
use super::input;
use super::state::SelectedVideoModes;

pub(crate) fn on_set_borderless_fullscreen(
    _trigger: On<SetBorderlessFullscreen>,
    mut windows: Query<(&mut Window, Option<&CurrentMonitor>)>,
    monitors_res: Res<Monitors>,
) {
    let Some((mut window, current_monitor)) = windows.iter_mut().find(|(window, _)| window.focused)
    else {
        return;
    };
    let monitor = current_monitor.copied().unwrap_or_else(|| CurrentMonitor {
        monitor:        *monitors_res.first(),
        effective_mode: window.mode,
    });
    window.mode = WindowMode::BorderlessFullscreen(MonitorSelection::Index(monitor.monitor.index));
}

pub(crate) fn on_set_windowed(_trigger: On<SetWindowed>, mut windows: Query<&mut Window>) {
    let Some(mut window) = windows.iter_mut().find(|window| window.focused) else {
        return;
    };
    window.mode = WindowMode::Windowed;
}

pub(crate) fn on_set_exclusive_fullscreen(
    _trigger: On<SetExclusiveFullscreen>,
    mut windows: Query<(&mut Window, Option<&CurrentMonitor>)>,
    monitors_res: Res<Monitors>,
    bevy_monitors: Query<(Entity, &Monitor)>,
    selected: Res<SelectedVideoModes>,
) {
    let Some((mut window, current_monitor)) = windows.iter_mut().find(|(window, _)| window.focused)
    else {
        return;
    };
    let monitor = current_monitor.copied().unwrap_or_else(|| CurrentMonitor {
        monitor:        *monitors_res.first(),
        effective_mode: window.mode,
    });

    let video_modes: Vec<VideoMode> = bevy_monitors
        .iter()
        .find(|(_, bevy_monitor)| bevy_monitor.physical_position == monitor.monitor.position)
        .map(|(_, bevy_monitor)| bevy_monitor.video_modes.clone())
        .unwrap_or_default();

    let selected_idx = selected
        .get(monitor.monitor.index)
        .min(video_modes.len().saturating_sub(1));
    let video_mode_selection = video_modes
        .get(selected_idx)
        .map_or(VideoModeSelection::Current, |mode| {
            VideoModeSelection::Specific(*mode)
        });

    window.mode = WindowMode::Fullscreen(
        MonitorSelection::Index(monitor.monitor.index),
        video_mode_selection,
    );
}

pub(crate) fn on_toggle_persistence(
    _trigger: On<TogglePersistence>,
    mut persistence: ResMut<ManagedWindowPersistence>,
) {
    *persistence = match *persistence {
        ManagedWindowPersistence::RememberAll => ManagedWindowPersistence::ActiveOnly,
        ManagedWindowPersistence::ActiveOnly => ManagedWindowPersistence::RememberAll,
    };
    info!("[restore_window] Persistence mode: {:?}", *persistence);
}

pub(crate) fn on_clear_state_and_quit(
    _trigger: On<ClearStateAndQuit>,
    managed_entities: Query<Entity, With<ManagedWindow>>,
    mut commands: Commands,
    mut app_exit: MessageWriter<AppExit>,
) {
    if let Some(state_path) = input::get_state_file_path() {
        if let Err(error) = std::fs::remove_file(&state_path) {
            warn!("[restore_window] Failed to remove state file: {error}");
        } else {
            info!("[restore_window] Cleared state file: {state_path:?}");
        }
    }
    input::despawn_managed_and_exit(&managed_entities, &mut commands, &mut app_exit);
}

pub(crate) fn on_quit_app(
    _trigger: On<QuitApp>,
    managed_entities: Query<Entity, With<ManagedWindow>>,
    mut commands: Commands,
    mut app_exit: MessageWriter<AppExit>,
) {
    input::despawn_managed_and_exit(&managed_entities, &mut commands, &mut app_exit);
}
