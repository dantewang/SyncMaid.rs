//! The two pieces of chrome `gpui-component` does not ship — and the one it ships wrong.
//!
//! Everything else that used to live here — the buttons, badges, hint boxes, segmented pickers
//! and checkboxes — is gone, replaced by the library's own. What is left is a glyph table
//! ([`Glyph`], a semantic name per icon), [`ChoiceCard`], which has no built-in counterpart, and
//! [`ScrollRail`], which replaces a library widget that cannot be restyled: see each module's
//! docs for why.

mod choice_card;
mod icon;
mod scroll_rail;

pub use choice_card::ChoiceCard;
pub use icon::Glyph;
pub use scroll_rail::{ScrollNotify, ScrollRail, RAIL_WIDTH};
