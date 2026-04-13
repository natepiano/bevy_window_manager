//! Window state persistence: types, serialization format, and I/O.

pub(crate) mod format;
mod load;
pub(crate) mod save;
mod types;

pub(crate) use load::get_default_state_path;
pub(crate) use load::get_state_path_for_app;
pub(crate) use load::load_all_states;
pub(crate) use save::save_active_window_state;
pub(crate) use save::save_all_states;
pub(crate) use save::save_window_state;
#[cfg(test)]
pub(crate) use types::SavedVideoMode;
pub(crate) use types::SavedWindowMode;
pub(crate) use types::WindowState;
