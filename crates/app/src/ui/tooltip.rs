use gpui_kit::{div, prelude::*, px, AnyView, App, Context, Render, SharedString, Window};

use crate::theme::ActiveTheme as _;

/// A line of text in a hairline box, for gpui's own `.tooltip(...)`.
pub struct Tooltip {
    text: SharedString,
}

impl Tooltip {
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self { text: text.into() }
    }

    pub fn build(self, _: &mut Window, cx: &mut App) -> AnyView {
        cx.new(|_| self).into()
    }
}

impl Render for Tooltip {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();

        // Offset from the pointer, so the box does not sit under the cursor that summoned it.
        div().pl(px(10.)).pt(px(10.)).child(
            div()
                .font_family(theme.font_family.clone())
                .text_sm()
                .py(px(2.))
                .px(px(8.))
                .bg(theme.background)
                .text_color(theme.foreground)
                .border_1()
                .border_color(theme.border)
                .rounded(theme.radius)
                .child(self.text.clone()),
        )
    }
}
