use std::rc::Rc;

use gpui_kit::{
    div, prelude::*, AnyElement, AnyView, App, Context, FocusHandle, IntoElement, KeyBinding,
    Render, WeakFocusHandle, Window,
};

use super::{Dialog, Sheet};
use crate::theme::ActiveTheme as _;

gpui_kit::actions!(root, [Tab, TabPrev]);

const CONTEXT: &str = "Root";

pub(super) fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("tab", Tab, Some(CONTEXT)),
        KeyBinding::new("shift-tab", TabPrev, Some(CONTEXT)),
    ]);
}

type SheetBuilder = Rc<dyn Fn(Sheet, &mut Window, &mut App) -> Sheet>;
type DialogBuilder = Rc<dyn Fn(Dialog, &mut Window, &mut App) -> Dialog>;

/// One open surface: the builder that draws it each frame, the focus it holds, and the focus to
/// give back when it closes.
struct Layer<B> {
    focus: FocusHandle,
    previous: Option<WeakFocusHandle>,
    builder: B,
}

/// Every window's root view: the application view, plus the sheet slot and the dialog stack.
///
/// It holds them but draws neither. The application view renders
/// [`Root::render_sheet_layer`] and [`Root::render_dialog_layer`] itself, which is what puts
/// them above its own content.
pub struct Root {
    view: AnyView,
    sheet: Option<Layer<SheetBuilder>>,
    dialogs: Vec<Layer<DialogBuilder>>,
}

impl Root {
    pub fn new(view: impl Into<AnyView>, _: &mut Window, _: &mut Context<Self>) -> Self {
        Self {
            view: view.into(),
            sheet: None,
            dialogs: Vec::new(),
        }
    }

    fn update<R>(
        window: &mut Window,
        cx: &mut App,
        f: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) -> R,
    ) -> R {
        let root = window
            .root::<Root>()
            .flatten()
            .expect("every window's root view is a ui::Root");
        root.update(cx, |root, cx| f(root, window, cx))
    }

    /// The open sheet, if there is one.
    pub fn render_sheet_layer(window: &mut Window, cx: &mut App) -> Option<AnyElement> {
        let root = window.root::<Root>()??;
        let (focus, builder) = root
            .read(cx)
            .sheet
            .as_ref()
            .map(|layer| (layer.focus.clone(), layer.builder.clone()))?;
        Some(builder(Sheet::new(focus), window, cx).into_any_element())
    }

    /// Every open dialog, the most recent on top.
    pub fn render_dialog_layer(window: &mut Window, cx: &mut App) -> Option<AnyElement> {
        let root = window.root::<Root>()??;
        let layers: Vec<_> = root
            .read(cx)
            .dialogs
            .iter()
            .map(|layer| (layer.focus.clone(), layer.builder.clone()))
            .collect();
        let top = layers.len().checked_sub(1)?;

        let dialogs: Vec<_> = layers
            .into_iter()
            .enumerate()
            .map(|(index, (focus, builder))| {
                builder(Dialog::new(focus, index, index == top), window, cx).into_any_element()
            })
            .collect();
        Some(div().children(dialogs).into_any_element())
    }

    fn open_sheet(&mut self, builder: SheetBuilder, window: &mut Window, cx: &mut Context<Self>) {
        // Replacing a sheet keeps the focus the first one took, so closing either way lands
        // where the user was before any sheet opened.
        let previous = self
            .sheet
            .take()
            .and_then(|layer| layer.previous)
            .or_else(|| window.focused(cx).map(|handle| handle.downgrade()));
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        self.sheet = Some(Layer {
            focus,
            previous,
            builder,
        });
        cx.notify();
    }

    fn close_sheet(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(previous) = self
            .sheet
            .take()
            .and_then(|layer| layer.previous)
            .and_then(|handle| handle.upgrade())
        {
            window.focus(&previous, cx);
        }
        cx.notify();
    }

    fn open_dialog(&mut self, builder: DialogBuilder, window: &mut Window, cx: &mut Context<Self>) {
        let previous = window.focused(cx).map(|handle| handle.downgrade());
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        self.dialogs.push(Layer {
            focus,
            previous,
            builder,
        });
        cx.notify();
    }

    fn close_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(previous) = self
            .dialogs
            .pop()
            .and_then(|layer| layer.previous)
            .and_then(|handle| handle.upgrade())
        {
            window.focus(&previous, cx);
        }
        cx.notify();
    }

    /// Tab stays inside an open sheet or dialog, wrapping at the end rather than escaping to the
    /// window behind it.
    fn cycle_focus(forward: bool, window: &mut Window, cx: &mut App) {
        let step = |window: &mut Window, cx: &mut App| {
            if forward {
                window.focus_next(cx)
            } else {
                window.focus_prev(cx)
            }
        };

        let Some(trap) = gpui_kit::base::active_focus_trap(window, cx) else {
            step(window, cx);
            return;
        };

        let start = window.focused(cx);
        step(window, cx);
        let mut attempts = 0;
        while !trap.contains_focused(window, cx) && attempts < 100 {
            step(window, cx);
            attempts += 1;
            if window.focused(cx) == start {
                break;
            }
        }
    }
}

impl Render for Root {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        window.set_rem_size(theme.font_size);

        div()
            .id("root")
            .key_context(CONTEXT)
            .on_action(|_: &Tab, window, cx| Self::cycle_focus(true, window, cx))
            .on_action(|_: &TabPrev, window, cx| Self::cycle_focus(false, window, cx))
            .relative()
            .size_full()
            .font_family(theme.font_family.clone())
            .bg(theme.background)
            .text_color(theme.foreground)
            .child(self.view.clone())
    }
}

/// Opening and closing the surfaces [`Root`] holds.
///
/// Do not call these from inside `WindowHandle::update`: that closure already holds `Root`, and
/// these need it too. `Window::defer` moves the call one tick later.
pub trait WindowExt {
    /// Opens a sheet from the right, replacing any sheet already open.
    fn open_sheet<F>(&mut self, cx: &mut App, build: F)
    where
        F: Fn(Sheet, &mut Window, &mut App) -> Sheet + 'static;

    fn close_sheet(&mut self, cx: &mut App);

    /// Opens a dialog on top of any already open.
    fn open_dialog<F>(&mut self, cx: &mut App, build: F)
    where
        F: Fn(Dialog, &mut Window, &mut App) -> Dialog + 'static;

    /// Closes the topmost dialog.
    fn close_dialog(&mut self, cx: &mut App);
}

impl WindowExt for Window {
    fn open_sheet<F>(&mut self, cx: &mut App, build: F)
    where
        F: Fn(Sheet, &mut Window, &mut App) -> Sheet + 'static,
    {
        Root::update(self, cx, move |root, window, cx| {
            root.open_sheet(Rc::new(build), window, cx)
        })
    }

    fn close_sheet(&mut self, cx: &mut App) {
        Root::update(self, cx, |root, window, cx| root.close_sheet(window, cx))
    }

    fn open_dialog<F>(&mut self, cx: &mut App, build: F)
    where
        F: Fn(Dialog, &mut Window, &mut App) -> Dialog + 'static,
    {
        Root::update(self, cx, move |root, window, cx| {
            root.open_dialog(Rc::new(build), window, cx)
        })
    }

    fn close_dialog(&mut self, cx: &mut App) {
        Root::update(self, cx, |root, window, cx| root.close_dialog(window, cx))
    }
}
