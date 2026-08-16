//! Shared scaffolding for the engine's integration tests.

#![allow(dead_code)]

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use chrono::{DateTime, TimeZone, Utc};
use syncmaid_core::filtering::FilterRule;
use syncmaid_core::io::{FileSystem, InMemoryFileSystem, ListedDirectory, ListedFile, TreeListing};
use syncmaid_core::model::{
    Destination, DestinationSyncStatus, SyncStrategy, SyncTask, SyncTaskKind,
};
use syncmaid_core::sync::{
    CancellationToken, DestinationProvider, LocalDestinationProvider,
    LocalDestinationProviderFactory, RetryOptions, SyncEngine, SyncOperation, SyncProgress,
};
use syncmaid_core::triggers::Trigger;

pub const SOURCE: &str = r"C:\src";
pub const DESTINATION: &str = r"D:\dst";

pub fn at(minute: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 8, 9, 2, minute, 0).unwrap()
}

pub fn memory() -> InMemoryFileSystem {
    InMemoryFileSystem::new()
}

pub fn shared(file_system: &InMemoryFileSystem) -> Arc<dyn FileSystem> {
    Arc::new(file_system.clone())
}

/// An engine that does not retry, so a test asserting on failure does not also pay for the
/// backoff. Retry has its own tests.
pub fn engine(file_system: &InMemoryFileSystem) -> SyncEngine {
    let file_system = shared(file_system);
    let factory = Arc::new(LocalDestinationProviderFactory::new(Arc::clone(
        &file_system,
    )));
    SyncEngine::new(file_system, factory, RetryOptions::NONE)
}

/// The engine as it ships, backoff and all.
pub fn engine_with_default_retry(file_system: &InMemoryFileSystem) -> SyncEngine {
    SyncEngine::local(shared(file_system))
}

pub fn provider(file_system: &InMemoryFileSystem, root: &str) -> LocalDestinationProvider {
    LocalDestinationProvider::new(shared(file_system), root)
}

pub fn mirror(name: &str, path: &str) -> Destination {
    Destination::new(name, path, [FilterRule::AllFiles], SyncStrategy::Mirror)
}

pub fn add_only(name: &str, path: &str, filters: Vec<FilterRule>) -> Destination {
    Destination::new(name, path, filters, SyncStrategy::AddOnly)
}

pub fn moving(name: &str, path: &str, filters: Vec<FilterRule>) -> Destination {
    Destination::new(name, path, filters, SyncStrategy::Move)
}

pub fn sync_task(destinations: Vec<Destination>) -> SyncTask {
    SyncTask::new("T", SOURCE, Trigger::Manual, destinations).with_kind(SyncTaskKind::Sync)
}

pub fn move_task(destinations: Vec<Destination>) -> SyncTask {
    SyncTask::new("T", SOURCE, Trigger::Manual, destinations).with_kind(SyncTaskKind::Move)
}

/// Runs a task to completion, discarding progress.
pub fn run(engine: &SyncEngine, task: &SyncTask) -> Vec<DestinationSyncStatus> {
    engine
        .execute(
            task,
            &CancellationToken::new(),
            &mut |_| {},
            &HashSet::new(),
        )
        .expect("the run was not cancelled")
}

/// Runs a task with a set of confirmed mass deletions.
pub fn run_confirming(
    engine: &SyncEngine,
    task: &SyncTask,
    confirmed: &HashSet<uuid::Uuid>,
) -> Vec<DestinationSyncStatus> {
    engine
        .execute(task, &CancellationToken::new(), &mut |_| {}, confirmed)
        .expect("the run was not cancelled")
}

/// Runs a task, collecting every progress report.
pub fn run_collecting_progress(
    engine: &SyncEngine,
    task: &SyncTask,
) -> (Vec<DestinationSyncStatus>, Vec<SyncProgress>) {
    let mut reports = Vec::new();
    let statuses = engine
        .execute(
            task,
            &CancellationToken::new(),
            &mut |report| reports.push(report),
            &HashSet::new(),
        )
        .expect("the run was not cancelled");
    (statuses, reports)
}

/// Plans one destination against the in-memory filesystem.
pub fn plan_for(
    file_system: &InMemoryFileSystem,
    destination: &Destination,
) -> syncmaid_core::sync::SyncPlan {
    let listing = file_system
        .list_tree(Path::new(SOURCE))
        .unwrap_or_else(|_| TreeListing::empty());
    let filtered: Vec<ListedFile> = listing
        .files
        .iter()
        .filter(|file| destination.includes(&file.relative_path))
        .cloned()
        .collect();
    let provider = provider(file_system, destination.local_path());
    syncmaid_core::sync::plan(
        Path::new(SOURCE),
        &provider as &dyn DestinationProvider,
        destination,
        &filtered,
        &listing.directories,
    )
    .expect("planning does not fail here")
}

/// The relative paths of a listing's files, sorted.
pub fn file_paths(listing: &TreeListing) -> Vec<String> {
    sorted(
        listing
            .files
            .iter()
            .map(|f: &ListedFile| f.relative_path.clone())
            .collect(),
    )
}

/// The relative paths of a listing's directories, sorted.
pub fn directory_paths(listing: &TreeListing) -> Vec<String> {
    sorted(
        listing
            .directories
            .iter()
            .map(|d: &ListedDirectory| d.relative_path.clone())
            .collect(),
    )
}

pub fn sorted(mut values: Vec<String>) -> Vec<String> {
    values.sort();
    values
}

/// A short label per operation, so a plan can be asserted as a readable sequence.
pub fn describe(operations: &[SyncOperation]) -> Vec<String> {
    operations
        .iter()
        .map(|operation| {
            let verb = match operation {
                SyncOperation::Copy { .. } => "copy",
                SyncOperation::CreateDirectory { .. } => "mkdir",
                SyncOperation::Delete { .. } => "rm",
                SyncOperation::DeleteDirectory { .. } => "rmdir",
                SyncOperation::SetDirectoryTimestamp { .. } => "time",
                SyncOperation::Move { .. } => "move",
            };
            format!("{verb} {}", operation.relative_path())
        })
        .collect()
}
