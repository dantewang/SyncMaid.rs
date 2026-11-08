//! Settings flipped in the window land in `settings.json` and reach the app after a restart.
//!
//! Its own test binary: picking a language switches the process-wide UI language.

mod gui_support;

use gpui_kit::TestAppContext;
use gui_support::*;

#[gpui::test]
fn settings_changes_persist_across_a_restart(cx: &mut TestAppContext) {
    let h = Harness::empty();
    let close_to_tray = |cx: &mut gpui_kit::VisualTestContext| {
        cx.read(|cx| {
            h.view
                .borrow()
                .as_ref()
                .expect("launched")
                .read(cx)
                .close_to_tray()
        })
    };
    let cx = h.launch(cx);

    // Never `settings-startup`: that one writes the real Run key.
    click(cx, "sidebar-settings");
    click(cx, "settings-close-to-tray");
    assert!(h.settings().close_to_tray);
    assert!(close_to_tray(cx));

    click(cx, "language-1");
    assert_eq!(Some("en"), h.settings().language.as_deref());
    click(cx, "language-0");
    assert_eq!(
        None,
        h.settings().language,
        "following the system is no tag"
    );

    cx.simulate_close();
    let cx = h.launch(cx);
    assert!(close_to_tray(cx));
    click(cx, "sidebar-settings");
    click(cx, "settings-close-to-tray");
    assert!(!h.settings().close_to_tray);
}
