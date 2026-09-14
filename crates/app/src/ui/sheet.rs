use std::rc::Rc;
use std::time::Duration;

use gpui_kit::base::{actions::Cancel, Sheet as BaseSheet};
use gpui_kit::{
    div, ease_out_quint, prelude::*, px, Animation, AnimationExt as _, AnyElement, App, ClickEvent,
    FocusHandle, FontWeight, IntoElement, ParentElement, Pixels, RenderOnce, Window,
};

use super::{h_flex, v_flex, Button, WindowExt as _};
use crate::components::Glyph;
use crate::theme::ActiveTheme as _;

type CloseHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// The horizontal padding a sheet puts around its body.
pub const SHEET_PADDING: Pixels = px(16.);

/// A full-height panel from the right: a title bar, a body, and a footer.
///
/// The body is clamped to the sheet — its width, and the height between the title bar and the
/// footer — so content can neither widen the sheet nor push the footer off the bottom. Content
/// that is taller than that has to scroll itself.
#[derive(IntoElement)]
pub struct Sheet {
    focus: FocusHandle,
    width: Pixels,
    overlay_closable: bool,
    title: Option<AnyElement>,
    footer: Option<AnyElement>,
    on_close: CloseHandler,
    children: Vec<AnyElement>,
}

impl Sheet {
    pub(super) fn new(focus: FocusHandle) -> Self {
        Self {
            focus,
            width: px(350.),
            overlay_closable: true,
            title: None,
            footer: None,
            on_close: Rc::new(|_, _, _| {}),
            children: Vec::new(),
        }
    }

    pub fn size(mut self, width: Pixels) -> Self {
        self.width = width;
        self
    }

    /// Whether a click on the scrim closes the sheet. On by default.
    pub fn overlay_closable(mut self, closable: bool) -> Self {
        self.overlay_closable = closable;
        self
    }

    pub fn title(mut self, title: impl IntoElement) -> Self {
        self.title = Some(title.into_any_element());
        self
    }

    pub fn footer(mut self, footer: impl IntoElement) -> Self {
        self.footer = Some(footer.into_any_element());
        self
    }

    /// Called when the sheet is dismissed — Esc, the ✕, or the scrim — but not when the caller
    /// closes it itself.
    pub fn on_close(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_close = Rc::new(handler);
        self
    }
}

impl ParentElement for Sheet {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Sheet {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let overlay = theme.overlay;
        let width = self.width;
        let on_close = self.on_close;

        let surface = v_flex()
            .id("sheet-surface")
            .absolute()
            .top_0()
            .bottom_0()
            .right_0()
            .w(width)
            .occlude()
            .bg(theme.background)
            .border_l_1()
            .border_color(theme.border)
            .child(
                h_flex()
                    .flex_none()
                    .w_full()
                    .h(px(44.))
                    .pl(SHEET_PADDING)
                    .pr(px(12.))
                    .justify_between()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(self.title.unwrap_or_else(|| div().into_any_element()))
                    .child(
                        Button::new("sheet-close")
                            .icon(Glyph::Close)
                            .ghost()
                            .small()
                            .on_click(|_, window, cx| window.dispatch_action(Box::new(Cancel), cx)),
                    ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .px(SHEET_PADDING)
                    .overflow_hidden()
                    .children(self.children),
            )
            .when_some(self.footer, |surface, footer| {
                surface.child(
                    h_flex()
                        .flex_none()
                        .w_full()
                        .px(SHEET_PADDING)
                        .py(px(12.))
                        .child(footer),
                )
            })
            // The whole width, eased out: it arrives quickly and settles, rather than popping
            // most of the way in and crawling the rest.
            .with_animation(
                "sheet-slide",
                Animation::new(Duration::from_millis(220)).with_easing(ease_out_quint()),
                move |surface, delta| surface.right(-width * (1. - delta)),
            );

        BaseSheet::new(cx)
            .focus_handle(self.focus)
            .overlay(div().absolute().inset_0().bg(overlay))
            .overlay_closable(self.overlay_closable)
            .request_close(|window, cx| window.close_sheet(cx))
            .on_close(move |event, window, cx| on_close(event, window, cx))
            .surface(surface)
    }
}
