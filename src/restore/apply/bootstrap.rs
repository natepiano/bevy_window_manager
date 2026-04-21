use bevy::ecs::system::NonSendMarker;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy::window::WindowMode;
#[cfg(target_os = "linux")]
use bevy::window::WindowPosition;
use bevy::winit::WINIT_WINDOWS;

use crate::Platform;
use crate::WindowKey;
use crate::config::RestoreWindowConfig;
use crate::constants::DEFAULT_SCALE_FACTOR;
use crate::monitors::CurrentMonitor;
use crate::monitors::Monitors;
use crate::persistence;
#[cfg(all(target_os = "windows", feature = "workaround-winit-3124"))]
use crate::persistence::SavedWindowMode;
use crate::restore::plan;
#[cfg(target_os = "linux")]
use crate::restore::target::TargetPosition;
use crate::restore::target::WindowDecoration;
use crate::restore::target::WinitInfo;
use crate::restore::target::X11FrameCompensated;

/// Populate `WinitInfo` resource from winit (decoration and starting monitor).
///
/// # Panics
///
/// Panics if no monitors are available (e.g., laptop lid closed at startup).
/// Window management requires at least one monitor to function.
pub fn init_winit_info(
    mut commands: Commands,
    window_entity: Single<Entity, With<PrimaryWindow>>,
    monitors: Res<Monitors>,
    _non_send: NonSendMarker,
) {
    assert!(
        !monitors.is_empty(),
        "No monitors available - cannot initialize window manager without a display"
    );

    WINIT_WINDOWS.with(|winit_windows| {
        let winit_windows = winit_windows.borrow();
        if let Some(winit_window) = winit_windows.get_window(*window_entity) {
            let outer = winit_window.outer_size();
            let inner = winit_window.inner_size();
            let decoration = WindowDecoration {
                physical_width:  outer.width.saturating_sub(inner.width),
                physical_height: outer.height.saturating_sub(inner.height),
            };

            let position = winit_window.outer_position().map_or(
                IVec2::ZERO,
                |position| IVec2::new(position.x, position.y),
            );

            debug!(
                "[init_winit_info] outer_position={position:?} platform={:?}",
                Platform::detect()
            );

            let starting_monitor = winit_window
                .current_monitor()
                .and_then(|current_monitor| {
                    let monitor_position = current_monitor.position();
                    let info = monitors.at(monitor_position.x, monitor_position.y);
                    debug!(
                        "[init_winit_info] current_monitor() position=({}, {}) -> index={:?}",
                        monitor_position.x,
                        monitor_position.y,
                        info.map(|monitor| monitor.index)
                    );
                    info.copied()
                })
                .unwrap_or_else(|| {
                    debug!(
                        "[init_winit_info] current_monitor() unavailable, falling back to closest_to({}, {})",
                        position.x,
                        position.y
                    );
                    *monitors.closest_to(position.x, position.y)
                });
            let starting_monitor_index = starting_monitor.index;

            debug!(
                "[init_winit_info] decoration={}x{} pos=({}, {}) starting_monitor={starting_monitor_index}",
                decoration.physical_width,
                decoration.physical_height,
                position.x,
                position.y,
            );

            commands.entity(*window_entity).insert(CurrentMonitor {
                monitor:        starting_monitor,
                effective_mode: WindowMode::Windowed,
            });

            commands.insert_resource(WinitInfo {
                starting_monitor_index,
                decoration,
            });
        }
    });
}

/// Load saved window state and insert `TargetPosition` on the primary window entity.
pub fn load_target_position(
    mut commands: Commands,
    window_entity: Single<Entity, With<PrimaryWindow>>,
    monitors: Res<Monitors>,
    winit_info: Res<WinitInfo>,
    mut config: ResMut<RestoreWindowConfig>,
    platform: Res<Platform>,
) {
    if let Some(all_states) = persistence::load_all_states(&config.path) {
        config.loaded_states = all_states;
    }

    let Some(state) = config.loaded_states.get(&WindowKey::Primary).cloned() else {
        debug!("[load_target_position] No saved bevy_window_manager state, showing window");
        commands.queue(|world: &mut World| {
            let mut query = world.query_filtered::<&mut Window, With<PrimaryWindow>>();
            if let Some(mut window) = query.iter_mut(world).next() {
                window.visible = true;
            }
        });
        return;
    };

    debug!(
        "[load_target_position] Loaded state: position={:?} logical_size={}x{} monitor_scale={} monitor_index={} mode={:?}",
        state.logical_position,
        state.logical_width,
        state.logical_height,
        state.scale,
        state.monitor,
        state.mode
    );

    let starting_monitor_index = winit_info.starting_monitor_index;
    let starting_scale = monitors
        .by_index(starting_monitor_index)
        .map_or(DEFAULT_SCALE_FACTOR, |monitor| monitor.scale);

    let resolved =
        plan::resolve_target_monitor_and_position(state.monitor, state.logical_position, &monitors);
    if matches!(
        resolved.source,
        plan::MonitorResolutionSource::FallbackToPrimary
    ) {
        warn!(
            "[load_target_position] Target monitor {} not found, falling back to monitor 0",
            state.monitor
        );
    }

    let target = plan::compute_target_position(
        &state,
        resolved.info,
        resolved.logical_position,
        winit_info.decoration(),
        starting_scale,
        *platform,
    );

    debug!(
        "[load_target_position] Starting monitor={starting_monitor_index} scale={starting_scale}, Target monitor={} scale={}, strategy={:?}, position={:?}",
        target.monitor_index, target.target_scale, target.scale_strategy, target.physical_position
    );

    #[cfg(all(target_os = "windows", feature = "workaround-winit-3124"))]
    if matches!(state.mode, SavedWindowMode::Fullscreen { .. }) {
        debug!(
            "[load_target_position] Windows exclusive fullscreen: showing window for surface creation"
        );
        commands.queue(|world: &mut World| {
            let mut query = world.query_filtered::<&mut Window, With<PrimaryWindow>>();
            if let Some(mut window) = query.iter_mut(world).next() {
                window.visible = true;
            }
        });
    }

    let entity = *window_entity;
    let is_fullscreen = state.mode.is_fullscreen();
    commands.entity(entity).insert(target);

    if is_fullscreen || !platform.needs_frame_compensation() {
        commands.entity(entity).insert(X11FrameCompensated);
    }
}

/// Move the primary window to the target monitor for fullscreen restore on X11.
#[cfg(target_os = "linux")]
pub fn move_to_target_monitor(
    mut window: Single<&mut Window, With<PrimaryWindow>>,
    targets: Query<&TargetPosition, With<PrimaryWindow>>,
    platform: Res<Platform>,
) {
    if platform.is_wayland() {
        return;
    }

    let Ok(target) = targets.single() else {
        return;
    };

    if !target.mode.is_fullscreen() {
        return;
    }

    if let Some(position) = target.physical_position {
        debug!("[move_to_target_monitor] X11 fullscreen: setting position={position:?}");
        window.position = WindowPosition::At(position);
    }
}
