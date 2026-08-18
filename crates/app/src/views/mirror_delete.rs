//! The mirror mass-delete confirmation.
//!
//! **A real top-level window, not a modal inside the main one.** A Mirror run can be started by
//! a trigger while SyncMaid is hidden in the tray, and a confirmation drawn inside a window
//! nobody can see is a run that silently never finishes. This one has its own frame, its own
//! taskbar entry, and it comes up centred on screen.
//!
//! Closing it by the title bar means **keep**: the destructive answer is only ever given by
//! pressing the destructive button. And an approval covers **this run only** — it is never
//! written to disk, so tomorrow's run asks again.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{
    div, prelude::*, px, size, App, Bounds, Context, FontWeight, TitlebarOptions, Window,
    WindowBounds, WindowKind, WindowOptions,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::{ActiveTheme as _, Icon, Root};
use syncmaid_core::model::{DeleteMode, Destination};
use syncmaid_core::sync::MirrorDeletePreview;

use crate::components::Glyph;
use crate::strings;

/// The window's own size. Fixed rather than sized to content: the sample list scrolls, so the
/// window is the same shape whether three files are going or three thousand.
const WINDOW_SIZE: (f32, f32) = (480., 460.);

/// What the user decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirrorDeleteDecision {
    /// Go ahead, for this run only.
    Delete,
    /// Leave them. Also what closing the window means.
    Keep,
}

/// The caller's callback, in the one cell that guarantees it runs at most once.
type Answer = Rc<RefCell<Option<Box<dyn FnOnce(MirrorDeleteDecision, &mut App)>>>>;

/// See the module docs.
pub struct ConfirmMirrorDelete {
    destination_name: String,
    destination_path: String,
    recycle: bool,
    count: usize,
    sample: Vec<String>,
    answer: Answer,
}

impl ConfirmMirrorDelete {
    fn new(destination: &Destination, preview: MirrorDeletePreview, answer: Answer) -> Self {
        Self {
            destination_name: destination.name.clone(),
            destination_path: destination.local_path().to_owned(),
            recycle: destination.delete_mode == DeleteMode::Recycle,
            count: preview.count,
            sample: preview.sample,
            answer,
        }
    }

    /// Answers, then closes. Answering is a one-shot: the window closing behind it must not
    /// count as a second, quieter answer.
    fn decide(
        &mut self,
        decision: MirrorDeleteDecision,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(answer) = self.answer.borrow_mut().take() {
            answer(decision, cx);
        }
        window.remove_window();
    }

    /// Why the run stopped, in the words of what is about to happen to the files.
    ///
    /// The two wordings are the difference between recoverable and not, and the wrong one here
    /// is a user consenting to something else entirely.
    fn explanation(&self) -> String {
        let count = self.count as i64;
        if self.recycle {
            strings::mirror_delete_explanation_recycle_format(&self.destination_name, count)
        } else {
            strings::mirror_delete_explanation_permanent_format(&self.destination_name, count)
        }
    }

    fn confirm_label(&self) -> String {
        if self.recycle {
            strings::mirror_delete_move_to_recycle_bin().to_owned()
        } else {
            // The count is named on the permanent one: it is the last chance to notice it.
            strings::mirror_delete_delete_count(self.count as i64)
        }
    }
}

impl Render for ConfirmMirrorDelete {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let hidden = self.count.saturating_sub(self.sample.len());
        let mono = cx.theme().mono_font_family.clone();

        div()
            .flex()
            .flex_col()
            .size_full()
            .gap(px(14.))
            .p(px(24.))
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        Icon::new(Glyph::Warning)
                            .size(px(22.))
                            .text_color(cx.theme().warning),
                    )
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(strings::mirror_delete_review_deletions()),
                    ),
            )
            .child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child(self.explanation()),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .font_family(mono.clone())
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(self.destination_path.clone()),
            )
            .child(
                div()
                    .id("mirror-delete-sample")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p(px(10.))
                    .rounded(cx.theme().radius)
                    .bg(cx.theme().muted)
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .font_family(mono)
                    // Deliberately not trimmed: inside this scrolling column `text_ellipsis`
                    // collapses the line to a few stray pixels (measured, with and without an
                    // explicit width). A long path wrapping onto a second line reads fine —
                    // and trimming would hide the file name, which is the half that matters.
                    .children(
                        self.sample
                            .iter()
                            .map(|path| div().py(px(1.)).child(path.clone())),
                    ),
            )
            .when(hidden > 0, |element| {
                element.child(
                    div()
                        .text_color(cx.theme().muted_foreground)
                        .child(strings::mirror_delete_more_format(hidden)),
                )
            })
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_end()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        Button::new("mirror-delete-keep")
                            .label(strings::mirror_delete_keep_them())
                            .outline()
                            .on_click(cx.listener(|view, _, window, cx| {
                                view.decide(MirrorDeleteDecision::Keep, window, cx)
                            })),
                    )
                    .child(
                        Button::new("mirror-delete-confirm")
                            .label(self.confirm_label())
                            .danger()
                            .on_click(cx.listener(|view, _, window, cx| {
                                view.decide(MirrorDeleteDecision::Delete, window, cx)
                            })),
                    ),
            )
    }
}

/// Opens the window and calls `decided` exactly once, with `Keep` if the user closes it.
///
/// Exactly once is the contract the caller depends on: it is holding a run that is waiting for
/// an answer, and two answers would start two runs.
pub fn ask(
    destination: &Destination,
    preview: MirrorDeletePreview,
    cx: &mut App,
    decided: impl FnOnce(MirrorDeleteDecision, &mut App) + 'static,
) {
    let bounds = Bounds::centered(None, size(px(WINDOW_SIZE.0), px(WINDOW_SIZE.1)), cx);
    let answer: Answer = Rc::new(RefCell::new(Some(Box::new(decided))));
    let destination = destination.clone();

    let opened = {
        let answer = Rc::clone(&answer);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                // Titled, so the taskbar entry and the Alt-Tab thumbnail say what this window
                // is. It can arrive with nothing else of SyncMaid's on screen.
                titlebar: Some(TitlebarOptions {
                    title: Some(strings::mirror_delete_window_title().into()),
                    appears_transparent: false,
                    traffic_light_position: None,
                }),
                // Fixed: the content is a fixed shape, and a resizable frame invites the user to
                // fight a layout that has nothing to give.
                is_resizable: false,
                is_minimizable: false,
                kind: WindowKind::Normal,
                ..Default::default()
            },
            // Wrapped in `Root` like the main window: the library's components expect its
            // layers to exist, and a bare view here would be a second, subtly different
            // rendering environment.
            move |window, cx| {
                let view = cx.new(|_| ConfirmMirrorDelete::new(&destination, preview, answer));
                let any: gpui::AnyView = view.into();
                cx.new(|cx| Root::new(any, window, cx))
            },
        )
    };

    let Ok(window) = opened else {
        // No window means no way to ask, and "could not ask" must never read as "yes".
        tracing::error!("could not open the mirror-delete confirmation; keeping the files");
        if let Some(answer) = answer.borrow_mut().take() {
            answer(MirrorDeleteDecision::Keep, cx);
        }
        return;
    };

    let _ = window.update(cx, |_, window, cx| {
        window.on_window_should_close(cx, move |_, cx| {
            // Closing by the title bar is "keep": the destructive answer is only ever given by
            // pressing the destructive button.
            if let Some(answer) = answer.borrow_mut().take() {
                answer(MirrorDeleteDecision::Keep, cx);
            }
            true
        });
    });
}
#[cfg(test)]
mod tests {
    use syncmaid_core::filtering::FilterRule;
    use syncmaid_core::model::SyncStrategy;

    use super::*;

    fn destination(delete_mode: DeleteMode) -> Destination {
        let mut destination = Destination::new(
            "NAS backup",
            r"N:\photos",
            [FilterRule::AllFiles],
            SyncStrategy::Mirror,
        );
        destination.delete_mode = delete_mode;
        destination
    }

    fn dialog(delete_mode: DeleteMode, count: usize, sample: Vec<String>) -> ConfirmMirrorDelete {
        ConfirmMirrorDelete::new(
            &destination(delete_mode),
            MirrorDeletePreview::new(count, sample),
            Rc::new(RefCell::new(None)),
        )
    }

    #[test]
    fn the_button_says_what_will_actually_happen_to_the_files() {
        assert_eq!(
            "Move to Recycle Bin",
            dialog(DeleteMode::Recycle, 128, vec![]).confirm_label()
        );
        assert_eq!(
            "Delete 128 files",
            dialog(DeleteMode::Permanent, 128, vec![]).confirm_label(),
            "a permanent delete has to name the number: it is the last chance to notice it"
        );
        assert_eq!(
            "Delete 1 file",
            dialog(DeleteMode::Permanent, 1, vec![]).confirm_label()
        );
    }

    #[test]
    fn the_explanation_names_the_destination_and_the_count() {
        let explanation = dialog(DeleteMode::Recycle, 128, vec![]).explanation();

        assert!(explanation.contains("NAS backup"), "{explanation}");
        assert!(explanation.contains("128 files"), "{explanation}");
        assert!(explanation.contains("Recycle Bin"), "{explanation}");
    }

    #[test]
    fn a_permanent_delete_never_mentions_the_recycle_bin() {
        // The two wordings are the difference between recoverable and not, and a wrong one
        // here is a user consenting to something else entirely.
        let explanation = dialog(DeleteMode::Permanent, 4, vec![]).explanation();

        assert!(explanation.contains("permanently delete"), "{explanation}");
        assert!(!explanation.contains("Recycle"), "{explanation}");
    }
}
