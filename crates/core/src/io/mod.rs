//! The filesystem seam the engine talks through, plus the value types a tree walk produces.
//!
//! Everything the engine does to files goes through [`FileSystem`], so the whole of its safety
//! behaviour can be fault-injected against [`InMemoryFileSystem`] in tests. All absolute paths
//! are [`Path`](std::path::Path); all relative paths are forward-slash `str`, relative to a
//! given root, with the root itself excluded.

mod file_busy;
mod file_system;
mod network_path;
mod physical;
mod relative_paths;

#[cfg(any(test, feature = "testing"))]
mod memory;

pub use file_busy::is_busy;
pub use file_system::FileSystem;
pub use network_path::is_network;
pub use physical::PhysicalFileSystem;
pub use relative_paths::{join, overlaps, paths_overlap};

#[cfg(any(test, feature = "testing"))]
pub use memory::{Faults, InMemoryFileSystem};

use chrono::{DateTime, Timelike, Utc};

/// A cheap, deterministic fingerprint used to decide whether a source and destination copy
/// differ.
///
/// Size and last-write-time, not a hash: change detection runs over every file on every run,
/// and hashing every file every scan is far too expensive. Hashing is for *copy verification*
/// — a different job, done once per copied file. See [`crate::sync::SafeFileTransfer`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileStamp {
    /// File length in bytes.
    pub length: u64,
    /// Last write time in UTC, truncated to whole seconds.
    pub last_write_time_utc: DateTime<Utc>,
}

impl FileStamp {
    /// Builds a stamp, normalizing the timestamp so filesystems with differing sub-second
    /// precision still compare equal.
    pub fn new(length: u64, last_write_time_utc: DateTime<Utc>) -> Self {
        Self {
            length,
            last_write_time_utc: normalize_utc(last_write_time_utc),
        }
    }
}

/// The stamp's timestamp normalization on its own — UTC, truncated to whole seconds — for
/// timestamps compared outside a full stamp (directory modified times).
///
/// Both sides of every comparison must go through this. Skip it on one side and every run
/// re-copies the whole tree.
pub fn normalize_utc(timestamp: DateTime<Utc>) -> DateTime<Utc> {
    timestamp.with_nanosecond(0).unwrap_or(timestamp)
}

/// A file seen by one tree walk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedFile {
    /// Root-relative, forward slashes.
    pub relative_path: String,
    pub stamp: FileStamp,
}

/// A directory seen by one tree walk.
///
/// Mirror preserves directory modified times the way copies preserve file times, so the walk
/// carries them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedDirectory {
    /// Root-relative, forward slashes.
    pub relative_path: String,
    pub last_write_time_utc: DateTime<Utc>,
}

/// The result of walking a tree once.
///
/// Produced in a single enumeration pass — stamps and times come from the walk's own directory
/// metadata, so callers never pay a second per-entry round trip for data the listing already
/// carried (one round trip per directory instead of per file, which is what makes network
/// paths bearable).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TreeListing {
    pub files: Vec<ListedFile>,
    pub directories: Vec<ListedDirectory>,
}

impl TreeListing {
    /// A tree with no files and no directories — e.g. a destination that does not exist yet.
    pub fn empty() -> Self {
        Self::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn a_stamp_truncates_sub_second_precision_on_both_sides() {
        let precise = Utc.with_ymd_and_hms(2026, 8, 9, 2, 0, 12).unwrap()
            + chrono::Duration::nanoseconds(418_000_000);
        let coarse = Utc.with_ymd_and_hms(2026, 8, 9, 2, 0, 12).unwrap();

        assert_eq!(FileStamp::new(10, precise), FileStamp::new(10, coarse));
        assert_eq!(coarse, normalize_utc(precise));
    }

    #[test]
    fn a_stamp_differs_on_length_alone() {
        let when = Utc.with_ymd_and_hms(2026, 8, 9, 2, 0, 12).unwrap();
        assert_ne!(FileStamp::new(10, when), FileStamp::new(11, when));
    }
}
