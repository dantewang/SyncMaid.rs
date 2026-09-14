//! SyncMaid's own components, drawn over gpui-base.
//!
//! gpui-base owns behaviour — focus, keyboard activation, focus traps, dismissal, text editing —
//! and deliberately draws nothing. What is here is the presentation, in Porcelain's terms, for
//! exactly the controls the app uses, and with the builder names its views already call. Every
//! colour comes from [`crate::theme`].
//!
//! [`Root`] is the one piece with state: it holds the open sheet and the dialog stack, and the
//! application's own root view renders both layers.

mod alert;
mod button;
mod dialog;
mod form;
mod icon;
mod input;
mod link;
mod root;
mod sheet;
mod tag;
mod toggle;
mod tooltip;

pub use alert::Alert;
pub use button::{Button, ButtonGroup, Size};
pub use dialog::Dialog;
pub use form::{field, v_form, Field};
pub use gpui_kit::base::{h_flex, v_flex, Scrollbar, ScrollbarMode};
pub use icon::Icon;
pub use input::{Input, InputState};
pub use link::Link;
pub use root::{Root, WindowExt};
pub use sheet::Sheet;
pub use tag::Tag;
pub use toggle::{Checkbox, Switch};
pub use tooltip::Tooltip;

use gpui_kit::App;

/// Key bindings owned by this layer. Call once, after `gpui_kit::init`.
pub fn init(cx: &mut App) {
    root::init(cx);
}
