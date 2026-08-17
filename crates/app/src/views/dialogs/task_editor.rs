//! Creating and editing a task: its name, its source, what it does, and what starts it.

use std::path::Path;
use std::sync::Arc;

use gpui::{div, prelude::*, px, Context, Entity, EventEmitter, PathPromptOptions, Window};
use gpui_component::input::{Input, InputState};
use syncmaid_core::io::FileSystem;
use syncmaid_core::model::{Destination, SyncTask, SyncTaskKind};
use syncmaid_core::triggers::{
    CronSchedule, Trigger, DEFAULT_SETTLE_SECONDS, MAX_SETTLE_SECONDS, MIN_SETTLE_SECONDS,
};
use uuid::Uuid;

use crate::components::{
    Button, ButtonTone, ChoiceCard, HintBox, HintTone, Icon, Segment, SegmentOption,
};
use crate::state::source_conflict;
use crate::strings;
use crate::views::dialogs::{dialog_card, dialog_footer, dialog_title, field_label};

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

    fn index(self) -> usize {
        match self {
            Self::Manual => 0,
            Self::Scheduled => 1,
            Self::Watch => 2,
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

    fn name_text(&self, cx: &Context<Self>) -> String {
        self.name.read(cx).value().trim().to_owned()
    }

    fn source_text(&self, cx: &Context<Self>) -> String {
        self.source.read(cx).value().trim().to_owned()
    }

    fn cron_text(&self, cx: &Context<Self>) -> String {
        self.cron.read(cx).value().trim().to_owned()
    }

    fn settle_seconds(&self, cx: &Context<Self>) -> i32 {
        self.settle
            .read(cx)
            .value()
            .trim()
            .parse::<i32>()
            .unwrap_or(DEFAULT_SETTLE_SECONDS)
            .clamp(MIN_SETTLE_SECONDS, MAX_SETTLE_SECONDS)
    }

    /// The name of the task this one's source would collide with, if any.
    fn source_conflict(&self, cx: &Context<Self>) -> Option<String> {
        let source = self.source_text(cx);
        if source.is_empty() {
            return None;
        }
        source_conflict(&self.tasks, Some(self.task_id), &source).map(|c| c.task_name)
    }

    /// What the cron field has to say for itself.
    fn cron_state(&self, cx: &Context<Self>) -> CronState {
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
    fn can_save(&self, cx: &Context<Self>) -> bool {
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
        let missing_folder = !source.is_empty()
            && conflict.is_none()
            && !self.file_system.file_exists(Path::new(&source))
            && self.file_system.list_tree(Path::new(&source)).is_err();
        let can_save = self.can_save(cx);

        dialog_card(px(470.))
            .child(dialog_title(strings::task_editor_title()))
            .child(
                div()
                    .child(field_label(strings::common_name_label()))
                    .child(Input::new(&self.name)),
            )
            .child(
                div()
                    .child(field_label(strings::task_editor_source_folder_label()))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap(px(8.))
                            .child(div().flex_1().min_w_0().child(Input::new(&self.source)))
                            .child(
                                Button::new("browse-source", strings::common_browse())
                                    .tone(ButtonTone::Secondary)
                                    .glyph(Icon::FolderOutline)
                                    .on_click(cx.listener(|editor, _, window, cx| {
                                        editor.browse(window, cx)
                                    })),
                            ),
                    )
                    .when_some(conflict, |element, other| {
                        element.child(
                            div().pt(px(6.)).child(
                                HintBox::new(strings::task_editor_source_overlap_hint_format(
                                    other,
                                ))
                                .tone(HintTone::Danger),
                            ),
                        )
                    })
                    .when(missing_folder, |element| {
                        // Advisory, not blocking: the user may be setting up ahead of the drive.
                        element.child(
                            div().pt(px(6.)).child(
                                HintBox::new(strings::task_editor_missing_folder_hint())
                                    .tone(HintTone::Warning),
                            ),
                        )
                    }),
            )
            .child(self.render_kind(cx))
            .child(self.render_trigger(cx))
            .child(
                dialog_footer()
                    .child(
                        Button::new("task-cancel", strings::common_cancel())
                            .tone(ButtonTone::Secondary)
                            .on_click(
                                cx.listener(|_, _, _, cx| cx.emit(TaskEditorEvent::Cancelled)),
                            ),
                    )
                    .child(
                        Button::new("task-save", strings::task_editor_save())
                            .disabled(!can_save)
                            .on_click(cx.listener(|editor, _, _, cx| editor.save(cx))),
                    ),
            )
    }
}

impl TaskEditor {
    fn render_kind(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let locked = self.kind_locked;
        let kind = self.kind;

        div()
            .child(field_label(strings::task_editor_kind_label()))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        ChoiceCard::new(
                            "kind-sync",
                            Icon::Sync,
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
                            Icon::CallSplit,
                            strings::enum_sync_task_kind_move(),
                            strings::task_editor_kind_move_desc(),
                        )
                        .selected(kind == SyncTaskKind::Move)
                        .disabled(locked)
                        .on_click(cx.listener(|editor, _, _, cx| {
                            editor.kind = SyncTaskKind::Move;
                            cx.notify();
                        })),
                    ),
            )
            .when(locked, |element| {
                element.child(
                    div()
                        .pt(px(6.))
                        .child(HintBox::new(strings::task_editor_kind_locked_hint())),
                )
            })
    }

    fn render_trigger(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let choice = self.trigger;

        div()
            .child(field_label(strings::task_editor_trigger_label()))
            .child(
                Segment::new(
                    "trigger",
                    vec![
                        SegmentOption::new(strings::task_trigger_manual())
                            .glyph(Icon::CursorDefaultClickOutline),
                        SegmentOption::new(strings::task_editor_trigger_scheduled())
                            .glyph(Icon::ClockOutline),
                        SegmentOption::new(strings::task_editor_trigger_watch()).glyph(Icon::Eye),
                    ],
                    choice.index(),
                )
                .on_select(cx.listener(|editor, index: &usize, _, cx| {
                    editor.trigger = TriggerChoice::from_index(*index);
                    cx.notify();
                })),
            )
            .when(choice == TriggerChoice::Scheduled, |element| {
                let state = self.cron_state(cx);
                element.child(
                    div()
                        .pt(px(12.))
                        .child(field_label(strings::task_editor_cron_label()))
                        .child(Input::new(&self.cron))
                        .child(
                            div().pt(px(5.)).child(match state {
                                CronState::Empty => {
                                    HintBox::new(strings::task_editor_cron_fields())
                                }
                                CronState::Invalid => {
                                    HintBox::new(strings::task_editor_cron_invalid())
                                        .tone(HintTone::Danger)
                                }
                                CronState::NeverOccurs => {
                                    HintBox::new(strings::task_editor_cron_no_upcoming())
                                        .tone(HintTone::Warning)
                                }
                                CronState::Next(when) => {
                                    HintBox::new(strings::task_editor_cron_next_run_format(when))
                                        .glyph(Icon::ClockOutline)
                                }
                            }),
                        ),
                )
            })
            .when(choice == TriggerChoice::Watch, |element| {
                element.child(
                    div()
                        .pt(px(12.))
                        .child(field_label(strings::task_editor_settle_label()))
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(8.))
                                .child(strings::task_editor_settle_prefix())
                                .child(div().w(px(90.)).child(Input::new(&self.settle)))
                                .child(strings::task_editor_settle_suffix()),
                        )
                        .child(
                            div()
                                .pt(px(6.))
                                .child(HintBox::new(strings::task_editor_settle_hint())),
                        ),
                )
            })
    }
}
