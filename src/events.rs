//! Public API events for window restoration.

use bevy::prelude::*;
use bevy::window::WindowMode;

use crate::WindowKey;

/// Event fired when a window restore completes and the window becomes visible.
///
/// This is an [`EntityEvent`] triggered on the window entity at the end of the restore
/// process, after position, size, and mode have been applied. Dependent crates can
/// observe this event to know the final restored window state.
///
/// Use an observer to receive this event:
/// ```ignore
/// // For all windows
/// app.add_observer(|trigger: On<WindowRestored>| {
///     let event = trigger.event();
///     // Use event.entity, event.size, event.mode, etc.
/// });
///
/// // For primary window only - check event.entity against PrimaryWindow query
/// fn on_window_restored(
///     trigger: On<WindowRestored>,
///     primary_window: Query<(), With<PrimaryWindow>>,
/// ) {
///     let event = trigger.event();
///     if primary_window.get(event.entity).is_ok() {
///         // Handle primary window only
///     }
/// }
/// ```
#[derive(EntityEvent, Debug, Clone, Reflect)]
pub struct WindowRestored {
    /// The window entity this event targets.
    pub entity:            Entity,
    /// Identifier for this window (primary or managed name).
    pub window_id:         WindowKey,
    /// Target position in physical pixels (None on Wayland).
    pub physical_position: Option<IVec2>,
    /// Target position in logical pixels (pre-scale, from the saved state).
    /// None on Wayland or when the saved state had no position.
    pub logical_position:  Option<IVec2>,
    /// Target physical size that was applied (content area).
    pub physical_size:     UVec2,
    /// Target logical size that was applied (content area).
    pub logical_size:      UVec2,
    /// Window mode that was applied.
    pub mode:              WindowMode,
    /// Monitor index the window was restored to.
    pub monitor_index:     usize,
}

/// Event fired when the actual window state doesn't match what was requested.
///
/// After `try_apply_restore` completes, the library compares the intended restore
/// target against the live window state. If any field differs, this event fires
/// instead of [`WindowRestored`].
///
/// ## Sources
///
/// **Expected values** come from [`TargetPosition`](super::TargetPosition), which is computed
/// from the saved RON state file at startup. These represent what the restore *intended* to
/// achieve.
///
/// **Actual values** come from two live ECS sources, each chosen for accuracy:
///
/// - **`monitor_index`** → [`CurrentMonitor`](crate::CurrentMonitor) component, maintained by
///   `update_current_monitor` which queries winit's `current_monitor()` and maps it to the
///   `Monitors` list. This updates quickly when the compositor moves the window.
///
/// - **`position`, `size`, `mode`, `scale`** → the [`Window`](bevy::window::Window) component.
///   Position and size reflect `window.position` / `window.resolution`, and scale comes from
///   `window.resolution.scale_factor()`. These lag behind the compositor because they only update
///   when winit fires corresponding events (`ScaleFactorChanged`, `Resized`, `Moved`). A common
///   mismatch is the scale factor still reflecting the launch monitor while `CurrentMonitor` has
///   already updated to the target monitor.
///
/// This intentional split means a mismatch signals that the window hasn't fully settled
/// — the compositor accepted the request but winit hasn't yet delivered all the
/// resulting state changes.
///
/// ## Field layout
///
/// The `expected_*` / `actual_*` pairs are deliberately flat rather than grouped into
/// nested comparison structs — the event is primarily consumed via reflection (BRP /
/// observers), where flat fields are easier to address than nested ones. The
/// `restore_window` example adapts this flat shape into nested `*Mismatch` types in
/// `examples/restore_window/state.rs`; any future reshape of the fields here must
/// update that adapter in tandem.
#[derive(EntityEvent, Debug, Clone, Reflect)]
pub struct WindowRestoreMismatch {
    /// The window entity this event targets.
    pub entity:                     Entity,
    /// Identifier for this window (primary or managed name).
    pub window_id:                  WindowKey,
    /// Target physical position from `TargetPosition` (None on Wayland).
    pub expected_physical_position: Option<IVec2>,
    /// Actual physical position from `Window.position` (None on Wayland).
    pub actual_physical_position:   Option<IVec2>,
    /// Target logical position from the saved state (None on Wayland or when unsaved).
    pub expected_logical_position:  Option<IVec2>,
    /// Actual logical position, derived from `Window.position / actual_scale`.
    /// None on Wayland.
    pub actual_logical_position:    Option<IVec2>,
    /// Target physical size from `TargetPosition`.
    pub expected_physical_size:     UVec2,
    /// Actual physical size from `Window.resolution`.
    pub actual_physical_size:       UVec2,
    /// Expected logical size from `TargetPosition`.
    pub expected_logical_size:      UVec2,
    /// Actual logical size from `Window.resolution.width()`/`height()`.
    pub actual_logical_size:        UVec2,
    /// Target window mode from `TargetPosition`.
    pub expected_mode:              WindowMode,
    /// Actual window mode from `Window.mode`.
    pub actual_mode:                WindowMode,
    /// Target monitor index from `TargetPosition`.
    pub expected_monitor:           usize,
    /// Actual monitor index from `CurrentMonitor` (winit `current_monitor()`).
    pub actual_monitor:             usize,
    /// Target scale factor from `TargetPosition.target_scale`.
    pub expected_scale:             f64,
    /// Actual scale factor from `Window.resolution.scale_factor()`.
    /// Lags behind monitor changes; updates only on winit `ScaleFactorChanged`.
    pub actual_scale:               f64,
}
