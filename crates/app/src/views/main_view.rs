use gpui::{div, prelude::*, px, Context, Window};
use gpui_component::{h_flex, v_flex, ActiveTheme as _, TitleBar};

/// The main window's content. A skeleton for now: the title bar is real, the body is a
/// placeholder until the task list lands.
pub struct MainView;

impl Render for MainView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                TitleBar::new().child(
                    h_flex()
                        .gap_2()
                        .child(div().text_size(px(13.)).child("SyncMaid")),
                ),
            )
            .child(div().flex_1().p_4().child("Sync tasks"))
    }
}
