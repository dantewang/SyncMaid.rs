//! Adding and editing a destination: where files go, which ones, and how they are reconciled.
//!
//! It is never a dialog of its own. Every destination edit goes through the task workspace,
//! because which rule catches a file is a property of the whole ordered list — so the editor
//! draws its fields and nothing else, and the row it opens inside owns the frame, the
//! accept/discard pair, and what happens to the result.

use std::path::Path;

use gpui::{div, prelude::*, px, App, Context, Entity, PathPromptOptions, SharedString, Window};
use gpui_component::input::{Input, InputState};
use syncmaid_core::io::{is_network, paths_overlap};
use syncmaid_core::model::{
    DeleteMode, Destination, FileNameCollisionPolicy, SyncStrategy, SyncTask, SyncTaskKind,
    DEFAULT_MASS_DELETE_THRESHOLD,
};
use uuid::Uuid;

use crate::components::{
    icon, Button, ButtonTone, Checkbox, ChoiceCard, HintBox, HintTone, Icon, IconButton,
    IconButtonTone, Segment, SegmentOption,
};
use crate::state::{destination_conflict, FilterEntry, FilterGroup, FilterKind, FilterModel};
use crate::theme;
use crate::views::dialogs::field_label;

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
                    .placeholder("Optional — defaults to the folder's name")
            }),
            path: cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(destination.local_path().to_owned())
                    .placeholder("Where the files go")
            }),
            threshold: cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(percent(destination.mass_delete_threshold).to_string())
            }),
            group_inputs: (0..groups)
                .map(|_| {
                    cx.new(|cx| {
                        InputState::new(window, cx)
                            .placeholder("e.g. photos/2024, jpg, or **/ChatGPT*.png")
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
            return Some("Choose a destination folder.".into());
        }

        // Task shape: a destination never sits inside its own source, and never contains it.
        if paths_overlap(Path::new(&path), Path::new(&self.source_path)) {
            return Some(
                "Destination must be a separate folder outside the source (and not contain it)."
                    .into(),
            );
        }

        if let Some(conflict) = self.overlapping_destination(&path) {
            return Some(conflict);
        }

        if self.shows_filters() && self.filters.selects_nothing() {
            return Some("No rules yet — this destination would sync nothing.".into());
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
            return Some(format!(
                "This folder overlaps \"{}\" in this task — destinations never overlap.",
                sibling.name
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
            format!(
                "This folder overlaps a destination of task \"{}\" — destinations never overlap.",
                conflict.task_name
            )
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
            prompt: Some("Choose the destination folder".into()),
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
            InputState::new(window, cx).placeholder("e.g. photos/2024, jpg, or **/ChatGPT*.png")
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

fn percent(fraction: f64) -> u32 {
    (fraction * 100.0).round() as u32
}

impl Render for DestinationEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // No card, no title, no footer: the workspace row it opens inside is the frame, and its
        // accept/discard pair sits up beside the summary line rather than below the fold.
        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(self.render_fields(cx))
            .when_some(self.blocked_reason(cx), |element, reason| {
                element.child(HintBox::new(reason).tone(HintTone::Danger))
            })
    }
}

impl DestinationEditor {
    /// Everything the editor edits, in one column.
    fn render_fields(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let path = self.path_text(cx);
        let network = !path.is_empty() && is_network(Path::new(&path));

        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(
                div()
                    .child(field_label("Name"))
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
                element.child(HintBox::new(
                    "This rule takes everything the rules above it left, so it has no file \
                     selection of its own.",
                ))
            })
            .child(self.render_verification(network, cx))
            .when(self.strategy == SyncStrategy::Mirror, |element| {
                element.child(self.render_deletions(cx))
            })
    }

    fn render_folder(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .child(field_label("Destination folder"))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap(px(8.))
                    .child(div().flex_1().min_w_0().child(Input::new(&self.path)))
                    .child(
                        Button::new("browse-destination", "Browse")
                            .tone(ButtonTone::Secondary)
                            .glyph(Icon::FolderOutline)
                            .on_click(
                                cx.listener(|editor, _, window, cx| editor.browse(window, cx)),
                            ),
                    ),
            )
            .child(div().pt(px(6.)).child(HintBox::new(
                "It doesn't have to exist yet — SyncMaid creates it on the first run.",
            )))
    }

    fn render_strategy(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let strategy = self.strategy;
        div().child(field_label("Sync strategy")).child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(
                    ChoiceCard::new(
                        "strategy-mirror",
                        Icon::Sync,
                        "Mirror",
                        "Make the destination identical to the source, empty folders \
                             included. Extras are deleted.",
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
                        Icon::Plus,
                        "Add-only",
                        "Copy new and changed files and never delete anything. The safe \
                             accumulator.",
                    )
                    .selected(strategy == SyncStrategy::AddOnly)
                    .on_click(cx.listener(|editor, _, _, cx| {
                        editor.strategy = SyncStrategy::AddOnly;
                        cx.notify();
                    })),
                ),
        )
    }

    fn render_move_options(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let flatten = self.flatten;
        let collision = self.collision;

        div()
            .child(field_label("Where files land"))
            .child(
                Segment::new(
                    "move-layout",
                    vec![
                        SegmentOption::new("Keep structure").glyph(Icon::FileTree),
                        SegmentOption::new("Put them all in the folder").glyph(Icon::FolderOutline),
                    ],
                    usize::from(flatten),
                )
                .on_select(cx.listener(|editor, index: &usize, _, cx| {
                    editor.flatten = *index == 1;
                    cx.notify();
                })),
            )
            .when(flatten, |element| {
                element.child(
                    div()
                        .pt(px(10.))
                        .child(field_label("If that name is already taken"))
                        .child(
                            Segment::new(
                                "collision",
                                vec![
                                    SegmentOption::new("Leave it in the source")
                                        .glyph(Icon::MinusCircle),
                                    SegmentOption::new("Add a number").glyph(Icon::ContentCopy),
                                ],
                                usize::from(collision == FileNameCollisionPolicy::Suffix),
                            )
                            .on_select(cx.listener(
                                |editor, index: &usize, _, cx| {
                                    editor.collision = if *index == 1 {
                                        FileNameCollisionPolicy::Suffix
                                    } else {
                                        FileNameCollisionPolicy::Skip
                                    };
                                    cx.notify();
                                },
                            )),
                        ),
                )
            })
    }

    fn render_filters(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let all_files = self.filters.all_files;
        let several_groups = self.filters.groups.len() > 1;
        let groups: Vec<_> = self
            .filters
            .groups
            .iter()
            .enumerate()
            .map(|(index, group)| self.render_group(index, group, several_groups, cx))
            .collect();

        div()
            .child(field_label("Files to sync"))
            .child(
                Segment::new(
                    "filter-mode",
                    vec![
                        SegmentOption::new("All files").glyph(Icon::Asterisk),
                        SegmentOption::new("Only matching").glyph(Icon::FilterOutline),
                    ],
                    usize::from(!all_files),
                )
                .on_select(cx.listener(|editor, index: &usize, _, cx| {
                    editor.filters.all_files = *index == 0;
                    if !editor.filters.all_files && editor.filters.groups.is_empty() {
                        editor.filters.groups.push(FilterGroup::default());
                    }
                    cx.notify();
                })),
            )
            .when(!all_files, |element| {
                element
                    .child(
                        div()
                            .pt(px(10.))
                            // Only worth asking once there is more than one group to combine.
                            .when(several_groups, |element| {
                                element.child(
                                    Segment::new(
                                        "group-combine",
                                        vec![
                                            SegmentOption::new("Match any group"),
                                            SegmentOption::new("Match all groups"),
                                        ],
                                        usize::from(self.filters.match_all_groups),
                                    )
                                    .small()
                                    .on_select(cx.listener(
                                        |editor, index: &usize, _, cx| {
                                            editor.filters.match_all_groups = *index == 1;
                                            cx.notify();
                                        },
                                    )),
                                )
                            })
                            .children(groups)
                            .child(
                                Button::new("add-group", "Add group")
                                    .tone(ButtonTone::Secondary)
                                    .glyph(Icon::Plus)
                                    .on_click(cx.listener(|editor, _, window, cx| {
                                        editor.add_group(window, cx)
                                    })),
                            ),
                    )
                    // The live plain-language line: the single best guard against a filter that
                    // quietly selects the wrong thing.
                    .child(
                        div()
                            .pt(px(10.))
                            .child(HintBox::new(self.filters.describe()).glyph(Icon::EyeOutline)),
                    )
            })
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

        div()
            .flex()
            .flex_col()
            .mt(px(8.))
            .p(px(10.))
            .rounded(theme::radius::block())
            .border_1()
            .border_color(theme::color(theme::HAIRLINE))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .pb(px(8.))
                    .child(
                        div()
                            .text_size(theme::text::small())
                            .text_color(theme::color(theme::TEXT_SECONDARY))
                            .child("Match"),
                    )
                    .child(
                        Segment::new(
                            SharedString::from(format!("group-{index}-mode")),
                            vec![
                                SegmentOption::new("any rule"),
                                SegmentOption::new("all rules"),
                            ],
                            usize::from(group.match_all),
                        )
                        .small()
                        .on_select(cx.listener(
                            move |editor, choice: &usize, _, cx| {
                                if let Some(group) = editor.filters.groups.get_mut(index) {
                                    group.match_all = *choice == 1;
                                }
                                cx.notify();
                            },
                        )),
                    )
                    .child(div().flex_1())
                    .when(removable, |element| {
                        element.child(
                            IconButton::new(
                                SharedString::from(format!("remove-group-{index}")),
                                Icon::Close,
                            )
                            .small()
                            .tone(IconButtonTone::Caption)
                            .on_click(
                                cx.listener(move |editor, _, _, cx| editor.remove_group(index, cx)),
                            ),
                        )
                    }),
            )
            .children(rules)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .pt(px(4.))
                    .child(
                        Segment::new(
                            SharedString::from(format!("group-{index}-kind")),
                            vec![
                                SegmentOption::new("Path"),
                                SegmentOption::new("Extension"),
                                SegmentOption::new("Wildcard"),
                            ],
                            kind.index(),
                        )
                        .small()
                        .on_select(cx.listener(
                            move |editor, choice: &usize, _, cx| {
                                if let Some(slot) = editor.group_kinds.get_mut(index) {
                                    *slot = FilterKind::from_index(*choice);
                                }
                                cx.notify();
                            },
                        )),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .children(self.group_inputs.get(index).map(Input::new)),
                    )
                    .child(
                        Button::new(SharedString::from(format!("add-rule-{index}")), "Add")
                            .glyph(Icon::Plus)
                            .on_click(cx.listener(move |editor, _, window, cx| {
                                editor.add_rule(index, window, cx)
                            })),
                    ),
            )
            .when(kind == FilterKind::Wildcard, |element| {
                element.child(
                    div().pt(px(8.)).child(
                        HintBox::new(
                            "* is any run of characters and ? is one, neither crossing a folder; \
                             ** spans any number of folders. A leading **/ is what makes a \
                             pattern apply at any depth.",
                        )
                        .glyph(Icon::AsteriskCircleOutline),
                    ),
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

        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .mb(px(6.))
            .px(px(10.))
            .py(px(6.))
            .rounded(theme::radius::control())
            .bg(theme::color(theme::SUBTLE))
            .child(icon(
                Icon::FilterOutline,
                px(15.),
                theme::color(theme::TEXT_SECONDARY),
            ))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(format!("{}: {}", rule.kind.label(), rule.pattern)),
            )
            .child(
                IconButton::new(
                    SharedString::from(format!("exclude-{group}-{index}")),
                    Icon::Cancel,
                )
                .small()
                .glyph_size(px(13.))
                .tone(if excluded {
                    IconButtonTone::Danger
                } else {
                    IconButtonTone::Neutral
                })
                .tooltip("Exclude what this matches instead of including it")
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
                IconButton::new(
                    SharedString::from(format!("remove-rule-{group}-{index}")),
                    Icon::Close,
                )
                .small()
                .glyph_size(px(13.))
                .on_click(cx.listener(move |editor, _, _, cx| {
                    if let Some(group) = editor.filters.groups.get_mut(group) {
                        group.rules.remove(index);
                    }
                    cx.notify();
                })),
            )
    }

    fn render_verification(&self, network: bool, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .child(field_label("Verification"))
            .child(
                Checkbox::new(
                    "verify-contents",
                    "Verify file contents (xxHash) after each copy",
                    self.verify_contents,
                )
                .description(
                    "Reads each copy back and compares it to the source, catching silent \
                     corruption a length check cannot see.",
                )
                .on_toggle(cx.listener(|editor, _, _, cx| {
                    editor.verify_contents = !editor.verify_contents;
                    cx.notify();
                })),
            )
            .when(network && self.verify_contents, |element| {
                element.child(div().pt(px(6.)).child(
                    HintBox::new(
                        "This is a network location. Content verification re-reads every copied \
                         file over the network (slower, more bandwidth). It guards against \
                         silent corruption the network protocol does not.",
                    )
                    .tone(HintTone::Danger),
                ))
            })
    }

    fn render_deletions(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let confirming = self.confirm_large_deletions;

        div()
            .child(field_label("When removing extra files"))
            .child(
                Segment::new(
                    "delete-mode",
                    vec![
                        SegmentOption::new("Recycle Bin").glyph(Icon::RecycleVariant),
                        SegmentOption::new("Delete permanently").glyph(Icon::DeleteForeverOutline),
                    ],
                    usize::from(self.delete_mode == DeleteMode::Permanent),
                )
                .on_select(cx.listener(|editor, index: &usize, _, cx| {
                    editor.delete_mode = if *index == 1 {
                        DeleteMode::Permanent
                    } else {
                        DeleteMode::Recycle
                    };
                    cx.notify();
                })),
            )
            .child(
                div().pt(px(10.)).child(
                    Checkbox::new(
                        "confirm-large",
                        "Confirm before a large deletion",
                        confirming,
                    )
                    .description(
                        "An empty or unavailable source never deletes anything, with or without \
                         this.",
                    )
                    .on_toggle(cx.listener(|editor, _, _, cx| {
                        editor.confirm_large_deletions = !editor.confirm_large_deletions;
                        cx.notify();
                    })),
                ),
            )
            .when(confirming, |element| {
                element.child(
                    div()
                        .pl(px(24.))
                        .pt(px(8.))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.))
                        .child("Ask when deleting more than")
                        .child(div().w(px(80.)).child(Input::new(&self.threshold)))
                        .child("% of the destination"),
                )
            })
    }
}
