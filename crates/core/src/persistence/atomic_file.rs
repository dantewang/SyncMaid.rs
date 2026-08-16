//! Writing a config file safely.
//!
//! A crash or power cut mid-write must never corrupt it — for `tasks.json` that would mean
//! losing every task definition, which is exactly the failure the sync engine was hardened
//! against. So config gets the same temp → flush → atomic-rename discipline user files get,
//! and the previous version is kept as `<path>.bak` so a corrupt main file can be recovered.

use std::io::Write;
use std::path::{Path, PathBuf};

use uuid::Uuid;

use crate::io::FileSystem;

/// The suffix of the previous-version backup written alongside the main file.
pub const BACKUP_SUFFIX: &str = ".bak";

/// Atomically writes `contents` to `path`.
///
/// On any failure the existing file is left untouched, and at no point does `path` stop
/// existing.
pub fn write(file_system: &dyn FileSystem, path: &Path, contents: &[u8]) -> std::io::Result<()> {
    let temp = temp_beside(path);

    let result = (|| {
        let mut stream = file_system.create_write_through(&temp)?;
        stream.write_all(contents)?;
        stream.flush()?;
        drop(stream);

        snapshot_backup(file_system, path)?;

        // Commit: one atomic rename over the live file.
        file_system.replace(&temp, path)
    })();

    if result.is_err() {
        // Best-effort: never let a cleanup failure replace the real cause.
        let _ = file_system.delete_file(&temp);
    }
    result
}

/// The path of the backup kept beside `path`.
pub fn backup_of(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(BACKUP_SUFFIX);
    PathBuf::from(name)
}

/// Copies the current good version aside, then renames the copy into place as `.bak`.
///
/// The obvious implementation — rename the live file to `.bak` — is wrong. A rename is a move,
/// so between it and the commit there is a window in which `path` does not exist at all. A
/// crash there, or a failure of the commit rename (a reader, an antivirus hold, a sharing
/// violation can all cause one), would leave the config with only a `.bak` — which for
/// `tasks.json` means the app starts with no visible tasks. Copying keeps the live file in
/// place until the single atomic commit replaces it, and renaming the *copy* keeps the
/// guarantee that `.bak` is never a partial write.
fn snapshot_backup(file_system: &dyn FileSystem, path: &Path) -> std::io::Result<()> {
    if !file_system.file_exists(path) {
        return Ok(()); // First save: nothing to back up.
    }

    let backup_temp = temp_beside(path);
    let result = (|| {
        let current = file_system.read_all_bytes(path)?;
        let mut stream = file_system.create_write_through(&backup_temp)?;
        stream.write_all(&current)?;
        stream.flush()?;
        drop(stream);

        file_system.replace(&backup_temp, &backup_of(path))
    })();

    if result.is_err() {
        let _ = file_system.delete_file(&backup_temp);
    }
    result
}

fn temp_beside(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(format!(".tmp-{}", Uuid::new_v4().simple()));
    PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::InMemoryFileSystem;

    const PATH: &str = r"C:\cfg\tasks.json";

    fn temps(file_system: &InMemoryFileSystem) -> Vec<String> {
        file_system
            .all_paths()
            .into_iter()
            .filter(|path| path.contains(".tmp-"))
            .collect()
    }

    #[test]
    fn a_first_save_writes_the_file_and_no_backup() {
        let fs = InMemoryFileSystem::new();

        write(&fs, Path::new(PATH), b"first").unwrap();

        assert_eq!(Some(b"first".to_vec()), fs.contents_of(PATH));
        assert!(fs.contents_of(format!("{PATH}.bak")).is_none());
        assert!(temps(&fs).is_empty());
    }

    #[test]
    fn a_second_save_keeps_the_previous_version_as_bak() {
        let fs = InMemoryFileSystem::new();

        write(&fs, Path::new(PATH), b"first").unwrap();
        write(&fs, Path::new(PATH), b"second").unwrap();

        assert_eq!(Some(b"second".to_vec()), fs.contents_of(PATH));
        assert_eq!(
            Some(b"first".to_vec()),
            fs.contents_of(format!("{PATH}.bak"))
        );
    }

    #[test]
    fn an_interrupted_write_leaves_the_live_file_and_no_temp_behind() {
        let fs = InMemoryFileSystem::new();
        write(&fs, Path::new(PATH), b"good").unwrap();
        fs.with_faults(|faults| faults.fail_writes = true);

        write(&fs, Path::new(PATH), b"never lands").unwrap_err();

        assert_eq!(Some(b"good".to_vec()), fs.contents_of(PATH));
        assert!(temps(&fs).is_empty());
    }

    #[test]
    fn a_failed_commit_leaves_the_live_file_intact() {
        let fs = InMemoryFileSystem::new();
        write(&fs, Path::new(PATH), b"good").unwrap();
        fs.with_faults(|faults| {
            faults.fail_replace_destination_fragment = Some("tasks.json".into());
        });

        write(&fs, Path::new(PATH), b"never lands").unwrap_err();

        assert_eq!(Some(b"good".to_vec()), fs.contents_of(PATH));
    }

    #[test]
    fn the_live_file_never_stops_existing_while_the_backup_is_taken() {
        // A rename-the-live-file implementation would fail this: the backup step must not be
        // able to leave the config with only a .bak.
        let fs = InMemoryFileSystem::new();
        write(&fs, Path::new(PATH), b"good").unwrap();
        fs.with_faults(|faults| faults.fail_read_path_fragment = Some("tasks.json".into()));

        write(&fs, Path::new(PATH), b"never lands").unwrap_err();

        assert_eq!(Some(b"good".to_vec()), fs.contents_of(PATH));
        assert!(temps(&fs).is_empty());
    }
}
