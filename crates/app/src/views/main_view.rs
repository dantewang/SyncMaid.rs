//! The main window: title bar, task sidebar, task list.

use gpui::{
    div, img, prelude::*, px, Context, FontWeight, SharedString, Window, WindowControlArea,
};
use syncmaid_core::model::{
    Destination, DestinationSyncStatus, SyncOutcome, SyncStrategy, SyncTask, SyncTaskKind,
};
use syncmaid_core::triggers::Trigger;

use crate::components::{
    icon, Badge, Button, ButtonTone, HintBox, HintTone, Icon, IconButton, IconButtonTone,
};
use crate::state::{health_of, Workspace};
use crate::theme;

/// The main window's content.
pub struct MainView {
    workspace: Workspace,
}

impl MainView {
    pub fn new(workspace: Workspace) -> Self {
        Self { workspace }
    }
}

impl Render for MainView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Pinned to the viewport in *layout* units.
        //
        // `viewport_size` reports device pixels while everything laid out here is logical, so
        // on a scaled display the difference is the scale factor — and inheriting it through
        // `size_full` makes every row that much too wide. Nothing looks broken at first: the
        // backgrounds still fill the window. What goes is each row's last children, because a
        // `flex_1` sibling grows into the surplus and pushes them past the edge — which is to
        // say every button on the right-hand side of the app.
        let viewport = window.viewport_size() / window.scale_factor();

        div()
            .flex()
            .flex_col()
            .w(viewport.width)
            .h(viewport.height)
            .bg(theme::color(theme::PAGE))
            .text_size(theme::text::body())
            .text_color(theme::color(theme::TEXT_PRIMARY))
            .child(self.render_title_bar())
            .child(
                div()
                    .flex()
                    .flex_row()
                    .w_full()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .child(self.render_sidebar(cx))
                    .child(self.render_main_pane(cx)),
            )
    }
}

impl MainView {
    /// 40 px, app-drawn, with the OS still owning drag and the three window controls.
    fn render_title_bar(&self) -> impl IntoElement {
        div()
            .flex()
            .flex_row()
            .items_center()
            .w_full()
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
            .child(IconButton::new("settings", Icon::CogOutline).tone(IconButtonTone::Caption))
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
            .child(Button::new("new-task", "New task").glyph(Icon::Plus))
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
                    destination,
                    self.workspace.statuses().get(&destination.id),
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
                                    } else {
                                        IconButton::new(
                                            SharedString::from(format!("run-{id}")),
                                            Icon::Play,
                                        )
                                        .tone(IconButtonTone::Run)
                                        .disabled(task.destinations.is_empty())
                                    })
                                    .child(IconButton::new(
                                        SharedString::from(format!("add-{id}")),
                                        Icon::Plus,
                                    ))
                                    .child(
                                        IconButton::new(
                                            SharedString::from(format!("edit-{id}")),
                                            Icon::Pencil,
                                        )
                                        .glyph_size(px(15.)),
                                    )
                                    .child(
                                        IconButton::new(
                                            SharedString::from(format!("delete-{id}")),
                                            Icon::TrashCanOutline,
                                        )
                                        .glyph_size(px(15.)),
                                    ),
                            ),
                    ),
            )
            .when(expanded, |element| element.children(rows))
    }

    fn render_destination_row(
        &self,
        destination: &Destination,
        status: Option<&DestinationSyncStatus>,
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
                    .child(status_text(status)),
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
                        .small(),
                    )
                    .child(
                        IconButton::new(
                            SharedString::from(format!("delete-dest-{id}")),
                            Icon::TrashCanOutline,
                        )
                        .small(),
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
