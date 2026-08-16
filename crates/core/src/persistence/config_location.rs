//! Where config and data live.
//!
//! **Portable, always**: a `Data` folder beside the executable, so the whole app is a folder
//! you can copy to a USB stick and it leaves nothing on the machine.
//!
//! The C# build offered a second location under `%APPDATA%` and a marker file to choose
//! between them, which brought a chicken-and-egg problem — the setting that picks the config
//! folder cannot live inside it — and a three-phase copy-verify-remove migration. Fixing the
//! location removes both. The one rule that outlives them: never put a "where is my config"
//! field inside the config.

use std::path::{Path, PathBuf};

/// The folder name beside the executable.
pub const DATA_DIRECTORY: &str = "Data";

/// Resolves the paths of everything SyncMaid writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigLocation {
    directory: PathBuf,
}

impl ConfigLocation {
    /// The real location: `<the folder holding SyncMaid.exe>/Data`.
    pub fn portable() -> Self {
        let executable_directory = std::env::current_exe()
            .ok()
            .and_then(|executable| executable.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| PathBuf::from("."));
        Self::at(executable_directory.join(DATA_DIRECTORY))
    }

    /// An explicit directory, for tests and for anything driving the app from elsewhere.
    pub fn at(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
        }
    }

    /// The folder holding every file below.
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// Tasks and destinations.
    pub fn tasks_path(&self) -> PathBuf {
        self.directory.join("tasks.json")
    }

    /// The last outcome per destination.
    pub fn status_path(&self) -> PathBuf {
        self.directory.join("status.json")
    }

    /// The settings page.
    pub fn settings_path(&self) -> PathBuf {
        self.directory.join("settings.json")
    }

    /// The activity and error log.
    pub fn log_path(&self) -> PathBuf {
        self.directory.join("logs").join("syncmaid.log")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_path_sits_under_the_one_directory() {
        let location = ConfigLocation::at(r"E:\Portable\SyncMaid\Data");

        assert_eq!(
            Path::new(r"E:\Portable\SyncMaid\Data\tasks.json"),
            location.tasks_path()
        );
        assert_eq!(
            Path::new(r"E:\Portable\SyncMaid\Data\status.json"),
            location.status_path()
        );
        assert_eq!(
            Path::new(r"E:\Portable\SyncMaid\Data\settings.json"),
            location.settings_path()
        );
        assert_eq!(
            Path::new(r"E:\Portable\SyncMaid\Data\logs\syncmaid.log"),
            location.log_path()
        );
    }

    #[test]
    fn the_portable_location_sits_beside_the_executable() {
        let location = ConfigLocation::portable();
        let executable_directory = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();

        assert_eq!(
            executable_directory.join(DATA_DIRECTORY),
            location.directory()
        );
    }
}
