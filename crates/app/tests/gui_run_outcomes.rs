mod gui_support;
use gpui_kit::TestAppContext;
use gui_support::*;
use syncmaid_core::{
    io::InMemoryFileSystem,
    model::{SyncOutcome, SyncStrategy},
};
#[gpui::test]
fn locked_file_is_deferred_while_others_land(cx: &mut TestAppContext) {
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
    let s = &statuses[&t.destinations[0].id];
    assert_eq!(s.outcome, SyncOutcome::Incomplete);
    assert_eq!(s.files_deferred, 1);
    assert_eq!(fs.contents_of(r"D:\dst\a.txt"), Some(b"a".to_vec()));
    assert_eq!(fs.contents_of(r"D:\dst\b.txt"), None);
}
#[gpui::test]
fn empty_mirror_cannot_be_overridden(cx: &mut TestAppContext) {
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
        h.statuses()[&t.destinations[0].id].outcome,
        SyncOutcome::Failed
    );
    assert_eq!(fs.contents_of(r"D:\dst\keep"), Some(b"keep".to_vec()));
    assert!(!is_shown(cx, &format!("review-{}", t.destinations[0].id)));
}
#[gpui::test]
fn cross_task_overlap_is_refused(cx: &mut TestAppContext) {
    for source_overlap in [false, true] {
        let fs = InMemoryFileSystem::new();
        fs.add_file(r"C:\src\a", b"a");
        let h = Harness::in_memory(fs.clone());
        let a = task("A", r"C:\src", r"D:\dst");
        let b = task(
            "B",
            if source_overlap {
                r"C:\src\sub"
            } else {
                r"C:\other"
            },
            if source_overlap {
                r"D:\other"
            } else {
                r"D:\dst\sub"
            },
        );
        h.seed_tasks(&[a.clone(), b]);
        let cx = h.launch(cx);
        click(cx, &format!("run-{}", a.id));
        let s = h.statuses();
        assert_eq!(s[&a.destinations[0].id].outcome, SyncOutcome::Failed);
        assert_eq!(
            s[&a.destinations[0].id].error.as_deref(),
            Some(syncmaid::strings::task_overlap_refused_format("B").as_str())
        );
        assert_eq!(fs.contents_of(r"D:\dst\a"), None);
    }
}
#[gpui::test]
fn chained_tasks_copy_through_both_windows_actions(cx: &mut TestAppContext) {
    let fs = InMemoryFileSystem::new();
    fs.add_file(r"C:\src\a", b"a");
    let h = Harness::in_memory(fs.clone());
    let a = task("A", r"C:\src", r"D:\middle");
    let b = task("B", r"D:\middle", r"E:\dst");
    h.seed_tasks(&[a.clone(), b.clone()]);
    let cx = h.launch(cx);
    click(cx, &format!("run-{}", a.id));
    click(cx, &format!("run-{}", b.id));
    assert_eq!(fs.contents_of(r"E:\dst\a"), Some(b"a".to_vec()));
    assert!(h
        .statuses()
        .values()
        .all(|s| s.outcome == SyncOutcome::Success));
}

fn mirror_scene() -> (Harness, InMemoryFileSystem, syncmaid_core::model::SyncTask) {
    let fs = InMemoryFileSystem::new();
    fs.add_file(r"C:\src\a", b"a").add_file(r"C:\src\b", b"b");
    for i in 0..20 {
        fs.add_file(format!(r"D:\dst\orphan{i}"), b"orphan");
    }
    let h = Harness::in_memory(fs.clone());
    let mut t = task("Mirror", r"C:\src", r"D:\dst");
    t.destinations[0].strategy = SyncStrategy::Mirror;
    h.seed_tasks(std::slice::from_ref(&t));
    (h, fs, t)
}
#[gpui::test]
fn mirror_approval_is_one_shot(cx: &mut TestAppContext) {
    let (h, fs, t) = mirror_scene();
    let config = fs.contents_of(h.config.tasks_path()).unwrap();
    let cx = h.launch(cx);
    let main = cx.windows()[0];
    let id = t.destinations[0].id;
    click(cx, &format!("run-{}", t.id));
    assert_eq!(h.statuses()[&id].outcome, SyncOutcome::NeedsConfirmation);
    for i in 0..20 {
        assert!(fs.contents_of(format!(r"D:\dst\orphan{i}")).is_some());
    }
    click(cx, &format!("review-{id}"));
    assert_eq!(cx.windows().len(), 2);
    let mut dialog = other_window(cx, main);
    click(&mut dialog, "mirror-delete-confirm");
    settle(cx);
    assert_eq!(h.statuses()[&id].outcome, SyncOutcome::Success);
    assert_eq!(fs.contents_of(h.config.tasks_path()).unwrap(), config);
    for i in 0..20 {
        assert_eq!(fs.contents_of(format!(r"D:\dst\orphan{i}")), None);
        fs.add_file(format!(r"D:\dst\orphan{i}"), b"orphan");
    }
    click(cx, &format!("run-{}", t.id));
    assert_eq!(h.statuses()[&id].outcome, SyncOutcome::NeedsConfirmation);
}
#[gpui::test]
fn keeping_or_closing_confirmation_never_deletes(cx: &mut TestAppContext) {
    let (h, fs, t) = mirror_scene();
    let cx = h.launch(cx);
    let main = cx.windows()[0];
    let id = t.destinations[0].id;
    click(cx, &format!("run-{}", t.id));
    let before = fs.contents_of(h.config.status_path());
    for close in [false, true] {
        click(cx, &format!("review-{id}"));
        let mut dialog = other_window(cx, main);
        if close {
            dialog.simulate_close();
        } else {
            click(&mut dialog, "mirror-delete-keep");
        }
        settle(cx);
        assert_eq!(fs.contents_of(h.config.status_path()), before);
        for i in 0..20 {
            assert!(fs.contents_of(format!(r"D:\dst\orphan{i}")).is_some());
        }
    }
}
#[gpui::test]
fn write_failure_reports_path_and_other_files_still_land(cx: &mut TestAppContext) {
    let fs = InMemoryFileSystem::new();
    fs.add_file(r"C:\src\a.txt", b"a")
        .add_file(r"C:\src\b.txt", b"b");
    fs.with_faults(|f| f.fail_write_path_fragment = Some("b.txt".into()));
    let h = Harness::in_memory(fs.clone());
    let t = task("T", r"C:\src", r"D:\dst");
    h.seed_tasks(std::slice::from_ref(&t));
    let cx = h.launch(cx);
    click(cx, &format!("run-{}", t.id));
    let statuses = h.statuses();
    let s = &statuses[&t.destinations[0].id];
    assert_eq!(s.outcome, SyncOutcome::Failed);
    assert!(s.error.as_ref().unwrap().contains("b.txt"));
    assert!(s.error.as_ref().unwrap().contains("Failed to copy"));
    assert_eq!(fs.contents_of(r"D:\dst\a.txt"), Some(b"a".to_vec()));
    assert_eq!(fs.contents_of(r"D:\dst\b.txt"), None);
}
#[gpui::test]
fn move_routes_each_file_to_first_matching_rule(cx: &mut TestAppContext) {
    use syncmaid_core::{filtering::FilterRule, model::Destination};
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
            [FilterRule::wildcard("**/*.pdf")],
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
    for (file, dest) in [
        (r"invoices\a.pdf", r"D:\pdf"),
        ("c.pdf", r"D:\pdf"),
        (r"invoices\b.txt", r"D:\invoices"),
    ] {
        assert_eq!(
            fs.contents_of(format!(r"{dest}\{file}")),
            Some(file.as_bytes().to_vec())
        );
        assert_eq!(fs.contents_of(format!(r"C:\src\{file}")), None);
    }
    assert_eq!(fs.contents_of(r"D:\invoices\invoices\a.pdf"), None);
    assert_eq!(fs.contents_of(r"D:\pdf\invoices\b.txt"), None);
    assert_eq!(fs.contents_of(r"C:\src\d.txt"), Some(b"d.txt".to_vec()));
}
#[gpui::test]
fn deleting_destination_cleans_status_then_task_is_removed(cx: &mut TestAppContext) {
    let fs = InMemoryFileSystem::new();
    fs.add_file(r"C:\src\a", b"a");
    let h = Harness::in_memory(fs);
    let t = task("T", r"C:\src", r"D:\dst");
    h.seed_tasks(std::slice::from_ref(&t));
    let cx = h.launch(cx);
    click(cx, &format!("run-{}", t.id));
    let id = t.destinations[0].id;
    assert!(h.statuses().contains_key(&id));
    click(cx, &format!("delete-dest-{id}"));
    click(cx, "confirm-ok");
    assert!(h.tasks()[0].destinations.is_empty());
    assert!(!h.statuses().contains_key(&id));
    click(cx, &format!("delete-{}", t.id));
    click(cx, "confirm-ok");
    assert!(h.tasks().is_empty());
}
#[gpui::test]
fn same_task_overlap_is_rejected_by_engine(cx: &mut TestAppContext) {
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
    assert_eq!(statuses.len(), 2);
    for s in statuses.values() {
        assert_eq!(s.outcome, SyncOutcome::Failed);
        assert!(s
            .error
            .as_ref()
            .unwrap()
            .contains("are the same folder or nested in one another"));
        assert_ne!(
            s.error.as_ref().unwrap(),
            &syncmaid::strings::task_overlap_refused_format("T")
        );
    }
    assert_eq!(fs.contents_of(r"D:\dst\a"), None);
}
