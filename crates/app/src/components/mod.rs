//! The app-specific pieces that sit above `ui`'s general-purpose controls.
//!
//! A glyph table ([`Glyph`], a semantic name per icon), [`ChoiceCard`], a radio option big enough
//! to explain itself, and [`ScrollRail`], Porcelain's hairline scrollbar: see each module's docs
//! for why they exist.

mod choice_card;
mod icon;
mod scroll_rail;

pub use choice_card::ChoiceCard;
pub use icon::Glyph;
pub use scroll_rail::{ScrollNotify, ScrollRail, RAIL_WIDTH};
