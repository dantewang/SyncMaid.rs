//! Copies one file safely: temp → verify → atomic rename.
//!
//! The destination is never left truncated or corrupted, even if the process is killed, the
//! disk fills, or the source read fails mid-stream — the existing destination is only ever
//! replaced by a complete, verified file, in one metadata-only rename.
//!
//! The orchestration lives here rather than inside [`FileSystem`] so it can be run against an
//! in-memory filesystem that injects faults (interrupted write, silent corruption, a failed
//! commit) and the safety properties can actually be proven.
//!
//! Verification has two tiers:
//!
//! - **Basic**, always: a length check after the write, which catches truncation and partial
//!   writes — the dominant corruption mode — plus the atomic rename, so a partial file is
//!   never visible at the destination. It is a metadata call, not a re-read.
//! - **Content**, opt-in per destination: the written temp is read back and its xxHash
//!   compared to the source's, catching silent hardware or environmental corruption a length
//!   check cannot see. This is *not* redundant with SMB or TLS integrity, which only protect
//!   the wire between the two protocol stacks — a read-back is the only thing that exercises
//!   the server-side segment. It costs a full re-read of every copied file.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use uuid::Uuid;
use xxhash_rust::xxh3::Xxh3;

use crate::io::FileSystem;
use crate::sync::OperationError;

/// 64 KiB streaming buffer — big enough to keep the syscall count down, small enough to stay
/// off the large-object heap's Rust equivalent (the stack, here) and to stream huge files.
const BUFFER_SIZE: usize = 1 << 16;

/// Marks our in-progress files so a leftover from a killed run is recognizable, and so it
/// lands as a sibling of the destination — same directory, same volume, which is what makes
/// the commit an atomic rename rather than a copy.
const TEMP_SUFFIX: &str = ".syncmaid-tmp-";

/// Copies `source` to `destination`, leaving the source in place.
pub fn copy(
    file_system: &dyn FileSystem,
    source: &Path,
    destination: &Path,
    verify_contents: bool,
) -> Result<(), OperationError> {
    let source_stamp = file_system.get_stamp(source)?;

    // Preflight: fail fast rather than fill the volume and leave a partial temp behind.
    if file_system.available_free_space(destination) < source_stamp.length {
        return Err(OperationError::Io(std::io::Error::other(format!(
            "Not enough free space at the destination for '{}' ({} bytes).",
            source.display(),
            source_stamp.length
        ))));
    }

    let temp = temp_path_for(destination);

    match transfer(
        file_system,
        source,
        destination,
        &temp,
        verify_contents,
        source_stamp.length,
    ) {
        Ok(()) => Ok(()),
        Err(error) => {
            // The existing destination was never touched. Clean up our temp, but never let a
            // cleanup failure replace the failure that actually matters.
            let _ = file_system.delete_file(&temp);
            Err(error)
        }
    }
}

fn transfer(
    file_system: &dyn FileSystem,
    source: &Path,
    destination: &Path,
    temp: &Path,
    verify_contents: bool,
    expected_length: u64,
) -> Result<(), OperationError> {
    let source_hash = {
        let mut input = file_system.open_read(source)?;
        let mut output = file_system.create_write_through(temp)?;
        let hash = copy_and_hash(&mut input, &mut output)?;
        output.flush()?;
        hash
    };

    // Preserve the source timestamp so source and copy share a FileStamp and the next run does
    // not see a change. Skip it and every run re-copies the whole tree.
    let source_stamp = file_system.get_stamp(source)?;
    file_system.set_last_write_time_utc(temp, source_stamp.last_write_time_utc)?;

    let temp_stamp = file_system.get_stamp(temp)?;
    if temp_stamp.length != expected_length {
        return Err(OperationError::verification(format!(
            "Copy of '{}' has the wrong length ({} vs {expected_length}).",
            source.display(),
            temp_stamp.length
        )));
    }

    if verify_contents {
        let mut read_back = file_system.open_read(temp)?;
        if hash_stream(&mut read_back)? != source_hash {
            return Err(OperationError::verification(format!(
                "Copy of '{}' failed content verification (xxHash mismatch).",
                source.display()
            )));
        }
    }

    // Commit.
    file_system.replace(temp, destination)?;
    Ok(())
}

fn temp_path_for(destination: &Path) -> PathBuf {
    let mut name = destination.as_os_str().to_os_string();
    name.push(TEMP_SUFFIX);
    name.push(Uuid::new_v4().simple().to_string());
    PathBuf::from(name)
}

/// Streams input to output, hashing the bytes as they pass through — one read of the source,
/// no extra pass.
fn copy_and_hash(input: &mut dyn Read, output: &mut dyn Write) -> std::io::Result<u128> {
    let mut hasher = Xxh3::new();
    let mut buffer = vec![0u8; BUFFER_SIZE];
    loop {
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        output.write_all(&buffer[..read])?;
    }
    Ok(hasher.digest128())
}

fn hash_stream(stream: &mut dyn Read) -> std::io::Result<u128> {
    let mut hasher = Xxh3::new();
    let mut buffer = vec![0u8; BUFFER_SIZE];
    loop {
        let read = stream.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.digest128())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::InMemoryFileSystem;

    fn paths() -> (PathBuf, PathBuf) {
        (
            PathBuf::from(r"C:\src\a.txt"),
            PathBuf::from(r"D:\dst\a.txt"),
        )
    }

    #[test]
    fn a_copy_lands_with_its_sources_bytes_and_stamp() {
        let fs = InMemoryFileSystem::new();
        let (source, destination) = paths();
        fs.add_file(&source, b"hello");
        fs.add_directory(r"D:\dst");

        copy(&fs, &source, &destination, false).unwrap();

        assert_eq!(Some(b"hello".to_vec()), fs.contents_of(&destination));
        assert_eq!(
            fs.get_stamp(&source).unwrap(),
            fs.get_stamp(&destination).unwrap(),
            "a copy that does not share its source's stamp gets re-copied forever"
        );
    }

    #[test]
    fn a_failed_write_leaves_the_previous_destination_intact_and_no_temp_behind() {
        let fs = InMemoryFileSystem::new();
        let (source, destination) = paths();
        fs.add_file(&source, b"new");
        fs.add_file(&destination, b"previous complete version");
        fs.with_faults(|faults| faults.fail_writes = true);

        let error = copy(&fs, &source, &destination, false).unwrap_err();

        assert!(matches!(error, OperationError::Io(_)));
        assert_eq!(
            Some(b"previous complete version".to_vec()),
            fs.contents_of(&destination)
        );
        assert!(!fs.all_paths().iter().any(|path| path.contains(TEMP_SUFFIX)));
    }

    #[test]
    fn a_failed_commit_leaves_the_previous_destination_intact() {
        let fs = InMemoryFileSystem::new();
        let (source, destination) = paths();
        fs.add_file(&source, b"new");
        fs.add_file(&destination, b"previous complete version");
        fs.with_faults(|faults| {
            faults.fail_replace_destination_fragment = Some("a.txt".into());
        });

        copy(&fs, &source, &destination, false).unwrap_err();

        assert_eq!(
            Some(b"previous complete version".to_vec()),
            fs.contents_of(&destination)
        );
        assert!(!fs.all_paths().iter().any(|path| path.contains(TEMP_SUFFIX)));
    }

    #[test]
    fn silent_corruption_is_caught_only_when_content_verification_is_on() {
        let fs = InMemoryFileSystem::new();
        let (source, destination) = paths();
        fs.add_file(&source, b"hello");
        fs.add_directory(r"D:\dst");
        fs.with_faults(|faults| faults.corrupt_writes = true);

        // The length still matches, so the always-on check cannot see this.
        copy(&fs, &source, &destination, false).expect("the basic tier only checks length");
        assert_ne!(Some(b"hello".to_vec()), fs.contents_of(&destination));

        let error = copy(&fs, &source, &destination, true).unwrap_err();
        assert!(
            matches!(&error, OperationError::Verification(message) if message.contains("xxHash")),
            "got {error:?}"
        );
    }

    #[test]
    fn a_corrupted_copy_never_reaches_the_destination() {
        let fs = InMemoryFileSystem::new();
        let (source, destination) = paths();
        fs.add_file(&source, b"hello");
        fs.add_file(&destination, b"olleh");
        fs.with_faults(|faults| faults.corrupt_writes = true);

        copy(&fs, &source, &destination, true).unwrap_err();

        assert_eq!(Some(b"olleh".to_vec()), fs.contents_of(&destination));
    }

    #[test]
    fn a_full_volume_is_refused_before_anything_is_written() {
        let fs = InMemoryFileSystem::new();
        let (source, destination) = paths();
        fs.add_file(&source, b"hello");
        fs.add_directory(r"D:\dst");
        fs.with_faults(|faults| faults.available_free_space = Some(2));

        let error = copy(&fs, &source, &destination, false).unwrap_err();

        assert!(
            error.to_string().contains("Not enough free space"),
            "got {error}"
        );
        assert!(fs
            .all_paths()
            .iter()
            .all(|path| !path.contains(TEMP_SUFFIX)));
    }

    #[test]
    fn a_truncated_copy_is_caught_by_the_always_on_length_check() {
        let fs = InMemoryFileSystem::new();
        let (source, destination) = paths();
        fs.add_file(&source, b"hello");
        fs.add_file(&destination, b"previous complete version");
        // A short write that reports success — the dominant corruption mode, and the one the
        // basic tier exists for. No opt-in needed.
        fs.with_faults(|faults| faults.truncate_writes = true);

        let error = copy(&fs, &source, &destination, false).unwrap_err();

        assert!(
            matches!(&error, OperationError::Verification(message) if message.contains("wrong length")),
            "got {error:?}"
        );
        assert_eq!(
            Some(b"previous complete version".to_vec()),
            fs.contents_of(&destination)
        );
    }

    #[test]
    fn the_temp_file_is_a_sibling_of_the_destination() {
        // Same directory means same volume, which is what makes the commit an atomic rename
        // instead of a cross-volume copy that could be interrupted halfway.
        let temp = temp_path_for(Path::new(r"D:\dst\sub\a.txt"));
        assert_eq!(Path::new(r"D:\dst\sub"), temp.parent().unwrap());
        assert!(temp.to_string_lossy().contains(TEMP_SUFFIX));
    }
}
