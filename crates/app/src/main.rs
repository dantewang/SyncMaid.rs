//! SyncMaid — one-way file sync for Windows, done for you.

// A GUI app should not flash a console. Debug builds keep one so tracing has somewhere to go.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod assets;
mod platform;
mod selftest;
mod views;

use anyhow::Result;
use gpui::{px, size, App, AppContext as _, Application, Bounds, WindowBounds, WindowOptions};
use gpui_component::{Root, TitleBar};

use crate::platform::tray::{TrayCommand, TrayLabels};
use crate::views::MainView;

/// The Avalonia build's window geometry, kept so the port lands in the same place.
const WINDOW_SIZE: (f32, f32) = (940., 620.);
const WINDOW_MIN_SIZE: (f32, f32) = (640., 480.);

fn main() {
    let self_test = std::env::args().any(|argument| argument == "--self-test-tray");

    Application::new()
        .with_assets(assets::Assets)
        .run(move |cx: &mut App| {
            gpui_component::init(cx);

            let window = open_main_window(cx).expect("open the main window");

            match start_tray(cx, window) {
                Ok(()) => {}
                // A missing tray is a degraded app, not a broken one: everything still works
                // from the window. Refusing to start over it would be the worse trade.
                Err(error) => eprintln!("the tray could not be started: {error:#}"),
            }

            if self_test {
                selftest::run_tray_gate(window, cx);
            }
        });
}

fn open_main_window(cx: &mut App) -> Result<gpui::WindowHandle<Root>> {
    let bounds = Bounds::centered(None, size(px(WINDOW_SIZE.0), px(WINDOW_SIZE.1)), cx);

    let handle = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitleBar::title_bar_options()),
            window_min_size: Some(size(px(WINDOW_MIN_SIZE.0), px(WINDOW_MIN_SIZE.1))),
            app_id: Some("SyncMaid".into()),
            ..Default::default()
        },
        |window, cx| {
            // Root is what gives the window its dialog, sheet and notification layers.
            let view: gpui::AnyView = cx.new(|_| MainView).into();
            cx.new(|cx| Root::new(view, window, cx))
        },
    )?;

    Ok(handle)
}

/// Wires the tray to the window: its commands arrive on a channel from the tray's own thread
/// and are applied here, on the UI thread.
fn start_tray(cx: &mut App, window: gpui::WindowHandle<Root>) -> Result<()> {
    let commands = platform::tray::start(TrayLabels {
        tooltip: "SyncMaid".into(),
        show_main_window: "Show main window".into(),
        exit: "Exit".into(),
    })?;

    cx.spawn(async move |cx| {
        while let Ok(command) = commands.recv_async().await {
            let applied = cx.update(|cx| match command {
                TrayCommand::ShowMainWindow => {
                    let _ = window.update(cx, |_, window, _| {
                        platform::window_visibility::show(window);
                    });
                }
                TrayCommand::Exit => cx.quit(),
            });

            if applied.is_err() {
                break;
            }
        }
    })
    .detach();

    Ok(())
}
