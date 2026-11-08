mod gui_support;
use gpui_kit::TestAppContext;
use gui_support::*;
use syncmaid_core::triggers::Trigger;
#[gpui::test]
fn correcting_failed_trigger_removes_badge(cx: &mut TestAppContext) {
    let h = Harness::physical();
    let mut t = task("T", r"C:\src", r"D:\dst");
    t.trigger = Trigger::scheduled("invalid");
    h.seed_tasks(std::slice::from_ref(&t));
    let cx = h.launch(cx);
    assert!(is_shown(cx, &format!("trigger-error-{}", t.id)));
    click(cx, &format!("edit-{}", t.id));
    click(cx, "trigger-manual");
    click(cx, "task-save");
    assert_eq!(h.tasks()[0].trigger, Trigger::Manual);
    assert!(!is_shown(cx, &format!("trigger-error-{}", t.id)));
}
