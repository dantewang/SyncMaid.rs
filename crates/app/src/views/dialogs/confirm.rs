//! The wording of the two destructive confirmations.
//!
//! Not a view any more: `gpui_component::dialog::Dialog` draws the card, the title and the button
//! row, so all that is left is deciding what the question says and how loud the accept button
//! should be.
//!
//! The footer is spelled out rather than taken from `Dialog::confirm()`, and that is the point.
//! `confirm()` wires the accept action to `on_ok`, which the library also binds to Enter — and a
//! destructive confirm a stray return key can approve is not a confirm. Leaving `on_ok` unset
//! keeps Esc cancelling (the library gates both on one `keyboard` flag) while reducing Enter to
//! "close", which is the safe direction.

use std::rc::Rc;

use gpui::{div, prelude::*, px, App, SharedString, Window};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::{ActiveTheme as _, WindowExt as _};

use crate::strings;

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
                .close_button(false)
                .overlay_closable(false)
                .footer(move |_, _, _, _| {
                    let accepted = Rc::clone(&accepted);
                    vec![
                        Button::new("confirm-cancel")
                            .label(strings::common_cancel())
                            .outline()
                            .on_click(|_, window, cx| window.close_dialog(cx))
                            .into_any_element(),
                        Button::new("confirm-ok")
                            .label(confirm_label.clone())
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
                            })
                            .into_any_element(),
                    ]
                })
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
