//! Adding and editing a destination: where files go, which ones, and how they are reconciled.

use std::path::Path;

use gpui::{
    div, prelude::*, px, Context, Entity, EventEmitter, PathPromptOptions, SharedString, Window,
};
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
use crate::views::dialogs::{dialog_card, dialog_footer, dialog_title, field_label};

/// What the editor decided.
pub enum DestinationEditorEvent {
    Saved(Box<Destination>),
    Cancelled,
}

/// See the module docs.
pub struct DestinationEditor {
    destination_id: Uuid,
    /// A Move task's destinations are routing rules and are always Move; the choice is hidden.
    task_kind: SyncTaskKind,
    source_path: String,

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
    pub fn new_destination(
        task: &SyncTask,
        tasks: Vec<SyncTask>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let strategy = match task.kind() {
            SyncTaskKind::Move => SyncStrategy::Move,
            // Add-only is the safe default for anyone unsure: it never deletes.
            SyncTaskKind::Sync => SyncStrategy::AddOnly,
        };
        // A Sync destination takes everything unless told otherwise; a Move rule must say what
        // it routes, and the catch-all is an explicit choice rather than a silent default.
        let filters = match task.kind() {
            SyncTaskKind::Sync => vec![syncmaid_core::filtering::FilterRule::AllFiles],
            SyncTaskKind::Move => Vec::new(),
        };
        let blank = Destination::new("", "", filters, strategy);
        Self::build(task, &blank, tasks, window, cx)
    }

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
            task_kind: task.kind(),
            source_path: task.source_path.clone(),
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

    fn path_text(&self, cx: &Context<Self>) -> String {
        self.path.read(cx).value().trim().to_owned()
    }

    /// The name to save: what the user typed, or the folder's own name.
    fn resolved_name(&self, cx: &Context<Self>) -> String {
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

    fn threshold_fraction(&self, cx: &Context<Self>) -> f64 {
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
    /// filtered subset contradicts that by definition.
    fn shows_filters(&self) -> bool {
        self.strategy != SyncStrategy::Mirror
    }

    /// Why the save is blocked, if it is.
    fn blocked_reason(&self, cx: &Context<Self>) -> Option<String> {
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

        if let Some(conflict) =
            destination_conflict(&self.tasks, None, Some(self.destination_id), &path)
        {
            return Some(format!(
                "This folder overlaps a destination of task \"{}\" — destinations never overlap.",
                conflict.task_name
            ));
        }

        if self.shows_filters() && self.filters.selects_nothing() {
            return Some("No rules yet — this destination would sync nothing.".into());
        }

        None
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        if self.blocked_reason(cx).is_some() {
            return;
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

        cx.emit(DestinationEditorEvent::Saved(Box::new(destination)));
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

impl EventEmitter<DestinationEditorEvent> for DestinationEditor {}

impl Render for DestinationEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let blocked = self.blocked_reason(cx);
        let path = self.path_text(cx);
        let network = !path.is_empty() && is_network(Path::new(&path));

        // Never taller than the window it sits in: the body scrolls, the title and the footer
        // stay put, so Save is always reachable.
        let available = window.viewport_size().height - px(64.);

        dialog_card(px(560.))
            .max_h(available)
            .child(dialog_title(if self.task_kind == SyncTaskKind::Move {
                "Routing rule"
            } else {
                "Destination"
            }))
            .child(
                div()
                    .id("destination-body")
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
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
                    .child(self.render_verification(network, cx))
                    .when(self.strategy == SyncStrategy::Mirror, |element| {
                        element.child(self.render_deletions(cx))
                    }),
            )
            .when_some(blocked.clone(), |element, reason| {
                element.child(HintBox::new(reason).tone(HintTone::Danger))
            })
            .child(
                dialog_footer()
                    .child(
                        Button::new("destination-cancel", "Cancel")
                            .tone(ButtonTone::Secondary)
                            .on_click(cx.listener(|_, _, _, cx| {
                                cx.emit(DestinationEditorEvent::Cancelled)
                            })),
                    )
                    .child(
                        Button::new("destination-save", "Save")
                            .disabled(blocked.is_some())
                            .on_click(cx.listener(|editor, _, _, cx| editor.save(cx))),
                    ),
            )
    }
}

impl DestinationEditor {
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
