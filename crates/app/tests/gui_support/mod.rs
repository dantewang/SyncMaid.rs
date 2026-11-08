//! Shared headless GUI harness.
#![allow(dead_code)]
use gpui_kit::{AnyWindowHandle, AppContext as _, Modifiers, TestAppContext, VisualTestContext};
use std::{collections::HashMap, path::Path, sync::Arc, time::Duration};
use syncmaid::{state::Workspace, ui::Root, views::MainView};
use syncmaid_core::{
    io::{FileSystem, InMemoryFileSystem, PhysicalFileSystem},
    model::{AppSettings, DestinationSyncStatus, SyncTask},
    persistence::{ConfigLocation, SettingsStore, StatusStore, TaskStore},
};
use tempfile::TempDir;
use uuid::Uuid;
pub struct Harness {
    pub data: TempDir,
    pub fs: Arc<dyn FileSystem>,
    pub config: ConfigLocation,
    pub view: std::cell::RefCell<Option<gpui_kit::Entity<MainView>>>,
}
impl Harness {
    pub fn physical() -> Self {
        Self::new(Arc::new(PhysicalFileSystem::new()))
    }
    pub fn in_memory(fs: InMemoryFileSystem) -> Self {
        Self::new(Arc::new(fs))
    }
    fn new(fs: Arc<dyn FileSystem>) -> Self {
        let data = TempDir::new().unwrap();
        let config = ConfigLocation::at(data.path());
        Self {
            data,
            fs,
            config,
            view: Default::default(),
        }
    }
    pub fn seed_tasks(&self, tasks: &[SyncTask]) {
        TaskStore::new(self.fs.clone(), self.config.tasks_path())
            .save(tasks)
            .unwrap();
    }
    pub fn tasks(&self) -> Vec<SyncTask> {
        TaskStore::new(self.fs.clone(), self.config.tasks_path()).load()
    }
    pub fn statuses(&self) -> HashMap<Uuid, DestinationSyncStatus> {
        StatusStore::new(self.fs.clone(), self.config.status_path()).load()
    }
    pub fn settings(&self) -> AppSettings {
        SettingsStore::new(self.fs.clone(), self.config.settings_path()).load()
    }
    pub fn launch<'a>(&self, cx: &'a mut TestAppContext) -> &'a mut VisualTestContext {
        cx.update(|cx| {
            gpui_kit::init(cx);
            syncmaid::ui::init(cx);
            syncmaid::theme::install(cx);
        });
        let workspace = Workspace::load(self.fs.clone(), &self.config);
        let fs = self.fs.clone();
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
pub fn is_shown(cx: &mut VisualTestContext, selector: &str) -> bool {
    settle(cx);
    cx.debug_bounds(Box::leak(selector.to_owned().into_boxed_str()))
        .is_some_and(|b| b.size.width > gpui_kit::px(0.) && b.size.height > gpui_kit::px(0.))
}
pub fn type_into(cx: &mut VisualTestContext, selector: &str, text: &str) {
    click(cx, selector);
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input(text);
    settle(cx);
}
pub fn cancel_folder_picker(cx: &mut VisualTestContext) {
    assert!(cx.did_prompt_for_paths());
    cx.simulate_path_prompt_response(|_| None);
    settle(cx);
}
pub fn other_window(cx: &mut VisualTestContext, main: AnyWindowHandle) -> VisualTestContext {
    let handle = cx
        .windows()
        .into_iter()
        .find(|w| *w != main)
        .expect("auxiliary window");
    VisualTestContext::from_window(handle, &cx.cx)
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

/// Clicks the middle of whatever is drawn under `selector`, then lets the app settle.
pub fn click(cx: &mut VisualTestContext, selector: &str) {
    settle(cx);
    // `debug_bounds` wants a `'static` name, and ids carrying a task's uuid are made at runtime.
    let selector: &'static str = Box::leak(selector.to_owned().into_boxed_str());
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("nothing on screen is called {selector:?}"));
    assert!(
        bounds.size.width > gpui_kit::px(0.) && bounds.size.height > gpui_kit::px(0.),
        "zero-sized selector {selector}"
    );
    let window = cx.update(|window, _| window.window_handle());
    cx.simulate_click(bounds.center(), Modifiers::none());
    if cx.windows().contains(&window) {
        settle(cx);
    }
}

/// Answers the folder picker the last click opened.
pub fn pick_folder(cx: &mut VisualTestContext, folder: &Path) {
    assert!(
        cx.did_prompt_for_paths(),
        "the click did not open a folder picker"
    );
    let folder = folder.to_path_buf();
    cx.simulate_path_prompt_response(move |_| Some(vec![folder]));
    cx.run_until_parked();
}

pub fn task(name: &str, source: &str, destination: &str) -> SyncTask {
    use syncmaid_core::{
        filtering::FilterRule,
        model::{Destination, SyncStrategy},
        triggers::Trigger,
    };
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
