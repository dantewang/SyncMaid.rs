//! Planning and applying a sync run.
//!
//! The shape is deliberate: **planning is pure** (it only reads, and returns a list of
//! operations), **applying is the only mutation** (one `match`, one code path), and the safety
//! invariants sit between them where a test can reach them. Stated priority: avoid file loss
//! at all costs. Nothing here gets a faster path that skips a guard.

mod applier;
mod engine;
mod error;
mod mirror_guard;
mod operation;
mod planner;
mod provider;
mod retry;
mod routing;
mod safe_transfer;

pub use applier::apply;
pub use engine::{CancellationToken, Cancelled, SyncEngine};
pub use error::{OperationError, SyncOperationError};
pub use mirror_guard::{
    evaluate as evaluate_mirror_guard, MirrorDeletePreview, MirrorGuardVerdict,
    MIN_DESTINATION_FILES_FOR_RATIO_GUARD,
};
pub use operation::{SyncOperation, SyncPlan, SyncProgress};
pub use planner::plan;
pub use provider::{
    DestinationCapabilities, DestinationProvider, DestinationProviderFactory,
    LocalDestinationProvider, LocalDestinationProviderFactory, LocalSourceFile, SourceFile,
};
pub use retry::{execute as retry, RetryOptions};
pub use routing::MoveRouting;
