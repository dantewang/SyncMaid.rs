//! The tray-residency gate, run by `SyncMaid.exe --self-test-tray`.
//!
//! gpui breaks its message loop when the last window is *closed*, which would make a tray-only
//! SyncMaid impossible. The port relies on hiding the native window instead, and this is the
//! check that the reliance holds: hide the window, wait well past the point where a broken
//! loop would have ended the process, then confirm the app is still scheduling work and can
//! bring the window back.
//!
//! It is a real gate, not a demo: if hiding killed the loop, the process exits before printing
//! anything and the exit code is not zero.

use std::time::Duration;

use gpui::{App, WindowHandle};
use gpui_component::Root;

use syncmaid::platform::window_visibility;

/// How long to stay hidden. Long enough that a loop which ends on hide has certainly ended.
const HIDDEN_FOR: Duration = Duration::from_secs(3);

pub fn run_tray_gate(window: WindowHandle<Root>, cx: &mut App) {
    cx.spawn(async move |cx| {
        let executor = cx.background_executor().clone();
        executor.timer(Duration::from_millis(800)).await;

        let hidden = window
            .update(cx, |_, window, _| window_visibility::hide(window))
            .expect("hide the window");
        assert!(hidden, "could not reach the native window handle");

        executor.timer(Duration::from_millis(200)).await;
        let visible = window
            .update(cx, |_, window, _| window_visibility::is_visible(window))
            .expect("read window visibility");
        assert!(!visible, "the window was still visible after hiding it");
        println!("[self-test] window hidden; the message loop is still ours");

        // The interesting part: does anything still run while nothing is on screen?
        let mut ticks = 0;
        let deadline = std::time::Instant::now() + HIDDEN_FOR;
        while std::time::Instant::now() < deadline {
            executor.timer(Duration::from_millis(250)).await;
            ticks += 1;
        }
        assert!(
            ticks >= 8,
            "the executor stalled while hidden ({ticks} ticks)"
        );
        println!("[self-test] {ticks} background ticks while hidden — triggers would keep running");

        let shown = window
            .update(cx, |_, window, _| window_visibility::show(window))
            .expect("show the window");
        assert!(shown, "could not show the window again");

        executor.timer(Duration::from_millis(300)).await;
        let visible = window
            .update(cx, |_, window, _| window_visibility::is_visible(window))
            .expect("read window visibility");
        assert!(visible, "the window did not come back");

        println!("[self-test] window restored");
        println!("[self-test] TRAY GATE PASSED");

        cx.update(|cx| cx.quit()).expect("quit");
    })
    .detach();
}
