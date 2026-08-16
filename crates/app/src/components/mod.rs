//! The pieces the design needs that `gpui-component` does not ship.
//!
//! Everything here is flat, light-only and drawn from the palette in [`crate::theme`], because
//! the design it is porting is.

// Each component ships its full set of tones and options; the dialogs that use the rest of
// them are the next thing to land. Drop this once they have.
#![allow(dead_code)]

mod badge;
mod button;
mod choice_card;
mod hint_box;
mod icon;
mod icon_button;
mod segment;

use gpui::{App, ClickEvent, Window};

/// What every clickable component stores. Named because the bare type is unreadable.
pub(crate) type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

pub use badge::{Badge, BadgeTone};
pub use button::{Button, ButtonTone};
pub use choice_card::ChoiceCard;
pub use hint_box::{HintBox, HintTone};
pub use icon::{icon, Icon};
pub use icon_button::{IconButton, IconButtonTone};
pub use segment::{Segment, SegmentOption};
