mod gui_support;
use gpui_kit::TestAppContext;
use gui_support::*;
#[gpui::test]
fn settings_changes_persist(cx: &mut TestAppContext) {
    let h = Harness::physical();
    let cx = h.launch(cx);
    click(cx, "sidebar-settings");
    click(cx, "settings-close-to-tray");
    assert!(h.settings().close_to_tray);
    assert!(cx.read(|cx| h.view.borrow().as_ref().unwrap().read(cx).close_to_tray()));
    click(cx, "language-1");
    assert_eq!(h.settings().language.as_deref(), Some("en"));
    click(cx, "language-0");
    assert_eq!(h.settings().language, None);
    cx.simulate_close();
    let cx = h.launch(cx);
    assert!(cx.read(|cx| h.view.borrow().as_ref().unwrap().read(cx).close_to_tray()));
    click(cx, "sidebar-settings");
    click(cx, "settings-close-to-tray");
    assert!(!h.settings().close_to_tray);
}
