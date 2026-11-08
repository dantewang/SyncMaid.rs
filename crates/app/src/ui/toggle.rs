use std::rc::Rc;

use gpui_kit::base::{
    Checkbox as BaseCheckbox, CheckboxIndicator, StyledExt as _, Switch as BaseSwitch, SwitchThumb,
    SwitchTrack,
};
use gpui_kit::{
    div, prelude::*, px, relative, svg, App, ElementId, IntoElement, MouseButton, RenderOnce,
    SharedString, Window,
};

use super::focus_ring::focus_ring;
use crate::components::Glyph;
use crate::theme::ActiveTheme as _;

type ToggleHandler = Rc<dyn Fn(&bool, &mut Window, &mut App)>;

/// A square box with a tick, and a label beside it. The handler receives the next value.
#[derive(IntoElement)]
pub struct Checkbox {
    id: ElementId,
    label: Option<SharedString>,
    checked: bool,
    on_click: Option<ToggleHandler>,
}

impl Checkbox {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            label: None,
            checked: false,
            on_click: None,
        }
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    pub fn on_click(mut self, handler: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for Checkbox {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        // Owned here so the ring can ask; a press never focuses the box, so this is keyboard focus.
        let focus = window
            .use_keyed_state(self.id.clone(), cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();
        let focused = focus.is_focused(window);

        let theme = cx.theme();
        let checked = self.checked;
        let (primary, mark) = (theme.primary, theme.primary_foreground);

        BaseCheckbox::new(self.id.clone())
            .debug_selector(move || self.id.to_string())
            .track_focus(&focus)
            .checked(checked)
            .when_some(self.label.clone(), |checkbox, label| {
                checkbox.accessibility_label(label)
            })
            .when_some(self.on_click, |checkbox, handler| {
                checkbox.on_change(move |_, _, window, cx| handler(&!checked, window, cx))
            })
            // Pressing the box must not move focus onto it.
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
            .h_flex()
            .gap(px(8.))
            .items_start()
            .line_height(relative(1.))
            .text_color(theme.foreground)
            .child(
                CheckboxIndicator::new()
                    .checked(checked)
                    .relative()
                    .size(px(16.))
                    .flex_shrink_0()
                    .border_1()
                    .rounded(theme.radius.min(px(4.)))
                    .bg(theme.background)
                    .border_color(theme.input)
                    // On the box rather than the row: the row has no edge of its own to line.
                    .when(focused, |indicator| {
                        indicator
                            .border_color(theme.ring)
                            .child(focus_ring(theme.radius.min(px(4.)), cx))
                    })
                    .styles(|styles| {
                        styles.checked(|style| style.border_color(primary).bg(primary))
                    })
                    .when(checked, |indicator| {
                        indicator.child(
                            svg()
                                .absolute()
                                .top(px(1.))
                                .left(px(1.))
                                .size(px(12.))
                                .text_color(mark)
                                .path(Glyph::Check.path()),
                        )
                    }),
            )
            .when_some(self.label, |checkbox, label| {
                checkbox.child(div().line_height(relative(1.)).child(label))
            })
    }
}

/// An on/off switch. The handler receives the next value.
#[derive(IntoElement)]
pub struct Switch {
    id: ElementId,
    checked: bool,
    on_click: Option<ToggleHandler>,
}

impl Switch {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            checked: false,
            on_click: None,
        }
    }

    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    pub fn on_click(mut self, handler: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for Switch {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        const WIDTH: f32 = 36.;
        const HEIGHT: f32 = 20.;
        const THUMB: f32 = 16.;
        const INSET: f32 = 2.;

        let theme = cx.theme();
        let checked = self.checked;
        let primary = theme.primary;

        BaseSwitch::new(self.id.clone())
            .debug_selector({
                let id = self.id.clone();
                move || id.to_string()
            })
            .checked(checked)
            .when_some(self.on_click, |switch, handler| {
                switch.on_change(move |next, _, window, cx| handler(&next, window, cx))
            })
            .child(
                SwitchTrack::new((self.id, "track"))
                    .checked(checked)
                    .w(px(WIDTH))
                    .h(px(HEIGHT))
                    .rounded(theme.radius)
                    .flex()
                    .items_center()
                    .border(px(INSET))
                    .border_color(gpui_kit::transparent_black())
                    .bg(theme.switch)
                    .styles(|styles| styles.checked(|style| style.bg(primary)))
                    .child(
                        SwitchThumb::new(checked)
                            .rounded(theme.radius)
                            .size(px(THUMB))
                            .left(px(if checked {
                                WIDTH - THUMB - INSET * 2.
                            } else {
                                0.
                            }))
                            .bg(theme.switch_thumb),
                    ),
            )
    }
}
