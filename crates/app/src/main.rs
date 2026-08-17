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

    // Before the window, before the triggers: a second copy over the same Data folder would
    // race the first on every file it writes. The copy already running has been asked to show
    // itself, so the click that started this one still did something.
    let instance = match platform::single_instance::acquire(config.directory()) {
        platform::single_instance::Launch::First(instance) => instance,
        platform::single_instance::Launch::Another => {
            tracing::info!("another copy of this install is already running; showing it instead");
            return;
        }
    };

    let file_system: Arc<dyn FileSystem> = Arc::new(PhysicalFileSystem::new());

    Application::new()
        .with_assets(assets::Assets)
        .run(move |cx: &mut App| {
            gpui_component::init(cx);

            let workspace = Workspace::load(Arc::clone(&file_system), &config);
            // Read before the window exists: "start minimized" is the difference between
            // showing it and never showing it, not something to undo afterwards.
            let start_minimized = workspace.settings().start_minimized;
            let (window, view) =
                open_main_window(workspace, Arc::clone(&file_system), start_minimized, cx)
                    .expect("open the main window");
            close_to_tray(window, view.clone(), cx);

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

            answer_later_launches(cx, window, instance.knock.clone());

            if self_test {
                selftest::run_tray_gate(window, cx);
            }
        });
}

fn open_main_window(
    workspace: Workspace,
    file_system: Arc<dyn FileSystem>,
    start_minimized: bool,
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
            // Never shown rather than shown and hidden: no flash, no taskbar entry. The window
            // still exists, so triggers run and the tray can summon it.
            show: !start_minimized,
            focus: !start_minimized,
            ..Default::default()
        },
        {
            let captured = Rc::clone(&captured);
            move |window, cx| {
                let view = cx.new(|cx| MainView::new(workspace, file_system, cx));
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

/// Makes the window's close button hide it while "close to the system tray" is on.
///
/// Only a user closing the window reaches here. The tray's Exit quits the app outright, which
/// is what keeps Exit able to actually exit.
fn close_to_tray(window: gpui::WindowHandle<Root>, view: Entity<MainView>, cx: &mut App) {
    let _ = window.update(cx, |_, window, cx| {
        window.on_window_should_close(cx, move |window, cx| {
            let hide = view.read(cx).close_to_tray();
            if hide {
                platform::window_visibility::hide(window);
            }
            !hide
        });
    });
}

/// Brings the window forward when someone launches this install again.
///
/// Starting an app that is already running, hidden in the tray, looks like nothing happening.
/// Showing the window is what the second launch almost always meant.
fn answer_later_launches(
    cx: &mut App,
    window: gpui::WindowHandle<Root>,
    knocks: flume::Receiver<()>,
) {
    cx.spawn(async move |cx| {
        while knocks.recv_async().await.is_ok() {
            let shown = cx.update(|cx| {
                let _ = window.update(cx, |_, window, _| {
                    platform::window_visibility::show(window);
                });
            });
            if shown.is_err() {
                return;
            }
        }
    })
    .detach();
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
