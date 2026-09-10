//! Creating and editing a task: its name, its source, what it does, and what starts it.

use std::path::Path;
use std::sync::Arc;

use gpui::{
    div, prelude::*, px, App, Context, Entity, EventEmitter, FontWeight, PathPromptOptions, Window,
};
use gpui_component::button::{Button, ButtonGroup, ButtonVariants as _};
use gpui_component::form::{field, v_form, Field};
use gpui_component::input::{Input, InputState};
use gpui_component::{alert::Alert, h_flex, v_flex, Disableable as _, Selectable as _};
use syncmaid_core::io::FileSystem;
use syncmaid_core::model::{Destination, SyncTask, SyncTaskKind};
use syncmaid_core::triggers::{
    CronSchedule, Trigger, DEFAULT_SETTLE_SECONDS, MAX_SETTLE_SECONDS, MIN_SETTLE_SECONDS,
};
use uuid::Uuid;

use crate::components::{ChoiceCard, Glyph};
use crate::state::source_conflict;
use crate::strings;

/// What the editor decided.
pub enum TaskEditorEvent {
    /// Save this. Destinations are carried through untouched — they are edited elsewhere.
    Saved(Box<SyncTask>),
    Cancelled,
}

/// Which of the three triggers is selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TriggerChoice {
    Manual,
    Scheduled,
    Watch,
}

impl TriggerChoice {
    fn of(trigger: &Trigger) -> Self {
        match trigger {
            Trigger::Manual => Self::Manual,
            Trigger::Scheduled { .. } => Self::Scheduled,
            Trigger::Watch { .. } => Self::Watch,
        }
    }

    fn from_index(index: usize) -> Self {
        match index {
            1 => Self::Scheduled,
            2 => Self::Watch,
            _ => Self::Manual,
        }
    }
}

/// See the module docs.
pub struct TaskEditor {
    task_id: Uuid,
    /// Carried through untouched: destinations are edited in the workspace, not here.
    destinations: Vec<Destination>,
    name: Entity<InputState>,
    source: Entity<InputState>,
    cron: Entity<InputState>,
    settle: Entity<InputState>,
    kind: SyncTaskKind,
    /// A task's kind is chosen while it is empty and locked afterwards: Move's postcondition
    /// contradicts every other strategy's precondition, so a task that already has destinations
    /// cannot change what it is without emptying first.
    kind_locked: bool,
    trigger: TriggerChoice,
    /// Every task, for the source-overlap probe. Includes this one; it is filtered out by id.
    tasks: Vec<SyncTask>,
    file_system: Arc<dyn FileSystem>,
}

impl TaskEditor {
    /// A brand-new task.
    pub fn new_task(
        tasks: Vec<SyncTask>,
        file_system: Arc<dyn FileSystem>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::build(
            Uuid::new_v4(),
            "",
            "",
            Vec::new(),
            SyncTaskKind::Sync,
            &Trigger::Manual,
            tasks,
            file_system,
            window,
            cx,
        )
    }

    /// An existing task, opened for editing.
    pub fn edit(
        task: &SyncTask,
        tasks: Vec<SyncTask>,
        file_system: Arc<dyn FileSystem>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::build(
            task.id,
            &task.name,
            &task.source_path,
            task.destinations.clone(),
            task.kind(),
            &task.trigger,
            tasks,
            file_system,
            window,
            cx,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn build(
        task_id: Uuid,
        name: &str,
        source_path: &str,
        destinations: Vec<Destination>,
        kind: SyncTaskKind,
        trigger: &Trigger,
        tasks: Vec<SyncTask>,
        file_system: Arc<dyn FileSystem>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let cron_text = match trigger {
            Trigger::Scheduled { cron_expression } => cron_expression.clone(),
            _ => String::new(),
        };
        let settle_text = match trigger {
            Trigger::Watch { settle_seconds } => settle_seconds.to_string(),
            _ => DEFAULT_SETTLE_SECONDS.to_string(),
        };

        Self {
            task_id,
            kind_locked: !destinations.is_empty(),
            destinations,
            name: cx.new(|cx| InputState::new(window, cx).default_value(name.to_owned())),
            source: cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(source_path.to_owned())
                    .placeholder(strings::task_editor_source_placeholder())
            }),
            cron: cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(cron_text)
                    .placeholder("*/5 * * * *")
            }),
            settle: cx.new(|cx| InputState::new(window, cx).default_value(settle_text)),
            kind,
            trigger: TriggerChoice::of(trigger),
            tasks,
            file_system,
        }
    }

    fn name_text(&self, cx: &App) -> String {
        self.name.read(cx).value().trim().to_owned()
    }

    fn source_text(&self, cx: &App) -> String {
        self.source.read(cx).value().trim().to_owned()
    }

    fn cron_text(&self, cx: &App) -> String {
        self.cron.read(cx).value().trim().to_owned()
    }

    fn settle_seconds(&self, cx: &App) -> i32 {
        self.settle
            .read(cx)
            .value()
            .trim()
            .parse::<i32>()
            .unwrap_or(DEFAULT_SETTLE_SECONDS)
            .clamp(MIN_SETTLE_SECONDS, MAX_SETTLE_SECONDS)
    }

    /// The name of the task this one's source would collide with, if any.
    fn source_conflict(&self, cx: &App) -> Option<String> {
        let source = self.source_text(cx);
        if source.is_empty() {
            return None;
        }
        source_conflict(&self.tasks, Some(self.task_id), &source).map(|c| c.task_name)
    }

    /// What the cron field has to say for itself.
    fn cron_state(&self, cx: &App) -> CronState {
        let expression = self.cron_text(cx);
        if expression.is_empty() {
            return CronState::Empty;
        }
        match CronSchedule::parse(&expression) {
            Err(_) => CronState::Invalid,
            Ok(schedule) => match schedule.next_occurrence() {
                Some(next) => CronState::Next(next.format("%Y-%m-%d %H:%M").to_string()),
                // A pattern like `0 0 30 2 *` parses and never comes round.
                None => CronState::NeverOccurs,
            },
        }
    }

    /// True when Save should be available.
    pub fn can_save(&self, cx: &App) -> bool {
        if self.name_text(cx).is_empty() || self.source_text(cx).is_empty() {
            return false;
        }
        if self.source_conflict(cx).is_some() {
            return false;
        }
        if self.trigger == TriggerChoice::Scheduled
            && !matches!(
                self.cron_state(cx),
                CronState::Next(_) | CronState::NeverOccurs
            )
        {
            return false;
        }
        true
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        if !self.can_save(cx) {
            return;
        }

        let trigger = match self.trigger {
            TriggerChoice::Manual => Trigger::Manual,
            TriggerChoice::Scheduled => Trigger::scheduled(self.cron_text(cx)),
            TriggerChoice::Watch => Trigger::Watch {
                settle_seconds: self.settle_seconds(cx),
            },
        };

        let mut task = SyncTask::new(
            self.name_text(cx),
            self.source_text(cx),
            trigger,
            self.destinations.clone(),
        );
        task.id = self.task_id;
        // Always explicit on the way out: a saved task should never make the next load guess.
        task.set_kind(self.kind);

        cx.emit(TaskEditorEvent::Saved(Box::new(task)));
    }

    fn browse(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(strings::dialog_select_source_folder().into()),
        });
        let _ = window;

        cx.spawn_in(window, async move |editor, cx| {
            let Ok(Ok(Some(chosen))) = paths.await else {
                return; // Cancelled, or the platform refused; the field keeps what it had.
            };
            let Some(path) = chosen.into_iter().next() else {
                return;
            };
            let _ = editor.update_in(cx, |editor, window, cx| {
                let text = path.to_string_lossy().into_owned();
                editor
                    .source
                    .update(cx, |state, cx| state.set_value(text, window, cx));
                cx.notify();
            });
        })
        .detach();
    }
}

/// What the cron expression currently means.
#[derive(Debug, Clone, PartialEq, Eq)]
enum CronState {
    Empty,
    Invalid,
    /// Valid, but it never comes round — `0 0 30 2 *` names the 30th of February.
    NeverOccurs,
    Next(String),
}

impl EventEmitter<TaskEditorEvent> for TaskEditor {}

impl Render for TaskEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let source = self.source_text(cx);
        let conflict = self.source_conflict(cx);
        // One directory probe, not a tree walk. `render` runs on every frame of the sheet's
        // slide-in and on every mouse move while its scrollbar is dragged, and the walk this
        // replaces enumerated the whole source tree each of those times.
        let missing_folder = !source.is_empty()
            && conflict.is_none()
            && !self.file_system.directory_exists(Path::new(&source));

        v_form()
            .child(
                field()
                    .label(strings::common_name_label())
                    .child(Input::new(&self.name)),
            )
            .child(
                field()
                    .label(strings::task_editor_source_folder_label())
                    .child(
                        v_flex()
                            .gap(px(6.))
                            .child(
                                h_flex()
                                    .gap(px(8.))
                                    .child(div().flex_1().min_w_0().child(Input::new(&self.source)))
                                    .child(
                                        Button::new("browse-source")
                                            .label(strings::common_browse())
                                            .icon(Glyph::Folder)
                                            .outline()
                                            .on_click(cx.listener(|editor, _, window, cx| {
                                                editor.browse(window, cx)
                                            })),
                                    ),
                            )
                            .when_some(conflict, |element, other| {
                                element.child(Alert::error(
                                    "source-overlap",
                                    strings::task_editor_source_overlap_hint_format(other),
                                ))
                            })
                            .when(missing_folder, |element| {
                                // Advisory, not blocking: the user may be setting up ahead of the
                                // drive.
                                element.child(Alert::warning(
                                    "source-missing",
                                    strings::task_editor_missing_folder_hint(),
                                ))
                            }),
                    ),
            )
            .child(self.render_kind(cx))
            .child(self.render_trigger(cx))
    }
}

impl TaskEditor {
    /// The footer the sheet wraps this editor in.
    ///
    /// Lives here rather than at the call site because whether Save is available is a property of
    /// the editor's contents, and `can_save` is the only thing that knows. The sheet's builder is
    /// a per-frame `Fn`, so this is re-evaluated as the user types.
    pub fn footer(editor: &Entity<Self>, cx: &mut App) -> impl IntoElement {
        let can_save = editor.read(cx).can_save(cx);

        h_flex()
            .w_full()
            .justify_end()
            .gap(px(8.))
            .child(
                Button::new("task-cancel")
                    .label(strings::common_cancel())
                    .outline()
                    .on_click({
                        let editor = editor.clone();
                        move |_, _, cx| {
                            editor.update(cx, |_, cx| cx.emit(TaskEditorEvent::Cancelled));
                        }
                    }),
            )
            .child(
                Button::new("task-save")
                    .label(strings::task_editor_save())
                    .primary()
                    .disabled(!can_save)
                    .on_click({
                        let editor = editor.clone();
                        move |_, _, cx| editor.update(cx, |editor, cx| editor.save(cx))
                    }),
            )
    }

    fn render_kind(&self, cx: &mut Context<Self>) -> Field {
        let locked = self.kind_locked;
        let kind = self.kind;

        field().label(strings::task_editor_kind_label()).child(
            v_flex()
                .gap(px(8.))
                .child(
                    ChoiceCard::new(
                        "kind-sync",
                        Glyph::Sync,
                        strings::enum_sync_task_kind_sync(),
                        strings::task_editor_kind_sync_desc(),
                    )
                    .selected(kind == SyncTaskKind::Sync)
                    .disabled(locked)
                    .on_click(cx.listener(|editor, _, _, cx| {
                        editor.kind = SyncTaskKind::Sync;
                        cx.notify();
                    })),
                )
                .child(
                    ChoiceCard::new(
                        "kind-move",
                        Glyph::Route,
                        strings::enum_sync_task_kind_move(),
                        strings::task_editor_kind_move_desc(),
                    )
                    .selected(kind == SyncTaskKind::Move)
                    .disabled(locked)
                    .on_click(cx.listener(|editor, _, _, cx| {
                        editor.kind = SyncTaskKind::Move;
                        cx.notify();
                    })),
                )
                .when(locked, |element| {
                    element.child(Alert::new(
                        "kind-locked",
                        strings::task_editor_kind_locked_hint(),
                    ))
                }),
        )
    }

    fn render_trigger(&self, cx: &mut Context<Self>) -> Field {
        let choice = self.trigger;

        field().label(strings::task_editor_trigger_label()).child(
            v_flex()
                .gap(px(12.))
                .child(
                    ButtonGroup::new("trigger")
                        .outline()
                        .child(
                            Button::new("trigger-manual")
                                .label(strings::task_trigger_manual())
                                .icon(Glyph::Manual)
                                .selected(choice == TriggerChoice::Manual),
                        )
                        .child(
                            Button::new("trigger-scheduled")
                                .label(strings::task_editor_trigger_scheduled())
                                .icon(Glyph::Clock)
                                .selected(choice == TriggerChoice::Scheduled),
                        )
                        .child(
                            Button::new("trigger-watch")
                                .label(strings::task_editor_trigger_watch())
                                .icon(Glyph::Eye)
                                .selected(choice == TriggerChoice::Watch),
                        )
                        .on_click(cx.listener(|editor, clicked: &Vec<usize>, _, cx| {
                            if let Some(index) = clicked.first() {
                                editor.trigger = TriggerChoice::from_index(*index);
                                cx.notify();
                            }
                        })),
                )
                .when(choice == TriggerChoice::Scheduled, |element| {
                    let state = self.cron_state(cx);
                    element.child(
                        v_flex()
                            .gap(px(5.))
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(strings::task_editor_cron_label()),
                            )
                            .child(Input::new(&self.cron))
                            .child(match state {
                                CronState::Empty => {
                                    Alert::new("cron", strings::task_editor_cron_fields())
                                }
                                CronState::Invalid => {
                                    Alert::error("cron", strings::task_editor_cron_invalid())
                                }
                                CronState::NeverOccurs => {
                                    Alert::warning("cron", strings::task_editor_cron_no_upcoming())
                                }
                                CronState::Next(when) => Alert::new(
                                    "cron",
                                    strings::task_editor_cron_next_run_format(when),
                                )
                                .icon(Glyph::Clock),
                            }),
                    )
                })
                .when(choice == TriggerChoice::Watch, |element| {
                    element.child(
                        v_flex()
                            .gap(px(6.))
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(strings::task_editor_settle_label()),
                            )
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(strings::task_editor_settle_prefix())
                                    .child(div().w(px(110.)).child(Input::new(&self.settle)))
                                    .child(strings::task_editor_settle_suffix()),
                            )
                            .child(Alert::new("settle", strings::task_editor_settle_hint())),
                    )
                }),
        )
    }
}
