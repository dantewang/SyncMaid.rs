//! The headless window every `gui_*` test drives.
//!
//! GPUI's test platform stands in for Windows: nothing is shown, no GPU is touched, the clock is
//! simulated and the folder pickers are answered by the test. Everything else — the editors, the
//! workspace, the run gate, the engine — is the code the app ships, wired the way `main` wires
//! it, minus the services that need a real window handle (tray, single instance, hiding).
#![allow(dead_code)] // Each test binary uses its own subset.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::{
    px, AnyWindowHandle, AppContext as _, Entity, Modifiers, TestAppContext, VisualTestContext,
};
use syncmaid::state::Workspace;
use syncmaid::ui::Root;
use syncmaid::views::MainView;
use syncmaid_core::filtering::FilterRule;
use syncmaid_core::io::{FileSystem, InMemoryFileSystem, PhysicalFileSystem};
use syncmaid_core::model::{
    AppSettings, Destination, DestinationSyncStatus, SyncStrategy, SyncTask,
};
use syncmaid_core::persistence::{ConfigLocation, SettingsStore, StatusStore, TaskStore};
use syncmaid_core::triggers::Trigger;
use tempfile::TempDir;
use uuid::Uuid;

/// One install of SyncMaid: a data folder, the file system everything goes through, and the
/// main view of the window most recently launched over them.
pub struct Harness {
    pub data: TempDir,
    pub fs: Arc<dyn FileSystem>,
    pub config: ConfigLocation,
    /// Read through its public accessors only; tests act through controls.
    pub view: RefCell<Option<Entity<MainView>>>,
}

impl Harness {
    /// The real file system. Only for tests whose task folders are temporary directories.
    pub fn physical() -> Self {
        Self::new(Arc::new(PhysicalFileSystem::new()))
    }

    /// Everything — task folders and config alike — in memory, so paths such as `C:\src` mean
    /// nothing on the machine running the test, and faults can be injected.
    pub fn in_memory(fs: InMemoryFileSystem) -> Self {
        Self::new(Arc::new(fs))
    }

    /// `in_memory` over an empty file system, for tests that never look at its contents.
    pub fn empty() -> Self {
        Self::in_memory(InMemoryFileSystem::new())
    }

    fn new(fs: Arc<dyn FileSystem>) -> Self {
        let data = TempDir::new().expect("data folder");
        let config = ConfigLocation::at(data.path());
        Self {
            data,
            fs,
            config,
            view: RefCell::default(),
        }
    }

    /// Config as if written by hand before launch, bypassing the editors' checks.
    pub fn seed_tasks(&self, tasks: &[SyncTask]) {
        TaskStore::new(Arc::clone(&self.fs), self.config.tasks_path())
            .save(tasks)
            .expect("seed tasks.json");
    }

    /// `tasks.json` exactly as it is on disk, or `None` when nothing was ever saved.
    pub fn tasks_file(&self) -> Option<Vec<u8>> {
        self.fs.read_all_bytes(&self.config.tasks_path()).ok()
    }

    pub fn write_tasks_file(&self, bytes: &[u8]) {
        let path = self.config.tasks_path();
        self.fs
            .ensure_directory(path.parent().expect("tasks.json has a folder"))
            .expect("create the data folder");
        self.fs
            .write_all_bytes(&path, bytes)
            .expect("write tasks.json");
    }

    pub fn tasks(&self) -> Vec<SyncTask> {
        TaskStore::new(Arc::clone(&self.fs), self.config.tasks_path()).load()
    }

    pub fn statuses(&self) -> HashMap<Uuid, DestinationSyncStatus> {
        StatusStore::new(Arc::clone(&self.fs), self.config.status_path()).load()
    }

    pub fn settings(&self) -> AppSettings {
        SettingsStore::new(Arc::clone(&self.fs), self.config.settings_path()).load()
    }

    /// Opens the main window over this install, loading whatever is on disk now. Called again
    /// after `simulate_close`, it is a restart.
    pub fn launch<'a>(&self, cx: &'a mut TestAppContext) -> &'a mut VisualTestContext {
        cx.update(|cx| {
            gpui_kit::init(cx);
            syncmaid::ui::init(cx);
            syncmaid::theme::install(cx);
        });
        let workspace = Workspace::load(Arc::clone(&self.fs), &self.config);
        let fs = Arc::clone(&self.fs);
        let stored_view = &self.view;
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let view = cx.new(|cx| MainView::new(workspace, fs, cx));
            *stored_view.borrow_mut() = Some(view.clone());
            Root::new(view, window, cx)
        });
        settle(cx);
        cx
    }
}

/// A Manual Add-only task with one all-files destination.
pub fn task(name: &str, source: &str, destination: &str) -> SyncTask {
    SyncTask::new(
        name,
        source,
        Trigger::Manual,
        [Destination::new(
            name,
            destination,
            [FilterRule::AllFiles],
            SyncStrategy::AddOnly,
        )],
    )
}

/// Lets every animation finish and the window draw what is now true.
///
/// A sheet slides in. Clicked mid-slide, the press lands on one frame and the release on the
/// next, by then somewhere else — and a press and a release on two different things is not a
/// click. The test platform's clock is simulated, so finishing the slide costs nothing.
pub fn settle(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
}

/// `debug_bounds` wants a `'static` name, and ids carrying a uuid are made at runtime.
fn leak(selector: &str) -> &'static str {
    Box::leak(selector.to_owned().into_boxed_str())
}

/// Whether something called `selector` is drawn, with a size, in this window right now.
pub fn is_shown(cx: &mut VisualTestContext, selector: &str) -> bool {
    settle(cx);
    cx.debug_bounds(leak(selector))
        .is_some_and(|bounds| bounds.size.width > px(0.) && bounds.size.height > px(0.))
}

/// Clicks the middle of whatever is drawn under `selector`, then lets the app settle.
pub fn click(cx: &mut VisualTestContext, selector: &str) {
    settle(cx);
    let selector = leak(selector);
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("nothing on screen is called {selector:?}"));
    assert!(
        bounds.size.width > px(0.) && bounds.size.height > px(0.),
        "{selector:?} is drawn with no size, so it cannot be clicked"
    );
    let window = cx.update(|window, _| window.window_handle());
    cx.simulate_click(bounds.center(), Modifiers::none());
    // A button that closes its own window takes the window with it; there is nothing to redraw.
    if cx.windows().contains(&window) {
        settle(cx);
    }
}

/// Replaces the text of the field called `selector`.
pub fn type_into(cx: &mut VisualTestContext, selector: &str, text: &str) {
    click(cx, selector);
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input(text);
    settle(cx);
}

/// Answers the folder picker the last click opened.
pub fn pick_folder(cx: &mut VisualTestContext, folder: &Path) {
    assert!(
        cx.did_prompt_for_paths(),
        "the click did not open a folder picker"
    );
    let folder = folder.to_path_buf();
    cx.simulate_path_prompt_response(move |_| Some(vec![folder]));
    settle(cx);
}

/// The one window that is not `main` — the confirmation or the log, whichever just opened.
pub fn other_window(cx: &mut VisualTestContext, main: AnyWindowHandle) -> VisualTestContext {
    let handle = cx
        .windows()
        .into_iter()
        .find(|window| *window != main)
        .expect("a second window to be open");
    VisualTestContext::from_window(handle, &cx.cx)
}
