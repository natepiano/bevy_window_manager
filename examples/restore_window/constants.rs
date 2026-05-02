use bevy::prelude::*;

/// When the selected video mode falls below the visible window, scroll down so
/// the new selection lands at row 2 of the 5-row window (i.e.
/// `selected - BACKWARD_SCROLL_OFFSET`). Mathematically:
/// `selected + FORWARD_SCROLL_OFFSET - VISIBLE_VIDEO_MODE_COUNT
///  = selected - BACKWARD_SCROLL_OFFSET`.
pub(crate) const BACKWARD_SCROLL_OFFSET: usize = 2;
pub(crate) const COMPARISON_COLUMN_PADDING: usize = 2;
pub(crate) const DEFAULT_COLOR: Color = Color::WHITE;
pub(crate) const FONT_SIZE: f32 = 14.0;
/// Forward-scroll constant for the video-mode list. See `BACKWARD_SCROLL_OFFSET`.
pub(crate) const FORWARD_SCROLL_OFFSET: usize = 3;
pub(crate) const LABEL_WIDTH: usize = 22;
pub(crate) const MARGIN: Val = Val::Px(20.0);
pub(crate) const MILLIHERTZ_PER_HERTZ: u32 = 1000;
pub(crate) const MIN_COMPARISON_COLUMN_WIDTH: usize = 16;
pub(crate) const MISMATCH_COLOR: Color = Color::linear_rgb(1.0, 0.3, 0.3);
pub(crate) const MISMATCH_WARN_COLOR: Color = Color::linear_rgb(1.0, 0.7, 0.2);
pub(crate) const SECONDARY_WINDOW_HEIGHT: u32 = 400;
pub(crate) const SECONDARY_WINDOW_WIDTH: u32 = 600;
pub(crate) const TEST_MODE_ENV_VAR: &str = "BWM_TEST_MODE";
/// Rows of context shown above the active video mode when centering the
/// 5-row visible window on it.
pub(crate) const VIDEO_MODE_CENTER_PADDING: usize = 2;
pub(crate) const VISIBLE_VIDEO_MODE_COUNT: usize = 5;
