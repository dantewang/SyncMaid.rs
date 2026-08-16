//! The three files SyncMaid keeps: tasks, their last outcomes, and the settings page.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use uuid::Uuid;

use crate::io::FileSystem;
use crate::model::{AppSettings, DestinationSyncStatus, SyncTask};
use crate::persistence::json_config;

/// `tasks.json` — the tasks and their destinations.
pub struct TaskStore {
    file_system: Arc<dyn FileSystem>,
    path: PathBuf,
}

impl TaskStore {
    pub fn new(file_system: Arc<dyn FileSystem>, path: impl Into<PathBuf>) -> Self {
        Self {
            file_system,
            path: path.into(),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The saved tasks, or an empty list when nothing is saved.
    pub fn load(&self) -> Vec<SyncTask> {
        self.load_reporting_unreadable().0
    }

    /// As [`TaskStore::load`], but says whether config is present and unreadable.
    ///
    /// The caller must refuse to save in that case — writing an empty list over a file that is
    /// merely locked would destroy every task the user has.
    pub fn load_reporting_unreadable(&self) -> (Vec<SyncTask>, bool) {
        let loaded = json_config::try_load_with_backup::<Vec<SyncTask>>(
            self.file_system.as_ref(),
            &self.path,
        );
        (loaded.value.unwrap_or_default(), loaded.unreadable)
    }

    pub fn save(&self, tasks: &[SyncTask]) -> std::io::Result<()> {
        json_config::save(self.file_system.as_ref(), &self.path, &tasks.to_vec())
    }
}

/// `status.json` — the last result per destination.
///
/// Persisted as a flat list and indexed by destination id on load, so a rename or a path change
/// never loses a destination's history.
pub struct StatusStore {
    file_system: Arc<dyn FileSystem>,
    path: PathBuf,
}

impl StatusStore {
    pub fn new(file_system: Arc<dyn FileSystem>, path: impl Into<PathBuf>) -> Self {
        Self {
            file_system,
            path: path.into(),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> HashMap<Uuid, DestinationSyncStatus> {
        json_config::try_load_with_backup::<Vec<DestinationSyncStatus>>(
            self.file_system.as_ref(),
            &self.path,
        )
        .value
        .unwrap_or_default()
        .into_iter()
        .map(|status| (status.destination_id, status))
        .collect()
    }

    pub fn save(&self, statuses: &HashMap<Uuid, DestinationSyncStatus>) -> std::io::Result<()> {
        // Ordered by id so a re-save with no changes produces the same bytes and the .bak
        // rotation stays meaningful.
        let mut flat: Vec<&DestinationSyncStatus> = statuses.values().collect();
        flat.sort_by_key(|status| status.destination_id);
        json_config::save(self.file_system.as_ref(), &self.path, &flat)
    }
}

/// `settings.json` — the settings page.
pub struct SettingsStore {
    file_system: Arc<dyn FileSystem>,
    path: PathBuf,
}

impl SettingsStore {
    pub fn new(file_system: Arc<dyn FileSystem>, path: impl Into<PathBuf>) -> Self {
        Self {
            file_system,
            path: path.into(),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The saved settings, or the defaults when nothing is saved or the file is unusable.
    pub fn load(&self) -> AppSettings {
        json_config::try_load_with_backup::<AppSettings>(self.file_system.as_ref(), &self.path)
            .value
            .unwrap_or_default()
    }

    pub fn save(&self, settings: &AppSettings) -> std::io::Result<()> {
        json_config::save(self.file_system.as_ref(), &self.path, settings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filtering::FilterRule;
    use crate::io::InMemoryFileSystem;
    use crate::model::{Destination, SyncOutcome, SyncStrategy, SyncTaskKind};
    use crate::triggers::Trigger;

    fn stores() -> (InMemoryFileSystem, TaskStore, StatusStore, SettingsStore) {
        let memory = InMemoryFileSystem::new();
        let file_system: Arc<dyn FileSystem> = Arc::new(memory.clone());
        (
            memory,
            TaskStore::new(Arc::clone(&file_system), r"C:\cfg\tasks.json"),
            StatusStore::new(Arc::clone(&file_system), r"C:\cfg\status.json"),
            SettingsStore::new(file_system, r"C:\cfg\settings.json"),
        )
    }

    fn task() -> SyncTask {
        SyncTask::new(
            "Photos",
            r"C:\src",
            Trigger::watch(),
            vec![Destination::new(
                "NAS",
                r"N:\backup",
                [FilterRule::AllFiles],
                SyncStrategy::Mirror,
            )],
        )
    }

    #[test]
    fn tasks_round_trip_with_their_ids_and_order() {
        let (_, tasks, _, _) = stores();
        let first = task();
        let second = SyncTask::new("Downloads", r"C:\downloads", Trigger::Manual, vec![]);
        let saved = vec![first.clone(), second.clone()];

        tasks.save(&saved).unwrap();
        let loaded = tasks.load();

        assert_eq!(
            vec![first.id, second.id],
            loaded.iter().map(|t| t.id).collect::<Vec<_>>()
        );
        assert_eq!(
            vec!["Photos", "Downloads"],
            loaded.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
            "order is semantic for a Move task's rules, so it is never rearranged on the way out"
        );
        assert_eq!(first.destinations[0].id, loaded[0].destinations[0].id);
        assert_eq!(Trigger::watch(), loaded[0].trigger);
        assert!(
            loaded.iter().all(SyncTask::has_explicit_kind),
            "one save is what makes a derived kind explicit"
        );
    }

    #[test]
    fn a_second_round_trip_changes_nothing() {
        let (memory, tasks, _, _) = stores();
        tasks.save(&[task()]).unwrap();
        tasks.save(&tasks.load()).unwrap();
        let once = memory.contents_of(r"C:\cfg\tasks.json").unwrap();

        tasks.save(&tasks.load()).unwrap();

        assert_eq!(once, memory.contents_of(r"C:\cfg\tasks.json").unwrap());
    }

    #[test]
    fn nothing_saved_yet_loads_as_an_empty_list() {
        let (_, tasks, _, _) = stores();

        let (loaded, unreadable) = tasks.load_reporting_unreadable();

        assert!(loaded.is_empty());
        assert!(!unreadable);
    }

    #[test]
    fn unreadable_config_is_reported_so_the_caller_refuses_to_save_over_it() {
        let (memory, tasks, _, _) = stores();
        memory.add_file(r"C:\cfg\tasks.json", b"{ not json");

        let (loaded, unreadable) = tasks.load_reporting_unreadable();

        assert!(loaded.is_empty());
        assert!(unreadable);
    }

    #[test]
    fn a_legacy_task_file_loads_and_one_save_makes_its_kind_explicit() {
        let (memory, tasks, _, _) = stores();
        memory.add_file(
            r"C:\cfg\tasks.json",
            br#"[{"Name":"Sort","SourcePath":"C:\\src","Trigger":{"kind":"manual"},
                 "Destinations":[{"Name":"D","Target":{"kind":"local","Path":"D:\\d"},
                 "Filters":[{"kind":"all"}],"Strategy":"Move"}]}]"#,
        );

        let loaded = tasks.load();
        assert_eq!(SyncTaskKind::Move, loaded[0].kind());
        assert!(!loaded[0].has_explicit_kind());

        tasks.save(&loaded).unwrap();

        let written = String::from_utf8(memory.contents_of(r"C:\cfg\tasks.json").unwrap()).unwrap();
        assert!(written.contains(r#""Kind": "Move""#), "{written}");
    }

    #[test]
    fn statuses_persist_flat_and_index_by_destination_id() {
        let (memory, _, statuses, _) = stores();
        let id = Uuid::new_v4();
        let mut saved = HashMap::new();
        saved.insert(id, DestinationSyncStatus::new(id, SyncOutcome::Success));

        statuses.save(&saved).unwrap();

        let written =
            String::from_utf8(memory.contents_of(r"C:\cfg\status.json").unwrap()).unwrap();
        assert!(
            written.trim_start().starts_with('['),
            "a flat list: {written}"
        );
        assert_eq!(saved, statuses.load());
    }

    #[test]
    fn settings_fall_back_to_the_defaults_when_the_file_is_unusable() {
        let (memory, _, _, settings) = stores();
        memory.add_file(r"C:\cfg\settings.json", b"{ not json");

        assert_eq!(AppSettings::default(), settings.load());
    }

    #[test]
    fn settings_round_trip() {
        let (_, _, _, settings) = stores();
        let saved = AppSettings {
            close_to_tray: true,
            start_minimized: true,
            language: Some("zh-Hant".into()),
        };

        settings.save(&saved).unwrap();

        assert_eq!(saved, settings.load());
    }

    #[test]
    fn a_save_keeps_the_previous_version_as_bak() {
        let (memory, tasks, _, _) = stores();

        tasks.save(&[task()]).unwrap();
        tasks.save(&[]).unwrap();

        assert!(memory.contents_of(r"C:\cfg\tasks.json.bak").is_some());
        assert!(memory
            .all_paths()
            .iter()
            .all(|path| !path.contains(".tmp-")));
    }
}
