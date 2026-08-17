//! A row of mutually exclusive choices, shown as one segmented control.
//!
//! Used wherever there are two or three options and all of them are worth seeing at once — the
//! trigger kind, where a Move destination puts files, whether a destination filters. A dropdown
//! would hide the alternatives behind a click for no gain.

use std::rc::Rc;

use gpui::{div, prelude::*, px, App, ElementId, IntoElement, RenderOnce, SharedString, Window};

use crate::components::{icon, Icon};

/// What a segment stores for its click handler. Named because the bare type is unreadable.
type SelectHandler = Rc<dyn Fn(&usize, &mut Window, &mut App)>;
use crate::theme;

/// One choice.
pub struct SegmentOption {
    pub label: SharedString,
    pub glyph: Option<Icon>,
}

impl SegmentOption {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            glyph: None,
        }
    }

    pub fn glyph(self, glyph: Icon) -> Self {
        Self {
            glyph: Some(glyph),
            ..self
        }
    }
}

/// See the module docs.
#[derive(IntoElement)]
pub struct Segment {
    id: ElementId,
    options: Vec<SegmentOption>,
    selected: usize,
    /// The tighter variant used inside a filter group's header.
    small: bool,
    /// Lets the choices run onto a second line instead of squeezing onto one.
    wrap: bool,
    /// Segments stretch to fill their row by default; a small one hugs its content.
    on_select: Option<SelectHandler>,
}

impl Segment {
    pub fn new(id: impl Into<ElementId>, options: Vec<SegmentOption>, selected: usize) -> Self {
        Self {
            id: id.into(),
            options,
            selected,
            small: false,
            wrap: false,
            on_select: None,
        }
    }

    pub fn small(mut self) -> Self {
        self.small = true;
        self
    }

    /// Wraps onto more lines rather than crushing the choices together.
    ///
    /// For the one control with five of them: five equal shares of a 440 px card leaves no room
    /// for any of the labels.
    pub fn wrapping(mut self) -> Self {
        self.wrap = true;
        self.small = true;
        self
    }

    pub fn on_select(mut self, handler: impl Fn(&usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for Segment {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let small = self.small;
        let wrap = self.wrap;
        let selected = self.selected;
        let handler = self.on_select.clone();
        let base_id = self.id;

        div()
            .flex()
            .flex_row()
            .when(wrap, |element| element.flex_wrap())
            .gap(px(if small { 6. } else { 8. }))
            .children(
                self.options
                    .into_iter()
                    .enumerate()
                    .map(move |(index, option)| {
                        let chosen = index == selected;
                        let (background, border, foreground) = if chosen {
                            (theme::TEAL_SUBTLE, theme::TEAL, theme::TEAL)
                        } else {
                            (theme::PAGE, theme::HAIRLINE_STRONG, theme::TEXT_SECONDARY)
                        };

                        let mut element = div()
                            .id(SharedString::from(format!("{base_id:?}-{index}")))
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_center()
                            .gap(px(6.))
                            .rounded(theme::radius::control())
                            .border_1()
                            .border_color(theme::color(border))
                            .bg(theme::color(background))
                            .text_color(theme::color(foreground))
                            .cursor_pointer();

                        element = if small {
                            element
                                .px(px(10.))
                                .py(px(3.))
                                .text_size(theme::text::small())
                        } else {
                            // Equal shares of the row, so the control reads as one thing.
                            element.flex_1().px(px(14.)).py(px(7.))
                        };

                        if let Some(glyph) = option.glyph {
                            element = element.child(icon(glyph, px(15.), theme::color(foreground)));
                        }
                        element = element.child(option.label);

                        if let Some(handler) = handler.clone() {
                            element =
                                element.on_click(move |_, window, cx| handler(&index, window, cx));
                        }
                        element
                    }),
            )
    }
}
