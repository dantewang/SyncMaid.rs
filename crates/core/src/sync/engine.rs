//! Runs a task: walk the source once, then for each destination filter, plan, guard, apply.
//!
//! This type only orchestrates. Planning is [`crate::sync::plan`], mutation is
//! [`crate::sync::apply`], and the deletion guard is [`crate::sync::evaluate_mirror_guard`] —
//! kept apart so each can be tested on its own and none of them can be quietly bypassed.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use chrono::Utc;
use uuid::Uuid;

use crate::io::{self, FileSystem, ListedDirectory, ListedFile};
use crate::model::{
    DeleteMode, Destination, DestinationSyncStatus, SyncOutcome, SyncStrategy, SyncTask,
    SyncTaskKind,
};
use crate::sync::{
    apply, evaluate_mirror_guard, plan, DestinationProviderFactory,
    LocalDestinationProviderFactory, MirrorDeletePreview, MirrorGuardVerdict, MoveRouting,
    RetryOptions, SyncOperation, SyncOperationError, SyncProgress,
};

/// How many operations may fail back-to-back before the destination is abandoned for this run.
///
/// Scattered failures — one locked file here, one permission problem there — are isolated and
/// the run carries on. An unbroken wall of them means the destination itself is gone, and
/// grinding through thousands of doomed operations helps nobody.
const MAX_CONSECUTIVE_FAILURES: u32 = 10;

/// Cooperative cancellation. Checked between destinations and before every operation.
#[derive(Debug, Clone, Default)]
pub struct CancellationToken(Arc<AtomicBool>);

impl CancellationToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// The run was stopped by the user.
///
/// Never a destination failure: work already applied stays applied, the in-flight file is
/// abandoned safely, and the caller reverts each destination to its previous status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the run was cancelled")]
pub struct Cancelled;

/// Runs sync tasks.
pub struct SyncEngine {
    file_system: Arc<dyn FileSystem>,
    destinations: Arc<dyn DestinationProviderFactory>,
    retry: RetryOptions,
}

impl SyncEngine {
    /// Full form: an explicit destination-provider factory, which is the extension seam for
    /// cloud and SFTP backends.
    pub fn new(
        file_system: Arc<dyn FileSystem>,
        destinations: Arc<dyn DestinationProviderFactory>,
        retry: RetryOptions,
    ) -> Self {
        Self {
            file_system,
            destinations,
            retry,
        }
    }

    /// Source and destinations on the same local filesystem — the phase-1 default.
    pub fn local(file_system: Arc<dyn FileSystem>) -> Self {
        let factory = Arc::new(LocalDestinationProviderFactory::new(Arc::clone(
            &file_system,
        )));
        Self::new(file_system, factory, RetryOptions::DEFAULT)
    }

    /// Executes `task` against every destination, in order, returning each one's outcome.
    ///
    /// A failure in one destination is captured as a failed status and does not stop the
    /// others. `confirmed_mass_deletes` carries the user's one-shot answers to a previous
    /// `NeedsConfirmation`; it is never persisted.
    pub fn execute(
        &self,
        task: &SyncTask,
        cancellation: &CancellationToken,
        progress: &mut dyn FnMut(SyncProgress),
        confirmed_mass_deletes: &HashSet<Uuid>,
    ) -> Result<Vec<DestinationSyncStatus>, Cancelled> {
        // Task shape: a task's kind decides which strategies its destinations may use. Move
        // and the copying strategies have contradictory postconditions — Move empties the
        // source the others treat as the truth — so a mixed task has no coherent semantics.
        // The whole run is refused rather than guessing which destination is the odd one out.
        if task.destinations.iter().any(|d| !task.accepts(d.strategy)) {
            let reason = match task.kind() {
                SyncTaskKind::Move => {
                    "A Move task takes Move destinations only; no files were changed."
                }
                SyncTaskKind::Sync => {
                    "A Sync task takes Mirror and Add-only destinations only; no files were changed."
                }
            };
            return Ok(refuse_run(task, reason));
        }

        // Task shape: destinations never overlap each other, inside a task as much as across
        // tasks. Two destinations writing the same tree race on the same files — a Mirror
        // destination deletes as orphans whatever a sibling just wrote.
        if let Some((first, second)) = find_overlapping_destinations(&task.destinations) {
            let reason = format!(
                "Destinations '{}' and '{}' are the same folder or nested in one another; \
                 no files were changed.",
                first.name, second.name
            );
            return Ok(refuse_run(task, &reason));
        }

        // One walk of the source serves every destination: files, stamps and directories
        // together, with no per-file stat calls. A failure here is *captured* rather than
        // thrown, so each destination's shape validation still runs and reports the clearer
        // problem first.
        let source_root = Path::new(&task.source_path);
        let (source_files, source_directories, source_error) =
            match self.file_system.list_tree(source_root) {
                Ok(listing) => (listing.files, listing.directories, None),
                Err(error) => (Vec::new(), Vec::new(), Some(error.to_string())),
            };

        // A Move task's destinations partition the source: a file only exists once, so it goes
        // to the first destination that matches it and the later ones never see it. Computed
        // once, before planning. Copying destinations stay independent.
        let routing = (task.kind() == SyncTaskKind::Move)
            .then(|| MoveRouting::route(&task.destinations, &source_files));

        let mut statuses = Vec::with_capacity(task.destinations.len());
        for (index, destination) in task.destinations.iter().enumerate() {
            if cancellation.is_cancelled() {
                return Err(Cancelled);
            }

            let routed = routing
                .as_ref()
                .map(|routing| routing.for_destination(index));
            statuses.push(self.execute_destination(
                task,
                destination,
                routed,
                &source_files,
                &source_directories,
                source_error.as_deref(),
                cancellation,
                progress,
                confirmed_mass_deletes,
            )?);
        }

        Ok(statuses)
    }

    /// What the mass-delete confirmation window shows: how many files a Mirror run would
    /// remove, and a sample of which.
    ///
    /// Advisory only. Anything that goes wrong — including the source having vanished since
    /// the run that was blocked — degrades to "nothing to show" rather than faulting the
    /// command the user just clicked; the follow-up run surfaces the real failure.
    pub fn preview_mirror_deletions(
        &self,
        task: &SyncTask,
        destination_id: Uuid,
    ) -> MirrorDeletePreview {
        let Some(destination) = task.destinations.iter().find(|d| d.id == destination_id) else {
            return MirrorDeletePreview::none();
        };

        // Config that would never run gets no preview.
        if destination.strategy != SyncStrategy::Mirror
            || !destination.has_only_the_all_files_filter()
            || io::paths_overlap(
                Path::new(destination.local_path()),
                Path::new(&task.source_path),
            )
        {
            return MirrorDeletePreview::none();
        }

        let Ok(provider) = self.destinations.create(&destination.target) else {
            return MirrorDeletePreview::none();
        };
        let source_root = Path::new(&task.source_path);
        let Ok(listing) = self.file_system.list_tree(source_root) else {
            return MirrorDeletePreview::none();
        };

        let filtered: Vec<ListedFile> = listing
            .files
            .iter()
            .filter(|file| destination.includes(&file.relative_path))
            .cloned()
            .collect();

        let Ok(plan) = plan(
            source_root,
            provider.as_ref(),
            destination,
            &filtered,
            &listing.directories,
        ) else {
            return MirrorDeletePreview::none();
        };

        let deletions: Vec<String> = plan.deletions().map(str::to_owned).collect();
        let sample = deletions
            .iter()
            .take(MirrorDeletePreview::SAMPLE_SIZE)
            .cloned()
            .collect();
        MirrorDeletePreview::new(deletions.len(), sample)
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_destination(
        &self,
        task: &SyncTask,
        destination: &Destination,
        routed_files: Option<&[ListedFile]>,
        source_files: &[ListedFile],
        source_directories: &[ListedDirectory],
        source_error: Option<&str>,
        cancellation: &CancellationToken,
        progress: &mut dyn FnMut(SyncProgress),
        confirmed_mass_deletes: &HashSet<Uuid>,
    ) -> Result<DestinationSyncStatus, Cancelled> {
        match self.run_destination(
            task,
            destination,
            routed_files,
            source_files,
            source_directories,
            source_error,
            cancellation,
            progress,
            confirmed_mass_deletes,
        ) {
            Ok(status) => Ok(status),
            Err(RunError::Cancelled) => Err(Cancelled),
            Err(RunError::Refused(reason)) => Ok(failed(destination.id, &reason)),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn run_destination(
        &self,
        task: &SyncTask,
        destination: &Destination,
        routed_files: Option<&[ListedFile]>,
        source_files: &[ListedFile],
        source_directories: &[ListedDirectory],
        source_error: Option<&str>,
        cancellation: &CancellationToken,
        progress: &mut dyn FnMut(SyncProgress),
        confirmed_mass_deletes: &HashSet<Uuid>,
    ) -> Result<DestinationSyncStatus, RunError> {
        // Task shape: source and destinations never nest, in either direction, for every
        // strategy. A destination inside the source feeds the app's own output back in as
        // input; a source inside a destination makes Mirror's orphan scan delete the live
        // source. Reject the layout — do not engineer around it.
        if io::paths_overlap(
            Path::new(destination.local_path()),
            Path::new(&task.source_path),
        ) {
            return Err(RunError::Refused(
                "Destination must be a separate folder outside the source (and not contain it); \
                 no files were changed."
                    .into(),
            ));
        }

        // Product rule: Mirror's contract is tree identity, so file filters have no coherent
        // meaning for it. The editor hides the filter section; hand-edited config is refused
        // here, before any file is touched.
        if destination.strategy == SyncStrategy::Mirror
            && !destination.has_only_the_all_files_filter()
        {
            return Err(RunError::Refused(
                "Mirror replicates the whole source tree and cannot be combined with file \
                 filters; no files were changed."
                    .into(),
            ));
        }

        if let Some(error) = source_error {
            return Err(RunError::Refused(error.to_owned()));
        }

        let filtered: Vec<ListedFile> = match routed_files {
            Some(routed) => routed.to_vec(),
            None => source_files
                .iter()
                .filter(|file| destination.includes(&file.relative_path))
                .cloned()
                .collect(),
        };

        let provider = self
            .destinations
            .create(&destination.target)
            .map_err(|error| RunError::Refused(error.to_string()))?;

        let source_root = Path::new(&task.source_path);
        let plan = plan(
            source_root,
            provider.as_ref(),
            destination,
            &filtered,
            source_directories,
        )
        .map_err(|error| RunError::Refused(error.to_string()))?;

        // Guard Mirror deletions before applying anything.
        let delete_count = plan.delete_count();
        if delete_count > 0 {
            let verdict = evaluate_mirror_guard(
                delete_count,
                plan.destination_file_count,
                filtered.is_empty(),
                destination.mass_delete_threshold,
                confirmed_mass_deletes.contains(&destination.id),
            );

            match verdict {
                MirrorGuardVerdict::EmptySource => {
                    return Err(RunError::Refused(
                        "Source is empty or unavailable; skipped deletions to avoid wiping the \
                         destination."
                            .into(),
                    ))
                }
                MirrorGuardVerdict::NeedsConfirmation => {
                    let mut status =
                        DestinationSyncStatus::new(destination.id, SyncOutcome::NeedsConfirmation);
                    status.last_run = Some(Utc::now().into());
                    status.error = Some(format!(
                        "Would delete {delete_count} files no longer in the source — review \
                         before syncing."
                    ));
                    // Zero operations applied.
                    return Ok(status);
                }
                MirrorGuardVerdict::Allowed => {}
            }
        }

        let mut copied: Vec<String> = Vec::new();
        let mut deferred: Vec<String> = Vec::new();
        let mut moved_sources: Vec<PathBuf> = Vec::new();
        let mut first_failure: Option<String> = None;
        let mut failure_count = 0usize;
        let mut consecutive_failures = 0u32;
        let mut abandoned = false;

        let total = plan.operations.len();
        for (index, operation) in plan.operations.iter().enumerate() {
            if cancellation.is_cancelled() {
                return Err(RunError::Cancelled);
            }

            progress(SyncProgress {
                destination_id: destination.id,
                operation: operation.clone(),
                completed_operations: index,
                total_operations: total,
            });

            let outcome = crate::sync::retry(self.retry, || {
                apply(&self.file_system, provider.as_ref(), operation)
            });

            if let Err(error) = outcome {
                // A file still being written, or one another process holds open, is not a
                // failure — and nothing reached the destination, so the tree stays consistent.
                // Defer it; the next run picks it up once the writer is done.
                if error.is_busy() {
                    deferred.push(operation.relative_path().to_owned());
                    consecutive_failures = 0;
                    continue;
                }

                failure_count += 1;
                if first_failure.is_none() {
                    first_failure = Some(
                        SyncOperationError::new(operation.clone(), error)
                            .message()
                            .to_owned(),
                    );
                }

                consecutive_failures += 1;
                if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                    abandoned = true;
                    break;
                }
                continue;
            }

            consecutive_failures = 0;
            if operation.is_transfer() {
                copied.push(operation.relative_path().to_owned());
            }
            if let SyncOperation::Move {
                source_full_path, ..
            } = operation
            {
                // The *source* path: a flattening destination gives the two ends different
                // shapes, and the folder to clean up afterwards is the source's.
                moved_sources.push(source_full_path.clone());
            }
        }

        // A flattening destination that refused a duplicate name left those files in the
        // source. Nothing resolves that on its own, so the run says so rather than reporting a
        // clean sweep that quietly kept working around them.
        if let Some(first) = plan.skipped_collisions.first() {
            failure_count += plan.skipped_collisions.len();
            first_failure.get_or_insert_with(|| {
                format!("Skipped '{first}': a file of that name is already there")
            });
        }

        // Move takes files out of the source, so the folders that held them are left behind
        // empty — and a watched inbox would collect them forever. Only the folders this run
        // emptied are removed: one that was already empty holds no moved file, so it is not an
        // ancestor of one and is never considered.
        if !moved_sources.is_empty() {
            if let Some(failure) = self.remove_emptied_source_directories(
                source_root,
                &moved_sources,
                destination.delete_mode,
            ) {
                failure_count += 1;
                first_failure.get_or_insert(failure);
            }
        }

        // Severity ladder: a real failure outranks a merely deferred file, which outranks a
        // clean run. The copied count stays honest either way — files that did make it across
        // are reported even when something else failed.
        let outcome = if first_failure.is_some() {
            SyncOutcome::Failed
        } else if !deferred.is_empty() {
            SyncOutcome::Incomplete
        } else {
            SyncOutcome::Success
        };

        Ok(DestinationSyncStatus {
            destination_id: destination.id,
            outcome,
            last_run: Some(Utc::now().into()),
            files_copied: copied.len() as i32,
            error: first_failure.map(|first| describe_failures(&first, failure_count, abandoned)),
            files_deferred: deferred.len() as i32,
            copied_relative_paths: copied,
            deferred_relative_paths: deferred,
        })
    }

    /// Removes the source folders this run's moves emptied, deepest first so a parent is only
    /// considered once its children are gone.
    ///
    /// Each removal is non-recursive and conditional on the folder actually being empty *now*,
    /// so one that gained content since the move is kept. Returns the first unexpected failure.
    fn remove_emptied_source_directories(
        &self,
        source_root: &Path,
        moved_source_paths: &[PathBuf],
        mode: DeleteMode,
    ) -> Option<String> {
        // Folded key → the real path, so iteration is deepest-first and case-insensitively
        // deduplicated.
        let mut directories: BTreeMap<String, PathBuf> = BTreeMap::new();
        for path in moved_source_paths {
            let mut current = path.parent();
            while let Some(directory) = current {
                // Strictly below the root: the source folder itself is never removed.
                if directory == source_root || !directory.starts_with(source_root) {
                    break;
                }
                directories.insert(
                    directory.to_string_lossy().to_uppercase(),
                    directory.to_path_buf(),
                );
                current = directory.parent();
            }
        }

        let mut failure = None;
        for directory in directories.values().rev() {
            let removed = match mode {
                DeleteMode::Recycle => self.file_system.recycle_empty_directory(directory),
                DeleteMode::Permanent => self.file_system.delete_empty_directory(directory),
            };

            // Both removals already tolerate "not empty", "in use" and "already gone";
            // anything left is a real problem — a permission wall, a read-only volume — that
            // will not fix itself, so it is reported rather than swallowed.
            if let Err(error) = removed {
                failure.get_or_insert_with(|| {
                    format!(
                        "Failed to remove the emptied source folder '{}': {error}",
                        directory.display()
                    )
                });
            }
        }

        failure
    }
}

/// Cancellation, or a reason the destination could not run at all.
enum RunError {
    Cancelled,
    Refused(String),
}

/// The whole task is refused: every destination reports the same reason, so the card explains
/// itself wherever the user looks.
fn refuse_run(task: &SyncTask, reason: &str) -> Vec<DestinationSyncStatus> {
    task.destinations
        .iter()
        .map(|destination| failed(destination.id, reason))
        .collect()
}

fn failed(destination_id: Uuid, reason: &str) -> DestinationSyncStatus {
    let mut status = DestinationSyncStatus::new(destination_id, SyncOutcome::Failed);
    status.last_run = Some(Utc::now().into());
    status.error = Some(reason.to_owned());
    status
}

fn find_overlapping_destinations(
    destinations: &[Destination],
) -> Option<(&Destination, &Destination)> {
    for (index, first) in destinations.iter().enumerate() {
        for second in &destinations[index + 1..] {
            if io::paths_overlap(
                Path::new(first.local_path()),
                Path::new(second.local_path()),
            ) {
                return Some((first, second));
            }
        }
    }
    None
}

/// The status line shows one sentence: name the first culprit, count the rest, and say so when
/// the run gave up early.
fn describe_failures(first: &str, failure_count: usize, abandoned: bool) -> String {
    let message = if failure_count <= 1 {
        first.to_owned()
    } else {
        format!("{first} (and {} more)", failure_count - 1)
    };

    if abandoned {
        format!("{message}; stopped after {MAX_CONSECUTIVE_FAILURES} consecutive failures.")
    } else {
        message
    }
}
