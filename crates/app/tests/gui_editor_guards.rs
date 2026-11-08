//! The editors refuse a task shape `AGENT.md` rules out, and say so before anything is saved.
//!
//! The engine refuses the same shapes from hand-edited config (`gui_run_outcomes.rs`); these pin
//! the other half: a blocked Save leaves `tasks.json` byte for byte as it was.

mod gui_support;

use std::path::Path;

use gpui_kit::TestAppContext;
use gui_support::*;
use syncmaid_core::filtering::FilterRule;
use syncmaid_core::model::{SyncStrategy, SyncTaskKind};

#[gpui::test]
fn an_unnamed_task_never_reaches_disk(cx: &mut TestAppContext) {
    let h = Harness::empty();
    let cx = h.launch(cx);

    click(cx, "new-task");
    click(cx, "browse-source");
    pick_folder(cx, Path::new(r"C:\src"));
    click(cx, "task-save");

    assert_eq!(None, h.tasks_file());
}

#[gpui::test]
fn a_source_nested_in_another_tasks_source_is_blocked(cx: &mut TestAppContext) {
    let h = Harness::empty();
    h.seed_tasks(&[task("A", r"C:\src", r"D:\dst")]);
    let before = h.tasks_file();
    let cx = h.launch(cx);

    click(cx, "new-task");
    type_into(cx, "task-name", "B");
    click(cx, "browse-source");
    pick_folder(cx, Path::new(r"C:\src\sub"));
    assert!(is_shown(cx, "source-overlap"));
    click(cx, "task-save");

    assert_eq!(before, h.tasks_file());
}

/// Inside the source, containing the source, and inside another task's destination.
#[gpui::test]
fn a_destination_that_nests_with_the_source_or_another_destination_is_blocked(
    cx: &mut TestAppContext,
) {
    for path in [r"C:\src\out", r"C:\", r"D:\taken\sub"] {
        let h = Harness::empty();
        let a = task("A", r"C:\src", r"D:\dst");
        h.seed_tasks(&[a.clone(), task("B", r"E:\src", r"D:\taken")]);
        let before = h.tasks_file();
        let cx = h.launch(cx);

        click(cx, &format!("add-{}", a.id));
        click(cx, "browse-destination");
        pick_folder(cx, Path::new(path));
        assert!(
            is_shown(cx, "destination-blocked") || is_shown(cx, "workspace-blocked"),
            "{path} was not explained"
        );
        click(cx, "workspace-save");

        assert_eq!(before, h.tasks_file(), "{path} was saved");
    }
}

/// A saved destination re-saved is not its own overlap; a task with destinations keeps its kind.
#[gpui::test]
fn an_existing_destination_saves_and_the_task_kind_stays_locked(cx: &mut TestAppContext) {
    let h = Harness::empty();
    let a = task("A", r"C:\src", r"D:\dst");
    h.seed_tasks(std::slice::from_ref(&a));
    let cx = h.launch(cx);

    click(cx, &format!("edit-dest-{}", a.destinations[0].id));
    type_into(cx, "destination-name", "Renamed");
    click(cx, "workspace-save");
    assert_eq!("Renamed", h.tasks()[0].destinations[0].name);

    click(cx, &format!("edit-{}", a.id));
    click(cx, "kind-move");
    click(cx, "task-save");
    assert_eq!(SyncTaskKind::Sync, h.tasks()[0].kind());
}

/// Switching a filtered destination to Mirror normalizes it to the lone all-files filter.
#[gpui::test]
fn mirror_persists_only_the_all_files_filter(cx: &mut TestAppContext) {
    let h = Harness::empty();
    let mut a = task("A", r"C:\src", r"D:\dst");
    a.destinations[0].filters = vec![FilterRule::extension("pdf")];
    h.seed_tasks(std::slice::from_ref(&a));
    let cx = h.launch(cx);

    click(cx, &format!("edit-dest-{}", a.destinations[0].id));
    click(cx, "strategy-mirror");
    click(cx, "workspace-save");

    assert_eq!(
        vec![FilterRule::AllFiles],
        h.tasks()[0].destinations[0].filters
    );
}

/// A Move rule starts selecting nothing; a pattern that does not compile is never added; a
/// valid one is what makes the rule saveable.
#[gpui::test]
fn a_move_rule_needs_a_valid_selection_before_it_saves(cx: &mut TestAppContext) {
    let h = Harness::empty();
    let cx = h.launch(cx);

    click(cx, "new-task");
    type_into(cx, "task-name", "Move");
    click(cx, "kind-move");
    click(cx, "browse-source");
    pick_folder(cx, Path::new(r"C:\src"));
    click(cx, "task-save");
    let id = h.tasks()[0].id;
    let before = h.tasks_file();

    click(cx, &format!("add-{id}"));
    click(cx, "browse-destination");
    pick_folder(cx, Path::new(r"D:\dst"));
    click(cx, "workspace-save");
    assert_eq!(before, h.tasks_file(), "a rule selecting nothing was saved");

    type_into(cx, "rule-input-0", "(");
    click(cx, "regex");
    click(cx, "add-rule-0");
    assert!(is_shown(cx, "rule-error-0"));
    click(cx, "workspace-save");
    assert_eq!(before, h.tasks_file(), "a broken regex was saved");

    type_into(cx, "rule-input-0", "pdf");
    click(cx, "extension");
    click(cx, "add-rule-0");
    click(cx, "workspace-save");

    let saved = h.tasks();
    assert_eq!(1, saved[0].destinations.len());
    assert_eq!(SyncStrategy::Move, saved[0].destinations[0].strategy);
    assert_eq!(
        vec![FilterRule::extension("pdf")],
        saved[0].destinations[0].filters
    );
}

/// Hand-edited config can bury a broken regex where the editor cannot draw it; Save still
/// refuses rather than writing it back.
#[gpui::test]
fn a_nested_invalid_regex_cannot_be_saved(cx: &mut TestAppContext) {
    let h = Harness::empty();
    let mut a = task("A", r"C:\src", r"D:\dst");
    a.destinations[0].filters = vec![FilterRule::any_of([FilterRule::all_of([
        FilterRule::regex("("),
    ])])];
    h.seed_tasks(std::slice::from_ref(&a));
    let before = h.tasks_file();
    let cx = h.launch(cx);

    click(cx, &format!("edit-dest-{}", a.destinations[0].id));
    click(cx, "workspace-save");

    assert_eq!(before, h.tasks_file());
    assert!(is_shown(cx, "workspace-blocked"));
}
