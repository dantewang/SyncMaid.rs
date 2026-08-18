//! The theme, and the handful of sizes that are ours rather than the component library's.
//!
//! Everything about colour now comes from `gpui-component`: `cx.theme().foreground`,
//! `cx.theme().danger`, and so on. The palette this module used to hold — sixteen `u32`
//! constants transcribed from the Avalonia build — is gone on purpose. Hard-coded colours are
//! why the app could not follow the component library forward, and why a dark mode would have
//! meant repainting every element by hand.
//!
//! What is left is `install`, plus the few fixed measurements no theme token covers.

use std::rc::Rc;

use gpui::{px, App, Pixels};
use gpui_component::{Theme, ThemeMode, ThemeSet};

/// The theme set, compiled in.
///
/// Deliberately `include_str!` rather than `ThemeRegistry::watch_dir`, which is what the
/// component library's own documentation suggests: that reads JSON from a `./themes` directory
/// at run time, and SyncMaid ships as one executable with nothing beside it. A portable app
/// that needs a folder of theme files is not portable.
const THEMES: &str = include_str!("../assets/themes/ayu.json");

/// The theme SyncMaid wears. Light only, as the Avalonia build was.
const ACTIVE: &str = "Ayu Light";

/// Applies [`ACTIVE`]. Call once, after `gpui_component::init`.
pub fn install(cx: &mut App) {
    let set: ThemeSet = match serde_json::from_str(THEMES) {
        Ok(set) => set,
        // A theme that will not parse is a cosmetic problem, not a reason to refuse to start:
        // the component library's built-in light theme is still perfectly usable.
        Err(error) => {
            tracing::error!(%error, "the bundled theme could not be read; keeping the default");
            return;
        }
    };

    match set.themes.into_iter().find(|theme| theme.name == ACTIVE) {
        Some(config) => Theme::global_mut(cx).apply_config(&Rc::new(config)),
        None => tracing::error!("the bundled theme set has no {ACTIVE:?}; keeping the default"),
    }

    // Explicit rather than relying on the default: the mode decides which of the set's two
    // themes is the one being drawn.
    Theme::change(ThemeMode::Light, None, cx);
}

/// The one type size the theme's own scale does not cover.
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
    /// The square holding a task's folder glyph.
    pub fn task_chip() -> Pixels {
        px(34.)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bundled_theme_set_contains_the_one_we_ask_for() {
        // A rename upstream would otherwise show up as the app quietly wearing the default
        // theme, which is close enough to Ayu Light to go unnoticed for a long time.
        let set: ThemeSet = serde_json::from_str(THEMES).expect("the bundled theme set parses");
        assert!(
            set.themes.iter().any(|theme| theme.name == ACTIVE),
            "no {ACTIVE:?} in {:?}",
            set.themes.iter().map(|t| &t.name).collect::<Vec<_>>()
        );
    }

    #[test]
    fn the_theme_we_wear_is_a_light_one() {
        let set: ThemeSet = serde_json::from_str(THEMES).expect("the bundled theme set parses");
        let active = set
            .themes
            .into_iter()
            .find(|theme| theme.name == ACTIVE)
            .expect("the active theme is present");
        assert!(!active.mode.is_dark());
    }
}
