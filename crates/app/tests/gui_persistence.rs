//! What survives the window, and what the window must not overwrite.

mod gui_support;

use std::path::Path;

use gpui_kit::TestAppContext;
use gui_support::*;
use syncmaid_core::filtering::FilterRule;
use syncmaid_core::model::{Destination, SyncStrategy};

/// Config that exists but cannot be read switches saving off: the window still works, and the
/// file the user still has is never replaced by one the app invented.
#[gpui::test]
fn unreadable_config_is_never_overwritten(cx: &mut TestAppContext) {
    let h = Harness::empty();
    let broken = b"{ broken config";
    h.write_tasks_file(broken);
    let cx = h.launch(cx);
    assert!(is_shown(cx, "config-unreadable"));

    click(cx, "new-task");
    type_into(cx, "task-name", "New");
    click(cx, "browse-source");
    pick_folder(cx, Path::new(r"C:\src"));
    click(cx, "task-save");

    assert_eq!(Some(broken.to_vec()), h.tasks_file());
}

/// A Move task's order is its matching order, so a reorder in the workspace is what lands on
/// disk and what the next launch shows.
#[gpui::test]
fn rule_order_survives_a_restart(cx: &mut TestAppContext) {
    let h = Harness::empty();
    let mut t = task("Move", r"C:\src", r"D:\one");
    t.destinations = (0..3)
        .map(|index| {
            Destination::new(
                format!("D{index}"),
                format!(r"D:\dest{index}"),
                [FilterRule::extension(format!("ext{index}"))],
                SyncStrategy::Move,
            )
        })
        .collect();
    let [first, second, third] = [0, 1, 2].map(|index| t.destinations[index].id);
    h.seed_tasks(std::slice::from_ref(&t));
    let order = || -> Vec<_> {
        h.tasks()[0]
            .destinations
            .iter()
            .map(|destination| destination.id)
            .collect()
    };
    let cx = h.launch(cx);

    click(cx, &format!("edit-dest-{first}"));
    click(cx, &format!("down-{first}"));
    click(cx, "workspace-save");
    assert_eq!(vec![second, first, third], order());

    cx.simulate_close();
    let cx = h.launch(cx);
    click(cx, &format!("edit-dest-{second}"));
    click(cx, &format!("up-{third}"));
    click(cx, "workspace-save");
    assert_eq!(vec![second, third, first], order());
}
