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

use super::constants::DEFAULT_COLOR;
use super::constants::FONT_SIZE;
use super::constants::LABEL_WIDTH;
use super::constants::MISMATCH_COLOR;
use super::constants::MISMATCH_WARN_COLOR;
use super::events::CachedMismatchState;
use super::events::CachedRestoredState;
use super::events::MismatchStates;
use super::events::RestoredStates;
use super::input;
use super::state::PrimaryDisplay;
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

struct ComparisonMismatch {
    expected: String,
    actual:   String,
}

struct ComparisonRow<'a> {
    label:    &'a str,
    restored: String,
    current:  String,
    mismatch: Option<ComparisonMismatch>,
}

struct RestoredValues {
    physical_position: String,
    logical_position:  String,
    physical_size:     String,
    logical_size:      String,
    monitor:           String,
    mode:              String,
}

impl RestoredValues {
    fn from_state(state: &CachedRestoredState) -> Self {
        Self {
            physical_position: state.physical_position.map_or_else(
                || "None".to_string(),
                |position| format!("({}, {})", position.x, position.y),
            ),
            logical_position:  state.logical_position.map_or_else(
                || "None".to_string(),
                |position| format!("({}, {})", position.x, position.y),
            ),
            physical_size:     format!("{}x{}", state.physical_size.x, state.physical_size.y),
            logical_size:      format!("{}x{}", state.logical_size.x, state.logical_size.y),
            monitor:           state.monitor.to_string(),
            mode:              format!("{:?}", state.mode),
        }
    }

    fn comparison_width(&self) -> usize {
        [
            self.physical_position.len(),
            self.logical_position.len(),
            self.physical_size.len(),
            self.logical_size.len(),
            self.monitor.len(),
            self.mode.len(),
        ]
        .into_iter()
        .max()
        .unwrap_or(0)
            + 2
    }
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
fn build_restored_spans(
    cb: &mut ChildSpawnerCommands,
    state: &CachedRestoredState,
    mismatch_state: Option<&CachedMismatchState>,
    current: &CurrentValues,
    font: &TextFont,
) {
    let restored = RestoredValues::from_state(state);
    let col_width = restored.comparison_width().max(16);

    add_restored_header(cb, font, mismatch_state.is_some(), col_width);
    add_position_rows(cb, font, &restored, current, mismatch_state, col_width);
    add_size_rows(cb, font, &restored, current, mismatch_state, col_width);
    add_scale_row(cb, font, current, mismatch_state, col_width);
    add_monitor_row(cb, font, &restored, current, mismatch_state, col_width);
    add_mode_row(cb, font, &restored, current, mismatch_state, col_width);
}

fn add_restored_header(
    cb: &mut ChildSpawnerCommands,
    font: &TextFont,
    has_mismatch: bool,
    col_width: usize,
) {
    let header = if has_mismatch {
        format!(
            "{:LABEL_WIDTH$}{:<col_width$}{:<col_width$}{:<col_width$}{}\n",
            "", "Restored", "Current", "Expected", "Actual"
        )
    } else {
        format!(
            "{:LABEL_WIDTH$}{:<col_width$}{}\n",
            "", "Restored", "Current"
        )
    };
    add_span(cb, font, &header, DEFAULT_COLOR);
}

fn add_position_rows(
    cb: &mut ChildSpawnerCommands,
    font: &TextFont,
    restored: &RestoredValues,
    current: &CurrentValues,
    mismatch_state: Option<&CachedMismatchState>,
    col_width: usize,
) {
    add_row(
        cb,
        font,
        &ComparisonRow {
            label:    "Position (physical):",
            restored: restored.physical_position.clone(),
            current:  current.position_phys.clone(),
            mismatch: mismatch_state.map(|mismatch| ComparisonMismatch {
                expected: mismatch.physical_position.expected.map_or_else(
                    || "None".to_string(),
                    |position| format!("({}, {})", position.x, position.y),
                ),
                actual:   mismatch.physical_position.actual.map_or_else(
                    || "None".to_string(),
                    |position| format!("({}, {})", position.x, position.y),
                ),
            }),
        },
        col_width,
    );
    add_row(
        cb,
        font,
        &ComparisonRow {
            label:    "Position (logical):",
            restored: restored.logical_position.clone(),
            current:  current.position_log.clone(),
            mismatch: mismatch_state.map(|mismatch| ComparisonMismatch {
                expected: mismatch.logical_position.expected.map_or_else(
                    || "None".to_string(),
                    |position| format!("({}, {})", position.x, position.y),
                ),
                actual:   mismatch.logical_position.actual.map_or_else(
                    || "None".to_string(),
                    |position| format!("({}, {})", position.x, position.y),
                ),
            }),
        },
        col_width,
    );
}

fn add_size_rows(
    cb: &mut ChildSpawnerCommands,
    font: &TextFont,
    restored: &RestoredValues,
    current: &CurrentValues,
    mismatch_state: Option<&CachedMismatchState>,
    col_width: usize,
) {
    add_row(
        cb,
        font,
        &ComparisonRow {
            label:    "Size (physical):",
            restored: restored.physical_size.clone(),
            current:  current.size_phys.clone(),
            mismatch: mismatch_state.map(|mismatch| ComparisonMismatch {
                expected: format!(
                    "{}x{}",
                    mismatch.physical_size.expected.x, mismatch.physical_size.expected.y
                ),
                actual:   format!(
                    "{}x{}",
                    mismatch.physical_size.actual.x, mismatch.physical_size.actual.y
                ),
            }),
        },
        col_width,
    );
    add_row(
        cb,
        font,
        &ComparisonRow {
            label:    "Size (logical):",
            restored: restored.logical_size.clone(),
            current:  current.size_log.clone(),
            mismatch: mismatch_state.map(|mismatch| ComparisonMismatch {
                expected: format!(
                    "{}x{}",
                    mismatch.logical_size.expected.x, mismatch.logical_size.expected.y
                ),
                actual:   format!(
                    "{}x{}",
                    mismatch.logical_size.actual.x, mismatch.logical_size.actual.y
                ),
            }),
        },
        col_width,
    );
}

fn add_scale_row(
    cb: &mut ChildSpawnerCommands,
    font: &TextFont,
    current: &CurrentValues,
    mismatch_state: Option<&CachedMismatchState>,
    col_width: usize,
) {
    if mismatch_state.is_none() {
        add_span(
            cb,
            font,
            &format!(
                "{:<LABEL_WIDTH$}{:<col_width$}{}\n",
                "Scale:", "", current.scale
            ),
            DEFAULT_COLOR,
        );
        return;
    }

    let row = ComparisonRow {
        label:    "Scale:",
        restored: String::new(),
        current:  current.scale.clone(),
        mismatch: mismatch_state.map(|mismatch| ComparisonMismatch {
            expected: mismatch.scale.expected.to_string(),
            actual:   mismatch.scale.actual.to_string(),
        }),
    };
    add_row(cb, font, &row, col_width);
}

fn add_monitor_row(
    cb: &mut ChildSpawnerCommands,
    font: &TextFont,
    restored: &RestoredValues,
    current: &CurrentValues,
    mismatch_state: Option<&CachedMismatchState>,
    col_width: usize,
) {
    add_row(
        cb,
        font,
        &ComparisonRow {
            label:    "Monitor:",
            restored: restored.monitor.clone(),
            current:  current.monitor.clone(),
            mismatch: mismatch_state.map(|mismatch| ComparisonMismatch {
                expected: mismatch.monitor.expected.to_string(),
                actual:   mismatch.monitor.actual.to_string(),
            }),
        },
        col_width,
    );
}

fn add_mode_row(
    cb: &mut ChildSpawnerCommands,
    font: &TextFont,
    restored: &RestoredValues,
    current: &CurrentValues,
    mismatch_state: Option<&CachedMismatchState>,
    col_width: usize,
) {
    add_row(
        cb,
        font,
        &ComparisonRow {
            label:    "Mode:",
            restored: restored.mode.clone(),
            current:  current.mode.clone(),
            mismatch: mismatch_state.map(|mismatch| ComparisonMismatch {
                expected: format!("{:?}", mismatch.mode.expected),
                actual:   format!("{:?}", mismatch.mode.actual),
            }),
        },
        col_width,
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
    row: &ComparisonRow<'_>,
    col_width: usize,
) {
    if let Some(mismatch) = row.mismatch.as_ref() {
        add_extended_comparison_row(cb, font, row, mismatch, col_width);
    } else {
        add_standard_comparison_row(cb, font, row, col_width);
    }
}

/// Add a comparison row: label + file value (white) + current value (white or red if mismatch).
fn add_standard_comparison_row(
    cb: &mut ChildSpawnerCommands,
    font: &TextFont,
    row: &ComparisonRow<'_>,
    col_width: usize,
) {
    let color = if row.restored == row.current {
        DEFAULT_COLOR
    } else {
        MISMATCH_COLOR
    };

    // Label + file value (always white)
    add_span(
        cb,
        font,
        &format!(
            "{label:<LABEL_WIDTH$}{restored:<col_width$}",
            label = row.label,
            restored = row.restored
        ),
        DEFAULT_COLOR,
    );
    // Current value (colored)
    add_span(cb, font, &format!("{}\n", row.current), color);
}

/// Add a 5-column comparison row: label + restored + current + expected + actual.
/// Expected/actual columns use warning color when they differ.
fn add_extended_comparison_row(
    cb: &mut ChildSpawnerCommands,
    font: &TextFont,
    row: &ComparisonRow<'_>,
    mismatch: &ComparisonMismatch,
    col_width: usize,
) {
    let current_color = if row.restored == row.current {
        DEFAULT_COLOR
    } else {
        MISMATCH_COLOR
    };
    let mismatch_color = if mismatch.expected == mismatch.actual {
        DEFAULT_COLOR
    } else {
        MISMATCH_WARN_COLOR
    };

    // Label + restored value (always white)
    add_span(
        cb,
        font,
        &format!(
            "{label:<LABEL_WIDTH$}{restored:<col_width$}",
            label = row.label,
            restored = row.restored
        ),
        DEFAULT_COLOR,
    );
    // Current value
    add_span(
        cb,
        font,
        &format!("{current:<col_width$}", current = row.current),
        current_color,
    );
    // Expected value (always white)
    add_span(
        cb,
        font,
        &format!("{expected:<col_width$}", expected = mismatch.expected),
        DEFAULT_COLOR,
    );
    // Actual value (warning color if mismatch)
    add_span(cb, font, &format!("{}\n", mismatch.actual), mismatch_color);
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
