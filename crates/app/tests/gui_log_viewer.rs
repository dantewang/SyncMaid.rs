mod gui_support;
use gpui_kit::TestAppContext;
use gui_support::*;
use syncmaid_core::io::InMemoryFileSystem;
#[gpui::test]
fn status_links_open_and_reuse_log_window(cx: &mut TestAppContext) {
    let fs = InMemoryFileSystem::new();
    fs.add_file(r"C:\src\a", b"a");
    let h = Harness::in_memory(fs);
    syncmaid::services::logging::install(&h.data.path().join("test.log"));
    let mut t = task("T", r"C:\src", r"D:\one");
    t.destinations
        .push(task("Two", r"C:\src", r"D:\two").destinations.remove(0));
    h.seed_tasks(std::slice::from_ref(&t));
    let cx = h.launch(cx);
    let main = cx.windows()[0];
    click(cx, &format!("run-{}", t.id));
    assert_eq!(h.statuses().len(), 2);
    assert!(h
        .statuses()
        .values()
        .all(|s| s.outcome == syncmaid_core::model::SyncOutcome::Success));
    click(cx, &format!("status-link-{}", t.destinations[0].id));
    assert_eq!(cx.windows().len(), 2);
    let mut log = other_window(cx, main);
    assert!(is_shown(&mut log, "log-lines"));
    let handle = cx.windows().into_iter().find(|w| *w != main).unwrap();
    click(cx, &format!("status-link-{}", t.destinations[1].id));
    assert_eq!(cx.windows().len(), 2);
    assert!(cx.windows().contains(&handle));
}
