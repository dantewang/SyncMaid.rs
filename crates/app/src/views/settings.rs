//! The settings page.
//!
//! Not a modal any more: it takes over the whole window body, and the gear in the sidebar is a
//! toggle rather than a one-way door. Settings is somewhere you *go*, and a scrim over a task
//! list you cannot use is a worse answer than replacing it.
//!
//! Every switch here applies the moment it is flipped. There is no save step: a settings page
//! that needs one invites the user to wonder whether the last change took.

use std::path::PathBuf;

use gpui::{
    div, prelude::*, px, App, Context, Div, EventEmitter, FontWeight, SharedString, Window,
};
use gpui_component::button::Button;

use gpui_component::alert::Alert;
use gpui_component::button::ButtonGroup;
use gpui_component::switch::Switch;
use gpui_component::{h_flex, v_flex, ActiveTheme as _, Selectable as _, Sizable as _};
use syncmaid_core::model::AppSettings;

use crate::components::Glyph;
use crate::platform::autostart::{auto_start, AutoStart, AutoStartState};
use crate::{i18n, strings};

/// What the settings page changed.
pub enum SettingsEvent {
    /// Apply and persist these. Emitted on every toggle.
    Changed(AppSettings),
}

/// See the module docs.
pub struct SettingsView {
    settings: AppSettings,
    auto_start: Box<dyn AutoStart>,
    auto_start_state: AutoStartState,
    data_directory: PathBuf,
    version: &'static str,
}

impl SettingsView {
    pub fn new(settings: AppSettings, data_directory: PathBuf) -> Self {
        let auto_start = auto_start();
        Self {
            settings,
            auto_start_state: auto_start.state(),
            auto_start,
            data_directory,
            version: env!("CARGO_PKG_VERSION"),
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

    /// Switches language immediately — no restart, no "apply".
    ///
    /// Nothing is cached, so the whole app changing language is one redraw of every window.
    fn choose_language(&mut self, tag: Option<String>, cx: &mut Context<Self>) {
        i18n::set_language(tag.as_deref());
        self.change(|settings| settings.language = tag, cx);
        cx.refresh_windows();
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

impl EventEmitter<SettingsEvent> for SettingsView {}

impl Render for SettingsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // One flowing page, not a page list. There were five pages holding one row each, which
        // is a table of contents for a document you can read in a single screenful — and the
        // inner sidebar it needed put a second navigation column beside the app's own.
        //
        // No back bar either: the gear in the sidebar is a toggle, and it is where the user
        // came from.
        v_flex()
            .id("settings")
            .size_full()
            .overflow_y_scroll()
            .bg(cx.theme().background)
            .px(px(22.))
            .py(px(16.))
            .gap(px(22.))
            .child(
                div()
                    .text_size(px(18.))
                    .font_weight(FontWeight::MEDIUM)
                    .pb(px(6.))
                    .child(strings::settings_title()),
            )
            .child(self.render_general(cx))
            .child(self.render_startup(cx))
            .child(self.render_window(cx))
            .child(self.render_storage(cx))
            .child(self.render_about(cx))
    }
}

impl SettingsView {
    /// A row of buttons rather than a dropdown.
    ///
    /// There are five choices and they are all short, so a list that has to be opened to be
    /// read is a click spent hiding something that fits. It also means the current language is
    /// visible without interacting with anything.
    fn render_general(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let options = Self::language_options();
        let current = self.language_value();
        let chosen = options
            .iter()
            .position(|(value, _)| *value == current)
            .unwrap_or(0);

        let group = options.iter().enumerate().fold(
            ButtonGroup::new("settings-language").outline().small(),
            |group, (index, (_, label))| {
                group.child(
                    Button::new(SharedString::from(format!("language-{index}")))
                        .label(label.clone())
                        .selected(index == chosen),
                )
            },
        );

        section(strings::settings_page_general(), cx).child(
            setting_row(
                strings::settings_language_label(),
                strings::settings_language_desc(),
                true,
                cx,
            )
            .child(
                group.on_click(cx.listener(|this, clicked: &Vec<usize>, _, cx| {
                    let Some(index) = clicked.first() else {
                        return;
                    };
                    let tag = Self::language_options()
                        .get(*index)
                        .map(|(value, _)| value.to_string())
                        .filter(|value| !value.is_empty());
                    this.choose_language(tag, cx);
                })),
            ),
        )
    }

    /// Windows' own switch wins, and the page says so rather than fighting it. SyncMaid never
    /// writes `StartupApproved\Run` to overrule it: that is exactly the behaviour antivirus
    /// heuristics look for, and the user meant it when they used it.
    fn render_startup(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let blocked = self.auto_start_state == AutoStartState::DisabledByWindows;
        let enabled = self.auto_start_state == AutoStartState::Enabled;

        section(strings::settings_startup_label(), cx)
            .child(
                setting_row(
                    strings::settings_start_with_windows(),
                    strings::settings_start_with_windows_desc(),
                    blocked,
                    cx,
                )
                .when(!blocked, |row| {
                    row.child(
                        Switch::new("settings-startup")
                            .checked(enabled)
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_auto_start(cx))),
                    )
                }),
            )
            .when(blocked, |group| {
                group.child(Alert::warning(
                    "startup-blocked",
                    strings::settings_startup_disabled_by_windows(),
                ))
            })
    }

    fn render_window(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let close_to_tray = self.settings.close_to_tray;
        let start_minimized = self.settings.start_minimized;

        section(strings::settings_window_label(), cx)
            .child(
                setting_row(
                    strings::settings_close_to_tray(),
                    strings::settings_close_to_tray_desc(),
                    false,
                    cx,
                )
                .child(
                    Switch::new("settings-close-to-tray")
                        .checked(close_to_tray)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.change(|settings| settings.close_to_tray = !close_to_tray, cx)
                        })),
                ),
            )
            .child(
                setting_row(
                    strings::settings_start_minimized(),
                    strings::settings_start_minimized_desc(),
                    true,
                    cx,
                )
                .child(
                    Switch::new("settings-start-minimized")
                        .checked(start_minimized)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.change(|settings| settings.start_minimized = !start_minimized, cx)
                        })),
                ),
            )
    }

    fn render_storage(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.weak_entity();
        let directory = self.data_directory.to_string_lossy().into_owned();

        section(strings::settings_storage_label(), cx)
            .child(
                setting_row(
                    strings::settings_storage_portable(),
                    strings::settings_storage_portable_desc(),
                    true,
                    cx,
                )
                .child(
                    Button::new("open-data-folder")
                        .label(strings::settings_open_folder())
                        .icon(Glyph::Folder)
                        .outline()
                        .small()
                        .on_click(move |_, _, cx| {
                            view.read_with(cx, |this, _| this.reveal_data_folder()).ok();
                        }),
                ),
            )
            // Not an input. A disabled text box reads as something that ought to be editable;
            // this is simply where the folder is.
            .child(
                div()
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(directory),
            )
    }

    fn render_about(&self, cx: &mut Context<Self>) -> impl IntoElement {
        section(strings::settings_about_label(), cx).child(
            h_flex()
                .pt(px(11.))
                .gap(px(14.))
                .items_center()
                .child(div().font_weight(FontWeight::SEMIBOLD).child("SyncMaid"))
                .child(
                    div()
                        .font_family(cx.theme().mono_font_family.clone())
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(strings::settings_version_format(self.version)),
                ),
        )
    }
}

/// A group of settings under a mono-caps caption and a hairline.
///
/// A caption rather than a box: the rows inside already read as a set, and a border around them
/// would be a second frame inside the pane's own.
fn section(label: &'static str, cx: &App) -> Div {
    v_flex()
        .gap(px(2.))
        .child(
            div()
                .font_family(cx.theme().mono_font_family.clone())
                .text_size(px(9.))
                .font_weight(FontWeight::BOLD)
                .text_color(cx.theme().muted_foreground)
                .pb(px(6.))
                .child(label),
        )
        .child(div().h(px(1.)).bg(cx.theme().border))
}

/// One setting: what it is, what it means, and the control that changes it.
///
/// The description is not optional garnish. Every switch on this page changes behaviour the
/// user will meet later without a prompt — a window that does not close, an app that starts
/// itself — and the sentence under the label is where that is said.
fn setting_row(
    label: &'static str,
    description: &'static str,
    last: bool,
    cx: &App,
) -> gpui::Stateful<Div> {
    h_flex()
        .id(label)
        .py(px(11.))
        .gap(px(16.))
        .items_start()
        .when(!last, |row| {
            row.border_b_1().border_color(cx.theme().border)
        })
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap(px(3.))
                .child(div().font_weight(FontWeight::MEDIUM).child(label))
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(description),
                ),
        )
}

impl SettingsView {
    /// The dropdown's `(value, label)` pairs. An empty value means "follow the system".
    ///
    /// Each language is listed in its own words — only "system default" is translated, because
    /// it is the only entry that is about the machine rather than a language.
    fn language_options() -> Vec<(SharedString, SharedString)> {
        let mut options = vec![(
            SharedString::default(),
            strings::settings_system_default().into(),
        )];
        options.extend(
            i18n::languages()
                .iter()
                .map(|language| (language.tag.into(), language.name.into())),
        );
        options
    }

    /// The dropdown value for the current setting: the stored tag, or empty for "system".
    fn language_value(&self) -> SharedString {
        match &self.settings.language {
            Some(tag) => tag.clone().into(),
            None => SharedString::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_shown_is_the_one_that_was_built() {
        // `Cargo.toml` is the one place a version is written; everything else reads it.
        let view = SettingsView::new(AppSettings::default(), PathBuf::from(r"C:\app\Data"));
        assert_eq!(env!("CARGO_PKG_VERSION"), view.version);
    }

    #[test]
    fn autostart_is_read_from_the_registry_rather_than_from_settings() {
        // Nothing about autostart is mirrored into settings.json, so the two cannot disagree.
        let settings = AppSettings {
            close_to_tray: true,
            start_minimized: true,
            language: Some("ja".into()),
        };
        let view = SettingsView::new(settings, PathBuf::from(r"C:\app\Data"));

        assert_eq!(view.auto_start.state(), view.auto_start_state);
    }

    #[test]
    fn following_the_system_is_the_empty_value_rather_than_a_tag() {
        // The dropdown carries `SharedString`s, so "no preference" has to be a value rather than
        // an absence — and it must round-trip, or opening settings would silently pin a language.
        let following = SettingsView::new(AppSettings::default(), PathBuf::new());
        assert!(following.language_value().is_empty());

        let options = SettingsView::language_options();
        assert!(options[0].0.is_empty(), "system default is the empty value");
        assert_eq!(
            i18n::languages().len() + 1,
            options.len(),
            "every shipped language is offered, plus the system entry"
        );
    }

    #[test]
    fn a_pinned_language_round_trips_through_the_dropdown_value() {
        let settings = AppSettings {
            language: Some("zh-Hant".into()),
            ..AppSettings::default()
        };
        let view = SettingsView::new(settings, PathBuf::new());

        assert_eq!("zh-Hant", view.language_value().as_ref());
        assert!(
            SettingsView::language_options()
                .iter()
                .any(|(value, _)| value.as_ref() == "zh-Hant"),
            "the pinned tag has to be one of the offered values or the dropdown shows nothing"
        );
    }
}
