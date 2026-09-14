//! Every glyph SyncMaid draws, named for what it means rather than what it looks like.
//!
//! The set is Lucide throughout, and every file is ours, under `assets/icons`. Nothing resolves
//! through gpui-kit's icon bundle, which is not linked. File names follow the Lucide names the
//! library's own `IconName` uses, because the library draws a few icons itself and asks for them
//! by those names (see `assets.rs`).
//!
//! Implementing [`IconNamed`] is the library's own extension point: a `Glyph` goes anywhere an
//! `IconName` does — `Button::icon`, `SidebarMenuItem::icon`, `Icon::new`, `Alert::icon`.

use gpui_kit::component::IconNamed;
use gpui_kit::SharedString;

/// See the module docs. Adding a variant means dropping its Lucide SVG into `assets/icons`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    // --- Navigation and structure ---
    ChevronDown,
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
    /// The activity log.
    Log,

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
    /// Hand something to whatever the OS opens it with.
    OpenExternal,

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
        let name = match self {
            Self::ChevronDown => "chevron-down",
            Self::ChevronRight => "chevron-right",
            Self::ArrowRight => "arrow-right",
            Self::MoveUp => "arrow-up",
            Self::MoveDown => "arrow-down",
            Self::Close => "close",
            Self::Remove => "circle-x",

            Self::Folder => "folder",
            Self::Inbox => "import",
            Self::Tree => "folder-tree",
            Self::Route => "split",
            Self::Filter => "funnel",
            Self::Asterisk => "asterisk",
            Self::Log => "file",

            Self::Run => "play",
            Self::Stop => "circle-stop",
            Self::Sync => "refresh-cw",
            Self::Add => "plus",
            Self::Edit => "pencil",
            Self::Trash => "trash-2",
            Self::Copy => "copy",
            Self::Settings => "settings",
            Self::Eye => "eye",
            Self::Check => "check",
            Self::OpenExternal => "external-link",

            Self::Manual => "mouse-pointer-click",
            Self::Clock => "clock",

            Self::Recycle => "recycle",

            Self::Success => "circle-check",
            Self::Warning => "triangle-alert",
            Self::Failure => "circle-alert",
            Self::Idle => "circle-minus",
        };
        format!("icons/{name}.svg").into()
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::AssetSource;

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
        Glyph::ChevronDown,
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
        Glyph::Log,
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
        Glyph::OpenExternal,
        Glyph::Manual,
        Glyph::Clock,
        Glyph::Recycle,
        Glyph::Success,
        Glyph::Warning,
        Glyph::Failure,
        Glyph::Idle,
    ];
}
