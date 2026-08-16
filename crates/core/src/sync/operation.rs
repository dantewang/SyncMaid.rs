use std::path::PathBuf;

use chrono::{DateTime, Utc};

use crate::io::FileStamp;
use crate::model::DeleteMode;

/// One thing to do to the destination.
///
/// Operations address the destination **only** by relative path; resolving that to a real
/// location is the provider's job, which is what lets a cloud or SFTP backend drop in without
/// the planner knowing. `source_full_path` is the one absolute path here, and it points at the
/// source, which is always local.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncOperation {
    /// Copy a source file in, leaving the source alone.
    Copy {
        relative_path: String,
        source_full_path: PathBuf,
        /// What the source looked like when the plan was made. A mismatch at apply time means
        /// something is still writing it.
        expected_stamp: FileStamp,
        /// Read the copy back and compare hashes before committing.
        verify: bool,
    },
    /// Create a directory the source has and the destination does not — including empty ones,
    /// because Mirror's contract is tree identity.
    CreateDirectory { relative_path: String },
    /// Remove a destination file the source no longer has.
    Delete {
        relative_path: String,
        mode: DeleteMode,
    },
    /// Remove a destination directory the source no longer has. Non-recursive: the file
    /// deletes that empty it are planned before it.
    DeleteDirectory { relative_path: String },
    /// Give a mirrored directory its source's modified time.
    SetDirectoryTimestamp {
        relative_path: String,
        last_write_time_utc: DateTime<Utc>,
    },
    /// Move a source file in: copy, verify, and only then delete the source.
    Move {
        relative_path: String,
        source_full_path: PathBuf,
        expected_stamp: FileStamp,
        verify: bool,
    },
}

impl SyncOperation {
    /// Where in the destination this operation lands.
    pub fn relative_path(&self) -> &str {
        match self {
            Self::Copy { relative_path, .. }
            | Self::CreateDirectory { relative_path }
            | Self::Delete { relative_path, .. }
            | Self::DeleteDirectory { relative_path }
            | Self::SetDirectoryTimestamp { relative_path, .. }
            | Self::Move { relative_path, .. } => relative_path,
        }
    }

    /// The path + verb prefix a failure is reported under.
    pub fn describe_failure(&self) -> String {
        let path = self.relative_path();
        match self {
            Self::Copy { .. } => format!("Failed to copy '{path}'"),
            Self::Move { .. } => format!("Failed to move '{path}'"),
            Self::Delete { .. } | Self::DeleteDirectory { .. } => {
                format!("Failed to delete '{path}'")
            }
            Self::CreateDirectory { .. } | Self::SetDirectoryTimestamp { .. } => {
                format!("Failed on '{path}'")
            }
        }
    }

    /// True for the two operations that put bytes at the destination.
    pub fn is_transfer(&self) -> bool {
        matches!(self, Self::Copy { .. } | Self::Move { .. })
    }
}

/// What a run intends to do to one destination.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncPlan {
    /// In apply order. For Mirror the order is load-bearing — see the planner.
    pub operations: Vec<SyncOperation>,
    /// How many files the destination held when the plan was made. Mirror's mass-delete guard
    /// compares against this; the other strategies never delete, so they report 0.
    pub destination_file_count: usize,
    /// Files a flattening Move left in the source because their name was taken and the policy
    /// is to skip. Reported as failures so the user hears about them.
    pub skipped_collisions: Vec<String>,
}

impl SyncPlan {
    pub fn new(operations: Vec<SyncOperation>, destination_file_count: usize) -> Self {
        Self {
            operations,
            destination_file_count,
            skipped_collisions: Vec::new(),
        }
    }

    /// How many files this plan would delete — the number the mirror guard weighs.
    pub fn delete_count(&self) -> usize {
        self.operations
            .iter()
            .filter(|op| matches!(op, SyncOperation::Delete { .. }))
            .count()
    }

    /// The relative paths this plan would delete, in plan order.
    pub fn deletions(&self) -> impl Iterator<Item = &str> {
        self.operations.iter().filter_map(|op| match op {
            SyncOperation::Delete { relative_path, .. } => Some(relative_path.as_str()),
            _ => None,
        })
    }
}

/// Progress through a destination's plan, reported before each operation.
#[derive(Debug, Clone)]
pub struct SyncProgress {
    /// Which destination this is about.
    pub destination_id: uuid::Uuid,
    /// The operation about to run.
    pub operation: SyncOperation,
    /// How many operations have already finished.
    pub completed_operations: usize,
    /// How many there are in total.
    pub total_operations: usize,
}
