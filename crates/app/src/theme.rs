//! The palette, and the handful of sizes that are ours rather than a component's.
//!
//! Every colour the UI draws comes from [`Theme`], through `cx.theme()`. The palette lives here in
//! Rust rather than in a JSON theme file: nothing reads a theme at run time any more, and a
//! compiled-in table is one less format to keep in step with the code that uses it.
//!
//! `install` also projects the palette onto gpui-base's own theme, which is what base-owned
//! parts — the scrollbar, an input's text and caret — paint from.

use gpui_kit::base::{
    ColorTokens, RadiusTokens, ScrollbarStyles, ScrollbarTheme, Theme as BaseTheme,
};
use gpui_kit::{px, rgb, rgba, App, Global, Hsla, Pixels, SharedString};

/// Porcelain: flat, square, ink on a pale ground, with teal meaning "happening now".
#[derive(Clone, Debug)]
pub struct Theme {
    pub background: Hsla,
    pub foreground: Hsla,
    pub border: Hsla,
    /// The scrim behind a sheet or a dialog.
    pub overlay: Hsla,

    pub muted: Hsla,
    pub muted_foreground: Hsla,
    pub accent: Hsla,

    pub secondary: Hsla,
    pub secondary_foreground: Hsla,
    pub secondary_active: Hsla,

    pub primary: Hsla,
    pub primary_foreground: Hsla,
    pub primary_hover: Hsla,
    pub primary_active: Hsla,

    pub danger: Hsla,
    pub danger_foreground: Hsla,
    pub danger_hover: Hsla,
    pub danger_active: Hsla,

    pub warning: Hsla,
    pub warning_foreground: Hsla,
    pub success: Hsla,

    /// A focused control's edge.
    pub ring: Hsla,
    pub caret: Hsla,
    pub selection: Hsla,
    /// An input's resting edge.
    pub input: Hsla,
    pub link: Hsla,

    pub sidebar: Hsla,
    pub sidebar_border: Hsla,
    pub sidebar_accent: Hsla,
    pub list_active: Hsla,

    pub switch: Hsla,
    pub switch_thumb: Hsla,
    pub scrollbar_thumb: Hsla,
    pub scrollbar_thumb_hover: Hsla,

    pub font_family: SharedString,
    pub mono_font_family: SharedString,
    pub font_size: Pixels,
    pub radius: Pixels,
    pub radius_lg: Pixels,
}

impl Global for Theme {}

fn hex(value: u32) -> Hsla {
    rgb(value).into()
}

impl Theme {
    /// The one theme SyncMaid wears. Light only.
    pub fn porcelain() -> Self {
        Self {
            background: hex(0xF8FAFB),
            foreground: hex(0x191C22),
            border: hex(0xC2CBD1),
            overlay: rgba(0x191C2247).into(),

            muted: hex(0xE7EAEC),
            muted_foreground: hex(0x7C878F),
            accent: hex(0xDEF0F4),

            secondary: hex(0xE7EAEC),
            secondary_foreground: hex(0x5B666E),
            secondary_active: hex(0xD9DFE3),

            primary: hex(0x191C22),
            primary_foreground: hex(0xF2F6F7),
            primary_hover: hex(0x116F8E),
            primary_active: hex(0x0C4C61),

            danger: hex(0xC62F3C),
            danger_foreground: hex(0xF2F6F7),
            danger_hover: hex(0xD34450),
            danger_active: hex(0xA82430),

            warning: hex(0x8A6410),
            warning_foreground: hex(0xFBF1DC),
            success: hex(0x191C22),

            ring: hex(0x116F8E),
            caret: hex(0x191C22),
            selection: hex(0xA5E5EF),
            input: hex(0xC2CBD1),
            link: hex(0x116F8E),

            sidebar: hex(0xD9DFE3),
            sidebar_border: hex(0xC2CBD1),
            sidebar_accent: hex(0xF8FAFB),
            list_active: hex(0xDEF0F4),

            switch: hex(0xC2CBD1),
            switch_thumb: hex(0xF8FAFB),
            scrollbar_thumb: rgba(0x5B666E66).into(),
            scrollbar_thumb_hover: rgba(0x5B666EAA).into(),

            font_family: ".SystemUIFont".into(),
            mono_font_family: "Consolas".into(),
            font_size: px(16.),
            radius: px(0.),
            radius_lg: px(0.),
        }
    }
}

/// `cx.theme()`.
pub trait ActiveTheme {
    fn theme(&self) -> &Theme;
}

impl ActiveTheme for App {
    #[inline]
    fn theme(&self) -> &Theme {
        self.global::<Theme>()
    }
}

/// Installs the palette. Call once, after `gpui_kit::init`.
pub fn install(cx: &mut App) {
    let theme = Theme::porcelain();

    let base = BaseTheme::global_mut(cx);
    base.tokens.colors = ColorTokens {
        background: theme.background,
        foreground: theme.foreground,
        surface: theme.background,
        surface_foreground: theme.foreground,
        primary: theme.primary,
        primary_foreground: theme.primary_foreground,
        secondary: theme.secondary,
        secondary_foreground: theme.secondary_foreground,
        muted: theme.muted,
        muted_foreground: theme.muted_foreground,
        accent: theme.accent,
        accent_foreground: theme.foreground,
        destructive: theme.danger,
        destructive_foreground: theme.danger_foreground,
        border: theme.border,
        input: theme.input,
        ring: theme.ring,
        selection: theme.selection,
    };
    base.tokens.radius = RadiusTokens {
        none: px(0.),
        sm: theme.radius,
        md: theme.radius,
        lg: theme.radius_lg,
        xl: theme.radius_lg,
        full: theme.radius,
    };
    base.tokens.typography.sans = theme.font_family.clone();
    base.tokens.typography.mono = theme.mono_font_family.clone();
    base.scrollbar = ScrollbarTheme::new().with_styles(
        ScrollbarStyles::default()
            .track(|style| style.bg(gpui_kit::transparent_black()))
            .thumb(|style| style.bg(theme.scrollbar_thumb).radius(theme.radius))
            .thumb_hover(|style| style.bg(theme.scrollbar_thumb_hover).radius(theme.radius))
            .thumb_active(|style| style.bg(theme.scrollbar_thumb_hover).radius(theme.radius)),
    );

    cx.set_global(theme);
}

/// The one type size the default scale does not cover.
pub mod text {
    use super::*;

    /// The page heading. Larger than `text_lg`, which is what a card title uses.
    pub fn heading() -> Pixels {
        px(18.)
    }
}

/// Fixed sizes the layout is built from.
pub mod layout {
    use super::*;

    /// The task list sidebar.
    pub fn sidebar_width() -> Pixels {
        px(210.)
    }
}
