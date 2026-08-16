//! The only code in the engine that changes anything.
//!
//! Everything else reads and decides. Keeping the mutations behind one `match` is what makes
//! "no faster path skips the safety" checkable rather than aspirational.

use std::path::Path;
use std::sync::Arc;

use crate::io::{FileStamp, FileSystem};
use crate::sync::{DestinationProvider, LocalSourceFile, OperationError, SyncOperation};

/// Applies one planned operation.
pub fn apply(
    source_file_system: &Arc<dyn FileSystem>,
    provider: &dyn DestinationProvider,
    operation: &SyncOperation,
) -> Result<(), OperationError> {
    match operation {
        SyncOperation::Copy {
            relative_path,
            source_full_path,
            expected_stamp,
            verify,
        } => {
            let stamp =
                ensure_source_settled(source_file_system, source_full_path, *expected_stamp)?;
            let source = LocalSourceFile::new(
                Arc::clone(source_file_system),
                relative_path.clone(),
                source_full_path.clone(),
                stamp,
            );
            provider.write(relative_path, &source, *verify)
        }

        SyncOperation::Move {
            relative_path,
            source_full_path,
            expected_stamp,
            verify,
        } => {
            let stamp =
                ensure_source_settled(source_file_system, source_full_path, *expected_stamp)?;
            let source = LocalSourceFile::new(
                Arc::clone(source_file_system),
                relative_path.clone(),
                source_full_path.clone(),
                stamp,
            );
            provider.write(relative_path, &source, *verify)?;

            // The source is about to be deleted, so the copy is checked *again* against the
            // live source before that happens. The provider already verified its own write;
            // this catches the case where the two ends disagree for any other reason.
            let landed = provider.get_stamp(relative_path)?;
            let still_there = source_file_system.get_stamp(source_full_path)?;
            if landed != still_there {
                return Err(OperationError::verification(format!(
                    "Refusing to delete source '{}': destination does not match after copy.",
                    source_full_path.display()
                )));
            }

            source_file_system.delete_file(source_full_path)?;
            Ok(())
        }

        SyncOperation::CreateDirectory { relative_path } => {
            provider.ensure_directory(relative_path)?;
            Ok(())
        }

        SyncOperation::Delete {
            relative_path,
            mode,
        } => {
            provider.delete(relative_path, *mode)?;
            Ok(())
        }

        SyncOperation::DeleteDirectory { relative_path } => {
            provider.delete_empty_directory(relative_path)?;
            Ok(())
        }

        SyncOperation::SetDirectoryTimestamp {
            relative_path,
            last_write_time_utc,
        } => {
            provider.set_directory_last_write_time_utc(relative_path, *last_write_time_utc)?;
            Ok(())
        }
    }
}

/// Confirms the source still looks the way it did when the plan was made.
///
/// A mismatch means something is still writing it. Copying it now would land a half-written
/// file, so the run defers it — untouched destination, no failure, picked up next run.
fn ensure_source_settled(
    file_system: &Arc<dyn FileSystem>,
    source_full_path: &Path,
    expected: FileStamp,
) -> Result<FileStamp, OperationError> {
    let current = file_system.get_stamp(source_full_path)?;
    if current != expected {
        return Err(OperationError::SourceBusy {
            path: source_full_path.display().to_string(),
        });
    }
    Ok(current)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use chrono::Utc;

    use super::*;
    use crate::io::InMemoryFileSystem;
    use crate::model::DeleteMode;
    use crate::sync::LocalDestinationProvider;

    fn setup() -> (
        Arc<dyn FileSystem>,
        InMemoryFileSystem,
        LocalDestinationProvider,
    ) {
        let memory = InMemoryFileSystem::new();
        let file_system: Arc<dyn FileSystem> = Arc::new(memory.clone());
        let provider = LocalDestinationProvider::new(Arc::clone(&file_system), r"D:\dst");
        (file_system, memory, provider)
    }

    fn copy_of(memory: &InMemoryFileSystem, path: &str) -> SyncOperation {
        SyncOperation::Copy {
            relative_path: "a.txt".into(),
            source_full_path: PathBuf::from(path),
            expected_stamp: memory.get_stamp(Path::new(path)).unwrap(),
            verify: false,
        }
    }

    #[test]
    fn a_copy_lands_and_leaves_the_source_alone() {
        let (file_system, memory, provider) = setup();
        memory.add_file(r"C:\src\a.txt", b"hello");
        memory.add_directory(r"D:\dst");

        apply(&file_system, &provider, &copy_of(&memory, r"C:\src\a.txt")).unwrap();

        assert_eq!(Some(b"hello".to_vec()), memory.contents_of(r"D:\dst\a.txt"));
        assert_eq!(Some(b"hello".to_vec()), memory.contents_of(r"C:\src\a.txt"));
    }

    #[test]
    fn a_source_that_changed_since_planning_is_deferred_and_the_destination_untouched() {
        let (file_system, memory, provider) = setup();
        memory.add_file(r"C:\src\a.txt", b"hello");
        memory.add_file(r"D:\dst\a.txt", b"previous");
        let operation = copy_of(&memory, r"C:\src\a.txt");

        // Something is still writing it.
        memory.add_file(r"C:\src\a.txt", b"hello, more");

        let error = apply(&file_system, &provider, &operation).unwrap_err();

        assert!(matches!(error, OperationError::SourceBusy { .. }));
        assert!(
            error.is_busy(),
            "a busy source is a deferral, not a failure"
        );
        assert!(
            !error.is_transient(),
            "retrying would only race the writer again"
        );
        assert_eq!(
            Some(b"previous".to_vec()),
            memory.contents_of(r"D:\dst\a.txt")
        );
    }

    #[test]
    fn a_move_deletes_the_source_only_after_the_destination_verifies() {
        let (file_system, memory, provider) = setup();
        memory.add_file(r"C:\src\a.txt", b"hello");
        memory.add_directory(r"D:\dst");
        let stamp = memory.get_stamp(Path::new(r"C:\src\a.txt")).unwrap();

        apply(
            &file_system,
            &provider,
            &SyncOperation::Move {
                relative_path: "a.txt".into(),
                source_full_path: PathBuf::from(r"C:\src\a.txt"),
                expected_stamp: stamp,
                verify: true,
            },
        )
        .unwrap();

        assert_eq!(Some(b"hello".to_vec()), memory.contents_of(r"D:\dst\a.txt"));
        assert!(!memory.file_exists(Path::new(r"C:\src\a.txt")));
    }

    #[test]
    fn a_move_whose_copy_drifts_refuses_to_delete_the_source() {
        let (file_system, memory, provider) = setup();
        memory.add_file(r"C:\src\a.txt", b"hello");
        memory.add_directory(r"D:\dst");
        let stamp = memory.get_stamp(Path::new(r"C:\src\a.txt")).unwrap();
        // The copy lands with a timestamp that does not match its source.
        memory.with_faults(|faults| {
            faults.set_last_write_time_offset = chrono::Duration::seconds(90);
        });

        let error = apply(
            &file_system,
            &provider,
            &SyncOperation::Move {
                relative_path: "a.txt".into(),
                source_full_path: PathBuf::from(r"C:\src\a.txt"),
                expected_stamp: stamp,
                verify: false,
            },
        )
        .unwrap_err();

        assert!(
            matches!(&error, OperationError::Verification(message)
                if message.contains("Refusing to delete source")),
            "got {error:?}"
        );
        assert!(
            memory.file_exists(Path::new(r"C:\src\a.txt")),
            "the source survives anything short of a verified destination"
        );
    }

    #[test]
    fn directory_operations_go_through_the_provider() {
        let (file_system, memory, provider) = setup();
        memory.add_directory(r"D:\dst");
        let when = Utc::now();

        apply(
            &file_system,
            &provider,
            &SyncOperation::CreateDirectory {
                relative_path: "sub".into(),
            },
        )
        .unwrap();
        assert!(memory.directory_exists(r"D:\dst\sub"));

        apply(
            &file_system,
            &provider,
            &SyncOperation::SetDirectoryTimestamp {
                relative_path: "sub".into(),
                last_write_time_utc: when,
            },
        )
        .unwrap();

        apply(
            &file_system,
            &provider,
            &SyncOperation::DeleteDirectory {
                relative_path: "sub".into(),
            },
        )
        .unwrap();
        assert!(!memory.directory_exists(r"D:\dst\sub"));
    }

    #[test]
    fn a_delete_honours_the_destinations_mode() {
        let (file_system, memory, provider) = setup();
        memory.add_file(r"D:\dst\a.txt", b"x");

        apply(
            &file_system,
            &provider,
            &SyncOperation::Delete {
                relative_path: "a.txt".into(),
                mode: DeleteMode::Recycle,
            },
        )
        .unwrap();

        memory.observed(|observed| assert_eq!(1, observed.recycled.len()));
    }

    #[test]
    fn a_source_deleted_between_plan_and_apply_fails_without_pretending_it_is_busy() {
        let (file_system, memory, provider) = setup();
        memory.add_file(r"C:\src\a.txt", b"hello");
        memory.add_directory(r"D:\dst");
        let operation = copy_of(&memory, r"C:\src\a.txt");
        memory.delete_file(Path::new(r"C:\src\a.txt")).unwrap();

        let error = apply(&file_system, &provider, &operation).unwrap_err();

        assert!(!error.is_busy());
        assert!(
            !error.is_transient(),
            "it will still be missing on the next attempt"
        );
    }
}
