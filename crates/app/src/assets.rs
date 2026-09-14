//! Everything the UI draws is compiled into the executable.
//!
//! SyncMaid ships as a single file the user can drop on a USB stick, so the app must not depend
//! on anything sitting next to it. That includes the icon set: gpui draws an SVG by asking the
//! registered `AssetSource` for its bytes, and this is that source.
//!
//! Every icon is this crate's own, under `assets/icons`, and every one is reached through
//! `components::Glyph` — whose test checks that each name has a file.

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
        // A miss is an error rather than `None`: a name that is not embedded renders as a silent
        // blank, and the error is the only trace of it.
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
