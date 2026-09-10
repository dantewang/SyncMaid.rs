//! The main window: a task sidebar, the task list, and the settings page it swaps to.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, FixedOffset, Local};

use gpui::{
    div, point, prelude::*, px, relative, App, Context, Div, Entity, FontWeight, Hsla, Pixels,
    ScrollHandle, SharedString, Stateful, Subscription, Window,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::link::Link;
use gpui_component::sheet::Sheet;
use gpui_component::tooltip::Tooltip;
use gpui_component::{
    alert::Alert, h_flex, tag::Tag, v_flex, ActiveTheme as _, Disableable as _, Icon, Root,
    Selectable as _, Sizable as _, WindowExt as _,
};
use syncmaid_core::io::FileSystem;
use syncmaid_core::model::{
    Destination, DestinationSyncStatus, SyncOutcome, SyncStrategy, SyncTask, SyncTaskKind,
};
use syncmaid_core::sync::{SyncEngine, SyncOperation, SyncProgress};
use syncmaid_core::triggers::{CronSchedule, DefaultTriggerSourceFactory, Notification, Trigger};
use uuid::Uuid;

use crate::components::{Glyph, ScrollNotify, ScrollRail, RAIL_WIDTH};
use crate::services::logging;
use crate::state::{health_of, run_conflict, RunGate, TriggerEvent, TriggerHost, Workspace};
use crate::strings;
use crate::theme;
use crate::views::dialogs::{
    Prompt, TaskEditor, TaskEditorEvent, TaskWorkspace, TaskWorkspaceEvent,
};
use crate::views::log_viewer::{self, LogRequest, OpenLogViewer};
use crate::views::mirror_delete::{self, MirrorDeleteDecision};
use crate::views::{SettingsEvent, SettingsView};

/// How often the "next run in ..." labels are re-made. Often enough that a minutes-away label
/// is never wrong by much, rare enough to cost nothing while the window sits idle.
const NEXT_RUN_REFRESH: Duration = Duration::from_secs(30);

/// The shape both editors arrive in: a sheet from the right, full window height.
///
/// A sheet rather than a centred dialog because both are tall, list-shaped editing surfaces that
/// want the height, and because it keeps the task list visible beside them.
///
/// `overlay_closable` is off on purpose. It defaults on, and an editor holding unsaved changes
/// that a stray click outside throws away is not a trade worth taking: Esc and the ✕ are both
/// still there, and both are deliberate.
///
/// The wrapper around `body` is doing work the sheet does not do for itself. Its body is
/// `flex_1` over an `overflow: scroll` on **both** axes, with no width and no `min-height: 0` —
/// so unclamped content both widens the sheet (carrying a row's buttons off the right edge) and
/// lengthens it (pushing the footer, and therefore Save, off the bottom). A dialog wraps its
/// content in `w_full().overflow_hidden()` under a bounded height; this is the same wrapper,
/// with real numbers so `text_ellipsis` has something definite to trim against.
fn editor_sheet(
    sheet: Sheet,
    window: &mut Window,
    cx: &App,
    preferred: Pixels,
    scroll: &ScrollHandle,
    notify: ScrollNotify,
    body: impl IntoElement,
) -> Sheet {
    let viewport = window.viewport_size();
    // Never wider than the window it slides into — the window's minimum is 640, and a sheet
    // asking for 760 there would run off the left edge.
    let width = px(f32::from(preferred).min(f32::from(viewport.width) - 48.));
    let (above, below) = clipped(scroll);

    sheet
        .size(width)
        .overlay_closable(false)
        // Zero, not the default: that default leaves room for the library's own drawn title
        // bar, and ours is the system's — the client area already starts below it.
        .margin_top(px(0.))
        .child(
            // The form gives up the rail's width rather than sharing it. There is no spare
            // margin in a sheet the way there is in the content pane — the sheet sets its own
            // width — so the choice is between a narrower form and a bar across the fields.
            div()
                .relative()
                .flex()
                .flex_col()
                .w(width - SHEET_PADDING * 2.)
                .child(
                    div()
                        .id("sheet-body")
                        .w_full()
                        .max_h(viewport.height - SHEET_CHROME)
                        .overflow_hidden()
                        .overflow_y_scroll()
                        .track_scroll(scroll)
                        .pr(RAIL_WIDTH)
                        .child(body),
                )
                .child(ScrollRail::vertical("sheet-rail", scroll).on_scroll({
                    let notify = notify.clone();
                    move |window, cx| notify(window, cx)
                }))
                // Stops where the form stops, the same as in the task list. Running it the full
                // width of the body put its right end past the rail's own hairline, which reads
                // as a rule that missed rather than one that measures something.
                .when(above, |element| {
                    element.child(clip_edge(cx).top_0().left_0().right(RAIL_WIDTH))
                })
                .when(below, |element| {
                    element.child(clip_edge(cx).bottom_0().left_0().right(RAIL_WIDTH))
                }),
        )
}

/// The horizontal padding a `Sheet` puts on its body.
const SHEET_PADDING: Pixels = px(16.);

/// The content pane's own horizontal padding, carried by the header and by the scrolling cards
/// rather than by the pane, so the scroll area can run full width underneath both.
const PANE_PADDING: Pixels = px(16.);

/// Whether a scroll area is hiding content above it and below it.
fn clipped(handle: &ScrollHandle) -> (bool, bool) {
    let max = f32::from(handle.max_offset().height);
    if max <= 1. {
        return (false, false);
    }
    let scrolled = -f32::from(handle.offset().y);
    (scrolled > 0.5, scrolled < max - 0.5)
}

/// The rule that appears where a scroll area is cutting through its content.
///
/// Not a frame. A box around the list would be four lines to say what one line says, and three of
/// them would be describing edges where nothing is being cut. This is drawn only on the side that
/// is actually hiding something, and it is ink rather than a fade because nothing else in this
/// interface has a soft edge.
fn clip_edge(cx: &App) -> Div {
    div().absolute().h(px(2.)).bg(cx.theme().foreground)
}

/// What a `Sheet`'s own title row and footer row occupy, measured (122) plus 2px of slack.
///
/// This is what keeps Cancel and Save in the same place whatever a sheet is for and however much
/// it holds. The body grows to fill the sheet, so with short content the footer sits at the
/// bottom on its own; with content taller than this the body would keep growing and carry the
/// footer down with it. Capping the content here means both cases end in the same place.
///
/// **Err high, never low.** Too high costs a couple of pixels of scroll area nobody can see. Too
/// low and tall sheets push their footer below short ones — and eventually off the window, which
/// is how Save went missing the first time. Re-measure by capturing `--show task` and
/// `--show destination-edit` and comparing where the Save button ends.
const SHEET_CHROME: Pixels = px(124.);

/// Which of the two things the window body is showing.
///
/// Deliberately not persisted. Settings is somewhere you go and come back from, not a mode the
/// app should still be in tomorrow morning.
enum Route {
    Tasks,
    Settings(Entity<SettingsView>),
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
    route: Route,
    /// Dropped when the editor sheet closes, which is what unsubscribes it.
    editor_subscription: Option<Subscription>,
    /// So picking a task in the sidebar can bring its card into view.
    task_list: ScrollHandle,
    /// The open sheet.s form area. One handle serves both editors: only one sheet is ever open,
    /// and opening the next one rewinds it.
    sheet_scroll: ScrollHandle,

    /// The live trigger runners. Dropping this stops every one of them.
    triggers: TriggerHost,
    /// Why a task will not run automatically, keyed by task. Shown as an amber badge.
    trigger_errors: HashMap<Uuid, String>,
    /// When each scheduled task fires next, refreshed on a timer so the label stays honest.
    next_runs: HashMap<Uuid, DateTime<Local>>,

    /// The one log window, while it is open. Kept so that the next status click re-points it
    /// rather than opening a second — see `views::log_viewer`.
    log_viewer: Option<OpenLogViewer>,
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
            route: Route::Tasks,
            editor_subscription: None,
            task_list: ScrollHandle::new(),
            sheet_scroll: ScrollHandle::new(),
            triggers,
            trigger_errors: HashMap::new(),
            next_runs: HashMap::new(),
            log_viewer: None,
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

    /// Opens one dialog by name, for `SyncMaid-rs.exe --show <dialog>`.
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
            // The real log, read from the real file, for the first destination there is.
            "log" => {
                if let Some((task, destination)) = self
                    .workspace
                    .tasks()
                    .first()
                    .and_then(|task| Some((task.id, task.destinations.first()?.id)))
                {
                    self.open_log_viewer(task, destination, cx);
                }
            }
            "settings" => self.open_settings(cx),
            "confirm" => {
                if let Some(id) = first_task {
                    let Some(task) = self.workspace.task(id) else {
                        return;
                    };
                    let prompt = Prompt::delete_task(&task.name, task.destinations.len());
                    self.confirm_delete_task(prompt, id, window, cx);
                }
            }
            other => tracing::warn!("no dialog called {other:?}"),
        }
    }

    /// Whether closing the window should hide it instead of quitting.
    pub fn close_to_tray(&self) -> bool {
        self.workspace.settings().close_to_tray
    }

    /// Shows this destination's log lines, in the window already open if there is one.
    ///
    /// One window, whichever destination is clicked: `OpenLogViewer::show` re-points it and
    /// reports whether it is still there, so a window the user closed is replaced rather than
    /// leaving the click doing nothing.
    fn open_log_viewer(&mut self, task_id: Uuid, destination_id: Uuid, cx: &mut Context<Self>) {
        let Some(task) = self.workspace.task(task_id) else {
            return;
        };
        let Some(destination) = task
            .destinations
            .iter()
            .find(|candidate| candidate.id == destination_id)
        else {
            return;
        };

        let request = LogRequest {
            task_id,
            task_name: task.name.clone(),
            destination_name: destination.name.clone(),
            log_path: self.workspace.log_path(),
        };

        if let Some(open) = &self.log_viewer {
            if open.show(request.clone(), cx) {
                return;
            }
        }
        self.log_viewer = log_viewer::open(request, cx);
    }

    fn gate_for(&mut self, task_id: Uuid) -> Arc<RunGate> {
        Arc::clone(
            self.gates
                .entry(task_id)
                .or_insert_with(|| Arc::new(RunGate::new())),
        )
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

        self.editor_subscription = Some(cx.subscribe_in(
            &editor,
            window,
            |view, _, event: &TaskEditorEvent, window, cx| {
                if let TaskEditorEvent::Saved(task) = event {
                    view.workspace.upsert_task((**task).clone());
                    // The trigger or the source may have changed under it.
                    view.sync_triggers(cx);
                }
                view.dismiss_editor(window, cx);
            },
        ));

        // The builder runs once a frame, so it may only clone and read — never act.
        let editor = editor.clone();
        let scroll = self.sheet_scroll.clone();
        let notify = self.sheet_notifier(cx);
        window.open_sheet(cx, move |sheet, window, cx| {
            editor_sheet(
                sheet,
                window,
                cx,
                px(470.),
                &scroll,
                notify.clone(),
                editor.clone(),
            )
            .title(strings::task_editor_title())
            .footer(TaskEditor::footer(&editor, cx))
            // Esc, the ✕ and — were it enabled — the overlay all arrive here. Save closes
            // the sheet itself and never reaches this, so this is only ever "gave up".
            .on_close({
                let editor = editor.clone();
                move |_, _, cx| {
                    editor.update(cx, |_, cx| cx.emit(TaskEditorEvent::Cancelled));
                }
            })
        });
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
        let routing = task.kind() == SyncTaskKind::Move;
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

        self.editor_subscription = Some(cx.subscribe_in(
            &editor,
            window,
            move |view, _, event: &TaskWorkspaceEvent, window, cx| {
                if let TaskWorkspaceEvent::Saved(destinations) = event {
                    view.save_destinations(task_id, destinations.clone());
                }
                view.dismiss_editor(window, cx);
            },
        ));

        let editor = editor.clone();
        let scroll = self.sheet_scroll.clone();
        let notify = self.sheet_notifier(cx);
        window.open_sheet(cx, move |sheet, window, cx| {
            editor_sheet(
                sheet,
                window,
                cx,
                px(760.),
                &scroll,
                notify.clone(),
                editor.clone(),
            )
            .title(TaskWorkspace::heading(routing))
            .footer(TaskWorkspace::footer(&editor))
            .on_close({
                let editor = editor.clone();
                move |_, _, cx| {
                    editor.update(cx, |_, cx| cx.emit(TaskWorkspaceEvent::Cancelled));
                }
            })
        });
        cx.notify();
    }

    /// Closes whichever editor sheet is open and forgets its subscription.
    fn dismiss_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editor_subscription = None;
        window.close_sheet(cx);
        cx.notify();
    }

    /// Replaces the task's destination list wholesale.
    ///
    /// Wholesale rather than one at a time, because for a Move task the order **is** the
    /// matching order: a merge that preserved the old positions would silently undo a reorder
    /// the user just made.
    fn save_destinations(&mut self, task_id: Uuid, destinations: Vec<Destination>) {
        let Some(mut task) = self.workspace.task(task_id).cloned() else {
            return;
        };
        task.destinations = destinations;
        self.workspace.upsert_task(task);
        // Statuses of destinations that are gone go with them.
        self.workspace.persist_statuses();
    }

    /// Swaps the window body over to settings.
    fn open_settings(&mut self, cx: &mut Context<Self>) {
        let settings = self.workspace.settings().clone();
        let directory = self.workspace.data_directory().to_path_buf();
        let view = cx.new(|_| SettingsView::new(settings, directory));

        self.editor_subscription = Some(cx.subscribe(
            &view,
            |view, _, event: &SettingsEvent, cx| match event {
                // Applied the moment the switch is flipped; there is no save step.
                SettingsEvent::Changed(settings) => {
                    let settings = settings.clone();
                    view.workspace
                        .update_settings(|current| *current = settings);
                    cx.notify();
                }
            },
        ));
        self.route = Route::Settings(view);
        cx.notify();
    }

    fn confirm_delete_task(
        &mut self,
        prompt: Prompt,
        task_id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = cx.entity();
        prompt.open(window, cx, move |_, cx| {
            view.update(cx, |view, cx| {
                // Stop it first: a task that is mid-run must not keep writing after it is gone.
                if let Some(gate) = view.gates.remove(&task_id) {
                    gate.refuse_further_requests();
                }
                view.triggers.stop(task_id);
                view.trigger_errors.remove(&task_id);
                view.next_runs.remove(&task_id);
                view.workspace.remove_task(task_id);
                cx.notify();
            });
        });
    }

    fn confirm_delete_destination(
        &mut self,
        prompt: Prompt,
        task_id: Uuid,
        destination_id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = cx.entity();
        prompt.open(window, cx, move |_, cx| {
            view.update(cx, |view, cx| {
                if let Some(mut owner) = view.workspace.task(task_id).cloned() {
                    owner
                        .destinations
                        .retain(|existing| existing.id != destination_id);
                    view.workspace.upsert_task(owner);
                    view.workspace.persist_statuses();
                }
                cx.notify();
            });
        });
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

        // Task shape across the list: no two tasks share a source, and no two share a
        // destination. The editors block a save that would break that, but the rule is a fact
        // about the *list* — the engine is handed one task and cannot see it — so hand-edited
        // config reaches here instead. Refuse before the source is so much as walked.
        if let Some(conflict) = run_conflict(self.workspace.tasks(), &task) {
            self.refuse_run(
                &task,
                strings::task_overlap_refused_format(&conflict.task_name),
                cx,
            );
            return;
        }

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

    /// Reports a refusal the way the engine reports its own: every destination of the task
    /// fails with the same sentence, so the card explains itself wherever the user looks.
    ///
    /// Nothing is read and nothing is written — the refusal is the point. It is logged, and it
    /// lands in `status.json` like any other outcome, so the next run replaces it once the
    /// layout is fixed.
    fn refuse_run(&mut self, task: &SyncTask, reason: String, cx: &mut Context<Self>) {
        let statuses: Vec<DestinationSyncStatus> = task
            .destinations
            .iter()
            .map(|destination| {
                let mut status = DestinationSyncStatus::new(destination.id, SyncOutcome::Failed);
                status.last_run = Some(Local::now().into());
                status.error = Some(reason.clone());
                status
            })
            .collect();

        for status in &statuses {
            log_destination(task, status);
        }
        self.workspace.apply_statuses(statuses);
        cx.notify();
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

    // Through `DestinationTag` rather than spelled out again: this opening is also the needle
    // the log window searches on, and the two drifting apart is a window that silently finds
    // nothing. The task's id rides along because the two names do not identify a destination.
    let tag = logging::DestinationTag::new(task.id, &task.name, name);
    let prefix = tag.line_prefix();

    match status.outcome {
        SyncOutcome::Failed | SyncOutcome::NeedsConfirmation => tracing::warn!(
            "{}: {:?} · {}",
            prefix,
            status.outcome,
            status.error.as_deref().unwrap_or("")
        ),
        _ => tracing::info!(
            "{}: {:?} · {} copied, {} in use",
            prefix,
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
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.render_body(cx))
            // `Root` owns the sheet and dialog stacks but draws neither — the application's own
            // root view has to, which is what puts them above everything here rather than behind
            // it. Leave these out and `open_sheet` / `open_dialog` succeed and show nothing.
            .children(Root::render_sheet_layer(window, cx))
            .children(Root::render_dialog_layer(window, cx))
            .children(Root::render_notification_layer(window, cx))
    }
}

impl MainView {
    /// The sidebar, and whatever it is pointing at.
    ///
    /// Settings replaces the content area rather than the whole body. The gear that opened it is
    /// in the sidebar; taking the sidebar away would hide the control that closes it again.
    fn render_body(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .flex_1()
            .min_w_0()
            .items_start()
            .overflow_hidden()
            .child(self.render_sidebar(cx))
            .child(match &self.route {
                Route::Tasks => self.render_main_pane(cx).into_any_element(),
                Route::Settings(view) => div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(view.clone())
                    .into_any_element(),
            })
    }

    /// The tasks, and the way into settings.
    ///
    /// Hand-built rather than the component library's `Sidebar`, and it no longer folds. The
    /// rail was a section row you had to know to click, and it bought back 160px in a window
    /// that is 940 wide — a control whose only job is to make the rest slightly bigger.
    ///
    /// The wordmark sits at the foot with settings beside it, because the top of a sidebar is
    /// where the list wants to start, and the product's name is the one thing on screen that
    /// never changes.
    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.workspace.selected();
        let in_settings = matches!(self.route, Route::Settings(_));

        let tasks: Vec<_> = self
            .workspace
            .tasks()
            .iter()
            .map(|task| {
                let id = task.id;
                let health = health_of(task, self.workspace.statuses());
                // The source path used to sit under the name; the row has no room for it. A
                // health mark says more per pixel, and the path is on the card.
                task_row(
                    id,
                    &task.name,
                    health.outcome,
                    !in_settings && selected == Some(id),
                    cx,
                )
                .on_click(cx.listener(move |view, _, _, cx| view.select_task(id, cx)))
            })
            .collect();

        v_flex()
            .w(theme::layout::sidebar_width())
            .h_full()
            .flex_none()
            .bg(cx.theme().sidebar)
            .border_r_1()
            .border_color(cx.theme().sidebar_border)
            .child(
                div()
                    .pt(px(13.))
                    .pb(px(6.))
                    .pl(px(14.))
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_size(px(9.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(cx.theme().muted_foreground)
                    .child(strings::main_tasks_heading()),
            )
            .children(tasks)
            .child(div().flex_1())
            .child(div().h(px(1.)).bg(cx.theme().sidebar_border))
            .child(
                h_flex()
                    .h(px(42.))
                    .items_center()
                    .pr(px(9.))
                    .child(wordmark(cx))
                    .child(div().flex_1())
                    .child(
                        // Filled while the page is open, not merely "selected": the ghost
                        // variant marks that state with a tint the sidebar ground already has.
                        Button::new("sidebar-settings")
                            .icon(Glyph::Settings)
                            .when(in_settings, |button| button.primary())
                            .when(!in_settings, |button| button.ghost())
                            .small()
                            .selected(in_settings)
                            .tooltip(strings::settings_title())
                            .on_click(cx.listener(|view, _, _, cx| view.toggle_settings(cx))),
                    ),
            )
    }

    /// Settings is somewhere you go and come back from, so the gear is a toggle rather than a
    /// one-way door. That is also what removed the back bar the page used to carry.
    fn toggle_settings(&mut self, cx: &mut Context<Self>) {
        match self.route {
            Route::Settings(_) => {
                self.route = Route::Tasks;
                cx.notify();
            }
            Route::Tasks => self.open_settings(cx),
        }
    }

    /// Brings the chosen task's card into view.
    ///
    /// Selecting used to only highlight, which made the sidebar decoration. Scrolling keeps
    /// every card reachable — filtering the list to one would not.
    fn select_task(&mut self, task_id: Uuid, cx: &mut Context<Self>) {
        self.workspace.select(task_id);
        if let Some(index) = self
            .workspace
            .tasks()
            .iter()
            .position(|task| task.id == task_id)
        {
            self.task_list.scroll_to_item(index);
        }
        cx.notify();
    }

    /// Repaints this view, which is what redraws the open sheet: `Root::render_sheet_layer` runs
    /// inside this view's own render. Handed to the sheet's rail, whose drag writes straight to a
    /// `ScrollHandle` that nothing else is watching.
    ///
    /// Rewinds the shared handle at the same time, so the next sheet opens at its top rather than
    /// wherever the last one was left.
    fn sheet_notifier(&self, cx: &mut Context<Self>) -> ScrollNotify {
        self.sheet_scroll.set_offset(point(px(0.), px(0.)));

        let view = cx.entity().downgrade();
        Rc::new(move |_window, cx: &mut App| {
            view.update(cx, |_, cx| cx.notify()).ok();
        })
    }

    fn render_main_pane(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let cards: Vec<_> = self
            .workspace
            .tasks()
            .iter()
            .map(|task| self.render_task_card(task, cx))
            .collect();
        let all_expanded = self.workspace.all_expanded();

        let (above, below) = clipped(&self.task_list);
        let view = cx.entity().downgrade();

        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_hidden()
            .bg(cx.theme().background)
            .py(px(12.))
            .child(
                div()
                    .mx(PANE_PADDING)
                    .child(self.render_header(all_expanded, cx)),
            )
            .when(self.workspace.config_unreadable(), |element| {
                element.child(
                    div()
                        .mx(PANE_PADDING)
                        .pb(px(12.))
                        .child(config_unreadable_banner()),
                )
            })
            .when(cards.is_empty(), |element| {
                element.child(div().mx(PANE_PADDING).child(empty_state(cx)))
            })
            .child(
                // The scroll area runs the full width of the pane and the cards carry the pane's
                // padding themselves. That is what puts the rail in margin that already existed
                // instead of on top of a card, without taking a pixel off the cards or breaking
                // their alignment with the header above.
                //
                // The wrapper is `relative` because the rail and the clipped-edge rules both lay
                // themselves out absolutely — as children of the scroll area they would scroll
                // away with the content they are describing.
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .child(
                        v_flex()
                            .id("task-list")
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.task_list)
                            .px(PANE_PADDING)
                            .children(cards),
                    )
                    .child(
                        ScrollRail::vertical("task-rail", &self.task_list).on_scroll({
                            let view = view.clone();
                            move |_, cx| {
                                view.update(cx, |_, cx| cx.notify()).ok();
                            }
                        }),
                    )
                    // Inset to the cards rather than run wall to wall: in a column of card edges
                    // one more edge that lines up with them reads as the list's own boundary,
                    // where a longer rule would read as a lid laid over the card it cuts.
                    .when(above, |element| {
                        element.child(clip_edge(cx).top_0().left(PANE_PADDING).right(PANE_PADDING))
                    })
                    .when(below, |element| {
                        element.child(
                            clip_edge(cx)
                                .bottom_0()
                                .left(PANE_PADDING)
                                .right(PANE_PADDING),
                        )
                    }),
            )
    }

    /// Three counts, beside the heading: tasks, destinations, and how many are running.
    ///
    /// They answer at a glance what the list would otherwise make you count, and the third is
    /// the one that earns the other two — "is anything happening right now" is a question the
    /// cards can only answer by being read.
    fn render_tallies(&self, cx: &App) -> impl IntoElement {
        let tasks = self.workspace.tasks();
        let statuses = self.workspace.statuses();
        let destinations: usize = tasks.iter().map(|task| task.destinations.len()).sum();
        let running = tasks
            .iter()
            .filter(|task| {
                task.destinations.iter().any(|destination| {
                    statuses
                        .get(&destination.id)
                        .is_some_and(|status| status.outcome == SyncOutcome::Running)
                })
            })
            .count();

        h_flex()
            .ml(px(12.))
            .child(tally(strings::main_tally_tasks(), tasks.len(), cx))
            .child(tally(strings::main_tally_destinations(), destinations, cx))
            .child(tally(strings::main_tally_running(), running, cx))
    }

    fn render_header(&self, all_expanded: bool, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .items_center()
            .gap(px(8.))
            .pb(px(12.))
            .child(
                div()
                    .text_size(theme::text::heading())
                    .font_weight(FontWeight::MEDIUM)
                    .child(strings::main_sync_tasks_heading()),
            )
            .child(self.render_tallies(cx))
            .child(div().flex_1())
            .child(
                Button::new("toggle-expand")
                    .label(if all_expanded {
                        strings::main_collapse_all()
                    } else {
                        strings::main_expand_all()
                    })
                    .outline()
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.workspace.set_all_expanded(!all_expanded);
                        cx.notify();
                    })),
            )
            .child(
                Button::new("run-all")
                    .label(strings::main_run_all())
                    .icon(Glyph::Run)
                    .outline()
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
                Button::new("new-task")
                    .label(strings::main_new_task())
                    .icon(Glyph::Add)
                    .primary()
                    .on_click(
                        cx.listener(|view, _, window, cx| view.open_task_editor(None, window, cx)),
                    ),
            )
    }

    fn render_task_card(&self, task: &SyncTask, cx: &mut Context<Self>) -> impl IntoElement {
        let id = task.id;
        let expanded = self.workspace.is_expanded(id);
        let health = health_of(task, self.workspace.statuses());
        let (_, health_color) = outcome_appearance(health.outcome, cx);
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

        v_flex()
            .mb(px(12.))
            .bg(cx.theme().background)
            .border_1()
            // The border carries the card's state. A list of cards is scanned down its left
            // edge, and a coloured outline is legible there in a way a badge in the middle of
            // the row is not.
            .border_color(match health.outcome {
                SyncOutcome::Running => cx.theme().primary,
                SyncOutcome::Failed => cx.theme().danger,
                _ => cx.theme().border,
            })
            .rounded(cx.theme().radius_lg)
            .child(
                h_flex()
                    .items_center()
                    .m(px(10.))
                    .child(
                        h_flex()
                            .items_center()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .child(
                                Button::new(SharedString::from(format!("expand-{id}")))
                                    .icon(if expanded {
                                        Glyph::ChevronDown
                                    } else {
                                        Glyph::ChevronRight
                                    })
                                    .ghost()
                                    .small()
                                    .on_click(cx.listener(move |view, _, _, cx| {
                                        view.workspace.toggle_expanded(id);
                                        cx.notify();
                                    })),
                            )
                            // No folder chip. It was a tinted block saying "this is a task" on
                            // a card that is already nothing but tasks, and it was the one
                            // rounded thing left in the header.
                            .child(div().w(px(4.)))
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .child(
                                        h_flex()
                                            .items_center()
                                            .gap(px(8.))
                                            // Wraps rather than truncates. Every badge says
                                            // something the name does not, and a card that
                                            // grows a line is cheaper than a fact that
                                            // silently disappears off the right edge.
                                            .flex_wrap()
                                            .child(
                                                div()
                                                    .text_base()
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .child(task.name.clone()),
                                            )
                                            .child(kind_badge(task.kind(), cx))
                                            .child(trigger_badge(&task.trigger, cx))
                                            .children(
                                                self.next_runs
                                                    .get(&id)
                                                    .map(|next| next_run_badge(*next, cx)),
                                            )
                                            .children(
                                                self.trigger_errors
                                                    .get(&id)
                                                    .map(|reason| trigger_error_badge(reason, cx)),
                                            ),
                                    )
                                    .child(path_text(&task.source_path, cx)),
                            ),
                    )
                    .child(
                        h_flex()
                            .items_center()
                            .gap(px(14.))
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap(px(7.))
                                    .text_color(health_color)
                                    .child(health_mark(health.outcome, px(7.), cx))
                                    .child(health.text),
                            )
                            .child(
                                h_flex()
                                    .gap(px(5.))
                                    .child(if running {
                                        Button::new(SharedString::from(format!("stop-{id}")))
                                            .icon(Glyph::Stop)
                                            .danger()
                                            .small()
                                            .tooltip(strings::task_stop_tip())
                                            .on_click(cx.listener(move |view, _, _, cx| {
                                                view.stop_task(id, cx)
                                            }))
                                    } else {
                                        Button::new(SharedString::from(format!("run-{id}")))
                                            .icon(Glyph::Run)
                                            .primary()
                                            .small()
                                            .tooltip(strings::task_run_now_tip())
                                            .disabled(task.destinations.is_empty())
                                            .on_click(cx.listener(move |view, _, _, cx| {
                                                view.run_task(id, HashSet::new(), cx)
                                            }))
                                    })
                                    .child(
                                        Button::new(SharedString::from(format!("add-{id}")))
                                            .icon(Glyph::Add)
                                            .outline()
                                            .small()
                                            .tooltip(add_destination_hint(task.kind()))
                                            .on_click(cx.listener(move |view, _, window, cx| {
                                                view.open_workspace(id, None, true, window, cx)
                                            })),
                                    )
                                    .child(
                                        Button::new(SharedString::from(format!("edit-{id}")))
                                            .icon(Glyph::Edit)
                                            .outline()
                                            .small()
                                            .tooltip(strings::task_edit_tip())
                                            .on_click(cx.listener(move |view, _, window, cx| {
                                                view.open_task_editor(Some(id), window, cx)
                                            })),
                                    )
                                    .child(
                                        Button::new(SharedString::from(format!("delete-{id}")))
                                            .icon(Glyph::Trash)
                                            .outline()
                                            .small()
                                            .tooltip(strings::task_delete_tip())
                                            .on_click(cx.listener(move |view, _, window, cx| {
                                                let Some(task) = view.workspace.task(id) else {
                                                    return;
                                                };
                                                let prompt = Prompt::delete_task(
                                                    &task.name,
                                                    task.destinations.len(),
                                                );
                                                view.confirm_delete_task(prompt, id, window, cx);
                                            })),
                                    ),
                            ),
                    ),
            )
            .when(running, |element| element.child(activity_bar(cx)))
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
        let (_, color) = outcome_appearance(outcome, cx);
        let id = destination.id;

        h_flex()
            .items_center()
            // Only a top hairline: the rows read as one block rather than a stack of boxes.
            .border_t_1()
            .border_color(cx.theme().border)
            .pl(px(16.))
            .pr(px(12.))
            .py(px(10.))
            .child(div().mr(px(11.)).child(health_mark(outcome, px(7.), cx)))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .child(
                        h_flex()
                            .items_center()
                            .gap(px(8.))
                            .child(div().child(destination.name.clone()))
                            // Move destinations are rules in an ordered list; naming the
                            // strategy on each one would be noise.
                            .when(destination.strategy != SyncStrategy::Move, |element| {
                                element.child(strategy_badge(destination.strategy, cx))
                            })
                            .child(filter_badge(destination, cx)),
                    )
                    .child(path_text(destination.local_path(), cx)),
            )
            .child(
                // A link, because the sentence here is a summary and the log is the rest of it.
                //
                // At rest it keeps the outcome's colour — the row's one signal of health must
                // not read as blue-for-clickable — and `Link` supplies the underline. Hover is
                // the library's link colour, which is the affordance rather than the signal.
                div()
                    .id(SharedString::from(format!("status-{id}")))
                    .tooltip(|window, cx| Tooltip::new(strings::dest_log_tip()).build(window, cx))
                    .child(
                        Link::new(SharedString::from(format!("status-link-{id}")))
                            .max_w(px(280.))
                            .overflow_hidden()
                            .text_ellipsis()
                            .text_color(color)
                            .text_decoration_color(color)
                            // Re-stated, not redundant: `text_decoration_color` builds a fresh
                            // underline in this refinement, and refining it over `Link`'s
                            // replaces the thickness `Link` set with the 0px default. Setting
                            // only the colour is how the underline goes missing entirely.
                            .text_decoration_1()
                            .child(
                                // A live progress line takes the row over while the run is going.
                                self.progress
                                    .get(&id)
                                    .cloned()
                                    .unwrap_or_else(|| status_text(status)),
                            )
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.open_log_viewer(task_id, id, cx)
                            })),
                    ),
            )
            .child(
                h_flex()
                    .gap(px(5.))
                    .ml(px(12.))
                    .when(outcome == SyncOutcome::NeedsConfirmation, |element| {
                        element.child(
                            Button::new(SharedString::from(format!("review-{id}")))
                                .icon(Glyph::Warning)
                                .danger()
                                .outline()
                                .xsmall()
                                .tooltip(strings::task_review_deletions_tip())
                                .on_click(cx.listener(move |view, _, _, cx| {
                                    view.review_deletions(task_id, id, cx)
                                })),
                        )
                    })
                    .child(
                        Button::new(SharedString::from(format!("edit-dest-{id}")))
                            .icon(Glyph::Edit)
                            .ghost()
                            .xsmall()
                            .tooltip(strings::dest_edit_tip())
                            .on_click(cx.listener(move |view, _, window, cx| {
                                view.open_workspace(task_id, Some(id), false, window, cx)
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("delete-dest-{id}")))
                            .icon(Glyph::Trash)
                            .ghost()
                            .xsmall()
                            .tooltip(strings::dest_delete_tip())
                            .on_click(cx.listener(move |view, _, window, cx| {
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
                                view.confirm_delete_destination(
                                    Prompt::delete_destination(&name),
                                    task_id,
                                    id,
                                    window,
                                    cx,
                                );
                            })),
                    ),
            )
    }
}

/// `SYNC` in a filled block, `MAID` beside it.
///
/// No icon. The title bar and the taskbar already carry the picture, and a third copy of it
/// 200px away was saying the same thing again.
fn wordmark(cx: &App) -> impl IntoElement {
    h_flex()
        .pl(px(11.))
        .items_center()
        .child(
            div()
                .px(px(6.))
                .py(px(4.))
                .bg(cx.theme().foreground)
                .text_color(cx.theme().background)
                .text_size(px(13.))
                .font_weight(FontWeight::BOLD)
                .child("SYNC"),
        )
        .child(
            div()
                .pl(px(6.))
                .py(px(4.))
                .text_size(px(13.))
                .font_weight(FontWeight::BOLD)
                .child("MAID"),
        )
}

/// One count and what it counts, divided from its neighbours by a hairline.
///
/// The number is monospaced so three of them line up whatever the digits are, and the caption
/// under it is small enough that the figure is what you read first.
fn tally(caption: &'static str, value: usize, cx: &App) -> impl IntoElement {
    v_flex()
        .px(px(12.))
        .border_l_1()
        .border_color(cx.theme().border)
        .child(
            div()
                .font_family(cx.theme().mono_font_family.clone())
                .text_size(px(17.))
                .font_weight(FontWeight::BOLD)
                .child(value.to_string()),
        )
        .child(
            div()
                .font_family(cx.theme().mono_font_family.clone())
                .text_size(px(9.))
                .text_color(cx.theme().muted_foreground)
                .child(caption),
        )
}

/// The indeterminate strip under a running card's header.
///
/// A single moving segment rather than a filling bar: the engine knows how many operations a
/// run has, but a run that is still walking the source does not, and a bar that jumps from 0 to
/// 90 lies about both.
fn activity_bar(cx: &App) -> impl IntoElement {
    div()
        .h(px(2.))
        .w_full()
        .bg(cx.theme().muted)
        .child(div().h_full().w(relative(0.3)).bg(cx.theme().primary))
}

/// One task in the sidebar.
///
/// Hand-built rather than a `SidebarMenuItem`, for the same reason `section_row` is: the menu
/// item marks the active row with a filled block, and what says "this one" here is a solid bar
/// down its left edge. The bar is 3px and always present — transparent when the row is not the
/// one selected — so nothing shifts sideways as the selection moves.
fn task_row(id: Uuid, name: &str, outcome: SyncOutcome, active: bool, cx: &App) -> Stateful<Div> {
    h_flex()
        .id(SharedString::from(format!("sidebar-task-{id}")))
        .h(px(37.))
        .items_center()
        .gap(px(10.))
        .pl(px(11.))
        .pr(px(12.))
        .border_l_3()
        .border_color(if active {
            cx.theme().foreground
        } else {
            gpui::transparent_black()
        })
        .when(active, |row| row.bg(cx.theme().background))
        .cursor_pointer()
        .hover(|style| style.bg(cx.theme().sidebar_accent.opacity(0.8)))
        .child(Icon::new(Glyph::Folder).size(px(16.)))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_sm()
                .when(active, |label| label.font_weight(FontWeight::SEMIBOLD))
                .child(name.to_owned()),
        )
        .child(health_mark(outcome, px(7.), cx))
}

/// Paths are monospaced and muted throughout, so a long one reads as reference rather than
/// competing with the name above it.
fn path_text(path: &str, cx: &App) -> impl IntoElement {
    div()
        .text_sm()
        .text_color(cx.theme().muted_foreground)
        .font_family(cx.theme().mono_font_family.clone())
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

/// The contents of a badge: a square pip and a label.
///
/// A pip rather than the glyph that used to be here. Ten badges each carrying a different
/// pictogram is ten little pictures to decode on a row whose words already say what they mean —
/// and the glyphs were never the thing being read. What the square is for is rhythm: every
/// badge starts at the same place, so the row scans as a list rather than as a jumble.
///
/// The colour has to be passed in because a `div` cannot inherit the tag's foreground the way
/// an `Icon` did.
fn badge_body(pip: Hsla, label: impl Into<SharedString>) -> impl IntoElement {
    h_flex()
        .gap(px(6.))
        .items_center()
        .child(div().size(px(6.)).bg(pip).flex_none())
        .child(label.into())
}

/// A fact about the task: its trigger, a destination's strategy, what it filters. A hairline
/// box, so the row reads as a set of labels rather than a row of buttons.
fn badge(label: impl Into<SharedString>, cx: &App) -> Tag {
    Tag::secondary()
        .outline()
        .child(badge_body(cx.theme().secondary_foreground, label))
}

/// What the thing *is*. Filled, so it reads before the facts beside it.
fn kind_badge(kind: SyncTaskKind, cx: &App) -> Tag {
    let label = match kind {
        SyncTaskKind::Sync => strings::enum_sync_task_kind_sync(),
        SyncTaskKind::Move => strings::enum_sync_task_kind_move(),
    };
    Tag::primary().child(badge_body(cx.theme().primary_foreground, label))
}

/// `next run in 2 h`. Relative, because that is the question being asked.
///
/// Outlined rather than filled: it is reference detail, and a filled one drew the eye harder
/// than the task's own name.
fn next_run_badge(next: DateTime<Local>, cx: &App) -> Tag {
    badge(
        strings::task_next_run_format(humanize(next - Local::now())),
        cx,
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
/// not the task — Run now still works. The reason itself rides along as the tooltip.
/// The task will not run by itself. Amber rather than red: what is broken is the automation,
/// not the task — Run now still works. The reason itself rides along as the tooltip, because a
/// badge wide enough to hold an OS error message is not a badge.
fn trigger_error_badge(reason: &str, cx: &App) -> impl IntoElement {
    let pip = cx.theme().warning_foreground;
    let reason = reason.to_owned();
    div()
        .id("trigger-error")
        .tooltip(move |window, cx| Tooltip::new(reason.clone()).build(window, cx))
        .child(
            // Filled, and the only badge on the row that is: everything else there is a fact,
            // and this one is a thing the user has to do something about.
            Tag::warning().child(badge_body(pip, strings::task_trigger_error_badge())),
        )
}

fn trigger_badge(trigger: &Trigger, cx: &App) -> Tag {
    match trigger {
        Trigger::Manual => badge(strings::task_trigger_manual(), cx),
        Trigger::Scheduled { cron_expression } => {
            badge(strings::task_trigger_scheduled_format(cron_expression), cx)
        }
        Trigger::Watch { .. } => badge(strings::task_trigger_watching(), cx),
    }
}

fn strategy_badge(strategy: SyncStrategy, cx: &App) -> Tag {
    let label = match strategy {
        SyncStrategy::Mirror => strings::enum_sync_strategy_mirror(),
        SyncStrategy::AddOnly => strings::enum_sync_strategy_add_only(),
        SyncStrategy::Move => strings::enum_sync_strategy_move(),
    };
    badge(label, cx)
}

fn filter_badge(destination: &Destination, cx: &App) -> Tag {
    badge(filter_summary(destination), cx)
}

/// What the filter badge says. Split out from the badge itself so it can be asserted on without
/// a window: the wording is the part that carries meaning.
fn filter_summary(destination: &Destination) -> String {
    if destination.has_only_the_all_files_filter() {
        strings::filter_all_files().to_owned()
    } else if destination.filters.is_empty() {
        // An empty filter list selects nothing, and the badge has to say so rather than
        // reading as "no filtering".
        strings::dest_no_rules().to_owned()
    } else {
        strings::dest_filters_count(destination.filters.len() as i64)
    }
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

/// One glyph and one colour per outcome, shared by the card summary, the rows beneath it and the
/// sidebar's health dots.
/// A destination's health, as a square.
///
/// A square rather than a glyph, and the same square in all three places it appears — the card's
/// summary, the rows under it, and the sidebar. Six outcomes drawn as six different pictograms
/// asked the reader to learn an alphabet; a filled square asks them to notice a colour, which is
/// the only thing that was ever being said. `size` is the one thing that differs, because the
/// sidebar's is beside a name and the row's is beside a path.
fn health_mark(outcome: SyncOutcome, size: Pixels, cx: &App) -> impl IntoElement {
    let (_, color) = outcome_appearance(outcome, cx);
    div().size(size).bg(color).flex_none()
}

/// The colour an outcome takes, and the glyph it used to take.
///
/// The glyph is still here for the one place a mark is not enough: `Incomplete` and
/// `NeedsConfirmation` are both amber, and the mass-delete review needs to say which.
fn outcome_appearance(outcome: SyncOutcome, cx: &App) -> (Glyph, Hsla) {
    match outcome {
        SyncOutcome::Never => (Glyph::Idle, cx.theme().muted_foreground),
        SyncOutcome::Running => (Glyph::Sync, cx.theme().primary),
        SyncOutcome::Success => (Glyph::Success, cx.theme().success),
        SyncOutcome::Incomplete => (Glyph::Idle, cx.theme().warning),
        SyncOutcome::Failed => (Glyph::Failure, cx.theme().danger),
        SyncOutcome::NeedsConfirmation => (Glyph::Warning, cx.theme().warning),
    }
}

fn config_unreadable_banner() -> impl IntoElement {
    Alert::warning(
        "config-unreadable",
        strings::main_config_unreadable_detail(),
    )
    .title(strings::main_config_unreadable_title())
}

fn empty_state(cx: &App) -> impl IntoElement {
    v_flex()
        .py(px(24.))
        .text_sm()
        .text_color(cx.theme().muted_foreground)
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
            filter_summary(&destination(
                SyncStrategy::Mirror,
                vec![FilterRule::AllFiles]
            ))
        );
        assert_eq!(
            "1 filter",
            filter_summary(&destination(
                SyncStrategy::AddOnly,
                vec![FilterRule::extension("jpg")]
            ))
        );
        assert_eq!(
            "2 filters",
            filter_summary(&destination(
                SyncStrategy::AddOnly,
                vec![FilterRule::extension("jpg"), FilterRule::extension("png")]
            ))
        );
        assert_eq!(
            "No rules",
            filter_summary(&destination(SyncStrategy::AddOnly, vec![])),
            "an empty filter list selects nothing, and the badge has to say so"
        );
    }
}
