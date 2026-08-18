//! Everything the UI draws is compiled into the executable.
//!
//! SyncMaid ships as a single file the user can drop on a USB stick, so the app must not depend
//! on anything sitting next to it. That includes the icon set: `gpui-component` deliberately
//! embeds no SVGs of its own, and hands the job to whichever `AssetSource` the application
//! registers.
//!
//! Two sources, one namespace. `gpui-component-assets` carries the Lucide file behind every
//! `IconName`; this crate's `assets/icons` carries the dozen Lucide glyphs `IconName` has no
//! variant for — Play, Stop, Refresh, Trash, Pencil, Clock, Funnel and friends, which is to say
//! most of the verbs a sync app needs. Ours is consulted first, so a name we ship always wins.

use std::borrow::Cow;

use anyhow::anyhow;
use gpui::{AssetSource, Result, SharedString};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "assets"]
#[include = "icons/**/*.svg"]
#[include = "*.png"]
struct Local;

/// The composite source registered with `Application::with_assets`.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path.is_empty() {
            return Ok(None);
        }
        if let Some(file) = Local::get(path) {
            return Ok(Some(file.data));
        }
        // Not ours, so it is one of `IconName`'s. Their source reports a miss as an error, which
        // is the right shape here too: a name in neither set renders as a silent blank.
        gpui_component_assets::Assets
            .load(path)
            .map_err(|_| anyhow!("no embedded asset at {path:?}"))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut listed: Vec<SharedString> = Local::iter()
            .filter(|candidate| candidate.starts_with(path))
            .map(|candidate| candidate.into())
            .collect();
        listed.extend(gpui_component_assets::Assets.list(path)?);
        Ok(listed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_halves_of_the_icon_set_resolve() {
        // One name from each source, so a broken composite cannot pass by covering only its own.
        for path in ["icons/play.svg", "icons/folder.svg"] {
            let loaded = Assets
                .load(path)
                .unwrap_or_else(|error| panic!("{path}: {error}"));
            assert!(loaded.is_some(), "{path} is not embedded");
        }
    }
}
