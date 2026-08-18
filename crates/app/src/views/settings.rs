//! The settings page.
//!
//! Not a modal any more: it takes over the whole window body, with a back arrow where the task
//! sidebar was. Settings is somewhere you *go*, and a scrim over a task list you cannot use is a
//! worse answer than replacing it.
//!
//! Every switch here applies the moment it is flipped. There is no save step: a settings page
//! that needs one invites the user to wonder whether the last change took.

use std::path::PathBuf;

use gpui::{div, prelude::*, px, Context, EventEmitter, SharedString, WeakEntity, Window};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::group_box::GroupBoxVariant;
use gpui_component::setting::{SettingField, SettingGroup, SettingItem, SettingPage, Settings};
use gpui_component::{h_flex, v_flex, ActiveTheme as _, Sizable as _};
use syncmaid_core::model::AppSettings;

use crate::components::Glyph;
use crate::platform::autostart::{auto_start, AutoStart, AutoStartState};
use crate::{i18n, strings};

/// What the settings page changed.
pub enum SettingsEvent {
    /// Apply and persist these. Emitted on every toggle.
    Changed(AppSettings),
    /// The back arrow: return to the task list.
    Closed,
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
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .child(self.render_back_bar(cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(self.pages(cx)),
            )
    }
}

impl SettingsView {
    fn render_back_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .items_center()
            .gap(px(8.))
            .px(px(12.))
            .py(px(10.))
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                Button::new("settings-back")
                    .icon(Glyph::Back)
                    .ghost()
                    .small()
                    .tooltip(strings::settings_back())
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(SettingsEvent::Closed))),
            )
            .child(
                div()
                    .text_lg()
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .child(strings::settings_title()),
            )
    }

    /// The page list and the active page.
    ///
    /// Every field reads its value from a snapshot captured here rather than from the entity.
    /// That is not an optimisation: `SettingField`'s getter runs *during* this render, when the
    /// entity is already mutably borrowed, so reading it back would panic. Immediate-mode
    /// rendering rebuilds this whole tree every frame, so the snapshot is never stale.
    fn pages(&self, cx: &mut Context<Self>) -> Settings {
        let auto_start_on = self.auto_start_state == AutoStartState::Enabled;
        let blocked_by_windows = self.auto_start_state == AutoStartState::DisabledByWindows;
        let close_to_tray = self.settings.close_to_tray;
        let start_minimized = self.settings.start_minimized;
        let language = self.language_value();
        let directory = self.data_directory.clone();
        let version = self.version;

        let view = cx.weak_entity();

        Settings::new("app-settings")
            .with_group_variant(GroupBoxVariant::Outline)
            .sidebar_width(px(170.))
            .pages(vec![
                // No group title: the page header already says it, and a box labelled with the
                // name of the page it is the only thing on says nothing twice.
                SettingPage::new(strings::settings_page_general()).group(SettingGroup::new().item(
                    SettingItem::new(
                        strings::settings_language_label(),
                        SettingField::dropdown(
                            Self::language_options(),
                            move |_| language.clone(),
                            {
                                let view = view.clone();
                                move |chosen: SharedString, cx| {
                                    let tag = (!chosen.is_empty()).then(|| chosen.to_string());
                                    view.update(cx, |this, cx| this.choose_language(tag, cx))
                                        .ok();
                                }
                            },
                        ),
                    ),
                )),
                SettingPage::new(strings::settings_startup_label()).group(
                    SettingGroup::new().item(Self::startup_item(
                        auto_start_on,
                        blocked_by_windows,
                        &view,
                    )),
                ),
                SettingPage::new(strings::settings_window_label()).group(
                    SettingGroup::new().items(vec![
                        SettingItem::new(
                            strings::settings_close_to_tray(),
                            SettingField::switch(move |_| close_to_tray, {
                                let view = view.clone();
                                move |value: bool, cx| {
                                    view.update(cx, |this, cx| {
                                        this.change(|settings| settings.close_to_tray = value, cx)
                                    })
                                    .ok();
                                }
                            }),
                        )
                        .description(strings::settings_close_to_tray_desc()),
                        SettingItem::new(
                            strings::settings_start_minimized(),
                            SettingField::switch(move |_| start_minimized, {
                                let view = view.clone();
                                move |value: bool, cx| {
                                    view.update(cx, |this, cx| {
                                        this.change(|settings| settings.start_minimized = value, cx)
                                    })
                                    .ok();
                                }
                            }),
                        )
                        .description(strings::settings_start_minimized_desc()),
                    ]),
                ),
                SettingPage::new(strings::settings_storage_label()).group(
                    SettingGroup::new()
                        .title(strings::settings_storage_portable())
                        .description(strings::settings_storage_portable_desc())
                        // Nothing to edit: the path is where the executable is. A custom row
                        // rather than a disabled input, because a disabled input reads as
                        // something that ought to be editable.
                        .item(SettingItem::render(move |_, _, cx| {
                            let directory = directory.clone();
                            let view = view.clone();
                            v_flex()
                                .gap(px(8.))
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(cx.theme().muted_foreground)
                                        .font_family(cx.theme().mono_font_family.clone())
                                        .child(directory.to_string_lossy().into_owned()),
                                )
                                .child(
                                    h_flex().child(
                                        Button::new("open-data-folder")
                                            .label(strings::settings_open_folder())
                                            .icon(Glyph::Folder)
                                            .outline()
                                            .small()
                                            .on_click(move |_, _, cx| {
                                                view.read_with(cx, |this, _| {
                                                    this.reveal_data_folder()
                                                })
                                                .ok();
                                            }),
                                    ),
                                )
                        })),
                ),
                SettingPage::new(strings::settings_about_label()).group(SettingGroup::new().item(
                    SettingItem::render(move |_, _, cx| {
                        div()
                            .text_color(cx.theme().muted_foreground)
                            .child(strings::settings_version_format(version))
                    }),
                )),
            ])
    }

    /// The "start with Windows" row.
    ///
    /// When Windows itself has turned the entry off, the row shows a fixed label instead of a
    /// switch. A switch that flips and springs back reads as a bug; a label that says who is in
    /// charge, next to a description saying where to change it, reads as the truth. SyncMaid
    /// never writes `StartupApproved\Run` to overrule that — doing so is exactly the behaviour
    /// that makes an app look like something to be suspicious of.
    fn startup_item(enabled: bool, blocked: bool, view: &WeakEntity<Self>) -> SettingItem {
        if blocked {
            return SettingItem::new(
                strings::settings_start_with_windows(),
                SettingField::render(|_, _, cx| {
                    div()
                        .text_sm()
                        .text_color(cx.theme().warning)
                        .child(strings::settings_startup_off())
                }),
            )
            .description(strings::settings_startup_disabled_by_windows());
        }

        let view = view.clone();
        SettingItem::new(
            strings::settings_start_with_windows(),
            SettingField::switch(
                move |_| enabled,
                move |_, cx| {
                    view.update(cx, |this, cx| this.toggle_auto_start(cx)).ok();
                },
            ),
        )
        .description(strings::settings_start_with_windows_desc())
    }

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
        // Stamped by build.rs from the git tag, so a release needs no file edited to match.
        let view = SettingsView::new(AppSettings::default(), PathBuf::from(r"C:\app\Data"));
        assert_eq!(env!("SYNCMAID_VERSION"), view.version);
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
