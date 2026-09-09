//! A scrollbar drawn in Porcelain's own terms: a hairline rail with a square tick.
//!
//! `gpui_component::Scrollbar` cannot be restyled into this. Its track width, thumb width and —
//! the part that shows — its 3px corner radius are private constants with no theme tokens behind
//! them, so on a theme whose `radius` is zero it renders the only rounded object on screen. At
//! six pixels wide that reads less as a scrollbar than as something that failed to paint square.
//!
//! What is here is deliberately less than the library's: one axis, no fade-out timer, no hover
//! growth. The bar is an indicator that also happens to be draggable, and the wheel is still how
//! anyone actually scrolls.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    div, point, prelude::*, px, App, Context, DragMoveEvent, ElementId, Empty, Pixels, Render,
    ScrollHandle, Window,
};
use gpui_component::ActiveTheme as _;

/// The column the rail reserves. The drawn parts are 1px and 3px; the rest is grab room.
pub const RAIL_WIDTH: Pixels = px(16.);

/// Distance from the column's right edge to the rail and to the tick. The tick straddles the
/// rail rather than sitting beside it: one line, thickened where you are.
const RAIL_RIGHT: Pixels = px(8.);
const TICK_RIGHT: Pixels = px(7.);
const TICK_WIDTH: Pixels = px(3.);

/// Below this a tick stops reading as a position and starts reading as a speck.
const TICK_MIN_HEIGHT: f32 = 26.;

/// Where the tick would sit, and how tall, for a viewport of `viewport` showing content that
/// extends `max` beyond it.
fn tick_metrics(viewport: Pixels, max: Pixels) -> (f32, f32) {
    let viewport = f32::from(viewport);
    let max = f32::from(max);
    let height = (viewport * viewport / (viewport + max)).max(TICK_MIN_HEIGHT);
    (height, (viewport - height).max(0.))
}

/// A repaint request for the view that owns the scroll area.
pub type ScrollNotify = Rc<dyn Fn(&mut Window, &mut App)>;

/// The drag needs a preview element. This one draws nothing: the tick is already following the
/// pointer, and a ghost of it beside the cursor would be a second tick.
struct NoPreview;

impl Render for NoPreview {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

/// Where the tick was when the drag began, and where the pointer first landed.
///
/// gpui hands the drag value to every move event but never says where the grab started, so the
/// first move records it. Without that the tick would jump to centre itself under the cursor.
struct RailDrag {
    tick_top: f32,
    grab: Rc<Cell<Option<f32>>>,
}

/// A vertical scrollbar for a `track_scroll`ed element.
///
/// Position it inside a `relative()` wrapper around the scroll area, not inside the scroll area
/// itself — it lays itself out absolutely, and as a child of the content it would scroll away
/// with the thing it is measuring.
#[derive(IntoElement)]
pub struct ScrollRail {
    id: ElementId,
    handle: ScrollHandle,
    on_scroll: Option<ScrollNotify>,
}

impl ScrollRail {
    pub fn vertical(id: impl Into<ElementId>, handle: &ScrollHandle) -> Self {
        Self {
            id: id.into(),
            handle: handle.clone(),
            on_scroll: None,
        }
    }

    /// Called after a drag has moved the offset, so the owning view can repaint.
    ///
    /// Dragging writes straight to the `ScrollHandle`, which nothing observes; without this the
    /// list would not move until something else asked for a frame.
    pub fn on_scroll(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_scroll = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for ScrollRail {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let viewport = self.handle.bounds().size.height;
        let max = self.handle.max_offset().height;

        // Nothing hidden, nothing drawn. Being absent when everything fits is what lets the bar
        // mean something when it is there.
        if max <= px(1.) || viewport <= px(0.) {
            return div().into_any_element();
        }

        let (tick_height, travel) = tick_metrics(viewport, max);
        let scrolled = (-f32::from(self.handle.offset().y)).clamp(0., f32::from(max));
        let tick_top = travel * (scrolled / f32::from(max));

        let handle = self.handle.clone();
        let on_scroll = self.on_scroll.clone();

        div()
            .id(self.id)
            .absolute()
            .top_0()
            .bottom_0()
            .right_0()
            .w(RAIL_WIDTH)
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .right(RAIL_RIGHT)
                    .w(px(1.))
                    .bg(cx.theme().border),
            )
            .child(
                div()
                    .id("tick")
                    .absolute()
                    .right(TICK_RIGHT)
                    .w(TICK_WIDTH)
                    .top(px(tick_top))
                    .h(px(tick_height))
                    .bg(cx.theme().secondary_foreground)
                    .on_drag(
                        RailDrag {
                            tick_top,
                            grab: Rc::new(Cell::new(None)),
                        },
                        |_, _, _, cx| cx.new(|_| NoPreview),
                    ),
            )
            .on_drag_move(move |event: &DragMoveEvent<RailDrag>, window, cx| {
                let viewport = handle.bounds().size.height;
                let max = handle.max_offset().height;
                let (_, travel) = tick_metrics(viewport, max);
                if travel <= 0. {
                    return;
                }

                let drag = event.drag(cx);
                let pointer = f32::from(event.event.position.y);
                let grabbed = drag.grab.get().unwrap_or_else(|| {
                    drag.grab.set(Some(pointer));
                    pointer
                });

                let top = (drag.tick_top + pointer - grabbed).clamp(0., travel);
                handle.set_offset(point(
                    handle.offset().x,
                    px(-(f32::from(max) * top / travel)),
                ));

                if let Some(on_scroll) = &on_scroll {
                    on_scroll(window, cx);
                }
            })
            .into_any_element()
    }
}
