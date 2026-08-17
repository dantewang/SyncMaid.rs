//! The settings page.
//!
//! Every switch here applies the moment it is flipped. There is no save step: a settings page
//! that needs one invites the user to wonder whether the last change took.

use std::path::PathBuf;

use gpui::{div, prelude::*, px, Context, EventEmitter, Window};
use syncmaid_core::model::AppSettings;

use crate::components::{
    Button, ButtonTone, Checkbox, HintBox, HintTone, Icon, Segment, SegmentOption,
};
use crate::platform::autostart::{auto_start, AutoStart, AutoStartState};
use crate::views::dialogs::{dialog_card, dialog_footer, dialog_title, field_label};
use crate::{i18n, strings, theme};

/// What the settings page changed.
pub enum SettingsEvent {
    /// Apply and persist these. Emitted on every toggle.
    Changed(AppSettings),
    Closed,
}

/// See the module docs.
pub struct SettingsDialog {
    settings: AppSettings,
    auto_start: Box<dyn AutoStart>,
    auto_start_state: AutoStartState,
    data_directory: PathBuf,
    version: &'static str,
}

impl SettingsDialog {
    pub fn new(settings: AppSettings, data_directory: PathBuf) -> Self {
        let auto_start = auto_start();
        Self {
            settings,
            auto_start_state: auto_start.state(),
            auto_start,
            data_directory,
            version: env!("SYNCMAID_VERSION"),
        }
    }

    fn toggle_auto_start(&mut self, cx: &mut Context<Self>) {
        // Windows' own switch wins. Fighting it from here is exactly the override that makes an
        // app look like something to be suspicious of.
        if self.auto_start_state == AutoStartState::DisabledByWindows {
            return;
        }

        let result = if self.auto_start_state == AutoStartState::Enabled {
            self.auto_start.disable()
        } else {
            self.auto_start.enable()
        };
        if let Err(error) = result {
            tracing::error!(%error, "could not change the autostart registration");
        }

        // Re-read rather than assume: the registry is the single source of truth.
        self.auto_start_state = self.auto_start.state();
        cx.notify();
    }

    fn change(&mut self, apply: impl FnOnce(&mut AppSettings), cx: &mut Context<Self>) {
        apply(&mut self.settings);
        cx.emit(SettingsEvent::Changed(self.settings.clone()));
        cx.notify();
    }

    /// Which entry the language row has selected. Index 0 is "system default", so the shipped
    /// languages start at 1.
    fn language_index(&self) -> usize {
        match &self.settings.language {
            None => 0,
            Some(tag) => i18n::languages()
                .iter()
                .position(|language| language.tag == tag)
                .map_or(0, |index| index + 1),
        }
    }

    /// Switches language immediately — no restart, no "apply".
    ///
    /// Nothing is cached, so the whole app changing language is one redraw of every window.
    fn choose_language(&mut self, index: usize, cx: &mut Context<Self>) {
        let tag = index
            .checked_sub(1)
            .and_then(|index| i18n::languages().get(index))
            .map(|language| language.tag.to_owned());

        i18n::set_language(tag.as_deref());
        self.change(|settings| settings.language = tag, cx);
        cx.refresh_windows();
    }
}

impl EventEmitter<SettingsEvent> for SettingsDialog {}

impl Render for SettingsDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let available = window.viewport_size().height - px(64.);
        let auto_start_on = self.auto_start_state == AutoStartState::Enabled;
        let blocked_by_windows = self.auto_start_state == AutoStartState::DisabledByWindows;

        dialog_card(px(440.))
            .max_h(available)
            .child(dialog_title(strings::settings_title()))
            .child(
                div()
                    .id("settings-body")
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(self.render_language(cx))
                    .child(
                        div()
                            .child(field_label(strings::settings_startup_label()))
                            .child(
                                Checkbox::new(
                                    "start-with-windows",
                                    strings::settings_start_with_windows(),
                                    auto_start_on,
                                )
                                .description(strings::settings_start_with_windows_desc())
                                .disabled(blocked_by_windows)
                                .on_toggle(
                                    cx.listener(|dialog, _, _, cx| dialog.toggle_auto_start(cx)),
                                ),
                            )
                            .when(blocked_by_windows, |element| {
                                element.child(
                                    div().pt(px(6.)).child(
                                        HintBox::new(
                                            strings::settings_startup_disabled_by_windows(),
                                        )
                                        .tone(HintTone::Danger),
                                    ),
                                )
                            }),
                    )
                    .child(
                        div()
                            .child(field_label(strings::settings_window_label()))
                            .child(
                                Checkbox::new(
                                    "close-to-tray",
                                    strings::settings_close_to_tray(),
                                    self.settings.close_to_tray,
                                )
                                .description(strings::settings_close_to_tray_desc())
                                .on_toggle(cx.listener(
                                    |dialog, _, _, cx| {
                                        dialog.change(
                                            |settings| {
                                                settings.close_to_tray = !settings.close_to_tray
                                            },
                                            cx,
                                        )
                                    },
                                )),
                            )
                            .child(
                                div().pt(px(10.)).child(
                                    Checkbox::new(
                                        "start-minimized",
                                        strings::settings_start_minimized(),
                                        self.settings.start_minimized,
                                    )
                                    .description(strings::settings_start_minimized_desc())
                                    .on_toggle(cx.listener(
                                        |dialog, _, _, cx| {
                                            dialog.change(
                                                |settings| {
                                                    settings.start_minimized =
                                                        !settings.start_minimized
                                                },
                                                cx,
                                            )
                                        },
                                    )),
                                ),
                            ),
                    )
                    .child(self.render_storage(cx))
                    .child(
                        div()
                            .child(field_label(strings::settings_about_label()))
                            .child(
                                div()
                                    .text_color(theme::color(theme::TEXT_SECONDARY))
                                    .child(strings::settings_version_format(self.version)),
                            ),
                    ),
            )
            .child(
                dialog_footer().child(
                    Button::new("settings-done", strings::settings_done())
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(SettingsEvent::Closed))),
                ),
            )
    }
}

impl SettingsDialog {
    /// The language row.
    ///
    /// Wrapping rather than a dropdown: five options that all fit on screen beat five hidden
    /// behind a click, and each language is listed in its own words — only "system default" is
    /// translated, because it is the only entry that is about the machine rather than a
    /// language.
    fn render_language(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut options = vec![SegmentOption::new(strings::settings_system_default())];
        options.extend(
            i18n::languages()
                .iter()
                .map(|language| SegmentOption::new(language.name)),
        );

        div()
            .child(field_label(strings::settings_language_label()))
            .child(
                Segment::new("language", options, self.language_index())
                    .wrapping()
                    .on_select(cx.listener(|dialog, index: &usize, _, cx| {
                        dialog.choose_language(*index, cx)
                    })),
            )
    }

    fn render_storage(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let directory = self.data_directory.clone();

        div()
            .child(field_label(strings::settings_storage_label()))
            .child(
                div()
                    .text_color(theme::color(theme::TEXT_SECONDARY))
                    .child(strings::settings_storage_portable_desc()),
            )
            .child(
                div()
                    .pt(px(6.))
                    .text_size(theme::text::small())
                    .text_color(theme::color(theme::TEXT_MUTED))
                    .font_family("Consolas")
                    .child(directory.to_string_lossy().into_owned()),
            )
            .child(
                // A row, so the button hugs its label instead of stretching across the card.
                div().pt(px(8.)).flex().flex_row().child(
                    Button::new("open-data-folder", strings::settings_open_folder())
                        .tone(ButtonTone::Secondary)
                        .glyph(Icon::FolderOutline)
                        .on_click(cx.listener(move |dialog, _, _, _| dialog.reveal_data_folder())),
                ),
            )
    }

    /// Opens the data folder in Explorer.
    fn reveal_data_folder(&self) {
        #[cfg(windows)]
        {
            // Created on demand: on a first run nothing has been written yet, and opening a
            // folder that is not there would just fail silently.
            let _ = std::fs::create_dir_all(&self.data_directory);
            let _ = std::process::Command::new("explorer.exe")
                .arg(self.data_directory.as_os_str())
                .spawn();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_shown_is_the_one_that_was_built() {
        // Stamped by build.rs from the git tag, so a release needs no file edited to match.
        let dialog = SettingsDialog::new(AppSettings::default(), PathBuf::from(r"C:\app\Data"));
        assert_eq!(env!("SYNCMAID_VERSION"), dialog.version);
    }

    #[test]
    fn autostart_is_read_from_the_registry_rather_than_from_settings() {
        // Nothing about autostart is mirrored into settings.json, so the two cannot disagree.
        let settings = AppSettings {
            close_to_tray: true,
            start_minimized: true,
            language: Some("ja".into()),
        };
        let dialog = SettingsDialog::new(settings, PathBuf::from(r"C:\app\Data"));

        assert_eq!(dialog.auto_start.state(), dialog.auto_start_state);
    }
}
