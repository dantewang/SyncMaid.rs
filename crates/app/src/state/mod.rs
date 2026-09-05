//! What the UI shows, kept apart from how it is drawn.
//!
//! Everything here is plain Rust with no window in sight, so the rules the user actually feels
//! — which summary a card shows, which button is enabled, what a burst of triggers costs — are
//! testable without a renderer.

mod filters;
mod health;
mod overlap;
mod preview;
mod routing;
mod run_gate;
mod triggers;
mod workspace;

pub use filters::{FilterEntry, FilterGroup, FilterKind, FilterModel, Summary};
pub use health::health_of;
pub use overlap::{destination_conflict, sibling_conflict, source_conflict, Conflict};
pub use preview::{scan, ContestedFile, DestinationPreview, ExtensionChip, Scan};
pub use routing::subsumes;
pub use run_gate::{RunGate, RunStart};
pub use triggers::{TriggerEvent, TriggerHost, TriggerStart};
pub use workspace::Workspace;
