//! A radio option big enough to explain itself.
//!
//! Used where the choice is consequential and the names alone are not enough — Sync versus
//! Move, Mirror versus Add-only. Each card carries a sentence saying what it will do to the
//! user's files, because that is the decision being made.

use gpui::{
    div, prelude::*, px, App, ElementId, FontWeight, IntoElement, RenderOnce, SharedString, Window,
};

use crate::components::{icon, ClickHandler, Icon};
use crate::theme;

/// See the module docs.
#[derive(IntoElement)]
pub struct ChoiceCard {
    id: ElementId,
    glyph: Icon,
    title: SharedString,
    description: SharedString,
    selected: bool,
    disabled: bool,
    on_click: Option<ClickHandler>,
}

impl ChoiceCard {
    pub fn new(
        id: impl Into<ElementId>,
        glyph: Icon,
        title: impl Into<SharedString>,
        description: impl Into<SharedString>,
    ) -> Self {
        Self {
            id: id.into(),
            glyph,
            title: title.into(),
            description: description.into(),
            selected: false,
            disabled: false,
            on_click: None,
        }
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// A locked choice stays legible rather than disappearing: the user needs to see which one
    /// the task already is, and the hint beside it says why it cannot change.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for ChoiceCard {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let (background, border) = if self.selected {
            (theme::TEAL_SUBTLE, theme::TEAL)
        } else {
            (theme::SURFACE, theme::HAIRLINE_STRONG)
        };

        let mut element = div()
            .id(self.id)
            .flex()
            .flex_row()
            .items_start()
            .px(px(12.))
            .py(px(10.))
            .rounded(theme::radius::block())
            .border_1()
            .border_color(theme::color(border))
            .bg(theme::color(background))
            .child(
                div()
                    .mr(px(11.))
                    .child(icon(self.glyph, px(20.), theme::color(theme::TEAL))),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .gap(px(2.))
                    .child(div().font_weight(FontWeight::SEMIBOLD).child(self.title))
                    .child(
                        div()
                            .text_size(theme::text::small())
                            .text_color(theme::color(theme::TEXT_SECONDARY))
                            .child(self.description),
                    ),
            );

        if self.disabled {
            element.opacity(0.45)
        } else {
            element = element.cursor_pointer();
            if let Some(handler) = self.on_click {
                element = element.on_click(handler);
            }
            element
        }
    }
}
