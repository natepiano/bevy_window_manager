//! Systems for window restoration and state management.
//!
//! # Monitor Detection
//!
//! [`update_current_monitor`] is the unified system that maintains `CurrentMonitor` on all
//! managed windows. It handles both position-based detection (non-Wayland) and winit polling
//! (Wayland), plus computes the effective window mode.
//!
//! On Wayland, `window.position` always returns `(0,0)` for security/privacy reasons, so
//! position-based detection would always return Monitor 0. Instead, winit's `current_monitor()`
//! is polled each frame.
//!
//! We only trust `current_monitor()` when the window has focus because testing showed
//! incorrect values when unfocused (possibly our own bug, possibly winit behavior).

use bevy::ecs::system::NonSendMarker;
use bevy::prelude::*;
use bevy::window::MonitorSelection;
use bevy::window::PrimaryWindow;
use bevy::window::WindowMode;
use bevy::window::WindowScaleFactorChanged;
use bevy::winit::WINIT_WINDOWS;

use super::state;
use super::types::RestoreWindowConfig;
use super::types::WindowState;
#[cfg(all(target_os = "macos", feature = "workaround-winit-4441"))]
use crate::macos_drag_back_fix::DragBackSizeProtection;
use crate::monitors::CurrentMonitor;
use crate::monitors::MonitorInfo;
use crate::monitors::Monitors;
#[cfg(all(target_os = "windows", feature = "workaround-winit-3124"))]
use crate::types::FullscreenRestoreState;
use crate::types::MonitorScaleStrategy;
use crate::types::SCALE_FACTOR_EPSILON;
use crate::types::SavedWindowMode;
use crate::types::TargetPosition;
use crate::types::WindowDecoration;
use crate::types::WindowIdentifier;
use crate::types::WindowRestoreState;
use crate::types::WindowRestored;
use crate::types::WinitInfo;
use crate::types::X11FrameCompensated;

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

    WINIT_WINDOWS.with(|ww| {
        let ww = ww.borrow();
        if let Some(winit_window) = ww.get_window(*window_entity) {
            let outer = winit_window.outer_size();
            let inner = winit_window.inner_size();
            let decoration = WindowDecoration {
                width:  outer.width.saturating_sub(inner.width),
                height: outer.height.saturating_sub(inner.height),
            };

            // Get actual position from winit to determine starting monitor
            let pos = winit_window
                .outer_position()
                .map(|p| IVec2::new(p.x, p.y))
                .unwrap_or(IVec2::ZERO);

            debug!(
                "[init_winit_info] outer_position={:?} is_wayland={}",
                pos,
                is_wayland()
            );

            // Use winit's current_monitor() as the primary source for starting monitor.
            // Falls back to position-based detection if current_monitor() returns None.
            let starting_monitor = winit_window
                .current_monitor()
                .and_then(|cm| {
                    let cm_pos = cm.position();
                    let info = monitors.at(cm_pos.x, cm_pos.y);
                    debug!(
                        "[init_winit_info] current_monitor() position=({}, {}) -> index={:?}",
                        cm_pos.x, cm_pos.y, info.map(|m| m.index)
                    );
                    info.copied()
                })
                .unwrap_or_else(|| {
                    debug!(
                        "[init_winit_info] current_monitor() unavailable, falling back to closest_to({}, {})",
                        pos.x, pos.y
                    );
                    *monitors.closest_to(pos.x, pos.y)
                });
            let starting_monitor_index = starting_monitor.index;

            debug!(
                "[init_winit_info] decoration={}x{} pos=({}, {}) starting_monitor={}",
                decoration.width, decoration.height, pos.x, pos.y, starting_monitor_index
            );

            // Insert initial CurrentMonitor component on window entity
            commands
                .entity(*window_entity)
                .insert(CurrentMonitor {
                    monitor:        starting_monitor,
                    effective_mode: WindowMode::Windowed,
                });

            commands.insert_resource(WinitInfo {
                starting_monitor_index,
                window_decoration: decoration,
            });
        }
    });
}

/// Calculate restored window position, with optional clamping for macOS.
///
/// On macOS, clamps to monitor bounds because macOS may resize/reposition windows
/// that extend beyond the screen. macOS does not allow windows to span monitors.
///
/// On Windows and Linux X11, windows can legitimately span multiple monitors,
/// so we preserve the exact saved position without clamping.
pub fn clamp_position_to_monitor(
    saved_x: i32,
    saved_y: i32,
    target_info: &crate::monitors::MonitorInfo,
    outer_width: u32,
    outer_height: u32,
) -> IVec2 {
    if cfg!(target_os = "macos") {
        let mon_right = target_info.position.x + target_info.size.x as i32;
        let mon_bottom = target_info.position.y + target_info.size.y as i32;

        let mut x = saved_x;
        let mut y = saved_y;

        if x + outer_width as i32 > mon_right {
            x = mon_right - outer_width as i32;
        }
        if y + outer_height as i32 > mon_bottom {
            y = mon_bottom - outer_height as i32;
        }
        x = x.max(target_info.position.x);
        y = y.max(target_info.position.y);

        if x != saved_x || y != saved_y {
            debug!(
                "[clamp_position_to_monitor] Clamped: ({saved_x}, {saved_y}) -> ({x}, {y}) for outer size {outer_width}x{outer_height}"
            );
        }

        IVec2::new(x, y)
    } else {
        IVec2::new(saved_x, saved_y)
    }
}

/// Load saved window state and insert `TargetPosition` component on the primary window entity.
///
/// Runs after `init_winit_info` so we have access to starting monitor info.
pub fn load_target_position(
    mut commands: Commands,
    window_entity: Single<Entity, With<PrimaryWindow>>,
    monitors: Res<Monitors>,
    winit_info: Res<WinitInfo>,
    mut config: ResMut<RestoreWindowConfig>,
) {
    // Load all states from the file into `loaded_states` as a startup snapshot.
    // This must happen before any managed window observers fire so they can check
    // `loaded_states` instead of re-reading the file (which may have been modified
    // by `on_managed_window_added` saving initial state for new windows).
    if let Some(all_states) = state::load_all_states(&config.path) {
        config.loaded_states = all_states;
    }

    let Some(state) = config.loaded_states.get(state::PRIMARY_WINDOW_KEY).cloned() else {
        debug!("[load_target_position] No saved bevy_window_manager state, showing window");
        // No saved state - show window at default position (user may have started hidden)
        commands.queue(|world: &mut World| {
            let mut query = world.query_filtered::<&mut Window, With<PrimaryWindow>>();
            if let Some(mut window) = query.iter_mut(world).next() {
                window.visible = true;
            }
        });
        return;
    };

    debug!(
        "[load_target_position] Loaded state: position={:?} size={}x{} monitor_index={} mode={:?}",
        state.position, state.width, state.height, state.monitor_index, state.mode
    );

    // Position may be None on Wayland where clients can't access window position.
    // We can still restore fullscreen mode, size, and monitor.
    let saved_pos = state.position;

    // Get starting monitor from WinitInfo
    let starting_monitor_index = winit_info.starting_monitor_index;
    let starting_info = monitors.by_index(starting_monitor_index);
    let starting_scale = starting_info.map_or(1.0, |m| m.scale);

    // Fall back to the first monitor if the saved monitor no longer exists
    // (e.g., external display unplugged). Drop the saved position since it
    // referred to coordinates on the missing monitor.
    let (target_info, fallback_pos) = if let Some(info) = monitors.by_index(state.monitor_index) {
        (info, saved_pos)
    } else {
        warn!(
            "[load_target_position] Target monitor {} not found, falling back to monitor 0",
            state.monitor_index
        );
        (monitors.first(), None)
    };

    let target_scale = target_info.scale;

    // File stores inner dimensions (content area)
    let width = state.width;
    let height = state.height;

    // Calculate outer dimensions for clamping (inner + decoration)
    let decoration = winit_info.decoration();
    let outer_width = width + decoration.x;
    let outer_height = height + decoration.y;

    // Determine monitor scale strategy based on scale relationship and platform.
    //
    // On Windows, winit handles position coordinates correctly, but Bevy's
    // set_physical_resolution still applies scale conversion. We use CompensateSizeOnly
    // when scales differ, or ApplyUnchanged when they match.
    //
    // On macOS, winit's coordinate handling is broken for multi-monitor setups with
    // different scale factors. Bevy processes size before position, so winit's
    // request_inner_size uses the launch monitor's scale factor instead of the target's.
    // We must compensate both position and size based on the scale factor relationship.
    //
    // The macOS compensation can be disabled via feature flag to test if upstream
    // fixes (e.g., Bevy processing position before size) resolve the issue.
    let strategy = determine_scale_strategy(starting_scale, target_scale);

    let position = fallback_pos
        .map(|(x, y)| clamp_position_to_monitor(x, y, target_info, outer_width, outer_height));

    debug!(
        "[load_target_position] Starting monitor={} scale={}, Target monitor={} scale={}, strategy={:?}, position={:?}",
        starting_monitor_index, starting_scale, target_info.index, target_scale, strategy, position
    );

    // Windows W3 workaround (winit #3124): For exclusive fullscreen restore, we must
    // show the window to ensure surfaces are created before the workaround applies
    // fullscreen mode. Otherwise, we want visible = false to prevent the flickering
    // jump from the default position to the restored position.
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

    // Store inner dimensions - decoration is only needed for clamping above
    let entity = *window_entity;

    commands.entity(entity).insert(TargetPosition {
        position,
        width,
        height,
        target_scale,
        starting_scale,
        monitor_scale_strategy: strategy,
        mode: state.mode,
        target_monitor_index: target_info.index,
        #[cfg(all(target_os = "windows", feature = "workaround-winit-3124"))]
        fullscreen_restore_state: FullscreenRestoreState::WaitingForSurface,
    });

    // Insert X11FrameCompensated token for platforms that don't need compensation.
    // On Linux + W6 + X11, the compensation system inserts this token after adjusting position.
    #[cfg(not(all(target_os = "linux", feature = "workaround-winit-4445")))]
    commands.entity(entity).insert(X11FrameCompensated);

    #[cfg(all(target_os = "linux", feature = "workaround-winit-4445"))]
    if is_wayland() {
        commands.entity(entity).insert(X11FrameCompensated);
    }
}

/// Apply the initial window move to the target monitor.
///
/// Sets position and size based on the `TargetPosition` strategy, handling fullscreen,
/// Wayland (no position), and cross-DPI scenarios. Called from `restore_windows` during
/// the `HigherToLower(NeedInitialMove)` phase for both primary and managed windows.
///
/// On macOS with `HigherToLower` strategy, the position is compensated because winit
/// divides coordinates by the launch monitor's scale factor.
///
/// On Windows, this compensation is never needed (strategy is always `ApplyUnchanged`).
///
/// For fullscreen modes, we still move to the target monitor so the fullscreen mode
/// is applied on the correct monitor when `try_apply_restore` runs.
pub fn apply_initial_move(target: &TargetPosition, window: &mut Window) {
    /// Computed parameters for the initial window move to target monitor.
    #[derive(Debug)]
    struct MoveParams {
        position: IVec2,
        width:    u32,
        height:   u32,
    }

    // For fullscreen modes, just move to target monitor position (no 1x1 size)
    // The fullscreen mode will be applied later in try_apply_restore
    if target.mode.is_fullscreen() {
        if let Some(pos) = target.position {
            debug!(
                "[apply_initial_move] Moving to target position {:?} for fullscreen mode {:?}",
                pos, target.mode
            );
            window.position = WindowPosition::At(pos);
        } else {
            debug!(
                "[apply_initial_move] No position available (Wayland), fullscreen mode {:?}",
                target.mode
            );
        }
        return;
    }

    // Position may be None on Wayland - skip position setting if unavailable
    let Some(pos) = target.position else {
        debug!(
            "[apply_initial_move] No position available (Wayland), setting size only: {}x{}",
            target.width, target.height
        );
        debug!(
            "[apply_initial_move] BEFORE set_physical_resolution: physical={}x{} logical={}x{} scale={}",
            window.resolution.physical_width(),
            window.resolution.physical_height(),
            window.resolution.width(),
            window.resolution.height(),
            window.resolution.scale_factor()
        );
        window
            .resolution
            .set_physical_resolution(target.width, target.height);
        debug!(
            "[apply_initial_move] AFTER set_physical_resolution: physical={}x{} logical={}x{} scale={}",
            window.resolution.physical_width(),
            window.resolution.physical_height(),
            window.resolution.width(),
            window.resolution.height(),
            window.resolution.scale_factor()
        );
        return;
    };

    // Compute move parameters based on scale strategy
    let params = match target.monitor_scale_strategy {
        MonitorScaleStrategy::HigherToLower(_) => {
            // Compensate position because winit divides by launch scale
            let ratio = target.starting_scale / target.target_scale;
            let comp_x = (f64::from(pos.x) * ratio) as i32;
            let comp_y = (f64::from(pos.y) * ratio) as i32;
            debug!(
                "[apply_initial_move] HigherToLower: compensating position {:?} -> ({}, {}) (ratio={})",
                pos, comp_x, comp_y, ratio
            );
            MoveParams {
                position: IVec2::new(comp_x, comp_y),
                // Use actual target size to avoid macOS caching tiny size
                width:    target.width,
                height:   target.height,
            }
        },
        _ => MoveParams {
            position: pos,
            width:    target.width,
            height:   target.height,
        },
    };

    debug!(
        "[apply_initial_move] position={:?} size={}x{} visible={}",
        params.position, params.width, params.height, window.visible
    );

    window.position = WindowPosition::At(params.position);
    window
        .resolution
        .set_physical_resolution(params.width, params.height);
}

/// Cached window state for change detection comparison.
#[derive(Default)]
pub struct CachedWindowState {
    position:      Option<IVec2>,
    width:         u32,
    height:        u32,
    mode:          Option<SavedWindowMode>,
    monitor_index: Option<usize>,
}

/// Save window state when position, size, or mode changes. Runs only when not restoring.
///
/// Handles both the primary window and any `ManagedWindow` entities. Uses
/// `ManagedWindowPersistence` to decide whether closed windows keep their saved state.
#[allow(
    clippy::type_complexity,
    clippy::too_many_lines,
    clippy::too_many_arguments,
    clippy::option_if_let_else
)]
pub fn save_window_state(
    config: Res<RestoreWindowConfig>,
    monitors: Res<Monitors>,
    persistence: Res<crate::ManagedWindowPersistence>,
    windows: Query<
        (
            Entity,
            &Window,
            Option<&CurrentMonitor>,
            Option<&crate::ManagedWindow>,
        ),
        (
            Or<(With<PrimaryWindow>, With<crate::ManagedWindow>)>,
            Changed<Window>,
        ),
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
            (*state::PRIMARY_WINDOW_KEY).to_string()
        } else if let Some(m) = managed {
            m.window_name.clone()
        } else {
            continue;
        };

        // Get window position for saving state.
        #[cfg(all(target_os = "linux", feature = "workaround-winit-4443"))]
        let pos = WINIT_WINDOWS.with(|ww| {
            let ww = ww.borrow();
            let winit_win = ww.get_window(window_entity)?;
            let outer_pos = winit_win.outer_position().ok()?;
            Some(IVec2::new(outer_pos.x, outer_pos.y))
        });

        #[cfg(not(all(target_os = "linux", feature = "workaround-winit-4443")))]
        let pos = match window.position {
            bevy::window::WindowPosition::At(p) => Some(p),
            _ => None,
        };

        let width = window.resolution.physical_width();
        let height = window.resolution.physical_height();
        let logical_w = window.resolution.width();
        let logical_h = window.resolution.height();
        let res_scale = window.resolution.scale_factor();
        debug!(
            "[save_window_state] [{key}] SAVE DETAIL: pos={pos:?} physical={}x{} logical={:.0}x{:.0} res_scale={res_scale}",
            width, height, logical_w, logical_h
        );
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

        // Log monitor transitions with detailed info
        let monitor_changed = entry.monitor_index != Some(monitor_index);
        if monitor_changed {
            let prev_scale = entry
                .monitor_index
                .and_then(|i| monitors.by_index(i))
                .map(|m| m.scale);
            debug!(
                "[save_window_state] [{key}] MONITOR CHANGE: {:?} (scale={:?}) -> {} (scale={})",
                entry.monitor_index, prev_scale, monitor_index, monitor_scale
            );
        }

        // Only save if position, size, or mode actually changed
        let position_changed = entry.position != pos;
        let size_changed = entry.width != width || entry.height != height;
        let mode_changed = entry.mode.as_ref() != Some(&mode);

        if !position_changed && !size_changed && !mode_changed {
            entry.monitor_index = Some(monitor_index);
            continue;
        }

        // Update cache
        entry.position = pos;
        entry.width = width;
        entry.height = height;
        entry.mode = Some(mode.clone());
        entry.monitor_index = Some(monitor_index);

        any_changed = true;

        debug!(
            "[save_window_state] [{key}] pos={:?} size={}x{} monitor={} scale={} mode={:?}",
            pos, width, height, monitor_index, monitor_scale, mode
        );
    }

    if !any_changed {
        return;
    }

    // Build the complete state map from all cached entries
    let app_name = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_stem().and_then(|s| s.to_str()).map(String::from))
        .unwrap_or_default();

    // Determine what to save based on persistence mode
    let mut states = match *persistence {
        crate::ManagedWindowPersistence::RememberAll => {
            // Load existing file first to preserve closed windows
            state::load_all_states(&config.path).unwrap_or_default()
        },
        crate::ManagedWindowPersistence::ActiveOnly => {
            // Start fresh - only save currently open windows
            std::collections::HashMap::new()
        },
    };

    // Update with current window states from cache
    for (entity, entry) in &*cached {
        let key = if primary_q.get(*entity).is_ok() {
            (*state::PRIMARY_WINDOW_KEY).to_string()
        } else {
            // Look up managed window name - entity may have been despawned already
            // if so, skip it (the cached entry is stale)
            continue;
        };

        if let Some(mode) = &entry.mode {
            states.insert(
                key,
                WindowState {
                    position:      entry.position.map(|p| (p.x, p.y)),
                    width:         entry.width,
                    height:        entry.height,
                    monitor_index: entry.monitor_index.unwrap_or(0),
                    mode:          mode.clone(),
                    app_name:      app_name.clone(),
                },
            );
        }
    }

    // Also save managed windows from the query (since they might not have changed
    // in this frame but we need their latest state for RememberAll)
    // We do this by re-querying all managed windows
    // Actually, the cached HashMap already has all the latest data, but for managed windows
    // we need their names. Let's handle this differently.

    // For managed windows, iterate cached and look up names from the query
    for (entity, entry) in &*cached {
        if primary_q.get(*entity).is_ok() {
            continue; // Already handled above
        }
        // Find managed window component
        if let Ok((_, _, _, Some(managed))) = windows.get(*entity)
            && let Some(mode) = &entry.mode
        {
            states.insert(
                managed.window_name.clone(),
                WindowState {
                    position:      entry.position.map(|p| (p.x, p.y)),
                    width:         entry.width,
                    height:        entry.height,
                    monitor_index: entry.monitor_index.unwrap_or(0),
                    mode:          mode.clone(),
                    app_name:      app_name.clone(),
                },
            );
        }
    }

    state::save_all_states(&config.path, &states);
}

/// Apply pending window restore. Runs only when entities with `TargetPosition` exist.
/// Processes all windows with both `TargetPosition` and `X11FrameCompensated` components.
#[allow(clippy::too_many_arguments)]
pub fn restore_windows(
    mut commands: Commands,
    mut scale_changed_messages: MessageReader<WindowScaleFactorChanged>,
    mut windows: Query<(Entity, &mut TargetPosition, &mut Window), With<X11FrameCompensated>>,
    primary_q: Query<(), With<PrimaryWindow>>,
    managed_q: Query<&crate::ManagedWindow>,
    config: Res<RestoreWindowConfig>,
) {
    let scale_changed = scale_changed_messages.read().last().is_some();

    for (entity, mut target, mut window) in &mut windows {
        // Unified initial move for HigherToLower: both primary and managed windows
        // enter here via `NeedInitialMove`. We call `apply_initial_move` to set the
        // compensated position (triggering a monitor scale change), then transition
        // to `WaitingForScaleChange` to wait for the scale event before applying size.
        if matches!(
            target.monitor_scale_strategy,
            MonitorScaleStrategy::HigherToLower(WindowRestoreState::NeedInitialMove)
        ) {
            apply_initial_move(&target, &mut window);
            target.monitor_scale_strategy =
                MonitorScaleStrategy::HigherToLower(WindowRestoreState::WaitingForScaleChange);
            continue;
        }

        // Handle HigherToLower state transition on scale change
        if target.monitor_scale_strategy
            == MonitorScaleStrategy::HigherToLower(WindowRestoreState::WaitingForScaleChange)
            && scale_changed
        {
            debug!(
                "[Restore] ScaleChanged received, transitioning to WindowRestoreState::ApplySize"
            );
            target.monitor_scale_strategy =
                MonitorScaleStrategy::HigherToLower(WindowRestoreState::ApplySize);
        }

        // Windows: transition fullscreen state after first frame (DX12/DXGI workaround)
        #[cfg(all(target_os = "windows", feature = "workaround-winit-3124"))]
        if target.mode.is_fullscreen()
            && target.fullscreen_restore_state == FullscreenRestoreState::WaitingForSurface
        {
            debug!("[Restore] First frame passed, transitioning to ApplyFullscreen");
            target.fullscreen_restore_state = FullscreenRestoreState::ApplyFullscreen;
            continue; // Wait one more frame for the state change to take effect
        }

        // Check if this is a HigherToLower restore about to complete (for W4 protection)
        #[cfg(all(target_os = "macos", feature = "workaround-winit-4441"))]
        let was_higher_to_lower = matches!(
            target.monitor_scale_strategy,
            MonitorScaleStrategy::HigherToLower(WindowRestoreState::ApplySize)
        );

        if matches!(
            try_apply_restore(&target, &mut window),
            RestoreStatus::Complete
        ) {
            // Insert W4 drag-back protection for HigherToLower restores
            #[cfg(all(target_os = "macos", feature = "workaround-winit-4441"))]
            if was_higher_to_lower {
                debug!(
                    "[Restore] Inserting DragBackSizeProtection: size={}x{} launch_scale={} restored_scale={}",
                    target.width, target.height, target.starting_scale, target.target_scale
                );
                // Phase 1 cached size is the physical size we set at launch scale before moving.
                // This is what AppKit will cache and restore when dragging back (W4 behavior).
                let phase1_cached_size = UVec2::new(target.width, target.height);
                commands.entity(entity).insert(DragBackSizeProtection {
                    expected_physical_size: UVec2::new(target.width, target.height),
                    launch_scale: target.starting_scale,
                    restored_scale: target.target_scale,
                    phase1_cached_size,
                    state: crate::macos_drag_back_fix::CorrectionState::WaitingForDragBack,
                });
            }

            // Determine window identity
            let window_id = if primary_q.get(entity).is_ok() {
                WindowIdentifier::Primary
            } else if let Ok(managed) = managed_q.get(entity) {
                WindowIdentifier::Managed(managed.window_name.clone())
            } else {
                WindowIdentifier::Primary // fallback, shouldn't happen
            };
            let key = window_id.to_string();

            // Compare intended vs actual and warn on mismatch
            if let Some(loaded) = config.loaded_states.get(&key) {
                let actual_pos = match window.position {
                    bevy::window::WindowPosition::At(p) => Some(IVec2::new(p.x, p.y)),
                    _ => None,
                };
                let actual_phys_w = window.resolution.physical_width();
                let actual_phys_h = window.resolution.physical_height();
                let target_pos = target.position;
                let target_w = target.width;
                let target_h = target.height;

                let pos_mismatch = target_pos != actual_pos;
                let size_mismatch = target_w != actual_phys_w || target_h != actual_phys_h;

                if pos_mismatch || size_mismatch {
                    warn!(
                        "[restore_windows] [{key}] RESTORE MISMATCH: \
                         file=({:?}, {}x{}) actual=({:?}, {}x{})",
                        loaded.position,
                        loaded.width,
                        loaded.height,
                        actual_pos.map(|p| (p.x, p.y)),
                        actual_phys_w,
                        actual_phys_h,
                    );
                }
            }

            // Fire `WindowRestored` event
            let target_mode = target.mode.to_window_mode(target.target_monitor_index);
            let target_size = target.size();
            let target_position = target.position;
            let target_monitor = target.target_monitor_index;
            commands.entity(entity).trigger(|entity| WindowRestored {
                entity,
                window_id,
                position: target_position,
                size: target_size,
                mode: target_mode,
                monitor_index: target_monitor,
            });

            commands.entity(entity).remove::<TargetPosition>();
            commands.entity(entity).remove::<X11FrameCompensated>();
        }
    }
}

/// Result of attempting to apply a window restore.
enum RestoreStatus {
    /// Restore completed successfully.
    Complete,
    /// Waiting for conditions to be met (scale change, window position, etc.).
    Waiting,
}

/// Run condition: returns true if running on Wayland.
pub fn is_wayland() -> bool {
    cfg!(target_os = "linux")
        && std::env::var("WAYLAND_DISPLAY")
            .map(|v| !v.is_empty())
            .unwrap_or(false)
}

/// Unified monitor detection system. Maintains `CurrentMonitor` on all managed windows.
///
/// - **Non-Wayland**: detects monitor from window position using center-point logic
/// - **Wayland**: polls winit's `current_monitor()` (only trusted when focused)
/// - **All platforms**: computes `effective_mode` (handles macOS green button fullscreen)
pub fn update_current_monitor(
    mut commands: Commands,
    windows: Query<
        (Entity, &Window, Option<&CurrentMonitor>),
        Or<(With<PrimaryWindow>, With<crate::ManagedWindow>)>,
    >,
    monitors: Res<Monitors>,
    _non_send: NonSendMarker,
) {
    if monitors.is_empty() {
        return;
    }

    for (entity, window, existing) in &windows {
        // Detect which monitor this window is on
        let monitor_info = if is_wayland() {
            // Wayland: poll winit's current_monitor() when focused, otherwise preserve existing
            if window.focused {
                wayland_detect_monitor(entity, &monitors)
                    .unwrap_or_else(|| existing.map_or(*monitors.first(), |cm| cm.monitor))
            } else {
                existing.map_or(*monitors.first(), |cm| cm.monitor)
            }
        } else if let bevy::window::WindowPosition::At(pos) = window.position {
            // Non-Wayland with known position: use center-point detection
            *monitors.monitor_for_window(pos, window.physical_width(), window.physical_height())
        } else {
            // Position unknown (e.g., Automatic): preserve existing or fallback
            existing.map_or(*monitors.first(), |cm| cm.monitor)
        };

        // Compute effective mode
        let effective_mode = compute_effective_mode(window, &monitor_info, &monitors);

        let new_current = CurrentMonitor {
            monitor: monitor_info,
            effective_mode,
        };

        // Only insert if changed to avoid unnecessary change detection triggers
        let changed = existing.map_or(true, |cm| {
            cm.monitor.index != new_current.monitor.index
                || cm.effective_mode != new_current.effective_mode
        });

        if changed {
            commands.entity(entity).insert(new_current);
        }
    }
}

/// Detect monitor on Wayland by polling winit's `current_monitor()`.
fn wayland_detect_monitor(entity: Entity, monitors: &Monitors) -> Option<MonitorInfo> {
    WINIT_WINDOWS.with(|ww| {
        let ww = ww.borrow();
        ww.get_window(entity).and_then(|winit_window| {
            winit_window.current_monitor().and_then(|current_monitor| {
                let pos = current_monitor.position();
                monitors.at(pos.x, pos.y).copied()
            })
        })
    })
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
    let bevy::window::WindowPosition::At(pos) = window.position else {
        return window.mode;
    };

    // Check if window spans full width and reaches bottom of monitor
    let full_width = window.physical_width() == monitor_info.size.x;
    let left_aligned = pos.x == monitor_info.position.x;
    let reaches_bottom = pos.y + window.physical_height() as i32
        == monitor_info.position.y + monitor_info.size.y as i32;

    if full_width && left_aligned && reaches_bottom {
        WindowMode::BorderlessFullscreen(MonitorSelection::Index(monitor_info.index))
    } else {
        WindowMode::Windowed
    }
}

/// Apply fullscreen mode, handling Wayland limitations.
fn apply_fullscreen_restore(target: &TargetPosition, window: &mut Window, monitor_index: usize) {
    // On Wayland, exclusive fullscreen is ignored by winit, so we restore it as
    // borderless fullscreen instead.
    let window_mode = if is_wayland() && matches!(target.mode, SavedWindowMode::Fullscreen { .. }) {
        warn!(
            "Exclusive fullscreen is not supported on Wayland, restoring as BorderlessFullscreen"
        );
        WindowMode::BorderlessFullscreen(MonitorSelection::Index(monitor_index))
    } else {
        target.mode.to_window_mode(monitor_index)
    };

    debug!(
        "[Restore] Applying fullscreen mode {:?} on monitor {} -> WindowMode::{:?}",
        target.mode, monitor_index, window_mode
    );
    debug!(
        "[Restore] Current window state: position={:?} mode={:?}",
        window.position, window.mode
    );

    window.mode = window_mode;
}

/// Apply position and/or size to window with logging.
fn apply_window_geometry(
    window: &mut Window,
    position: Option<IVec2>,
    size: UVec2,
    strategy: &str,
    ratio: Option<f64>,
) {
    if let Some(pos) = position {
        if let Some(r) = ratio {
            debug!(
                "[try_apply_restore] position={:?} size={}x{} ({strategy}, ratio={r})",
                pos, size.x, size.y
            );
        } else {
            debug!(
                "[try_apply_restore] position={:?} size={}x{} ({strategy})",
                pos, size.x, size.y
            );
        }
        window.position = WindowPosition::At(pos);
        window.resolution.set_physical_resolution(size.x, size.y);
    } else {
        if let Some(r) = ratio {
            debug!(
                "[try_apply_restore] size={}x{} only ({strategy}, ratio={r}, no position)",
                size.x, size.y
            );
        } else {
            debug!(
                "[try_apply_restore] size={}x{} only ({strategy}, no position)",
                size.x, size.y
            );
        }
        window.resolution.set_physical_resolution(size.x, size.y);
    }
}

/// Try to apply a pending window restore.
fn try_apply_restore(target: &TargetPosition, window: &mut Window) -> RestoreStatus {
    // Handle fullscreen modes - use saved monitor index from TargetPosition
    if target.mode.is_fullscreen() {
        apply_fullscreen_restore(target, window, target.target_monitor_index);
        window.visible = true;
        return RestoreStatus::Complete;
    }

    debug!(
        "[Restore] target_pos={:?} target_scale={} strategy={:?}",
        target.position, target.target_scale, target.monitor_scale_strategy
    );

    match target.monitor_scale_strategy {
        MonitorScaleStrategy::ApplyUnchanged => {
            apply_window_geometry(
                window,
                target.position(),
                target.size(),
                "ApplyUnchanged",
                None,
            );
        },
        #[cfg(all(target_os = "windows", feature = "workaround-winit-4440"))]
        MonitorScaleStrategy::CompensateSizeOnly => {
            apply_window_geometry(
                window,
                target.position(),
                target.compensated_size(),
                "CompensateSizeOnly",
                Some(target.ratio()),
            );
        },
        #[cfg(all(not(target_os = "windows"), feature = "workaround-winit-4440"))]
        MonitorScaleStrategy::LowerToHigher => {
            apply_window_geometry(
                window,
                target.compensated_position(),
                target.compensated_size(),
                "LowerToHigher",
                Some(target.ratio()),
            );
        },
        MonitorScaleStrategy::HigherToLower(WindowRestoreState::ApplySize) => {
            let size = target.size();
            debug!(
                "[try_apply_restore] size={}x{} ONLY (HigherToLower::ApplySize, position already set)",
                size.x, size.y
            );
            window.resolution.set_physical_resolution(size.x, size.y);
        },
        MonitorScaleStrategy::HigherToLower(WindowRestoreState::NeedInitialMove)
        | MonitorScaleStrategy::HigherToLower(WindowRestoreState::WaitingForScaleChange) => {
            debug!("[Restore] HigherToLower: waiting for initial move or ScaleChanged message");
            return RestoreStatus::Waiting;
        },
    }

    // Show window now that restore is complete
    window.visible = true;
    RestoreStatus::Complete
}

/// Determine the monitor scale strategy based on platform and scale factors.
/// Windows: compensate size only when scales differ.
#[cfg(all(target_os = "windows", feature = "workaround-winit-4440"))]
fn determine_scale_strategy(starting_scale: f64, target_scale: f64) -> MonitorScaleStrategy {
    if (starting_scale - target_scale).abs() < SCALE_FACTOR_EPSILON {
        MonitorScaleStrategy::ApplyUnchanged
    } else {
        MonitorScaleStrategy::CompensateSizeOnly
    }
}

/// Determine the monitor scale strategy based on platform and scale factors.
/// Windows without workaround: always use `ApplyUnchanged`.
#[cfg(all(target_os = "windows", not(feature = "workaround-winit-4440")))]
fn determine_scale_strategy(_starting_scale: f64, _target_scale: f64) -> MonitorScaleStrategy {
    MonitorScaleStrategy::ApplyUnchanged
}

/// Determine the monitor scale strategy based on platform and scale factors.
/// macOS with workaround: compensate position and size based on scale relationship.
/// Wayland: always use `ApplyUnchanged` (can't detect starting monitor, can't set position).
#[cfg(all(not(target_os = "windows"), feature = "workaround-winit-4440"))]
pub fn determine_scale_strategy(starting_scale: f64, target_scale: f64) -> MonitorScaleStrategy {
    // On Wayland, we can't reliably detect the starting monitor (outer_position returns 0,0
    // and current_monitor/primary_monitor return None at init). Since we also can't set
    // position on Wayland, skip scale compensation entirely.
    if is_wayland() {
        return MonitorScaleStrategy::ApplyUnchanged;
    }

    if (starting_scale - target_scale).abs() < SCALE_FACTOR_EPSILON {
        MonitorScaleStrategy::ApplyUnchanged
    } else if starting_scale < target_scale {
        // Low DPI -> high DPI
        MonitorScaleStrategy::LowerToHigher
    } else {
        // High DPI -> low DPI
        MonitorScaleStrategy::HigherToLower(WindowRestoreState::NeedInitialMove)
    }
}

/// Determine the monitor scale strategy based on platform and scale factors.
/// macOS without workaround: always use `ApplyUnchanged`.
#[cfg(all(not(target_os = "windows"), not(feature = "workaround-winit-4440")))]
pub fn determine_scale_strategy(_starting_scale: f64, _target_scale: f64) -> MonitorScaleStrategy {
    // Without workaround, assume upstream fixes handle scale factor correctly
    MonitorScaleStrategy::ApplyUnchanged
}
