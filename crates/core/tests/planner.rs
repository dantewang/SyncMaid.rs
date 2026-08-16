//! Planning is pure and its order is load-bearing.
//!
//! Mirror's operation order is not cosmetic: creates before copies, file deletes before the
//! directory deletes that they empty, directory times last of all. These assertions are what
//! stop a "tidier" reordering from breaking tree identity.

mod common;

use common::*;
use syncmaid_core::filtering::FilterRule;
use syncmaid_core::model::{DeleteMode, FileNameCollisionPolicy};
use syncmaid_core::sync::SyncOperation;

#[test]
fn planning_does_not_mutate_the_filesystem() {
    let fs = memory();
    fs.add_file(r"C:\src\a.txt", b"a");
    fs.add_file(r"D:\dst\orphan.txt", b"o");
    let before = fs.all_paths();

    plan_for(&fs, &mirror("D", DESTINATION));

    assert_eq!(before, fs.all_paths());
}

#[test]
fn mirror_creates_parents_first_and_empty_directories_too() {
    let fs = memory();
    fs.add_directory(r"C:\src\empty\nested");
    fs.add_directory(DESTINATION);

    let plan = plan_for(&fs, &mirror("D", DESTINATION));

    assert_eq!(
        vec![
            "mkdir empty",
            "mkdir empty/nested",
            "time empty",
            "time empty/nested"
        ],
        describe(&plan.operations),
        "Mirror's contract is tree identity, so an empty directory is part of the tree"
    );
}

#[test]
fn mirror_skips_creates_for_directories_its_copies_will_make() {
    let fs = memory();
    fs.add_file(r"C:\src\photos\2024\a.jpg", b"a");
    fs.add_directory(DESTINATION);

    let plan = plan_for(&fs, &mirror("D", DESTINATION));

    assert!(
        !describe(&plan.operations)
            .iter()
            .any(|step| step.starts_with("mkdir")),
        "a copy creates its own parents; planning them again is pure noise: {:?}",
        describe(&plan.operations)
    );
}

#[test]
fn mirror_deletes_files_before_the_directories_they_empty_and_children_before_parents() {
    let fs = memory();
    fs.add_directory(SOURCE);
    fs.add_file(r"D:\dst\gone\deep\a.txt", b"a");
    fs.add_file(r"D:\dst\gone\b.txt", b"b");

    // An empty source would trip the deletion guard at run time; here we only plan.
    let plan = plan_for(&fs, &mirror("D", DESTINATION));

    assert_eq!(
        vec![
            "rm gone/b.txt",
            "rm gone/deep/a.txt",
            "rmdir gone/deep",
            "rmdir gone",
        ],
        describe(&plan.operations)
    );
}

#[test]
fn mirror_never_plans_the_destination_root_for_removal() {
    let fs = memory();
    fs.add_directory(SOURCE);
    fs.add_file(r"D:\dst\a.txt", b"a");

    let plan = plan_for(&fs, &mirror("D", DESTINATION));

    assert!(!describe(&plan.operations).contains(&"rmdir ".to_owned()));
    assert!(plan
        .operations
        .iter()
        .all(|op| !op.relative_path().is_empty()));
}

#[test]
fn mirror_repairs_a_drifted_directory_time_and_resets_the_ones_its_operations_bump() {
    let fs = memory();
    fs.add_directory_at(r"C:\src\drifted", at(10));
    fs.add_directory_at(r"C:\src\bumped", at(20));
    fs.add_file(r"C:\src\bumped\new.txt", b"n");
    fs.add_directory_at(r"C:\src\settled", at(30));

    fs.add_directory_at(r"D:\dst\drifted", at(11));
    fs.add_directory_at(r"D:\dst\bumped", at(20));
    fs.add_directory_at(r"D:\dst\settled", at(30));

    let plan = plan_for(&fs, &mirror("D", DESTINATION));
    let timestamped: Vec<&str> = plan
        .operations
        .iter()
        .filter_map(|op| match op {
            SyncOperation::SetDirectoryTimestamp { relative_path, .. } => {
                Some(relative_path.as_str())
            }
            _ => None,
        })
        .collect();

    assert_eq!(
        vec!["bumped", "drifted"],
        sorted(timestamped.iter().map(|s| (*s).to_owned()).collect()),
        "a directory whose time already matches and that nothing touched is left alone"
    );
}

#[test]
fn mirror_sets_directory_times_last_of_all() {
    let fs = memory();
    fs.add_directory_at(r"C:\src\sub", at(10));
    fs.add_file(r"C:\src\sub\a.txt", b"a");
    fs.add_directory(DESTINATION);

    let plan = plan_for(&fs, &mirror("D", DESTINATION));
    let steps = describe(&plan.operations);

    let last_time = steps
        .iter()
        .position(|step| step.starts_with("time"))
        .unwrap();
    assert!(
        steps[last_time..].iter().all(|step| step.starts_with("time")),
        "entry changes bump their parent's time, so the timestamp pass has to come after them: {steps:?}"
    );
}

#[test]
fn mirror_deletes_go_to_the_destinations_delete_mode() {
    let fs = memory();
    fs.add_file(r"C:\src\keep.txt", b"k");
    fs.add_file(r"D:\dst\orphan.txt", b"o");
    let mut destination = mirror("D", DESTINATION);
    destination.delete_mode = DeleteMode::Permanent;

    let plan = plan_for(&fs, &destination);

    assert!(plan.operations.iter().any(|op| matches!(
        op,
        SyncOperation::Delete {
            mode: DeleteMode::Permanent,
            ..
        }
    )));
}

#[test]
fn add_only_never_deletes_and_never_touches_directories() {
    let fs = memory();
    fs.add_file(r"C:\src\a.txt", b"a");
    fs.add_directory(r"C:\src\empty");
    fs.add_file(r"D:\dst\orphan.txt", b"o");
    fs.add_directory(r"D:\dst\stale");

    let plan = plan_for(&fs, &add_only("D", DESTINATION, vec![FilterRule::AllFiles]));

    assert_eq!(vec!["copy a.txt"], describe(&plan.operations));
    assert_eq!(
        0, plan.destination_file_count,
        "Add-only never deletes, so it never counts"
    );
}

#[test]
fn add_only_reads_only_candidate_stamps_and_does_not_walk_the_destination() {
    let fs = memory();
    fs.add_file(r"C:\src\a.txt", b"a");
    fs.add_file(r"C:\src\b.txt", b"b");
    for index in 0..50 {
        fs.add_file(format!(r"D:\dst\unrelated{index}.txt"), b"x");
    }

    plan_for(&fs, &add_only("D", DESTINATION, vec![FilterRule::AllFiles]));

    assert_eq!(
        0,
        fs.tree_walks_of(DESTINATION),
        "a destination holding thousands of unrelated files must not be enumerated"
    );
}

#[test]
fn mirror_plans_from_one_destination_walk_without_per_file_stamp_calls() {
    let fs = memory();
    for index in 0..20 {
        fs.add_file(format!(r"C:\src\f{index}.txt"), b"x");
        fs.add_file(format!(r"D:\dst\f{index}.txt"), b"x");
    }
    let before = fs.observed(|observed| observed.get_stamp_calls);

    plan_for(&fs, &mirror("D", DESTINATION));

    assert_eq!(1, fs.tree_walks_of(DESTINATION));
    assert_eq!(
        before,
        fs.observed(|observed| observed.get_stamp_calls),
        "the walk already carried every stamp; asking again is a round trip per file"
    );
}

#[test]
fn an_unchanged_file_is_not_re_copied() {
    let fs = memory();
    fs.add_file_at(r"C:\src\a.txt", b"same", at(10));
    fs.add_file_at(r"D:\dst\a.txt", b"same", at(10));

    let plan = plan_for(&fs, &mirror("D", DESTINATION));

    assert!(
        !describe(&plan.operations)
            .iter()
            .any(|step| step.starts_with("copy")),
        "size and whole-second mtime agree, so there is nothing to do"
    );
}

#[test]
fn a_file_that_differs_in_size_or_time_is_copied() {
    let fs = memory();
    fs.add_file_at(r"C:\src\size.txt", b"longer", at(10));
    fs.add_file_at(r"D:\dst\size.txt", b"short", at(10));
    fs.add_file_at(r"C:\src\time.txt", b"same", at(20));
    fs.add_file_at(r"D:\dst\time.txt", b"same", at(10));

    let plan = plan_for(&fs, &mirror("D", DESTINATION));

    assert_eq!(
        vec!["copy size.txt".to_owned(), "copy time.txt".to_owned()],
        sorted(
            describe(&plan.operations)
                .into_iter()
                .filter(|step| step.starts_with("copy"))
                .collect()
        )
    );
}

#[test]
fn a_move_keeps_the_source_structure_by_default() {
    let fs = memory();
    fs.add_file(r"C:\src\2026\a.pdf", b"a");

    let plan = plan_for(&fs, &moving("D", DESTINATION, vec![FilterRule::AllFiles]));

    assert_eq!(vec!["move 2026/a.pdf"], describe(&plan.operations));
}

#[test]
fn a_flattening_move_drops_the_folders_a_file_happened_to_sit_in() {
    let fs = memory();
    fs.add_file(r"C:\src\2026\a.pdf", b"a");
    let mut destination = moving("D", DESTINATION, vec![FilterRule::AllFiles]);
    destination.flatten_structure = true;

    let plan = plan_for(&fs, &destination);

    assert_eq!(vec!["move a.pdf"], describe(&plan.operations));
}

#[test]
fn a_flattened_collision_is_skipped_by_default_with_nothing_renamed_or_overwritten() {
    let fs = memory();
    fs.add_file(r"C:\src\2025\report.pdf", b"old");
    fs.add_file(r"C:\src\2026\report.pdf", b"new");
    let mut destination = moving("D", DESTINATION, vec![FilterRule::AllFiles]);
    destination.flatten_structure = true;

    let plan = plan_for(&fs, &destination);

    assert_eq!(vec!["move report.pdf"], describe(&plan.operations));
    assert_eq!(
        1,
        plan.skipped_collisions.len(),
        "the loser stays in the source and is reported"
    );
}

#[test]
fn a_flattened_collision_can_take_a_numbered_name_instead() {
    let fs = memory();
    fs.add_file(r"C:\src\2025\report.pdf", b"old");
    fs.add_file(r"C:\src\2026\report.pdf", b"new");
    fs.add_file(r"D:\dst\report.pdf", b"already there");
    let mut destination = moving("D", DESTINATION, vec![FilterRule::AllFiles]);
    destination.flatten_structure = true;
    destination.collision_policy = FileNameCollisionPolicy::Suffix;

    let plan = plan_for(&fs, &destination);

    assert_eq!(
        vec!["move report (2).pdf", "move report (3).pdf"],
        describe(&plan.operations),
        "numbering steps past both the existing file and the earlier claim in this plan"
    );
    assert!(plan.skipped_collisions.is_empty());
}
