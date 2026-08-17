//! Every destination of one task, in one place.
//!
//! A routing task is a rule set, and a rule set can only be judged as a whole — which rule
//! catches a given file, which one wins when two match, what nothing matches. So the rules are
//! listed together and edited in place rather than one modal at a time.

use std::sync::Arc;

use gpui::{div, prelude::*, px, Context, Entity, EventEmitter, FontWeight, SharedString, Window};
use syncmaid_core::io::FileSystem;
use syncmaid_core::model::{Destination, SyncStrategy, SyncTask, SyncTaskKind};
use uuid::Uuid;

use crate::components::{
    icon, Badge, BadgeTone, Button, ButtonTone, HintBox, HintTone, Icon, IconButton, IconButtonTone,
};
use crate::state::{self, DestinationPreview, ExtensionChip, FilterModel, Scan, Summary};
use crate::views::dialogs::{dialog_card, dialog_footer, dialog_title};
use crate::{strings, theme};

/// What the workspace decided.
pub enum TaskWorkspaceEvent {
    /// The task's destinations, in order. For a Move task that order is the matching order.
    Saved(Vec<Destination>),
    Cancelled,
}

/// One destination as a row: a one-line summary that expands in place into the full editor.
struct Row {
    destination: Destination,
    /// True until an edit is applied: the row exists but the destination does not.
    draft: bool,
    editor: Option<Entity<crate::views::dialogs::DestinationEditor>>,
    /// What the last preview says this rule would take; cleared whenever the rules change.
    preview: Option<DestinationPreview>,
    /// The earlier rule that provably takes everything this one would, if there is one.
    shadowed_by: Option<String>,
}

impl Row {
    fn new(destination: Destination) -> Self {
        Self {
            // A row that has never been through the editor holds nothing worth keeping; the
            // workspace drops it if the user backs out.
            draft: destination.local_path().is_empty(),
            destination,
            editor: None,
            preview: None,
            shadowed_by: None,
        }
    }

    /// True for the "everything else" rule: a Move destination taking all files. Under
    /// first-match-wins it collects whatever the rules above it left, which is only meaningful
    /// at the end of the list — so it is pinned there and never reordered.
    fn is_catch_all(&self) -> bool {
        self.destination.strategy == SyncStrategy::Move
            && self.destination.has_only_the_all_files_filter()
    }

    /// What this destination selects, in one line.
    fn summary(&self) -> String {
        if self.is_catch_all() {
            return strings::workspace_everything_else().to_owned();
        }
        match FilterModel::of(&self.destination.filters).summary() {
            Summary::Nothing => strings::workspace_nothing_yet().to_owned(),
            Summary::Selection(what) => what,
        }
    }
}

/// See the module docs.
pub struct TaskWorkspace {
    task: SyncTask,
    tasks: Vec<SyncTask>,
    file_system: Arc<dyn FileSystem>,
    rows: Vec<Row>,

    /// Why the last save was refused, or none. A refusal only ever comes from a rule that is
    /// not finished — the workspace itself has nothing to validate.
    save_blocked: Option<String>,

    scanning: bool,
    /// The preview's headline: how many files the source holds, or why it could not be read.
    preview_summary: Option<String>,
    /// What no rule claims, and so stays in the source.
    preview_unmatched: Option<String>,
    contested: Vec<String>,
    extensions: Vec<ExtensionChip>,
}

impl EventEmitter<TaskWorkspaceEvent> for TaskWorkspace {}

impl TaskWorkspace {
    pub fn new(
        task: &SyncTask,
        tasks: Vec<SyncTask>,
        file_system: Arc<dyn FileSystem>,
        expand: Option<Uuid>,
        start_with_new_rule: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut workspace = Self {
            rows: task.destinations.iter().cloned().map(Row::new).collect(),
            task: task.clone(),
            tasks,
            file_system,
            save_blocked: None,
            scanning: false,
            preview_summary: None,
            preview_unmatched: None,
            contested: Vec::new(),
            extensions: Vec::new(),
        };
        workspace.renumber();

        // Opened from a row's edit button: that row starts open, so the click lands where the
        // user aimed it instead of on a list they then have to search.
        if let Some(id) = expand {
            workspace.expand(id, window, cx);
        }
        if start_with_new_rule {
            workspace.add_rule(window, cx);
        }
        workspace
    }

    /// True when the destinations are an ordered rule list rather than independent sync
    /// targets: numbering, reordering and the catch-all only mean something here.
    fn is_routing(&self) -> bool {
        self.task.kind() == SyncTaskKind::Move
    }

    /// True when a Move task has no "everything else" rule, so files nothing matches stay in
    /// the source. That is a legitimate choice — the catch-all is added deliberately, never
    /// conjured — so this only decides whether the button is offered.
    fn can_add_catch_all(&self) -> bool {
        self.is_routing() && !self.rows.iter().any(Row::is_catch_all)
    }

    fn index_of(&self, id: Uuid) -> Option<usize> {
        self.rows.iter().position(|row| row.destination.id == id)
    }

    /// The destinations the rules are checked against: everything but the row being edited,
    /// including rows the user has not saved yet.
    fn siblings(&self) -> Vec<Destination> {
        self.rows
            .iter()
            .filter(|row| !row.draft)
            .map(|row| row.destination.clone())
            .collect()
    }

    fn expand(&mut self, id: Uuid, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.index_of(id) else {
            return;
        };
        if self.rows[index].editor.is_some() {
            return; // Reopening keeps the pending edit.
        }

        let destination = self.rows[index].destination.clone();
        let catch_all = self.rows[index].is_catch_all();
        let siblings = self.siblings();
        let task = self.task.clone();
        let tasks = self.tasks.clone();

        let editor = cx.new(|cx| {
            let editor = crate::views::dialogs::DestinationEditor::edit(
                &task,
                &destination,
                tasks,
                window,
                cx,
            )
            .with_siblings(siblings);
            if catch_all {
                editor.catch_all()
            } else {
                editor
            }
        });

        self.rows[index].editor = Some(editor);
        // Whatever the refusal was about, opening an editor is the user acting on it.
        self.save_blocked = None;
        cx.notify();
    }

    /// Accepts the row's open editor, or explains why it cannot be accepted yet.
    fn accept(&mut self, id: Uuid, cx: &mut Context<Self>) {
        let Some(index) = self.index_of(id) else {
            return;
        };
        let Some(editor) = self.rows[index].editor.clone() else {
            return;
        };

        match editor.read(cx).build_destination(cx) {
            Some(destination) => {
                self.rows[index].destination = destination;
                self.rows[index].draft = false;
                self.rows[index].editor = None;
                self.save_blocked = None;
                self.settle(index);
                self.renumber();
            }
            None => self.save_blocked = editor.read(cx).incomplete_reason(cx),
        }
        cx.notify();
    }

    /// Discards a row's pending edit. Backing out of a rule that was never saved leaves nothing
    /// behind — the row was only ever the editor's frame.
    fn discard(&mut self, id: Uuid, cx: &mut Context<Self>) {
        let Some(index) = self.index_of(id) else {
            return;
        };
        self.rows[index].editor = None;
        if self.rows[index].draft {
            self.rows.remove(index);
        }
        self.save_blocked = None;
        self.renumber();
        cx.notify();
    }

    /// An "everything else" rule that was just accepted has to move to the end, the only place
    /// it means anything.
    fn settle(&mut self, index: usize) {
        if self.rows[index].is_catch_all() && index != self.rows.len() - 1 {
            let row = self.rows.remove(index);
            self.rows.push(row);
        }
    }

    fn add_rule(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let strategy = if self.is_routing() {
            SyncStrategy::Move
        } else {
            // Add-only is the safe default for anyone unsure: it never deletes.
            SyncStrategy::AddOnly
        };
        // A Sync destination takes everything unless told otherwise; a Move rule must say what
        // it routes, so it starts with a selection to make.
        let filters = if self.is_routing() {
            Vec::new()
        } else {
            vec![syncmaid_core::filtering::FilterRule::AllFiles]
        };

        let row = Row::new(Destination::new("", "", filters, strategy));
        let id = row.destination.id;
        // Above the catch-all: a rule below "everything else" could never match.
        let at = self
            .rows
            .iter()
            .position(Row::is_catch_all)
            .unwrap_or(self.rows.len());
        self.rows.insert(at, row);
        self.renumber();
        self.expand(id, window, cx);
    }

    /// Adds the "everything else" rule: an all-files Move destination, which under
    /// first-match-wins takes exactly what the rules above it left.
    fn add_catch_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_add_catch_all() {
            return;
        }
        let row = Row::new(Destination::new(
            strings::workspace_everything_else(),
            "",
            [syncmaid_core::filtering::FilterRule::AllFiles],
            SyncStrategy::Move,
        ));
        let id = row.destination.id;
        self.rows.push(row);
        self.renumber();
        self.expand(id, window, cx);
    }

    fn duplicate(&mut self, id: Uuid, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.index_of(id) else {
            return;
        };
        // A new id: the copy is a different destination, and sharing one would make both rows
        // report the same last-run status.
        let mut copy = self.rows[index].destination.clone();
        copy.id = Uuid::new_v4();
        // The copy has the original's path, which is an overlap with it — so it is a draft
        // until the user gives it one of its own.
        let mut row = Row::new(copy);
        row.draft = true;
        let copy_id = row.destination.id;

        self.rows.insert(index + 1, row);
        self.renumber();
        self.expand(copy_id, window, cx);
    }

    fn delete(&mut self, id: Uuid, cx: &mut Context<Self>) {
        self.rows.retain(|row| row.destination.id != id);
        self.renumber();
        cx.notify();
    }

    fn move_up(&mut self, id: Uuid, cx: &mut Context<Self>) {
        let Some(index) = self.index_of(id) else {
            return;
        };
        if index > 0 && !self.rows[index].is_catch_all() {
            self.rows.swap(index, index - 1);
            self.renumber();
            cx.notify();
        }
    }

    fn move_down(&mut self, id: Uuid, cx: &mut Context<Self>) {
        let Some(index) = self.index_of(id) else {
            return;
        };
        // The catch-all stays last: a rule below "everything else" could never match.
        if index + 1 < self.rows.len()
            && !self.rows[index].is_catch_all()
            && !self.rows[index + 1].is_catch_all()
        {
            self.rows.swap(index, index + 1);
            self.renumber();
            cx.notify();
        }
    }

    /// Saving means "keep what I typed", so a rule still being edited is committed rather than
    /// dropped — forgetting to close the editor first used to lose the edit silently, and a row
    /// that was already saved would quietly revert to what it was before.
    fn save(&mut self, cx: &mut Context<Self>) {
        if !self.commit_open_editors(cx) {
            cx.notify();
            return;
        }

        // Anything still a draft is a rule the user abandoned rather than one they were
        // editing: the commit above accepted every editor that could be accepted.
        let destinations: Vec<Destination> = self
            .rows
            .iter()
            .filter(|row| !row.draft)
            .map(|row| row.destination.clone())
            .collect();
        cx.emit(TaskWorkspaceEvent::Saved(destinations));
    }

    /// Accepts every open editor, stopping at the first one that cannot be accepted — that rule
    /// stays open with the reason shown, so what needs finishing is on screen rather than
    /// discarded.
    fn commit_open_editors(&mut self, cx: &mut Context<Self>) -> bool {
        let open: Vec<Uuid> = self
            .rows
            .iter()
            .filter(|row| row.editor.is_some())
            .map(|row| row.destination.id)
            .collect();

        for id in open {
            let Some(index) = self.index_of(id) else {
                continue;
            };
            let Some(editor) = self.rows[index].editor.clone() else {
                continue;
            };

            if let Some(reason) = editor.read(cx).incomplete_reason(cx) {
                self.save_blocked = Some(strings::workspace_save_blocked_format(reason));
                return false;
            }
            // Accepting can reorder the list, so the index is looked up again each time.
            self.accept(id, cx);
        }

        self.save_blocked = None;
        true
    }

    /// Positions change whenever the list does, and the shadowed-rule warnings depend on the
    /// same order, so the two are recomputed together.
    fn renumber(&mut self) {
        for index in 0..self.rows.len() {
            self.rows[index].shadowed_by =
                self.is_routing().then(|| self.shadowed_by(index)).flatten();
        }
        self.clear_preview();
    }

    /// The earlier rule that provably takes everything this one would.
    ///
    /// Deliberately partial: it reports only what it can prove, so a rule it stays quiet about
    /// may still overlap. The point is to catch a rule that can never match at all.
    fn shadowed_by(&self, index: usize) -> Option<String> {
        for earlier in 0..index {
            if state::subsumes(
                &self.rows[earlier].destination,
                &self.rows[index].destination,
            ) {
                return Some(strings::workspace_shadowed_by_format(
                    earlier + 1,
                    &self.rows[earlier].destination.name,
                ));
            }
        }
        None
    }

    /// Scans the source and shows where each file would go — the same first-match-wins
    /// assignment the engine runs. Reads only: nothing is written and no plan is applied.
    fn rescan(&mut self, cx: &mut Context<Self>) {
        if self.scanning {
            return;
        }
        // Only rules that exist: a row still being written has no destination behind it.
        let destinations: Vec<Destination> = self
            .rows
            .iter()
            .filter(|row| !row.draft)
            .map(|row| row.destination.clone())
            .collect();
        let file_system = Arc::clone(&self.file_system);
        let task = self.task.clone();

        self.scanning = true;
        cx.notify();

        cx.spawn(async move |workspace, cx| {
            let scanned = cx
                .background_executor()
                .spawn(async move { state::scan(&file_system, &task, &destinations) })
                .await;

            let _ = workspace.update(cx, |workspace, cx| {
                workspace.scanning = false;
                match scanned {
                    Ok(scan) => workspace.show_preview(scan),
                    // A source that cannot be read is worth saying out loud — an empty preview
                    // would otherwise read as "no files match your rules".
                    Err(error) => {
                        workspace.clear_preview();
                        workspace.preview_summary =
                            Some(strings::workspace_preview_failed_format(error));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn show_preview(&mut self, scan: Scan) {
        self.preview_summary = Some(strings::workspace_preview_source_format(files(
            scan.file_count,
        )));

        for row in &mut self.rows {
            row.preview = scan.per_destination.get(&row.destination.id).cloned();
        }

        self.preview_unmatched = (scan.unmatched.count > 0).then(|| {
            strings::workspace_preview_unmatched_format(
                files(scan.unmatched.count),
                scan.unmatched.sample.join(", "),
            )
        });

        self.contested = scan
            .contested
            .iter()
            .map(|file| {
                let positions: Vec<String> = file
                    .rules
                    .iter()
                    .map(|rule| (rule + 1).to_string())
                    .collect();
                let winner = file
                    .rules
                    .first()
                    .and_then(|rule| self.rows.get(*rule))
                    .map(|row| row.destination.name.clone())
                    .unwrap_or_default();
                strings::workspace_preview_contested_format(
                    &file.relative_path,
                    positions.join(", "),
                    winner,
                )
            })
            .collect();

        self.extensions = scan.extensions;
    }

    /// A preview describes one set of rules; the moment they change it is a claim about
    /// something that no longer exists, so it goes rather than quietly going stale.
    fn clear_preview(&mut self) {
        self.preview_summary = None;
        self.preview_unmatched = None;
        self.contested.clear();
        self.extensions.clear();
        for row in &mut self.rows {
            row.preview = None;
        }
    }
}

/// `1 file` / `12 files`.
fn files(count: usize) -> String {
    strings::common_files_count(count as i64)
}

impl Render for TaskWorkspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let routing = self.is_routing();
        // Never taller than the window it sits in: the rows scroll, the heading and the footer
        // stay put, so Save is always reachable.
        let available = window.viewport_size().height - px(64.);
        let rows: Vec<_> = (0..self.rows.len())
            .map(|index| self.render_row(index, routing, cx))
            .collect();

        dialog_card(px(760.))
            .max_h(available.min(px(600.)))
            .child(self.render_heading(routing))
            .child(
                div()
                    .id("workspace-rows")
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .when(self.rows.is_empty(), |element| {
                        element.child(div().text_color(theme::color(theme::TEXT_SECONDARY)).child(
                            if routing {
                                strings::workspace_routing_empty()
                            } else {
                                strings::workspace_sync_empty()
                            },
                        ))
                    })
                    .children(rows),
            )
            .child(self.render_preview(cx))
            .when_some(self.save_blocked.clone(), |element, reason| {
                element.child(HintBox::new(reason).tone(HintTone::Danger))
            })
            .child(self.render_footer(routing, cx))
    }
}

impl TaskWorkspace {
    fn render_heading(&self, routing: bool) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap(px(3.))
            .child(dialog_title(if routing {
                strings::workspace_rules_title()
            } else {
                strings::workspace_destinations_title()
            }))
            .child(
                div()
                    .text_color(theme::color(theme::TEXT_SECONDARY))
                    .child(if routing {
                        strings::workspace_routing_subtitle()
                    } else {
                        strings::workspace_sync_subtitle()
                    }),
            )
            .child(
                div()
                    .pt(px(3.))
                    .text_size(theme::text::small())
                    .text_color(theme::color(theme::TEXT_MUTED))
                    .font_family("Consolas")
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(self.task.source_path.clone()),
            )
    }

    fn render_row(&self, index: usize, routing: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let row = &self.rows[index];
        let id = row.destination.id;
        let open = row.editor.is_some();
        let catch_all = row.is_catch_all();

        div()
            .flex()
            .flex_col()
            .p(px(10.))
            .rounded(theme::radius::block())
            .border_1()
            .border_color(theme::color(theme::HAIRLINE))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .when(routing, |element| element.child(rule_number(index + 1)))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .mx(px(10.))
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .flex_shrink()
                                            .min_w_0()
                                            .overflow_hidden()
                                            .whitespace_nowrap()
                                            .text_ellipsis()
                                            .font_weight(FontWeight::MEDIUM)
                                            .child(row.summary()),
                                    )
                                    .child(icon(
                                        Icon::ArrowRight,
                                        px(14.),
                                        theme::color(theme::TEXT_MUTED),
                                    ))
                                    .child(
                                        div()
                                            .flex_shrink()
                                            .min_w_0()
                                            .overflow_hidden()
                                            .whitespace_nowrap()
                                            .text_ellipsis()
                                            .child(row.destination.name.clone()),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(theme::text::small())
                                    .text_color(theme::color(theme::TEXT_MUTED))
                                    .font_family("Consolas")
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .child(row.destination.local_path().to_owned()),
                            ),
                    )
                    .child(self.render_row_actions(index, routing, open, catch_all, id, cx)),
            )
            .when_some(row.shadowed_by.clone(), |element, warning| {
                // Advisory only: saving is never blocked on it.
                element.child(
                    div()
                        .pt(px(6.))
                        .child(HintBox::new(warning).tone(HintTone::Warning)),
                )
            })
            .when(open, |element| {
                element.child(
                    div()
                        .mt(px(10.))
                        .pt(px(10.))
                        .border_t_1()
                        .border_color(theme::color(theme::HAIRLINE))
                        .child(self.render_extension_chips(id, cx))
                        .children(self.rows[index].editor.clone()),
                )
            })
    }

    #[allow(clippy::too_many_arguments)]
    fn render_row_actions(
        &self,
        index: usize,
        routing: bool,
        open: bool,
        catch_all: bool,
        id: Uuid,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let preview = self.rows[index].preview.clone();
        let last = index + 1 == self.rows.len();

        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(5.))
            // What the last preview says this rule would take. A count alone does not tell you
            // whether it caught the right things, so the sample rides along as the tooltip.
            .children(preview.map(|preview| {
                div()
                    .mr(px(5.))
                    .child(Badge::new(files(preview.count)).tone(BadgeTone::Live))
            }))
            .when(routing, |element| {
                element
                    .child(
                        IconButton::new(SharedString::from(format!("up-{id}")), Icon::ArrowUp)
                            .small()
                            .glyph_size(px(14.))
                            .tooltip(strings::workspace_move_up_tip())
                            .disabled(index == 0 || catch_all)
                            .on_click(
                                cx.listener(move |workspace, _, _, cx| workspace.move_up(id, cx)),
                            ),
                    )
                    .child(
                        IconButton::new(SharedString::from(format!("down-{id}")), Icon::ArrowDown)
                            .small()
                            .glyph_size(px(14.))
                            .tooltip(strings::workspace_move_down_tip())
                            .disabled(last || catch_all)
                            .on_click(
                                cx.listener(move |workspace, _, _, cx| workspace.move_down(id, cx)),
                            ),
                    )
            })
            .child(
                IconButton::new(SharedString::from(format!("copy-{id}")), Icon::ContentCopy)
                    .small()
                    .glyph_size(px(14.))
                    .tooltip(strings::workspace_duplicate_tip())
                    .on_click(cx.listener(move |workspace, _, window, cx| {
                        workspace.duplicate(id, window, cx)
                    })),
            )
            // While the row is open its accept/discard pair lives here, beside the row it
            // belongs to: at the foot of a tall editor it sat below the fold, and a button
            // nobody sees is a button nobody presses.
            .when(!open, |element| {
                element.child(
                    IconButton::new(SharedString::from(format!("edit-{id}")), Icon::Pencil)
                        .small()
                        .glyph_size(px(14.))
                        .tooltip(strings::workspace_edit_tip())
                        .on_click(cx.listener(move |workspace, _, window, cx| {
                            workspace.expand(id, window, cx)
                        })),
                )
            })
            .when(open, |element| {
                element
                    .child(
                        IconButton::new(SharedString::from(format!("done-{id}")), Icon::Check)
                            .small()
                            .glyph_size(px(15.))
                            .tone(IconButtonTone::Run)
                            .tooltip(strings::workspace_done_tip())
                            .on_click(
                                cx.listener(move |workspace, _, _, cx| workspace.accept(id, cx)),
                            ),
                    )
                    .child(
                        IconButton::new(SharedString::from(format!("discard-{id}")), Icon::Close)
                            .small()
                            .glyph_size(px(14.))
                            .tooltip(strings::workspace_discard_tip())
                            .on_click(
                                cx.listener(move |workspace, _, _, cx| workspace.discard(id, cx)),
                            ),
                    )
            })
            .child(
                IconButton::new(
                    SharedString::from(format!("remove-{id}")),
                    Icon::TrashCanOutline,
                )
                .small()
                .glyph_size(px(14.))
                .tooltip(strings::workspace_remove_tip())
                .on_click(cx.listener(move |workspace, _, _, cx| workspace.delete(id, cx))),
            )
    }

    /// The file types the preview scan actually found in the source, offered inside an open
    /// editor as one-click rules: picking beats guessing at globs.
    fn render_extension_chips(&self, id: Uuid, cx: &mut Context<Self>) -> impl IntoElement {
        let chips: Vec<_> = self
            .extensions
            .iter()
            .map(|chip| {
                let extension = chip.extension.clone();
                Button::new(
                    SharedString::from(format!("chip-{id}-{}", chip.extension)),
                    chip.label(),
                )
                .tone(ButtonTone::Secondary)
                .on_click(cx.listener(move |workspace, _, window, cx| {
                    let Some(index) = workspace.index_of(id) else {
                        return;
                    };
                    let Some(editor) = workspace.rows[index].editor.clone() else {
                        return;
                    };
                    editor.update(cx, |editor, cx| {
                        editor.add_extension(&extension, window, cx)
                    });
                    cx.notify();
                }))
            })
            .collect();

        div().when(!chips.is_empty(), |element| {
            element
                .flex()
                .flex_row()
                .flex_wrap()
                .gap(px(6.))
                .pb(px(12.))
                .children(chips)
        })
    }

    fn render_preview(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let contested: Vec<_> = self.contested.clone();

        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        Button::new("preview", strings::workspace_preview())
                            .tone(ButtonTone::Secondary)
                            .glyph(Icon::EyeOutline)
                            .tooltip(strings::workspace_preview_tip())
                            .disabled(self.scanning)
                            .on_click(cx.listener(|workspace, _, _, cx| workspace.rescan(cx))),
                    )
                    .when(self.scanning, |element| {
                        element.child(
                            div()
                                .text_color(theme::color(theme::TEXT_SECONDARY))
                                .child(strings::workspace_scanning_source()),
                        )
                    })
                    .children(self.preview_summary.clone().map(|summary| {
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(summary)
                    })),
            )
            .when_some(self.preview_unmatched.clone(), |element, unmatched| {
                element.child(HintBox::new(unmatched).tone(HintTone::Warning))
            })
            // Contested files are information, not a problem: the order resolved them, and this
            // is where the user sees which rule actually won.
            .when(!contested.is_empty(), |element| {
                element.child(
                    div()
                        .flex()
                        .flex_col()
                        .text_size(theme::text::small())
                        .text_color(theme::color(theme::TEXT_SECONDARY))
                        .children(contested),
                )
            })
    }

    fn render_footer(&self, routing: bool, cx: &mut Context<Self>) -> impl IntoElement {
        dialog_footer()
            .child(
                Button::new(
                    "add-rule",
                    if routing {
                        strings::workspace_add_rule()
                    } else {
                        strings::workspace_add_destination()
                    },
                )
                .tone(ButtonTone::Secondary)
                .glyph(Icon::Plus)
                .on_click(cx.listener(|workspace, _, window, cx| workspace.add_rule(window, cx))),
            )
            // Offered only while the task has no catch-all: it is added deliberately, and a
            // second one could never match.
            .when(self.can_add_catch_all(), |element| {
                element.child(
                    Button::new("add-catch-all", strings::workspace_add_catch_all())
                        .tone(ButtonTone::Secondary)
                        .glyph(Icon::TrayArrowDown)
                        .tooltip(strings::workspace_add_catch_all_tip())
                        .on_click(cx.listener(|workspace, _, window, cx| {
                            workspace.add_catch_all(window, cx)
                        })),
                )
            })
            .child(div().flex_1())
            .child(
                Button::new("workspace-cancel", strings::common_cancel())
                    .tone(ButtonTone::Secondary)
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(TaskWorkspaceEvent::Cancelled))),
            )
            .child(
                Button::new("workspace-save", strings::workspace_save())
                    .on_click(cx.listener(|workspace, _, _, cx| workspace.save(cx))),
            )
    }
}

/// The rule's position, which for a Move task is the order it is matched in.
fn rule_number(number: usize) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .justify_center()
        .size(px(22.))
        .rounded(theme::radius::control())
        .bg(theme::color(theme::SUBTLE))
        .text_size(theme::text::small())
        .text_color(theme::color(theme::TEXT_SECONDARY))
        .child(number.to_string())
}
