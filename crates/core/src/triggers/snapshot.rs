//! What a polling watcher compares between rounds.

use std::collections::{HashMap, HashSet};

use crate::io::{FileStamp, TreeListing};

/// A comparable view of a tree: which files with which stamps, and which directories exist.
///
/// Directory *times* are deliberately left out. A run that copies a file bumps its parent
/// directory's time, so including them would make the poll after every run look like a fresh
/// change and fire again — forever.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TreeSnapshot {
    files: HashMap<String, FileStamp>,
    directories: HashSet<String>,
}

impl TreeSnapshot {
    pub fn of(listing: &TreeListing) -> Self {
        Self {
            files: listing
                .files
                .iter()
                .map(|file| (fold(&file.relative_path), file.stamp))
                .collect(),
            directories: listing
                .directories
                .iter()
                .map(|directory| fold(&directory.relative_path))
                .collect(),
        }
    }

    /// True when nothing at all is in the tree.
    pub fn is_empty(&self) -> bool {
        self.files.is_empty() && self.directories.is_empty()
    }
}

fn fold(relative_path: &str) -> String {
    relative_path.to_uppercase()
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::*;
    use crate::io::{ListedDirectory, ListedFile};

    fn listing(files: &[(&str, u64)], directories: &[&str]) -> TreeListing {
        let when = Utc.with_ymd_and_hms(2026, 8, 9, 2, 0, 0).unwrap();
        TreeListing {
            files: files
                .iter()
                .map(|(path, length)| ListedFile {
                    relative_path: (*path).into(),
                    stamp: FileStamp::new(*length, when),
                })
                .collect(),
            directories: directories
                .iter()
                .map(|path| ListedDirectory {
                    relative_path: (*path).into(),
                    last_write_time_utc: when,
                })
                .collect(),
        }
    }

    #[test]
    fn an_unchanged_tree_compares_equal() {
        let one = TreeSnapshot::of(&listing(&[("a.txt", 3)], &["sub"]));
        let two = TreeSnapshot::of(&listing(&[("a.txt", 3)], &["sub"]));

        assert_eq!(one, two);
    }

    #[test]
    fn a_changed_size_a_new_file_or_a_new_directory_all_show_up() {
        let baseline = TreeSnapshot::of(&listing(&[("a.txt", 3)], &["sub"]));

        assert_ne!(
            baseline,
            TreeSnapshot::of(&listing(&[("a.txt", 4)], &["sub"]))
        );
        assert_ne!(
            baseline,
            TreeSnapshot::of(&listing(&[("a.txt", 3), ("b.txt", 1)], &["sub"]))
        );
        assert_ne!(
            baseline,
            TreeSnapshot::of(&listing(&[("a.txt", 3)], &["sub", "other"]))
        );
        assert_ne!(baseline, TreeSnapshot::of(&listing(&[], &["sub"])));
    }

    #[test]
    fn a_directory_whose_time_moved_is_not_a_change() {
        // A run that copies a file bumps its parent's time. Counting that as a change would
        // make the poll after every run fire again, forever.
        let before = listing(&[("sub/a.txt", 3)], &["sub"]);
        let mut after = listing(&[("sub/a.txt", 3)], &["sub"]);
        after.directories[0].last_write_time_utc =
            Utc.with_ymd_and_hms(2026, 8, 9, 3, 0, 0).unwrap();

        assert_eq!(TreeSnapshot::of(&before), TreeSnapshot::of(&after));
    }

    #[test]
    fn paths_compare_case_insensitively() {
        assert_eq!(
            TreeSnapshot::of(&listing(&[("Photos/A.JPG", 1)], &["Photos"])),
            TreeSnapshot::of(&listing(&[("photos/a.jpg", 1)], &["photos"]))
        );
    }
}
