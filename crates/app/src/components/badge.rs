//! The small pills on a task card: what kind it is, what triggers it, when it next runs.

use gpui::{div, prelude::*, px, IntoElement, RenderOnce, SharedString, Window};

use crate::components::{icon, Icon};
use crate::theme;

/// What a badge is saying.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BadgeTone {
    /// Plain fact: the task's kind, its trigger.
    #[default]
    Quiet,
    /// Something is coming: the next scheduled run.
    Live,
    /// Something needs attention: a trigger that stopped working.
    Warn,
}

/// A pill with an optional leading glyph.
#[derive(IntoElement)]
pub struct Badge {
    label: SharedString,
    glyph: Option<Icon>,
    tone: BadgeTone,
}

impl Badge {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            glyph: None,
            tone: BadgeTone::Quiet,
        }
    }

    pub fn glyph(mut self, glyph: Icon) -> Self {
        self.glyph = Some(glyph);
        self
    }

    pub fn tone(mut self, tone: BadgeTone) -> Self {
        self.tone = tone;
        self
    }

    /// The text this badge will show, for tests that assert on wording.
    pub fn label(&self) -> &str {
        &self.label
    }
}

impl RenderOnce for Badge {
    fn render(self, _window: &mut Window, _cx: &mut gpui::App) -> impl IntoElement {
        let (background, foreground) = match self.tone {
            BadgeTone::Quiet => (theme::SUBTLE, theme::TEXT_SECONDARY),
            BadgeTone::Live => (theme::TEAL_SUBTLE, theme::TEAL),
            BadgeTone::Warn => (theme::WARNING_SUBTLE, theme::WARNING),
        };

        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(4.))
            .px(px(8.))
            .py(px(2.))
            .rounded(theme::radius::pill())
            .bg(theme::color(background))
            .text_size(theme::text::tiny())
            .text_color(theme::color(foreground))
            .when_some(self.glyph, |element, glyph| {
                element.child(icon(glyph, px(12.), theme::color(foreground)))
            })
            .child(self.label)
    }
}
