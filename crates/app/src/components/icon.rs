//! The icon set, lifted from the same Material Design Icons package the C# build drew from, so
//! every glyph is the one that shipped rather than a lookalike.

use gpui::{svg, Hsla, Pixels, SharedString, Styled as _, Svg};

/// Every icon SyncMaid draws. Adding one means adding its SVG under `assets/icons`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    AlertCircle,
    AlertOutline,
    ArrowDown,
    ArrowRight,
    ArrowUp,
    Asterisk,
    AsteriskCircleOutline,
    CallSplit,
    Cancel,
    Check,
    CheckCircle,
    ChevronDown,
    ChevronLeft,
    ChevronRight,
    ClockOutline,
    Close,
    CogOutline,
    ContentCopy,
    CursorDefaultClickOutline,
    DeleteForeverOutline,
    Eye,
    EyeOutline,
    FileTree,
    FilterOutline,
    FolderOutline,
    MinusCircle,
    Pencil,
    Play,
    Plus,
    RecycleVariant,
    Stop,
    Sync,
    TrashCanOutline,
    TrayArrowDown,
    WindowClose,
    WindowMaximize,
    WindowMinimize,
    WindowRestore,
}

impl Icon {
    /// Where the embedded SVG lives. The window-control names match what `gpui-component`'s
    /// title bar asks for, which is why those four are spelled `window-*`.
    pub fn path(self) -> SharedString {
        let name = match self {
            Self::AlertCircle => "alert-circle",
            Self::AlertOutline => "alert-outline",
            Self::ArrowDown => "arrow-down",
            Self::ArrowRight => "arrow-right",
            Self::ArrowUp => "arrow-up",
            Self::Asterisk => "asterisk",
            Self::AsteriskCircleOutline => "asterisk-circle-outline",
            Self::CallSplit => "call-split",
            Self::Cancel => "cancel",
            Self::Check => "check",
            Self::CheckCircle => "check-circle",
            Self::ChevronDown => "chevron-down",
            Self::ChevronLeft => "chevron-left",
            Self::ChevronRight => "chevron-right",
            Self::ClockOutline => "clock-outline",
            Self::Close => "close",
            Self::CogOutline => "cog-outline",
            Self::ContentCopy => "content-copy",
            Self::CursorDefaultClickOutline => "cursor-default-click-outline",
            Self::DeleteForeverOutline => "delete-forever-outline",
            Self::Eye => "eye",
            Self::EyeOutline => "eye-outline",
            Self::FileTree => "file-tree",
            Self::FilterOutline => "filter-outline",
            Self::FolderOutline => "folder-outline",
            Self::MinusCircle => "minus-circle",
            Self::Pencil => "pencil",
            Self::Play => "play",
            Self::Plus => "plus",
            Self::RecycleVariant => "recycle-variant",
            Self::Stop => "stop",
            Self::Sync => "sync",
            Self::TrashCanOutline => "trash-can-outline",
            Self::TrayArrowDown => "tray-arrow-down",
            Self::WindowClose => "window-close",
            Self::WindowMaximize => "window-maximize",
            Self::WindowMinimize => "window-minimize",
            Self::WindowRestore => "window-restore",
        };
        format!("icons/{name}.svg").into()
    }
}

/// Draws `icon` at `size`, tinted `color`.
///
/// GPUI renders an SVG as a coverage mask and paints it in the element's text colour, so the
/// icon takes the colour it is given rather than the one in the file.
pub fn icon(icon: Icon, size: Pixels, color: Hsla) -> Svg {
    svg().path(icon.path()).size(size).text_color(color)
}

#[cfg(test)]
mod tests {
    use gpui::AssetSource;

    use super::*;
    use crate::assets::Assets;

    /// Every icon named here has to exist, or it renders as a silent blank.
    #[test]
    fn every_icon_has_an_embedded_svg() {
        let all = [
            Icon::AlertCircle,
            Icon::AlertOutline,
            Icon::ArrowDown,
            Icon::ArrowRight,
            Icon::ArrowUp,
            Icon::Asterisk,
            Icon::AsteriskCircleOutline,
            Icon::CallSplit,
            Icon::Cancel,
            Icon::Check,
            Icon::CheckCircle,
            Icon::ChevronDown,
            Icon::ChevronLeft,
            Icon::ChevronRight,
            Icon::ClockOutline,
            Icon::Close,
            Icon::CogOutline,
            Icon::ContentCopy,
            Icon::CursorDefaultClickOutline,
            Icon::DeleteForeverOutline,
            Icon::Eye,
            Icon::EyeOutline,
            Icon::FileTree,
            Icon::FilterOutline,
            Icon::FolderOutline,
            Icon::MinusCircle,
            Icon::Pencil,
            Icon::Play,
            Icon::Plus,
            Icon::RecycleVariant,
            Icon::Stop,
            Icon::Sync,
            Icon::TrashCanOutline,
            Icon::TrayArrowDown,
            Icon::WindowClose,
            Icon::WindowMaximize,
            Icon::WindowMinimize,
            Icon::WindowRestore,
        ];

        for candidate in all {
            let path = candidate.path();
            let loaded = Assets
                .load(&path)
                .unwrap_or_else(|error| panic!("{path}: {error}"));
            assert!(loaded.is_some(), "{path} is not embedded");
        }
    }

    #[test]
    fn the_title_bars_icons_use_the_names_gpui_component_asks_for() {
        assert_eq!("icons/window-close.svg", Icon::WindowClose.path().as_ref());
        assert_eq!(
            "icons/window-minimize.svg",
            Icon::WindowMinimize.path().as_ref()
        );
        assert_eq!(
            "icons/window-maximize.svg",
            Icon::WindowMaximize.path().as_ref()
        );
        assert_eq!(
            "icons/window-restore.svg",
            Icon::WindowRestore.path().as_ref()
        );
    }
}
