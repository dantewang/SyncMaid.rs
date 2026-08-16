//! Everything the main window is showing.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use syncmaid_core::io::FileSystem;
use syncmaid_core::model::{AppSettings, DestinationSyncStatus, SyncOutcome, SyncTask};
use syncmaid_core::persistence::{ConfigLocation, SettingsStore, StatusStore, TaskStore};
use uuid::Uuid;

/// The loaded tasks, their last outcomes, and what the window is doing with them.
pub struct Workspace {
    tasks: Vec<SyncTask>,
    statuses: HashMap<Uuid, DestinationSyncStatus>,
    settings: AppSettings,
    /// Config is on disk but could not be read. Saving would rotate the last good copy away,
    /// so persistence is switched off and the window says why.
    config_unreadable: bool,

    task_store: TaskStore,
    status_store: StatusStore,
    settings_store: SettingsStore,

    sidebar_visible: bool,
    expanded: HashSet<Uuid>,
    selected: Option<Uuid>,
}

impl Workspace {
    /// Loads everything from `location`.
    pub fn load(file_system: Arc<dyn FileSystem>, location: &ConfigLocation) -> Self {
        let task_store = TaskStore::new(Arc::clone(&file_system), location.tasks_path());
        let status_store = StatusStore::new(Arc::clone(&file_system), location.status_path());
        let settings_store = SettingsStore::new(file_system, location.settings_path());

        let (tasks, config_unreadable) = task_store.load_reporting_unreadable();
        let statuses = status_store.load();
        let settings = settings_store.load();

        // Every task starts expanded, the way the Avalonia card did.
        let expanded = tasks.iter().map(|task| task.id).collect();

        Self {
            tasks,
            statuses,
            settings,
            config_unreadable,
            task_store,
            status_store,
            settings_store,
            sidebar_visible: true,
            expanded,
            selected: None,
        }
    }

    pub fn tasks(&self) -> &[SyncTask] {
        &self.tasks
    }

    pub fn statuses(&self) -> &HashMap<Uuid, DestinationSyncStatus> {
        &self.statuses
    }

    pub fn settings(&self) -> &AppSettings {
        &self.settings
    }

    pub fn config_unreadable(&self) -> bool {
        self.config_unreadable
    }

    pub fn sidebar_visible(&self) -> bool {
        self.sidebar_visible
    }

    pub fn toggle_sidebar(&mut self) {
        self.sidebar_visible = !self.sidebar_visible;
    }

    pub fn selected(&self) -> Option<Uuid> {
        self.selected
    }

    /// Selecting a task also opens it, so the card the sidebar scrolls to is readable.
    pub fn select(&mut self, task_id: Uuid) {
        self.selected = Some(task_id);
        self.expanded.insert(task_id);
    }

    pub fn is_expanded(&self, task_id: Uuid) -> bool {
        self.expanded.contains(&task_id)
    }

    pub fn toggle_expanded(&mut self, task_id: Uuid) {
        if !self.expanded.remove(&task_id) {
            self.expanded.insert(task_id);
        }
    }

    /// True when at least one task is closed, which is what the header button offers to change.
    pub fn all_expanded(&self) -> bool {
        !self.tasks.is_empty()
            && self
                .tasks
                .iter()
                .all(|task| self.expanded.contains(&task.id))
    }

    pub fn set_all_expanded(&mut self, expanded: bool) {
        self.expanded.clear();
        if expanded {
            self.expanded.extend(self.tasks.iter().map(|task| task.id));
        }
    }

    /// The task with this id, if it is still here.
    pub fn task(&self, task_id: Uuid) -> Option<&SyncTask> {
        self.tasks.iter().find(|task| task.id == task_id)
    }

    /// Adds a task, or replaces the one with the same id, and writes the list out.
    pub fn upsert_task(&mut self, task: SyncTask) {
        match self
            .tasks
            .iter_mut()
            .find(|existing| existing.id == task.id)
        {
            Some(existing) => *existing = task,
            None => {
                self.expanded.insert(task.id);
                self.tasks.push(task);
            }
        }
        self.persist_tasks();
    }

    /// Removes a task and everything keyed to it.
    pub fn remove_task(&mut self, task_id: Uuid) {
        self.tasks.retain(|task| task.id != task_id);
        self.expanded.remove(&task_id);
        if self.selected == Some(task_id) {
            self.selected = None;
        }
        self.persist_tasks();
        self.persist_statuses();
    }

    /// What every destination of `task` currently reads, so a cancelled run can put it back.
    ///
    /// A cancelled run is not a failure: the work that landed stays, and the rows go back to
    /// what they said before rather than reporting something that never finished.
    pub fn status_snapshot(&self, task: &SyncTask) -> Vec<DestinationSyncStatus> {
        task.destinations
            .iter()
            .map(|destination| {
                self.statuses
                    .get(&destination.id)
                    .cloned()
                    .unwrap_or_else(|| DestinationSyncStatus::never(destination.id))
            })
            .collect()
    }

    /// Puts every destination of `task` into the running state, without writing to disk —
    /// `Running` is transient and has no business in `status.json`.
    pub fn mark_running(&mut self, task: &SyncTask) {
        for destination in &task.destinations {
            self.statuses.insert(
                destination.id,
                DestinationSyncStatus::new(destination.id, SyncOutcome::Running),
            );
        }
    }

    /// Puts statuses back without writing them out, for a run that was cancelled.
    pub fn restore_statuses(&mut self, statuses: Vec<DestinationSyncStatus>) {
        for status in statuses {
            self.statuses.insert(status.destination_id, status);
        }
    }

    /// Records a run's outcomes and writes them out.
    pub fn apply_statuses(&mut self, statuses: impl IntoIterator<Item = DestinationSyncStatus>) {
        for status in statuses {
            self.statuses.insert(status.destination_id, status);
        }
        self.persist_statuses();
    }

    /// Writes the tasks, unless config is present and unreadable.
    pub fn persist_tasks(&self) {
        if self.config_unreadable {
            // Writing now would replace a file we could not read with one we invented.
            tracing::warn!("refusing to save tasks: the config on disk could not be read");
            return;
        }
        if let Err(error) = self.task_store.save(&self.tasks) {
            tracing::error!(%error, "could not save tasks");
        }
    }

    /// Drops statuses whose destination no longer exists, then writes them out.
    pub fn persist_statuses(&mut self) {
        let live: HashSet<Uuid> = self
            .tasks
            .iter()
            .flat_map(|task| task.destinations.iter().map(|destination| destination.id))
            .collect();
        self.statuses.retain(|id, _| live.contains(id));

        if self.config_unreadable {
            return;
        }
        if let Err(error) = self.status_store.save(&self.statuses) {
            tracing::error!(%error, "could not save statuses");
        }
    }

    /// Applies a settings change and writes it out. Settings take effect immediately — there is
    /// no save step on that page.
    pub fn update_settings(&mut self, change: impl FnOnce(&mut AppSettings)) {
        let mut updated = self.settings.clone();
        change(&mut updated);
        if updated == self.settings {
            return; // Nothing to write.
        }
        self.settings = updated;
        if let Err(error) = self.settings_store.save(&self.settings) {
            tracing::error!(%error, "could not save settings");
        }
    }
}

#[cfg(test)]
mod tests {
    use syncmaid_core::filtering::FilterRule;
    use syncmaid_core::io::InMemoryFileSystem;
    use syncmaid_core::model::{Destination, SyncOutcome, SyncStrategy};
    use syncmaid_core::triggers::Trigger;

    use super::*;

    const CONFIG: &str = r"C:\app\Data";

    fn task(name: &str) -> SyncTask {
        SyncTask::new(
            name,
            r"C:\src",
            Trigger::Manual,
            vec![Destination::new(
                "D",
                r"D:\d",
                [FilterRule::AllFiles],
                SyncStrategy::Mirror,
            )],
        )
    }

    fn workspace(memory: &InMemoryFileSystem) -> Workspace {
        Workspace::load(Arc::new(memory.clone()), &ConfigLocation::at(CONFIG))
    }

    #[test]
    fn a_first_run_loads_empty_and_is_free_to_save() {
        let memory = InMemoryFileSystem::new();
        let workspace = workspace(&memory);

        assert!(workspace.tasks().is_empty());
        assert!(!workspace.config_unreadable());
    }

    #[test]
    fn tasks_round_trip_through_the_store() {
        let memory = InMemoryFileSystem::new();
        {
            let mut workspace = workspace(&memory);
            workspace.tasks = vec![task("Photos")];
            workspace.persist_tasks();
        }

        assert_eq!(1, workspace(&memory).tasks().len());
    }

    #[test]
    fn unreadable_config_switches_persistence_off_rather_than_overwriting_it() {
        let memory = InMemoryFileSystem::new();
        memory.add_file(format!(r"{CONFIG}\tasks.json"), b"{ not json");
        let workspace = workspace(&memory);

        assert!(workspace.config_unreadable());
        workspace.persist_tasks();

        assert_eq!(
            Some(b"{ not json".to_vec()),
            memory.contents_of(format!(r"{CONFIG}\tasks.json")),
            "the file the user still has must not be replaced by the empty list we invented"
        );
    }

    #[test]
    fn statuses_for_destinations_that_no_longer_exist_are_pruned() {
        let memory = InMemoryFileSystem::new();
        let mut workspace = workspace(&memory);
        let live = task("Photos");
        let live_id = live.destinations[0].id;
        workspace.tasks = vec![live];
        workspace.apply_statuses([
            DestinationSyncStatus::new(live_id, SyncOutcome::Success),
            DestinationSyncStatus::new(Uuid::new_v4(), SyncOutcome::Failed),
        ]);

        assert_eq!(1, workspace.statuses().len());
        assert!(workspace.statuses().contains_key(&live_id));
    }

    #[test]
    fn every_task_starts_expanded_and_selecting_one_opens_it() {
        let memory = InMemoryFileSystem::new();
        {
            let mut workspace = workspace(&memory);
            workspace.tasks = vec![task("Photos"), task("Downloads")];
            workspace.persist_tasks();
        }

        let mut workspace = workspace(&memory);
        assert!(workspace.all_expanded());

        workspace.set_all_expanded(false);
        assert!(!workspace.all_expanded());

        let first = workspace.tasks()[0].id;
        workspace.select(first);
        assert!(workspace.is_expanded(first));
        assert_eq!(Some(first), workspace.selected());
    }

    #[test]
    fn a_settings_change_that_changes_nothing_is_not_written() {
        let memory = InMemoryFileSystem::new();
        let mut workspace = workspace(&memory);

        workspace.update_settings(|settings| settings.close_to_tray = false);
        assert!(memory
            .contents_of(format!(r"{CONFIG}\settings.json"))
            .is_none());

        workspace.update_settings(|settings| settings.close_to_tray = true);
        assert!(memory
            .contents_of(format!(r"{CONFIG}\settings.json"))
            .is_some());
    }

    #[test]
    fn settings_survive_a_reload() {
        let memory = InMemoryFileSystem::new();
        workspace(&memory).update_settings(|settings| settings.language = Some("ja".into()));

        assert_eq!(
            Some("ja".to_owned()),
            workspace(&memory).settings().language
        );
    }
}
