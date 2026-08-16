//! The seam between the engine and wherever a destination actually lives.
//!
//! The engine never touches a destination path directly. It hands a provider relative paths
//! and a [`SourceFile`], and the provider owns both its commit and its verification — because
//! "atomic" and "verified" mean different things on a local volume, on S3, and over SFTP, and
//! only the backend knows which. Phase 1 ships the local/mounted provider; the contract is
//! shaped so cloud and SFTP can drop in without the planner learning about them.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, Utc};

use crate::io::{self, FileStamp, FileSystem, TreeListing};
use crate::model::{DeleteMode, DestinationLocation};
use crate::sync::{safe_transfer, OperationError};

/// What a destination can and cannot do, as a runtime fact rather than a type.
///
/// The same local provider is a fast local disk on one machine and a network share on another,
/// which is why network-ness is asked at run time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DestinationCapabilities {
    /// Reads and writes cross a network.
    pub is_remote: bool,
    /// Deletions can go somewhere recoverable.
    pub supports_recycle: bool,
}

/// One source file handed to a provider.
///
/// `local_path` is nullable on purpose: a provider that can copy path-to-path uses the OS fast
/// path, and one that cannot streams instead.
pub trait SourceFile {
    fn relative_path(&self) -> &str;
    /// The file's absolute path when it is local, which it always is in phase 1.
    fn local_path(&self) -> Option<&Path>;
    fn length(&self) -> u64;
    fn stamp(&self) -> FileStamp;
    fn open_read(&self) -> std::io::Result<Box<dyn Read + Send>>;
}

/// Where a destination's files go.
pub trait DestinationProvider: Send {
    fn capabilities(&self) -> DestinationCapabilities;

    /// Walks the destination once. A destination that does not exist yet is **empty**, not an
    /// error — unlike the source, which must fail so an unplugged drive cannot look empty.
    fn list_tree(&self) -> std::io::Result<TreeListing>;

    fn get_stamp(&self, relative_path: &str) -> std::io::Result<FileStamp>;

    /// The stamp, or `None` when nothing is there. Add-only planning calls this per candidate,
    /// so a miss must not cost an error.
    fn try_get_stamp(&self, relative_path: &str) -> std::io::Result<Option<FileStamp>>;

    /// Puts `source` at `relative_path`, atomically and verified. This is the only way bytes
    /// reach a destination.
    fn write(
        &self,
        relative_path: &str,
        source: &dyn SourceFile,
        verify_contents: bool,
    ) -> Result<(), OperationError>;

    fn delete(&self, relative_path: &str, mode: DeleteMode) -> std::io::Result<()>;

    fn ensure_directory(&self, relative_path: &str) -> std::io::Result<()>;

    /// Non-recursive and best-effort: a directory with content, or none at all, is left alone.
    fn delete_empty_directory(&self, relative_path: &str) -> std::io::Result<()>;

    fn set_directory_last_write_time_utc(
        &self,
        relative_path: &str,
        last_write_time_utc: DateTime<Utc>,
    ) -> std::io::Result<()>;
}

/// Resolves a [`DestinationLocation`] to the provider that serves it.
pub trait DestinationProviderFactory: Send + Sync {
    fn create(&self, target: &DestinationLocation) -> anyhow::Result<Box<dyn DestinationProvider>>;
}

/// A source file on the local disk.
pub struct LocalSourceFile {
    relative_path: String,
    full_path: PathBuf,
    stamp: FileStamp,
    file_system: Arc<dyn FileSystem>,
}

impl LocalSourceFile {
    pub fn new(
        file_system: Arc<dyn FileSystem>,
        relative_path: impl Into<String>,
        full_path: impl Into<PathBuf>,
        stamp: FileStamp,
    ) -> Self {
        Self {
            relative_path: relative_path.into(),
            full_path: full_path.into(),
            stamp,
            file_system,
        }
    }
}

impl SourceFile for LocalSourceFile {
    fn relative_path(&self) -> &str {
        &self.relative_path
    }

    fn local_path(&self) -> Option<&Path> {
        Some(&self.full_path)
    }

    fn length(&self) -> u64 {
        self.stamp.length
    }

    fn stamp(&self) -> FileStamp {
        self.stamp
    }

    fn open_read(&self) -> std::io::Result<Box<dyn Read + Send>> {
        self.file_system.open_read(&self.full_path)
    }
}

/// A local or already-mounted destination directory.
pub struct LocalDestinationProvider {
    file_system: Arc<dyn FileSystem>,
    root: PathBuf,
}

impl LocalDestinationProvider {
    pub fn new(file_system: Arc<dyn FileSystem>, root: impl Into<PathBuf>) -> Self {
        Self {
            file_system,
            root: root.into(),
        }
    }

    fn resolve(&self, relative_path: &str) -> PathBuf {
        io::join(&self.root, relative_path)
    }
}

impl DestinationProvider for LocalDestinationProvider {
    fn capabilities(&self) -> DestinationCapabilities {
        let is_remote = io::is_network(&self.root);
        DestinationCapabilities {
            is_remote,
            // Network shares have no Recycle Bin; deletions there are permanent regardless.
            supports_recycle: !is_remote,
        }
    }

    fn list_tree(&self) -> std::io::Result<TreeListing> {
        match self.file_system.list_tree(&self.root) {
            Ok(listing) => Ok(listing),
            // A destination folder is created on first run, so "not there yet" is empty.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(TreeListing::empty()),
            Err(error) => Err(error),
        }
    }

    fn get_stamp(&self, relative_path: &str) -> std::io::Result<FileStamp> {
        self.file_system.get_stamp(&self.resolve(relative_path))
    }

    fn try_get_stamp(&self, relative_path: &str) -> std::io::Result<Option<FileStamp>> {
        self.file_system.try_get_stamp(&self.resolve(relative_path))
    }

    fn write(
        &self,
        relative_path: &str,
        source: &dyn SourceFile,
        verify_contents: bool,
    ) -> Result<(), OperationError> {
        let Some(local) = source.local_path() else {
            return Err(OperationError::Io(std::io::Error::other(
                "a local destination can only take a local source file",
            )));
        };
        safe_transfer::copy(
            self.file_system.as_ref(),
            local,
            &self.resolve(relative_path),
            verify_contents,
        )
    }

    fn delete(&self, relative_path: &str, mode: DeleteMode) -> std::io::Result<()> {
        let path = self.resolve(relative_path);
        match mode {
            DeleteMode::Recycle => self.file_system.recycle(&path),
            DeleteMode::Permanent => self.file_system.delete_file(&path),
        }
    }

    fn ensure_directory(&self, relative_path: &str) -> std::io::Result<()> {
        self.file_system
            .ensure_directory(&self.resolve(relative_path))
    }

    fn delete_empty_directory(&self, relative_path: &str) -> std::io::Result<()> {
        self.file_system
            .delete_empty_directory(&self.resolve(relative_path))
    }

    fn set_directory_last_write_time_utc(
        &self,
        relative_path: &str,
        last_write_time_utc: DateTime<Utc>,
    ) -> std::io::Result<()> {
        self.file_system
            .set_directory_last_write_time_utc(&self.resolve(relative_path), last_write_time_utc)
    }
}

/// Serves [`DestinationLocation::Local`].
pub struct LocalDestinationProviderFactory {
    file_system: Arc<dyn FileSystem>,
}

impl LocalDestinationProviderFactory {
    pub fn new(file_system: Arc<dyn FileSystem>) -> Self {
        Self { file_system }
    }
}

impl DestinationProviderFactory for LocalDestinationProviderFactory {
    fn create(&self, target: &DestinationLocation) -> anyhow::Result<Box<dyn DestinationProvider>> {
        match target {
            DestinationLocation::Local { path } => Ok(Box::new(LocalDestinationProvider::new(
                Arc::clone(&self.file_system),
                path,
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::InMemoryFileSystem;

    fn provider(fs: &InMemoryFileSystem, root: &str) -> LocalDestinationProvider {
        LocalDestinationProvider::new(Arc::new(fs.clone()), root)
    }

    #[test]
    fn a_destination_that_does_not_exist_yet_lists_as_empty() {
        let fs = InMemoryFileSystem::new();
        let provider = provider(&fs, r"D:\not-created-yet");

        assert_eq!(TreeListing::empty(), provider.list_tree().unwrap());
    }

    #[test]
    fn a_missing_file_has_no_stamp_and_is_not_an_error() {
        let fs = InMemoryFileSystem::new();
        fs.add_directory(r"D:\dst");
        let provider = provider(&fs, r"D:\dst");

        assert!(provider.try_get_stamp("nothing.txt").unwrap().is_none());
        assert!(provider.get_stamp("nothing.txt").is_err());
    }

    #[test]
    fn a_network_destination_reports_that_it_has_no_recycle_bin() {
        let fs = InMemoryFileSystem::new();

        assert_eq!(
            DestinationCapabilities {
                is_remote: false,
                supports_recycle: true
            },
            provider(&fs, r"C:\dst").capabilities()
        );
        assert_eq!(
            DestinationCapabilities {
                is_remote: true,
                supports_recycle: false
            },
            provider(&fs, r"\\server\share").capabilities()
        );
    }

    #[test]
    fn writing_goes_through_the_safe_transfer() {
        let fs = InMemoryFileSystem::new();
        fs.add_file(r"C:\src\a.txt", b"hello");
        fs.add_directory(r"D:\dst");
        let provider = provider(&fs, r"D:\dst");
        let stamp = fs.get_stamp(Path::new(r"C:\src\a.txt")).unwrap();
        let source = LocalSourceFile::new(Arc::new(fs.clone()), "a.txt", r"C:\src\a.txt", stamp);

        provider.write("sub/a.txt", &source, true).unwrap();

        assert_eq!(Some(b"hello".to_vec()), fs.contents_of(r"D:\dst\sub\a.txt"));
    }

    #[test]
    fn deleting_honours_the_destinations_delete_mode() {
        let fs = InMemoryFileSystem::new();
        fs.add_file(r"D:\dst\bin.txt", b"a");
        fs.add_file(r"D:\dst\gone.txt", b"b");
        let provider = provider(&fs, r"D:\dst");

        provider.delete("bin.txt", DeleteMode::Recycle).unwrap();
        provider.delete("gone.txt", DeleteMode::Permanent).unwrap();

        fs.observed(|observed| {
            assert_eq!(vec![PathBuf::from(r"D:\dst\bin.txt")], observed.recycled);
        });
        assert!(fs.all_paths().is_empty());
    }

    #[test]
    fn the_factory_serves_a_local_location() {
        let fs = InMemoryFileSystem::new();
        let factory = LocalDestinationProviderFactory::new(Arc::new(fs));
        let provider = factory
            .create(&DestinationLocation::local(r"D:\dst"))
            .unwrap();

        assert!(!provider.capabilities().is_remote);
    }
}
