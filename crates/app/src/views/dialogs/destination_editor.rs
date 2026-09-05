//! Adding and editing a destination: where files go, which ones, and how they are reconciled.
//!
//! It is never a dialog of its own. Every destination edit goes through the task workspace,
//! because which rule catches a file is a property of the whole ordered list — so the editor
//! draws its fields and nothing else, and the row it opens inside owns the frame, the
//! accept/discard pair, and what happens to the result.

use std::path::Path;

use gpui::{div, prelude::*, px, App, Context, Entity, PathPromptOptions, SharedString, Window};
use gpui_component::button::{Button, ButtonGroup, ButtonVariants as _};
use gpui_component::form::{field, v_form, Field};
use gpui_component::input::{Input, InputState};
use gpui_component::{
    alert::Alert, checkbox::Checkbox, h_flex, v_flex, ActiveTheme as _, Icon, Selectable as _,
    Sizable as _, Size,
};
use syncmaid_core::io::{is_network, paths_overlap};
use syncmaid_core::model::{
    DeleteMode, Destination, FileNameCollisionPolicy, SyncStrategy, SyncTask, SyncTaskKind,
    DEFAULT_MASS_DELETE_THRESHOLD,
};
use uuid::Uuid;

use crate::components::{ChoiceCard, Glyph};
use crate::state::{destination_conflict, FilterEntry, FilterGroup, FilterKind, FilterModel};

use crate::strings;

/// See the module docs.
pub struct DestinationEditor {
    destination_id: Uuid,
    task_id: Uuid,
    /// A Move task's destinations are routing rules and are always Move; the choice is hidden.
    task_kind: SyncTaskKind,
    source_path: String,
    /// The "everything else" rule. It takes whatever the rules above it left, which is what it
    /// is for, so there is no file selection to edit.
    catch_all: bool,
    /// The rows beside this one in the workspace, which are not on the task yet. Empty for the
    /// standalone modal, where the persisted task list already holds every sibling.
    siblings: Vec<Destination>,

    name: Entity<InputState>,
    path: Entity<InputState>,
    threshold: Entity<InputState>,
    /// One "new rule" field per group.
    group_inputs: Vec<Entity<InputState>>,
    group_kinds: Vec<FilterKind>,

    strategy: SyncStrategy,
    verify_contents: bool,
    delete_mode: DeleteMode,
    flatten: bool,
    collision: FileNameCollisionPolicy,
    confirm_large_deletions: bool,
    filters: FilterModel,

    tasks: Vec<SyncTask>,
}

impl DestinationEditor {
    pub fn edit(
        task: &SyncTask,
        destination: &Destination,
        tasks: Vec<SyncTask>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::build(task, destination, tasks, window, cx)
    }

    fn build(
        task: &SyncTask,
        destination: &Destination,
        tasks: Vec<SyncTask>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let filters = FilterModel::of(&destination.filters);
        let groups = filters.groups.len().max(1);

        Self {
            destination_id: destination.id,
            task_id: task.id,
            task_kind: task.kind(),
            source_path: task.source_path.clone(),
            catch_all: false,
            siblings: Vec::new(),
            name: cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(destination.name.clone())
                    .placeholder(strings::dest_editor_name_placeholder())
            }),
            path: cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(destination.local_path().to_owned())
                    .placeholder(strings::dest_editor_path_placeholder())
            }),
            threshold: cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(percent(destination.mass_delete_threshold).to_string())
            }),
            group_inputs: (0..groups)
                .map(|_| {
                    cx.new(|cx| {
                        InputState::new(window, cx)
                            .placeholder(strings::dest_editor_pattern_placeholder())
                    })
                })
                .collect(),
            group_kinds: vec![FilterKind::default(); groups],
            strategy: destination.strategy,
            verify_contents: destination.verify_contents,
            delete_mode: destination.delete_mode,
            flatten: destination.flatten_structure,
            collision: destination.collision_policy,
            // Zero means the ratio guard is off; the empty-source guard always applies.
            confirm_large_deletions: destination.mass_delete_threshold > 0.0,
            filters,
            tasks,
        }
    }

    /// Marks this as the "everything else" rule: it has no file selection to edit, and it
    /// saves the lone all-files filter that makes it one.
    pub fn catch_all(mut self) -> Self {
        self.catch_all = true;
        self.filters = FilterModel::of(&[syncmaid_core::filtering::FilterRule::AllFiles]);
        self
    }

    /// The rows beside this one that are not on the task yet, for the overlap check.
    pub fn with_siblings(mut self, siblings: Vec<Destination>) -> Self {
        self.siblings = siblings;
        self
    }

    /// Why this editor cannot be accepted yet, in one sentence; null when it can.
    ///
    /// The workspace commits open editors when the user saves, so it needs to explain a refusal
    /// in the same words the editor itself would use.
    pub fn incomplete_reason(&self, cx: &App) -> Option<String> {
        self.blocked_reason(cx)
    }

    /// Adds a rule for one of the file types the preview scan found in the source, turning glob
    /// authoring into picking.
    pub fn add_extension(&mut self, extension: &str, window: &mut Window, cx: &mut Context<Self>) {
        let extension = extension.trim();
        if extension.is_empty() || self.catch_all {
            return;
        }

        // Picking a type is a file selection, so "all files" cannot still be the answer.
        self.filters.all_files = false;
        if self.filters.groups.is_empty() {
            self.add_group(window, cx);
        }
        self.filters.groups[0]
            .rules
            .push(FilterEntry::new(FilterKind::Extension, extension));
        cx.notify();
    }

    fn path_text(&self, cx: &App) -> String {
        self.path.read(cx).value().trim().to_owned()
    }

    /// The name to save: what the user typed, or the folder's own name.
    fn resolved_name(&self, cx: &App) -> String {
        let typed = self.name.read(cx).value().trim().to_owned();
        if !typed.is_empty() {
            return typed;
        }
        let path = self.path_text(cx);
        Path::new(&path)
            .file_name()
            .map(|leaf| leaf.to_string_lossy().into_owned())
            .unwrap_or(path)
    }

    fn threshold_fraction(&self, cx: &App) -> f64 {
        if !self.confirm_large_deletions {
            return 0.0;
        }
        let percent = self
            .threshold
            .read(cx)
            .value()
            .trim()
            .parse::<f64>()
            .unwrap_or(percent(DEFAULT_MASS_DELETE_THRESHOLD) as f64)
            .clamp(1.0, 100.0);
        percent / 100.0
    }

    /// Mirror is the one strategy with no filter section: its contract is tree identity, and a
    /// filtered subset contradicts that by definition. The catch-all has none either — taking
    /// what the rules above it left is the whole rule.
    fn shows_filters(&self) -> bool {
        self.strategy != SyncStrategy::Mirror && !self.catch_all
    }

    /// Why the save is blocked, if it is.
    fn blocked_reason(&self, cx: &App) -> Option<String> {
        let path = self.path_text(cx);
        if path.is_empty() {
            return Some(strings::dest_editor_needs_folder().into());
        }

        // Task shape: a destination never sits inside its own source, and never contains it.
        if paths_overlap(Path::new(&path), Path::new(&self.source_path)) {
            return Some(strings::dest_editor_needs_separate_folder().into());
        }

        if let Some(conflict) = self.overlapping_destination(&path) {
            return Some(conflict);
        }

        if self.shows_filters() && self.filters.selects_nothing() {
            return Some(strings::dest_editor_needs_filter_rule().into());
        }

        None
    }

    /// Task shape: destinations never overlap — the rows beside this one, and every other
    /// task's destinations.
    fn overlapping_destination(&self, path: &str) -> Option<String> {
        let candidate = Path::new(path);
        if let Some(sibling) = self.siblings.iter().find(|other| {
            other.id != self.destination_id
                && paths_overlap(Path::new(other.local_path()), candidate)
        }) {
            return Some(strings::dest_editor_sibling_overlap_hint_format(
                &sibling.name,
            ));
        }

        // With siblings in hand the whole owning task is skipped, because its saved
        // destinations are exactly the rows the workspace is already holding — half of them
        // possibly edited. Without them the persisted list is the only sibling list there is.
        let (task, destination) = if self.siblings.is_empty() {
            (None, Some(self.destination_id))
        } else {
            (Some(self.task_id), None)
        };
        destination_conflict(&self.tasks, task, destination, path).map(|conflict| {
            strings::dest_editor_destination_overlap_hint_format(conflict.task_name)
        })
    }

    /// The destination this editor currently describes, or `None` while `incomplete_reason` has
    /// something to say.
    ///
    /// The workspace reads it rather than being told, because saving the whole task has to fold
    /// every open editor in before it writes anything, and an event would arrive too late.
    pub fn build_destination(&self, cx: &App) -> Option<Destination> {
        if self.blocked_reason(cx).is_some() {
            return None;
        }

        let mut destination = Destination::new(
            self.resolved_name(cx),
            self.path_text(cx),
            // Mirror persists a lone all-files filter whatever the editor was showing, which is
            // what normalizes a legacy filtered Mirror on its next save.
            if self.strategy == SyncStrategy::Mirror {
                vec![syncmaid_core::filtering::FilterRule::AllFiles]
            } else {
                self.filters.to_rules()
            },
            self.strategy,
        );
        destination.id = self.destination_id;
        destination.verify_contents = self.verify_contents;
        destination.delete_mode = self.delete_mode;
        destination.flatten_structure = self.strategy == SyncStrategy::Move && self.flatten;
        destination.collision_policy = self.collision;
        destination.mass_delete_threshold = self.threshold_fraction(cx);

        Some(destination)
    }

    fn browse(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(strings::dialog_select_destination_folder().into()),
        });

        cx.spawn_in(window, async move |editor, cx| {
            let Ok(Ok(Some(chosen))) = paths.await else {
                return;
            };
            let Some(path) = chosen.into_iter().next() else {
                return;
            };
            let _ = editor.update_in(cx, |editor, window, cx| {
                let text = path.to_string_lossy().into_owned();
                editor
                    .path
                    .update(cx, |state, cx| state.set_value(text, window, cx));
                cx.notify();
            });
        })
        .detach();
    }

    fn add_rule(&mut self, group: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(input) = self.group_inputs.get(group) else {
            return;
        };
        let pattern = input.read(cx).value().trim().to_owned();
        if pattern.is_empty() {
            return;
        }

        while self.filters.groups.len() <= group {
            self.filters.groups.push(FilterGroup::default());
        }
        self.filters.groups[group]
            .rules
            .push(FilterEntry::new(self.group_kinds[group], pattern));
        self.filters.all_files = false;

        input.update(cx, |state, cx| state.set_value("", window, cx));
        cx.notify();
    }

    fn add_group(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.filters.groups.push(FilterGroup::default());
        self.group_kinds.push(FilterKind::default());
        self.group_inputs.push(cx.new(|cx| {
            InputState::new(window, cx).placeholder(strings::dest_editor_pattern_placeholder())
        }));
        cx.notify();
    }

    fn remove_group(&mut self, group: usize, cx: &mut Context<Self>) {
        if self.filters.groups.len() <= 1 {
            return;
        }
        self.filters.groups.remove(group);
        self.group_kinds.remove(group);
        self.group_inputs.remove(group);
        cx.notify();
    }
}

/// One rule as its own row reads it: the kind named, then the pattern.
fn describe_rule(rule: &FilterEntry) -> String {
    let body = match rule.kind {
        FilterKind::Path => strings::filter_path_row_format(&rule.pattern),
        FilterKind::Extension => strings::filter_extension_row_format(&rule.pattern),
        FilterKind::Wildcard => strings::filter_wildcard_row_format(&rule.pattern),
    };
    if rule.excluded {
        strings::filter_exclude_row_format(body)
    } else {
        body
    }
}

fn percent(fraction: f64) -> u32 {
    (fraction * 100.0).round() as u32
}

impl Render for DestinationEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // No card, no title, no footer: the workspace row it opens inside is the frame, and its
        // accept/discard pair sits up beside the summary line rather than below the fold.
        v_flex()
            .w_full()
            .min_w_0()
            .gap(px(16.))
            .child(self.render_fields(cx))
            .when_some(self.blocked_reason(cx), |element, reason| {
                element.child(Alert::error("destination-blocked", reason))
            })
    }
}

impl DestinationEditor {
    /// Everything the editor edits, in one column.
    fn render_fields(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let path = self.path_text(cx);
        let network = !path.is_empty() && is_network(Path::new(&path));

        v_form()
            .child(
                field()
                    .label(strings::common_name_label())
                    .child(Input::new(&self.name)),
            )
            .child(self.render_folder(cx))
            .when(self.task_kind == SyncTaskKind::Sync, |element| {
                element.child(self.render_strategy(cx))
            })
            .when(self.strategy == SyncStrategy::Move, |element| {
                element.child(self.render_move_options(cx))
            })
            .when(self.shows_filters(), |element| {
                element.child(self.render_filters(cx))
            })
            .when(self.catch_all, |element| {
                element.child(field().child(Alert::new(
                    "catch-all",
                    strings::dest_editor_catch_all_hint(),
                )))
            })
            .child(self.render_verification(network, cx))
            .when(self.strategy == SyncStrategy::Mirror, |element| {
                element.child(self.render_deletions(cx))
            })
    }

    fn render_folder(&self, cx: &mut Context<Self>) -> Field {
        field().label(strings::dest_editor_folder_label()).child(
            v_flex()
                .gap(px(6.))
                .child(
                    h_flex()
                        .gap(px(8.))
                        .child(div().flex_1().min_w_0().child(Input::new(&self.path)))
                        .child(
                            Button::new("browse-destination")
                                .label(strings::common_browse())
                                .icon(Glyph::Folder)
                                .outline()
                                .on_click(
                                    cx.listener(|editor, _, window, cx| editor.browse(window, cx)),
                                ),
                        ),
                )
                .child(Alert::new(
                    "destination-nesting",
                    strings::workspace_sync_destination_hint(),
                )),
        )
    }

    fn render_strategy(&self, cx: &mut Context<Self>) -> Field {
        let strategy = self.strategy;

        field().label(strings::dest_editor_strategy_label()).child(
            v_flex()
                .gap(px(8.))
                .child(
                    ChoiceCard::new(
                        "strategy-mirror",
                        Glyph::Sync,
                        strings::enum_sync_strategy_mirror(),
                        strings::dest_editor_mirror_desc(),
                    )
                    .selected(strategy == SyncStrategy::Mirror)
                    .on_click(cx.listener(|editor, _, _, cx| {
                        editor.strategy = SyncStrategy::Mirror;
                        cx.notify();
                    })),
                )
                .child(
                    ChoiceCard::new(
                        "strategy-add-only",
                        Glyph::Add,
                        strings::enum_sync_strategy_add_only(),
                        strings::dest_editor_add_only_desc(),
                    )
                    .selected(strategy == SyncStrategy::AddOnly)
                    .on_click(cx.listener(|editor, _, _, cx| {
                        editor.strategy = SyncStrategy::AddOnly;
                        cx.notify();
                    })),
                ),
        )
    }

    fn render_move_options(&self, cx: &mut Context<Self>) -> Field {
        let flatten = self.flatten;
        let collision = self.collision;

        field()
            .label(strings::dest_editor_move_options_label())
            .child(
                v_flex()
                    .gap(px(10.))
                    .child(
                        ButtonGroup::new("move-layout")
                            .outline()
                            .child(
                                Button::new("keep-structure")
                                    .label(strings::dest_editor_keep_structure())
                                    .icon(Glyph::Tree)
                                    .selected(!flatten),
                            )
                            .child(
                                Button::new("flatten")
                                    .label(strings::dest_editor_flatten())
                                    .icon(Glyph::Folder)
                                    .selected(flatten),
                            )
                            .on_click(cx.listener(|editor, clicked: &Vec<usize>, _, cx| {
                                if let Some(index) = clicked.first() {
                                    editor.flatten = *index == 1;
                                    cx.notify();
                                }
                            })),
                    )
                    .when(flatten, |element| {
                        element.child(
                            v_flex()
                                .gap(px(5.))
                                .child(sub_label(strings::dest_editor_collision_label(), cx))
                                .child(
                                    ButtonGroup::new("collision")
                                        .outline()
                                        .child(
                                            Button::new("collision-skip")
                                                .label(
                                                    strings::enum_file_name_collision_policy_skip(),
                                                )
                                                .icon(Glyph::Idle)
                                                .selected(
                                                    collision == FileNameCollisionPolicy::Skip,
                                                ),
                                        )
                                        .child(
                                            Button::new("collision-suffix")
                                                .label(
                                                    strings::enum_file_name_collision_policy_suffix(
                                                    ),
                                                )
                                                .icon(Glyph::Copy)
                                                .selected(
                                                    collision == FileNameCollisionPolicy::Suffix,
                                                ),
                                        )
                                        .on_click(cx.listener(
                                            |editor, clicked: &Vec<usize>, _, cx| {
                                                if let Some(index) = clicked.first() {
                                                    editor.collision = if *index == 1 {
                                                        FileNameCollisionPolicy::Suffix
                                                    } else {
                                                        FileNameCollisionPolicy::Skip
                                                    };
                                                    cx.notify();
                                                }
                                            },
                                        )),
                                ),
                        )
                    }),
            )
    }

    fn render_filters(&self, cx: &mut Context<Self>) -> Field {
        let all_files = self.filters.all_files;
        let several_groups = self.filters.groups.len() > 1;
        let groups: Vec<_> = self
            .filters
            .groups
            .iter()
            .enumerate()
            .map(|(index, group)| self.render_group(index, group, several_groups, cx))
            .collect();

        field()
            .label(strings::dest_editor_files_to_sync_label())
            .child(
                v_flex()
                    .gap(px(10.))
                    .child(
                        ButtonGroup::new("filter-mode")
                            .outline()
                            .child(
                                Button::new("filter-all")
                                    .label(strings::filter_all_files())
                                    .icon(Glyph::Asterisk)
                                    .selected(all_files),
                            )
                            .child(
                                Button::new("filter-matching")
                                    .label(strings::dest_editor_only_matching())
                                    .icon(Glyph::Filter)
                                    .selected(!all_files),
                            )
                            .on_click(cx.listener(|editor, clicked: &Vec<usize>, _, cx| {
                                let Some(index) = clicked.first() else {
                                    return;
                                };
                                editor.filters.all_files = *index == 0;
                                if !editor.filters.all_files && editor.filters.groups.is_empty() {
                                    editor.filters.groups.push(FilterGroup::default());
                                }
                                cx.notify();
                            })),
                    )
                    .when(!all_files, |element| {
                        element
                            // Only worth asking once there is more than one group to combine.
                            .when(several_groups, |element| {
                                element.child(
                                    ButtonGroup::new("group-combine")
                                        .outline()
                                        .compact()
                                        .with_size(Size::Small)
                                        .child(
                                            Button::new("groups-any")
                                                .label(strings::dest_editor_match_any_group())
                                                .selected(!self.filters.match_all_groups),
                                        )
                                        .child(
                                            Button::new("groups-all")
                                                .label(strings::dest_editor_match_all_groups())
                                                .selected(self.filters.match_all_groups),
                                        )
                                        .on_click(cx.listener(
                                            |editor, clicked: &Vec<usize>, _, cx| {
                                                if let Some(index) = clicked.first() {
                                                    editor.filters.match_all_groups = *index == 1;
                                                    cx.notify();
                                                }
                                            },
                                        )),
                                )
                            })
                            .children(groups)
                            .child(
                                h_flex().child(
                                    Button::new("add-group")
                                        .label(strings::dest_editor_add_group())
                                        .icon(Glyph::Add)
                                        .outline()
                                        .on_click(cx.listener(|editor, _, window, cx| {
                                            editor.add_group(window, cx)
                                        })),
                                ),
                            )
                            // The live plain-language line: the single best guard against a
                            // filter that quietly selects the wrong thing.
                            .child(
                                Alert::new("filter-summary", self.filters.describe())
                                    .icon(Glyph::Eye),
                            )
                    }),
            )
    }

    fn render_group(
        &self,
        index: usize,
        group: &FilterGroup,
        removable: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let kind = self.group_kinds.get(index).copied().unwrap_or_default();
        let rules: Vec<_> = group
            .rules
            .iter()
            .enumerate()
            .map(|(rule_index, rule)| self.render_rule(index, rule_index, rule, cx))
            .collect();

        v_flex()
            .gap(px(6.))
            .p(px(10.))
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .items_center()
                    .gap(px(8.))
                    .child(sub_label(strings::dest_editor_match_label(), cx))
                    .child(
                        ButtonGroup::new(SharedString::from(format!("group-{index}-mode")))
                            .outline()
                            .compact()
                            .with_size(Size::Small)
                            .child(
                                Button::new("any")
                                    .label(strings::dest_editor_any_rule())
                                    .selected(!group.match_all),
                            )
                            .child(
                                Button::new("all")
                                    .label(strings::dest_editor_all_rules())
                                    .selected(group.match_all),
                            )
                            .on_click(cx.listener(move |editor, clicked: &Vec<usize>, _, cx| {
                                if let Some(choice) = clicked.first() {
                                    if let Some(group) = editor.filters.groups.get_mut(index) {
                                        group.match_all = *choice == 1;
                                    }
                                    cx.notify();
                                }
                            })),
                    )
                    .child(div().flex_1())
                    .when(removable, |element| {
                        element.child(
                            Button::new(SharedString::from(format!("remove-group-{index}")))
                                .icon(Glyph::Close)
                                .ghost()
                                .xsmall()
                                .on_click(cx.listener(move |editor, _, _, cx| {
                                    editor.remove_group(index, cx)
                                })),
                        )
                    }),
            )
            .children(rules)
            .child(
                h_flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        ButtonGroup::new(SharedString::from(format!("group-{index}-kind")))
                            .outline()
                            .compact()
                            .with_size(Size::Small)
                            .child(
                                Button::new("path")
                                    .label(strings::enum_filter_kind_path())
                                    .selected(kind == FilterKind::Path),
                            )
                            .child(
                                Button::new("extension")
                                    .label(strings::enum_filter_kind_extension())
                                    .selected(kind == FilterKind::Extension),
                            )
                            .child(
                                Button::new("wildcard")
                                    .label(strings::enum_filter_kind_wildcard())
                                    .selected(kind == FilterKind::Wildcard),
                            )
                            .on_click(cx.listener(move |editor, clicked: &Vec<usize>, _, cx| {
                                if let Some(choice) = clicked.first() {
                                    if let Some(slot) = editor.group_kinds.get_mut(index) {
                                        *slot = FilterKind::from_index(*choice);
                                    }
                                    cx.notify();
                                }
                            })),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .children(self.group_inputs.get(index).map(Input::new)),
                    )
                    .child(
                        Button::new(SharedString::from(format!("add-rule-{index}")))
                            .label(strings::common_add())
                            .icon(Glyph::Add)
                            .primary()
                            .on_click(cx.listener(move |editor, _, window, cx| {
                                editor.add_rule(index, window, cx)
                            })),
                    ),
            )
            .when(kind == FilterKind::Wildcard, |element| {
                element.child(
                    Alert::new(
                        SharedString::from(format!("wildcard-hint-{index}")),
                        strings::dest_editor_wildcard_hint(),
                    )
                    .icon(Glyph::Asterisk)
                    .with_size(Size::Small),
                )
            })
    }

    fn render_rule(
        &self,
        group: usize,
        index: usize,
        rule: &FilterEntry,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let excluded = rule.excluded;

        h_flex()
            .items_center()
            .gap(px(8.))
            .px(px(10.))
            .py(px(6.))
            .rounded(cx.theme().radius)
            .bg(cx.theme().muted)
            .child(
                Icon::new(Glyph::Filter)
                    .size(px(15.))
                    .text_color(cx.theme().muted_foreground),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(describe_rule(rule)),
            )
            .child(
                Button::new(SharedString::from(format!("exclude-{group}-{index}")))
                    .icon(Glyph::Remove)
                    .ghost()
                    .xsmall()
                    .map(|button| if excluded { button.danger() } else { button })
                    .tooltip(strings::dest_editor_exclude_tip())
                    .on_click(cx.listener(move |editor, _, _, cx| {
                        if let Some(entry) = editor
                            .filters
                            .groups
                            .get_mut(group)
                            .and_then(|group| group.rules.get_mut(index))
                        {
                            entry.excluded = !entry.excluded;
                        }
                        cx.notify();
                    })),
            )
            .child(
                Button::new(SharedString::from(format!("remove-rule-{group}-{index}")))
                    .icon(Glyph::Close)
                    .ghost()
                    .xsmall()
                    .on_click(cx.listener(move |editor, _, _, cx| {
                        if let Some(group) = editor.filters.groups.get_mut(group) {
                            group.rules.remove(index);
                        }
                        cx.notify();
                    })),
            )
    }

    fn render_verification(&self, network: bool, cx: &mut Context<Self>) -> Field {
        field()
            .label(strings::dest_editor_verification_label())
            .child(
                v_flex()
                    .gap(px(6.))
                    .child(
                        Checkbox::new("verify-contents")
                            .label(strings::dest_editor_verify_contents())
                            .checked(self.verify_contents)
                            .on_click(cx.listener(|editor, checked: &bool, _, cx| {
                                editor.verify_contents = *checked;
                                cx.notify();
                            })),
                    )
                    .when(network && self.verify_contents, |element| {
                        element.child(Alert::error(
                            "verify-network",
                            strings::dest_editor_verify_network_warning(),
                        ))
                    }),
            )
    }

    fn render_deletions(&self, cx: &mut Context<Self>) -> Field {
        let confirming = self.confirm_large_deletions;

        field()
            .label(strings::dest_editor_delete_mode_label())
            .child(
                v_flex()
                    .gap(px(10.))
                    .child(
                        ButtonGroup::new("delete-mode")
                            .outline()
                            .child(
                                Button::new("delete-recycle")
                                    .label(strings::enum_delete_mode_recycle())
                                    .icon(Glyph::Recycle)
                                    .selected(self.delete_mode == DeleteMode::Recycle),
                            )
                            .child(
                                Button::new("delete-permanent")
                                    .label(strings::enum_delete_mode_permanent())
                                    .icon(Glyph::Trash)
                                    .selected(self.delete_mode == DeleteMode::Permanent),
                            )
                            .on_click(cx.listener(|editor, clicked: &Vec<usize>, _, cx| {
                                if let Some(index) = clicked.first() {
                                    editor.delete_mode = if *index == 1 {
                                        DeleteMode::Permanent
                                    } else {
                                        DeleteMode::Recycle
                                    };
                                    cx.notify();
                                }
                            })),
                    )
                    .child(
                        Checkbox::new("confirm-large")
                            .label(strings::dest_editor_confirm_large_deletions())
                            .checked(confirming)
                            .on_click(cx.listener(|editor, checked: &bool, _, cx| {
                                editor.confirm_large_deletions = *checked;
                                cx.notify();
                            })),
                    )
                    .when(confirming, |element| {
                        element.child(
                            h_flex()
                                .pl(px(24.))
                                .items_center()
                                .gap(px(8.))
                                .child(strings::dest_editor_ask_when_deleting_more_than())
                                .child(div().w(px(80.)).child(Input::new(&self.threshold)))
                                .child(strings::dest_editor_percent_of_destination()),
                        )
                    }),
            )
    }
}

/// A label for a control nested under a form field's own label.
fn sub_label(text: impl Into<SharedString>, cx: &App) -> impl IntoElement {
    div()
        .text_sm()
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
}
