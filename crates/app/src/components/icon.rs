//! Every glyph SyncMaid draws, named for what it means rather than what it looks like.
//!
//! The set is Lucide throughout, reached two ways. Most names resolve to a `gpui-component`
//! [`IconName`], so they cost nothing and follow the library forward. The rest resolve to an SVG
//! under `assets/icons`, because `IconName` — 86 variants — happens to have no Play, Stop,
//! Refresh, Trash, Pencil, Clock or Funnel, which is most of the verbs a sync app needs. Both
//! halves are the same icon family, so the seam is invisible.
//!
//! Implementing [`IconNamed`] is the library's own extension point: a `Glyph` goes anywhere an
//! `IconName` does — `Button::icon`, `SidebarMenuItem::icon`, `Icon::new`, `Alert::icon`.

use gpui::SharedString;
use gpui_component::{IconName, IconNamed};

/// See the module docs. Adding a variant means either finding an `IconName` for it or dropping
/// its Lucide SVG into `assets/icons`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    // --- Navigation and structure ---
    Back,
    ChevronDown,
    ChevronLeft,
    ChevronRight,
    ArrowRight,
    MoveUp,
    MoveDown,
    Close,
    /// Remove one row from a list.
    Remove,

    // --- Objects ---
    Folder,
    /// A Move destination: where files land.
    Inbox,
    /// "Keep the folder structure".
    Tree,
    /// A Move task's ordered routing table.
    Route,
    Filter,
    /// The catch-all "everything else" rule.
    Asterisk,

    // --- Verbs ---
    Run,
    Stop,
    Sync,
    Add,
    Edit,
    Trash,
    Copy,
    Settings,
    /// Show what a filter or a plan would do.
    Eye,
    Check,

    // --- Triggers ---
    /// Run only when asked.
    Manual,
    /// A schedule.
    Clock,

    // --- Delete modes ---
    /// Move to the recycle bin.
    Recycle,

    // --- Status, as a family of circles so a column of them reads as one scale ---
    Success,
    Warning,
    Failure,
    /// Never run, or finished with work left over.
    Idle,
}

impl IconNamed for Glyph {
    fn path(self) -> SharedString {
        match self {
            Self::Back => IconName::ArrowLeft.path(),
            Self::ChevronDown => IconName::ChevronDown.path(),
            Self::ChevronLeft => IconName::ChevronLeft.path(),
            Self::ChevronRight => IconName::ChevronRight.path(),
            Self::ArrowRight => IconName::ArrowRight.path(),
            Self::MoveUp => IconName::ArrowUp.path(),
            Self::MoveDown => IconName::ArrowDown.path(),
            Self::Close => IconName::Close.path(),
            Self::Remove => IconName::CircleX.path(),

            Self::Folder => IconName::Folder.path(),
            Self::Asterisk => IconName::Asterisk.path(),

            Self::Add => IconName::Plus.path(),
            Self::Copy => IconName::Copy.path(),
            Self::Settings => IconName::Settings.path(),
            Self::Eye => IconName::Eye.path(),
            Self::Check => IconName::Check.path(),

            Self::Success => IconName::CircleCheck.path(),
            Self::Warning => IconName::TriangleAlert.path(),

            // The supplements. See the module docs for why these are not `IconName`s.
            Self::Inbox => supplement("import"),
            Self::Tree => supplement("folder-tree"),
            Self::Route => supplement("split"),
            Self::Filter => supplement("funnel"),
            Self::Run => supplement("play"),
            Self::Stop => supplement("circle-stop"),
            Self::Sync => supplement("refresh-cw"),
            Self::Edit => supplement("pencil"),
            Self::Trash => supplement("trash-2"),
            Self::Manual => supplement("mouse-pointer-click"),
            Self::Clock => supplement("clock"),
            Self::Recycle => supplement("recycle"),
            Self::Failure => supplement("circle-alert"),
            Self::Idle => supplement("circle-minus"),
        }
    }
}

/// One of ours. Same `icons/` namespace as `IconName`'s, which is why the names must not collide.
fn supplement(name: &str) -> SharedString {
    format!("icons/{name}.svg").into()
}

#[cfg(test)]
mod tests {
    use gpui::AssetSource;

    use super::*;
    use crate::assets::Assets;

    /// Every glyph named here has to resolve, or it renders as a silent blank.
    #[test]
    fn every_glyph_has_an_embedded_svg() {
        for candidate in ALL {
            let path = candidate.path();
            let loaded = Assets
                .load(&path)
                .unwrap_or_else(|error| panic!("{candidate:?} ({path}): {error}"));
            assert!(loaded.is_some(), "{candidate:?} ({path}) is not embedded");
        }
    }

    /// Two glyphs pointing at one file is fine; two names for one *concept* is not, and this is
    /// where that would first show up.
    #[test]
    fn the_status_family_is_four_distinct_glyphs() {
        let paths = [
            Glyph::Success.path(),
            Glyph::Warning.path(),
            Glyph::Failure.path(),
            Glyph::Idle.path(),
        ];
        let mut unique = paths.to_vec();
        unique.sort();
        unique.dedup();
        assert_eq!(paths.len(), unique.len(), "{paths:?}");
    }

    const ALL: [Glyph; 32] = [
        Glyph::Back,
        Glyph::ChevronDown,
        Glyph::ChevronLeft,
        Glyph::ChevronRight,
        Glyph::ArrowRight,
        Glyph::MoveUp,
        Glyph::MoveDown,
        Glyph::Close,
        Glyph::Remove,
        Glyph::Folder,
        Glyph::Inbox,
        Glyph::Tree,
        Glyph::Route,
        Glyph::Filter,
        Glyph::Asterisk,
        Glyph::Run,
        Glyph::Stop,
        Glyph::Sync,
        Glyph::Add,
        Glyph::Edit,
        Glyph::Trash,
        Glyph::Copy,
        Glyph::Settings,
        Glyph::Eye,
        Glyph::Check,
        Glyph::Manual,
        Glyph::Clock,
        Glyph::Recycle,
        Glyph::Success,
        Glyph::Warning,
        Glyph::Failure,
        Glyph::Idle,
    ];
}
