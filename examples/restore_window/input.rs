use std::path::PathBuf;

use bevy::prelude::*;
use bevy::window::Monitor;
use bevy::window::VideoMode;
use bevy::window::VideoModeSelection;
use bevy::window::WindowMode;
use bevy_window_manager::CurrentMonitor;
use bevy_window_manager::ManagedWindow;
use bevy_window_manager::Monitors;

use super::events::ClearStateAndQuit;
use super::events::QuitApp;
use super::events::SetBorderlessFullscreen;
use super::events::SetExclusiveFullscreen;
use super::events::SetWindowed;
use super::events::SpawnManagedWindow;
use super::events::TogglePersistence;
use super::state::RestoredStates;
use super::state::SelectedVideoModes;

pub(crate) fn handle_global_input(
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window>,
    mut commands: Commands,
) {
    if !windows.iter().any(|window| window.focused) {
        return;
    }

    if keys.just_pressed(KeyCode::Space) {
        commands.trigger(SpawnManagedWindow);
    }
    if keys.just_pressed(KeyCode::KeyP) {
        commands.trigger(TogglePersistence);
    }
    if keys.just_pressed(KeyCode::Backspace)
        && keys.pressed(KeyCode::ShiftLeft)
        && keys.pressed(KeyCode::ControlLeft)
    {
        commands.trigger(ClearStateAndQuit);
    }
    if keys.just_pressed(KeyCode::KeyQ) {
        commands.trigger(QuitApp);
    }
}

pub(crate) fn despawn_managed_and_exit(
    managed_entities: &Query<Entity, With<ManagedWindow>>,
    commands: &mut Commands,
    app_exit: &mut MessageWriter<AppExit>,
) {
    for entity in managed_entities.iter() {
        commands.entity(entity).despawn();
    }
    app_exit.write(AppExit::Success);
}

pub(crate) fn get_state_file_path() -> Option<PathBuf> {
    let exe_name = std::env::current_exe()
        .ok()?
        .file_stem()?
        .to_str()?
        .to_string();
    dirs::config_dir().map(|config_dir| config_dir.join(exe_name).join("windows.ron"))
}

pub(crate) fn handle_window_mode_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut windows: Query<(Entity, &mut Window, Option<&CurrentMonitor>)>,
    monitors_res: Res<Monitors>,
    bevy_monitors: Query<(Entity, &Monitor)>,
    mut selected: ResMut<SelectedVideoModes>,
    restored_states: Res<RestoredStates>,
    mut commands: Commands,
) {
    let Some((entity, mut window, current_monitor)) =
        windows.iter_mut().find(|(_, window, _)| window.focused)
    else {
        return;
    };

    let monitor = current_monitor.copied().unwrap_or_else(|| CurrentMonitor {
        monitor:        *monitors_res.first(),
        effective_mode: window.mode,
    });

    let is_fullscreen = !matches!(window.mode, WindowMode::Windowed);
    let restore_complete = restored_states.states.contains_key(&entity);
    let mode_desynced = window.mode != monitor.effective_mode;
    if restore_complete && !is_fullscreen && mode_desynced {
        window.mode = monitor.effective_mode;
    }

    let video_modes: Vec<VideoMode> = bevy_monitors
        .iter()
        .find(|(_, bevy_monitor)| bevy_monitor.physical_position == monitor.position)
        .map(|(_, bevy_monitor)| bevy_monitor.video_modes.clone())
        .unwrap_or_default();

    let current_idx = selected.get(monitor.index);
    if keys.just_pressed(KeyCode::ArrowUp) && current_idx > 0 {
        selected.set(monitor.index, current_idx - 1);
    }
    if keys.just_pressed(KeyCode::ArrowDown) && current_idx < video_modes.len().saturating_sub(1) {
        selected.set(monitor.index, current_idx + 1);
    }

    if keys.just_pressed(KeyCode::Enter) {
        commands.trigger(SetExclusiveFullscreen);
    }
    if keys.just_pressed(KeyCode::KeyB) {
        commands.trigger(SetBorderlessFullscreen);
    }
    if keys.just_pressed(KeyCode::KeyW) {
        commands.trigger(SetWindowed);
    }
}

pub(crate) fn get_video_modes_for_monitor<'a>(
    bevy_monitors: &'a Query<(Entity, &Monitor)>,
    monitor: &CurrentMonitor,
) -> (Vec<&'a VideoMode>, Option<u32>) {
    bevy_monitors
        .iter()
        .find(|(_, bevy_monitor)| bevy_monitor.physical_position == monitor.position)
        .map(|(_, bevy_monitor)| {
            (
                bevy_monitor.video_modes.iter().collect(),
                bevy_monitor.refresh_rate_millihertz.map(|rate| rate / 1000),
            )
        })
        .unwrap_or_default()
}

pub(crate) fn format_refresh_rate(window: &Window, monitor_refresh: Option<u32>) -> String {
    let active_refresh = match &window.mode {
        WindowMode::Fullscreen(_, VideoModeSelection::Specific(mode)) => {
            Some(mode.refresh_rate_millihertz / 1000)
        },
        _ => monitor_refresh,
    };
    active_refresh.map_or_else(|| "N/A".into(), |hz| format!("{hz}Hz"))
}

pub(crate) fn find_active_video_mode_index(
    window: &Window,
    video_modes: &[&VideoMode],
) -> Option<usize> {
    match &window.mode {
        WindowMode::Fullscreen(_, VideoModeSelection::Specific(active)) => {
            video_modes.iter().position(|mode| {
                mode.physical_size == active.physical_size
                    && mode.refresh_rate_millihertz == active.refresh_rate_millihertz
            })
        },
        _ => None,
    }
}

pub(crate) fn sync_selected_to_active(
    window: &Window,
    monitor: &CurrentMonitor,
    active_mode_idx: Option<usize>,
    selected: &mut SelectedVideoModes,
) {
    if let WindowMode::Fullscreen(_, VideoModeSelection::Specific(active)) = &window.mode {
        let current_mode = (active.physical_size, active.refresh_rate_millihertz);
        if selected.last_sync != Some(current_mode)
            && let Some(active_idx) = active_mode_idx
        {
            selected.set(monitor.index, active_idx);
            selected.last_sync = Some(current_mode);
        }
    } else {
        selected.last_sync = None;
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn platform_suffix() -> &'static str {
    use bevy_window_manager::Platform;

    if Platform::detect().is_wayland() {
        " (Wayland)"
    } else {
        " (X11)"
    }
}

#[cfg(not(target_os = "linux"))]
pub(crate) const fn platform_suffix() -> &'static str { "" }

pub(crate) fn format_monitor_row(monitor: &CurrentMonitor, refresh_display: &str) -> String {
    let primary_marker = if monitor.index == 0 {
        " Primary Monitor -"
    } else {
        " -"
    };
    format!(
        "Monitor: {}{primary_marker} Scale: {} - Refresh Rate: {refresh_display}{}",
        monitor.index,
        monitor.scale,
        platform_suffix()
    )
}

pub(crate) fn build_video_modes_display(
    video_modes: &[&VideoMode],
    selected_idx: usize,
    active_mode_idx: Option<usize>,
) -> String {
    if video_modes.is_empty() {
        return "  (no video modes available)".into();
    }

    let selected_idx = selected_idx.min(video_modes.len().saturating_sub(1));
    let len = video_modes.len();

    let start = if len <= 5 {
        0
    } else {
        let center_target = active_mode_idx.unwrap_or(selected_idx);
        let ideal_start = center_target.saturating_sub(2);
        let ideal_end = ideal_start + 5;

        if selected_idx < ideal_start {
            selected_idx.saturating_sub(2)
        } else if selected_idx >= ideal_end {
            (selected_idx + 3).saturating_sub(5)
        } else {
            ideal_start
        }
        .min(len.saturating_sub(5))
    };
    let end = (start + 5).min(len);

    video_modes[start..end]
        .iter()
        .enumerate()
        .map(|(i, mode)| {
            let actual_idx = start + i;
            let left_marker = if actual_idx == selected_idx { ">" } else { " " };
            let right_marker = if Some(actual_idx) == active_mode_idx {
                " <- active"
            } else {
                ""
            };
            format!(
                "  {left_marker} {}x{} @ {}Hz{right_marker}",
                mode.physical_size.x,
                mode.physical_size.y,
                mode.refresh_rate_millihertz / 1000
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}
