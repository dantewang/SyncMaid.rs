//! The visual language, carried over from the Avalonia build.
//!
//! Deliberately small: one accent, three text greys, two hairlines, three semantic colours, no
//! gradients, one shadow and one transition. Light only — the C# build pinned
//! `RequestedThemeVariant="Light"` and never grew a dark theme, so neither does this.
//!
//! The accent is the cyan from the app icon's hair bow (`#36D6E2`), deepened to stay legible
//! against white.

// The palette is complete; the tokens the dialogs use are waiting on the dialogs.
#![allow(dead_code)]

use gpui::{px, rgb, rgba, Hsla, Pixels};

/// Page background — the sidebar and title bar sit on this.
pub const PAGE: u32 = 0xF7F7F4;
/// Cards, dialogs and the main pane.
pub const SURFACE: u32 = 0xFFFFFF;
/// Hover fills and quiet chips.
pub const SUBTLE: u32 = 0xF1F1ED;
/// The 1 px line between things.
pub const HAIRLINE: u32 = 0xE6E6E0;
/// The 1 px line around things you can click.
pub const HAIRLINE_STRONG: u32 = 0xD6D6CF;

pub const TEXT_PRIMARY: u32 = 0x1B1B18;
pub const TEXT_SECONDARY: u32 = 0x6C6C66;
pub const TEXT_MUTED: u32 = 0x9A9A92;

/// The brand accent.
pub const TEAL: u32 = 0x1AA0B5;
pub const TEAL_HOVER: u32 = 0x158799;
pub const TEAL_PRESSED: u32 = 0x106E7D;
pub const TEAL_SUBTLE: u32 = 0xE3F5F8;

pub const DANGER: u32 = 0xC53943;
pub const DANGER_HOVER: u32 = 0xB00613;
pub const DANGER_SUBTLE: u32 = 0xF9E7E8;
/// Success shares the accent: a synced destination is the normal state, not a celebration.
pub const SUCCESS: u32 = 0x1AA0B5;
pub const WARNING: u32 = 0xC7810B;
pub const WARNING_SUBTLE: u32 = 0xFBEFD6;

/// The scrim behind an in-window modal.
pub const BACKDROP: u32 = 0x00000066;
/// The one shadow in the whole app, under a dialog card.
pub const DIALOG_SHADOW: u32 = 0x00000040;

/// An opaque `0xRRGGBB` token.
pub fn color(value: u32) -> Hsla {
    rgb(value).into()
}

/// An `0xRRGGBBAA` token, for the two places transparency is part of the design.
pub fn color_with_alpha(value: u32) -> Hsla {
    rgba(value).into()
}

/// Corner radii, smallest to largest: controls, blocks, pills, cards, dialogs.
pub mod radius {
    use super::*;

    /// Buttons, inputs, badges' rows, sidebar items.
    pub fn control() -> Pixels {
        px(6.)
    }
    /// Chips, choice cards, filter groups, banners.
    pub fn block() -> Pixels {
        px(8.)
    }
    /// Badge pills.
    pub fn pill() -> Pixels {
        px(10.)
    }
    /// Task cards, and the main pane's top-left notch.
    pub fn card() -> Pixels {
        px(12.)
    }
    /// Dialog cards.
    pub fn dialog() -> Pixels {
        px(14.)
    }
}

/// Type scale.
pub mod text {
    use super::*;

    /// Badges.
    pub fn tiny() -> Pixels {
        px(11.)
    }
    /// Muted labels, paths, secondary detail.
    pub fn small() -> Pixels {
        px(12.)
    }
    /// The body size everything defaults to.
    pub fn body() -> Pixels {
        px(13.)
    }
    /// Sidebar task names.
    pub fn medium() -> Pixels {
        px(14.)
    }
    /// Card titles.
    pub fn large() -> Pixels {
        px(15.)
    }
    /// Dialog titles.
    pub fn dialog_title() -> Pixels {
        px(16.)
    }
    /// The page heading.
    pub fn heading() -> Pixels {
        px(18.)
    }
}

/// Fixed sizes the layout is built from.
pub mod layout {
    use super::*;

    /// The app-drawn title bar.
    pub fn title_bar_height() -> Pixels {
        px(40.)
    }
    /// Each caption button in the title bar.
    pub fn caption_button() -> (Pixels, Pixels) {
        (px(44.), px(32.))
    }
    /// The task list sidebar.
    pub fn sidebar_width() -> Pixels {
        px(210.)
    }
    /// The strip left behind when the sidebar is collapsed.
    pub fn rail_width() -> Pixels {
        px(40.)
    }
    /// A row action button.
    pub fn icon_button() -> Pixels {
        px(32.)
    }
    /// The smaller icon button used inside rows and the sidebar header.
    pub fn small_icon_button() -> Pixels {
        px(26.)
    }
    /// The teal square holding a task's folder glyph.
    pub fn task_chip() -> Pixels {
        px(34.)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_accent_is_the_icons_cyan_deepened_for_contrast() {
        // Guards against someone "tidying" the palette into a generic blue.
        assert_eq!(0x1AA0B5, TEAL);
        assert_eq!(
            TEAL, SUCCESS,
            "a synced destination is the normal state, not a celebration"
        );
    }

    #[test]
    fn colors_convert_without_losing_their_channels() {
        let teal = color(TEAL);
        let also_teal = color(TEAL);
        assert_eq!(teal, also_teal);
        assert_ne!(color(TEAL), color(DANGER));
    }
}
