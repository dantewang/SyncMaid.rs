use gpui_kit::{
    div, prelude::*, px, AnyElement, App, Div, FontWeight, IntoElement, ParentElement, RenderOnce,
    SharedString, Window,
};

use super::v_flex;

/// A column of fields.
pub fn v_form() -> Div {
    v_flex().w_full().gap(px(8.))
}

/// A labelled field. Children are the control and anything that explains it.
pub fn field() -> Field {
    Field {
        label: None,
        children: Vec::new(),
    }
}

#[derive(IntoElement)]
pub struct Field {
    label: Option<SharedString>,
    children: Vec<AnyElement>,
}

impl Field {
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }
}

impl ParentElement for Field {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Field {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        v_flex()
            .w_full()
            .gap(px(4.))
            .when_some(self.label, |field, label| {
                field.child(div().text_sm().font_weight(FontWeight::MEDIUM).child(label))
            })
            .child(div().w_full().children(self.children))
    }
}
