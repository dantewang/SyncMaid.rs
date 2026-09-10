use std::io::{self, Read, Write};
use std::path::Path;

use chrono::{DateTime, Utc};

use crate::io::{FileStamp, TreeListing};

/// The filesystem operations the sync engine needs, abstracted so the engine can run against a
/// real disk ([`super::PhysicalFileSystem`]) or an in-memory fake in tests.
///
/// All paths are absolute unless noted; relative paths are always relative to a given root and
/// use forward slashes.
pub trait FileSystem: Send + Sync {
    /// Walks the tree under `root` once, recursively, returning every file with its stamp and
    /// every directory with its modified time.
    ///
    /// A root that does not exist **must** fail rather than read as empty: an unplugged source
    /// drive has to fail the run, not look like an empty source that Mirror would then
    /// reconcile by deleting the entire destination.
    fn list_tree(&self, root: &Path) -> io::Result<TreeListing>;

    /// True when a file exists at `path`.
    fn file_exists(&self, path: &Path) -> bool;

    /// True when a directory exists at `path`.
    ///
    /// Cheap on purpose. The alternative — asking [`FileSystem::list_tree`] whether it errors —
    /// answers the same question by walking the entire tree, which is a fine cost once per run
    /// and a ruinous one anywhere it might be asked repeatedly.
    fn directory_exists(&self, path: &Path) -> bool;

    /// The stamp of the file at `path`. Fails if it does not exist.
    fn get_stamp(&self, path: &Path) -> io::Result<FileStamp>;

    /// The stamp of the file at `path`, or `None` when it is not there.
    ///
    /// Add-only planning asks this per candidate file, so a missing file must not cost an
    /// error construction.
    fn try_get_stamp(&self, path: &Path) -> io::Result<Option<FileStamp>> {
        match self.get_stamp(path) {
            Ok(stamp) => Ok(Some(stamp)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Reads the full contents of the file at `path`.
    fn read_all_bytes(&self, path: &Path) -> io::Result<Vec<u8>>;

    /// Writes `contents` to `path`, overwriting any existing file and creating parents.
    fn write_all_bytes(&self, path: &Path, contents: &[u8]) -> io::Result<()>;

    /// Deletes the file at `path` if it exists; otherwise a no-op.
    fn delete_file(&self, path: &Path) -> io::Result<()>;

    /// Sends the file at `path` to the Recycle Bin if it exists, so the deletion is
    /// recoverable. On volumes without a Recycle Bin (network shares) this falls back to a
    /// permanent delete.
    fn recycle(&self, path: &Path) -> io::Result<()>;

    /// Ensures the directory at `path` exists, creating parents as needed.
    fn ensure_directory(&self, path: &Path) -> io::Result<()>;

    /// Deletes the directory at `path` only if it is empty. Never recursive; a directory with
    /// content or none at all is left alone without error.
    fn delete_empty_directory(&self, path: &Path) -> io::Result<()>;

    /// Like [`FileSystem::delete_empty_directory`], but to the Recycle Bin — and still only if
    /// it is empty, so content that appeared since the caller decided is never taken along.
    fn recycle_empty_directory(&self, path: &Path) -> io::Result<()>;

    /// Sets the last-write time of the directory at `path`, so a mirrored directory can share
    /// its source's modified time. A directory that does not exist is left alone without error
    /// — the next run replans.
    fn set_directory_last_write_time_utc(
        &self,
        path: &Path,
        last_write_time_utc: DateTime<Utc>,
    ) -> io::Result<()>;

    /// Opens the file at `path` for reading.
    fn open_read(&self, path: &Path) -> io::Result<Box<dyn Read + Send>>;

    /// Creates (or overwrites) the file at `path` for writing, creating parents as needed,
    /// with write-through semantics so bytes reach the storage device rather than sitting in a
    /// write cache. That is what makes "the copy is complete or it never happened" true across
    /// a power cut, not just across a crash.
    fn create_write_through(&self, path: &Path) -> io::Result<Box<dyn Write + Send>>;

    /// Sets the last-write time of the file at `path`, so a copy shares its source's stamp and
    /// the next run sees no change.
    fn set_last_write_time_utc(
        &self,
        path: &Path,
        last_write_time_utc: DateTime<Utc>,
    ) -> io::Result<()>;

    /// Atomically replaces `destination_path` with the file at `source_path` — an on-volume
    /// rename/overwrite — creating the destination's parents as needed.
    ///
    /// This is the commit step of every write. After it returns the source no longer exists
    /// and the destination holds the source's bytes; before it returns, the destination still
    /// holds its previous complete contents.
    fn replace(&self, source_path: &Path, destination_path: &Path) -> io::Result<()>;

    /// Bytes of free space available on the volume that would hold `path`, as a preflight
    /// before a copy. [`u64::MAX`] when the amount cannot be determined — an unknown is not a
    /// reason to refuse a copy.
    fn available_free_space(&self, path: &Path) -> u64;
}
