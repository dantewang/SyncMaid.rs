//! End-to-end runs, and the guards that stop one.

mod common;

use std::collections::HashSet;
use std::path::Path;

use common::*;
use syncmaid_core::filtering::FilterRule;
use syncmaid_core::io::FileSystem;
use syncmaid_core::model::{DeleteMode, Destination, SyncOutcome, SyncTask, SyncTaskKind};
use syncmaid_core::sync::{CancellationToken, MirrorDeletePreview};
use syncmaid_core::triggers::Trigger;

// -------------------------------------------------------------------------------------------
// Strategies
// -------------------------------------------------------------------------------------------

#[test]
fn a_mirror_run_makes_the_two_trees_identical_empty_directories_included() {
    let fs = memory();
    fs.add_file(r"C:\src\photos\a.jpg", b"a");
    fs.add_directory(r"C:\src\empty");
    fs.add_file(r"D:\dst\orphan.txt", b"o");
    fs.add_directory(r"D:\dst\stale");
    let task = sync_task(vec![mirror("D", DESTINATION)]);

    let statuses = run(&engine(&fs), &task);

    assert_eq!(SyncOutcome::Success, statuses[0].outcome);
    let source = fs.list_tree(Path::new(SOURCE)).unwrap();
    let destination = fs.list_tree(Path::new(DESTINATION)).unwrap();
    assert_eq!(file_paths(&source), file_paths(&destination));
    assert_eq!(directory_paths(&source), directory_paths(&destination));
}

#[test]
fn a_second_mirror_run_over_an_unchanged_tree_copies_nothing() {
    let fs = memory();
    fs.add_file(r"C:\src\a.txt", b"a");
    let task = sync_task(vec![mirror("D", DESTINATION)]);
    let engine = engine(&fs);

    assert_eq!(1, run(&engine, &task)[0].files_copied);
    assert_eq!(
        0,
        run(&engine, &task)[0].files_copied,
        "planning is idempotent, which is what makes a self-triggered follow-up a no-op"
    );
}

#[test]
fn add_only_copies_what_matches_and_never_deletes() {
    let fs = memory();
    fs.add_file(r"C:\src\a.jpg", b"a");
    fs.add_file(r"C:\src\b.txt", b"b");
    fs.add_file(r"D:\dst\kept-by-hand.txt", b"mine");
    let task = sync_task(vec![add_only(
        "D",
        DESTINATION,
        vec![FilterRule::extension("jpg")],
    )]);

    let statuses = run(&engine(&fs), &task);

    assert_eq!(SyncOutcome::Success, statuses[0].outcome);
    assert_eq!(1, statuses[0].files_copied);
    assert!(fs.file_exists(Path::new(r"D:\dst\a.jpg")));
    assert!(
        !fs.file_exists(Path::new(r"D:\dst\b.txt")),
        "the filter selected only jpgs"
    );
    assert!(
        fs.file_exists(Path::new(r"D:\dst\kept-by-hand.txt")),
        "Add-only is the safe accumulator: it never removes anything"
    );
}

#[test]
fn a_move_run_empties_the_source_and_cleans_up_the_folders_it_emptied() {
    let fs = memory();
    fs.add_file(r"C:\src\2026\a.pdf", b"a");
    fs.add_directory(r"C:\src\already-empty");
    let task = move_task(vec![moving("D", DESTINATION, vec![FilterRule::AllFiles])]);

    let statuses = run(&engine(&fs), &task);

    assert_eq!(SyncOutcome::Success, statuses[0].outcome);
    assert!(fs.file_exists(Path::new(r"D:\dst\2026\a.pdf")));
    assert!(!fs.file_exists(Path::new(r"C:\src\2026\a.pdf")));
    assert!(
        !fs.directory_exists(r"C:\src\2026"),
        "a watched inbox would collect these forever"
    );
    assert!(
        fs.directory_exists(r"C:\src\already-empty"),
        "a folder that was already empty holds no moved file and is never a candidate"
    );
    assert!(
        fs.directory_exists(SOURCE),
        "the source folder itself is never removed"
    );
}

#[test]
fn a_move_task_routes_each_file_to_the_first_rule_that_matches_it() {
    let fs = memory();
    fs.add_file(r"C:\src\a.pdf", b"a");
    fs.add_file(r"C:\src\invoices\b.pdf", b"b");
    fs.add_file(r"C:\src\notes.txt", b"n");
    let task = move_task(vec![
        moving("Books", r"D:\books", vec![FilterRule::extension("pdf")]),
        moving("Bills", r"D:\bills", vec![FilterRule::path("invoices")]),
    ]);

    run(&engine(&fs), &task);

    assert!(fs.file_exists(Path::new(r"D:\books\a.pdf")));
    assert!(
        fs.file_exists(Path::new(r"D:\books\invoices\b.pdf")),
        "the pdf rule is listed first, so it takes the contested file"
    );
    assert!(!fs.file_exists(Path::new(r"D:\bills\invoices\b.pdf")));
    assert!(
        fs.file_exists(Path::new(r"C:\src\notes.txt")),
        "nothing matched it, so it stays"
    );
}

#[test]
fn several_destinations_each_get_their_own_status() {
    let fs = memory();
    fs.add_file(r"C:\src\a.jpg", b"a");
    fs.add_file(r"C:\src\b.raw", b"b");
    let task = sync_task(vec![
        mirror("Full", r"E:\full"),
        add_only("Raws", r"N:\raws", vec![FilterRule::extension("raw")]),
    ]);

    let statuses = run(&engine(&fs), &task);

    assert_eq!(2, statuses.len());
    assert_eq!(2, statuses[0].files_copied);
    assert_eq!(1, statuses[1].files_copied);
}

// -------------------------------------------------------------------------------------------
// Task shape guards — each refuses before a single file is touched
// -------------------------------------------------------------------------------------------

#[test]
fn a_task_mixing_move_with_a_copying_strategy_fails_every_destination() {
    let fs = memory();
    fs.add_file(r"C:\src\a.txt", b"a");
    let task = move_task_with_a_mirror();

    let statuses = run(&engine(&fs), &task);

    assert!(statuses
        .iter()
        .all(|status| status.outcome == SyncOutcome::Failed));
    assert!(statuses[0]
        .error
        .as_ref()
        .unwrap()
        .contains("A Move task takes Move destinations only"));
    assert!(
        fs.all_paths()
            .iter()
            .all(|path| path.starts_with(r"C:\src")),
        "no file was touched"
    );
}

#[test]
fn a_sync_task_holding_a_move_destination_fails_every_destination() {
    let fs = memory();
    fs.add_file(r"C:\src\a.txt", b"a");
    let task = sync_task(vec![moving("D", DESTINATION, vec![FilterRule::AllFiles])]);

    let statuses = run(&engine(&fs), &task);

    assert_eq!(SyncOutcome::Failed, statuses[0].outcome);
    assert!(statuses[0]
        .error
        .as_ref()
        .unwrap()
        .contains("A Sync task takes Mirror and Add-only destinations only"));
}

#[test]
fn two_destinations_of_one_task_that_overlap_fail_the_whole_run() {
    let fs = memory();
    fs.add_file(r"C:\src\a.txt", b"a");
    let task = sync_task(vec![
        mirror("Outer", r"D:\dst"),
        add_only("Inner", r"D:\dst\inside", vec![FilterRule::AllFiles]),
    ]);

    let statuses = run(&engine(&fs), &task);

    assert!(statuses
        .iter()
        .all(|status| status.outcome == SyncOutcome::Failed));
    assert!(statuses[0]
        .error
        .as_ref()
        .unwrap()
        .contains("are the same folder or nested in one another"));
    assert!(!fs.file_exists(Path::new(r"D:\dst\a.txt")));
}

#[test]
fn a_destination_nested_in_its_source_is_refused_in_either_direction() {
    for (source, destination_path) in [(SOURCE, r"C:\src\backup"), (r"C:\src\inner", r"C:\src")] {
        let fs = memory();
        fs.add_file(format!(r"{source}\a.txt"), b"a");
        let task = sync_task_from(source, mirror("D", destination_path));

        let statuses = run(&engine(&fs), &task);

        assert_eq!(
            SyncOutcome::Failed,
            statuses[0].outcome,
            "{source} → {destination_path}"
        );
        assert!(statuses[0]
            .error
            .as_ref()
            .unwrap()
            .contains("Destination must be a separate folder outside the source"));
    }
}

#[test]
fn a_mirror_destination_carrying_file_filters_is_refused() {
    let fs = memory();
    fs.add_file(r"C:\src\a.jpg", b"a");
    // Only a hand-edited config can produce this — the editor hides the filter section.
    let mut destination = mirror("D", DESTINATION);
    destination.filters = vec![FilterRule::extension("jpg")];
    let task = sync_task(vec![destination]);

    let statuses = run(&engine(&fs), &task);

    assert_eq!(SyncOutcome::Failed, statuses[0].outcome);
    assert!(statuses[0]
        .error
        .as_ref()
        .unwrap()
        .contains("Mirror replicates the whole source tree"));
}

#[test]
fn a_mirror_destination_with_no_filters_or_a_duplicated_one_is_also_refused() {
    for filters in [vec![], vec![FilterRule::AllFiles, FilterRule::AllFiles]] {
        let fs = memory();
        fs.add_file(r"C:\src\a.jpg", b"a");
        let mut destination = mirror("D", DESTINATION);
        destination.filters = filters.clone();
        let task = sync_task(vec![destination]);

        let statuses = run(&engine(&fs), &task);

        assert_eq!(
            SyncOutcome::Failed,
            statuses[0].outcome,
            "filters: {filters:?}"
        );
    }
}

// -------------------------------------------------------------------------------------------
// Mirror deletion guards
// -------------------------------------------------------------------------------------------

#[test]
fn an_empty_source_deletes_nothing_and_cannot_be_overridden() {
    let fs = memory();
    fs.add_directory(SOURCE);
    for index in 0..20 {
        fs.add_file(format!(r"D:\dst\f{index}.txt"), b"x");
    }
    let destination = mirror("D", DESTINATION);
    let mut confirmed = HashSet::new();
    confirmed.insert(destination.id);
    let task = sync_task(vec![destination]);

    let statuses = run_confirming(&engine(&fs), &task, &confirmed);

    assert_eq!(SyncOutcome::Failed, statuses[0].outcome);
    assert!(statuses[0]
        .error
        .as_ref()
        .unwrap()
        .contains("Source is empty or unavailable"));
    assert_eq!(
        20,
        fs.all_paths().len(),
        "not one file went, even with a confirmation in hand"
    );
}

#[test]
fn a_missing_source_fails_the_run_rather_than_mirroring_nothing() {
    let fs = memory();
    for index in 0..20 {
        fs.add_file(format!(r"D:\dst\f{index}.txt"), b"x");
    }
    // The source root was never created — an unplugged drive.
    let task = sync_task(vec![mirror("D", DESTINATION)]);

    let statuses = run(&engine(&fs), &task);

    assert_eq!(SyncOutcome::Failed, statuses[0].outcome);
    assert!(statuses[0]
        .error
        .as_ref()
        .unwrap()
        .contains("Folder not found or unavailable"));
    assert_eq!(20, fs.all_paths().len());
}

#[test]
fn a_mass_deletion_holds_the_run_and_applies_nothing() {
    let fs = memory();
    fs.add_file(r"C:\src\keep.txt", b"k");
    for index in 0..20 {
        fs.add_file(format!(r"D:\dst\f{index}.txt"), b"x");
    }
    let task = sync_task(vec![mirror("D", DESTINATION)]);

    let statuses = run(&engine(&fs), &task);

    assert_eq!(SyncOutcome::NeedsConfirmation, statuses[0].outcome);
    assert!(statuses[0]
        .error
        .as_ref()
        .unwrap()
        .contains("Would delete 20 files"));
    assert_eq!(
        20,
        fs.all_paths().len() - 1,
        "zero operations applied, not even the copy"
    );
}

#[test]
fn a_confirmed_mass_deletion_goes_ahead_for_that_run_only() {
    let fs = memory();
    fs.add_file(r"C:\src\keep.txt", b"k");
    for index in 0..20 {
        fs.add_file(format!(r"D:\dst\f{index}.txt"), b"x");
    }
    let destination = mirror("D", DESTINATION);
    let mut confirmed = HashSet::new();
    confirmed.insert(destination.id);
    let task = sync_task(vec![destination]);

    let statuses = run_confirming(&engine(&fs), &task, &confirmed);

    assert_eq!(SyncOutcome::Success, statuses[0].outcome);
    assert_eq!(
        vec!["keep.txt".to_owned()],
        file_paths(&fs.list_tree(Path::new(DESTINATION)).unwrap())
    );
}

#[test]
fn turning_the_ratio_guard_off_lets_a_mass_deletion_through() {
    let fs = memory();
    fs.add_file(r"C:\src\keep.txt", b"k");
    for index in 0..20 {
        fs.add_file(format!(r"D:\dst\f{index}.txt"), b"x");
    }
    let mut destination = mirror("D", DESTINATION);
    destination.mass_delete_threshold = 0.0;
    let task = sync_task(vec![destination]);

    let statuses = run(&engine(&fs), &task);

    assert_eq!(SyncOutcome::Success, statuses[0].outcome);
}

#[test]
fn mirror_deletions_go_to_the_recycle_bin_by_default_and_permanently_on_request() {
    for (mode, recycled, permanent) in [(DeleteMode::Recycle, 1, 0), (DeleteMode::Permanent, 0, 1)]
    {
        let fs = memory();
        fs.add_file(r"C:\src\keep.txt", b"k");
        fs.add_file(r"D:\dst\keep.txt", b"k");
        fs.add_file(r"D:\dst\orphan.txt", b"o");
        let mut destination = mirror("D", DESTINATION);
        destination.delete_mode = mode;
        let task = sync_task(vec![destination]);

        run(&engine(&fs), &task);

        fs.observed(|observed| {
            assert_eq!(recycled, observed.recycled.len(), "{mode:?}");
        });
        let _ = permanent;
        assert!(!fs.file_exists(Path::new(r"D:\dst\orphan.txt")));
    }
}

#[test]
fn a_mirror_deletion_on_a_network_destination_falls_back_to_a_permanent_delete() {
    let fs = memory();
    fs.add_file(r"C:\src\keep.txt", b"k");
    fs.add_file(r"\\server\share\keep.txt", b"k");
    fs.add_file(r"\\server\share\orphan.txt", b"o");
    let task = sync_task(vec![mirror("D", r"\\server\share")]);

    run(&engine(&fs), &task);

    fs.observed(|observed| {
        assert!(observed.recycled.is_empty());
        assert_eq!(
            1,
            observed.recycle_fell_back_to_permanent.len(),
            "shares have no Recycle Bin"
        );
    });
}

#[test]
fn the_deletion_preview_lists_a_bounded_sample() {
    let fs = memory();
    fs.add_file(r"C:\src\keep.txt", b"k");
    for index in 0..40 {
        fs.add_file(format!(r"D:\dst\f{index:02}.txt"), b"x");
    }
    let destination = mirror("D", DESTINATION);
    let destination_id = destination.id;
    let task = sync_task(vec![destination]);

    let preview = engine(&fs).preview_mirror_deletions(&task, destination_id);

    assert_eq!(40, preview.count);
    assert_eq!(MirrorDeletePreview::SAMPLE_SIZE, preview.sample.len());
}

#[test]
fn the_deletion_preview_degrades_to_nothing_when_the_source_vanished() {
    let fs = memory();
    for index in 0..40 {
        fs.add_file(format!(r"D:\dst\f{index}.txt"), b"x");
    }
    let destination = mirror("D", DESTINATION);
    let destination_id = destination.id;
    let task = sync_task(vec![destination]);

    let preview = engine(&fs).preview_mirror_deletions(&task, destination_id);

    assert!(
        preview.is_empty(),
        "the follow-up run surfaces the real failure, not the preview"
    );
}

// -------------------------------------------------------------------------------------------
// Busy files, failures and cancellation
// -------------------------------------------------------------------------------------------

#[test]
fn a_locked_source_file_is_deferred_and_the_rest_of_the_run_carries_on() {
    let fs = memory();
    fs.add_file(r"C:\src\locked.txt", b"l");
    fs.add_file(r"C:\src\fine.txt", b"f");
    fs.lock_path(r"C:\src\locked.txt");
    let task = sync_task(vec![add_only("D", DESTINATION, vec![FilterRule::AllFiles])]);

    // The shipping engine, backoff included: a sharing violation is retried first, and only a
    // file that is *still* locked afterwards becomes a deferral.
    let statuses = run(&engine_with_default_retry(&fs), &task);

    assert_eq!(SyncOutcome::Incomplete, statuses[0].outcome);
    assert_eq!(1, statuses[0].files_copied);
    assert_eq!(1, statuses[0].files_deferred);
    assert!(statuses[0].error.is_none(), "a file in use is not an error");
    assert!(fs.file_exists(Path::new(r"D:\dst\fine.txt")));
}

#[test]
fn a_failure_names_the_file_and_the_verb_and_lets_the_others_finish() {
    let fs = memory();
    fs.add_file(r"C:\src\bad.txt", b"b");
    fs.add_file(r"C:\src\good.txt", b"g");
    fs.with_faults(|faults| faults.fail_write_path_fragment = Some("bad.txt".into()));
    let task = sync_task(vec![add_only("D", DESTINATION, vec![FilterRule::AllFiles])]);

    let statuses = run(&engine(&fs), &task);

    assert_eq!(SyncOutcome::Failed, statuses[0].outcome);
    assert!(statuses[0]
        .error
        .as_ref()
        .unwrap()
        .starts_with("Failed to copy 'bad.txt'"));
    assert_eq!(1, statuses[0].files_copied, "the copied count stays honest");
    assert!(fs.file_exists(Path::new(r"D:\dst\good.txt")));
}

#[test]
fn a_run_with_both_a_failure_and_a_deferral_reports_failed() {
    let fs = memory();
    fs.add_file(r"C:\src\bad.txt", b"b");
    fs.add_file(r"C:\src\locked.txt", b"l");
    fs.lock_path(r"C:\src\locked.txt");
    fs.with_faults(|faults| faults.fail_write_path_fragment = Some("bad.txt".into()));
    let task = sync_task(vec![add_only("D", DESTINATION, vec![FilterRule::AllFiles])]);

    let statuses = run(&engine(&fs), &task);

    assert_eq!(
        SyncOutcome::Failed,
        statuses[0].outcome,
        "a real failure outranks a deferral"
    );
    assert_eq!(1, statuses[0].files_deferred);
}

#[test]
fn a_destination_failing_everything_is_abandoned_early() {
    let fs = memory();
    for index in 0..40 {
        fs.add_file(format!(r"C:\src\f{index:02}.txt"), b"x");
    }
    fs.with_faults(|faults| faults.fail_writes = true);
    let task = sync_task(vec![add_only("D", DESTINATION, vec![FilterRule::AllFiles])]);

    let statuses = run(&engine(&fs), &task);

    let error = statuses[0].error.as_ref().unwrap();
    assert!(
        error.contains("stopped after 10 consecutive failures"),
        "{error}"
    );
    assert!(error.contains("(and 9 more)"), "{error}");
}

#[test]
fn a_flattened_collision_is_reported_as_a_failure_rather_than_a_clean_sweep() {
    let fs = memory();
    fs.add_file(r"C:\src\2025\report.pdf", b"old");
    fs.add_file(r"C:\src\2026\report.pdf", b"new");
    let mut destination = moving("D", DESTINATION, vec![FilterRule::AllFiles]);
    destination.flatten_structure = true;
    let task = move_task(vec![destination]);

    let statuses = run(&engine(&fs), &task);

    assert_eq!(SyncOutcome::Failed, statuses[0].outcome);
    assert!(statuses[0]
        .error
        .as_ref()
        .unwrap()
        .contains("a file of that name is already there"));
    assert_eq!(
        1, statuses[0].files_copied,
        "the one that did move is still reported"
    );
}

#[test]
fn cancellation_propagates_and_fabricates_no_status_for_an_idle_destination() {
    let fs = memory();
    fs.add_file(r"C:\src\a.txt", b"a");
    let task = sync_task(vec![
        mirror("First", r"E:\first"),
        mirror("Second", r"F:\second"),
    ]);
    let cancellation = CancellationToken::new();
    cancellation.cancel();

    let result = engine(&fs).execute(&task, &cancellation, &mut |_| {}, &HashSet::new());

    assert!(result.is_err(), "cancellation is not a destination failure");
    assert!(!fs.file_exists(Path::new(r"E:\first\a.txt")));
}

#[test]
fn cancelling_mid_run_leaves_what_already_landed_in_place() {
    let fs = memory();
    for index in 0..10 {
        fs.add_file(format!(r"C:\src\f{index}.txt"), b"x");
    }
    let task = sync_task(vec![add_only("D", DESTINATION, vec![FilterRule::AllFiles])]);
    let cancellation = CancellationToken::new();
    let mut seen = 0;

    let result = engine(&fs).execute(
        &task,
        &cancellation,
        &mut |_| {
            seen += 1;
            if seen == 4 {
                cancellation.cancel();
            }
        },
        &HashSet::new(),
    );

    assert!(result.is_err());
    let landed = fs.list_tree(Path::new(DESTINATION)).unwrap().files.len();
    assert!(
        (1..10).contains(&landed),
        "work already applied stays applied, got {landed}"
    );
}

// -------------------------------------------------------------------------------------------
// Progress and I/O economy
// -------------------------------------------------------------------------------------------

#[test]
fn progress_is_reported_before_each_operation_with_a_stable_total() {
    let fs = memory();
    fs.add_file(r"C:\src\a.txt", b"a");
    fs.add_file(r"C:\src\b.txt", b"b");
    let task = sync_task(vec![add_only("D", DESTINATION, vec![FilterRule::AllFiles])]);

    let (_, reports) = run_collecting_progress(&engine(&fs), &task);

    assert_eq!(2, reports.len());
    assert_eq!(
        vec![0, 1],
        reports
            .iter()
            .map(|r| r.completed_operations)
            .collect::<Vec<_>>()
    );
    assert!(reports.iter().all(|r| r.total_operations == 2));
}

#[test]
fn a_run_walks_the_source_once_and_each_mirror_destination_once() {
    let fs = memory();
    fs.add_file(r"C:\src\a.txt", b"a");
    let task = sync_task(vec![
        mirror("First", r"E:\first"),
        mirror("Second", r"F:\second"),
    ]);

    run(&engine(&fs), &task);

    assert_eq!(
        1,
        fs.tree_walks_of(SOURCE),
        "one walk serves every destination"
    );
    assert_eq!(1, fs.tree_walks_of(r"E:\first"));
    assert_eq!(1, fs.tree_walks_of(r"F:\second"));
}

// -------------------------------------------------------------------------------------------
// Task shapes only a hand-edited config can produce
// -------------------------------------------------------------------------------------------

/// A Move task holding a Mirror destination. The editor makes this unreachable; `tasks.json`
/// does not.
fn move_task_with_a_mirror() -> SyncTask {
    SyncTask::new(
        "T",
        SOURCE,
        Trigger::Manual,
        vec![
            moving("Move", r"D:\moved", vec![FilterRule::AllFiles]),
            mirror("Mirror", r"D:\mirrored"),
        ],
    )
    .with_kind(SyncTaskKind::Move)
}

fn sync_task_from(source: &str, destination: Destination) -> SyncTask {
    SyncTask::new("T", source, Trigger::Manual, vec![destination]).with_kind(SyncTaskKind::Sync)
}
