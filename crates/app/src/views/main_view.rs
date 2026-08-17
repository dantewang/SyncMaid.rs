//! The main window: title bar, task sidebar, task list.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, FixedOffset, Local};

use gpui::{
    div, img, prelude::*, px, Context, Entity, FontWeight, SharedString, Subscription, Window,
    WindowControlArea,
};
use syncmaid_core::io::FileSystem;
use syncmaid_core::model::{
    Destination, DestinationSyncStatus, SyncOutcome, SyncStrategy, SyncTask, SyncTaskKind,
};
use syncmaid_core::sync::{SyncEngine, SyncOperation, SyncProgress};
use syncmaid_core::triggers::{CronSchedule, DefaultTriggerSourceFactory, Notification, Trigger};
use uuid::Uuid;

use crate::components::{
    icon, Badge, BadgeTone, Button, ButtonTone, HintBox, HintTone, Icon, IconButton, IconButtonTone,
};
use crate::state::{health_of, RunGate, TriggerEvent, TriggerHost, Workspace};
use crate::strings;
use crate::theme;
use crate::views::dialogs::{
    ConfirmDialog, ConfirmEvent, SettingsDialog, SettingsEvent, TaskEditor, TaskEditorEvent,
    TaskWorkspace, TaskWorkspaceEvent,
};
use crate::views::mirror_delete::{self, MirrorDeleteDecision};

/// How often the "next run in ..." labels are re-made. Often enough that a minutes-away label
/// is never wrong by much, rare enough to cost nothing while the window sits idle.
const NEXT_RUN_REFRESH: Duration = Duration::from_secs(30);

/// Which modal is open, and what it will do when it says yes.
enum ActiveDialog {
    Confirm(Entity<ConfirmDialog>),
    TaskEditor(Entity<TaskEditor>),
    Workspace(Entity<TaskWorkspace>),
    Settings(Entity<SettingsDialog>),
}

/// What a confirmation is confirming.
#[derive(Debug, Clone, Copy)]
enum PendingAction {
    DeleteTask(Uuid),
    DeleteDestination { task: Uuid, destination: Uuid },
}

/// The main window's content.
pub struct MainView {
    workspace: Workspace,
    file_system: Arc<dyn FileSystem>,
    engine: Arc<SyncEngine>,
    /// One gate per task: runs of a task are serialized, runs of different tasks are not.
    gates: HashMap<Uuid, Arc<RunGate>>,
    /// Live progress text per destination, replaced by the row's status when the run ends.
    progress: HashMap<Uuid, String>,
    dialog: Option<ActiveDialog>,
    /// Dropped when the dialog closes, which is what unsubscribes it.
    dialog_subscription: Option<Subscription>,

    /// The live trigger runners. Dropping this stops every one of them.
    triggers: TriggerHost,
    /// Why a task will not run automatically, keyed by task. Shown as an amber badge.
    trigger_errors: HashMap<Uuid, String>,
    /// When each scheduled task fires next, refreshed on a timer so the label stays honest.
    next_runs: HashMap<Uuid, DateTime<Local>>,
}

impl MainView {
    pub fn new(
        workspace: Workspace,
        file_system: Arc<dyn FileSystem>,
        cx: &mut Context<Self>,
    ) -> Self {
        let factory = Arc::new(DefaultTriggerSourceFactory::new(Arc::clone(&file_system)));
        let (triggers, events) = TriggerHost::new(factory);

        let mut view = Self {
            engine: Arc::new(SyncEngine::local(Arc::clone(&file_system))),
            workspace,
            file_system,
            gates: HashMap::new(),
            progress: HashMap::new(),
            dialog: None,
            dialog_subscription: None,
            triggers,
            trigger_errors: HashMap::new(),
            next_runs: HashMap::new(),
        };
        view.sync_triggers(cx);

        // Trigger runners fire from their own threads; this is where what they say arrives
        // somewhere it is safe to act on.
        cx.spawn(async move |view, cx| {
            while let Ok(event) = events.recv_async().await {
                if view
                    .update(cx, |view, cx| view.on_trigger(event, cx))
                    .is_err()
                {
                    return; // The window is gone.
                }
            }
        })
        .detach();

        // "next run in 2 h" is a claim that goes stale by itself, so it is re-made on a timer
        // rather than only when something else happens to redraw.
        cx.spawn(async move |view, cx| loop {
            cx.background_executor().timer(NEXT_RUN_REFRESH).await;
            let refreshed = view.update(cx, |view, cx| {
                view.refresh_next_runs();
                cx.notify();
            });
            if refreshed.is_err() {
                return;
            }
        })
        .detach();

        view
    }

    fn on_trigger(&mut self, event: TriggerEvent, cx: &mut Context<Self>) {
        match event.notification {
            Notification::Fired => self.run_task(event.task_id, HashSet::new(), cx),
            // The reason stays as the OS produced it; only the sentence around it is ours.
            Notification::Error(reason) => {
                self.trigger_errors.insert(
                    event.task_id,
                    strings::task_trigger_error_recoverable_format(reason),
                );
                cx.notify();
            }
            Notification::Recovered => {
                self.trigger_errors.remove(&event.task_id);
                cx.notify();
            }
        }
    }

    /// Brings the running triggers in line with the tasks, after anything that changed them.
    fn sync_triggers(&mut self, cx: &mut Context<Self>) {
        let tasks = self.workspace.tasks().to_vec();
        for start in self.triggers.reconcile(&tasks) {
            match start.error {
                // Degraded to manual-only — but said out loud, because a task that silently
                // never runs is the worst of the three outcomes.
                Some(reason) => self.trigger_errors.insert(
                    start.task_id,
                    strings::task_trigger_error_start_format(reason),
                ),
                None => self.trigger_errors.remove(&start.task_id),
            };
        }
        self.refresh_next_runs();
        cx.notify();
    }

    /// Recomputes when each scheduled task fires next.
    ///
    /// A cron expression that will not parse, or has no future occurrence at all, simply has no
    /// next run — the trigger reports its own failure separately.
    fn refresh_next_runs(&mut self) {
        let now = Local::now();
        self.next_runs = self
            .workspace
            .tasks()
            .iter()
            .filter_map(|task| match &task.trigger {
                Trigger::Scheduled { cron_expression } => {
                    let next = CronSchedule::parse(cron_expression)
                        .ok()?
                        .next_occurrence_after(now)?;
                    Some((task.id, next))
                }
                _ => None,
            })
            .collect();
    }

    /// Opens one modal by name, for `SyncMaid.exe --show <dialog>`.
    ///
    /// Several of these sit three clicks deep; checking one should not need those three clicks.
    pub fn show_dialog(&mut self, dialog: &str, window: &mut Window, cx: &mut Context<Self>) {
        let first_task = self.workspace.tasks().first().map(|task| task.id);
        match dialog {
            "task" => self.open_task_editor(None, window, cx),
            "task-edit" => {
                if let Some(id) = first_task {
                    self.open_task_editor(Some(id), window, cx);
                }
            }
            "destination" => {
                if let Some(id) = first_task {
                    self.open_workspace(id, None, true, window, cx);
                }
            }
            "destination-edit" => {
                if let Some((task, destination)) = self
                    .workspace
                    .tasks()
                    .first()
                    .and_then(|task| Some((task.id, task.destinations.first()?.id)))
                {
                    self.open_workspace(task, Some(destination), false, window, cx);
                }
            }
            "workspace" => {
                if let Some(id) = first_task {
                    self.open_workspace(id, None, false, window, cx);
                }
            }
            // The routing view only exists for a Move task, so it names one rather than taking
            // whichever task happens to be first.
            "routing" => {
                if let Some(id) = self
                    .workspace
                    .tasks()
                    .iter()
                    .find(|task| task.kind() == SyncTaskKind::Move)
                    .map(|task| task.id)
                {
                    self.open_workspace(id, None, false, window, cx);
                }
            }
            // The real preview against the real destination — but it never starts a run, which
            // the review button itself does. A developer flag must not move anyone's files.
            "mirror-delete" => {
                let found = self.workspace.tasks().iter().find_map(|task| {
                    task.destinations
                        .iter()
                        .filter(|candidate| candidate.strategy == SyncStrategy::Mirror)
                        .find_map(|candidate| {
                            let preview = self.engine.preview_mirror_deletions(task, candidate.id);
                            (preview.count > 0).then(|| (candidate.clone(), preview))
                        })
                });
                let Some((destination, preview)) = found else {
                    tracing::warn!("no Mirror destination would delete anything to confirm");
                    return;
                };
                mirror_delete::ask(&destination, preview, cx, |decision, _| {
                    tracing::info!(?decision, "the confirmation window was answered");
                });
            }
            "settings" => self.open_settings(cx),
            "confirm" => {
                if let Some(id) = first_task {
                    let Some(task) = self.workspace.task(id) else {
                        return;
                    };
                    let confirm = ConfirmDialog::delete_task(&task.name, task.destinations.len());
                    self.open_confirm(confirm, PendingAction::DeleteTask(id), cx);
                }
            }
            other => tracing::warn!("no dialog called {other:?}"),
        }
    }

    /// Whether closing the window should hide it instead of quitting.
    pub fn close_to_tray(&self) -> bool {
        self.workspace.settings().close_to_tray
    }

    fn gate_for(&mut self, task_id: Uuid) -> Arc<RunGate> {
        Arc::clone(
            self.gates
                .entry(task_id)
                .or_insert_with(|| Arc::new(RunGate::new())),
        )
    }

    fn close_dialog(&mut self, cx: &mut Context<Self>) {
        self.dialog = None;
        self.dialog_subscription = None;
        cx.notify();
    }

    fn open_task_editor(
        &mut self,
        task_id: Option<Uuid>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let tasks = self.workspace.tasks().to_vec();
        let file_system = Arc::clone(&self.file_system);
        let existing = task_id.and_then(|id| self.workspace.task(id).cloned());

        let editor = cx.new(|cx| match &existing {
            Some(task) => TaskEditor::edit(task, tasks, file_system, window, cx),
            None => TaskEditor::new_task(tasks, file_system, window, cx),
        });

        self.dialog_subscription = Some(cx.subscribe(
            &editor,
            |view, _, event: &TaskEditorEvent, cx| match event {
                TaskEditorEvent::Saved(task) => {
                    view.workspace.upsert_task((**task).clone());
                    // The trigger or the source may have changed under it.
                    view.sync_triggers(cx);
                    view.close_dialog(cx);
                }
                TaskEditorEvent::Cancelled => view.close_dialog(cx),
            },
        ));
        self.dialog = Some(ActiveDialog::TaskEditor(editor));
        cx.notify();
    }

    /// Opens the task's destination workspace.
    ///
    /// Every destination edit goes through it: which rule catches a file is a property of the
    /// whole ordered list, so the list is what is edited — the card's add and edit buttons just
    /// say where to start.
    fn open_workspace(
        &mut self,
        task_id: Uuid,
        expand: Option<Uuid>,
        start_with_new_rule: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(task) = self.workspace.task(task_id).cloned() else {
            return;
        };
        let tasks = self.workspace.tasks().to_vec();
        let file_system = Arc::clone(&self.file_system);

        let editor = cx.new(|cx| {
            TaskWorkspace::new(
                &task,
                tasks,
                file_system,
                expand,
                start_with_new_rule,
                window,
                cx,
            )
        });

        self.dialog_subscription = Some(cx.subscribe(
            &editor,
            move |view, _, event: &TaskWorkspaceEvent, cx| match event {
                TaskWorkspaceEvent::Saved(destinations) => {
                    view.save_destinations(task_id, destinations.clone());
                    view.close_dialog(cx);
                }
                TaskWorkspaceEvent::Cancelled => view.close_dialog(cx),
            },
        ));
        self.dialog = Some(ActiveDialog::Workspace(editor));
        cx.notify();
    }

    /// Replaces the task's destination list wholesale.
    ///
    /// Wholesale rather than one at a time, because for a Move task the order **is** the
    /// matching order: a merge that preserved the old positions would silently undo a reorder
    /// the user just made.
    fn save_destinations(
        &mut self,
        task_id: Uuid,
        destinations: Vec<syncmaid_core::model::Destination>,
    ) {
        let Some(mut task) = self.workspace.task(task_id).cloned() else {
            return;
        };
        task.destinations = destinations;
        self.workspace.upsert_task(task);
        // Statuses of destinations that are gone go with them.
        self.workspace.persist_statuses();
    }

    fn open_settings(&mut self, cx: &mut Context<Self>) {
        let settings = self.workspace.settings().clone();
        let directory = self.workspace.data_directory().to_path_buf();
        let dialog = cx.new(|_| SettingsDialog::new(settings, directory));

        self.dialog_subscription = Some(cx.subscribe(
            &dialog,
            |view, _, event: &SettingsEvent, cx| match event {
                // Applied the moment the switch is flipped; there is no save step.
                SettingsEvent::Changed(settings) => {
                    let settings = settings.clone();
                    view.workspace
                        .update_settings(|current| *current = settings);
                    cx.notify();
                }
                SettingsEvent::Closed => view.close_dialog(cx),
            },
        ));
        self.dialog = Some(ActiveDialog::Settings(dialog));
        cx.notify();
    }

    fn open_confirm(
        &mut self,
        dialog: ConfirmDialog,
        action: PendingAction,
        cx: &mut Context<Self>,
    ) {
        let entity = cx.new(|_| dialog);
        self.dialog_subscription = Some(cx.subscribe(
            &entity,
            move |view, _, event: &ConfirmEvent, cx| {
                if *event == ConfirmEvent::Confirmed {
                    view.apply_pending(action, cx);
                }
                view.close_dialog(cx);
            },
        ));
        self.dialog = Some(ActiveDialog::Confirm(entity));
        cx.notify();
    }

    fn apply_pending(&mut self, action: PendingAction, _cx: &mut Context<Self>) {
        match action {
            PendingAction::DeleteTask(task_id) => {
                // Stop it first: a task that is mid-run must not keep writing after it is gone.
                if let Some(gate) = self.gates.remove(&task_id) {
                    gate.refuse_further_requests();
                }
                self.triggers.stop(task_id);
                self.trigger_errors.remove(&task_id);
                self.next_runs.remove(&task_id);
                self.workspace.remove_task(task_id);
            }
            PendingAction::DeleteDestination { task, destination } => {
                if let Some(mut owner) = self.workspace.task(task).cloned() {
                    owner
                        .destinations
                        .retain(|existing| existing.id != destination);
                    self.workspace.upsert_task(owner);
                    self.workspace.persist_statuses();
                }
            }
        }
    }

    /// A destination is blocked on a mass-delete confirmation: preview the current deletions,
    /// ask in an independent window, and re-run just that destination if the user approves.
    fn review_deletions(&mut self, task_id: Uuid, destination_id: Uuid, cx: &mut Context<Self>) {
        let Some(task) = self.workspace.task(task_id).cloned() else {
            return;
        };
        let Some(destination) = task
            .destinations
            .iter()
            .find(|candidate| candidate.id == destination_id)
            .cloned()
        else {
            return;
        };
        let engine = Arc::clone(&self.engine);

        cx.spawn(async move |view, cx| {
            // Off the UI thread: the preview walks the destination tree, which over a network
            // share is not something to do between two frames.
            let preview = cx
                .background_executor()
                .spawn(async move { engine.preview_mirror_deletions(&task, destination_id) })
                .await;

            let _ = cx.update(|cx| {
                if preview.count == 0 {
                    // The situation resolved — the source came back, say — so there is nothing
                    // to confirm and the run can simply go.
                    let _ = view.update(cx, |view, cx| view.run_task(task_id, HashSet::new(), cx));
                    return;
                }

                let view = view.clone();
                mirror_delete::ask(&destination, preview, cx, move |decision, cx| {
                    if decision == MirrorDeleteDecision::Delete {
                        let _ = view.update(cx, |view, cx| {
                            // Scoped to the one destination that was reviewed, and to this run:
                            // the approval is never written to disk.
                            view.run_task(task_id, HashSet::from([destination_id]), cx)
                        });
                    }
                });
            });
        })
        .detach();
    }

    /// Starts a run, or folds the request into the one already going.
    fn run_task(
        &mut self,
        task_id: Uuid,
        confirmed_mass_deletes: HashSet<Uuid>,
        cx: &mut Context<Self>,
    ) {
        let Some(task) = self.workspace.task(task_id).cloned() else {
            return;
        };
        let gate = self.gate_for(task_id);
        let engine = Arc::clone(&self.engine);
        let before = self.workspace.status_snapshot(&task);

        self.workspace.mark_running(&task);
        cx.notify();

        cx.spawn(async move |view, cx| {
            let Some(mut start) = gate.request(confirmed_mass_deletes) else {
                return; // An active run absorbed it; its drain will pick it up.
            };

            loop {
                let (reports, progress) = flume::unbounded::<SyncProgress>();
                let forwarding = {
                    let view = view.clone();
                    cx.spawn(async move |cx| {
                        while let Ok(report) = progress.recv_async().await {
                            let _ = view.update(cx, |view, cx| {
                                view.progress
                                    .insert(report.destination_id, describe_progress(&report));
                                cx.notify();
                            });
                        }
                    })
                };

                let run = {
                    let engine = Arc::clone(&engine);
                    let task = task.clone();
                    let start = start.clone();
                    cx.background_executor().spawn(async move {
                        let outcome = engine.execute(
                            &task,
                            &start.cancellation,
                            &mut |report| {
                                let _ = reports.send(report);
                            },
                            &start.confirmed_mass_deletes,
                        );
                        // Dropping the sender is what ends the forwarding task.
                        drop(reports);
                        outcome
                    })
                };

                let outcome = run.await;
                forwarding.await;

                let applied = view.update(cx, |view, cx| {
                    view.progress.clear();
                    match outcome {
                        Ok(statuses) => {
                            for status in &statuses {
                                log_destination(&task, status);
                            }
                            view.workspace.apply_statuses(statuses);
                        }
                        // Cancellation is not a failure: what landed stays, and the rows go
                        // back to what they said before rather than inventing an outcome.
                        Err(_) => view.workspace.restore_statuses(before.clone()),
                    }
                    cx.notify();
                });
                if applied.is_err() {
                    return;
                }

                match gate.next() {
                    Some(next) => start = next,
                    None => return,
                }
            }
        })
        .detach();
    }

    fn stop_task(&mut self, task_id: Uuid, cx: &mut Context<Self>) {
        if let Some(gate) = self.gates.get(&task_id) {
            gate.cancel();
        }
        cx.notify();
    }
}

/// `Copying photos/2024/img_0042.jpg (3/120)`
fn describe_progress(report: &SyncProgress) -> String {
    let verb = match report.operation {
        SyncOperation::Copy { .. } => strings::progress_copying(),
        SyncOperation::Move { .. } => strings::progress_moving(),
        SyncOperation::Delete { .. } | SyncOperation::DeleteDirectory { .. } => {
            strings::progress_removing()
        }
        SyncOperation::CreateDirectory { .. } => strings::progress_creating(),
        SyncOperation::SetDirectoryTimestamp { .. } => strings::progress_tidying(),
    };
    strings::progress_line_format(
        verb,
        report.operation.relative_path(),
        report.completed_operations + 1,
        report.total_operations,
    )
}

/// One line per destination per run — the run history the rows do not show.
fn log_destination(task: &SyncTask, status: &DestinationSyncStatus) {
    let name = task
        .destinations
        .iter()
        .find(|destination| destination.id == status.destination_id)
        .map_or("?", |destination| destination.name.as_str());

    match status.outcome {
        SyncOutcome::Failed | SyncOutcome::NeedsConfirmation => tracing::warn!(
            "Sync '{}' → '{}': {:?} · {}",
            task.name,
            name,
            status.outcome,
            status.error.as_deref().unwrap_or("")
        ),
        _ => tracing::info!(
            "Sync '{}' → '{}': {:?} · {} copied, {} in use",
            task.name,
            name,
            status.outcome,
            status.files_copied,
            status.files_deferred
        ),
    }
}

impl Render for MainView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Pinned to the viewport, in the units gpui lays out in — which is what
        // `viewport_size` already reports. Dividing it by the scale factor was a mistake that
        // painted the whole UI into the top-left two-thirds of the window on a scaled display.
        let viewport = window.viewport_size();

        div()
            .relative()
            .flex()
            .flex_col()
            .w(viewport.width)
            .h(viewport.height)
            .bg(theme::color(theme::PAGE))
            .text_size(theme::text::body())
            .text_color(theme::color(theme::TEXT_PRIMARY))
            .child(self.render_title_bar(cx))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .child(self.render_sidebar(cx))
                    .child(self.render_main_pane(cx)),
            )
            .children(self.render_modal())
    }
}

impl MainView {
    /// The scrim and the one dialog on it.
    ///
    /// It covers the title bar too, which is deliberate: while a modal is open the window
    /// cannot be dragged, and the app should look as unavailable as it is.
    fn render_modal(&self) -> Option<impl IntoElement> {
        let dialog = self.dialog.as_ref()?;
        Some(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .bg(theme::color_with_alpha(theme::BACKDROP))
                .child(match dialog {
                    ActiveDialog::Confirm(entity) => entity.clone().into_any_element(),
                    ActiveDialog::TaskEditor(entity) => entity.clone().into_any_element(),
                    ActiveDialog::Workspace(entity) => entity.clone().into_any_element(),
                    ActiveDialog::Settings(entity) => entity.clone().into_any_element(),
                }),
        )
    }
}

impl MainView {
    /// 40 px, app-drawn, with the OS still owning drag and the three window controls.
    fn render_title_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_row()
            .items_center()
            .h(theme::layout::title_bar_height())
            .child(
                div()
                    .id("title-bar-drag")
                    .flex()
                    .flex_row()
                    .items_center()
                    .flex_1()
                    .h_full()
                    .pl(px(12.))
                    .gap(px(8.))
                    // Handing the strip to the OS is what keeps dragging, double-click to
                    // maximize and Win+arrow snapping working on a bar we drew ourselves.
                    .window_control_area(WindowControlArea::Drag)
                    .child(img("syncmaid-32.png").size(px(16.)))
                    .child(
                        div()
                            .text_size(theme::text::body())
                            .text_color(theme::color(theme::TEXT_SECONDARY))
                            .child("SyncMaid"),
                    ),
            )
            .child(
                IconButton::new("settings", Icon::CogOutline)
                    .tone(IconButtonTone::Caption)
                    .tooltip(strings::main_settings_tip())
                    .on_click(cx.listener(|view, _, _, cx| view.open_settings(cx))),
            )
            .child(
                IconButton::new("minimize", Icon::WindowMinimize)
                    .tone(IconButtonTone::Caption)
                    .tooltip(strings::main_minimize_tip())
                    .window_control(WindowControlArea::Min),
            )
            .child(
                IconButton::new("maximize", Icon::WindowMaximize)
                    .tone(IconButtonTone::Caption)
                    .tooltip(strings::main_maximize_tip())
                    .window_control(WindowControlArea::Max),
            )
            .child(
                IconButton::new("close", Icon::WindowClose)
                    .tone(IconButtonTone::CaptionClose)
                    .tooltip(strings::main_close_tip())
                    .window_control(WindowControlArea::Close),
            )
    }

    /// The task list, or the thin rail it collapses to.
    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.workspace.sidebar_visible() {
            return div()
                .flex()
                .flex_col()
                .items_center()
                .w(theme::layout::rail_width())
                .pt(px(8.))
                .child(
                    IconButton::new("expand-sidebar", Icon::ChevronRight)
                        .tone(IconButtonTone::Caption)
                        .small()
                        .tooltip(strings::main_show_sidebar_tip())
                        .on_click(cx.listener(|view, _, _, cx| {
                            view.workspace.toggle_sidebar();
                            cx.notify();
                        })),
                );
        }

        let selected = self.workspace.selected();
        let items: Vec<_> = self
            .workspace
            .tasks()
            .iter()
            .map(|task| self.render_sidebar_item(task, selected == Some(task.id), cx))
            .collect();

        div()
            .flex()
            .flex_col()
            .w(theme::layout::sidebar_width())
            .p(px(8.))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .pl(px(6.))
                    .pt(px(6.))
                    .pb(px(8.))
                    .child(
                        div()
                            .text_size(theme::text::small())
                            .text_color(theme::color(theme::TEXT_SECONDARY))
                            .child(strings::main_tasks_heading()),
                    )
                    .child(
                        IconButton::new("collapse-sidebar", Icon::ChevronLeft)
                            .tone(IconButtonTone::Caption)
                            .small()
                            .tooltip(strings::main_hide_sidebar_tip())
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.workspace.toggle_sidebar();
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .id("sidebar-list")
                    .flex()
                    .flex_col()
                    .overflow_y_scroll()
                    .children(items),
            )
    }

    fn render_sidebar_item(
        &self,
        task: &SyncTask,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let id = task.id;
        div()
            .id(SharedString::from(format!("sidebar-{id}")))
            .flex()
            .flex_row()
            .items_center()
            .my(px(1.))
            .px(px(8.))
            .py(px(6.))
            .rounded(theme::radius::control())
            .when(selected, |element| {
                element.bg(theme::color(theme::TEAL_SUBTLE))
            })
            .hover(|style| style.bg(theme::color(theme::SUBTLE)))
            .cursor_pointer()
            .on_click(cx.listener(move |view, _, _, cx| {
                view.workspace.select(id);
                cx.notify();
            }))
            .child(div().mr(px(9.)).child(icon(
                Icon::FolderOutline,
                px(18.),
                theme::color(theme::TEAL),
            )))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .child(
                        div()
                            .text_size(theme::text::medium())
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::color(theme::TEAL))
                            .text_ellipsis()
                            .child(task.name.clone()),
                    )
                    .child(path_text(&task.source_path)),
            )
    }

    fn render_main_pane(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let cards: Vec<_> = self
            .workspace
            .tasks()
            .iter()
            .map(|task| self.render_task_card(task, cx))
            .collect();
        let all_expanded = self.workspace.all_expanded();

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .bg(theme::color(theme::SURFACE))
            .rounded_tl(theme::radius::card())
            .px(px(16.))
            .py(px(12.))
            .child(self.render_header(all_expanded, cx))
            .when(self.workspace.config_unreadable(), |element| {
                element.child(div().pb(px(12.)).child(config_unreadable_banner()))
            })
            .when(cards.is_empty(), |element| element.child(empty_state()))
            .child(
                div()
                    .id("task-list")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .overflow_y_scroll()
                    .children(cards),
            )
    }

    fn render_header(&self, all_expanded: bool, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .pb(px(12.))
            .child(
                div()
                    .flex_1()
                    .text_size(theme::text::heading())
                    .font_weight(FontWeight::MEDIUM)
                    .child(strings::main_sync_tasks_heading()),
            )
            .child(
                Button::new(
                    "toggle-expand",
                    if all_expanded {
                        strings::main_collapse_all()
                    } else {
                        strings::main_expand_all()
                    },
                )
                .tone(ButtonTone::Secondary)
                .on_click(cx.listener(move |view, _, _, cx| {
                    view.workspace.set_all_expanded(!all_expanded);
                    cx.notify();
                })),
            )
            .child(
                Button::new("run-all", strings::main_run_all())
                    .tone(ButtonTone::Secondary)
                    .glyph(Icon::Play)
                    // Every task at once, but each behind its own gate: runs of one task are
                    // serialized, runs of different tasks are not.
                    .disabled(
                        !self
                            .workspace
                            .tasks()
                            .iter()
                            .any(|task| !task.destinations.is_empty()),
                    )
                    .on_click(cx.listener(|view, _, _, cx| {
                        let runnable: Vec<Uuid> = view
                            .workspace
                            .tasks()
                            .iter()
                            .filter(|task| !task.destinations.is_empty())
                            .map(|task| task.id)
                            .collect();
                        for id in runnable {
                            view.run_task(id, HashSet::new(), cx);
                        }
                    })),
            )
            .child(
                Button::new("new-task", strings::main_new_task())
                    .glyph(Icon::Plus)
                    .on_click(
                        cx.listener(|view, _, window, cx| view.open_task_editor(None, window, cx)),
                    ),
            )
    }

    fn render_task_card(&self, task: &SyncTask, cx: &mut Context<Self>) -> impl IntoElement {
        let id = task.id;
        let expanded = self.workspace.is_expanded(id);
        let health = health_of(task, self.workspace.statuses());
        let (health_glyph, health_color) = outcome_appearance(health.outcome);
        let running = health.outcome == SyncOutcome::Running;
        let rows: Vec<_> = task
            .destinations
            .iter()
            .map(|destination| {
                self.render_destination_row(
                    id,
                    destination,
                    self.workspace.statuses().get(&destination.id),
                    cx,
                )
            })
            .collect();

        div()
            .flex()
            .flex_col()
            .mb(px(12.))
            .bg(theme::color(theme::SURFACE))
            .border_1()
            .border_color(theme::color(theme::HAIRLINE))
            .rounded(theme::radius::card())
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .m(px(10.))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .child(
                                IconButton::new(
                                    SharedString::from(format!("expand-{id}")),
                                    if expanded {
                                        Icon::ChevronDown
                                    } else {
                                        Icon::ChevronRight
                                    },
                                )
                                .tone(IconButtonTone::Caption)
                                .small()
                                .glyph_size(px(18.))
                                .on_click(cx.listener(
                                    move |view, _, _, cx| {
                                        view.workspace.toggle_expanded(id);
                                        cx.notify();
                                    },
                                )),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .size(theme::layout::task_chip())
                                    .ml(px(4.))
                                    .mr(px(11.))
                                    .rounded(theme::radius::block())
                                    .bg(theme::color(theme::TEAL_SUBTLE))
                                    .child(icon(
                                        Icon::FolderOutline,
                                        px(19.),
                                        theme::color(theme::TEAL),
                                    )),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .child(
                                        div()
                                            .flex()
                                            .flex_row()
                                            .items_center()
                                            .gap(px(8.))
                                            // Wraps rather than truncates. Every badge says
                                            // something the name does not, and a card that
                                            // grows a line is cheaper than a fact that
                                            // silently disappears off the right edge.
                                            .flex_wrap()
                                            .child(
                                                div()
                                                    .text_size(theme::text::large())
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .child(task.name.clone()),
                                            )
                                            .child(kind_badge(task.kind()))
                                            .child(trigger_badge(&task.trigger))
                                            .children(
                                                self.next_runs
                                                    .get(&id)
                                                    .map(|next| next_run_badge(id, *next)),
                                            )
                                            .children(
                                                self.trigger_errors
                                                    .get(&id)
                                                    .map(|reason| trigger_error_badge(id, reason)),
                                            ),
                                    )
                                    .child(path_text(&task.source_path)),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(14.))
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap(px(5.))
                                    .text_color(theme::color(health_color))
                                    .child(icon(health_glyph, px(15.), theme::color(health_color)))
                                    .child(health.text),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .gap(px(5.))
                                    .child(if running {
                                        IconButton::new(
                                            SharedString::from(format!("stop-{id}")),
                                            Icon::Stop,
                                        )
                                        .tone(IconButtonTone::Danger)
                                        .tooltip(strings::task_stop_tip())
                                        .on_click(
                                            cx.listener(move |view, _, _, cx| {
                                                view.stop_task(id, cx)
                                            }),
                                        )
                                    } else {
                                        IconButton::new(
                                            SharedString::from(format!("run-{id}")),
                                            Icon::Play,
                                        )
                                        .tone(IconButtonTone::Run)
                                        .tooltip(strings::task_run_now_tip())
                                        .disabled(task.destinations.is_empty())
                                        .on_click(
                                            cx.listener(move |view, _, _, cx| {
                                                view.run_task(id, HashSet::new(), cx)
                                            }),
                                        )
                                    })
                                    .child(
                                        IconButton::new(
                                            SharedString::from(format!("add-{id}")),
                                            Icon::Plus,
                                        )
                                        .tooltip(add_destination_hint(task.kind()))
                                        .on_click(
                                            cx.listener(move |view, _, window, cx| {
                                                view.open_workspace(id, None, true, window, cx)
                                            }),
                                        ),
                                    )
                                    .child(
                                        IconButton::new(
                                            SharedString::from(format!("edit-{id}")),
                                            Icon::Pencil,
                                        )
                                        .glyph_size(px(15.))
                                        .tooltip(strings::task_edit_tip())
                                        .on_click(
                                            cx.listener(move |view, _, window, cx| {
                                                view.open_task_editor(Some(id), window, cx)
                                            }),
                                        ),
                                    )
                                    .child(
                                        IconButton::new(
                                            SharedString::from(format!("delete-{id}")),
                                            Icon::TrashCanOutline,
                                        )
                                        .glyph_size(px(15.))
                                        .tooltip(strings::task_delete_tip())
                                        .on_click(
                                            cx.listener(move |view, _, _, cx| {
                                                let Some(task) = view.workspace.task(id) else {
                                                    return;
                                                };
                                                let dialog = ConfirmDialog::delete_task(
                                                    &task.name,
                                                    task.destinations.len(),
                                                );
                                                view.open_confirm(
                                                    dialog,
                                                    PendingAction::DeleteTask(id),
                                                    cx,
                                                );
                                            }),
                                        ),
                                    ),
                            ),
                    ),
            )
            .when(expanded, |element| element.children(rows))
    }

    fn render_destination_row(
        &self,
        task_id: Uuid,
        destination: &Destination,
        status: Option<&DestinationSyncStatus>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let outcome = status.map_or(SyncOutcome::Never, |status| status.outcome);
        let (glyph, color) = outcome_appearance(outcome);
        let id = destination.id;

        div()
            .flex()
            .flex_row()
            .items_center()
            // Only a top hairline: the rows read as one block rather than a stack of boxes.
            .border_t_1()
            .border_color(theme::color(theme::HAIRLINE))
            .pl(px(16.))
            .pr(px(12.))
            .py(px(10.))
            .child(
                div()
                    .mr(px(11.))
                    .child(icon(glyph, px(17.), theme::color(color))),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(8.))
                            .child(div().child(destination.name.clone()))
                            // Move destinations are rules in an ordered list; naming the
                            // strategy on each one would be noise.
                            .when(destination.strategy != SyncStrategy::Move, |element| {
                                element.child(strategy_badge(destination.strategy))
                            })
                            .child(filter_badge(destination)),
                    )
                    .child(path_text(destination.local_path())),
            )
            .child(
                div()
                    .max_w(px(280.))
                    .overflow_hidden()
                    .text_ellipsis()
                    .text_color(theme::color(color))
                    // A live progress line takes the row over while the run is going.
                    .child(
                        self.progress
                            .get(&id)
                            .cloned()
                            .unwrap_or_else(|| status_text(status)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap(px(5.))
                    .ml(px(12.))
                    .when(outcome == SyncOutcome::NeedsConfirmation, |element| {
                        element.child(
                            IconButton::new(
                                SharedString::from(format!("review-{id}")),
                                Icon::AlertOutline,
                            )
                            .small()
                            .tone(IconButtonTone::Danger)
                            .tooltip(strings::task_review_deletions_tip())
                            .on_click(cx.listener(
                                move |view, _, _, cx| view.review_deletions(task_id, id, cx),
                            )),
                        )
                    })
                    .child(
                        IconButton::new(
                            SharedString::from(format!("edit-dest-{id}")),
                            Icon::Pencil,
                        )
                        .small()
                        .tooltip(strings::dest_edit_tip())
                        .on_click(cx.listener(
                            move |view, _, window, cx| {
                                view.open_workspace(task_id, Some(id), false, window, cx)
                            },
                        )),
                    )
                    .child(
                        IconButton::new(
                            SharedString::from(format!("delete-dest-{id}")),
                            Icon::TrashCanOutline,
                        )
                        .small()
                        .tooltip(strings::dest_delete_tip())
                        .on_click(cx.listener(move |view, _, _, cx| {
                            let Some(name) = view
                                .workspace
                                .task(task_id)
                                .and_then(|task| {
                                    task.destinations
                                        .iter()
                                        .find(|candidate| candidate.id == id)
                                })
                                .map(|destination| destination.name.clone())
                            else {
                                return;
                            };
                            view.open_confirm(
                                ConfirmDialog::delete_destination(&name),
                                PendingAction::DeleteDestination {
                                    task: task_id,
                                    destination: id,
                                },
                                cx,
                            );
                        })),
                    ),
            )
    }
}

/// Paths are monospaced and muted throughout, so a long one reads as reference rather than
/// competing with the name above it.
fn path_text(path: &str) -> impl IntoElement {
    div()
        .text_size(theme::text::small())
        .text_color(theme::color(theme::TEXT_MUTED))
        .font_family("Consolas")
        .overflow_hidden()
        // Both are needed: without nowrap a long path wraps instead of ellipsizing, and a
        // wrapped path pushes everything below it out of line.
        .whitespace_nowrap()
        .text_ellipsis()
        .child(path.to_owned())
}

/// A Move task's list is ordered, so what is being added there is the next routing rule, not
/// just another destination.
fn add_destination_hint(kind: SyncTaskKind) -> &'static str {
    match kind {
        SyncTaskKind::Move => strings::task_add_routing_rule_tip(),
        SyncTaskKind::Sync => strings::task_add_destination_tip(),
    }
}

fn kind_badge(kind: SyncTaskKind) -> Badge {
    match kind {
        SyncTaskKind::Sync => Badge::new(strings::enum_sync_task_kind_sync()).glyph(Icon::Sync),
        SyncTaskKind::Move => {
            Badge::new(strings::enum_sync_task_kind_move()).glyph(Icon::CallSplit)
        }
    }
}

/// `next run in 2 h`. Relative, because that is the question being asked — with the absolute
/// time on hover, which never goes stale between refreshes.
fn next_run_badge(task_id: Uuid, next: DateTime<Local>) -> Badge {
    Badge::new(strings::task_next_run_format(humanize(next - Local::now())))
        .glyph(Icon::ClockOutline)
        .tone(BadgeTone::Live)
        .tooltip(
            SharedString::from(format!("next-run-{task_id}")),
            next.format("%Y-%m-%d %H:%M").to_string(),
        )
}

/// Rounded down to the coarsest unit that still says something useful. "in 90 minutes" is
/// arithmetic; "in 1 h" is an answer.
fn humanize(span: chrono::TimeDelta) -> String {
    if span <= chrono::TimeDelta::zero() {
        return strings::time_due_now().to_owned();
    }
    if span < chrono::TimeDelta::minutes(1) {
        return strings::time_in_under_a_minute().to_owned();
    }
    if span < chrono::TimeDelta::hours(1) {
        return strings::time_in_minutes_format(span.num_minutes());
    }
    if span < chrono::TimeDelta::days(1) {
        return strings::time_in_hours_format(span.num_hours());
    }
    strings::time_in_days_format(span.num_days())
}

/// The task will not run by itself. Amber rather than red: what is broken is the automation,
/// not the task — Run now still works.
fn trigger_error_badge(task_id: Uuid, reason: &str) -> Badge {
    Badge::new(strings::task_trigger_error_badge())
        .glyph(Icon::AlertOutline)
        .tone(BadgeTone::Warn)
        .tooltip(
            SharedString::from(format!("trigger-error-{task_id}")),
            reason.to_owned(),
        )
}

fn trigger_badge(trigger: &Trigger) -> Badge {
    match trigger {
        Trigger::Manual => {
            Badge::new(strings::task_trigger_manual()).glyph(Icon::CursorDefaultClickOutline)
        }
        Trigger::Scheduled { cron_expression } => {
            Badge::new(strings::task_trigger_scheduled_format(cron_expression))
                .glyph(Icon::ClockOutline)
        }
        Trigger::Watch { .. } => Badge::new(strings::task_trigger_watching()).glyph(Icon::Eye),
    }
}

fn strategy_badge(strategy: SyncStrategy) -> Badge {
    match strategy {
        SyncStrategy::Mirror => Badge::new(strings::enum_sync_strategy_mirror()).glyph(Icon::Sync),
        SyncStrategy::AddOnly => {
            Badge::new(strings::enum_sync_strategy_add_only()).glyph(Icon::Plus)
        }
        SyncStrategy::Move => Badge::new(strings::enum_sync_strategy_move()).glyph(Icon::CallSplit),
    }
}

fn filter_badge(destination: &Destination) -> Badge {
    let label = if destination.has_only_the_all_files_filter() {
        strings::filter_all_files().to_owned()
    } else if destination.filters.is_empty() {
        // An empty filter list selects nothing, and the badge has to say so rather than
        // reading as "no filtering".
        strings::dest_no_rules().to_owned()
    } else {
        strings::dest_filters_count(destination.filters.len() as i64)
    };
    Badge::new(label).glyph(Icon::FilterOutline)
}

fn status_text(status: Option<&DestinationSyncStatus>) -> String {
    let Some(status) = status else {
        return strings::status_never_run().to_owned();
    };
    match status.outcome {
        SyncOutcome::Never => strings::status_never_run().to_owned(),
        SyncOutcome::Running => strings::status_syncing().to_owned(),
        SyncOutcome::NeedsConfirmation => strings::status_needs_confirmation().to_owned(),
        // The engine's own sentence, in English, wrapped in one that is translated: the core
        // carries no display strings, and paraphrasing an OS error loses what it said.
        SyncOutcome::Failed => match &status.error {
            Some(error) => strings::status_failed_format(error),
            None => strings::status_failed().to_owned(),
        },
        SyncOutcome::Incomplete => strings::status_incomplete_format(
            ago(status.last_run),
            strings::common_files_count(i64::from(status.files_copied)),
            status.files_deferred,
        ),
        SyncOutcome::Success => strings::status_synced_format(
            ago(status.last_run),
            strings::common_files_count(i64::from(status.files_copied)),
        ),
    }
}

/// How long ago a run finished, in the coarsest unit that still says something useful.
///
/// A status with no timestamp comes from a run this session that has not been written out yet,
/// which is as recent as it gets.
fn ago(last_run: Option<DateTime<FixedOffset>>) -> String {
    let Some(last_run) = last_run else {
        return strings::time_just_now().to_owned();
    };

    let span = Local::now().signed_duration_since(last_run);
    if span < chrono::TimeDelta::minutes(1) {
        return strings::time_just_now().to_owned();
    }
    if span < chrono::TimeDelta::hours(1) {
        return strings::time_minutes_ago_format(span.num_minutes());
    }
    if span < chrono::TimeDelta::days(1) {
        return strings::time_hours_ago_format(span.num_hours());
    }
    strings::time_days_ago_format(span.num_days())
}

/// One glyph and one colour per outcome, shared by the card summary and the rows beneath it.
fn outcome_appearance(outcome: SyncOutcome) -> (Icon, u32) {
    match outcome {
        SyncOutcome::Never => (Icon::MinusCircle, theme::TEXT_MUTED),
        SyncOutcome::Running => (Icon::Sync, theme::TEAL),
        SyncOutcome::Success => (Icon::CheckCircle, theme::SUCCESS),
        SyncOutcome::Incomplete => (Icon::MinusCircle, theme::WARNING),
        SyncOutcome::Failed => (Icon::AlertCircle, theme::DANGER),
        SyncOutcome::NeedsConfirmation => (Icon::AlertOutline, theme::WARNING),
    }
}

fn config_unreadable_banner() -> impl IntoElement {
    HintBox::new(strings::main_config_unreadable_detail()).tone(HintTone::Warning)
}

fn empty_state() -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .py(px(24.))
        .text_size(theme::text::small())
        .text_color(theme::color(theme::TEXT_SECONDARY))
        .child(strings::main_empty_state())
}

#[cfg(test)]
mod tests {
    use syncmaid_core::filtering::FilterRule;

    use super::*;

    fn destination(strategy: SyncStrategy, filters: Vec<FilterRule>) -> Destination {
        Destination::new("D", r"D:\d", filters, strategy)
    }

    #[test]
    fn every_outcome_has_its_own_glyph_and_colour() {
        let outcomes = [
            SyncOutcome::Never,
            SyncOutcome::Running,
            SyncOutcome::Success,
            SyncOutcome::Incomplete,
            SyncOutcome::Failed,
            SyncOutcome::NeedsConfirmation,
        ];
        let appearances: Vec<_> = outcomes.iter().map(|o| outcome_appearance(*o)).collect();

        assert_eq!(
            (Icon::AlertCircle, theme::DANGER),
            outcome_appearance(SyncOutcome::Failed)
        );
        assert_eq!(outcomes.len(), appearances.len());
    }

    #[test]
    fn a_failed_row_shows_the_engines_own_sentence() {
        let mut status = DestinationSyncStatus::new(uuid::Uuid::nil(), SyncOutcome::Failed);
        status.error = Some("Failed to copy 'a.txt': access denied.".into());

        // Wrapped, not paraphrased: the engine's own words survive intact inside a sentence
        // that is translated.
        assert_eq!(
            "Failed · Failed to copy 'a.txt': access denied.",
            status_text(Some(&status))
        );
    }

    #[test]
    fn an_incomplete_row_reports_both_counts() {
        let mut status = DestinationSyncStatus::new(uuid::Uuid::nil(), SyncOutcome::Incomplete);
        status.files_copied = 126;
        status.files_deferred = 2;

        assert_eq!(
            "Synced just now · 126 files, 2 in use",
            status_text(Some(&status)),
            "a status with no timestamp is one this session has not written out yet"
        );
    }

    #[test]
    fn a_filter_badge_says_all_files_only_for_the_lone_all_files_rule() {
        assert_eq!(
            "All files",
            filter_badge(&destination(
                SyncStrategy::Mirror,
                vec![FilterRule::AllFiles]
            ))
            .label()
        );
        assert_eq!(
            "1 filter",
            filter_badge(&destination(
                SyncStrategy::AddOnly,
                vec![FilterRule::extension("jpg")]
            ))
            .label()
        );
        assert_eq!(
            "2 filters",
            filter_badge(&destination(
                SyncStrategy::AddOnly,
                vec![FilterRule::extension("jpg"), FilterRule::extension("png")]
            ))
            .label()
        );
        assert_eq!(
            "No rules",
            filter_badge(&destination(SyncStrategy::AddOnly, vec![])).label(),
            "an empty filter list selects nothing, and the badge has to say so"
        );
    }
}
