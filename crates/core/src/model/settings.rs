use serde::{Deserialize, Serialize};

/// The persisted settings page.
///
/// Autostart is deliberately **absent**: the `HKCU\...\Run` value is its own single source of
/// truth, so nothing here can drift from what Windows actually does.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct AppSettings {
    /// Closing the window hides it instead of exiting, so triggers keep running.
    #[serde(default)]
    pub close_to_tray: bool,

    /// Launch straight to the tray with no window. Applies to every launch, which is why it
    /// is a setting rather than an argument on the autostart command.
    #[serde(default)]
    pub start_minimized: bool,

    /// BCP-47 UI language tag. `None` follows the OS. A malformed tag from a hand-edited file
    /// falls back to the OS culture rather than blocking startup.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_omit_the_language_null() {
        assert_eq!(
            r#"{"CloseToTray":false,"StartMinimized":false}"#,
            serde_json::to_string(&AppSettings::default()).unwrap()
        );
    }

    #[test]
    fn settings_round_trip() {
        let settings = AppSettings {
            close_to_tray: true,
            start_minimized: true,
            language: Some("zh-Hans".into()),
        };
        let json = serde_json::to_string(&settings).unwrap();
        assert_eq!(
            r#"{"CloseToTray":true,"StartMinimized":true,"Language":"zh-Hans"}"#,
            json
        );
        assert_eq!(
            settings,
            serde_json::from_str::<AppSettings>(&json).unwrap()
        );
    }

    #[test]
    fn an_empty_settings_file_loads_as_defaults() {
        assert_eq!(
            AppSettings::default(),
            serde_json::from_str::<AppSettings>("{}").unwrap()
        );
    }
}
