use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::model::{Destination, SyncStrategy, SyncTaskKind};
use crate::triggers::Trigger;

/// A unit of work: one source synced to one or more destinations, started by a trigger.
/// One-directional only (source → destinations).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "SyncTaskRepr", into = "SyncTaskRepr")]
pub struct SyncTask {
    /// Display name.
    pub name: String,

    /// The folder this task reads. Never modified except by the Move strategy.
    pub source_path: String,

    /// What starts a run.
    pub trigger: Trigger,

    /// Where files go. For a Move task this is an **ordered** rule list whose order is
    /// semantic — persist it, and never reorder it as a side effect of anything.
    pub destinations: Vec<Destination>,

    /// Stable identity, preserved across edits so external state keyed by the task survives
    /// renames.
    pub id: Uuid,

    /// `None` means "config written before the field existed", which has to stay
    /// distinguishable from an explicit `Sync` — see [`SyncTask::kind`].
    kind: Option<SyncTaskKind>,
}

/// The persisted form. `Kind` is always written even when it was derived, so one save is
/// enough to make a legacy task explicit.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct SyncTaskRepr {
    name: String,
    source_path: String,
    trigger: Trigger,
    destinations: Vec<Destination>,
    #[serde(default = "Uuid::new_v4")]
    id: Uuid,
    #[serde(default)]
    kind: Option<SyncTaskKind>,
}

impl SyncTask {
    /// A new task whose kind is derived from its destinations until the editor sets one.
    pub fn new(
        name: impl Into<String>,
        source_path: impl Into<String>,
        trigger: Trigger,
        destinations: impl IntoIterator<Item = Destination>,
    ) -> Self {
        Self {
            name: name.into(),
            source_path: source_path.into(),
            trigger,
            destinations: destinations.into_iter().collect(),
            id: Uuid::new_v4(),
            kind: None,
        }
    }

    /// What this task does, and so which destinations it accepts.
    ///
    /// Config written before the field existed carries no value; the destinations imply it,
    /// since Move was exclusive then — so it is derived from them and written back on the next
    /// save. A task with no destinations yet has nothing to derive from and is a Sync task
    /// until the user says otherwise, which is why the field exists at all: it gives a fresh
    /// task a shape the editor can tailor to.
    pub fn kind(&self) -> SyncTaskKind {
        self.kind.unwrap_or_else(|| {
            if self
                .destinations
                .iter()
                .any(|d| d.strategy == SyncStrategy::Move)
            {
                SyncTaskKind::Move
            } else {
                SyncTaskKind::Sync
            }
        })
    }

    /// Pins the kind. The editor does this while the task is still empty; afterwards the kind
    /// is locked, because Move and the copying strategies have no coherent mixture.
    pub fn set_kind(&mut self, kind: SyncTaskKind) {
        self.kind = Some(kind);
    }

    /// Builder form of [`SyncTask::set_kind`].
    pub fn with_kind(mut self, kind: SyncTaskKind) -> Self {
        self.set_kind(kind);
        self
    }

    /// False when the kind is still being derived from the destinations — i.e. this task was
    /// loaded from config written before the field existed.
    pub fn has_explicit_kind(&self) -> bool {
        self.kind.is_some()
    }

    /// True when `strategy` is one this task's kind accepts.
    pub fn accepts(&self, strategy: SyncStrategy) -> bool {
        match self.kind() {
            SyncTaskKind::Move => strategy == SyncStrategy::Move,
            SyncTaskKind::Sync => strategy != SyncStrategy::Move,
        }
    }
}

impl From<SyncTaskRepr> for SyncTask {
    fn from(repr: SyncTaskRepr) -> Self {
        Self {
            name: repr.name,
            source_path: repr.source_path,
            trigger: repr.trigger,
            destinations: repr.destinations,
            id: repr.id,
            kind: repr.kind,
        }
    }
}

impl From<SyncTask> for SyncTaskRepr {
    fn from(task: SyncTask) -> Self {
        // Resolve before writing: the nullable field only exists so a *load* can tell
        // "never written" from "explicitly Sync". Nothing should have to keep guessing.
        let kind = Some(task.kind());
        Self {
            name: task.name,
            source_path: task.source_path,
            trigger: task.trigger,
            destinations: task.destinations,
            id: task.id,
            kind,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filtering::FilterRule;

    fn destination(strategy: SyncStrategy) -> Destination {
        Destination::new("D", r"D:\d", [FilterRule::AllFiles], strategy)
    }

    #[test]
    fn an_empty_task_without_an_explicit_kind_is_a_sync_task() {
        let task = SyncTask::new("T", r"C:\src", Trigger::Manual, []);
        assert_eq!(SyncTaskKind::Sync, task.kind());
        assert!(!task.has_explicit_kind());
    }

    #[test]
    fn a_legacy_task_derives_move_from_its_destinations() {
        let task = SyncTask::new(
            "T",
            r"C:\src",
            Trigger::Manual,
            [destination(SyncStrategy::Move)],
        );
        assert_eq!(SyncTaskKind::Move, task.kind());
    }

    #[test]
    fn an_explicit_kind_wins_over_what_the_destinations_imply() {
        let task = SyncTask::new(
            "T",
            r"C:\src",
            Trigger::Manual,
            [destination(SyncStrategy::Move)],
        )
        .with_kind(SyncTaskKind::Sync);
        assert_eq!(SyncTaskKind::Sync, task.kind());
        assert!(
            !task.accepts(SyncStrategy::Move),
            "the engine must refuse this task rather than guess which destination is the odd one"
        );
    }

    #[test]
    fn a_tasks_kind_decides_which_strategies_it_accepts() {
        let sync = SyncTask::new("T", r"C:\src", Trigger::Manual, []).with_kind(SyncTaskKind::Sync);
        assert!(sync.accepts(SyncStrategy::Mirror));
        assert!(sync.accepts(SyncStrategy::AddOnly));
        assert!(!sync.accepts(SyncStrategy::Move));

        let moving =
            SyncTask::new("T", r"C:\src", Trigger::Manual, []).with_kind(SyncTaskKind::Move);
        assert!(moving.accepts(SyncStrategy::Move));
        assert!(!moving.accepts(SyncStrategy::Mirror));
        assert!(!moving.accepts(SyncStrategy::AddOnly));
    }

    #[test]
    fn a_task_persists_in_the_csharp_key_order() {
        let task = SyncTask {
            id: Uuid::nil(),
            ..SyncTask::new("Sort", r"C:\src", Trigger::Manual, [])
        };
        assert_eq!(
            concat!(
                r#"{"Name":"Sort","SourcePath":"C:\\src","Trigger":{"kind":"manual"},"#,
                r#""Destinations":[],"Id":"00000000-0000-0000-0000-000000000000","Kind":"Sync"}"#
            ),
            serde_json::to_string(&task).unwrap()
        );
    }

    #[test]
    fn a_legacy_task_without_kind_loads_derived_and_saves_explicit() {
        let legacy = concat!(
            r#"{"Name":"Sort","SourcePath":"C:\\src","Trigger":{"kind":"manual"},"#,
            r#""Destinations":[{"Name":"D","Target":{"kind":"local","Path":"D:\\d"},"#,
            r#""Filters":[{"kind":"all"}],"Strategy":"Move"}]}"#
        );

        let task: SyncTask = serde_json::from_str(legacy).unwrap();
        assert!(!task.has_explicit_kind());
        assert_eq!(SyncTaskKind::Move, task.kind());

        let saved = serde_json::to_string(&task).unwrap();
        assert!(
            saved.contains(r#""Kind":"Move""#),
            "one save makes a legacy task explicit: {saved}"
        );

        let reloaded: SyncTask = serde_json::from_str(&saved).unwrap();
        assert!(reloaded.has_explicit_kind());
        assert_eq!(SyncTaskKind::Move, reloaded.kind());
    }

    #[test]
    fn a_task_missing_a_required_member_is_corrupt_rather_than_defaulted() {
        let json = r#"{"Name":"Sort","Trigger":{"kind":"manual"},"Destinations":[]}"#;
        assert!(
            serde_json::from_str::<SyncTask>(json).is_err(),
            "SourcePath is required"
        );
    }

    #[test]
    fn saving_twice_is_byte_stable() {
        let task = SyncTask::new(
            "T",
            r"C:\src",
            Trigger::watch(),
            [destination(SyncStrategy::Mirror)],
        );
        let once = serde_json::to_string(&task).unwrap();
        let twice =
            serde_json::to_string(&serde_json::from_str::<SyncTask>(&once).unwrap()).unwrap();
        assert_eq!(once, twice);
    }
}
