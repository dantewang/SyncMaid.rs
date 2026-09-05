//! The task-shape rules that need to see more than one task at a time.
//!
//! The engine checks everything it can from inside a single run, but it only ever sees one
//! task. "Two tasks never share a source" is not a fact about a task — it is a fact about the
//! list — so it lives here, where the whole list is, and is checked both in the editors (as a
//! hint that blocks saving) and again at run start (as a refusal that touches no files).
//!
//! Chaining is deliberately allowed: one task's destination may be another task's source. Runs
//! of the two converge, because triggers coalesce and planning is idempotent.

use std::path::Path;

use syncmaid_core::io::paths_overlap;
use syncmaid_core::model::SyncTask;
use uuid::Uuid;

/// Which task a path collides with, and how.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    /// The other task's name, for the sentence shown to the user.
    pub task_name: String,
}

/// The task, if any, whose **source** overlaps `source`.
///
/// Overlapping sources double-process the same input, which is fatal the moment one of them is
/// a Move: the second run finds the files already gone.
pub fn source_conflict(
    tasks: &[SyncTask],
    editing: Option<Uuid>,
    source: &str,
) -> Option<Conflict> {
    let candidate = Path::new(source);
    tasks
        .iter()
        .filter(|task| Some(task.id) != editing)
        .find(|task| paths_overlap(Path::new(&task.source_path), candidate))
        .map(|task| Conflict {
            task_name: task.name.clone(),
        })
}

/// The task, if any, one of whose **destinations** overlaps `destination`.
///
/// Overlapping destinations race on the same files: one task's Mirror deletes as orphans
/// whatever another just wrote there.
pub fn destination_conflict(
    tasks: &[SyncTask],
    editing_task: Option<Uuid>,
    editing_destination: Option<Uuid>,
    destination: &str,
) -> Option<Conflict> {
    let candidate = Path::new(destination);
    for task in tasks {
        for existing in &task.destinations {
            // The destination being edited is not its own conflict.
            if Some(existing.id) == editing_destination {
                continue;
            }
            // A whole task can be excluded when its own editor is checking its siblings
            // separately, which is what lets the two produce different wording.
            if Some(task.id) == editing_task && editing_destination.is_none() {
                continue;
            }
            if paths_overlap(Path::new(existing.local_path()), candidate) {
                return Some(Conflict {
                    task_name: task.name.clone(),
                });
            }
        }
    }
    None
}

/// The first pair of destinations *within one task* that overlap.
///
/// Same rule one level down, and the engine re-checks it at run start: a Mirror destination
/// deletes as orphans whatever the sibling writing into its subtree just put there.
pub fn sibling_conflict(task: &SyncTask) -> Option<(String, String)> {
    for (index, first) in task.destinations.iter().enumerate() {
        for second in &task.destinations[index + 1..] {
            if paths_overlap(
                Path::new(first.local_path()),
                Path::new(second.local_path()),
            ) {
                return Some((first.name.clone(), second.name.clone()));
            }
        }
    }
    None
}

/// The other task, if any, whose paths mean `task` must not run.
///
/// The editors already block a save that would create the overlap, but this rule is a fact
/// about the *list*, and the engine only ever sees one task — so it can re-check a task's own
/// shape at run start and cannot re-check this. Hand-edited config, and config written before
/// the editors enforced it, arrive here instead. Same-task destination overlap is left to the
/// engine, which names both destinations rather than a task.
pub fn run_conflict(tasks: &[SyncTask], task: &SyncTask) -> Option<Conflict> {
    source_conflict(tasks, Some(task.id), &task.source_path).or_else(|| {
        task.destinations.iter().find_map(|destination| {
            destination_conflict(tasks, Some(task.id), None, destination.local_path())
        })
    })
}

#[cfg(test)]
mod tests {
    use syncmaid_core::filtering::FilterRule;
    use syncmaid_core::model::{Destination, SyncStrategy};
    use syncmaid_core::triggers::Trigger;

    use super::*;

    fn task(name: &str, source: &str, destinations: &[&str]) -> SyncTask {
        SyncTask::new(
            name,
            source,
            Trigger::Manual,
            destinations
                .iter()
                .map(|path| {
                    Destination::new(*path, *path, [FilterRule::AllFiles], SyncStrategy::Mirror)
                })
                .collect::<Vec<_>>(),
        )
    }

    #[test]
    fn a_source_that_equals_or_nests_with_another_tasks_source_conflicts() {
        let tasks = vec![task("Photos", r"C:\Photos", &[])];

        assert_eq!(
            Some("Photos".to_owned()),
            source_conflict(&tasks, None, r"C:\Photos").map(|c| c.task_name)
        );
        assert_eq!(
            Some("Photos".to_owned()),
            source_conflict(&tasks, None, r"C:\Photos\2024").map(|c| c.task_name)
        );
        assert_eq!(
            Some("Photos".to_owned()),
            source_conflict(&tasks, None, r"C:\").map(|c| c.task_name),
            "containing another task's source is nesting too"
        );
    }

    #[test]
    fn a_sibling_folder_is_not_a_conflict() {
        let tasks = vec![task("Photos", r"C:\Photos", &[])];
        assert_eq!(None, source_conflict(&tasks, None, r"C:\Videos"));
    }

    #[test]
    fn a_task_does_not_conflict_with_itself_while_being_edited() {
        let tasks = vec![task("Photos", r"C:\Photos", &[])];
        let editing = tasks[0].id;

        assert_eq!(None, source_conflict(&tasks, Some(editing), r"C:\Photos"));
    }

    #[test]
    fn chaining_one_tasks_destination_into_anothers_source_is_allowed() {
        let tasks = vec![task("Sorter", r"C:\Downloads", &[r"D:\Filed"])];

        assert_eq!(
            None,
            source_conflict(&tasks, None, r"D:\Filed"),
            "task A files into a folder task B backs up: the runs converge, so this is a layout \
             the app supports rather than refuses"
        );
    }

    #[test]
    fn destinations_conflict_across_tasks() {
        let tasks = vec![task("Photos", r"C:\Photos", &[r"N:\backup"])];

        assert_eq!(
            Some("Photos".to_owned()),
            destination_conflict(&tasks, None, None, r"N:\backup\inner").map(|c| c.task_name)
        );
        assert_eq!(None, destination_conflict(&tasks, None, None, r"N:\other"));
    }

    #[test]
    fn a_destination_being_edited_is_not_its_own_conflict() {
        let tasks = vec![task("Photos", r"C:\Photos", &[r"N:\backup"])];
        let editing = tasks[0].destinations[0].id;

        assert_eq!(
            None,
            destination_conflict(&tasks, None, Some(editing), r"N:\backup")
        );
    }

    #[test]
    fn two_destinations_of_one_task_that_overlap_are_found() {
        let overlapping = task("Photos", r"C:\Photos", &[r"N:\backup", r"N:\backup\inner"]);
        let separate = task("Photos", r"C:\Photos", &[r"N:\one", r"N:\two"]);

        assert_eq!(
            Some((r"N:\backup".to_owned(), r"N:\backup\inner".to_owned())),
            sibling_conflict(&overlapping)
        );
        assert_eq!(None, sibling_conflict(&separate));
    }

    #[test]
    fn a_half_typed_path_conflicts_with_nothing() {
        let tasks = vec![task("Photos", r"C:\Photos", &[r"N:\backup"])];

        // The editor probes on every keystroke.
        for partial in ["", r"\\", r"\\server"] {
            assert_eq!(None, source_conflict(&tasks, None, partial), "{partial:?}");
            assert_eq!(
                None,
                destination_conflict(&tasks, None, None, partial),
                "{partial:?}"
            );
        }
    }

    #[test]
    fn a_run_is_refused_when_either_end_overlaps_another_task() {
        let photos = task("Photos", r"C:\Photos", &[r"N:\backup"]);

        let shared_source = task("Copy of photos", r"C:\Photos\2024", &[r"N:\elsewhere"]);
        assert_eq!(
            Some("Photos".to_owned()),
            run_conflict(&[photos.clone(), shared_source.clone()], &shared_source)
                .map(|c| c.task_name)
        );

        let shared_destination = task("Second backup", r"C:\Videos", &[r"N:\backup\inner"]);
        assert_eq!(
            Some("Photos".to_owned()),
            run_conflict(
                &[photos.clone(), shared_destination.clone()],
                &shared_destination
            )
            .map(|c| c.task_name)
        );
    }

    #[test]
    fn a_run_that_shares_nothing_is_not_refused() {
        let photos = task("Photos", r"C:\Photos", &[r"N:\backup"]);
        let videos = task("Videos", r"C:\Videos", &[r"N:\videos"]);

        assert_eq!(
            None,
            run_conflict(&[photos.clone(), videos.clone()], &videos)
        );
        assert_eq!(
            None,
            run_conflict(&[photos.clone(), videos], &photos),
            "a task in the list it is checked against is not its own conflict"
        );
    }

    #[test]
    fn a_run_of_a_chained_task_is_allowed() {
        let sorter = task("Sorter", r"C:\Downloads", &[r"D:\Filed"]);
        let backup = task("Backup", r"D:\Filed", &[r"N:\archive"]);

        assert_eq!(
            None,
            run_conflict(&[sorter.clone(), backup.clone()], &backup),
            "chaining is the layout the trigger coalescing and idempotent planning exist for"
        );
        assert_eq!(None, run_conflict(&[sorter.clone(), backup], &sorter));
    }

    #[test]
    fn two_destinations_of_one_task_are_left_to_the_engine() {
        let overlapping = task("Photos", r"C:\Photos", &[r"N:\backup", r"N:\backup\inner"]);

        assert_eq!(
            None,
            run_conflict(std::slice::from_ref(&overlapping), &overlapping),
            "the engine refuses this one, and names both destinations rather than a task"
        );
        assert!(sibling_conflict(&overlapping).is_some());
    }
}
