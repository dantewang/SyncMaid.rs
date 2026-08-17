//! The in-window modals.
//!
//! Everything the user starts from the visible main window is a modal *inside* it: a scrim over
//! the whole window, including the title bar, with one centred card. Only one is ever open.
//!
//! The one exception is the mirror mass-delete confirmation, which is a real top-level window,
//! because it can appear while SyncMaid is hidden in the tray — inside the main window, nobody
//! would see it.

mod confirm;
mod destination_editor;
mod settings;
mod task_editor;
mod task_workspace;

pub use confirm::{ConfirmDialog, ConfirmEvent};
pub use destination_editor::DestinationEditor;
pub use settings::{SettingsDialog, SettingsEvent};
pub use task_editor::{TaskEditor, TaskEditorEvent};
pub use task_workspace::{TaskWorkspace, TaskWorkspaceEvent};

use gpui::{div, prelude::*, px, Div, Pixels};

use crate::theme;

/// The white card every modal sits on.
pub fn dialog_card(width: Pixels) -> Div {
    div()
        .flex()
        .flex_col()
        .w(width)
        .p(px(24.))
        .gap(px(16.))
        .bg(theme::color(theme::SURFACE))
        .border_1()
        .border_color(theme::color(theme::HAIRLINE))
        .rounded(theme::radius::dialog())
        .shadow_lg()
}

/// A field label: small, muted, tight to the control beneath it.
pub fn field_label(text: impl Into<gpui::SharedString>) -> Div {
    div()
        .pb(px(5.))
        .text_size(theme::text::small())
        .text_color(theme::color(theme::TEXT_SECONDARY))
        .child(text.into())
}

/// A dialog's title.
pub fn dialog_title(text: impl Into<gpui::SharedString>) -> Div {
    div()
        .text_size(theme::text::dialog_title())
        .font_weight(gpui::FontWeight::MEDIUM)
        .child(text.into())
}

/// The right-aligned button row every dialog ends with.
pub fn dialog_footer() -> Div {
    div()
        .flex()
        .flex_row()
        .justify_end()
        .items_center()
        .gap(px(10.))
        .pt(px(4.))
}
