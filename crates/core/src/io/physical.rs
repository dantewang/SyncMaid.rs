//! The real disk.

use std::fs::{self, File, FileTimes, OpenOptions};
use std::io::{self, Read, Write};
use std::path::Path;
use std::time::SystemTime;

use chrono::{DateTime, Utc};

use crate::io::{FileStamp, FileSystem, ListedDirectory, ListedFile, TreeListing};

/// [`FileSystem`] backed by the operating system.
#[derive(Debug, Clone, Copy, Default)]
pub struct PhysicalFileSystem;

impl PhysicalFileSystem {
    pub fn new() -> Self {
        Self
    }
}

impl FileSystem for PhysicalFileSystem {
    /// Walks the tree in one pass, reading each entry's length and modified time from the
    /// directory scan itself rather than re-opening every file.
    ///
    /// Known limitation carried over from the C# engine: the walk follows reparse points, so a
    /// directory junction that points at an ancestor makes it recurse forever. Guarding that
    /// is tracked separately; changing it here silently would change which trees sync.
    fn list_tree(&self, root: &Path) -> io::Result<TreeListing> {
        let mut listing = TreeListing::default();
        let mut pending = vec![(root.to_path_buf(), String::new())];

        while let Some((directory, prefix)) = pending.pop() {
            let entries =
                fs::read_dir(&directory).map_err(|error| describe_missing(error, root))?;

            for entry in entries {
                let entry = entry?;
                let name = entry.file_name().to_string_lossy().into_owned();
                let relative_path = if prefix.is_empty() {
                    name
                } else {
                    format!("{prefix}/{name}")
                };

                let metadata = entry.metadata()?;
                let modified = to_utc(metadata.modified()?);

                if metadata.is_dir() {
                    listing.directories.push(ListedDirectory {
                        relative_path: relative_path.clone(),
                        last_write_time_utc: crate::io::normalize_utc(modified),
                    });
                    pending.push((entry.path(), relative_path));
                } else {
                    listing.files.push(ListedFile {
                        relative_path,
                        stamp: FileStamp::new(metadata.len(), modified),
                    });
                }
            }
        }

        Ok(listing)
    }

    fn file_exists(&self, path: &Path) -> bool {
        path.is_file()
    }

    fn directory_exists(&self, path: &Path) -> bool {
        path.is_dir()
    }

    fn get_stamp(&self, path: &Path) -> io::Result<FileStamp> {
        let metadata = fs::metadata(path)?;
        Ok(FileStamp::new(metadata.len(), to_utc(metadata.modified()?)))
    }

    fn read_all_bytes(&self, path: &Path) -> io::Result<Vec<u8>> {
        fs::read(path)
    }

    fn write_all_bytes(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        ensure_parent(path)?;
        fs::write(path, contents)
    }

    fn delete_file(&self, path: &Path) -> io::Result<()> {
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }

    fn recycle(&self, path: &Path) -> io::Result<()> {
        if !path.exists() {
            return Ok(());
        }
        // Network shares have no Recycle Bin, so a removal there is permanent regardless.
        // Saying so plainly beats failing the run over a facility the volume does not have.
        if crate::io::is_network(path) {
            return self.delete_file(path);
        }
        send_to_recycle_bin(path)
    }

    fn ensure_directory(&self, path: &Path) -> io::Result<()> {
        fs::create_dir_all(path)
    }

    fn delete_empty_directory(&self, path: &Path) -> io::Result<()> {
        // `remove_dir` refuses a non-empty directory, which is exactly the guarantee wanted:
        // content that appeared since the caller decided is never taken along.
        match fs::remove_dir(path) {
            Ok(()) => Ok(()),
            Err(error) if is_absent_or_occupied(&error) => Ok(()),
            Err(error) => Err(error),
        }
    }

    fn recycle_empty_directory(&self, path: &Path) -> io::Result<()> {
        if !is_empty_directory(path) {
            return Ok(());
        }
        if crate::io::is_network(path) {
            return self.delete_empty_directory(path);
        }
        match send_to_recycle_bin(path) {
            Ok(()) => Ok(()),
            Err(error) if is_absent_or_occupied(&error) => Ok(()),
            Err(error) => Err(error),
        }
    }

    fn set_directory_last_write_time_utc(
        &self,
        path: &Path,
        last_write_time_utc: DateTime<Utc>,
    ) -> io::Result<()> {
        match open_directory_for_attributes(path) {
            Ok(directory) => {
                directory.set_times(FileTimes::new().set_modified(last_write_time_utc.into()))
            }
            // A directory that is not there is left alone without error — the next run replans.
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }

    fn open_read(&self, path: &Path) -> io::Result<Box<dyn Read + Send>> {
        Ok(Box::new(File::open(path)?))
    }

    fn create_write_through(&self, path: &Path) -> io::Result<Box<dyn Write + Send>> {
        ensure_parent(path)?;
        Ok(Box::new(open_write_through(path)?))
    }

    fn set_last_write_time_utc(
        &self,
        path: &Path,
        last_write_time_utc: DateTime<Utc>,
    ) -> io::Result<()> {
        let file = OpenOptions::new().write(true).open(path)?;
        file.set_times(FileTimes::new().set_modified(last_write_time_utc.into()))
    }

    fn replace(&self, source_path: &Path, destination_path: &Path) -> io::Result<()> {
        ensure_parent(destination_path)?;
        // On Windows this is MoveFileEx with MOVEFILE_REPLACE_EXISTING: a metadata-only,
        // atomic rename when both ends are on the same volume — which they are, because the
        // temp file is written as a sibling of the destination precisely to guarantee it.
        fs::rename(source_path, destination_path)
    }

    fn available_free_space(&self, path: &Path) -> u64 {
        available_free_space(path)
    }
}

fn ensure_parent(path: &Path) -> io::Result<()> {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => fs::create_dir_all(parent),
        _ => Ok(()),
    }
}

fn to_utc(time: SystemTime) -> DateTime<Utc> {
    DateTime::<Utc>::from(time)
}

fn describe_missing(error: io::Error, root: &Path) -> io::Error {
    if error.kind() == io::ErrorKind::NotFound {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("Folder not found or unavailable: {}", root.display()),
        )
    } else {
        error
    }
}

fn is_absent_or_occupied(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::DirectoryNotEmpty
    ) || crate::io::is_busy(error)
        || error.raw_os_error() == Some(145) // ERROR_DIR_NOT_EMPTY
}

fn is_empty_directory(path: &Path) -> bool {
    fs::read_dir(path).is_ok_and(|mut entries| entries.next().is_none())
}

#[cfg(windows)]
fn open_write_through(path: &Path) -> io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_WRITE_THROUGH;

    OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .custom_flags(FILE_FLAG_WRITE_THROUGH)
        .open(path)
}

#[cfg(not(windows))]
fn open_write_through(path: &Path) -> io::Result<File> {
    File::create(path)
}

#[cfg(windows)]
fn open_directory_for_attributes(path: &Path) -> io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_WRITE_ATTRIBUTES,
    };

    // A directory handle needs BACKUP_SEMANTICS; WRITE_ATTRIBUTES is the least authority that
    // still lets the timestamp through.
    OpenOptions::new()
        .access_mode(FILE_WRITE_ATTRIBUTES)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
}

#[cfg(not(windows))]
fn open_directory_for_attributes(path: &Path) -> io::Result<File> {
    File::open(path)
}

#[cfg(windows)]
fn send_to_recycle_bin(path: &Path) -> io::Result<()> {
    trash::delete(path).map_err(|error| match error {
        trash::Error::CouldNotAccess { .. } => {
            io::Error::new(io::ErrorKind::NotFound, error.to_string())
        }
        other => io::Error::other(other.to_string()),
    })
}

#[cfg(not(windows))]
fn send_to_recycle_bin(path: &Path) -> io::Result<()> {
    // No Recycle Bin here; the caller's contract allows a permanent delete as the fallback.
    if path.is_dir() {
        fs::remove_dir(path)
    } else {
        fs::remove_file(path)
    }
}

#[cfg(windows)]
fn available_free_space(path: &Path) -> u64 {
    use std::os::windows::ffi::OsStrExt;
    use std::path::Component;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    // A share reports its own free space unreliably (quotas, DFS), which is why the C# engine
    // skipped the preflight there rather than refusing a copy on a bad number.
    if crate::io::is_network(path) {
        return u64::MAX;
    }

    let Ok(absolute) = std::path::absolute(path) else {
        return u64::MAX;
    };
    let Some(Component::Prefix(prefix)) = absolute.components().next() else {
        return u64::MAX;
    };

    let root = format!("{}\\", prefix.as_os_str().to_string_lossy());
    let wide: Vec<u16> = std::ffi::OsStr::new(&root)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let mut available: u64 = 0;
    // SAFETY: `wide` is a NUL-terminated UTF-16 buffer and `available` is a live u64; both
    // outlive the call. A failure leaves `available` untouched, which the check below covers.
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut available,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };

    if ok == 0 {
        u64::MAX
    } else {
        available
    }
}

#[cfg(not(windows))]
fn available_free_space(_path: &Path) -> u64 {
    u64::MAX
}
