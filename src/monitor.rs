//! Monitor detection logic.
//!
//! Maintains `CurrentMonitor` on all managed windows using winit detection
//! with position-based fallback.

use bevy::ecs::system::NonSendMarker;
use bevy::prelude::*;
use bevy::window::MonitorSelection;
use bevy::window::PrimaryWindow;
use bevy::window::WindowMode;
use bevy::winit::WINIT_WINDOWS;
use bevy_kana::ToI32;

use crate::ManagedWindow;
use crate::monitors::CurrentMonitor;
use crate::monitors::MonitorInfo;
use crate::monitors::Monitors;

/// Unified monitor detection system. Maintains `CurrentMonitor` on all managed windows.
///
/// Detection priority:
/// 1. winit's `current_monitor()` — most reliable, works even before `window.position` is set
/// 2. Position-based center-point detection — uses `window.position` when available
/// 3. Existing `CurrentMonitor` value — preserves last-known monitor during transient states
/// 4. `monitors.first()` — last resort fallback
///
/// All platforms: computes `effective_mode` (handles macOS green button fullscreen).
pub(crate) fn update_current_monitor(
    mut commands: Commands,
    windows: Query<
        (Entity, &Window, Option<&CurrentMonitor>),
        Or<(With<PrimaryWindow>, With<ManagedWindow>)>,
    >,
    monitors: Res<Monitors>,
    _: NonSendMarker,
) {
    if monitors.is_empty() {
        return;
    }

    for (entity, window, existing) in &windows {
        let winit_result = winit_detect_monitor(entity, &monitors);
        let position_result = if winit_result.is_none() {
            position_detect_monitor(window, &monitors)
        } else {
            None
        };

        let (monitor_info, source) = match (winit_result, position_result, existing) {
            (Some(info), _, _) => (info, "winit"),
            (_, Some(info), _) => (info, "position"),
            (_, _, Some(cm)) => (cm.monitor, "existing"),
            _ => (*monitors.first(), "fallback"),
        };

        // Compute effective mode
        let effective_mode = compute_effective_mode(window, &monitor_info, &monitors);

        let new_current = CurrentMonitor {
            monitor: monitor_info,
            effective_mode,
        };

        // Only insert if changed to avoid unnecessary change detection triggers
        let changed = existing.is_none_or(|cm| {
            cm.monitor.index != new_current.monitor.index
                || cm.effective_mode != new_current.effective_mode
        });

        if changed {
            debug!(
                "[update_current_monitor] source={} index={} scale={} effective_mode={:?}",
                source, monitor_info.index, monitor_info.scale, effective_mode
            );
            commands.entity(entity).insert(new_current);
        }
    }
}

/// Detect monitor via winit's `current_monitor()`.
fn winit_detect_monitor(entity: Entity, monitors: &Monitors) -> Option<MonitorInfo> {
    WINIT_WINDOWS.with(|winit_windows| {
        let winit_windows = winit_windows.borrow();
        winit_windows.get_window(entity).and_then(|winit_window| {
            winit_window.current_monitor().and_then(|current_monitor| {
                let pos = current_monitor.position();
                monitors.at(pos.x, pos.y).copied()
            })
        })
    })
}

/// Detect monitor from `window.position` using center-point logic.
fn position_detect_monitor(window: &Window, monitors: &Monitors) -> Option<MonitorInfo> {
    if let WindowPosition::At(pos) = window.position {
        Some(*monitors.monitor_for_window(pos, window.physical_width(), window.physical_height()))
    } else {
        None
    }
}

/// Compute the effective window mode, including macOS green button detection.
///
/// On macOS, clicking the green "maximize" button fills the screen but `window.mode`
/// remains `Windowed`. This detects that case and returns `BorderlessFullscreen`.
fn compute_effective_mode(
    window: &Window,
    monitor_info: &MonitorInfo,
    monitors: &Monitors,
) -> WindowMode {
    // Trust exclusive fullscreen - OS manages this mode
    if matches!(window.mode, WindowMode::Fullscreen(_, _)) {
        return window.mode;
    }

    // Can't determine effective mode without monitors
    if monitors.is_empty() {
        return window.mode;
    }

    // On Wayland, position is unavailable so we can only trust self.mode
    let WindowPosition::At(pos) = window.position else {
        return window.mode;
    };

    // Check if window spans full width and reaches bottom of monitor
    let full_width = window.physical_width() == monitor_info.physical_size.x;
    let left_aligned = pos.x == monitor_info.physical_position.x;
    let reaches_bottom = pos.y + window.physical_height().to_i32()
        == monitor_info.physical_position.y + monitor_info.physical_size.y.to_i32();

    if full_width && left_aligned && reaches_bottom {
        WindowMode::BorderlessFullscreen(MonitorSelection::Index(monitor_info.index))
    } else {
        WindowMode::Windowed
    }
}

#[cfg(test)]
mod tests {
    use bevy::window::MonitorSelection;
    use bevy::window::VideoModeSelection;
    use bevy::window::WindowMode;
    use bevy::window::WindowPosition;

    use super::*;

    fn monitor_0() -> MonitorInfo {
        MonitorInfo {
            index:             0,
            scale:             2.0,
            physical_position: IVec2::ZERO,
            physical_size:     UVec2::new(3456, 2234),
        }
    }

    fn monitors_with(info: MonitorInfo) -> Monitors { Monitors { list: vec![info] } }

    fn window_at(pos: IVec2, width: u32, height: u32) -> Window {
        let mut window = Window {
            position: WindowPosition::At(pos),
            mode: WindowMode::Windowed,
            ..Default::default()
        };
        window.resolution.set_physical_resolution(width, height);
        window
    }

    #[test]
    fn effective_mode_fullscreen_when_window_fills_monitor() {
        let mon = monitor_0();
        let monitors = monitors_with(mon);
        let window = window_at(
            mon.physical_position,
            mon.physical_size.x,
            mon.physical_size.y,
        );

        let mode = compute_effective_mode(&window, &mon, &monitors);
        assert_eq!(
            mode,
            WindowMode::BorderlessFullscreen(MonitorSelection::Index(0))
        );
    }

    #[test]
    fn effective_mode_windowed_when_window_smaller_than_monitor() {
        let mon = monitor_0();
        let monitors = monitors_with(mon);
        let window = window_at(IVec2::new(100, 100), 1600, 1200);

        let mode = compute_effective_mode(&window, &mon, &monitors);
        assert_eq!(mode, WindowMode::Windowed);
    }

    #[test]
    fn effective_mode_windowed_when_not_left_aligned() {
        let mon = monitor_0();
        let monitors = monitors_with(mon);
        // Full width + reaches bottom, but offset from left edge
        let window = window_at(IVec2::new(1, 0), mon.physical_size.x, mon.physical_size.y);

        let mode = compute_effective_mode(&window, &mon, &monitors);
        assert_eq!(mode, WindowMode::Windowed);
    }

    #[test]
    fn effective_mode_trusts_exclusive_fullscreen() {
        let mon = monitor_0();
        let monitors = monitors_with(mon);
        let mut window = window_at(IVec2::ZERO, 800, 600);
        window.mode =
            WindowMode::Fullscreen(MonitorSelection::Index(0), VideoModeSelection::Current);

        let mode = compute_effective_mode(&window, &mon, &monitors);
        assert!(matches!(mode, WindowMode::Fullscreen(_, _)));
    }

    #[test]
    fn effective_mode_returns_mode_when_no_position() {
        let mon = monitor_0();
        let monitors = monitors_with(mon);
        let mut window = Window::default();
        window
            .resolution
            .set_physical_resolution(mon.physical_size.x, mon.physical_size.y);
        // position is Automatic (no position available, like Wayland)

        let mode = compute_effective_mode(&window, &mon, &monitors);
        assert_eq!(mode, WindowMode::Windowed);
    }

    #[test]
    fn effective_mode_returns_mode_when_no_monitors() {
        let mon = monitor_0();
        let empty = Monitors { list: vec![] };
        let window = window_at(IVec2::ZERO, mon.physical_size.x, mon.physical_size.y);

        let mode = compute_effective_mode(&window, &mon, &empty);
        assert_eq!(mode, WindowMode::Windowed);
    }
}
