//! An in-memory filesystem with fault-injection switches.
//!
//! Safety behaviour is only as good as the failures it has been shown. This fake exists so a
//! test can make a write fail halfway, corrupt a copy silently, hold a file open, or make an
//! enumeration blow up — and then assert that the destination still holds its previous
//! complete contents. **New safety behaviour ships with a fault-injection test.**
//!
//! It is a real filesystem model, not a dictionary of bytes: directories are tracked
//! explicitly, so a directory outlives the files it held, setting a directory's time does not
//! conjure it into existence, and a created-but-empty root enumerates empty while a missing
//! one fails — which is the distinction the whole "unplugged drive" guard rests on.

use std::collections::{BTreeMap, HashSet};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use chrono::{DateTime, Duration, Utc};

use crate::io::{FileStamp, FileSystem, ListedDirectory, ListedFile, TreeListing};

/// `ERROR_SHARING_VIOLATION`, the failure a really locked file produces on Windows.
const SHARING_VIOLATION: i32 = 32;

/// What should go wrong, and when.
#[derive(Debug, Default)]
pub struct Faults {
    /// Reported by [`FileSystem::available_free_space`]. `None` means "plenty".
    pub available_free_space: Option<u64>,
    /// Every write fails.
    pub fail_writes: bool,
    /// The next N writes fail, then writes succeed — a transient fault the retry should absorb.
    pub fail_writes_times: u32,
    /// Writes land, but with byte 0 flipped: same length, different content. Only a content
    /// check can see this.
    pub corrupt_writes: bool,
    /// Writes report success but quietly drop their last byte — a short write, the dominant
    /// corruption mode, which the always-on length check has to catch.
    pub truncate_writes: bool,
    /// Writes to paths containing this fragment fail.
    pub fail_write_path_fragment: Option<String>,
    /// Reads of paths containing this fragment fail.
    pub fail_read_path_fragment: Option<String>,
    /// Opening these paths reports a sharing violation, as a really locked file does.
    pub locked_paths: HashSet<String>,
    /// A commit (`replace`) into a path containing this fragment fails, leaving the temp file
    /// behind and the destination untouched.
    pub fail_replace_destination_fragment: Option<String>,
    /// Deleting paths containing this fragment fails.
    pub fail_delete_fragment: Option<String>,
    /// The next N tree walks fail.
    pub fail_enumerations: u32,
    /// Added to every timestamp written by `set_last_write_time_utc`, so a copy ends up with a
    /// stamp that does not match its source.
    pub set_last_write_time_offset: Duration,
}

/// What actually happened, for tests that assert on the *route* taken rather than the result.
#[derive(Debug, Default)]
pub struct Observed {
    pub recycled: Vec<PathBuf>,
    pub recycle_fell_back_to_permanent: Vec<PathBuf>,
    pub deleted_directories: Vec<PathBuf>,
    pub recycled_directories: Vec<PathBuf>,
    pub get_stamp_calls: usize,
    pub get_stamp_calls_by_path: BTreeMap<String, usize>,
    pub tree_walks: BTreeMap<String, usize>,
}

#[derive(Debug)]
struct FileEntry {
    /// Original casing, back-slash separated, no trailing separator.
    display: String,
    contents: Vec<u8>,
    last_write_time_utc: DateTime<Utc>,
}

#[derive(Debug)]
struct DirectoryEntry {
    display: String,
    last_write_time_utc: DateTime<Utc>,
}

#[derive(Debug, Default)]
struct State {
    files: BTreeMap<String, FileEntry>,
    directories: BTreeMap<String, DirectoryEntry>,
    faults: Faults,
    observed: Observed,
    writes_attempted: u32,
}

/// See the module docs.
#[derive(Debug, Clone, Default)]
pub struct InMemoryFileSystem {
    state: Arc<Mutex<State>>,
}

impl InMemoryFileSystem {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().expect("in-memory filesystem poisoned")
    }

    // -- seeding ------------------------------------------------------------------------------

    /// Adds a file, creating its parent directories.
    pub fn add_file(&self, path: impl AsRef<Path>, contents: impl AsRef<[u8]>) -> &Self {
        self.add_file_at(path, contents, Utc::now())
    }

    /// Adds a file with an explicit modified time.
    pub fn add_file_at(
        &self,
        path: impl AsRef<Path>,
        contents: impl AsRef<[u8]>,
        last_write_time_utc: DateTime<Utc>,
    ) -> &Self {
        let path = path.as_ref();
        let mut state = self.lock();
        state.create_parents(path);
        let display = display_of(path);
        state.files.insert(
            key_of(path),
            FileEntry {
                display,
                contents: contents.as_ref().to_vec(),
                last_write_time_utc: crate::io::normalize_utc(last_write_time_utc),
            },
        );
        drop(state);
        self
    }

    /// Adds a directory (and its parents) with no files in it.
    pub fn add_directory(&self, path: impl AsRef<Path>) -> &Self {
        let mut state = self.lock();
        state.ensure_directory_chain(path.as_ref(), Utc::now());
        drop(state);
        self
    }

    /// Adds a directory with an explicit modified time, for tests about time reconciliation.
    pub fn add_directory_at(
        &self,
        path: impl AsRef<Path>,
        last_write_time_utc: DateTime<Utc>,
    ) -> &Self {
        let path = path.as_ref();
        let mut state = self.lock();
        state.ensure_directory_chain(path, last_write_time_utc);
        if let Some(entry) = state.directories.get_mut(&key_of(path)) {
            entry.last_write_time_utc = crate::io::normalize_utc(last_write_time_utc);
        }
        drop(state);
        self
    }

    // -- inspection ---------------------------------------------------------------------------

    /// Every file path currently present, in stable order.
    pub fn all_paths(&self) -> Vec<String> {
        self.lock()
            .files
            .values()
            .map(|f| f.display.clone())
            .collect()
    }

    /// The contents of a file, or `None` when it is not there.
    pub fn contents_of(&self, path: impl AsRef<Path>) -> Option<Vec<u8>> {
        self.lock()
            .files
            .get(&key_of(path.as_ref()))
            .map(|f| f.contents.clone())
    }

    /// True when a directory is present, whether or not it holds anything.
    pub fn directory_exists(&self, path: impl AsRef<Path>) -> bool {
        self.lock().directories.contains_key(&key_of(path.as_ref()))
    }

    /// Reads out what happened.
    pub fn observed<T>(&self, read: impl FnOnce(&Observed) -> T) -> T {
        read(&self.lock().observed)
    }

    /// How many tree walks this root has seen — the assertion behind "a run walks the source
    /// once and each mirror destination once".
    pub fn tree_walks_of(&self, root: impl AsRef<Path>) -> usize {
        self.lock()
            .observed
            .tree_walks
            .get(&key_of(root.as_ref()))
            .copied()
            .unwrap_or(0)
    }

    // -- fault injection ----------------------------------------------------------------------

    /// Adjusts what should go wrong.
    pub fn with_faults(&self, configure: impl FnOnce(&mut Faults)) -> &Self {
        configure(&mut self.lock().faults);
        self
    }

    /// Makes `path` report a sharing violation, as a file held open by another process does.
    pub fn lock_path(&self, path: impl AsRef<Path>) -> &Self {
        let key = key_of(path.as_ref());
        self.lock().faults.locked_paths.insert(key);
        self
    }
}

impl State {
    fn create_parents(&mut self, path: &Path) {
        if let Some(parent) = path.parent() {
            self.ensure_directory_chain(parent, Utc::now());
        }
    }

    fn ensure_directory_chain(&mut self, path: &Path, at: DateTime<Utc>) {
        let mut current = Some(path);
        while let Some(directory) = current {
            if directory.as_os_str().is_empty() {
                break;
            }
            let key = key_of(directory);
            if key.is_empty() {
                break;
            }
            self.directories
                .entry(key)
                .or_insert_with(|| DirectoryEntry {
                    display: display_of(directory),
                    last_write_time_utc: crate::io::normalize_utc(at),
                });
            current = directory.parent();
        }
    }

    fn is_write_blocked(&self, path: &Path) -> Option<io::Error> {
        if self.faults.fail_writes {
            return Some(io::Error::other("injected write failure"));
        }
        if let Some(fragment) = &self.faults.fail_write_path_fragment {
            if display_of(path)
                .to_lowercase()
                .contains(&fragment.to_lowercase())
            {
                return Some(io::Error::other("injected write failure"));
            }
        }
        if self.faults.locked_paths.contains(&key_of(path)) {
            return Some(io::Error::from_raw_os_error(SHARING_VIOLATION));
        }
        None
    }
}

fn key_of(path: &Path) -> String {
    display_of(path).to_lowercase()
}

fn display_of(path: &Path) -> String {
    let text = path.to_string_lossy().replace('/', "\\");
    let trimmed = text.trim_end_matches('\\');
    if trimmed.is_empty() {
        text
    } else {
        trimmed.to_owned()
    }
}

fn not_found(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::NotFound,
        format!("File not found: {}", path.display()),
    )
}

/// Relative path of `child` under `root`, forward slashes, or `None` when it is not under it.
fn relative_under(root_key: &str, child_key: &str, child_display: &str) -> Option<String> {
    let prefix_len = root_key.len();
    if child_key.len() <= prefix_len + 1
        || !child_key.starts_with(root_key)
        || !child_key[prefix_len..].starts_with('\\')
    {
        return None;
    }
    Some(child_display[prefix_len + 1..].replace('\\', "/"))
}

impl FileSystem for InMemoryFileSystem {
    fn list_tree(&self, root: &Path) -> io::Result<TreeListing> {
        let mut state = self.lock();
        let root_key = key_of(root);

        *state
            .observed
            .tree_walks
            .entry(root_key.clone())
            .or_default() += 1;

        if state.faults.fail_enumerations > 0 {
            state.faults.fail_enumerations -= 1;
            return Err(io::Error::other("injected enumeration failure"));
        }

        // A missing root fails; an empty one that exists enumerates empty. That is the whole
        // difference between "the drive is unplugged" and "the folder is empty", and Mirror's
        // deletion guard is built on it.
        if !state.directories.contains_key(&root_key) {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("Folder not found or unavailable: {}", root.display()),
            ));
        }

        let mut listing = TreeListing::default();
        for entry in state.files.values() {
            if let Some(relative_path) =
                relative_under(&root_key, &entry.display.to_lowercase(), &entry.display)
            {
                listing.files.push(ListedFile {
                    relative_path,
                    stamp: FileStamp::new(entry.contents.len() as u64, entry.last_write_time_utc),
                });
            }
        }
        for entry in state.directories.values() {
            if let Some(relative_path) =
                relative_under(&root_key, &entry.display.to_lowercase(), &entry.display)
            {
                listing.directories.push(ListedDirectory {
                    relative_path,
                    last_write_time_utc: entry.last_write_time_utc,
                });
            }
        }

        Ok(listing)
    }

    fn file_exists(&self, path: &Path) -> bool {
        self.lock().files.contains_key(&key_of(path))
    }

    fn directory_exists(&self, path: &Path) -> bool {
        InMemoryFileSystem::directory_exists(self, path)
    }

    fn get_stamp(&self, path: &Path) -> io::Result<FileStamp> {
        let mut state = self.lock();
        state.observed.get_stamp_calls += 1;
        *state
            .observed
            .get_stamp_calls_by_path
            .entry(key_of(path))
            .or_default() += 1;

        state
            .files
            .get(&key_of(path))
            .map(|f| FileStamp::new(f.contents.len() as u64, f.last_write_time_utc))
            .ok_or_else(|| not_found(path))
    }

    fn read_all_bytes(&self, path: &Path) -> io::Result<Vec<u8>> {
        let state = self.lock();
        if let Some(fragment) = &state.faults.fail_read_path_fragment {
            if display_of(path)
                .to_lowercase()
                .contains(&fragment.to_lowercase())
            {
                return Err(io::Error::other("injected read failure"));
            }
        }
        if state.faults.locked_paths.contains(&key_of(path)) {
            return Err(io::Error::from_raw_os_error(SHARING_VIOLATION));
        }
        state
            .files
            .get(&key_of(path))
            .map(|f| f.contents.clone())
            .ok_or_else(|| not_found(path))
    }

    fn write_all_bytes(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        let mut state = self.lock();
        if let Some(error) = state.is_write_blocked(path) {
            return Err(error);
        }
        state.create_parents(path);
        let display = display_of(path);
        state.files.insert(
            key_of(path),
            FileEntry {
                display,
                contents: contents.to_vec(),
                last_write_time_utc: crate::io::normalize_utc(Utc::now()),
            },
        );
        Ok(())
    }

    fn delete_file(&self, path: &Path) -> io::Result<()> {
        let mut state = self.lock();
        if let Some(fragment) = &state.faults.fail_delete_fragment {
            if display_of(path)
                .to_lowercase()
                .contains(&fragment.to_lowercase())
            {
                return Err(io::Error::other("injected delete failure"));
            }
        }
        state.files.remove(&key_of(path));
        Ok(())
    }

    fn recycle(&self, path: &Path) -> io::Result<()> {
        if !self.file_exists(path) {
            return Ok(());
        }
        let network = crate::io::is_network(path);
        self.delete_file(path)?;

        let mut state = self.lock();
        if network {
            state
                .observed
                .recycle_fell_back_to_permanent
                .push(path.to_path_buf());
        } else {
            state.observed.recycled.push(path.to_path_buf());
        }
        Ok(())
    }

    fn ensure_directory(&self, path: &Path) -> io::Result<()> {
        let mut state = self.lock();
        state.ensure_directory_chain(path, Utc::now());
        Ok(())
    }

    fn delete_empty_directory(&self, path: &Path) -> io::Result<()> {
        let mut state = self.lock();
        if !state.remove_directory_if_empty(path) {
            return Ok(());
        }
        state.observed.deleted_directories.push(path.to_path_buf());
        Ok(())
    }

    fn recycle_empty_directory(&self, path: &Path) -> io::Result<()> {
        let network = crate::io::is_network(path);
        let mut state = self.lock();
        if !state.remove_directory_if_empty(path) {
            return Ok(());
        }
        if network {
            state.observed.deleted_directories.push(path.to_path_buf());
        } else {
            state.observed.recycled_directories.push(path.to_path_buf());
        }
        Ok(())
    }

    fn set_directory_last_write_time_utc(
        &self,
        path: &Path,
        last_write_time_utc: DateTime<Utc>,
    ) -> io::Result<()> {
        let mut state = self.lock();
        // Deliberately does *not* create the directory: setting a time on something that is
        // not there is a no-op, so a planner bug shows up as a missing directory rather than
        // as a phantom one the fake invented.
        if let Some(entry) = state.directories.get_mut(&key_of(path)) {
            entry.last_write_time_utc = crate::io::normalize_utc(last_write_time_utc);
        }
        Ok(())
    }

    fn open_read(&self, path: &Path) -> io::Result<Box<dyn Read + Send>> {
        Ok(Box::new(io::Cursor::new(self.read_all_bytes(path)?)))
    }

    fn create_write_through(&self, path: &Path) -> io::Result<Box<dyn Write + Send>> {
        let mut state = self.lock();
        if let Some(error) = state.is_write_blocked(path) {
            return Err(error);
        }
        if state.faults.fail_writes_times > 0 {
            state.faults.fail_writes_times -= 1;
            return Err(io::Error::other("injected transient write failure"));
        }

        state.writes_attempted += 1;
        state.create_parents(path);
        let display = display_of(path);
        // The file exists the moment it is created and grows as bytes land, exactly like a
        // real one — which is what makes an interrupted write observable as a partial temp
        // file rather than as nothing at all.
        state.files.insert(
            key_of(path),
            FileEntry {
                display,
                contents: Vec::new(),
                last_write_time_utc: crate::io::normalize_utc(Utc::now()),
            },
        );
        let corrupt = state.faults.corrupt_writes;
        let truncate = state.faults.truncate_writes;
        drop(state);

        Ok(Box::new(MemoryWriter {
            state: Arc::clone(&self.state),
            key: key_of(path),
            corrupt,
            truncate,
            wrote_any: false,
        }))
    }

    fn set_last_write_time_utc(
        &self,
        path: &Path,
        last_write_time_utc: DateTime<Utc>,
    ) -> io::Result<()> {
        let mut state = self.lock();
        let offset = state.faults.set_last_write_time_offset;
        let entry = state
            .files
            .get_mut(&key_of(path))
            .ok_or_else(|| not_found(path))?;
        entry.last_write_time_utc = crate::io::normalize_utc(last_write_time_utc + offset);
        Ok(())
    }

    fn replace(&self, source_path: &Path, destination_path: &Path) -> io::Result<()> {
        let mut state = self.lock();
        if let Some(fragment) = &state.faults.fail_replace_destination_fragment {
            if display_of(destination_path)
                .to_lowercase()
                .contains(&fragment.to_lowercase())
            {
                return Err(io::Error::other("injected commit failure"));
            }
        }

        let Some(mut entry) = state.files.remove(&key_of(source_path)) else {
            return Err(not_found(source_path));
        };
        state.create_parents(destination_path);
        entry.display = display_of(destination_path);
        state.files.insert(key_of(destination_path), entry);
        Ok(())
    }

    fn available_free_space(&self, _path: &Path) -> u64 {
        self.lock().faults.available_free_space.unwrap_or(u64::MAX)
    }
}

impl State {
    fn remove_directory_if_empty(&mut self, path: &Path) -> bool {
        let key = key_of(path);
        if !self.directories.contains_key(&key) {
            return false;
        }
        let occupied = self
            .files
            .keys()
            .chain(self.directories.keys().filter(|k| **k != key))
            .any(|k| k.starts_with(&key) && k[key.len()..].starts_with('\\'));
        if occupied {
            return false;
        }
        self.directories.remove(&key);
        true
    }
}

struct MemoryWriter {
    state: Arc<Mutex<State>>,
    key: String,
    corrupt: bool,
    truncate: bool,
    wrote_any: bool,
}

impl Write for MemoryWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let mut state = self.state.lock().expect("in-memory filesystem poisoned");
        if state.faults.fail_writes {
            return Err(io::Error::other("injected write failure"));
        }

        let entry = state
            .files
            .get_mut(&self.key)
            .ok_or_else(|| io::Error::other("the file being written disappeared"))?;

        // A short write reports full success and quietly drops a byte, which is exactly how a
        // truncation looks from the caller's side.
        let landed = if self.truncate && !buffer.is_empty() {
            &buffer[..buffer.len() - 1]
        } else {
            buffer
        };
        entry.contents.extend_from_slice(landed);

        if self.corrupt && !self.wrote_any && !entry.contents.is_empty() {
            // Same length, different content: only a read-back content check can see it.
            entry.contents[0] = entry.contents[0].wrapping_add(1);
            self.wrote_any = true;
        }

        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_directory_outlives_the_files_it_held() {
        let fs = InMemoryFileSystem::new();
        fs.add_file(r"C:\src\sub\a.txt", b"a");

        fs.delete_file(Path::new(r"C:\src\sub\a.txt")).unwrap();

        assert!(fs.directory_exists(r"C:\src\sub"));
        let listing = fs.list_tree(Path::new(r"C:\src")).unwrap();
        assert!(listing.files.is_empty());
        assert_eq!(vec!["sub"], relative_directories(&listing));
    }

    #[test]
    fn setting_a_directory_time_does_not_create_the_directory() {
        let fs = InMemoryFileSystem::new();
        fs.add_directory(r"C:\src");

        fs.set_directory_last_write_time_utc(Path::new(r"C:\src\ghost"), Utc::now())
            .unwrap();

        assert!(!fs.directory_exists(r"C:\src\ghost"));
    }

    #[test]
    fn a_created_but_empty_root_enumerates_empty_while_a_missing_one_fails() {
        let fs = InMemoryFileSystem::new();
        fs.add_directory(r"C:\src");

        assert_eq!(
            TreeListing::empty(),
            fs.list_tree(Path::new(r"C:\src")).unwrap()
        );

        let error = fs.list_tree(Path::new(r"C:\gone")).unwrap_err();
        assert_eq!(io::ErrorKind::NotFound, error.kind());
    }

    #[test]
    fn a_tree_walk_reports_root_relative_forward_slash_paths_preserving_case() {
        let fs = InMemoryFileSystem::new();
        fs.add_file(r"C:\src\Photos\2024\IMG_0042.JPG", b"x");

        let listing = fs.list_tree(Path::new(r"c:\SRC")).unwrap();

        assert_eq!(vec!["Photos/2024/IMG_0042.JPG"], relative_files(&listing));
        assert_eq!(
            vec!["Photos", "Photos/2024"],
            relative_directories(&listing)
        );
    }

    #[test]
    fn a_write_through_stream_grows_the_file_as_bytes_land() {
        let fs = InMemoryFileSystem::new();
        fs.add_directory(r"C:\dst");

        let mut writer = fs.create_write_through(Path::new(r"C:\dst\a.txt")).unwrap();
        assert_eq!(Some(Vec::new()), fs.contents_of(r"C:\dst\a.txt"));

        writer.write_all(b"hello").unwrap();
        writer.flush().unwrap();
        drop(writer);

        assert_eq!(Some(b"hello".to_vec()), fs.contents_of(r"C:\dst\a.txt"));
    }

    #[test]
    fn a_locked_path_reports_the_failure_file_busy_recognizes() {
        let fs = InMemoryFileSystem::new();
        fs.add_file(r"C:\src\a.txt", b"a");
        fs.lock_path(r"C:\src\a.txt");

        let error = fs.read_all_bytes(Path::new(r"C:\src\a.txt")).unwrap_err();
        assert!(crate::io::is_busy(&error));
    }

    #[test]
    fn corrupt_writes_keep_the_length_and_change_the_content() {
        let fs = InMemoryFileSystem::new();
        fs.add_directory(r"C:\dst");
        fs.with_faults(|faults| faults.corrupt_writes = true);

        let mut writer = fs.create_write_through(Path::new(r"C:\dst\a.txt")).unwrap();
        writer.write_all(b"hello").unwrap();
        drop(writer);

        let written = fs.contents_of(r"C:\dst\a.txt").unwrap();
        assert_eq!(5, written.len());
        assert_ne!(b"hello".to_vec(), written);
    }

    #[test]
    fn a_transient_write_failure_clears_itself() {
        let fs = InMemoryFileSystem::new();
        fs.add_directory(r"C:\dst");
        fs.with_faults(|faults| faults.fail_writes_times = 2);

        assert!(fs.create_write_through(Path::new(r"C:\dst\a.txt")).is_err());
        assert!(fs.create_write_through(Path::new(r"C:\dst\a.txt")).is_err());
        assert!(fs.create_write_through(Path::new(r"C:\dst\a.txt")).is_ok());
    }

    #[test]
    fn replace_moves_the_bytes_and_leaves_no_source() {
        let fs = InMemoryFileSystem::new();
        fs.add_file(r"C:\dst\a.txt.tmp", b"new");
        fs.add_file(r"C:\dst\a.txt", b"old");

        fs.replace(Path::new(r"C:\dst\a.txt.tmp"), Path::new(r"C:\dst\a.txt"))
            .unwrap();

        assert_eq!(Some(b"new".to_vec()), fs.contents_of(r"C:\dst\a.txt"));
        assert!(!fs.file_exists(Path::new(r"C:\dst\a.txt.tmp")));
    }

    #[test]
    fn an_empty_directory_is_removable_and_an_occupied_one_is_not() {
        let fs = InMemoryFileSystem::new();
        fs.add_file(r"C:\src\keep\a.txt", b"a");
        fs.add_directory(r"C:\src\empty");

        fs.delete_empty_directory(Path::new(r"C:\src\keep"))
            .unwrap();
        fs.delete_empty_directory(Path::new(r"C:\src\empty"))
            .unwrap();
        fs.delete_empty_directory(Path::new(r"C:\src\gone"))
            .unwrap();

        assert!(
            fs.directory_exists(r"C:\src\keep"),
            "a directory with content is left alone"
        );
        assert!(!fs.directory_exists(r"C:\src\empty"));
        fs.observed(|observed| {
            assert_eq!(
                vec![PathBuf::from(r"C:\src\empty")],
                observed.deleted_directories
            );
        });
    }

    #[test]
    fn recycling_on_a_network_path_falls_back_to_a_permanent_delete() {
        let fs = InMemoryFileSystem::new();
        fs.add_file(r"\\server\share\a.txt", b"a");
        fs.add_file(r"C:\dst\b.txt", b"b");

        fs.recycle(Path::new(r"\\server\share\a.txt")).unwrap();
        fs.recycle(Path::new(r"C:\dst\b.txt")).unwrap();

        fs.observed(|observed| {
            assert_eq!(vec![PathBuf::from(r"C:\dst\b.txt")], observed.recycled);
            assert_eq!(
                vec![PathBuf::from(r"\\server\share\a.txt")],
                observed.recycle_fell_back_to_permanent
            );
        });
    }

    #[test]
    fn try_get_stamp_reports_a_missing_file_without_an_error() {
        let fs = InMemoryFileSystem::new();
        fs.add_file(r"C:\src\a.txt", b"a");

        assert!(fs
            .try_get_stamp(Path::new(r"C:\src\a.txt"))
            .unwrap()
            .is_some());
        assert!(fs
            .try_get_stamp(Path::new(r"C:\src\gone.txt"))
            .unwrap()
            .is_none());
    }

    fn relative_files(listing: &TreeListing) -> Vec<&str> {
        listing
            .files
            .iter()
            .map(|f| f.relative_path.as_str())
            .collect()
    }

    fn relative_directories(listing: &TreeListing) -> Vec<&str> {
        listing
            .directories
            .iter()
            .map(|d| d.relative_path.as_str())
            .collect()
    }
}
