//! What the UI shows, kept apart from how it is drawn.
//!
//! Everything here is plain Rust with no window in sight, so the rules the user actually feels
//! — which summary a card shows, which button is enabled, what a burst of triggers costs — are
//! testable without a renderer.

// The state layer is complete ahead of the views that read it: the editors, the settings page
// and the run commands are the callers still to land. Drop this once they have.
#![allow(dead_code)]

mod health;
mod overlap;
mod run_gate;
mod workspace;

pub use health::health_of;
pub use overlap::{destination_conflict, sibling_conflict, source_conflict, Conflict};
pub use run_gate::{RunGate, RunStart};
pub use workspace::Workspace;
