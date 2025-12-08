//! Monitor management for window restoration.
//!
//! Provides a `Monitors` resource that maintains a sorted list of monitors,
//! automatically updated when monitors are added or removed.

use bevy::prelude::*;
use bevy::window::Monitor;
use bevy::window::MonitorSelection;
use bevy::window::WindowMode;
use bevy::window::WindowPosition;

/// Extension trait for `Window` that provides monitor-aware methods.
///
/// Import this trait to access additional window functionality that requires
/// monitor information.
pub trait WindowExt {
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
    /// use bevy_restore_windows::WindowExt;
    ///
    /// fn check_mode(window: &Window, monitors: Res<Monitors>) {
    ///     let effective = window.effective_mode(&monitors);
    ///     // effective reflects what the user actually sees,
    ///     // even if window.mode says Windowed
    /// }
    /// ```
    fn effective_mode(&self, monitors: &Monitors) -> WindowMode;
}

impl WindowExt for Window {
    fn effective_mode(&self, monitors: &Monitors) -> WindowMode {
        // If Bevy knows it's fullscreen, return as-is
        if !matches!(self.mode, WindowMode::Windowed) {
            return self.mode;
        }

        // Check for macOS green button (fills monitor but mode says Windowed)
        let WindowPosition::At(pos) = self.position else {
            return WindowMode::Windowed;
        };

        let Some(monitor) = monitors.at(pos.x, pos.y) else {
            return WindowMode::Windowed;
        };

        let at_origin = pos.x == monitor.position.x && pos.y == monitor.position.y;
        let fills_monitor =
            self.physical_width() == monitor.size.x && self.physical_height() == monitor.size.y;

        if at_origin && fills_monitor {
            WindowMode::BorderlessFullscreen(MonitorSelection::Index(monitor.index))
        } else {
            WindowMode::Windowed
        }
    }
}

/// Plugin that manages the `Monitors` resource.
pub struct MonitorPlugin;

impl Plugin for MonitorPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreStartup, init_monitors)
            .add_systems(Update, update_monitors);
    }
}

/// Information about a single monitor.
#[derive(Clone, Debug)]
pub struct MonitorInfo {
    /// Index in the sorted monitor list.
    pub index:    usize,
    /// Scale factor (typically 1.0 or 2.0 on macOS).
    pub scale:    f64,
    /// Physical position of top-left corner.
    pub position: IVec2,
    /// Physical size in pixels.
    pub size:     UVec2,
}

/// Sorted monitor list, updated when monitors change.
///
/// Monitors are sorted with primary (at 0,0) first, then by position.
#[derive(Resource)]
pub struct Monitors {
    list: Vec<MonitorInfo>,
}

impl Monitors {
    /// Find monitor containing position (x, y).
    #[must_use]
    #[expect(
        clippy::cast_possible_wrap,
        reason = "monitor dimensions are always within i32 range"
    )]
    pub fn at(&self, x: i32, y: i32) -> Option<&MonitorInfo> {
        self.list.iter().find(|mon| {
            x >= mon.position.x
                && x < mon.position.x + mon.size.x as i32
                && y >= mon.position.y
                && y < mon.position.y + mon.size.y as i32
        })
    }

    /// Get monitor by index in sorted list.
    #[must_use]
    pub fn by_index(&self, index: usize) -> Option<&MonitorInfo> { self.list.get(index) }

    /// Infer monitor index when position is outside all monitor bounds.
    ///
    /// Finds the closest monitor to the given position by calculating
    /// the distance to each monitor's bounding box.
    #[must_use]
    #[expect(
        clippy::cast_possible_wrap,
        reason = "monitor dimensions are always within i32 range"
    )]
    pub fn infer_index(&self, x: i32, y: i32) -> usize {
        if self.list.is_empty() {
            return 0;
        }

        self.list
            .iter()
            .enumerate()
            .min_by_key(|(_, mon)| {
                let right = mon.position.x + mon.size.x as i32;
                let bottom = mon.position.y + mon.size.y as i32;

                // Calculate distance to monitor bounding box
                let dx = if x < mon.position.x {
                    mon.position.x - x
                } else if x >= right {
                    x - right + 1
                } else {
                    0
                };

                let dy = if y < mon.position.y {
                    mon.position.y - y
                } else if y >= bottom {
                    y - bottom + 1
                } else {
                    0
                };

                // Use squared distance to avoid sqrt
                dx * dx + dy * dy
            })
            .map_or(0, |(idx, _)| idx)
    }
}

/// Get sort key for monitor (primary at 0,0 first, then by position).
const fn monitor_sort_key(position: IVec2) -> (bool, i32, i32) {
    let is_primary = position.x == 0 && position.y == 0;
    (!is_primary, position.x, position.y)
}

/// Build sorted monitor list from query.
fn build_monitors(monitors: &Query<&Monitor>) -> Monitors {
    let mut list: Vec<_> = monitors
        .iter()
        .map(|mon| MonitorInfo {
            index:    0, // Will be set after sorting
            scale:    mon.scale_factor,
            position: mon.physical_position,
            size:     mon.physical_size(),
        })
        .collect();

    list.sort_by_key(|mon| monitor_sort_key(mon.position));

    // Update indices after sorting
    for (idx, mon) in list.iter_mut().enumerate() {
        mon.index = idx;
    }

    Monitors { list }
}

/// Initialize `Monitors` resource at startup.
pub fn init_monitors(mut commands: Commands, monitors: Query<&Monitor>) {
    let monitors_resource = build_monitors(&monitors);
    debug!(
        "[init_monitors] Found {} monitors",
        monitors_resource.list.len()
    );
    for mon in &monitors_resource.list {
        debug!(
            "[init_monitors] Monitor {}: pos=({}, {}) size={}x{} scale={}",
            mon.index, mon.position.x, mon.position.y, mon.size.x, mon.size.y, mon.scale
        );
    }
    commands.insert_resource(monitors_resource);
}

/// Update `Monitors` resource when monitors are added or removed.
fn update_monitors(
    mut commands: Commands,
    monitors: Query<&Monitor>,
    added: Query<Entity, Added<Monitor>>,
    mut removed: RemovedComponents<Monitor>,
) {
    let has_changes = !added.is_empty() || removed.read().next().is_some();

    if has_changes {
        let monitors_resource = build_monitors(&monitors);
        debug!(
            "[update_monitors] Monitors changed, now {} monitors",
            monitors_resource.list.len()
        );
        commands.insert_resource(monitors_resource);
    }
}
