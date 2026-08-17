//! SyncMaid — one-way file sync for Windows, done for you.

// A GUI app should not flash a console. Debug builds keep one so tracing has somewhere to go.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod selftest;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use anyhow::Result;
use gpui::{
    px, size, App, AppContext as _, Application, Bounds, Entity, WindowBounds, WindowOptions,
};
use gpui_component::{Root, TitleBar};
use syncmaid::platform::tray::{TrayCommand, TrayLabels};
use syncmaid::state::Workspace;
use syncmaid::views::MainView;
use syncmaid::{assets, platform, services};
use syncmaid_core::io::{FileSystem, PhysicalFileSystem};
use syncmaid_core::persistence::ConfigLocation;

/// The Avalonia build's window geometry, kept so the port lands in the same place.
const WINDOW_SIZE: (f32, f32) = (940., 620.);
const WINDOW_MIN_SIZE: (f32, f32) = (640., 480.);

fn main() {
    let arguments: Vec<String> = std::env::args().collect();
    let self_test = arguments
        .iter()
        .any(|argument| argument == "--self-test-tray");
    // `--show <dialog>` opens straight into one modal. A development affordance: several of
    // these are three clicks deep, and checking one should not need those three clicks.
    let show = arguments
        .iter()
        .position(|argument| argument == "--show")
        .and_then(|index| arguments.get(index + 1))
        .cloned();

    // Portable: everything SyncMaid writes lives in a Data folder beside the executable, so
    // the whole app is a folder you can copy to a USB stick.
    let config = ConfigLocation::portable();
    services::logging::install(&config.log_path());
    tracing::info!(directory = %config.directory().display(), "SyncMaid starting");

    let file_system: Arc<dyn FileSystem> = Arc::new(PhysicalFileSystem::new());

    Application::new()
        .with_assets(assets::Assets)
        .run(move |cx: &mut App| {
            gpui_component::init(cx);

            let workspace = Workspace::load(Arc::clone(&file_system), &config);
            let (window, view) = open_main_window(workspace, Arc::clone(&file_system), cx)
                .expect("open the main window");

            if let Some(dialog) = show.clone() {
                let _ = window.update(cx, |_, window, cx| {
                    view.update(cx, |view, cx| view.show_dialog(&dialog, window, cx));
                });
            }

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

fn open_main_window(
    workspace: Workspace,
    file_system: Arc<dyn FileSystem>,
    cx: &mut App,
) -> Result<(gpui::WindowHandle<Root>, Entity<MainView>)> {
    let captured: Rc<RefCell<Option<Entity<MainView>>>> = Rc::default();
    // Logical pixels: gpui applies the display scale itself. Scaling the request here as well
    // opens a window that is scale-times too big, with the UI painting its true logical size
    // into the top-left corner of it.
    let bounds = Bounds::centered(None, size(px(WINDOW_SIZE.0), px(WINDOW_SIZE.1)), cx);

    let handle = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitleBar::title_bar_options()),
            window_min_size: Some(size(px(WINDOW_MIN_SIZE.0), px(WINDOW_MIN_SIZE.1))),
            app_id: Some("SyncMaid".into()),
            ..Default::default()
        },
        {
            let captured = Rc::clone(&captured);
            move |window, cx| {
                let view = cx.new(|_| MainView::new(workspace, file_system));
                *captured.borrow_mut() = Some(view.clone());
                // Root is what gives the window its dialog, sheet and notification layers.
                let any: gpui::AnyView = view.into();
                cx.new(|cx| Root::new(any, window, cx))
            }
        },
    )?;

    let view = captured
        .borrow_mut()
        .take()
        .expect("the window built its view");
    Ok((handle, view))
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
