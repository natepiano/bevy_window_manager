//! Window state persistence.
//!
//! Saves window position, size, and mode to the state file on change.

use bevy::ecs::system::NonSendMarker;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
#[cfg(any(
    target_os = "macos",
    all(target_os = "linux", feature = "workaround-winit-4443")
))]
use bevy::winit::WINIT_WINDOWS;
use bevy_kana::ToI32;
use bevy_kana::ToU32;

use super::format;
use super::format::WindowKey;
use super::load;
use super::types::SavedWindowMode;
use super::types::WindowState;
use crate::ManagedWindow;
use crate::ManagedWindowPersistence;
use crate::config::RestoreWindowConfig;
use crate::constants::DEFAULT_SCALE_FACTOR;
use crate::monitors::CurrentMonitor;
use crate::monitors::Monitors;

/// Save all window states to the given path.
pub fn save_all_states(
    path: &std::path::Path,
    states: &std::collections::HashMap<WindowKey, WindowState>,
) {
    if let Some(parent) = path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        warn!("[save_all_states] Failed to create directory {parent:?}: {e}");
        return;
    }
    match format::encode(states) {
        Ok(contents) => {
            if let Err(e) = std::fs::write(path, &contents) {
                warn!("[save_all_states] Failed to write state file {path:?}: {e}");
            }
        },
        Err(e) => {
            warn!("[save_all_states] Failed to serialize state: {e}");
        },
    }
}

/// Cached window state for change detection comparison.
#[derive(Default)]
pub struct CachedWindowState {
    position: Option<IVec2>,
    size:     UVec2,
    mode:     Option<SavedWindowMode>,
    monitor:  Option<usize>,
}

/// Build state from all currently-active windows and write it to the state file.
///
/// Iterates every primary and managed window, captures position/size/monitor/mode,
/// and writes the full persisted state map in one shot. Used by the
/// `ActiveOnly` persistence mode so that the file always reflects exactly which
/// windows are open right now.
///
/// `exclude_entity` allows callers (e.g., `On<Remove>` observers) to skip an entity
/// whose component is still visible in the query but is being removed.
pub fn save_active_window_state(
    config: &RestoreWindowConfig,
    monitors: &Monitors,
    all_windows: &Query<
        (
            Entity,
            &Window,
            Option<&CurrentMonitor>,
            Option<&ManagedWindow>,
        ),
        Or<(With<PrimaryWindow>, With<ManagedWindow>)>,
    >,
    primary_q: &Query<(), With<PrimaryWindow>>,
    exclude_entity: Option<Entity>,
) {
    if monitors.is_empty() {
        return;
    }

    let app_name = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_stem().and_then(|s| s.to_str()).map(String::from))
        .unwrap_or_default();

    let mut states = std::collections::HashMap::new();

    for (entity, window, existing_monitor, managed) in all_windows {
        if exclude_entity == Some(entity) {
            continue;
        }

        let key = if primary_q.get(entity).is_ok() {
            WindowKey::Primary
        } else if let Some(m) = managed {
            WindowKey::Managed(m.name.clone())
        } else {
            continue;
        };

        let pos = get_window_position(entity, window);

        let (monitor_index, monitor_scale) = existing_monitor.map_or_else(
            || {
                let p = monitors.first();
                (p.index, p.scale)
            },
            |m| (m.index, m.scale),
        );
        let mode: SavedWindowMode =
            existing_monitor.map_or_else(|| (&window.mode).into(), |m| (&m.effective_mode).into());
        let position = pos.map(|p| {
            let logical_x = (f64::from(p.x) / monitor_scale).round().to_i32();
            let logical_y = (f64::from(p.y) / monitor_scale).round().to_i32();
            (logical_x, logical_y)
        });
        states.insert(
            key,
            WindowState {
                logical_position: position,
                logical_width: window.resolution.width().to_u32(),
                logical_height: window.resolution.height().to_u32(),
                scale: monitor_scale,
                monitor: monitor_index,
                mode,
                app_name: app_name.clone(),
            },
        );
    }

    save_all_states(&config.path, &states);
}

/// Persist window states using the `RememberAll` strategy: load existing file,
/// merge with cached entries, and save. Preserves entries for closed windows.
fn persist_remember_all(
    config: &RestoreWindowConfig,
    monitors: &Monitors,
    cached: &std::collections::HashMap<Entity, CachedWindowState>,
    all_windows: &Query<
        (
            Entity,
            &Window,
            Option<&CurrentMonitor>,
            Option<&ManagedWindow>,
        ),
        Or<(With<PrimaryWindow>, With<ManagedWindow>)>,
    >,
    primary_q: &Query<(), With<PrimaryWindow>>,
) {
    let app_name = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_stem().and_then(|s| s.to_str()).map(String::from))
        .unwrap_or_default();

    let mut states = load::load_all_states(&config.path).unwrap_or_default();

    // Update with current window states from cache
    for (entity, entry) in cached {
        let key = if primary_q.get(*entity).is_ok() {
            WindowKey::Primary
        } else if let Ok((_, _, _, Some(managed))) = all_windows.get(*entity) {
            WindowKey::Managed(managed.name.clone())
        } else {
            // Entity may have been despawned - skip stale cached entry
            continue;
        };

        if let Some(mode) = &entry.mode {
            let monitor_index = entry.monitor.unwrap_or(0);
            let monitor_scale = monitors
                .by_index(monitor_index)
                .map_or(DEFAULT_SCALE_FACTOR, |m| m.scale);
            let position = entry.position.map(|p| {
                let logical_x = (f64::from(p.x) / monitor_scale).round().to_i32();
                let logical_y = (f64::from(p.y) / monitor_scale).round().to_i32();
                (logical_x, logical_y)
            });
            states.insert(
                key,
                WindowState {
                    logical_position: position,
                    logical_width:    entry.size.x,
                    logical_height:   entry.size.y,
                    scale:            monitor_scale,
                    monitor:          monitor_index,
                    mode:             mode.clone(),
                    app_name:         app_name.clone(),
                },
            );
        }
    }

    save_all_states(&config.path, &states);
}

/// Save window state when position, size, or mode changes. Runs only when not restoring.
///
/// Handles both the primary window and any `ManagedWindow` entities. Uses
/// `ManagedWindowPersistence` to decide whether closed windows keep their saved state.
pub fn save_window_state(
    config: Res<RestoreWindowConfig>,
    monitors: Res<Monitors>,
    persistence: Res<ManagedWindowPersistence>,
    windows: Query<
        (
            Entity,
            &Window,
            Option<&CurrentMonitor>,
            Option<&ManagedWindow>,
        ),
        (
            Or<(With<PrimaryWindow>, With<ManagedWindow>)>,
            Or<(Changed<Window>, Changed<CurrentMonitor>)>,
        ),
    >,
    all_windows: Query<
        (
            Entity,
            &Window,
            Option<&CurrentMonitor>,
            Option<&ManagedWindow>,
        ),
        Or<(With<PrimaryWindow>, With<ManagedWindow>)>,
    >,
    primary_q: Query<(), With<PrimaryWindow>>,
    mut cached: Local<std::collections::HashMap<Entity, CachedWindowState>>,
    _non_send: NonSendMarker,
) {
    // Can't save state if no monitors exist (e.g., laptop lid closed).
    if monitors.is_empty() {
        return;
    }

    let mut any_changed = false;

    for (window_entity, window, existing_monitor, managed) in &windows {
        // Determine the key for this window in the state file
        let key = if primary_q.get(window_entity).is_ok() {
            WindowKey::Primary
        } else if let Some(m) = managed {
            WindowKey::Managed(m.name.clone())
        } else {
            continue;
        };

        // Get window position for saving state.
        let pos = get_window_position(window_entity, window);

        let physical_w = window.resolution.physical_width();
        let physical_h = window.resolution.physical_height();
        let logical_w = window.resolution.width().to_u32();
        let logical_h = window.resolution.height().to_u32();
        let res_scale = window.resolution.scale_factor();

        // Read monitor and effective mode from `CurrentMonitor` (maintained by
        // `update_current_monitor`)
        let (monitor_index, monitor_scale) = existing_monitor.map_or_else(
            || {
                let p = monitors.first();
                (p.index, p.scale)
            },
            |m| (m.index, m.scale),
        );
        let mode: SavedWindowMode =
            existing_monitor.map_or_else(|| (&window.mode).into(), |m| (&m.effective_mode).into());

        let entry = cached.entry(window_entity).or_default();

        // Only save if position, size, or mode actually changed
        let position_changed = entry.position != pos;
        let size_changed = entry.size != UVec2::new(logical_w, logical_h);
        let mode_changed = entry.mode.as_ref() != Some(&mode);
        let monitor_changed = entry.monitor != Some(monitor_index);

        if !position_changed && !size_changed && !mode_changed && !monitor_changed {
            continue;
        }

        debug!(
            "[save_window_state] [{key}] SAVE DETAIL: pos={pos:?} physical={physical_w}x{physical_h} logical={logical_w}x{logical_h} res_scale={res_scale} monitor={monitor_index} mode={mode:?}",
        );

        // Log monitor transitions with detailed info
        if monitor_changed {
            let prev_scale = entry
                .monitor
                .and_then(|i| monitors.by_index(i))
                .map(|m| m.scale);
            debug!(
                "[save_window_state] [{key}] MONITOR CHANGE: {:?} (scale={prev_scale:?}) -> {monitor_index} (scale={monitor_scale})",
                entry.monitor,
            );
        }

        // Update cache
        entry.position = pos;
        entry.size = UVec2::new(logical_w, logical_h);
        entry.mode = Some(mode.clone());
        entry.monitor = Some(monitor_index);

        any_changed = true;

        debug!(
            "[save_window_state] [{key}] pos={pos:?} logical={logical_w}x{logical_h} physical={physical_w}x{physical_h} monitor={monitor_index} scale={monitor_scale} mode={mode:?}",
        );
    }

    if !any_changed {
        return;
    }

    match *persistence {
        ManagedWindowPersistence::ActiveOnly => {
            // Build state from all active windows and write in one shot
            save_active_window_state(&config, &monitors, &all_windows, &primary_q, None);
        },
        ManagedWindowPersistence::RememberAll => {
            persist_remember_all(&config, &monitors, &cached, &all_windows, &primary_q);
        },
    }
}

/// Get window position from the OS via winit, falling back to `Window.position`.
///
/// On macOS, `Window.position` stays `Automatic` even after the OS places the window,
/// so we must query winit directly. On Linux with W5 workaround, we also use winit
/// to get `outer_position` (frame origin). On other platforms, `Window.position` suffices.
pub(super) fn get_window_position(entity: Entity, window: &Window) -> Option<IVec2> {
    #[cfg(any(
        target_os = "macos",
        all(target_os = "linux", feature = "workaround-winit-4443")
    ))]
    {
        let _ = window;
        WINIT_WINDOWS.with(|winit_windows| {
            let winit_windows = winit_windows.borrow();
            let winit_win = winit_windows.get_window(entity)?;
            let outer_pos = winit_win.outer_position().ok()?;
            Some(IVec2::new(outer_pos.x, outer_pos.y))
        })
    }
    #[cfg(not(any(
        target_os = "macos",
        all(target_os = "linux", feature = "workaround-winit-4443")
    )))]
    {
        let _ = entity;
        match window.position {
            WindowPosition::At(p) => Some(p),
            _ => None,
        }
    }
}
