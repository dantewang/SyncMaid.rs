//! Everything the UI draws is compiled into the executable.
//!
//! SyncMaid ships as a single file the user can drop on a USB stick, so the app must not depend
//! on anything sitting next to it. That includes the icon set: `gpui-component` deliberately
//! embeds no SVGs of its own, and hands the job to whichever `AssetSource` the application
//! registers.
//!
//! Every icon comes from this crate's `assets/icons`, and nothing else is consulted. That covers
//! both the glyphs the app draws itself (see `components::icon`) and the handful the library
//! draws on its own: the sheet's close button, the alert and checkbox icons. The library asks for
//! those by `IconName`, which is only a path, so the file names have to match its catalogue.
//! gpui-kit's own `assets` bundle stays switched off. It embeds a hundred icons to cover what
//! the dozen here already do.

use std::borrow::Cow;

use anyhow::anyhow;
use gpui_kit::{AssetSource, Result, SharedString};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "assets"]
#[include = "icons/**/*.svg"]
#[include = "*.png"]
struct Local;

/// The source registered with `Application::with_assets`.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path.is_empty() {
            return Ok(None);
        }
        // A miss is an error rather than `None`, the shape the library's own source used: a name
        // that is not embedded renders as a silent blank, and the error is the only trace of it.
        Local::get(path)
            .map(|file| Some(file.data))
            .ok_or_else(|| anyhow!("no embedded asset at {path:?}"))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(Local::iter()
            .filter(|candidate| candidate.starts_with(path))
            .map(|candidate| candidate.into())
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::component::{IconName, IconNamed as _};

    use super::*;

    #[test]
    fn the_icons_the_library_draws_by_itself_are_embedded() {
        // Not reached through `Glyph`, so `every_glyph_has_an_embedded_svg` cannot see them:
        // `Sheet`'s close button, `Alert::new`/`warning`/`error`, and `Checkbox`.
        for icon in [
            IconName::Close,
            IconName::Info,
            IconName::TriangleAlert,
            IconName::CircleX,
            IconName::Check,
        ] {
            let path = icon.path();
            let loaded = Assets
                .load(&path)
                .unwrap_or_else(|error| panic!("{path}: {error}"));
            assert!(loaded.is_some(), "{path} is not embedded");
        }
    }
}
