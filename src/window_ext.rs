//! Extension trait for `Window` providing monitor-aware methods.

use bevy::prelude::*;
use bevy::window::MonitorSelection;
use bevy::window::WindowMode;
use bevy::window::WindowPosition;

use crate::MonitorInfo;
use crate::Monitors;

/// Extension trait for `Window` providing monitor-aware methods.
///
/// Import this trait to access methods that require monitor information.
pub trait WindowExt {
    /// Get the monitor this window is currently on.
    ///
    /// If the window position is unknown or outside all monitors, returns the
    /// closest monitor (or monitor 0 as a last resort).
    ///
    /// # Example
    ///
    /// ```ignore
    /// use bevy_window_manager::WindowExt;
    ///
    /// fn show_monitor(window: &Window, monitors: Res<Monitors>) {
    ///     let monitor = window.monitor(&monitors);
    ///     println!("Window is on monitor {}", monitor.index);
    /// }
    /// ```
    fn monitor<'a>(&self, monitors: &'a Monitors) -> &'a MonitorInfo;

    /// Detect the effective window mode, including macOS green button detection.
    ///
    /// On macOS, clicking the green "maximize" button makes the window fill the
    /// screen, but `window.mode` remains `Windowed`. This method detects that case
    /// and returns `BorderlessFullscreen` with the correct monitor selection.
    ///
    /// # Returns
    ///
    /// A properly populated [`WindowMode`]:
    /// - If `window.mode` is already fullscreen, returns it unchanged
    /// - If window fills a monitor (macOS green button), returns `BorderlessFullscreen` with
    ///   [`MonitorSelection::Index`] set to the correct monitor
    /// - Otherwise returns `Windowed`
    ///
    /// # Example
    ///
    /// ```ignore
    /// use bevy_window_manager::WindowExt;
    ///
    /// fn check_mode(window: &Window, monitors: Res<Monitors>) {
    ///     let effective = window.effective_mode(&monitors);
    ///     // effective reflects what the user actually sees,
    ///     // even if window.mode says Windowed
    /// }
    /// ```
    fn effective_mode(&self, monitors: &Monitors) -> WindowMode;

    /// Set window position and size in one call.
    ///
    /// This is a convenience method that sets both `window.position` to
    /// [`WindowPosition::At`] and calls `resolution.set_physical_resolution`.
    fn set_position_and_size(&mut self, position: IVec2, size: UVec2);
}

impl WindowExt for Window {
    fn monitor<'a>(&self, monitors: &'a Monitors) -> &'a MonitorInfo {
        let WindowPosition::At(pos) = self.position else {
            return monitors.primary();
        };
        // Use window center for monitor detection because:
        // - It correctly handles windows spanning monitor boundaries
        // - It avoids Windows invisible border offset (winit #4107) where maximized/snapped windows
        //   report top-left outside monitor bounds
        let center_x = pos.x + (self.physical_width() / 2) as i32;
        let center_y = pos.y + (self.physical_height() / 2) as i32;
        monitors.closest_to(center_x, center_y)
    }

    fn effective_mode(&self, monitors: &Monitors) -> WindowMode {
        // Trust any fullscreen mode set programmatically
        if matches!(
            self.mode,
            WindowMode::Fullscreen(_, _) | WindowMode::BorderlessFullscreen(_)
        ) {
            return self.mode;
        }

        // For Windowed mode, check actual screen coverage to detect macOS green button
        // fullscreen where Bevy doesn't update window.mode.
        // On Wayland, position is unavailable so we can only trust self.mode.
        let WindowPosition::At(pos) = self.position else {
            return self.mode;
        };

        let monitor = self.monitor(monitors);

        // Check if window spans full width and reaches bottom of monitor.
        // We check edges rather than origin+size because on some displays
        // (e.g., primary monitor with menu bar), the reported position may
        // be offset even when the window visually fills the entire screen.
        let full_width = self.physical_width() == monitor.size.x;
        let left_aligned = pos.x == monitor.position.x;
        let reaches_bottom =
            pos.y + self.physical_height() as i32 == monitor.position.y + monitor.size.y as i32;

        if full_width && left_aligned && reaches_bottom {
            WindowMode::BorderlessFullscreen(MonitorSelection::Index(monitor.index))
        } else {
            WindowMode::Windowed
        }
    }

    fn set_position_and_size(&mut self, position: IVec2, size: UVec2) {
        self.position = WindowPosition::At(position);
        self.resolution.set_physical_resolution(size.x, size.y);
    }
}
