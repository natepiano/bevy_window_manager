use bevy::prelude::*;
use bevy::window::Monitor;
use bevy::window::PrimaryWindow;
use bevy::window::WindowPosition;
use bevy_kana::ToI32;
use bevy_kana::ToU32;
use bevy_window_manager::CurrentMonitor;
use bevy_window_manager::ManagedWindow;
use bevy_window_manager::ManagedWindowPersistence;
use bevy_window_manager::Monitors;

use super::input;
use super::state::CachedMismatchState;
use super::state::CachedRestoredState;
use super::state::DEFAULT_COLOR;
use super::state::FONT_SIZE;
use super::state::LABEL_WIDTH;
use super::state::MISMATCH_COLOR;
use super::state::MISMATCH_WARN_COLOR;
use super::state::MismatchStates;
use super::state::PrimaryDisplay;
use super::state::RestoredStates;
use super::state::SecondaryDisplay;
use super::state::SelectedVideoModes;

struct CurrentValues {
    position_phys: String,
    position_log:  String,
    size_phys:     String,
    size_log:      String,
    scale:         String,
    monitor:       String,
    mode:          String,
}

/// Build comparison spans (restored vs current) for a window and add them as `TextSpan` children.
fn build_comparison_spans(
    cb: &mut ChildSpawnerCommands,
    restored_state: Option<&CachedRestoredState>,
    mismatch_state: Option<&CachedMismatchState>,
    window: &Window,
    monitor: &CurrentMonitor,
    font: &TextFont,
) {
    let effective_mode = monitor.effective_mode;
    let scale = window.resolution.scale_factor();

    let current = CurrentValues {
        position_phys: match window.position {
            WindowPosition::At(pos) => format!("({}, {})", pos.x, pos.y),
            _ => "Automatic".to_string(),
        },
        position_log:  match window.position {
            WindowPosition::At(pos) => {
                let logical_x = (f64::from(pos.x) / f64::from(scale)).round().to_i32();
                let logical_y = (f64::from(pos.y) / f64::from(scale)).round().to_i32();
                format!("({logical_x}, {logical_y})")
            },
            _ => "Automatic".to_string(),
        },
        size_phys:     format!("{}x{}", window.physical_width(), window.physical_height()),
        size_log:      format!(
            "{}x{}",
            window.resolution.width().to_u32(),
            window.resolution.height().to_u32()
        ),
        scale:         format!("{scale}"),
        monitor:       format!("{}", monitor.index),
        mode:          format!("{effective_mode:?}"),
    };

    if let Some(state) = restored_state {
        build_restored_spans(cb, state, mismatch_state, &current, font);
    } else {
        build_current_only_spans(cb, &current, font);
    }

    add_span(
        cb,
        font,
        &format!("\nEffective Mode: {effective_mode:?}\n"),
        DEFAULT_COLOR,
    );
}

/// Render comparison rows when restore data is available.
#[expect(
    clippy::too_many_lines,
    reason = "UI builder — splitting would scatter tightly-coupled formatting logic"
)]
fn build_restored_spans(
    cb: &mut ChildSpawnerCommands,
    state: &CachedRestoredState,
    mismatch_state: Option<&CachedMismatchState>,
    current: &CurrentValues,
    font: &TextFont,
) {
    let file_pos_phys = state
        .physical_position
        .map_or_else(|| "None".to_string(), |p| format!("({}, {})", p.x, p.y));
    let file_pos_log = state
        .logical_position
        .map_or_else(|| "None".to_string(), |p| format!("({}, {})", p.x, p.y));
    let file_size_phys = format!("{}x{}", state.physical_size.x, state.physical_size.y);
    let file_size_log = format!("{}x{}", state.logical_size.x, state.logical_size.y);
    let file_monitor = format!("{}", state.monitor);
    let file_mode = format!("{:?}", state.mode);

    let col1_width = [
        file_pos_phys.len(),
        file_pos_log.len(),
        file_size_phys.len(),
        file_monitor.len(),
        file_mode.len(),
    ]
    .into_iter()
    .max()
    .unwrap_or(0)
        + 2;
    let col1_width = col1_width.max(16);

    // Header
    if mismatch_state.is_some() {
        let header = format!(
            "{:LABEL_WIDTH$}{:<col1_width$}{:<col1_width$}{:<col1_width$}{}\n",
            "", "Restored", "Current", "Expected", "Actual"
        );
        add_span(cb, font, &header, DEFAULT_COLOR);
    } else {
        let header = format!(
            "{:LABEL_WIDTH$}{:<col1_width$}{}\n",
            "", "Restored", "Current"
        );
        add_span(cb, font, &header, DEFAULT_COLOR);
    }

    // Position (physical)
    let mm = mismatch_state.map(|m| {
        let exp = m
            .physical_position
            .expected
            .map_or_else(|| "None".to_string(), |p| format!("({}, {})", p.x, p.y));
        let act = m
            .physical_position
            .actual
            .map_or_else(|| "None".to_string(), |p| format!("({}, {})", p.x, p.y));
        (exp, act)
    });
    add_row(
        cb,
        font,
        "Position (physical):",
        &file_pos_phys,
        &current.position_phys,
        mm.as_ref(),
        col1_width,
    );

    // Position (logical)
    let mm = mismatch_state.map(|m| {
        let exp = m
            .logical_position
            .expected
            .map_or_else(|| "None".to_string(), |p| format!("({}, {})", p.x, p.y));
        let act = m
            .logical_position
            .actual
            .map_or_else(|| "None".to_string(), |p| format!("({}, {})", p.x, p.y));
        (exp, act)
    });
    add_row(
        cb,
        font,
        "Position (logical):",
        &file_pos_log,
        &current.position_log,
        mm.as_ref(),
        col1_width,
    );

    // Size (physical)
    let mm = mismatch_state.map(|m| {
        (
            format!(
                "{}x{}",
                m.physical_size.expected.x, m.physical_size.expected.y
            ),
            format!("{}x{}", m.physical_size.actual.x, m.physical_size.actual.y),
        )
    });
    add_row(
        cb,
        font,
        "Size (physical):",
        &file_size_phys,
        &current.size_phys,
        mm.as_ref(),
        col1_width,
    );

    // Size (logical)
    let mm = mismatch_state.map(|m| {
        (
            format!(
                "{}x{}",
                m.logical_size.expected.x, m.logical_size.expected.y
            ),
            format!("{}x{}", m.logical_size.actual.x, m.logical_size.actual.y),
        )
    });
    add_row(
        cb,
        font,
        "Size (logical):",
        &file_size_log,
        &current.size_log,
        mm.as_ref(),
        col1_width,
    );

    // Scale (no file value; custom no-mismatch rendering)
    if let Some(m) = mismatch_state {
        let exp_scale = format!("{}", m.scale.expected);
        let act_scale = format!("{}", m.scale.actual);
        add_comparison_row_5(
            cb,
            font,
            "Scale:",
            "",
            &current.scale,
            &exp_scale,
            &act_scale,
            col1_width,
        );
    } else {
        add_span(
            cb,
            font,
            &format!(
                "{:<LABEL_WIDTH$}{:<col1_width$}{}\n",
                "Scale:", "", current.scale
            ),
            DEFAULT_COLOR,
        );
    }

    // Monitor
    let mm = mismatch_state.map(|m| {
        (
            format!("{}", m.monitor.expected),
            format!("{}", m.monitor.actual),
        )
    });
    add_row(
        cb,
        font,
        "Monitor:",
        &file_monitor,
        &current.monitor,
        mm.as_ref(),
        col1_width,
    );

    // Mode
    let mm = mismatch_state.map(|m| {
        (
            format!("{:?}", m.mode.expected),
            format!("{:?}", m.mode.actual),
        )
    });
    add_row(
        cb,
        font,
        "Mode:",
        &file_mode,
        &current.mode,
        mm.as_ref(),
        col1_width,
    );
}

/// Render current-only values when no restore data exists.
fn build_current_only_spans(
    cb: &mut ChildSpawnerCommands,
    current: &CurrentValues,
    font: &TextFont,
) {
    add_span(cb, font, "State: No restore data\n\n", MISMATCH_COLOR);
    add_span(
        cb,
        font,
        &format!(
            "{:<LABEL_WIDTH$}{}\n",
            "Position (physical):", current.position_phys
        ),
        DEFAULT_COLOR,
    );
    add_span(
        cb,
        font,
        &format!(
            "{:<LABEL_WIDTH$}{}\n",
            "Position (logical):", current.position_log
        ),
        DEFAULT_COLOR,
    );
    add_span(
        cb,
        font,
        &format!(
            "{:<LABEL_WIDTH$}{}\n",
            "Size (physical):", current.size_phys
        ),
        DEFAULT_COLOR,
    );
    add_span(
        cb,
        font,
        &format!("{:<LABEL_WIDTH$}{}\n", "Size (logical):", current.size_log),
        DEFAULT_COLOR,
    );
    add_span(
        cb,
        font,
        &format!("{:<LABEL_WIDTH$}{}\n", "Scale:", current.scale),
        DEFAULT_COLOR,
    );
    add_span(
        cb,
        font,
        &format!("{:<LABEL_WIDTH$}{}\n", "Monitor:", current.monitor),
        DEFAULT_COLOR,
    );
    add_span(
        cb,
        font,
        &format!("{:<LABEL_WIDTH$}{}\n", "Mode:", current.mode),
        DEFAULT_COLOR,
    );
}

/// Add a comparison row, dispatching to 3-column or 5-column layout based on mismatch data.
fn add_row(
    cb: &mut ChildSpawnerCommands,
    font: &TextFont,
    label: &str,
    file_val: &str,
    current_val: &str,
    mismatch: Option<&(String, String)>,
    col_width: usize,
) {
    if let Some((expected, actual)) = mismatch {
        add_comparison_row_5(
            cb,
            font,
            label,
            file_val,
            current_val,
            expected,
            actual,
            col_width,
        );
    } else {
        add_comparison_row(cb, font, label, file_val, current_val, col_width);
    }
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

/// Add a 5-column comparison row: label + restored + current + expected + actual.
/// Expected/actual columns use warning color when they differ.
fn add_comparison_row_5(
    cb: &mut ChildSpawnerCommands,
    font: &TextFont,
    label: &str,
    file_val: &str,
    current_val: &str,
    expected_val: &str,
    actual_val: &str,
    col_width: usize,
) {
    let current_color = if file_val == current_val {
        DEFAULT_COLOR
    } else {
        MISMATCH_COLOR
    };
    let mismatch_color = if expected_val == actual_val {
        DEFAULT_COLOR
    } else {
        MISMATCH_WARN_COLOR
    };

    // Label + restored value (always white)
    add_span(
        cb,
        font,
        &format!("{label:<LABEL_WIDTH$}{file_val:<col_width$}"),
        DEFAULT_COLOR,
    );
    // Current value
    add_span(
        cb,
        font,
        &format!("{current_val:<col_width$}"),
        current_color,
    );
    // Expected value (always white)
    add_span(
        cb,
        font,
        &format!("{expected_val:<col_width$}"),
        DEFAULT_COLOR,
    );
    // Actual value (warning color if mismatch)
    add_span(cb, font, &format!("{actual_val}\n"), mismatch_color);
}

/// Add a single `TextSpan` child.
fn add_span(cb: &mut ChildSpawnerCommands, font: &TextFont, text: &str, color: Color) {
    cb.spawn((TextSpan(text.to_string()), font.clone(), TextColor(color)));
}

// --- Primary Window Display ---

#[expect(
    clippy::too_many_arguments,
    reason = "Bevy system — each param is a distinct system resource"
)]
pub(crate) fn update_primary_display(
    primary_display: Single<Entity, With<PrimaryDisplay>>,
    window_query: Single<(Entity, &Window, &CurrentMonitor), With<PrimaryWindow>>,
    monitors_res: Res<Monitors>,
    bevy_monitors: Query<(Entity, &Monitor)>,
    mut selected: ResMut<SelectedVideoModes>,
    persistence: Res<ManagedWindowPersistence>,
    managed_q: Query<(&Window, &ManagedWindow, Option<&CurrentMonitor>)>,
    restored_states: Res<RestoredStates>,
    mismatch_states: Res<MismatchStates>,
    mut commands: Commands,
) {
    let display_entity = *primary_display;
    let (window_entity, window, monitor) = *window_query;

    let restored_state = restored_states.states.get(&window_entity);
    let mismatch_state = mismatch_states.states.get(&window_entity);

    let (video_modes, refresh_rate) = input::get_video_modes_for_monitor(&bevy_monitors, monitor);
    let refresh_display = input::format_refresh_rate(window, refresh_rate);
    let active_mode_idx = input::find_active_video_mode_index(window, &video_modes);
    input::sync_selected_to_active(window, monitor, active_mode_idx, &mut selected);
    let selected_idx = selected.get(monitor.index);
    let video_modes_display =
        input::build_video_modes_display(&video_modes, selected_idx, active_mode_idx);

    let font = TextFont {
        font_size: FONT_SIZE,
        ..default()
    };

    commands.entity(display_entity).despawn_children();
    commands.entity(display_entity).with_children(|cb| {
        // Monitor header
        let monitor_row = input::format_monitor_row(monitor, &refresh_display);
        add_span(cb, &font, &format!("{monitor_row}\n\n"), DEFAULT_COLOR);

        // Comparison table
        build_comparison_spans(cb, restored_state, mismatch_state, window, monitor, &font);

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
                 [Enter] Exclusive Fullscreen\n\
                 [B] Borderless Fullscreen\n\
                 [W] Windowed\n\
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
            let mon = current_monitor.map_or_else(|| *monitors_res.first(), |cm| cm.monitor);
            let pos = match mw.position {
                WindowPosition::At(p) => format!("({}, {})", p.x, p.y),
                _ => "Automatic".to_string(),
            };
            managed_lines.push(format!(
                "  {}: pos={pos} phys={}x{} log={}x{} scale={} monitor={}\n",
                managed.name,
                mw.physical_width(),
                mw.physical_height(),
                mw.resolution.width().to_u32(),
                mw.resolution.height().to_u32(),
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

#[expect(
    clippy::too_many_arguments,
    reason = "Bevy system — each param is a distinct system resource"
)]
pub(crate) fn update_secondary_displays(
    mut displays: Query<(Entity, &SecondaryDisplay)>,
    windows: Query<(&Window, Option<&CurrentMonitor>)>,
    managed_q: Query<&ManagedWindow>,
    monitors_res: Res<Monitors>,
    bevy_monitors: Query<(Entity, &Monitor)>,
    mut selected: ResMut<SelectedVideoModes>,
    restored_states: Res<RestoredStates>,
    mismatch_states: Res<MismatchStates>,
    mut commands: Commands,
) {
    for (display_entity, display) in &mut displays {
        let Ok((window, current_monitor)) = windows.get(display.0) else {
            continue;
        };
        let monitor_info = current_monitor.copied().unwrap_or_else(|| CurrentMonitor {
            monitor:        *monitors_res.first(),
            effective_mode: window.mode,
        });

        let name = managed_q.get(display.0).map_or("unknown", |m| &m.name);
        let restored_state = restored_states.states.get(&display.0);
        let mismatch_state = mismatch_states.states.get(&display.0);

        let (video_modes, refresh_rate) =
            input::get_video_modes_for_monitor(&bevy_monitors, &monitor_info);
        let refresh_display = input::format_refresh_rate(window, refresh_rate);
        let active_mode_idx = input::find_active_video_mode_index(window, &video_modes);
        input::sync_selected_to_active(window, &monitor_info, active_mode_idx, &mut selected);
        let selected_idx = selected.get(monitor_info.index);
        let video_modes_display =
            input::build_video_modes_display(&video_modes, selected_idx, active_mode_idx);

        let font = TextFont {
            font_size: FONT_SIZE,
            ..default()
        };

        commands.entity(display_entity).despawn_children();
        commands.entity(display_entity).with_children(|cb| {
            // Window name + monitor header
            let monitor_row = input::format_monitor_row(&monitor_info, &refresh_display);
            add_span(
                cb,
                &font,
                &format!("Window: {name}\n{monitor_row}\n\n"),
                DEFAULT_COLOR,
            );

            // Comparison table
            build_comparison_spans(
                cb,
                restored_state,
                mismatch_state,
                window,
                &monitor_info,
                &font,
            );

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
                 [Enter] Exclusive Fullscreen\n\
                 [B] Borderless Fullscreen\n\
                 [W] Windowed\n\
                 [Space] Spawn managed window\n\
                 [P] Toggle persistence\n\
                 [Ctrl+Shift+Backspace] Clear state and quit\n\
                 [Q] Quit\n",
                DEFAULT_COLOR,
            );
        });
    }
}
