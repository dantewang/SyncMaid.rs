use gpui_kit::{
    div, prelude::*, px, App, ElementId, FontWeight, IntoElement, RenderOnce, SharedString, Window,
};

use super::{h_flex, v_flex, Icon, Size};
use crate::components::Glyph;
use crate::theme::ActiveTheme as _;

#[derive(Clone, Copy)]
enum Tone {
    Default,
    Warning,
    Error,
}

/// A boxed sentence with an icon: a hint, a warning, or why something is refused.
#[derive(IntoElement)]
pub struct Alert {
    id: ElementId,
    tone: Tone,
    glyph: Glyph,
    title: Option<SharedString>,
    message: SharedString,
    size: Size,
}

impl Alert {
    pub fn new(id: impl Into<ElementId>, message: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            tone: Tone::Default,
            glyph: Glyph::Info,
            title: None,
            message: message.into(),
            size: Size::Medium,
        }
    }

    pub fn warning(id: impl Into<ElementId>, message: impl Into<SharedString>) -> Self {
        Self {
            tone: Tone::Warning,
            glyph: Glyph::Warning,
            ..Self::new(id, message)
        }
    }

    pub fn error(id: impl Into<ElementId>, message: impl Into<SharedString>) -> Self {
        Self {
            tone: Tone::Error,
            glyph: Glyph::Error,
            ..Self::new(id, message)
        }
    }

    pub fn icon(mut self, glyph: Glyph) -> Self {
        self.glyph = glyph;
        self
    }

    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn with_size(mut self, size: Size) -> Self {
        self.size = size;
        self
    }
}

impl RenderOnce for Alert {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        // A faint wash of the tone behind its own ink, and a stronger one for the edge.
        let (ink, fill, edge) = match self.tone {
            Tone::Default => (theme.foreground, theme.background, theme.border),
            Tone::Warning => (
                theme.warning,
                theme.warning.opacity(0.04),
                theme.warning.opacity(0.3),
            ),
            Tone::Error => (
                theme.danger,
                theme.danger.opacity(0.04),
                theme.danger.opacity(0.3),
            ),
        };
        let (padding_x, padding_y, gap) = match self.size {
            Size::XSmall | Size::Small => (px(12.), px(8.), px(6.)),
            Size::Medium => (px(16.), px(10.), px(12.)),
        };

        h_flex()
            .id(self.id.clone())
            .debug_selector(move || self.id.to_string())
            .w_full()
            .items_start()
            .gap(gap)
            .px(padding_x)
            .py(padding_y)
            .text_sm()
            .text_color(ink)
            .bg(fill)
            .border_1()
            .border_color(edge)
            .rounded(theme.radius)
            .child(div().mt(px(3.)).child(Icon::new(self.glyph).size(px(14.))))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(2.))
                    .when_some(self.title, |column, title| {
                        column.child(
                            div()
                                .w_full()
                                .truncate()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(title),
                        )
                    })
                    .child(self.message),
            )
    }
}
