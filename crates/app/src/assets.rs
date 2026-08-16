//! Everything the UI draws is compiled into the executable.
//!
//! SyncMaid ships as a folder the user can drop on a USB stick, so the app must not depend on
//! files sitting next to it. `gpui-component` looks icons up by path through this source, which
//! is why the SVG names match its `IconName` mapping exactly.

use std::borrow::Cow;

use anyhow::anyhow;
use gpui::{AssetSource, Result, SharedString};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "assets"]
#[include = "icons/**/*.svg"]
#[include = "fonts/**/*"]
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path.is_empty() {
            return Ok(None);
        }
        Self::get(path)
            .map(|file| Some(file.data))
            .ok_or_else(|| anyhow!("no embedded asset at {path:?}"))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(Self::iter()
            .filter(|candidate| candidate.starts_with(path))
            .map(|candidate| candidate.into())
            .collect())
    }
}
