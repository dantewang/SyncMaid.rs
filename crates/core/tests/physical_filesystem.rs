//! Pins [`PhysicalFileSystem`] against a real disk.
//!
//! The engine's tests run against the in-memory fake, which is only worth anything while the
//! fake and the real thing agree. These run the same questions against a temp directory, so a
//! divergence shows up here rather than in production.

use std::io::Write;
use std::path::Path;

use chrono::{TimeZone, Utc};
use syncmaid_core::io::{FileSystem, PhysicalFileSystem};
use tempfile::TempDir;

fn temp() -> TempDir {
    TempDir::new().expect("create temp dir")
}

fn sorted(mut values: Vec<String>) -> Vec<String> {
    values.sort();
    values
}

#[test]
fn a_tree_walk_reports_root_relative_forward_slash_paths_including_empty_directories() {
    let root = temp();
    let fs = PhysicalFileSystem::new();

    fs.write_all_bytes(&root.path().join("photos/2024/img.jpg"), b"x")
        .unwrap();
    fs.ensure_directory(&root.path().join("empty/nested"))
        .unwrap();

    let listing = fs.list_tree(root.path()).unwrap();

    assert_eq!(
        vec!["photos/2024/img.jpg".to_owned()],
        sorted(
            listing
                .files
                .iter()
                .map(|f| f.relative_path.clone())
                .collect()
        )
    );
    assert_eq!(
        vec![
            "empty".to_owned(),
            "empty/nested".to_owned(),
            "photos".to_owned(),
            "photos/2024".to_owned(),
        ],
        sorted(
            listing
                .directories
                .iter()
                .map(|d| d.relative_path.clone())
                .collect()
        ),
        "Mirror replicates empty directories, so the walk has to see them"
    );
}

#[test]
fn a_missing_root_fails_rather_than_reading_as_empty() {
    let root = temp();
    let fs = PhysicalFileSystem::new();

    let error = fs.list_tree(&root.path().join("gone")).unwrap_err();

    assert_eq!(std::io::ErrorKind::NotFound, error.kind());
    assert!(
        error
            .to_string()
            .contains("Folder not found or unavailable"),
        "an unplugged drive has to fail the run, not look like an empty source: {error}"
    );
}

#[test]
fn an_existing_but_empty_root_enumerates_empty() {
    let root = temp();
    let fs = PhysicalFileSystem::new();

    let listing = fs.list_tree(root.path()).unwrap();

    assert!(listing.files.is_empty());
    assert!(listing.directories.is_empty());
}

#[test]
fn a_stamp_survives_a_write_and_normalizes_to_whole_seconds() {
    let root = temp();
    let fs = PhysicalFileSystem::new();
    let file = root.path().join("a.txt");

    fs.write_all_bytes(&file, b"hello").unwrap();
    let when = Utc.with_ymd_and_hms(2026, 8, 9, 2, 0, 12).unwrap();
    fs.set_last_write_time_utc(&file, when).unwrap();

    let stamp = fs.get_stamp(&file).unwrap();
    assert_eq!(5, stamp.length);
    assert_eq!(when, stamp.last_write_time_utc);
}

#[test]
fn a_copy_that_shares_its_sources_time_shares_its_stamp() {
    let root = temp();
    let fs = PhysicalFileSystem::new();
    let source = root.path().join("src/a.txt");
    let copy = root.path().join("dst/a.txt");

    fs.write_all_bytes(&source, b"hello").unwrap();
    fs.write_all_bytes(&copy, b"hello").unwrap();
    let stamp = fs.get_stamp(&source).unwrap();
    fs.set_last_write_time_utc(&copy, stamp.last_write_time_utc)
        .unwrap();

    assert_eq!(
        stamp,
        fs.get_stamp(&copy).unwrap(),
        "otherwise the next run sees a change and re-copies the whole tree forever"
    );
}

#[test]
fn create_write_through_creates_parents_and_commits_its_bytes() {
    let root = temp();
    let fs = PhysicalFileSystem::new();
    let file = root.path().join("deep/nested/a.txt");

    let mut writer = fs.create_write_through(&file).unwrap();
    writer.write_all(b"hello").unwrap();
    writer.flush().unwrap();
    drop(writer);

    assert_eq!(b"hello".to_vec(), fs.read_all_bytes(&file).unwrap());
}

#[test]
fn replace_overwrites_the_destination_and_leaves_no_source() {
    let root = temp();
    let fs = PhysicalFileSystem::new();
    let temp_file = root.path().join("a.txt.syncmaid-tmp-1");
    let destination = root.path().join("sub/a.txt");

    fs.write_all_bytes(&temp_file, b"new").unwrap();
    fs.write_all_bytes(&destination, b"old").unwrap();

    fs.replace(&temp_file, &destination).unwrap();

    assert_eq!(b"new".to_vec(), fs.read_all_bytes(&destination).unwrap());
    assert!(!fs.file_exists(&temp_file));
}

#[test]
fn delete_empty_directory_leaves_an_occupied_or_absent_one_alone() {
    let root = temp();
    let fs = PhysicalFileSystem::new();
    let occupied = root.path().join("occupied");
    let empty = root.path().join("empty");

    fs.write_all_bytes(&occupied.join("a.txt"), b"a").unwrap();
    fs.ensure_directory(&empty).unwrap();

    fs.delete_empty_directory(&occupied).unwrap();
    fs.delete_empty_directory(&empty).unwrap();
    fs.delete_empty_directory(&root.path().join("never-existed"))
        .unwrap();

    assert!(
        occupied.is_dir(),
        "content that appeared since the caller decided is never taken along"
    );
    assert!(!empty.exists());
}

#[test]
fn a_directory_time_round_trips_and_a_missing_directory_is_a_no_op() {
    let root = temp();
    let fs = PhysicalFileSystem::new();
    let directory = root.path().join("sub");
    fs.ensure_directory(&directory).unwrap();

    let when = Utc.with_ymd_and_hms(2026, 3, 1, 12, 30, 0).unwrap();
    fs.set_directory_last_write_time_utc(&directory, when)
        .unwrap();

    let listing = fs.list_tree(root.path()).unwrap();
    assert_eq!(when, listing.directories[0].last_write_time_utc);

    fs.set_directory_last_write_time_utc(&root.path().join("gone"), when)
        .expect("a directory that is not there is left alone — the next run replans");
}

#[test]
fn deleting_a_file_that_is_not_there_is_a_no_op() {
    let root = temp();
    let fs = PhysicalFileSystem::new();
    fs.delete_file(&root.path().join("gone.txt")).unwrap();
}

#[test]
fn free_space_on_a_real_volume_is_a_usable_number() {
    let root = temp();
    let fs = PhysicalFileSystem::new();

    let available = fs.available_free_space(root.path());

    assert!(
        available > 0,
        "the preflight has to have something to compare against"
    );
}

#[test]
#[cfg(windows)]
fn a_really_locked_file_raises_the_failure_file_busy_recognizes() {
    use std::os::windows::fs::OpenOptionsExt;

    let root = temp();
    let fs = PhysicalFileSystem::new();
    let file = root.path().join("locked.txt");
    fs.write_all_bytes(&file, b"held").unwrap();

    // share_mode(0) is what a program writing the file holds it with.
    let _exclusive = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&file)
        .expect("take an exclusive handle");

    let error = fs.open_read(&file).map(|_| ()).unwrap_err();

    assert!(
        syncmaid_core::io::is_busy(&error),
        "a locked file must read as busy so the run defers it instead of failing: {error:?}"
    );
}

#[test]
#[cfg(windows)]
fn recycling_a_file_removes_it_and_recycling_a_missing_one_is_a_no_op() {
    let root = temp();
    let fs = PhysicalFileSystem::new();
    let file = root.path().join("bin-me.txt");
    fs.write_all_bytes(&file, b"x").unwrap();

    fs.recycle(&file).unwrap();
    assert!(!fs.file_exists(&file));

    fs.recycle(&root.path().join("gone.txt")).unwrap();
}

#[test]
fn a_relative_path_joins_back_onto_its_root() {
    let joined = syncmaid_core::io::join(Path::new(r"C:\src"), "photos/2024/img.jpg");
    assert_eq!(Path::new(r"C:\src\photos\2024\img.jpg"), joined);
}
