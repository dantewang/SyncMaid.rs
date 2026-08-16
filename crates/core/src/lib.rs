//! SyncMaid's engine: everything that decides *what* to do to files, and does it safely.
//!
//! This crate is deliberately UI-free so its safety behaviour can be fault-injected against
//! an in-memory filesystem. Its error messages stay English — display strings live in the app.

pub mod filtering;
pub mod io;
pub mod model;
pub mod sync;
pub mod triggers;

mod text;
