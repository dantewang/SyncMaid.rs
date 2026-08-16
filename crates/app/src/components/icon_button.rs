//! The square glyph buttons on task cards and destination rows.

use gpui::{
    div, prelude::*, px, App, ClickEvent, ElementId, IntoElement, Pixels, RenderOnce, SharedString,
    Window, WindowControlArea,
};

use crate::components::{icon, ClickHandler, Icon};
use crate::theme;

/// What the button does, which is what decides its colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IconButtonTone {
    /// Edit, add, browse — the everyday ones.
    #[default]
    Neutral,
    /// Run now.
    Run,
    /// Delete, stop.
    Danger,
    /// A title-bar caption button: square, borderless, full-height.
    Caption,
    /// The close caption button, which turns red under the pointer.
    CaptionClose,
}

/// See the module docs.
#[derive(IntoElement)]
pub struct IconButton {
    id: ElementId,
    glyph: Icon,
    tone: IconButtonTone,
    size: Pixels,
    glyph_size: Pixels,
    disabled: bool,
    tooltip: Option<SharedString>,
    window_control: Option<WindowControlArea>,
    on_click: Option<ClickHandler>,
}

impl IconButton {
    pub fn new(id: impl Into<ElementId>, glyph: Icon) -> Self {
        Self {
            id: id.into(),
            glyph,
            tone: IconButtonTone::Neutral,
            size: theme::layout::icon_button(),
            glyph_size: px(16.),
            disabled: false,
            tooltip: None,
            window_control: None,
            on_click: None,
        }
    }

    /// Hands the button to the OS as a window control.
    ///
    /// This is what keeps the native behaviours a hand-drawn title bar would otherwise lose —
    /// most visibly Windows 11's snap-layout flyout when the pointer rests on maximize. The OS
    /// also performs the action, so these need no click handler.
    pub fn window_control(mut self, area: WindowControlArea) -> Self {
        self.window_control = Some(area);
        self
    }

    pub fn tone(mut self, tone: IconButtonTone) -> Self {
        self.tone = tone;
        self
    }

    /// The 26 px variant used inside rows and the sidebar header.
    pub fn small(mut self) -> Self {
        self.size = theme::layout::small_icon_button();
        self.glyph_size = px(14.);
        self
    }

    pub fn glyph_size(mut self, size: Pixels) -> Self {
        self.glyph_size = size;
        self
    }

    /// Disabled buttons stay visible and keep their tooltip: the tooltip is usually the reason
    /// they are disabled.
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

impl RenderOnce for IconButton {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let caption = matches!(
            self.tone,
            IconButtonTone::Caption | IconButtonTone::CaptionClose
        );
        let (border, glyph_color, hover) = match self.tone {
            IconButtonTone::Neutral => (theme::HAIRLINE, theme::TEXT_SECONDARY, theme::SUBTLE),
            IconButtonTone::Run => (theme::TEAL, theme::TEAL, theme::TEAL_SUBTLE),
            IconButtonTone::Danger => (theme::DANGER, theme::DANGER, theme::DANGER_SUBTLE),
            IconButtonTone::Caption => (theme::PAGE, theme::TEXT_SECONDARY, theme::SUBTLE),
            IconButtonTone::CaptionClose => (theme::PAGE, theme::TEXT_SECONDARY, theme::DANGER),
        };
        // The close button is the one place a glyph inverts, because its hover fill is solid red.
        let hover_glyph = if self.tone == IconButtonTone::CaptionClose {
            theme::SURFACE
        } else {
            glyph_color
        };

        let (width, height) = if caption {
            theme::layout::caption_button()
        } else {
            (self.size, self.size)
        };

        let mut element = div()
            .id(self.id)
            .flex()
            .items_center()
            .justify_center()
            .w(width)
            .h(height)
            .child(icon(self.glyph, self.glyph_size, theme::color(glyph_color)));

        if caption {
            // Square, borderless and flush with the window edge, so a pointer slammed into the
            // corner still lands on Close.
            element = element.hover(|style| {
                style
                    .bg(theme::color(hover))
                    .text_color(theme::color(hover_glyph))
            });
        } else {
            element = element
                .rounded(theme::radius::control())
                .border_1()
                .border_color(theme::color(border))
                .hover(|style| style.bg(theme::color(hover)));
        }

        if let Some(area) = self.window_control {
            element = element.window_control_area(area);
        }

        if self.disabled {
            element = element.opacity(0.4);
        } else {
            element = element.cursor_pointer();
            if let Some(handler) = self.on_click {
                element = element.on_click(handler);
            }
        }

        element
    }
}
