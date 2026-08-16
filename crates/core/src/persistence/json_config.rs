//! Shared primary/backup JSON loading and atomic saving for config files.

use std::path::Path;

use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::io::FileSystem;
use crate::persistence::atomic_file;

/// The result of loading a config file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loaded<T> {
    /// What was read, or `None` when there is nothing usable.
    pub value: Option<T>,
    /// True when a config file is present but neither it nor its backup could be read —
    /// corrupt JSON, or an I/O or permission failure (an antivirus scan, a roaming-profile
    /// lock, a sync client holding the file).
    ///
    /// Callers must **not** treat that as "the user has nothing configured": persisting over
    /// it would rotate the last good copy away and lose everything.
    pub unreadable: bool,
}

/// Loads `path`, falling back to its `.bak` when the main file cannot be read.
pub fn try_load_with_backup<T: DeserializeOwned>(
    file_system: &dyn FileSystem,
    path: &Path,
) -> Loaded<T> {
    let backup = atomic_file::backup_of(path);
    let value = try_load(file_system, path).or_else(|| try_load(file_system, &backup));
    let unreadable =
        value.is_none() && (file_system.file_exists(path) || file_system.file_exists(&backup));
    Loaded { value, unreadable }
}

/// Serializes `value` and writes it atomically.
pub fn save<T: Serialize>(
    file_system: &dyn FileSystem,
    path: &Path,
    value: &T,
) -> std::io::Result<()> {
    atomic_file::write(file_system, path, to_config_json(value)?.as_bytes())
}

/// The exact bytes the C# app writes: two-space indentation and CRLF line endings, which is
/// what `System.Text.Json` produces with `WriteIndented` on Windows.
///
/// Matching it means a user can move between the two builds without every save showing up as
/// a whole-file diff. Replacing the separators wholesale is safe because JSON escapes a
/// newline inside a string as the two characters `\` and `n`, so a raw newline in the output
/// is always formatting.
pub fn to_config_json<T: Serialize>(value: &T) -> std::io::Result<String> {
    let json = serde_json::to_string_pretty(value).map_err(std::io::Error::other)?;
    Ok(json.replace('\n', "\r\n"))
}

fn try_load<T: DeserializeOwned>(file_system: &dyn FileSystem, path: &Path) -> Option<T> {
    if !file_system.file_exists(path) {
        return None;
    }

    let bytes = file_system.read_all_bytes(path).ok()?;
    let json = String::from_utf8(bytes).ok()?;
    if json.trim().is_empty() {
        return None;
    }
    serde_json::from_str(&json).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::InMemoryFileSystem;
    use crate::model::AppSettings;

    const PATH: &str = r"C:\cfg\settings.json";

    fn settings() -> AppSettings {
        AppSettings {
            close_to_tray: true,
            start_minimized: false,
            language: Some("ja".into()),
        }
    }

    #[test]
    fn nothing_saved_yet_is_not_unreadable() {
        let fs = InMemoryFileSystem::new();

        let loaded: Loaded<AppSettings> = try_load_with_backup(&fs, Path::new(PATH));

        assert_eq!(None, loaded.value);
        assert!(!loaded.unreadable, "a first run must be free to save");
    }

    #[test]
    fn a_saved_file_round_trips() {
        let fs = InMemoryFileSystem::new();
        save(&fs, Path::new(PATH), &settings()).unwrap();

        let loaded: Loaded<AppSettings> = try_load_with_backup(&fs, Path::new(PATH));

        assert_eq!(Some(settings()), loaded.value);
        assert!(!loaded.unreadable);
    }

    #[test]
    fn a_corrupt_main_file_falls_back_to_the_backup() {
        let fs = InMemoryFileSystem::new();
        save(&fs, Path::new(PATH), &settings()).unwrap();
        save(&fs, Path::new(PATH), &AppSettings::default()).unwrap();
        // The .bak now holds the first version; corrupt the live file.
        fs.add_file(PATH, b"{ this is not json");

        let loaded: Loaded<AppSettings> = try_load_with_backup(&fs, Path::new(PATH));

        assert_eq!(Some(settings()), loaded.value);
        assert!(
            !loaded.unreadable,
            "the backup was readable, so nothing is lost"
        );
    }

    #[test]
    fn a_file_present_but_unreadable_is_reported_rather_than_read_as_empty() {
        let fs = InMemoryFileSystem::new();
        fs.add_file(PATH, b"{ this is not json");

        let loaded: Loaded<AppSettings> = try_load_with_backup(&fs, Path::new(PATH));

        assert_eq!(None, loaded.value);
        assert!(
            loaded.unreadable,
            "saving over this would rotate the last good copy away and lose the user's config"
        );
    }

    #[test]
    fn a_blank_file_reads_as_nothing_saved() {
        let fs = InMemoryFileSystem::new();
        fs.add_file(PATH, b"   \n  ");

        let loaded: Loaded<AppSettings> = try_load_with_backup(&fs, Path::new(PATH));

        assert_eq!(None, loaded.value);
        assert!(
            loaded.unreadable,
            "a blank file is still a file that is there"
        );
    }

    #[test]
    fn config_is_written_with_the_line_endings_the_csharp_writer_uses() {
        let json = to_config_json(&settings()).unwrap();

        assert!(
            json.contains("\r\n"),
            "System.Text.Json writes CRLF on Windows: {json:?}"
        );
        assert!(
            !json.contains("\n\n"),
            "no stray bare newlines survived the replacement"
        );
        assert_eq!(
            settings(),
            serde_json::from_str::<AppSettings>(&json).unwrap()
        );
    }

    #[test]
    fn a_newline_inside_a_value_is_escaped_rather_than_turned_into_a_line_ending() {
        let awkward = AppSettings {
            language: Some("a\nb".into()),
            ..AppSettings::default()
        };

        let json = to_config_json(&awkward).unwrap();

        assert!(json.contains(r#""a\nb""#), "{json}");
        assert_eq!(awkward, serde_json::from_str::<AppSettings>(&json).unwrap());
    }

    #[test]
    fn saving_twice_is_byte_stable() {
        let fs = InMemoryFileSystem::new();
        save(&fs, Path::new(PATH), &settings()).unwrap();
        let once = fs.contents_of(PATH).unwrap();

        let loaded: Loaded<AppSettings> = try_load_with_backup(&fs, Path::new(PATH));
        save(&fs, Path::new(PATH), &loaded.value.unwrap()).unwrap();

        assert_eq!(once, fs.contents_of(PATH).unwrap());
    }
}
