mod gui_support;
use gpui_kit::TestAppContext;
use gui_support::*;
use std::path::Path;
#[gpui::test]
fn unreadable_config_is_preserved(cx: &mut TestAppContext) {
    let h = Harness::physical();
    let bytes = b"{ broken config";
    std::fs::write(h.config.tasks_path(), bytes).unwrap();
    let cx = h.launch(cx);
    assert!(is_shown(cx, "config-unreadable"));
    click(cx, "new-task");
    type_into(cx, "task-name", "New");
    click(cx, "browse-source");
    pick_folder(cx, Path::new(r"C:\src"));
    click(cx, "task-save");
    assert_eq!(std::fs::read(h.config.tasks_path()).unwrap(), bytes);
}
#[gpui::test]
fn rule_order_survives_reload(cx: &mut TestAppContext) {
    use syncmaid_core::{
        filtering::FilterRule,
        model::{Destination, SyncStrategy},
    };
    let h = Harness::physical();
    let mut t = task("Move", r"C:\src", r"D:\one");
    t.destinations = (0..3)
        .map(|i| {
            Destination::new(
                format!("D{i}"),
                format!(r"D:\dest{i}"),
                [FilterRule::extension(format!("ext{i}"))],
                SyncStrategy::Move,
            )
        })
        .collect();
    h.seed_tasks(std::slice::from_ref(&t));
    let cx = h.launch(cx);
    click(cx, &format!("edit-dest-{}", t.destinations[0].id));
    click(cx, &format!("down-{}", t.destinations[0].id));
    click(cx, "workspace-save");
    let saved = h.tasks();
    assert_eq!(
        saved[0]
            .destinations
            .iter()
            .map(|d| d.id)
            .collect::<Vec<_>>(),
        vec![
            t.destinations[1].id,
            t.destinations[0].id,
            t.destinations[2].id
        ]
    );
    cx.simulate_close();
    let cx = h.launch(cx);
    assert!(is_shown(cx, &format!("run-{}", t.id)));
    assert_eq!(h.tasks(), saved);
    click(cx, &format!("edit-dest-{}", t.destinations[1].id));
    click(cx, &format!("up-{}", t.destinations[2].id));
    click(cx, "workspace-save");
    assert_eq!(
        h.tasks()[0]
            .destinations
            .iter()
            .map(|d| d.id)
            .collect::<Vec<_>>(),
        vec![
            t.destinations[1].id,
            t.destinations[2].id,
            t.destinations[0].id
        ]
    );
}
