//! Turns a desired sync into a list of operations, **without mutating anything**.
//!
//! Keeping planning pure is what makes the engine dry-runnable and exhaustively testable: the
//! same inputs always yield the same plan, and nothing touches disk until the applier runs.
//! The planner only reads, and only the destination — the source arrives pre-listed, stamps
//! included, from the engine's single tree walk.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use chrono::{DateTime, Utc};

use crate::io::{self, FileStamp, ListedDirectory, ListedFile, TreeListing};
use crate::model::{Destination, FileNameCollisionPolicy, SyncStrategy};
use crate::sync::{DestinationProvider, SyncOperation, SyncPlan};

/// Plans the operations that reconcile the destination with the filtered source set.
///
/// `source_directories` is every directory under the source root. Only Mirror uses it: it
/// replicates the source directory tree exactly, structure and times, because file filters
/// select files, not structure.
pub fn plan(
    source_root: &Path,
    destination_provider: &dyn DestinationProvider,
    destination: &Destination,
    filtered_files: &[ListedFile],
    source_directories: &[ListedDirectory],
) -> std::io::Result<SyncPlan> {
    match destination.strategy {
        SyncStrategy::Move => {
            let mut skipped = Vec::new();
            let operations = plan_move(
                source_root,
                destination_provider,
                destination,
                filtered_files,
                &mut skipped,
            )?;
            let mut plan = SyncPlan::new(operations, 0);
            plan.skipped_collisions = skipped;
            Ok(plan)
        }
        SyncStrategy::AddOnly => {
            // No destination walk at all: Add-only never deletes, so it only needs to know
            // about the files it is considering copying.
            let mut operations = Vec::new();
            for file in filtered_files {
                let destination_stamp = destination_provider.try_get_stamp(&file.relative_path)?;
                if needs_copy(file.stamp, destination_stamp) {
                    operations.push(copy_operation(source_root, destination, file));
                }
            }
            Ok(SyncPlan::new(operations, 0))
        }
        SyncStrategy::Mirror => {
            let snapshot = DestinationSnapshot::create(destination_provider)?;
            let file_count = snapshot.file_count();
            let operations = plan_mirror(
                source_root,
                destination,
                filtered_files,
                source_directories,
                &snapshot,
            );
            Ok(SyncPlan::new(operations, file_count))
        }
    }
}

/// A copy is needed when the destination is missing the file, or when the two stamps differ in
/// size or whole-second modified time. Both stamps come from their side's listing — planning
/// never stats a file, and never hashes one.
fn needs_copy(source_stamp: FileStamp, destination_stamp: Option<FileStamp>) -> bool {
    destination_stamp != Some(source_stamp)
}

fn copy_operation(
    source_root: &Path,
    destination: &Destination,
    file: &ListedFile,
) -> SyncOperation {
    SyncOperation::Copy {
        relative_path: file.relative_path.clone(),
        source_full_path: io::join(source_root, &file.relative_path),
        expected_stamp: file.stamp,
        verify: destination.verify_contents,
    }
}

/// Mirror: Add-only's copies, plus a delete for every destination file the filtered source no
/// longer has, plus reconciliation of the directory tree itself — so a file-tree compare of
/// source and destination reports identical, empty directories included.
///
/// The order below is load-bearing and asserted by tests: creates (parents first) → copies →
/// file deletes → directory deletes (deepest first, after the deletes that empty them) →
/// directory timestamps (last of all).
fn plan_mirror(
    source_root: &Path,
    destination: &Destination,
    filtered_files: &[ListedFile],
    source_directories: &[ListedDirectory],
    snapshot: &DestinationSnapshot,
) -> Vec<SyncOperation> {
    // Sorted by folded key, so iteration is the ordinal-ignore-case order the C# planner used
    // and parents always come before their children.
    let mut source_directory_times: BTreeMap<String, (String, DateTime<Utc>)> = BTreeMap::new();
    for directory in source_directories {
        source_directory_times.insert(
            fold(&directory.relative_path),
            (
                directory.relative_path.clone(),
                directory.last_write_time_utc,
            ),
        );
    }

    // Ancestors of the filtered files are source directories by definition; folding them in
    // guards against a listing that raced a concurrent change. They carry no listed time, so
    // they get no timestamp operation this run.
    let mut source_directory_names: BTreeMap<String, String> = source_directory_times
        .iter()
        .map(|(key, (name, _))| (key.clone(), name.clone()))
        .collect();
    for file in filtered_files {
        for ancestor in ancestor_directories(&file.relative_path) {
            source_directory_names
                .entry(fold(ancestor))
                .or_insert_with(|| ancestor.to_owned());
        }
    }

    let copies: Vec<SyncOperation> = filtered_files
        .iter()
        .filter(|file| needs_copy(file.stamp, snapshot.stamp_of(&file.relative_path)))
        .map(|file| copy_operation(source_root, destination, file))
        .collect();

    // Copies create their parents as a side effect, so explicit creates are only planned for
    // directories no copy will touch — typically the empty ones.
    let created_by_copies: HashSet<String> = copies
        .iter()
        .flat_map(|copy| ancestor_directories(copy.relative_path()).map(fold))
        .collect();

    let mut operations = Vec::new();

    for (key, name) in &source_directory_names {
        if !snapshot.has_directory(key) && !created_by_copies.contains(key) {
            operations.push(SyncOperation::CreateDirectory {
                relative_path: name.clone(),
            });
        }
    }

    operations.extend(copies);

    let keep: HashSet<String> = filtered_files
        .iter()
        .map(|file| fold(&file.relative_path))
        .collect();
    for destination_relative in &snapshot.relative_paths {
        if !keep.contains(&fold(destination_relative)) {
            operations.push(SyncOperation::Delete {
                relative_path: destination_relative.clone(),
                mode: destination.delete_mode,
            });
        }
    }

    // Destination directories the source no longer has, deepest first so children go before
    // the parents that hold them — and after the file deletions that empty them. The listings
    // exclude the roots, so a root is never planned for removal.
    for (key, name) in snapshot.directories.iter().rev() {
        if !source_directory_names.contains_key(key) {
            operations.push(SyncOperation::DeleteDirectory {
                relative_path: name.clone(),
            });
        }
    }

    // Directory times go last of all: the operations above bump the times of the directories
    // they touch, and NTFS does not bump a parent when a child's own timestamps change, so one
    // trailing pass converges within the run.
    let timestamps = plan_directory_timestamps(&operations, &source_directory_times, snapshot);
    operations.extend(timestamps);

    operations
}

/// A destination directory needs its time (re)set when it is about to be created (its time
/// would read "now"), its current time differs from the source's, or one of this plan's
/// operations changes an entry inside it.
fn plan_directory_timestamps(
    planned: &[SyncOperation],
    source_directory_times: &BTreeMap<String, (String, DateTime<Utc>)>,
    snapshot: &DestinationSnapshot,
) -> Vec<SyncOperation> {
    let bumped: HashSet<String> = planned
        .iter()
        .filter_map(|operation| parent_directory(operation.relative_path()))
        .map(fold)
        .collect();

    source_directory_times
        .iter()
        .filter(|(key, (_, time))| {
            let missing = !snapshot.has_directory(key);
            let mismatched = snapshot
                .directory_times
                .get(*key)
                .is_some_and(|current| current != time);
            missing || mismatched || bumped.contains(*key)
        })
        .map(|(_, (name, time))| SyncOperation::SetDirectoryTimestamp {
            relative_path: name.clone(),
            last_write_time_utc: *time,
        })
        .collect()
}

/// Move: move each filtered source file to the destination.
///
/// Keeping the structure maps each file to its own relative path. Flattening maps every file
/// into the root, where names can collide — with a file already there or with another file in
/// this same plan — so the destination's policy decides each one.
fn plan_move(
    source_root: &Path,
    destination_provider: &dyn DestinationProvider,
    destination: &Destination,
    filtered_files: &[ListedFile],
    skipped: &mut Vec<String>,
) -> std::io::Result<Vec<SyncOperation>> {
    let mut operations = Vec::new();
    let mut claimed: HashSet<String> = HashSet::new();

    for file in filtered_files {
        let mut target = file.relative_path.clone();

        if destination.flatten_structure {
            let leaf = leaf_name(&file.relative_path);
            match resolve_collision(destination_provider, destination, &claimed, leaf)? {
                Some(free) => {
                    claimed.insert(fold(&free));
                    target = free;
                }
                None => {
                    // Nothing is renamed and nothing is overwritten: the file stays where it
                    // is and the run reports it.
                    skipped.push(file.relative_path.clone());
                    continue;
                }
            }
        }

        operations.push(SyncOperation::Move {
            relative_path: target,
            source_full_path: io::join(source_root, &file.relative_path),
            expected_stamp: file.stamp,
            verify: destination.verify_contents,
        });
    }

    Ok(operations)
}

/// A free destination name for `target`, or `None` when the policy is to skip and the name is
/// taken. "Taken" covers both what is already at the destination and what an earlier file in
/// this same plan claimed.
fn resolve_collision(
    destination_provider: &dyn DestinationProvider,
    destination: &Destination,
    claimed: &HashSet<String>,
    target: &str,
) -> std::io::Result<Option<String>> {
    if !is_taken(destination_provider, claimed, target)? {
        return Ok(Some(target.to_owned()));
    }

    if destination.collision_policy == FileNameCollisionPolicy::Skip {
        return Ok(None);
    }

    // "report.pdf" → "report (2).pdf", "report (3).pdf", … — the shape Explorer and browsers
    // already use, so a numbered file reads as a duplicate at a glance.
    let (stem, extension) = split_extension(target);
    for suffix in 2u32..u32::MAX {
        let candidate = format!("{stem} ({suffix}){extension}");
        if !is_taken(destination_provider, claimed, &candidate)? {
            return Ok(Some(candidate));
        }
    }

    Ok(None)
}

fn is_taken(
    destination_provider: &dyn DestinationProvider,
    claimed: &HashSet<String>,
    relative_path: &str,
) -> std::io::Result<bool> {
    if claimed.contains(&fold(relative_path)) {
        return Ok(true);
    }
    Ok(destination_provider.try_get_stamp(relative_path)?.is_some())
}

/// `"a/b/c.txt"` → `"c.txt"`; a file already at the root is its own leaf.
fn leaf_name(relative_path: &str) -> &str {
    match relative_path.rfind('/') {
        Some(separator) => &relative_path[separator + 1..],
        None => relative_path,
    }
}

/// `"report.pdf"` → `("report", ".pdf")`; a name with no dot keeps an empty extension.
fn split_extension(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(dot) => (&name[..dot], &name[dot..]),
        None => (name, ""),
    }
}

/// `"a/b/c"` → `Some("a/b")`. A path directly under the root has no parent to bump — the
/// destination root itself never gets operations.
fn parent_directory(relative_path: &str) -> Option<&str> {
    relative_path
        .rfind('/')
        .map(|separator| &relative_path[..separator])
}

/// `"a/b/c.txt"` yields `"a"` then `"a/b"` — every directory between the root (exclusive) and
/// the file.
fn ancestor_directories(relative_path: &str) -> impl Iterator<Item = &str> {
    relative_path
        .match_indices('/')
        .map(move |(index, _)| &relative_path[..index])
}

/// The case-insensitive key relative paths are compared and ordered by. Windows filesystems on
/// both ends, so `Photos/a.jpg` and `photos/A.JPG` are the same file.
fn fold(relative_path: &str) -> String {
    relative_path.to_uppercase()
}

/// One stable view of the destination for existence, stamp, delete, and guard decisions.
struct DestinationSnapshot {
    /// In listing order, so deletions come out in the order the destination presented them.
    relative_paths: Vec<String>,
    stamps: HashMap<String, FileStamp>,
    /// Folded key → original name, sorted so the deepest-first iteration is just `.rev()`.
    directories: BTreeMap<String, String>,
    directory_times: HashMap<String, DateTime<Utc>>,
}

impl DestinationSnapshot {
    /// One walk yields files, stamps, and directories together — there is no per-file stamping
    /// phase, and so no window between listing and stamping in which a file can vanish.
    fn create(provider: &dyn DestinationProvider) -> std::io::Result<Self> {
        let TreeListing { files, directories } = provider.list_tree()?;

        let mut relative_paths = Vec::with_capacity(files.len());
        let mut stamps = HashMap::with_capacity(files.len());
        for file in files {
            stamps.insert(fold(&file.relative_path), file.stamp);
            relative_paths.push(file.relative_path);
        }

        let mut folded_directories = BTreeMap::new();
        let mut directory_times = HashMap::with_capacity(directories.len());
        for directory in directories {
            let key = fold(&directory.relative_path);
            directory_times.insert(key.clone(), directory.last_write_time_utc);
            folded_directories.insert(key, directory.relative_path);
        }

        Ok(Self {
            relative_paths,
            stamps,
            directories: folded_directories,
            directory_times,
        })
    }

    fn file_count(&self) -> usize {
        self.relative_paths.len()
    }

    fn stamp_of(&self, relative_path: &str) -> Option<FileStamp> {
        self.stamps.get(&fold(relative_path)).copied()
    }

    fn has_directory(&self, folded: &str) -> bool {
        self.directories.contains_key(folded)
    }
}
