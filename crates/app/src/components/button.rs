//! The text buttons: one filled accent, one outlined, one filled red.

use gpui::{
    div, prelude::*, px, App, ClickEvent, ElementId, FontWeight, IntoElement, RenderOnce,
    SharedString, Window,
};

use gpui_component::tooltip::Tooltip;

use crate::components::{icon, ClickHandler, Icon};
use crate::theme;

/// What the button is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonTone {
    /// The one thing this view is for — New task, Save, Done.
    #[default]
    Primary,
    /// Everything else — Cancel, Browse, Run all.
    Secondary,
    /// Something that removes files.
    Danger,
}

/// See the module docs.
#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    label: SharedString,
    glyph: Option<Icon>,
    tone: ButtonTone,
    disabled: bool,
    /// The long form, for a button whose label cannot say the whole thing.
    tooltip: Option<SharedString>,
    on_click: Option<ClickHandler>,
}

impl Button {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            glyph: None,
            tone: ButtonTone::Primary,
            disabled: false,
            tooltip: None,
            on_click: None,
        }
    }

    pub fn glyph(mut self, glyph: Icon) -> Self {
        self.glyph = Some(glyph);
        self
    }

    pub fn tone(mut self, tone: ButtonTone) -> Self {
        self.tone = tone;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.tooltip = Some(tooltip.into());
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

impl RenderOnce for Button {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let filled = self.tone != ButtonTone::Secondary;
        let (background, foreground, hover) = match self.tone {
            ButtonTone::Primary => (theme::TEAL, theme::SURFACE, theme::TEAL_HOVER),
            ButtonTone::Secondary => (theme::PAGE, theme::TEXT_PRIMARY, theme::SUBTLE),
            ButtonTone::Danger => (theme::DANGER, theme::SURFACE, theme::DANGER_HOVER),
        };

        let mut element = div()
            .id(self.id)
            .flex()
            .flex_row()
            .items_center()
            .justify_center()
            .gap(px(6.))
            .rounded(theme::radius::control())
            .text_size(theme::text::body())
            .text_color(theme::color(foreground));

        if filled {
            element = element
                .px(px(13.))
                .py(px(7.))
                .bg(theme::color(background))
                .font_weight(FontWeight::MEDIUM);
        } else {
            element = element
                .px(px(12.))
                .py(px(6.))
                .border_1()
                .border_color(theme::color(theme::HAIRLINE_STRONG));
        }

        element = element
            .when_some(self.glyph, |element, glyph| {
                element.child(icon(glyph, px(15.), theme::color(foreground)))
            })
            .child(self.label);

        // Before the disabled branch: a disabled button keeps its tooltip, because the tooltip
        // is often the reason it is disabled.
        if let Some(tooltip) = self.tooltip {
            element =
                element.tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx));
        }

        if self.disabled {
            element.opacity(0.4)
        } else {
            element = element
                .cursor_pointer()
                .hover(|style| style.bg(theme::color(hover)));
            if let Some(handler) = self.on_click {
                element = element.on_click(handler);
            }
            element
        }
    }
}
