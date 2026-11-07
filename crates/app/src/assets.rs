//! Everything the UI draws is compiled into the executable.
//!
//! SyncMaid ships as a single file the user can drop on a USB stick, so the app must not depend
//! on anything sitting next to it. That includes the icon set: gpui draws an SVG by asking the
//! registered `AssetSource` for its bytes, and this is that source.
//!
//! Every icon is this crate's own, under `assets/icons`, and every one is reached through
//! `components::Glyph` — whose test checks that each name has a file. `build/assets.rs` turns
//! the folder into `FILES`, a sorted table of `include_bytes!`.

use std::borrow::Cow;

use anyhow::anyhow;
use gpui_kit::{AssetSource, Result, SharedString};

include!(concat!(env!("OUT_DIR"), "/assets.rs"));

/// The source registered with `Application::with_assets`.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path.is_empty() {
            return Ok(None);
        }
        // A miss is an error rather than `None`: a name that is not embedded renders as a silent
        // blank, and the error is the only trace of it.
        FILES
            .binary_search_by_key(&path, |(name, _)| name)
            .map(|index| Some(Cow::Borrowed(FILES[index].1)))
            .map_err(|_| anyhow!("no embedded asset at {path:?}"))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(FILES
            .iter()
            .filter(|(name, _)| name.starts_with(path))
            .map(|(name, _)| SharedString::from(*name))
            .collect())
    }
}
