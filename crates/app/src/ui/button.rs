use std::rc::Rc;

use gpui_kit::base::Button as BaseButton;
use gpui_kit::{
    prelude::*, px, App, ClickEvent, ElementId, Hsla, IntoElement, MouseButton, Pixels, RenderOnce,
    SharedString, Window,
};

use super::{h_flex, Icon, Tooltip};
use crate::components::Glyph;
use crate::theme::{ActiveTheme as _, Theme};

type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
type GroupHandler = Rc<dyn Fn(&Vec<usize>, &mut Window, &mut App)>;

/// How big a control is. Medium is the default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Size {
    XSmall,
    Small,
    #[default]
    Medium,
}

impl Size {
    /// Height, horizontal padding (normal and compact), gap and icon size.
    fn metrics(self) -> (Pixels, Pixels, Pixels, Pixels, Pixels) {
        match self {
            Size::XSmall => (px(20.), px(4.), px(4.), px(4.), px(12.)),
            Size::Small => (px(24.), px(8.), px(6.), px(4.), px(14.)),
            Size::Medium => (px(32.), px(10.), px(8.), px(8.), px(16.)),
        }
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Variant {
    /// A hairline box on the ground colour.
    #[default]
    Default,
    Primary,
    Danger,
    /// Nothing until hovered.
    Ghost,
}

/// Every colour a button takes, in every state it can be in.
struct Colors {
    bg: Hsla,
    border: Hsla,
    fg: Hsla,
    hover: Hsla,
    active: Hsla,
    selected: Hsla,
    disabled_bg: Hsla,
    disabled_border: Hsla,
}

impl Variant {
    fn colors(self, outline: bool, theme: &Theme) -> Colors {
        let clear = gpui_kit::transparent_black();
        match (self, outline) {
            (Variant::Default, _) => Colors {
                bg: theme.background,
                border: theme.input,
                fg: theme.foreground,
                hover: theme.input.opacity(0.5),
                active: theme.input.opacity(0.7),
                selected: theme.input.opacity(0.7),
                disabled_bg: theme.background.opacity(0.5),
                disabled_border: theme.input.opacity(0.5),
            },
            (Variant::Primary, false) => Colors {
                bg: theme.primary,
                border: theme.primary,
                fg: theme.primary_foreground,
                hover: theme.primary_hover,
                active: theme.primary_active,
                selected: theme.primary_active,
                disabled_bg: theme.primary.opacity(0.15),
                disabled_border: theme.primary.opacity(0.15),
            },
            (Variant::Primary, true) => Colors {
                bg: theme.primary.opacity(0.1),
                border: theme.primary,
                fg: theme.primary,
                hover: theme.primary_hover.opacity(0.2),
                active: theme.primary_active.opacity(0.4),
                selected: theme.primary_active.opacity(0.4),
                disabled_bg: theme.primary.opacity(0.05),
                disabled_border: theme.primary.opacity(0.5),
            },
            (Variant::Danger, false) => Colors {
                bg: theme.danger,
                border: theme.danger,
                fg: theme.danger_foreground,
                hover: theme.danger_hover,
                active: theme.danger_active,
                selected: theme.danger_active,
                disabled_bg: theme.danger.opacity(0.15),
                disabled_border: theme.danger.opacity(0.15),
            },
            (Variant::Danger, true) => Colors {
                bg: theme.danger.opacity(0.1),
                border: theme.danger.opacity(0.4),
                fg: theme.danger,
                hover: theme.danger_hover.opacity(0.2),
                active: theme.danger_active.opacity(0.4),
                selected: theme.danger_active.opacity(0.4),
                disabled_bg: theme.danger.opacity(0.05),
                disabled_border: theme.danger.opacity(0.2),
            },
            (Variant::Ghost, _) => Colors {
                bg: clear,
                border: clear,
                fg: theme.secondary_foreground,
                hover: theme.secondary_active.opacity(0.8),
                active: theme.border.opacity(0.8),
                selected: theme.secondary_active,
                disabled_bg: clear,
                disabled_border: clear,
            },
        }
    }
}

/// A button: an icon, a label, or both.
#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    label: Option<SharedString>,
    icon: Option<Glyph>,
    tooltip: Option<SharedString>,
    variant: Variant,
    outline: bool,
    size: Size,
    compact: bool,
    selected: bool,
    disabled: bool,
    /// Set on every button of a group but the first, so neighbours share one hairline.
    joined: bool,
    on_click: Option<ClickHandler>,
}

impl Button {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            label: None,
            icon: None,
            tooltip: None,
            variant: Variant::Default,
            outline: false,
            size: Size::Medium,
            compact: false,
            selected: false,
            disabled: false,
            joined: false,
            on_click: None,
        }
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn icon(mut self, glyph: Glyph) -> Self {
        self.icon = Some(glyph);
        self
    }

    pub fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    pub fn primary(mut self) -> Self {
        self.variant = Variant::Primary;
        self
    }

    pub fn danger(mut self) -> Self {
        self.variant = Variant::Danger;
        self
    }

    pub fn ghost(mut self) -> Self {
        self.variant = Variant::Ghost;
        self
    }

    pub fn outline(mut self) -> Self {
        self.outline = true;
        self
    }

    pub fn with_size(mut self, size: Size) -> Self {
        self.size = size;
        self
    }

    pub fn small(self) -> Self {
        self.with_size(Size::Small)
    }

    pub fn xsmall(self) -> Self {
        self.with_size(Size::XSmall)
    }

    pub fn compact(mut self) -> Self {
        self.compact = true;
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }

    fn joined(mut self, joined: bool) -> Self {
        self.joined = joined;
        self
    }
}

impl RenderOnce for Button {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let colors = self.variant.colors(self.outline, theme);
        let radius = theme.radius;
        let disabled_fg = theme.muted_foreground.opacity(0.5);
        let (height, padding, compact_padding, gap, icon_size) = self.size.metrics();
        let icon_only = self.label.is_none();
        let bordered = self.outline || self.variant == Variant::Default;
        let reacts = !self.disabled && !self.selected;

        BaseButton::new(self.id)
            .flex_shrink_0()
            .h(height)
            .map(|button| {
                if icon_only {
                    button.w(height)
                } else if self.compact {
                    button.px(compact_padding).min_w(height)
                } else {
                    button.px(padding)
                }
            })
            .rounded(radius)
            .when(bordered, |button| {
                button
                    .border_t_1()
                    .border_r_1()
                    .border_b_1()
                    .when(!self.joined, |button| button.border_l_1())
            })
            .bg(colors.bg)
            .border_color(colors.border)
            .text_color(colors.fg)
            .when(reacts, |button| {
                button
                    .hover(|style| style.bg(colors.hover))
                    .active(|style| style.bg(colors.active))
            })
            .selected(self.selected)
            .disabled(self.disabled)
            .styles(|styles| {
                styles
                    .selected(|style| style.bg(colors.selected))
                    .disabled(|style| {
                        style
                            .bg(colors.disabled_bg)
                            .border_color(colors.disabled_border)
                            .text_color(disabled_fg)
                    })
            })
            // A press must not pull focus out of the field the user is typing in.
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
            .when_some(self.on_click, |button, handler| {
                button.on_click(move |event, window, cx| handler(event, window, cx))
            })
            .when_some(self.tooltip, |button, text| {
                button.tooltip(move |window, cx| Tooltip::new(text.clone()).build(window, cx))
            })
            .child(
                h_flex()
                    .gap(gap)
                    .whitespace_nowrap()
                    .map(|content| match self.size {
                        Size::XSmall => content.text_xs(),
                        Size::Small => content.text_sm(),
                        Size::Medium => content.text_base(),
                    })
                    .when_some(self.icon, |content, glyph| {
                        content.child(Icon::new(glyph).size(icon_size))
                    })
                    .when_some(self.label, |content, label| content.child(label)),
            )
    }
}

/// Buttons joined into one segmented control. Clicking one reports its index.
#[derive(IntoElement)]
pub struct ButtonGroup {
    id: ElementId,
    children: Vec<Button>,
    outline: bool,
    compact: bool,
    size: Option<Size>,
    on_click: Option<GroupHandler>,
}

impl ButtonGroup {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            children: Vec::new(),
            outline: false,
            compact: false,
            size: None,
            on_click: None,
        }
    }

    pub fn child(mut self, button: Button) -> Self {
        self.children.push(button);
        self
    }

    pub fn outline(mut self) -> Self {
        self.outline = true;
        self
    }

    pub fn compact(mut self) -> Self {
        self.compact = true;
        self
    }

    pub fn with_size(mut self, size: Size) -> Self {
        self.size = Some(size);
        self
    }

    pub fn small(self) -> Self {
        self.with_size(Size::Small)
    }

    /// `clicked` holds the one index that was pressed.
    pub fn on_click(
        mut self,
        handler: impl Fn(&Vec<usize>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for ButtonGroup {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let on_click = self.on_click;

        h_flex()
            .id(self.id)
            .children(
                self.children
                    .into_iter()
                    .enumerate()
                    .map(move |(index, button)| {
                        button
                            .joined(index > 0)
                            .when(self.outline, Button::outline)
                            .when(self.compact, Button::compact)
                            .when_some(self.size, Button::with_size)
                            .when_some(on_click.clone(), |button, handler| {
                                button.on_click(move |_, window, cx| {
                                    handler(&vec![index], window, cx)
                                })
                            })
                    }),
            )
    }
}
