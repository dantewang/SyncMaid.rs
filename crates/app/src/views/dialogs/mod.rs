//! The modals, all of them on `gpui-component`'s dialog layer.
//!
//! `Root` — installed on the main window — owns a stack of dialogs; `Window::open_dialog` pushes
//! one. So the card, the scrim, the title, the button row, the focus trap and Esc all come from
//! `gpui_component::dialog::Dialog`, and what lives here is only each dialog's contents. Field
//! labels come from `gpui_component::form`.
//!
//! The one exception is the mirror mass-delete confirmation, which is a real top-level window,
//! because it can appear while SyncMaid is hidden in the tray — inside the main window, nobody
//! would see it.

mod confirm;
mod destination_editor;
mod task_editor;
mod task_workspace;

pub use confirm::Prompt;
pub use destination_editor::DestinationEditor;
pub use task_editor::{TaskEditor, TaskEditorEvent};
pub use task_workspace::{TaskWorkspace, TaskWorkspaceEvent};
