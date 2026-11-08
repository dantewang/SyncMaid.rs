mod gui_support;
use gpui_kit::TestAppContext;
use gui_support::*;
use std::path::Path;
#[gpui::test]
fn unnamed_task_never_reaches_disk(cx: &mut TestAppContext) {
    let h = Harness::physical();
    let cx = h.launch(cx);
    click(cx, "new-task");
    click(cx, "browse-source");
    pick_folder(cx, Path::new(r"C:\src"));
    click(cx, "task-save");
    assert!(!h.config.tasks_path().exists());
}
#[gpui::test]
fn overlapping_source_is_blocked(cx: &mut TestAppContext) {
    let h = Harness::physical();
    h.seed_tasks(&[task("A", r"C:\src", r"D:\dst")]);
    let before = std::fs::read(h.config.tasks_path()).unwrap();
    let cx = h.launch(cx);
    click(cx, "new-task");
    type_into(cx, "task-name", "B");
    click(cx, "browse-source");
    pick_folder(cx, Path::new(r"C:\src\sub"));
    assert!(is_shown(cx, "source-overlap"));
    click(cx, "task-save");
    assert_eq!(std::fs::read(h.config.tasks_path()).unwrap(), before);
}
#[gpui::test]
fn nested_destinations_and_other_tasks_destinations_are_blocked(cx: &mut TestAppContext) {
    for path in [r"C:\src\out", r"C:\", r"D:\taken\sub"] {
        let h = Harness::physical();
        let a = task("A", r"C:\src", r"D:\dst");
        h.seed_tasks(&[a.clone(), task("B", r"E:\src", r"D:\taken")]);
        let before = std::fs::read(h.config.tasks_path()).unwrap();
        let cx = h.launch(cx);
        click(cx, &format!("add-{}", a.id));
        click(cx, "browse-destination");
        pick_folder(cx, Path::new(path));
        assert!(is_shown(cx, "destination-blocked") || is_shown(cx, "workspace-blocked"));
        click(cx, "workspace-save");
        assert_eq!(std::fs::read(h.config.tasks_path()).unwrap(), before);
    }
}
#[gpui::test]
fn existing_destination_can_be_saved_and_task_kind_stays_locked(cx: &mut TestAppContext) {
    use syncmaid_core::model::SyncTaskKind;
    let h = Harness::physical();
    let a = task("A", r"C:\src", r"D:\dst");
    h.seed_tasks(std::slice::from_ref(&a));
    let cx = h.launch(cx);
    click(cx, &format!("edit-dest-{}", a.destinations[0].id));
    type_into(cx, "destination-name", "Renamed");
    click(cx, "workspace-save");
    assert_eq!(h.tasks()[0].destinations[0].name, "Renamed");
    click(cx, &format!("edit-{}", a.id));
    click(cx, "kind-move");
    click(cx, "task-save");
    assert_eq!(h.tasks()[0].kind(), SyncTaskKind::Sync);
}
#[gpui::test]
fn mirror_persists_only_all_files(cx: &mut TestAppContext) {
    use syncmaid_core::filtering::FilterRule;
    let h = Harness::physical();
    let mut a = task("A", r"C:\src", r"D:\dst");
    a.destinations[0].filters = vec![FilterRule::extension("pdf")];
    h.seed_tasks(std::slice::from_ref(&a));
    let cx = h.launch(cx);
    click(cx, &format!("edit-dest-{}", a.destinations[0].id));
    click(cx, "strategy-mirror");
    click(cx, "workspace-save");
    assert_eq!(
        h.tasks()[0].destinations[0].filters,
        vec![FilterRule::AllFiles]
    );
}

#[gpui::test]
fn move_requires_a_valid_rule_before_saving(cx: &mut TestAppContext) {
    use syncmaid_core::model::SyncStrategy;
    let h = Harness::physical();
    let cx = h.launch(cx);
    click(cx, "new-task");
    type_into(cx, "task-name", "Move");
    click(cx, "kind-move");
    click(cx, "browse-source");
    pick_folder(cx, Path::new(r"C:\src"));
    click(cx, "task-save");
    let id = h.tasks()[0].id;
    let before = std::fs::read(h.config.tasks_path()).unwrap();
    click(cx, &format!("add-{id}"));
    click(cx, "browse-destination");
    pick_folder(cx, Path::new(r"D:\dst"));
    click(cx, "workspace-save");
    assert_eq!(std::fs::read(h.config.tasks_path()).unwrap(), before);
    type_into(cx, "rule-input-0", "(");
    click(cx, "regex");
    click(cx, "add-rule-0");
    assert!(is_shown(cx, "rule-error-0"));
    click(cx, "workspace-save");
    assert_eq!(std::fs::read(h.config.tasks_path()).unwrap(), before);
    type_into(cx, "rule-input-0", "pdf");
    click(cx, "extension");
    click(cx, "add-rule-0");
    click(cx, "workspace-save");
    let saved = h.tasks();
    assert_eq!(saved[0].destinations.len(), 1);
    assert_eq!(saved[0].destinations[0].strategy, SyncStrategy::Move);
    assert_eq!(
        saved[0].destinations[0].filters,
        vec![syncmaid_core::filtering::FilterRule::extension("pdf")]
    );
    assert!(saved[0].destinations[0]
        .filters
        .iter()
        .any(|r| r.matches("a.pdf")));
    assert!(!saved[0].destinations[0]
        .filters
        .iter()
        .any(|r| r.matches("a.txt")));
}
#[gpui::test]
fn invalid_nested_regex_cannot_be_saved(cx: &mut TestAppContext) {
    use syncmaid_core::filtering::FilterRule;
    let h = Harness::physical();
    let mut a = task("A", r"C:\src", r"D:\dst");
    a.destinations[0].filters = vec![FilterRule::any_of([FilterRule::all_of([
        FilterRule::regex("("),
    ])])];
    h.seed_tasks(std::slice::from_ref(&a));
    let before = std::fs::read(h.config.tasks_path()).unwrap();
    let cx = h.launch(cx);
    click(cx, &format!("edit-dest-{}", a.destinations[0].id));
    click(cx, "workspace-save");
    assert_eq!(std::fs::read(h.config.tasks_path()).unwrap(), before);
    assert!(is_shown(cx, "workspace-blocked"));
}
