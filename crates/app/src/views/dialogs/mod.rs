//! The editors and the confirmations, all of them hosted by `gpui-component`.
//!
//! `Root` — installed on the main window — owns a sheet slot and a dialog stack.
//! `Window::open_sheet` puts the two editing surfaces in a full-height panel on the right;
//! `Window::open_dialog` puts a confirmation in a centred card. Either way the frame, the
//! scrim, the title, the focus trap and Esc come from the library, and what lives here is only
//! each surface's contents. Field labels come from `gpui_component::form`.
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
