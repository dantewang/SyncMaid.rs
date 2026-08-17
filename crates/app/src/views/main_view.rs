//! The main window: title bar, task sidebar, task list.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use gpui::{
    div, img, prelude::*, px, Context, Entity, FontWeight, SharedString, Subscription, Window,
    WindowControlArea,
};
use syncmaid_core::io::FileSystem;
use syncmaid_core::model::{
    Destination, DestinationSyncStatus, SyncOutcome, SyncStrategy, SyncTask, SyncTaskKind,
};
use syncmaid_core::sync::{SyncEngine, SyncOperation, SyncProgress};
use syncmaid_core::triggers::Trigger;
use uuid::Uuid;

use crate::components::{
    icon, Badge, Button, ButtonTone, HintBox, HintTone, Icon, IconButton, IconButtonTone,
};
use crate::state::{health_of, RunGate, Workspace};
use crate::theme;
use crate::views::dialogs::{
    ConfirmDialog, ConfirmEvent, DestinationEditor, DestinationEditorEvent, SettingsDialog,
    SettingsEvent, TaskEditor, TaskEditorEvent,
};

/// Which modal is open, and what it will do when it says yes.
enum ActiveDialog {
    Confirm(Entity<ConfirmDialog>),
    TaskEditor(Entity<TaskEditor>),
    DestinationEditor(Entity<DestinationEditor>),
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
}

impl MainView {
    pub fn new(workspace: Workspace, file_system: Arc<dyn FileSystem>) -> Self {
        Self {
            engine: Arc::new(SyncEngine::local(Arc::clone(&file_system))),
            workspace,
            file_system,
            gates: HashMap::new(),
            progress: HashMap::new(),
            dialog: None,
            dialog_subscription: None,
        }
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
                    self.open_destination_editor(id, None, window, cx);
                }
            }
            "destination-edit" => {
                if let Some((task, destination)) = self
                    .workspace
                    .tasks()
                    .first()
                    .and_then(|task| Some((task.id, task.destinations.first()?.id)))
                {
                    self.open_destination_editor(task, Some(destination), window, cx);
                }
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
                    view.close_dialog(cx);
                }
                TaskEditorEvent::Cancelled => view.close_dialog(cx),
            },
        ));
        self.dialog = Some(ActiveDialog::TaskEditor(editor));
        cx.notify();
    }

    fn open_destination_editor(
        &mut self,
        task_id: Uuid,
        destination_id: Option<Uuid>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(task) = self.workspace.task(task_id).cloned() else {
            return;
        };
        let tasks = self.workspace.tasks().to_vec();
        let existing = destination_id
            .and_then(|id| {
                task.destinations
                    .iter()
                    .find(|candidate| candidate.id == id)
            })
            .cloned();

        let editor = cx.new(|cx| match &existing {
            Some(destination) => DestinationEditor::edit(&task, destination, tasks, window, cx),
            None => DestinationEditor::new_destination(&task, tasks, window, cx),
        });

        self.dialog_subscription = Some(cx.subscribe(
            &editor,
            move |view, _, event: &DestinationEditorEvent, cx| match event {
                DestinationEditorEvent::Saved(destination) => {
                    view.save_destination(task_id, (**destination).clone());
                    view.close_dialog(cx);
                }
                DestinationEditorEvent::Cancelled => view.close_dialog(cx),
            },
        ));
        self.dialog = Some(ActiveDialog::DestinationEditor(editor));
        cx.notify();
    }

    /// Replaces the destination with the same id, or appends it.
    ///
    /// Appending rather than inserting matters for a Move task: its destinations are an
    /// ordered rule list where the first match wins, so a new rule goes last, where it can
    /// only take what the rules above it left.
    fn save_destination(&mut self, task_id: Uuid, destination: syncmaid_core::model::Destination) {
        let Some(mut task) = self.workspace.task(task_id).cloned() else {
            return;
        };
        match task
            .destinations
            .iter_mut()
            .find(|existing| existing.id == destination.id)
        {
            Some(existing) => *existing = destination,
            None => task.destinations.push(destination),
        }
        self.workspace.upsert_task(task);
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

    /// Starts a run, or folds the request into the one already going.
    fn run_task(&mut self, task_id: Uuid, cx: &mut Context<Self>) {
        let Some(task) = self.workspace.task(task_id).cloned() else {
            return;
        };
        let gate = self.gate_for(task_id);
        let engine = Arc::clone(&self.engine);
        let before = self.workspace.status_snapshot(&task);

        self.workspace.mark_running(&task);
        cx.notify();

        cx.spawn(async move |view, cx| {
            let Some(mut start) = gate.request(HashSet::new()) else {
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
        SyncOperation::Copy { .. } => "Copying",
        SyncOperation::Move { .. } => "Moving",
        SyncOperation::Delete { .. } | SyncOperation::DeleteDirectory { .. } => "Removing",
        SyncOperation::CreateDirectory { .. } => "Creating",
        SyncOperation::SetDirectoryTimestamp { .. } => "Tidying",
    };
    format!(
        "{verb} {} ({}/{})",
        report.operation.relative_path(),
        report.completed_operations + 1,
        report.total_operations
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
                    ActiveDialog::DestinationEditor(entity) => entity.clone().into_any_element(),
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
                    .on_click(cx.listener(|view, _, _, cx| view.open_settings(cx))),
            )
            .child(
                IconButton::new("minimize", Icon::WindowMinimize)
                    .tone(IconButtonTone::Caption)
                    .window_control(WindowControlArea::Min),
            )
            .child(
                IconButton::new("maximize", Icon::WindowMaximize)
                    .tone(IconButtonTone::Caption)
                    .window_control(WindowControlArea::Max),
            )
            .child(
                IconButton::new("close", Icon::WindowClose)
                    .tone(IconButtonTone::CaptionClose)
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
                            .child("Tasks"),
                    )
                    .child(
                        IconButton::new("collapse-sidebar", Icon::ChevronLeft)
                            .tone(IconButtonTone::Caption)
                            .small()
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
                    .child("Sync tasks"),
            )
            .child(
                Button::new(
                    "toggle-expand",
                    if all_expanded {
                        "Collapse all"
                    } else {
                        "Expand all"
                    },
                )
                .tone(ButtonTone::Secondary)
                .on_click(cx.listener(move |view, _, _, cx| {
                    view.workspace.set_all_expanded(!all_expanded);
                    cx.notify();
                })),
            )
            .child(
                Button::new("run-all", "Run all")
                    .tone(ButtonTone::Secondary)
                    .glyph(Icon::Play),
            )
            .child(
                Button::new("new-task", "New task")
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
                                            .child(
                                                div()
                                                    .text_size(theme::text::large())
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .child(task.name.clone()),
                                            )
                                            .child(kind_badge(task.kind()))
                                            .child(trigger_badge(&task.trigger)),
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
                                        .disabled(task.destinations.is_empty())
                                        .on_click(
                                            cx.listener(move |view, _, _, cx| {
                                                view.run_task(id, cx)
                                            }),
                                        )
                                    })
                                    .child(
                                        IconButton::new(
                                            SharedString::from(format!("add-{id}")),
                                            Icon::Plus,
                                        )
                                        .on_click(
                                            cx.listener(move |view, _, window, cx| {
                                                view.open_destination_editor(id, None, window, cx)
                                            }),
                                        ),
                                    )
                                    .child(
                                        IconButton::new(
                                            SharedString::from(format!("edit-{id}")),
                                            Icon::Pencil,
                                        )
                                        .glyph_size(px(15.))
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
                            .tone(IconButtonTone::Danger),
                        )
                    })
                    .child(
                        IconButton::new(
                            SharedString::from(format!("edit-dest-{id}")),
                            Icon::Pencil,
                        )
                        .small()
                        .on_click(cx.listener(
                            move |view, _, window, cx| {
                                view.open_destination_editor(task_id, Some(id), window, cx)
                            },
                        )),
                    )
                    .child(
                        IconButton::new(
                            SharedString::from(format!("delete-dest-{id}")),
                            Icon::TrashCanOutline,
                        )
                        .small()
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

fn kind_badge(kind: SyncTaskKind) -> Badge {
    match kind {
        SyncTaskKind::Sync => Badge::new("Sync").glyph(Icon::Sync),
        SyncTaskKind::Move => Badge::new("Move").glyph(Icon::CallSplit),
    }
}

fn trigger_badge(trigger: &Trigger) -> Badge {
    match trigger {
        Trigger::Manual => Badge::new("Manual").glyph(Icon::CursorDefaultClickOutline),
        Trigger::Scheduled { cron_expression } => {
            Badge::new(format!("Scheduled · {cron_expression}")).glyph(Icon::ClockOutline)
        }
        Trigger::Watch { .. } => Badge::new("Watching").glyph(Icon::Eye),
    }
}

fn strategy_badge(strategy: SyncStrategy) -> Badge {
    match strategy {
        SyncStrategy::Mirror => Badge::new("Mirror").glyph(Icon::Sync),
        SyncStrategy::AddOnly => Badge::new("Add-only").glyph(Icon::Plus),
        SyncStrategy::Move => Badge::new("Move").glyph(Icon::CallSplit),
    }
}

fn filter_badge(destination: &Destination) -> Badge {
    let label = if destination.has_only_the_all_files_filter() {
        "All files".to_owned()
    } else {
        match destination.filters.len() {
            0 => "No rules".to_owned(),
            1 => "1 filter".to_owned(),
            count => format!("{count} filters"),
        }
    };
    Badge::new(label).glyph(Icon::FilterOutline)
}

fn status_text(status: Option<&DestinationSyncStatus>) -> String {
    let Some(status) = status else {
        return "Never run".to_owned();
    };
    match status.outcome {
        SyncOutcome::Never => "Never run".to_owned(),
        SyncOutcome::Running => "Syncing…".to_owned(),
        SyncOutcome::NeedsConfirmation => "Needs confirmation".to_owned(),
        SyncOutcome::Failed => status.error.clone().unwrap_or_else(|| "Failed".to_owned()),
        SyncOutcome::Incomplete => format!(
            "Synced · {} files, {} in use",
            status.files_copied, status.files_deferred
        ),
        SyncOutcome::Success => format!("Synced · {} files", status.files_copied),
    }
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
    HintBox::new(
        "SyncMaid could not read its saved tasks, so it will not save over them. \
         Check Data\\tasks.json and its .bak, then restart.",
    )
    .tone(HintTone::Warning)
}

fn empty_state() -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .py(px(24.))
        .text_size(theme::text::small())
        .text_color(theme::color(theme::TEXT_SECONDARY))
        .child("No tasks yet — add the first one with New task.")
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

        assert_eq!(
            "Failed to copy 'a.txt': access denied.",
            status_text(Some(&status))
        );
    }

    #[test]
    fn an_incomplete_row_reports_both_counts() {
        let mut status = DestinationSyncStatus::new(uuid::Uuid::nil(), SyncOutcome::Incomplete);
        status.files_copied = 126;
        status.files_deferred = 2;

        assert_eq!("Synced · 126 files, 2 in use", status_text(Some(&status)));
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
