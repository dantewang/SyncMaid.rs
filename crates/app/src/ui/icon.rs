use gpui_kit::{prelude::*, svg, App, Hsla, IntoElement, Pixels, RenderOnce, Window};

use crate::components::Glyph;

/// One glyph, drawn in the surrounding text colour at the surrounding text size unless told
/// otherwise.
#[derive(IntoElement)]
pub struct Icon {
    glyph: Glyph,
    size: Option<Pixels>,
    color: Option<Hsla>,
}

impl Icon {
    pub fn new(glyph: Glyph) -> Self {
        Self {
            glyph,
            size: None,
            color: None,
        }
    }

    pub fn size(mut self, size: Pixels) -> Self {
        self.size = Some(size);
        self
    }

    pub fn text_color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }
}

impl RenderOnce for Icon {
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
        let style = window.text_style();
        let size = self
            .size
            .unwrap_or_else(|| style.font_size.to_pixels(window.rem_size()));

        svg()
            .path(self.glyph.path())
            .flex_shrink_0()
            .size(size)
            .text_color(self.color.unwrap_or(style.color))
    }
}
