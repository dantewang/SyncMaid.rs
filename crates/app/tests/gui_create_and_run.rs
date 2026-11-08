//! One user journey through the real window, from the first click to the bytes on disk.
//!
//! GPUI's headless test platform stands in for Windows: nothing is shown, no GPU is touched,
//! and the folder pickers are answered by the test. Everything else — the editors, the
//! workspace, the run gate, the engine, the physical file system — is the code the app ships,
//! wired the way `main` wires it.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::{AppContext as _, Modifiers, TestAppContext, VisualTestContext};
use syncmaid::state::Workspace;
use syncmaid::ui::Root;
use syncmaid::views::MainView;
use syncmaid_core::io::{FileSystem, PhysicalFileSystem};
use syncmaid_core::model::{SyncOutcome, SyncStrategy};
use syncmaid_core::persistence::{ConfigLocation, StatusStore, TaskStore};
use tempfile::TempDir;

/// Lets every animation finish and the window draw what is now true.
///
/// A sheet slides in. Clicked mid-slide, the press lands on one frame and the release on the
/// next, by then somewhere else — and a press and a release on two different things is not a
/// click. The test platform's clock is simulated, so finishing the slide costs nothing.
fn settle(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
}

/// Clicks the middle of whatever is drawn under `selector`, then lets the app settle.
fn click(cx: &mut VisualTestContext, selector: &str) {
    settle(cx);
    // `debug_bounds` wants a `'static` name, and ids carrying a task's uuid are made at runtime.
    let selector: &'static str = Box::leak(selector.to_owned().into_boxed_str());
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("nothing on screen is called {selector:?}"));
    cx.simulate_click(bounds.center(), Modifiers::none());
    settle(cx);
}

/// Answers the folder picker the last click opened.
fn pick_folder(cx: &mut VisualTestContext, folder: &Path) {
    assert!(
        cx.did_prompt_for_paths(),
        "the click did not open a folder picker"
    );
    let folder = folder.to_path_buf();
    cx.simulate_path_prompt_response(move |_| Some(vec![folder]));
    cx.run_until_parked();
}

#[gpui::test]
fn a_task_made_in_the_window_runs_and_lands_on_disk(cx: &mut TestAppContext) {
    let data = TempDir::new().expect("data");
    let source = TempDir::new().expect("source");
    let destination = TempDir::new().expect("destination");
    std::fs::write(source.path().join("photo.jpg"), b"jpeg bytes").expect("write");
    std::fs::create_dir(source.path().join("nested")).expect("mkdir");
    std::fs::write(source.path().join("nested").join("note.txt"), b"text").expect("write");

    let file_system: Arc<dyn FileSystem> = Arc::new(PhysicalFileSystem::new());
    let config = ConfigLocation::at(data.path());
    let tasks = TaskStore::new(Arc::clone(&file_system), config.tasks_path());
    let statuses = StatusStore::new(Arc::clone(&file_system), config.status_path());

    cx.update(|cx| {
        gpui_kit::init(cx);
        syncmaid::ui::init(cx);
        syncmaid::theme::install(cx);
    });
    let workspace = Workspace::load(Arc::clone(&file_system), &config);
    let (_, cx) = cx.add_window_view({
        let file_system = Arc::clone(&file_system);
        move |window, cx| {
            let view = cx.new(|cx| MainView::new(workspace, file_system, cx));
            Root::new(view, window, cx)
        }
    });

    // The task: a name, a source picked with the folder picker, everything else as it opens.
    click(cx, "new-task");
    click(cx, "task-name");
    cx.simulate_input("Photos");
    click(cx, "browse-source");
    pick_folder(cx, source.path());
    click(cx, "task-save");

    let saved = tasks.load();
    assert_eq!(1, saved.len(), "Save should have written the one task");
    let task = &saved[0];
    assert_eq!("Photos", task.name);
    assert_eq!(source.path().to_string_lossy(), task.source_path);
    assert!(task.destinations.is_empty());

    // One destination, through the card's add button and the workspace it opens.
    click(cx, &format!("add-{}", task.id));
    click(cx, "browse-destination");
    pick_folder(cx, destination.path());
    click(cx, "workspace-save");

    let saved = tasks.load();
    let target = saved[0]
        .destinations
        .first()
        .expect("the workspace should have saved the destination")
        .clone();
    assert_eq!(destination.path().to_string_lossy(), target.local_path());
    assert_eq!(
        SyncStrategy::AddOnly,
        target.strategy,
        "a new Sync destination starts as the strategy that never deletes"
    );

    click(cx, &format!("run-{}", task.id));

    assert_eq!(
        b"jpeg bytes".as_slice(),
        std::fs::read(destination.path().join("photo.jpg")).expect("photo.jpg was not copied")
    );
    assert_eq!(
        b"text".as_slice(),
        std::fs::read(destination.path().join("nested").join("note.txt"))
            .expect("nested/note.txt was not copied")
    );
    let status = statuses
        .load()
        .remove(&target.id)
        .expect("the run should have written the destination's status");
    assert_eq!(SyncOutcome::Success, status.outcome);
    assert_eq!(2, status.files_copied);
}
