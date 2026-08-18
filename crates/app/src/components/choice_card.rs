//! A radio option big enough to explain itself.
//!
//! Used where the choice is consequential and the names alone are not enough — Sync versus Move,
//! Mirror versus Add-only. Each card carries a sentence saying what it will do to the user's
//! files, because that is the decision being made.
//!
//! Deliberately not a `gpui_component::radio::RadioGroup`: `Radio::label` is one line, and
//! folding "Mirror deletes anything the source does not have" into a label or hiding it in a
//! tooltip trades away the sentence that stops the mistake. Everything about its *appearance*
//! comes from the theme, so it follows the palette like the built-in components do — the only
//! thing bespoke here is the shape.

use gpui::{
    div, prelude::*, px, App, ClickEvent, ElementId, FontWeight, IntoElement, RenderOnce,
    SharedString, Window,
};
use gpui_component::{ActiveTheme as _, Icon};

use crate::components::Glyph;

/// What a card stores for its click. Named because the bare type is unreadable.
type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// See the module docs.
#[derive(IntoElement)]
pub struct ChoiceCard {
    id: ElementId,
    glyph: Glyph,
    title: SharedString,
    description: SharedString,
    selected: bool,
    disabled: bool,
    on_click: Option<ClickHandler>,
}

impl ChoiceCard {
    pub fn new(
        id: impl Into<ElementId>,
        glyph: Glyph,
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
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for ChoiceCard {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let (background, border) = if self.selected {
            (cx.theme().list_active, cx.theme().primary)
        } else {
            (cx.theme().background, cx.theme().border)
        };

        let mut element = div()
            .id(self.id)
            .flex()
            .flex_row()
            .items_start()
            .px(px(12.))
            .py(px(10.))
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(border)
            .bg(background)
            .child(
                div().mr(px(11.)).child(
                    Icon::new(self.glyph)
                        .size(px(20.))
                        .text_color(cx.theme().primary),
                ),
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
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
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
