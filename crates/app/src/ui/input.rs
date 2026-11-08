use gpui_kit::base::input::InputEditorStyle;
use gpui_kit::{
    prelude::*, px, rems, App, Entity, Focusable as _, IntoElement, MouseButton, RenderOnce, Window,
};

pub use gpui_kit::base::input::InputState;

use super::focus_ring::focus_ring;
use super::h_flex;
use crate::theme::ActiveTheme as _;

/// A single-line text field: gpui-base's editing engine in a hairline frame.
#[derive(IntoElement)]
pub struct Input {
    state: Entity<InputState>,
    selector: Option<&'static str>,
}

impl Input {
    pub fn new(state: &Entity<InputState>) -> Self {
        Self {
            state: state.clone(),
            selector: None,
        }
    }

    /// The name a GUI test finds this field by. A field has no id of its own to reuse, the way
    /// a button does. A no-op outside test builds.
    pub fn debug_selector(mut self, selector: &'static str) -> Self {
        self.selector = Some(selector);
        self
    }
}

impl RenderOnce for Input {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let style = InputEditorStyle {
            foreground: theme.foreground,
            muted_foreground: theme.muted_foreground,
            background: theme.background,
            border: theme.border,
            selection: theme.selection,
            caret: theme.caret,
            ..InputEditorStyle::default()
        };
        let (ground, edge, ring, radius) =
            (theme.background, theme.input, theme.ring, theme.radius);

        // Projected every frame rather than once at construction, so the field follows the
        // palette that is current now.
        self.state
            .update(cx, |state, _| state.set_editor_style(style));
        let focused = self.state.read(cx).focus_handle(cx).is_focused(window);
        let state = self.state.clone();

        h_flex()
            .id(("input", self.state.entity_id()))
            .when_some(self.selector, |field, selector| {
                field.debug_selector(move || selector.to_owned())
            })
            .w_full()
            .h(px(32.))
            .px(px(10.))
            .text_sm()
            .line_height(rems(1.25))
            .bg(ground)
            .border_1()
            .border_color(if focused { ring } else { edge })
            .rounded(radius)
            .relative()
            .when(focused, |field| field.child(focus_ring(radius, cx)))
            .cursor_text()
            // The frame's padding is part of the field: a click there still means "type here".
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                state.update(cx, |state, cx| state.focus(window, cx));
            })
            .child(self.state)
    }
}
