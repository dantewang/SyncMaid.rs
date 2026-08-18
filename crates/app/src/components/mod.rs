//! The one piece of chrome `gpui-component` does not ship.
//!
//! Everything else that used to live here — the buttons, badges, hint boxes, segmented pickers
//! and checkboxes — is gone, replaced by the library's own. What is left is a glyph table
//! ([`Glyph`], a semantic name per icon) and [`ChoiceCard`], which has no built-in counterpart:
//! see its module docs for why it is worth keeping rather than flattening into a `RadioGroup`.

mod choice_card;
mod icon;

pub use choice_card::ChoiceCard;
pub use icon::Glyph;
