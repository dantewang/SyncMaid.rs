//! A checkbox with its label, and room for the sentence underneath that explains it.

use gpui::{div, prelude::*, px, App, ElementId, IntoElement, RenderOnce, SharedString, Window};

use crate::components::{icon, ClickHandler, Icon};
use crate::theme;

/// See the module docs.
#[derive(IntoElement)]
pub struct Checkbox {
    id: ElementId,
    label: SharedString,
    description: Option<SharedString>,
    checked: bool,
    disabled: bool,
    on_toggle: Option<ClickHandler>,
}

impl Checkbox {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>, checked: bool) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            description: None,
            checked,
            disabled: false,
            on_toggle: None,
        }
    }

    /// The sentence under the label, where the consequence of the setting belongs.
    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_toggle(
        mut self,
        handler: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_toggle = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for Checkbox {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let (background, border) = if self.checked {
            (theme::TEAL, theme::TEAL)
        } else {
            (theme::SURFACE, theme::HAIRLINE_STRONG)
        };

        let mut element = div()
            .id(self.id)
            .flex()
            .flex_row()
            .items_start()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(16.))
                    .mt(px(2.))
                    .rounded(px(4.))
                    .border_1()
                    .border_color(theme::color(border))
                    .bg(theme::color(background))
                    .when(self.checked, |box_| {
                        box_.child(icon(Icon::Check, px(12.), theme::color(theme::SURFACE)))
                    }),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .child(self.label)
                    .when_some(self.description, |element, description| {
                        element.child(
                            div()
                                .pt(px(2.))
                                .text_size(theme::text::small())
                                .text_color(theme::color(theme::TEXT_SECONDARY))
                                .child(description),
                        )
                    }),
            );

        if self.disabled {
            element.opacity(0.45)
        } else {
            element = element.cursor_pointer();
            if let Some(handler) = self.on_toggle {
                element = element.on_click(handler);
            }
            element
        }
    }
}
