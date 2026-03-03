//! Interactive example for testing window restoration, fullscreen modes, and multi-window
//! management.
//!
//! Run with: `cargo run --example restore_window`
//!
//! Controls (all windows):
//! - Press `1` or `Enter` for exclusive fullscreen (uses selected video mode) WARNING: Exclusive fullscreen on
//!   macOS may panic on exit due to winit bugs. See: <https://github.com/rust-windowing/winit/issues/3668>
//! - Press `2` for borderless fullscreen (recommended on macOS)
//! - Press `W` or `Escape` for windowed mode
//! - Press `Up`/`Down` to cycle through available video modes
//!
//! - Press `Space` to spawn a new managed window
//! - Press `P` to toggle persistence mode (`RememberAll` / `ActiveOnly`)
//! - Press `Ctrl+Shift+Backspace` to clear saved state and quit
//! - Press `Q` to quit
//!
//! Move and resize windows to test state persistence across restarts.

// Monitor dimensions always fit in i32
#![allow(clippy::cast_possible_wrap)]

use std::collections::HashMap;

use bevy::camera::RenderTarget;
use bevy::ecs::system::NonSendMarker;
use bevy::prelude::*;
use bevy::ui::UiTargetCamera;
use bevy::window::Monitor;
use bevy::window::MonitorSelection;
use bevy::window::PrimaryWindow;
use bevy::window::VideoMode;
use bevy::window::VideoModeSelection;
use bevy::window::WindowMode;
use bevy::window::WindowPosition;
use bevy::window::WindowRef;
use bevy::window::WindowScaleFactorChanged;
use bevy::winit::WINIT_WINDOWS;
use bevy_brp_extras::BrpExtrasPlugin;
use bevy_window_manager::CurrentMonitor;
use bevy_window_manager::ManagedWindow;
use bevy_window_manager::ManagedWindowPersistence;
use bevy_window_manager::Monitors;
use bevy_window_manager::WindowManagerPlugin;
use bevy_window_manager::WindowRestored;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Window Restore - Primary Window".into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(WindowManagerPlugin)
        .add_plugins(BrpExtrasPlugin::default())
        .add_observer(on_window_restored)
        .add_observer(on_secondary_window_added)
        .add_observer(on_secondary_window_removed)
        .init_resource::<SelectedVideoModes>()
        .init_resource::<WindowCounter>()
        .init_resource::<RestoredStates>()
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                update_primary_display,
                update_secondary_displays,
                handle_global_input,
                handle_window_mode_input,
                debug_winit_monitor,
                debug_window_changed,
                debug_scale_factor_changed,
            ),
        )
        .run();
}

// --- Resources ---

/// Tracks the next window number for auto-incrementing names.
#[derive(Resource, Default)]
struct WindowCounter {
    next: usize,
}

/// Tracks the selected video mode index per monitor for exclusive fullscreen.
#[derive(Resource, Default)]
struct SelectedVideoModes {
    /// Selected index per monitor (keyed by monitor index).
    indices:   HashMap<usize, usize>,
    /// Track last synced mode to avoid overriding user selection.
    last_sync: Option<(UVec2, u32)>,
}

impl SelectedVideoModes {
    fn get(&self, monitor_index: usize) -> usize {
        self.indices.get(&monitor_index).copied().unwrap_or(0)
    }

    fn set(&mut self, monitor_index: usize, index: usize) {
        self.indices.insert(monitor_index, index);
    }
}

// --- Components ---

/// Marker for the primary window's text display.
#[derive(Component)]
struct PrimaryDisplay;

/// Marker for a secondary window's text display, storing the window entity.
#[derive(Component)]
struct SecondaryDisplay(Entity);

// --- WindowRestored Test Support ---

/// Resource inserted when `WindowRestored` event is received.
/// Queryable via BRP to verify the event fired with expected values.
#[derive(Resource, Debug, Clone, Reflect)]
#[reflect(Resource)]
struct WindowRestoredReceived {
    position:      Option<IVec2>,
    size:          UVec2,
    mode:          WindowMode,
    monitor_index: usize,
}

/// Cached restored state per window, populated from `WindowRestored` events.
/// Used for the "File" column comparison display.
#[derive(Resource, Default)]
struct RestoredStates {
    states: HashMap<Entity, CachedRestoredState>,
}

/// State cached from a `WindowRestored` event for display comparison.
struct CachedRestoredState {
    position:      Option<IVec2>,
    width:         u32,
    height:        u32,
    monitor_index: usize,
    mode:          WindowMode,
}

// --- Constants ---

const MARGIN: Val = Val::Px(20.0);
const FONT_SIZE: f32 = 14.0;
const SECONDARY_WINDOW_WIDTH: u32 = 600;
const SECONDARY_WINDOW_HEIGHT: u32 = 400;
const MISMATCH_COLOR: Color = Color::linear_rgb(1.0, 0.3, 0.3);
const DEFAULT_COLOR: Color = Color::WHITE;
const LABEL_WIDTH: usize = 18;

// --- Setup ---

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);

    commands.spawn((
        Text::new(""),
        TextFont {
            font_size: FONT_SIZE,
            ..default()
        },
        Node {
            position_type: PositionType::Absolute,
            top: MARGIN,
            left: MARGIN,
            ..default()
        },
        PrimaryDisplay,
    ));
}

// --- Secondary Window Lifecycle ---

/// Observer: spawn `Camera2d` and text display when a `ManagedWindow` is added.
fn on_secondary_window_added(
    add: On<Add, ManagedWindow>,
    mut commands: Commands,
    primary_q: Query<(), With<PrimaryWindow>>,
) {
    let entity = add.entity;
    if primary_q.get(entity).is_ok() {
        return;
    }

    let camera = commands
        .spawn((Camera2d, RenderTarget::Window(WindowRef::Entity(entity))))
        .id();

    commands.spawn((
        Text::new(""),
        TextFont {
            font_size: FONT_SIZE,
            ..default()
        },
        Node {
            position_type: PositionType::Absolute,
            top: MARGIN,
            left: MARGIN,
            ..default()
        },
        UiTargetCamera(camera),
        SecondaryDisplay(entity),
    ));
}

/// Observer: clean up display entities when a `ManagedWindow` is removed.
fn on_secondary_window_removed(
    remove: On<Remove, ManagedWindow>,
    mut commands: Commands,
    displays: Query<(Entity, &SecondaryDisplay)>,
) {
    let entity = remove.entity;
    for (display_entity, display) in &displays {
        if display.0 == entity {
            commands.entity(display_entity).despawn();
        }
    }
}

// --- Comparison Display ---

/// Build comparison spans (restored vs current) for a window and add them as `TextSpan` children.
fn build_comparison_spans(
    cb: &mut ChildSpawnerCommands,
    restored_state: Option<&CachedRestoredState>,
    window: &Window,
    monitor: &CurrentMonitor,
    font: &TextFont,
) {
    let effective_mode = monitor.effective_mode;
    let scale = window.resolution.scale_factor();

    // Current values
    let current_pos = match window.position {
        WindowPosition::At(pos) => format!("({}, {})", pos.x, pos.y),
        _ => "Automatic".to_string(),
    };
    let current_size_phys = format!("{}x{}", window.physical_width(), window.physical_height());
    let current_size_log = format!(
        "{}x{}",
        window.resolution.width() as u32,
        window.resolution.height() as u32
    );
    let current_scale = format!("{scale}");
    let current_monitor = format!("{}", monitor.index);
    let current_mode = format!("{effective_mode:?}");

    if let Some(state) = restored_state {
        // Restored values from `WindowRestored` event
        let file_pos = state
            .position
            .map_or_else(|| "None".to_string(), |p| format!("({}, {})", p.x, p.y));
        let file_size_phys = format!("{}x{}", state.width, state.height);
        let file_monitor = format!("{}", state.monitor_index);
        let file_mode = format!("{:?}", state.mode);

        // Column width: fit the longest file value + 2 padding, minimum 16
        let col1_width = [
            file_pos.len(),
            file_size_phys.len(),
            file_monitor.len(),
            file_mode.len(),
        ]
        .into_iter()
        .max()
        .unwrap_or(0)
            + 2;
        let col1_width = col1_width.max(16);

        let header = format!(
            "{:LABEL_WIDTH$}{:<col1_width$}{}\n",
            "", "Restored", "Current"
        );
        add_span(cb, font, &header, DEFAULT_COLOR);

        // Rows with mismatch highlighting
        add_comparison_row(cb, font, "Position:", &file_pos, &current_pos, col1_width);
        add_comparison_row(
            cb,
            font,
            "Size (physical):",
            &file_size_phys,
            &current_size_phys,
            col1_width,
        );

        // Logical size and scale — no file equivalent, show current only
        add_span(
            cb,
            font,
            &format!(
                "{:<LABEL_WIDTH$}{:<col1_width$}{current_size_log}\n",
                "Size (logical):", ""
            ),
            DEFAULT_COLOR,
        );
        add_span(
            cb,
            font,
            &format!(
                "{:<LABEL_WIDTH$}{:<col1_width$}{current_scale}\n",
                "Scale:", ""
            ),
            DEFAULT_COLOR,
        );

        add_comparison_row(
            cb,
            font,
            "Monitor:",
            &file_monitor,
            &current_monitor,
            col1_width,
        );
        add_comparison_row(cb, font, "Mode:", &file_mode, &current_mode, col1_width);
    } else {
        add_span(cb, font, "State: No restore data\n\n", MISMATCH_COLOR);
        add_span(
            cb,
            font,
            &format!("{:<LABEL_WIDTH$}{current_pos}\n", "Position:"),
            DEFAULT_COLOR,
        );
        add_span(
            cb,
            font,
            &format!("{:<LABEL_WIDTH$}{current_size_phys}\n", "Size (physical):"),
            DEFAULT_COLOR,
        );
        add_span(
            cb,
            font,
            &format!("{:<LABEL_WIDTH$}{current_size_log}\n", "Size (logical):"),
            DEFAULT_COLOR,
        );
        add_span(
            cb,
            font,
            &format!("{:<LABEL_WIDTH$}{current_scale}\n", "Scale:"),
            DEFAULT_COLOR,
        );
        add_span(
            cb,
            font,
            &format!("{:<LABEL_WIDTH$}{current_monitor}\n", "Monitor:"),
            DEFAULT_COLOR,
        );
        add_span(
            cb,
            font,
            &format!("{:<LABEL_WIDTH$}{current_mode}\n", "Mode:"),
            DEFAULT_COLOR,
        );
    }

    add_span(
        cb,
        font,
        &format!("\nEffective Mode: {effective_mode:?}\n"),
        DEFAULT_COLOR,
    );
}

/// Add a comparison row: label + file value (white) + current value (white or red if mismatch).
fn add_comparison_row(
    cb: &mut ChildSpawnerCommands,
    font: &TextFont,
    label: &str,
    file_val: &str,
    current_val: &str,
    col_width: usize,
) {
    let color = if file_val == current_val {
        DEFAULT_COLOR
    } else {
        MISMATCH_COLOR
    };

    // Label + file value (always white)
    add_span(
        cb,
        font,
        &format!("{label:<LABEL_WIDTH$}{file_val:<col_width$}"),
        DEFAULT_COLOR,
    );
    // Current value (colored)
    add_span(cb, font, &format!("{current_val}\n"), color);
}

/// Add a single `TextSpan` child.
fn add_span(cb: &mut ChildSpawnerCommands, font: &TextFont, text: &str, color: Color) {
    cb.spawn((TextSpan(text.to_string()), font.clone(), TextColor(color)));
}

// --- Primary Window Display ---

fn update_primary_display(
    primary_display: Single<Entity, With<PrimaryDisplay>>,
    window_query: Single<(Entity, &Window, &CurrentMonitor), With<PrimaryWindow>>,
    monitors_res: Res<Monitors>,
    bevy_monitors: Query<(Entity, &Monitor)>,
    mut selected: ResMut<SelectedVideoModes>,
    persistence: Res<ManagedWindowPersistence>,
    managed_q: Query<(&Window, &ManagedWindow, Option<&CurrentMonitor>)>,
    restored_states: Res<RestoredStates>,
    mut commands: Commands,
) {
    let display_entity = *primary_display;
    let (window_entity, window, monitor) = *window_query;

    let restored_state = restored_states.states.get(&window_entity);

    let (video_modes, refresh_rate) = get_video_modes_for_monitor(&bevy_monitors, monitor);
    let refresh_display = format_refresh_rate(window, refresh_rate);
    let active_mode_idx = find_active_video_mode_index(window, &video_modes);
    sync_selected_to_active(window, monitor, active_mode_idx, &mut selected);
    let selected_idx = selected.get(monitor.index);
    let video_modes_display =
        build_video_modes_display(&video_modes, selected_idx, active_mode_idx);

    let font = TextFont {
        font_size: FONT_SIZE,
        ..default()
    };

    commands.entity(display_entity).despawn_children();
    commands.entity(display_entity).with_children(|cb| {
        // Monitor header
        let monitor_row = format_monitor_row(monitor, &refresh_display);
        add_span(cb, &font, &format!("{monitor_row}\n\n"), DEFAULT_COLOR);

        // Comparison table
        build_comparison_spans(cb, restored_state, window, monitor, &font);

        // Video modes
        add_span(
            cb,
            &font,
            &format!("\nVideo Modes (Up/Down to select):\n{video_modes_display}\n"),
            DEFAULT_COLOR,
        );

        // Controls
        add_span(
            cb,
            &font,
            &format!(
                "\nControls:\n\
                 [1/Enter] Exclusive Fullscreen  [2] Borderless Fullscreen\n\
                 [W/Esc] Windowed\n\
                 [Space] Spawn managed window\n\
                 [P] Toggle persistence ({persistence:?})\n\
                 [Ctrl+Shift+Backspace] Clear state and quit\n\
                 [Q] Quit\n"
            ),
            DEFAULT_COLOR,
        );

        // Managed windows list
        let mut managed_lines = Vec::new();
        for (mw, managed, current_monitor) in &managed_q {
            let mon = current_monitor.map_or(*monitors_res.first(), |cm| cm.monitor);
            let pos = match mw.position {
                WindowPosition::At(p) => format!("({}, {})", p.x, p.y),
                _ => "Automatic".to_string(),
            };
            managed_lines.push(format!(
                "  {}: pos={pos} phys={}x{} log={}x{} scale={} monitor={}\n",
                managed.window_name,
                mw.physical_width(),
                mw.physical_height(),
                mw.resolution.width() as u32,
                mw.resolution.height() as u32,
                mw.resolution.scale_factor(),
                mon.index,
            ));
        }
        let managed_header = "\nManaged Windows:\n";
        add_span(cb, &font, managed_header, DEFAULT_COLOR);
        if managed_lines.is_empty() {
            add_span(cb, &font, "  (none)\n", DEFAULT_COLOR);
        } else {
            for line in &managed_lines {
                add_span(cb, &font, line, DEFAULT_COLOR);
            }
        }
    });
}

// --- Secondary Window Displays ---

fn update_secondary_displays(
    mut displays: Query<(Entity, &SecondaryDisplay)>,
    windows: Query<(&Window, Option<&CurrentMonitor>)>,
    managed_q: Query<&ManagedWindow>,
    monitors_res: Res<Monitors>,
    bevy_monitors: Query<(Entity, &Monitor)>,
    mut selected: ResMut<SelectedVideoModes>,
    restored_states: Res<RestoredStates>,
    mut commands: Commands,
) {
    for (display_entity, display) in &mut displays {
        let Ok((window, current_monitor)) = windows.get(display.0) else {
            continue;
        };
        let monitor_info = current_monitor.copied().unwrap_or(CurrentMonitor {
            monitor:        *monitors_res.first(),
            effective_mode: window.mode,
        });

        let name = managed_q
            .get(display.0)
            .map_or("unknown", |m| &m.window_name);
        let restored_state = restored_states.states.get(&display.0);

        let (video_modes, refresh_rate) =
            get_video_modes_for_monitor(&bevy_monitors, &monitor_info);
        let refresh_display = format_refresh_rate(window, refresh_rate);
        let active_mode_idx = find_active_video_mode_index(window, &video_modes);
        sync_selected_to_active(window, &monitor_info, active_mode_idx, &mut selected);
        let selected_idx = selected.get(monitor_info.index);
        let video_modes_display =
            build_video_modes_display(&video_modes, selected_idx, active_mode_idx);

        let font = TextFont {
            font_size: FONT_SIZE,
            ..default()
        };

        commands.entity(display_entity).despawn_children();
        commands.entity(display_entity).with_children(|cb| {
            // Window name + monitor header
            let monitor_row = format_monitor_row(&monitor_info, &refresh_display);
            add_span(
                cb,
                &font,
                &format!("Window: {name}\n{monitor_row}\n\n"),
                DEFAULT_COLOR,
            );

            // Comparison table
            build_comparison_spans(cb, restored_state, window, &monitor_info, &font);

            // Video modes
            add_span(
                cb,
                &font,
                &format!("\nVideo Modes (Up/Down to select):\n{video_modes_display}\n"),
                DEFAULT_COLOR,
            );

            // Controls
            add_span(
                cb,
                &font,
                "\nControls:\n\
                 [1/Enter] Exclusive Fullscreen  [2] Borderless Fullscreen\n\
                 [W/Esc] Windowed\n\
                 [Space] Spawn managed window\n\
                 [P] Toggle persistence\n\
                 [Ctrl+Shift+Backspace] Clear state and quit\n\
                 [Q] Quit\n",
                DEFAULT_COLOR,
            );
        });
    }
}

// --- Input Handling ---

/// Handle global inputs: spawn windows, toggle persistence, quit, reset.
///
/// These work from any focused window, not just the primary.
fn handle_global_input(
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window>,
    managed_entities: Query<Entity, With<ManagedWindow>>,
    mut commands: Commands,
    mut counter: ResMut<WindowCounter>,
    mut persistence: ResMut<ManagedWindowPersistence>,
    mut app_exit: MessageWriter<AppExit>,
) {
    // Only process input when any window is focused
    if !windows.iter().any(|w| w.focused) {
        return;
    }

    if keys.just_pressed(KeyCode::Space) {
        counter.next += 1;
        let name = format!("window-{}", counter.next);
        let title = format!("Managed: {name}");

        commands.spawn((
            Window {
                title,
                resolution: bevy::window::WindowResolution::new(
                    SECONDARY_WINDOW_WIDTH,
                    SECONDARY_WINDOW_HEIGHT,
                ),
                ..default()
            },
            ManagedWindow {
                window_name: name.clone(),
            },
        ));

        info!("[restore_window] Spawned managed window \"{name}\"");
    }

    if keys.just_pressed(KeyCode::KeyP) {
        *persistence = match *persistence {
            ManagedWindowPersistence::RememberAll => ManagedWindowPersistence::ActiveOnly,
            ManagedWindowPersistence::ActiveOnly => ManagedWindowPersistence::RememberAll,
        };
        info!("[restore_window] Persistence mode: {:?}", *persistence);
    }

    // Ctrl+Shift+Backspace: clear saved state and exit
    if keys.just_pressed(KeyCode::Backspace)
        && keys.pressed(KeyCode::ShiftLeft)
        && keys.pressed(KeyCode::ControlLeft)
    {
        if let Some(state_path) = get_state_file_path() {
            if let Err(e) = std::fs::remove_file(&state_path) {
                warn!("[restore_window] Failed to remove state file: {e}");
            } else {
                info!("[restore_window] Cleared state file: {state_path:?}");
            }
        }
        despawn_managed_and_exit(&managed_entities, &mut commands, &mut app_exit);
    }

    if keys.just_pressed(KeyCode::KeyQ) {
        despawn_managed_and_exit(&managed_entities, &mut commands, &mut app_exit);
    }
}

/// Despawn all managed windows before writing `AppExit::Success`.
///
/// Bevy's graceful shutdown via `AppExit::Success` intermittently hangs on macOS
/// (spinning beach ball, requires force quit). This was reproduced in a bare Bevy
/// app with no plugins — it's a Bevy/winit issue, not ours. The hang is much more
/// reliable when `NSWindow.tabbingMode` is set to `Disallowed` (our tabbing fix).
///
/// The alternative — `std::process::exit(0)` — never hangs but panics when exiting
/// exclusive fullscreen with multiple windows, because it bypasses winit's cleanup
/// of fullscreen state before TLS destruction.
///
/// Despawning managed windows first lets them go through Bevy's normal window
/// teardown path (rendering thread gets notified), leaving only the primary window
/// for the `exiting()` callback to handle. This avoids the hang in practice.
fn despawn_managed_and_exit(
    managed_entities: &Query<Entity, With<ManagedWindow>>,
    commands: &mut Commands,
    app_exit: &mut MessageWriter<AppExit>,
) {
    for entity in managed_entities.iter() {
        commands.entity(entity).despawn();
    }
    app_exit.write(AppExit::Success);
}

/// Compute the state file path using the same logic as the plugin.
fn get_state_file_path() -> Option<std::path::PathBuf> {
    let exe_name = std::env::current_exe()
        .ok()?
        .file_stem()?
        .to_str()?
        .to_string();
    dirs::config_dir().map(|d| d.join(exe_name).join("windows.ron"))
}

/// Handle mode-switching and video mode navigation for the focused window.
fn handle_window_mode_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut windows: Query<(&mut Window, Option<&CurrentMonitor>)>,
    monitors_res: Res<Monitors>,
    bevy_monitors: Query<(Entity, &Monitor)>,
    mut selected: ResMut<SelectedVideoModes>,
) {
    // Find the focused window
    let Some((mut window, current_monitor)) = windows.iter_mut().find(|(w, _)| w.focused) else {
        return;
    };

    let monitor = current_monitor.copied().unwrap_or(CurrentMonitor {
        monitor:        *monitors_res.first(),
        effective_mode: window.mode,
    });

    // Sync `window.mode` to the effective mode so bevy's cached state matches reality.
    // The OS can change fullscreen state (e.g. macOS green button) without updating
    // `window.mode`, causing bevy's `changed_windows` to skip the mode change.
    //
    // Skip sync when the user has explicitly set an exclusive fullscreen mode — macOS
    // may reject it and briefly oscillate between Fullscreen and Windowed. Syncing
    // during that transition creates a feedback loop.
    let is_explicit_fullscreen = matches!(window.mode, WindowMode::Fullscreen(_, _));
    if !is_explicit_fullscreen && window.mode != monitor.effective_mode {
        window.mode = monitor.effective_mode;
    }

    let video_modes: Vec<VideoMode> = bevy_monitors
        .iter()
        .find(|(_, m)| m.physical_position == monitor.position)
        .map(|(_, m)| m.video_modes.clone())
        .unwrap_or_default();

    // Navigate video modes
    let current_idx = selected.get(monitor.index);
    if keys.just_pressed(KeyCode::ArrowUp) && current_idx > 0 {
        selected.set(monitor.index, current_idx - 1);
    }
    if keys.just_pressed(KeyCode::ArrowDown) && current_idx < video_modes.len().saturating_sub(1) {
        selected.set(monitor.index, current_idx + 1);
    }

    if keys.just_pressed(KeyCode::Digit1) || keys.just_pressed(KeyCode::Enter) {
        let selected_idx = selected
            .get(monitor.index)
            .min(video_modes.len().saturating_sub(1));
        let video_mode_selection = video_modes
            .get(selected_idx)
            .map_or(VideoModeSelection::Current, |mode| {
                VideoModeSelection::Specific(*mode)
            });

        window.mode =
            WindowMode::Fullscreen(MonitorSelection::Index(monitor.index), video_mode_selection);
    }
    if keys.just_pressed(KeyCode::Digit2) {
        window.mode = WindowMode::BorderlessFullscreen(MonitorSelection::Index(monitor.index));
    }
    if keys.just_pressed(KeyCode::KeyW) || keys.just_pressed(KeyCode::Escape) {
        window.mode = WindowMode::Windowed;
    }
}

// --- Video Mode Helpers ---

/// Get video modes and refresh rate for the monitor matching the given position.
fn get_video_modes_for_monitor<'a>(
    bevy_monitors: &'a Query<(Entity, &Monitor)>,
    monitor: &CurrentMonitor,
) -> (Vec<&'a VideoMode>, Option<u32>) {
    bevy_monitors
        .iter()
        .find(|(_, m)| m.physical_position == monitor.position)
        .map(|(_, m)| {
            (
                m.video_modes.iter().collect(),
                m.refresh_rate_millihertz.map(|r| r / 1000),
            )
        })
        .unwrap_or_default()
}

/// Format refresh rate - use video mode rate in exclusive fullscreen, otherwise monitor rate.
fn format_refresh_rate(window: &Window, monitor_refresh: Option<u32>) -> String {
    let active_refresh = match &window.mode {
        WindowMode::Fullscreen(_, VideoModeSelection::Specific(mode)) => {
            Some(mode.refresh_rate_millihertz / 1000)
        },
        _ => monitor_refresh,
    };
    active_refresh.map_or_else(|| "N/A".into(), |hz| format!("{hz}Hz"))
}

/// Find the index of the currently active video mode if in exclusive fullscreen.
fn find_active_video_mode_index(window: &Window, video_modes: &[&VideoMode]) -> Option<usize> {
    match &window.mode {
        WindowMode::Fullscreen(_, VideoModeSelection::Specific(active)) => {
            video_modes.iter().position(|m| {
                m.physical_size == active.physical_size
                    && m.refresh_rate_millihertz == active.refresh_rate_millihertz
            })
        },
        _ => None,
    }
}

/// Sync selected video mode index to active mode when mode changes.
fn sync_selected_to_active(
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

// --- Formatting Helpers ---

/// Get platform suffix for Linux (Wayland or X11).
///
/// Not const on Linux due to `std::env::var` check; clippy false positive on other platforms.
#[cfg_attr(not(target_os = "linux"), allow(clippy::missing_const_for_fn))]
fn platform_suffix() -> &'static str {
    #[cfg(target_os = "linux")]
    {
        if std::env::var("WAYLAND_DISPLAY")
            .map(|v| !v.is_empty())
            .unwrap_or(false)
        {
            " (Wayland)"
        } else {
            " (X11)"
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        ""
    }
}

/// Format the first row with monitor info.
fn format_monitor_row(monitor: &CurrentMonitor, refresh_display: &str) -> String {
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

/// Builds the video modes display string showing a scrollable window of modes.
fn build_video_modes_display(
    video_modes: &[&VideoMode],
    selected_idx: usize,
    active_mode_idx: Option<usize>,
) -> String {
    if video_modes.is_empty() {
        return "  (no video modes available)".into();
    }

    let selected_idx = selected_idx.min(video_modes.len().saturating_sub(1));
    let len = video_modes.len();

    // Determine the visible window start position
    let start = if len <= 5 {
        // Show all modes if 5 or fewer
        0
    } else {
        // Center on active mode (slot 3 of 5) if it exists, otherwise center on selected
        let center_target = active_mode_idx.unwrap_or(selected_idx);

        // But always ensure selected is visible by adjusting if needed
        let ideal_start = center_target.saturating_sub(2);
        let ideal_end = ideal_start + 5;

        // Check if selected would be outside the ideal window
        if selected_idx < ideal_start {
            // Selected is above the window, scroll up to show it
            selected_idx.saturating_sub(2)
        } else if selected_idx >= ideal_end {
            // Selected is below the window, scroll down to show it
            (selected_idx + 3).saturating_sub(5)
        } else {
            // Selected is visible, use the ideal centering on active
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

// --- Debug Systems ---

/// Debug system that runs every frame and logs winit-detected monitor changes.
fn debug_winit_monitor(
    window: Single<Entity, With<PrimaryWindow>>,
    monitors: Res<Monitors>,
    mut cached_monitor: Local<Option<usize>>,
    _non_send: NonSendMarker,
) {
    let window_entity = *window;

    let winit_monitor_index: Option<usize> = WINIT_WINDOWS.with(|ww| {
        let ww = ww.borrow();
        ww.get_window(window_entity).and_then(|winit_window| {
            winit_window.current_monitor().and_then(|current_monitor| {
                let pos = current_monitor.position();
                monitors.at(pos.x, pos.y).map(|mon| mon.index)
            })
        })
    });

    if *cached_monitor != winit_monitor_index {
        info!(
            "[debug_winit_monitor] Monitor changed: {:?} -> {:?}",
            *cached_monitor, winit_monitor_index
        );
        *cached_monitor = winit_monitor_index;
    }
}

/// Cached state for detecting what changed in Window component.
#[derive(Default)]
struct CachedWindowDebug {
    position: Option<WindowPosition>,
    width:    u32,
    height:   u32,
    mode:     Option<WindowMode>,
    focused:  bool,
}

/// Debug system that logs when Changed<Window> fires and what changed.
fn debug_window_changed(
    window: Single<&Window, (With<PrimaryWindow>, Changed<Window>)>,
    mut cached: Local<CachedWindowDebug>,
) {
    let w = *window;

    let position_changed = cached.position.as_ref() != Some(&w.position);
    let size_changed = cached.width != w.physical_width() || cached.height != w.physical_height();
    let mode_changed = cached.mode.as_ref() != Some(&w.mode);
    let focused_changed = cached.focused != w.focused;

    let mut changes = Vec::new();
    if position_changed {
        changes.push(format!(
            "position: {:?} -> {:?}",
            cached.position, w.position
        ));
    }
    if size_changed {
        changes.push(format!(
            "size: {}x{} -> {}x{}",
            cached.width,
            cached.height,
            w.physical_width(),
            w.physical_height()
        ));
    }
    if mode_changed {
        changes.push(format!("mode: {:?} -> {:?}", cached.mode, w.mode));
    }
    if focused_changed {
        changes.push(format!("focused: {} -> {}", cached.focused, w.focused));
    }

    if !changes.is_empty() {
        info!("[debug_window_changed] {}", changes.join(", "));
    }

    // Update cache
    cached.position = Some(w.position);
    cached.width = w.physical_width();
    cached.height = w.physical_height();
    cached.mode = Some(w.mode);
    cached.focused = w.focused;
}

/// Debug system that logs when `WindowScaleFactorChanged` messages are received.
fn debug_scale_factor_changed(mut messages: MessageReader<WindowScaleFactorChanged>) {
    for msg in messages.read() {
        info!(
            "[debug_scale_factor_changed] WindowScaleFactorChanged received: scale_factor={}",
            msg.scale_factor
        );
    }
}

/// Observer that logs when `WindowRestored` event is received and caches the restored state.
fn on_window_restored(
    trigger: On<WindowRestored>,
    mut commands: Commands,
    mut restored_states: ResMut<RestoredStates>,
) {
    let event = trigger.event();
    info!(
        "[on_window_restored] Restore complete: window_id={} entity={:?} position={:?} size={} mode={:?} monitor={}",
        event.window_id, event.entity, event.position, event.size, event.mode, event.monitor_index
    );

    restored_states.states.insert(
        event.entity,
        CachedRestoredState {
            position:      event.position,
            width:         event.size.x,
            height:        event.size.y,
            monitor_index: event.monitor_index,
            mode:          event.mode,
        },
    );

    commands.insert_resource(WindowRestoredReceived {
        position:      event.position,
        size:          event.size,
        mode:          event.mode,
        monitor_index: event.monitor_index,
    });
}
