//! Monitor management for window restoration.
//!
//! Provides a `Monitors` resource that maintains a sorted list of monitors,
//! automatically updated when monitors are added or removed.

use bevy::prelude::*;
use bevy::window::Monitor;

/// Plugin that manages the `Monitors` resource.
pub struct MonitorPlugin;

impl Plugin for MonitorPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreStartup, init_monitors)
            .add_systems(Update, update_monitors);
    }
}

/// Information about a single monitor.
#[derive(Clone, Copy, Debug, Reflect)]
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
#[derive(Resource, Reflect)]
#[reflect(Resource)]
pub struct Monitors {
    list: Vec<MonitorInfo>,
}

/// Component storing the current monitor for a window.
///
/// Query this alongside your window to get monitor information:
/// ```ignore
/// fn my_system(q: Query<(&Window, &CurrentMonitor), With<PrimaryWindow>>) {
///     let (window, monitor) = q.single();
///     println!("Window on monitor {} at scale {}", monitor.index, monitor.scale);
/// }
/// ```
#[derive(Component, Clone, Copy, Debug, Reflect)]
pub struct CurrentMonitor(pub MonitorInfo);

impl std::ops::Deref for CurrentMonitor {
    type Target = MonitorInfo;

    fn deref(&self) -> &Self::Target { &self.0 }
}

impl Monitors {
    /// Find monitor containing position (x, y).
    #[must_use]
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

    /// Get the first monitor (index 0). Used as fallback when no specific monitor is known.
    ///
    /// # Panics
    ///
    /// Panics if no monitors exist (should never happen on a real system).
    #[must_use]
    pub fn first(&self) -> &MonitorInfo { &self.list[0] }

    /// Find the monitor at position, or the closest one if outside all bounds.
    ///
    /// Unlike [`at`](Self::at), this always returns a monitor by finding
    /// the closest monitor when position is outside all bounds.
    #[must_use]
    pub fn closest_to(&self, x: i32, y: i32) -> &MonitorInfo {
        // Try exact match first
        if let Some(monitor) = self.at(x, y) {
            return monitor;
        }

        // Find closest monitor by distance to bounding box
        self.list
            .iter()
            .min_by_key(|mon| {
                let right = mon.position.x + mon.size.x as i32;
                let bottom = mon.position.y + mon.size.y as i32;

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

                dx * dx + dy * dy
            })
            .unwrap_or(&self.list[0])
    }
}

/// Build monitor list from query (preserves winit enumeration order).
fn build_monitors(monitors: &Query<&Monitor>) -> Monitors {
    let list: Vec<_> = monitors
        .iter()
        .enumerate()
        .map(|(idx, mon)| MonitorInfo {
            index:    idx,
            scale:    mon.scale_factor,
            position: mon.physical_position,
            size:     mon.physical_size(),
        })
        .collect();

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
