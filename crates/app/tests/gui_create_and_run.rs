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

    assert!(is_shown(cx, &format!("edit-dest-{}", target.id)));
    click(cx, &format!("expand-{}", task.id));
    assert!(!is_shown(cx, &format!("edit-dest-{}", target.id)));
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
    cx.simulate_close();
    let cx = harness.launch(cx);
    assert!(is_shown(cx, &format!("run-{}", task.id)));
    std::fs::write(source.path().join("after-restart.txt"), b"restart").unwrap();
    click(cx, &format!("run-{}", task.id));
    assert_eq!(
        std::fs::read(destination.path().join("after-restart.txt")).unwrap(),
        b"restart"
    );
}

#[gpui::test]
fn editing_preserves_identity_and_destinations(cx: &mut TestAppContext) {
    let h = Harness::physical();
    let original = task("Before", r"C:\source", r"D:\target");
    h.seed_tasks(std::slice::from_ref(&original));
    let cx = h.launch(cx);
    click(cx, &format!("edit-{}", original.id));
    type_into(cx, "task-name", "After");
    click(cx, "task-save");
    let saved = h.tasks();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].id, original.id);
    assert_eq!(saved[0].destinations, original.destinations);
    assert_eq!(saved[0].name, "After");
}
#[gpui::test]
fn run_all_copies_both_tasks(cx: &mut TestAppContext) {
    use syncmaid_core::io::InMemoryFileSystem;
    let fs = InMemoryFileSystem::new();
    fs.add_file(r"C:\one\a", b"one")
        .add_file(r"C:\two\b", b"two");
    let h = Harness::in_memory(fs.clone());
    h.seed_tasks(&[
        task("one", r"C:\one", r"D:\one"),
        task("two", r"C:\two", r"D:\two"),
    ]);
    let cx = h.launch(cx);
    click(cx, "run-all");
    assert_eq!(fs.contents_of(r"D:\one\a"), Some(b"one".to_vec()));
    assert_eq!(fs.contents_of(r"D:\two\b"), Some(b"two".to_vec()));
}
