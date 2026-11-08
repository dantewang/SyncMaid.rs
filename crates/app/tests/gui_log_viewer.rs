//! A destination's status opens the log, and the next one re-points that window rather than
//! opening a second.
//!
//! Its own test binary: the log subscriber is process-wide. What the viewer filters to is
//! `services::logging`'s to test; a headless window cannot read text.

mod gui_support;

use gpui_kit::TestAppContext;
use gui_support::*;
use syncmaid_core::io::InMemoryFileSystem;
use syncmaid_core::model::SyncOutcome;

#[gpui::test]
fn status_links_open_and_reuse_one_log_window(cx: &mut TestAppContext) {
    let fs = InMemoryFileSystem::new();
    fs.add_file(r"C:\src\a", b"a");
    let h = Harness::in_memory(fs);
    // Where the viewer reads, so the run's lines are really there to show.
    syncmaid::services::logging::install(&h.config.log_path());
    let mut t = task("T", r"C:\src", r"D:\one");
    t.destinations
        .push(task("Two", r"C:\src", r"D:\two").destinations.remove(0));
    h.seed_tasks(std::slice::from_ref(&t));
    let cx = h.launch(cx);
    let main = cx.windows()[0];

    click(cx, &format!("run-{}", t.id));
    let statuses = h.statuses();
    assert_eq!(2, statuses.len());
    assert!(statuses
        .values()
        .all(|status| status.outcome == SyncOutcome::Success));

    click(cx, &format!("status-link-{}", t.destinations[0].id));
    assert_eq!(2, cx.windows().len());
    let mut log = other_window(cx, main);
    assert!(is_shown(&mut log, "log-lines"));
    let viewer = cx
        .windows()
        .into_iter()
        .find(|window| *window != main)
        .expect("the log window");

    click(cx, &format!("status-link-{}", t.destinations[1].id));
    assert_eq!(2, cx.windows().len());
    assert!(
        cx.windows().contains(&viewer),
        "a second log window replaced the first"
    );
}
