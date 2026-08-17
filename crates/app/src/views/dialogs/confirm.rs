//! The yes/no modal, used for the two destructive actions.

use gpui::{div, prelude::*, Context, EventEmitter, SharedString, Window};

use crate::components::{Button, ButtonTone};
use crate::strings;
use crate::theme;
use crate::views::dialogs::{dialog_card, dialog_footer, dialog_title};

/// What the user decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmEvent {
    Confirmed,
    Cancelled,
}

/// A confirmation the user asked for by clicking something destructive.
pub struct ConfirmDialog {
    title: SharedString,
    message: SharedString,
    confirm_label: SharedString,
    destructive: bool,
}

impl ConfirmDialog {
    pub fn new(
        title: impl Into<SharedString>,
        message: impl Into<SharedString>,
        confirm_label: impl Into<SharedString>,
    ) -> Self {
        Self {
            title: title.into(),
            message: message.into(),
            confirm_label: confirm_label.into(),
            destructive: true,
        }
    }

    /// Deleting a task, with the count of what goes with it.
    pub fn delete_task(name: &str, destinations: usize) -> Self {
        Self::new(
            strings::main_delete_task_title(),
            strings::main_delete_task_message_format(
                name,
                strings::main_delete_task_suffix(destinations as i64),
            ),
            strings::main_delete_task_confirm(),
        )
    }

    /// Deleting one destination from a task.
    pub fn delete_destination(name: &str) -> Self {
        Self::new(
            strings::task_delete_destination_title(),
            strings::task_delete_destination_message_format(name),
            strings::task_delete_destination_confirm(),
        )
    }

    /// A confirmation that is not about removing anything.
    pub fn benign(mut self) -> Self {
        self.destructive = false;
        self
    }

    fn confirm(&mut self, cx: &mut Context<Self>) {
        cx.emit(ConfirmEvent::Confirmed);
    }

    fn cancel(&mut self, cx: &mut Context<Self>) {
        cx.emit(ConfirmEvent::Cancelled);
    }
}

impl EventEmitter<ConfirmEvent> for ConfirmDialog {}

impl Render for ConfirmDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tone = if self.destructive {
            ButtonTone::Danger
        } else {
            ButtonTone::Primary
        };

        dialog_card(gpui::px(400.))
            .gap(gpui::px(14.))
            .child(dialog_title(self.title.clone()))
            .child(
                div()
                    .text_color(theme::color(theme::TEXT_SECONDARY))
                    .child(self.message.clone()),
            )
            .child(
                dialog_footer()
                    .child(
                        Button::new("confirm-cancel", strings::common_cancel())
                            .tone(ButtonTone::Secondary)
                            .on_click(cx.listener(|dialog, _, _, cx| dialog.cancel(cx))),
                    )
                    // Enter is deliberately not bound to this button. A destructive confirm
                    // that a stray return key can accept is not a confirm.
                    .child(
                        Button::new("confirm-ok", self.confirm_label.clone())
                            .tone(tone)
                            .on_click(cx.listener(|dialog, _, _, cx| dialog.confirm(cx))),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deleting_a_task_says_what_goes_with_it_and_what_does_not() {
        let dialog = ConfirmDialog::delete_task("Photos", 2);

        assert!(dialog.message.contains("\"Photos\""));
        assert!(dialog.message.contains("its 2 destinations"));
        assert!(
            dialog.message.contains("files at both ends are left alone"),
            "the commonest fear on this button is that it deletes the files"
        );
    }

    #[test]
    fn the_destination_count_reads_naturally_at_one() {
        assert!(ConfirmDialog::delete_task("Photos", 1)
            .message
            .contains("its 1 destination?"));
    }

    #[test]
    fn a_destructive_confirm_is_destructive_unless_told_otherwise() {
        assert!(ConfirmDialog::delete_destination("NAS").destructive);
        assert!(
            !ConfirmDialog::delete_destination("NAS")
                .benign()
                .destructive
        );
    }
}
