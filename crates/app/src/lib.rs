//! SyncMaid's desktop shell.
//!
//! A library as well as a binary, so the parts that carry real rules — the run gate, the card
//! summary, the workspace's persistence guards — can be tested end to end against the engine
//! without a window.

pub mod assets;
pub mod components;
pub mod i18n;
pub mod platform;
pub mod services;
pub mod state;
pub mod strings;
pub mod theme;
pub mod views;
