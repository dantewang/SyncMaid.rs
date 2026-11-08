//! What a run started from a card's button leaves behind: files, statuses, and — for a mass
//! deletion — a question asked once, in its own window.
//!
//! Tasks are seeded as hand-edited config, because the editors are `gui_editor_guards.rs`'s
//! business and several of these shapes are ones the editors refuse. Everything is in memory:
//! faults can be injected, and a Mirror deletion never reaches a real Recycle Bin.

mod gui_support;

use gpui_kit::TestAppContext;
use gui_support::*;
use syncmaid_core::filtering::FilterRule;
use syncmaid_core::io::InMemoryFileSystem;
use syncmaid_core::model::{Destination, SyncOutcome, SyncStrategy, SyncTask};

#[gpui::test]
fn a_locked_file_is_deferred_while_the_others_land(cx: &mut TestAppContext) {
    let fs = InMemoryFileSystem::new();
    fs.add_file(r"C:\src\a.txt", b"a")
        .add_file(r"C:\src\b.txt", b"b")
        .lock_path(r"C:\src\b.txt");
    let h = Harness::in_memory(fs.clone());
    let t = task("T", r"C:\src", r"D:\dst");
    h.seed_tasks(std::slice::from_ref(&t));
    let cx = h.launch(cx);

    click(cx, &format!("run-{}", t.id));

    let statuses = h.statuses();
    let status = &statuses[&t.destinations[0].id];
    assert_eq!(SyncOutcome::Incomplete, status.outcome);
    assert_eq!(1, status.files_deferred);
    assert_eq!(Some(b"a".to_vec()), fs.contents_of(r"D:\dst\a.txt"));
    assert_eq!(None, fs.contents_of(r"D:\dst\b.txt"));
}

#[gpui::test]
fn a_failed_write_names_the_file_and_the_verb_and_the_rest_still_land(cx: &mut TestAppContext) {
    let fs = InMemoryFileSystem::new();
    fs.add_file(r"C:\src\a.txt", b"a")
        .add_file(r"C:\src\b.txt", b"b");
    fs.with_faults(|faults| faults.fail_write_path_fragment = Some("b.txt".into()));
    let h = Harness::in_memory(fs.clone());
    let t = task("T", r"C:\src", r"D:\dst");
    h.seed_tasks(std::slice::from_ref(&t));
    let cx = h.launch(cx);

    click(cx, &format!("run-{}", t.id));

    let statuses = h.statuses();
    let status = &statuses[&t.destinations[0].id];
    assert_eq!(SyncOutcome::Failed, status.outcome);
    let error = status.error.as_deref().expect("a failure says why");
    assert!(error.contains("b.txt"), "{error}");
    assert!(error.contains("Failed to copy"), "{error}");
    assert_eq!(Some(b"a".to_vec()), fs.contents_of(r"D:\dst\a.txt"));
    assert_eq!(None, fs.contents_of(r"D:\dst\b.txt"));
}

/// An empty source deletes nothing, and offers no review either: the guard is not overridable.
#[gpui::test]
fn an_empty_mirror_source_deletes_nothing_and_offers_no_review(cx: &mut TestAppContext) {
    let fs = InMemoryFileSystem::new();
    fs.add_directory(r"C:\src")
        .add_file(r"D:\dst\keep", b"keep");
    let h = Harness::in_memory(fs.clone());
    let mut t = task("T", r"C:\src", r"D:\dst");
    t.destinations[0].strategy = SyncStrategy::Mirror;
    h.seed_tasks(std::slice::from_ref(&t));
    let cx = h.launch(cx);

    click(cx, &format!("run-{}", t.id));

    assert_eq!(
        SyncOutcome::Failed,
        h.statuses()[&t.destinations[0].id].outcome
    );
    assert_eq!(Some(b"keep".to_vec()), fs.contents_of(r"D:\dst\keep"));
    assert!(!is_shown(cx, &format!("review-{}", t.destinations[0].id)));
}

/// Two tasks sharing a source, or a destination, is refused before the source is walked —
/// with the refusal's own sentence rather than an engine failure.
#[gpui::test]
fn a_task_overlapping_another_task_is_refused(cx: &mut TestAppContext) {
    for (source, destination) in [(r"C:\other", r"D:\dst\sub"), (r"C:\src\sub", r"D:\other")] {
        let fs = InMemoryFileSystem::new();
        fs.add_file(r"C:\src\a", b"a");
        let h = Harness::in_memory(fs.clone());
        let a = task("A", r"C:\src", r"D:\dst");
        h.seed_tasks(&[a.clone(), task("B", source, destination)]);
        let cx = h.launch(cx);

        click(cx, &format!("run-{}", a.id));

        let statuses = h.statuses();
        let status = &statuses[&a.destinations[0].id];
        assert_eq!(SyncOutcome::Failed, status.outcome);
        assert_eq!(
            Some(syncmaid::strings::task_overlap_refused_format("B").as_str()),
            status.error.as_deref()
        );
        assert_eq!(None, fs.contents_of(r"D:\dst\a"));
    }
}

/// Chaining — one task filing into the folder another backs up — is explicitly allowed.
#[gpui::test]
fn chained_tasks_both_run(cx: &mut TestAppContext) {
    let fs = InMemoryFileSystem::new();
    fs.add_file(r"C:\src\a", b"a");
    let h = Harness::in_memory(fs.clone());
    let a = task("A", r"C:\src", r"D:\middle");
    let b = task("B", r"D:\middle", r"E:\dst");
    h.seed_tasks(&[a.clone(), b.clone()]);
    let cx = h.launch(cx);

    click(cx, &format!("run-{}", a.id));
    click(cx, &format!("run-{}", b.id));

    assert_eq!(Some(b"a".to_vec()), fs.contents_of(r"E:\dst\a"));
    assert!(h
        .statuses()
        .values()
        .all(|status| status.outcome == SyncOutcome::Success));
}

/// Two destinations of one task overlapping is the engine's to refuse, in its own words — not
/// the cross-task refusal above.
#[gpui::test]
fn overlapping_destinations_of_one_task_are_refused_by_the_engine(cx: &mut TestAppContext) {
    let fs = InMemoryFileSystem::new();
    fs.add_file(r"C:\src\a", b"a");
    let h = Harness::in_memory(fs.clone());
    let mut t = task("T", r"C:\src", r"D:\dst");
    t.destinations.push(
        task("Nested", r"C:\src", r"D:\dst\sub")
            .destinations
            .remove(0),
    );
    h.seed_tasks(std::slice::from_ref(&t));
    let cx = h.launch(cx);

    click(cx, &format!("run-{}", t.id));

    let statuses = h.statuses();
    assert_eq!(2, statuses.len());
    for status in statuses.values() {
        assert_eq!(SyncOutcome::Failed, status.outcome);
        let error = status.error.as_deref().expect("a failure says why");
        assert!(
            error.contains("are the same folder or nested in one another"),
            "{error}"
        );
        assert_ne!(syncmaid::strings::task_overlap_refused_format("T"), error);
    }
    assert_eq!(None, fs.contents_of(r"D:\dst\a"));
}

/// First match wins, and overlapping rules are legal: `invoices/a.pdf` matches both and goes to
/// the first. What no rule matches stays in the source.
#[gpui::test]
fn a_move_sends_each_file_to_the_first_rule_that_matches_it(cx: &mut TestAppContext) {
    let fs = InMemoryFileSystem::new();
    for file in [r"invoices\a.pdf", r"invoices\b.txt", "c.pdf", "d.txt"] {
        fs.add_file(format!(r"C:\src\{file}"), file.as_bytes());
    }
    let h = Harness::in_memory(fs.clone());
    let mut t = task("Move", r"C:\src", r"D:\pdf");
    t.destinations = vec![
        Destination::new(
            "PDF",
            r"D:\pdf",
            [FilterRule::extension("pdf")],
            SyncStrategy::Move,
        ),
        Destination::new(
            "Invoices",
            r"D:\invoices",
            [FilterRule::path("invoices/")],
            SyncStrategy::Move,
        ),
    ];
    h.seed_tasks(std::slice::from_ref(&t));
    let cx = h.launch(cx);

    click(cx, &format!("run-{}", t.id));

    for (file, destination) in [
        (r"invoices\a.pdf", r"D:\pdf"),
        ("c.pdf", r"D:\pdf"),
        (r"invoices\b.txt", r"D:\invoices"),
    ] {
        assert_eq!(
            Some(file.as_bytes().to_vec()),
            fs.contents_of(format!(r"{destination}\{file}")),
            "{file} did not reach {destination}"
        );
        assert_eq!(None, fs.contents_of(format!(r"C:\src\{file}")));
    }
    assert_eq!(None, fs.contents_of(r"D:\invoices\invoices\a.pdf"));
    assert_eq!(Some(b"d.txt".to_vec()), fs.contents_of(r"C:\src\d.txt"));
}

#[gpui::test]
fn deleting_a_destination_drops_its_status_and_deleting_the_task_removes_it(
    cx: &mut TestAppContext,
) {
    let fs = InMemoryFileSystem::new();
    fs.add_file(r"C:\src\a", b"a");
    let h = Harness::in_memory(fs);
    let t = task("T", r"C:\src", r"D:\dst");
    h.seed_tasks(std::slice::from_ref(&t));
    let cx = h.launch(cx);
    let id = t.destinations[0].id;
    click(cx, &format!("run-{}", t.id));
    assert!(h.statuses().contains_key(&id));

    click(cx, &format!("delete-dest-{id}"));
    click(cx, "confirm-ok");
    assert!(h.tasks()[0].destinations.is_empty());
    assert!(!h.statuses().contains_key(&id));

    click(cx, &format!("delete-{}", t.id));
    click(cx, "confirm-ok");
    assert!(h.tasks().is_empty());
}

/// Two files in the source, twenty more in the Mirror destination: far past the guard.
fn mass_deletion() -> (Harness, InMemoryFileSystem, SyncTask) {
    let fs = InMemoryFileSystem::new();
    fs.add_file(r"C:\src\a", b"a").add_file(r"C:\src\b", b"b");
    add_orphans(&fs);
    let h = Harness::in_memory(fs.clone());
    let mut t = task("Mirror", r"C:\src", r"D:\dst");
    t.destinations[0].strategy = SyncStrategy::Mirror;
    h.seed_tasks(std::slice::from_ref(&t));
    (h, fs, t)
}

fn add_orphans(fs: &InMemoryFileSystem) {
    for index in 0..20 {
        fs.add_file(format!(r"D:\dst\orphan{index}"), b"orphan");
    }
}

fn orphans_left(fs: &InMemoryFileSystem) -> usize {
    (0..20)
        .filter(|index| fs.contents_of(format!(r"D:\dst\orphan{index}")).is_some())
        .count()
}

/// "Delete" in the confirmation window deletes, for that run only: nothing about the approval
/// is written down, so the next mass deletion asks again.
#[gpui::test]
fn a_mass_deletion_approval_is_spent_by_one_run(cx: &mut TestAppContext) {
    let (h, fs, t) = mass_deletion();
    let tasks_before = h.tasks_file();
    let cx = h.launch(cx);
    let main = cx.windows()[0];
    let id = t.destinations[0].id;

    click(cx, &format!("run-{}", t.id));
    assert_eq!(SyncOutcome::NeedsConfirmation, h.statuses()[&id].outcome);
    assert_eq!(20, orphans_left(&fs));

    click(cx, &format!("review-{id}"));
    assert_eq!(2, cx.windows().len());
    let mut dialog = other_window(cx, main);
    click(&mut dialog, "mirror-delete-confirm");
    settle(cx);
    assert_eq!(SyncOutcome::Success, h.statuses()[&id].outcome);
    assert_eq!(0, orphans_left(&fs));
    assert_eq!(tasks_before, h.tasks_file(), "the approval was persisted");

    add_orphans(&fs);
    click(cx, &format!("run-{}", t.id));
    assert_eq!(SyncOutcome::NeedsConfirmation, h.statuses()[&id].outcome);
    assert_eq!(20, orphans_left(&fs));
}

/// "Keep" and closing the window by its title bar are the same answer: nothing deleted, and
/// no run started on the strength of it.
#[gpui::test]
fn keeping_or_closing_the_confirmation_deletes_nothing(cx: &mut TestAppContext) {
    let (h, fs, t) = mass_deletion();
    let cx = h.launch(cx);
    let main = cx.windows()[0];
    let id = t.destinations[0].id;
    click(cx, &format!("run-{}", t.id));
    let statuses_before = fs.contents_of(h.config.status_path());

    for close in [false, true] {
        click(cx, &format!("review-{id}"));
        let mut dialog = other_window(cx, main);
        if close {
            dialog.simulate_close();
        } else {
            click(&mut dialog, "mirror-delete-keep");
        }
        settle(cx);

        assert_eq!(statuses_before, fs.contents_of(h.config.status_path()));
        assert_eq!(20, orphans_left(&fs));
    }
}
