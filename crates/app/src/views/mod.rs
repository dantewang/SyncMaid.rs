//! The window's views.

pub mod dialogs;
mod main_view;
pub mod mirror_delete;
mod settings;

pub use main_view::MainView;
pub use settings::{SettingsEvent, SettingsView};
