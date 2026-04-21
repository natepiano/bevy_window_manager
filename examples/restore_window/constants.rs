use bevy::prelude::*;

pub(crate) const DEFAULT_COLOR: Color = Color::WHITE;
pub(crate) const FONT_SIZE: f32 = 14.0;
pub(crate) const LABEL_WIDTH: usize = 22;
pub(crate) const MARGIN: Val = Val::Px(20.0);
pub(crate) const MISMATCH_COLOR: Color = Color::linear_rgb(1.0, 0.3, 0.3);
pub(crate) const MISMATCH_WARN_COLOR: Color = Color::linear_rgb(1.0, 0.7, 0.2);
pub(crate) const SECONDARY_WINDOW_HEIGHT: u32 = 400;
pub(crate) const SECONDARY_WINDOW_WIDTH: u32 = 600;
pub(crate) const TEST_MODE_ENV_VAR: &str = "BWM_TEST_MODE";
