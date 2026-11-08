mod gui_support;
use gpui_kit::TestAppContext;
use gui_support::*;
use syncmaid_core::model::{SyncOutcome, SyncStrategy};
use tempfile::TempDir;
#[gpui::test]
fn a_task_made_in_the_window_runs_and_lands_on_disk(cx: &mut TestAppContext) {
    let source = TempDir::new().expect("source");
    let destination = TempDir::new().expect("destination");
    std::fs::write(source.path().join("photo.jpg"), b"jpeg bytes").expect("write");
    std::fs::create_dir(source.path().join("nested")).expect("mkdir");
    std::fs::write(source.path().join("nested").join("note.txt"), b"text").expect("write");

    let harness = Harness::physical();
    let cx = harness.launch(cx);

    // The task: a name, a source picked with the folder picker, everything else as it opens.
    click(cx, "new-task");
    click(cx, "task-name");
    cx.simulate_input("Photos");
    click(cx, "browse-source");
    pick_folder(cx, source.path());
    click(cx, "task-save");

    let saved = harness.tasks();
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

    let saved = harness.tasks();
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
    let status = harness
        .statuses()
        .remove(&target.id)
        .expect("the run should have written the destination's status");
    assert_eq!(SyncOutcome::Success, status.outcome);
    assert_eq!(2, status.files_copied);
}
