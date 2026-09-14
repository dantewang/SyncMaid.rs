//! The wording of the two destructive confirmations.
//!
//! Not a view any more: `ui::Dialog` draws the card and the title, so all that is left is
//! deciding what the question says and how loud the accept button should be.
//!
//! Accepting is only ever the accept button. The dialog binds Esc and Enter, but both of them just
//! close it — and a destructive confirm a stray return key could approve would not be a confirm.

use std::rc::Rc;

use gpui_kit::{div, prelude::*, px, App, SharedString, Window};

use crate::strings;
use crate::theme::ActiveTheme as _;
use crate::ui::{h_flex, Button, WindowExt as _};

/// A confirmation the user asked for by clicking something destructive.
#[derive(Debug, Clone)]
pub struct Prompt {
    title: SharedString,
    message: SharedString,
    confirm_label: SharedString,
    destructive: bool,
}

impl Prompt {
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

    /// Puts the question on screen. `accepted` runs only when the accept button is pressed.
    pub fn open(
        self,
        window: &mut Window,
        cx: &mut App,
        accepted: impl Fn(&mut Window, &mut App) + 'static,
    ) {
        // The dialog builder is called once per frame, so anything it hands out has to be
        // shareable rather than moved.
        let accepted = Rc::new(accepted);

        window.open_dialog(cx, move |dialog, _, cx| {
            let destructive = self.destructive;
            let confirm_label = self.confirm_label.clone();
            let accepted = Rc::clone(&accepted);

            dialog
                .w(px(400.))
                .title(self.title.clone())
                .overlay_closable(false)
                .footer(
                    h_flex()
                        .gap_2()
                        .justify_end()
                        .child(
                            Button::new("confirm-cancel")
                                .label(strings::common_cancel())
                                .outline()
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new("confirm-ok")
                                .label(confirm_label)
                                .map(|button| {
                                    if destructive {
                                        button.danger()
                                    } else {
                                        button.primary()
                                    }
                                })
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    accepted(window, cx);
                                }),
                        ),
                )
                .child(
                    div()
                        .text_color(cx.theme().muted_foreground)
                        .child(self.message.clone()),
                )
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deleting_a_task_says_what_goes_with_it_and_what_does_not() {
        let prompt = Prompt::delete_task("Photos", 2);

        assert!(prompt.message.contains("\"Photos\""));
        assert!(prompt.message.contains("its 2 destinations"));
        assert!(
            prompt.message.contains("files at both ends are left alone"),
            "the commonest fear on this button is that it deletes the files"
        );
    }

    #[test]
    fn the_destination_count_reads_naturally_at_one() {
        assert!(Prompt::delete_task("Photos", 1)
            .message
            .contains("its 1 destination?"));
    }

    #[test]
    fn a_destructive_confirm_is_destructive_unless_told_otherwise() {
        assert!(Prompt::delete_destination("NAS").destructive);
        assert!(!Prompt::delete_destination("NAS").benign().destructive);
    }
}
