//! Reading and writing SyncMaid's own files.
//!
//! Config writes go through [`atomic_file::write`] — temp, then rename, with the previous
//! version kept as `.bak` and loaded as a fallback. Nothing ever writes over `tasks.json` in
//! place. Old config keeps loading; legacy shapes are normalized on save rather than
//! discarded.

pub mod atomic_file;

mod config_location;
mod json_config;
mod stores;

pub use config_location::{ConfigLocation, DATA_DIRECTORY};
pub use json_config::{save as save_json, to_config_json, try_load_with_backup, Loaded};
pub use stores::{SettingsStore, StatusStore, TaskStore};
