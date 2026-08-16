//! The advisory notice under an input.
//!
//! Severity tints the **icon only**; the text stays muted. A hint that shouted would compete
//! with the field it belongs to, and most of these are advice rather than errors — "this folder
//! doesn't exist yet" does not block saving.

use gpui::{div, prelude::*, px, IntoElement, RenderOnce, SharedString, Window};

use crate::components::{icon, Icon};
use crate::theme;

/// How loud the hint is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HintTone {
    /// Neutral guidance — a syntax reminder, a live preview of what a filter selects.
    #[default]
    Neutral,
    /// Something to look at before saving.
    Warning,
    /// Something that will stop the save, or a real risk.
    Danger,
}

/// See the module docs.
#[derive(IntoElement)]
pub struct HintBox {
    text: SharedString,
    glyph: Icon,
    tone: HintTone,
}

impl HintBox {
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            glyph: Icon::AlertOutline,
            tone: HintTone::Neutral,
        }
    }

    pub fn glyph(mut self, glyph: Icon) -> Self {
        self.glyph = glyph;
        self
    }

    pub fn tone(mut self, tone: HintTone) -> Self {
        self.tone = tone;
        self
    }
}

impl RenderOnce for HintBox {
    fn render(self, _window: &mut Window, _cx: &mut gpui::App) -> impl IntoElement {
        let glyph_color = match self.tone {
            HintTone::Neutral => theme::TEXT_MUTED,
            HintTone::Warning => theme::WARNING,
            HintTone::Danger => theme::DANGER,
        };

        div()
            .flex()
            .flex_row()
            .items_start()
            .gap(px(8.))
            .px(px(10.))
            .py(px(6.))
            .rounded(theme::radius::control())
            .bg(theme::color(theme::SUBTLE))
            .child(icon(self.glyph, px(16.), theme::color(glyph_color)))
            .child(
                div()
                    .flex_1()
                    .text_size(theme::text::small())
                    // The text stays muted whatever the severity — only the icon carries it.
                    .text_color(theme::color(theme::TEXT_SECONDARY))
                    .child(self.text),
            )
    }
}
