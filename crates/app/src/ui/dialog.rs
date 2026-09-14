use std::time::Duration;

use gpui_kit::base::Dialog as BaseDialog;
use gpui_kit::{
    div, ease_out_quint, prelude::*, px, relative, Animation, AnimationExt as _, AnyElement, App,
    FocusHandle, FontWeight, IntoElement, ParentElement, Pixels, RenderOnce, SharedString, Window,
};

use super::{v_flex, WindowExt as _};
use crate::theme::ActiveTheme as _;

/// A centred card: a title, a body, and a footer the caller lays out.
///
/// Esc cancels and Enter confirms, and either one closes the card. Neither does anything else:
/// an action belongs to a button in the footer, pressed on purpose.
#[derive(IntoElement)]
pub struct Dialog {
    focus: FocusHandle,
    layer: usize,
    topmost: bool,
    width: Pixels,
    overlay_closable: bool,
    title: Option<SharedString>,
    footer: Option<AnyElement>,
    children: Vec<AnyElement>,
}

impl Dialog {
    pub(super) fn new(focus: FocusHandle, layer: usize, topmost: bool) -> Self {
        Self {
            focus,
            layer,
            topmost,
            width: px(448.),
            overlay_closable: true,
            title: None,
            footer: None,
            children: Vec::new(),
        }
    }

    pub fn w(mut self, width: Pixels) -> Self {
        self.width = width;
        self
    }

    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Whether a click on the scrim closes the dialog. On by default.
    pub fn overlay_closable(mut self, closable: bool) -> Self {
        self.overlay_closable = closable;
        self
    }

    pub fn footer(mut self, footer: impl IntoElement) -> Self {
        self.footer = Some(footer.into_any_element());
        self
    }
}

impl ParentElement for Dialog {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Dialog {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let overlay = theme.overlay;
        let viewport = window.viewport_size();
        let left = viewport.width / 2. - self.width / 2.;
        let top = viewport.height / 10. + px(self.layer as f32 * 16.);

        let popup = v_flex()
            .id(SharedString::from(format!("dialog-{}", self.layer)))
            .absolute()
            .left(left)
            .top(top)
            .w(self.width)
            .occlude()
            .py(px(16.))
            .gap(px(16.))
            .bg(theme.background)
            .border_1()
            .border_color(theme.border)
            .rounded(theme.radius_lg)
            .child(
                v_flex()
                    .gap(px(8.))
                    .px(px(16.))
                    .when_some(self.title, |content, title| {
                        content.child(
                            div()
                                .text_base()
                                .font_weight(FontWeight::SEMIBOLD)
                                .line_height(relative(1.))
                                .child(title),
                        )
                    })
                    .children(self.children),
            )
            .when_some(self.footer, |popup, footer| {
                popup.child(div().px(px(16.)).child(footer))
            })
            .with_animation(
                SharedString::from(format!("dialog-in-{}", self.layer)),
                Animation::new(Duration::from_millis(200)).with_easing(ease_out_quint()),
                move |popup, delta| popup.top(top - px(12.) * (1. - delta)).opacity(delta),
            );

        BaseDialog::new(cx)
            .focus_handle(self.focus)
            .layer(self.layer, self.topmost)
            .close_on_backdrop_press(self.overlay_closable)
            // Only the topmost dialog dims what is behind it; a second scrim would double it.
            .backdrop(
                div()
                    .size_full()
                    .when(self.topmost, |scrim| scrim.bg(overlay)),
            )
            .request_close(|_, window, cx| window.close_dialog(cx))
            .popup(popup)
    }
}
