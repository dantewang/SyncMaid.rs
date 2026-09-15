use gpui_kit::{div, prelude::*, px, App, Div, Pixels};

use crate::theme::ActiveTheme as _;

/// How wide the band is.
const WIDTH: Pixels = px(2.);

/// The mark a focused control wears: a faint band of the ring colour just inside its border.
///
/// Add it as a child of a `relative()` control. An absolute child is laid out inside its parent's
/// border, so the band lines the border from within and never reaches past the control — nothing
/// beside it can cover it and no clipping ancestor can cut it. `radius` is the control's corner
/// radius. Add it before the control's content, so the content paints over it.
pub fn focus_ring(radius: Pixels, cx: &App) -> Div {
    div()
        .absolute()
        .inset_0()
        .border(WIDTH)
        .border_color(cx.theme().ring.opacity(0.25))
        .rounded(radius)
}
