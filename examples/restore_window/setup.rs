use bevy::camera::RenderTarget;
use bevy::prelude::*;
use bevy::ui::UiTargetCamera;
use bevy::window::PrimaryWindow;
use bevy::window::WindowRef;
use bevy::window::WindowResolution;
use bevy_window_manager::ManagedWindow;

use super::events::SpawnManagedWindow;
use super::state::FONT_SIZE;
use super::state::MARGIN;
use super::state::PrimaryDisplay;
use super::state::SECONDARY_WINDOW_HEIGHT;
use super::state::SECONDARY_WINDOW_WIDTH;
use super::state::SecondaryDisplay;
use super::state::WindowCounter;

pub(crate) fn setup(mut commands: Commands) {
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

pub(crate) fn on_spawn_managed_window(
    _trigger: On<SpawnManagedWindow>,
    mut commands: Commands,
    mut counter: ResMut<WindowCounter>,
) {
    counter.next += 1;
    let name = format!("window-{}", counter.next);
    let title = format!("Managed: {name}");

    commands.spawn((
        Window {
            title,
            resolution: WindowResolution::new(SECONDARY_WINDOW_WIDTH, SECONDARY_WINDOW_HEIGHT),
            ..default()
        },
        ManagedWindow { name: name.clone() },
    ));

    info!("[restore_window] Spawned managed window \"{name}\"");
}

pub(crate) fn on_secondary_window_added(
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

pub(crate) fn on_secondary_window_removed(
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
