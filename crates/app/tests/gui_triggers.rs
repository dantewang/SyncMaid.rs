//! A trigger that will not start is said out loud on the card, and fixing it clears the badge.
//!
//! An invalid cron fails while the trigger is built, synchronously — the one trigger outcome a
//! headless test can see without real threads and a real clock.

mod gui_support;

use gpui_kit::TestAppContext;
use gui_support::*;
use syncmaid_core::triggers::Trigger;

#[gpui::test]
fn correcting_a_failed_trigger_removes_its_badge(cx: &mut TestAppContext) {
    let h = Harness::empty();
    let mut t = task("T", r"C:\src", r"D:\dst");
    t.trigger = Trigger::scheduled("invalid");
    h.seed_tasks(std::slice::from_ref(&t));
    let cx = h.launch(cx);
    let badge = format!("trigger-error-{}", t.id);
    assert!(is_shown(cx, &badge));

    click(cx, &format!("edit-{}", t.id));
    click(cx, "trigger-manual");
    click(cx, "task-save");

    assert_eq!(Trigger::Manual, h.tasks()[0].trigger);
    assert!(!is_shown(cx, &badge));
}
