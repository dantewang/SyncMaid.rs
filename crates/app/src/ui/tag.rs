use gpui_kit::{
    div, prelude::*, px, relative, AnyElement, App, IntoElement, ParentElement, RenderOnce, Window,
};

use crate::theme::ActiveTheme as _;

#[derive(Clone, Copy)]
enum Tone {
    Primary,
    Secondary,
    Warning,
}

/// A small label: filled to be read first, outlined to be reference.
#[derive(IntoElement)]
pub struct Tag {
    tone: Tone,
    outline: bool,
    pill: bool,
    children: Vec<AnyElement>,
}

impl Tag {
    fn new(tone: Tone) -> Self {
        Self {
            tone,
            outline: false,
            pill: false,
            children: Vec::new(),
        }
    }

    pub fn primary() -> Self {
        Self::new(Tone::Primary)
    }

    pub fn secondary() -> Self {
        Self::new(Tone::Secondary)
    }

    pub fn warning() -> Self {
        Self::new(Tone::Warning)
    }

    pub fn outline(mut self) -> Self {
        self.outline = true;
        self
    }

    pub fn rounded_full(mut self) -> Self {
        self.pill = true;
        self
    }
}

impl ParentElement for Tag {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Tag {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let (fill, ink, edge) = match self.tone {
            Tone::Primary => (theme.primary, theme.primary_foreground, theme.primary),
            Tone::Secondary => (theme.secondary, theme.secondary_foreground, theme.border),
            Tone::Warning => (theme.warning, theme.warning_foreground, theme.warning),
        };
        let (fill, ink) = if self.outline {
            let ink = match self.tone {
                Tone::Primary => theme.primary,
                Tone::Secondary => theme.muted_foreground,
                Tone::Warning => theme.warning,
            };
            (gpui_kit::transparent_black(), ink)
        } else {
            (fill, ink)
        };

        div()
            .flex()
            .items_center()
            .border_1()
            .line_height(relative(1.))
            .text_xs()
            .px(px(10.))
            .py(px(4.))
            .bg(fill)
            .text_color(ink)
            .border_color(edge)
            .rounded(if self.pill { px(16.) } else { theme.radius })
            .children(self.children)
    }
}
