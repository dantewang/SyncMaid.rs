use std::rc::Rc;

use gpui_kit::base::{Link as BaseLink, StyledExt as _};
use gpui_kit::{
    prelude::*, AnyElement, App, ClickEvent, ElementId, IntoElement, MouseButton, ParentElement,
    RenderOnce, StyleRefinement, Styled, Window,
};

use crate::theme::ActiveTheme as _;

type Handler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// Underlined text that does something when clicked.
///
/// The caller's styling is kept apart and laid over the link's own, so a caller that recolours
/// it replaces the colour rather than racing the default for it.
#[derive(IntoElement)]
pub struct Link {
    id: ElementId,
    style: StyleRefinement,
    children: Vec<AnyElement>,
    on_click: Option<Handler>,
}

impl Link {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            style: StyleRefinement::default(),
            children: Vec::new(),
            on_click: None,
        }
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl Styled for Link {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl ParentElement for Link {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Link {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let link = cx.theme().link;

        BaseLink::new(self.id)
            .text_color(link)
            .text_decoration_1()
            .text_decoration_color(link.opacity(0.5))
            .cursor_pointer()
            .hover(move |style| {
                style
                    .text_color(link.opacity(0.8))
                    .text_decoration_1()
                    .text_decoration_color(link)
            })
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .when_some(self.on_click, |element, handler| {
                element.on_activate(move |event, window, cx| handler(event, window, cx))
            })
            .refine_style(&self.style)
            .children(self.children)
    }
}
