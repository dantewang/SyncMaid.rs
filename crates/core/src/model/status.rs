use chrono::{DateTime, FixedOffset};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The outcome of a destination's most recent sync.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SyncOutcome {
    /// Has not been synced yet.
    #[default]
    Never,
    /// A sync is currently in progress. Transient — never meaningfully persisted.
    Running,
    /// The last sync completed successfully.
    Success,
    /// The last sync applied everything it could, but left one or more files for the next run
    /// because they were in use. Nothing is wrong; the destination is simply not caught up.
    Incomplete,
    /// The last sync failed; see [`DestinationSyncStatus::error`].
    Failed,
    /// A Mirror run was blocked by the mass-delete guard and is awaiting confirmation before
    /// it will delete anything.
    NeedsConfirmation,
}

/// The persisted result of a destination's last sync, keyed by the destination's stable id so
/// it survives renames and path changes.
///
/// `status.json` holds a flat list of these; it is indexed by `destination_id` on load.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DestinationSyncStatus {
    /// The destination this describes.
    pub destination_id: Uuid,

    /// How the last run ended.
    pub outcome: SyncOutcome,

    /// When the last run finished.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run: Option<DateTime<FixedOffset>>,

    /// How many files the last run copied or moved.
    #[serde(default)]
    pub files_copied: i32,

    /// Why the last run failed, if it did. English — this comes from the engine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,

    /// How many files the last run left for the next one because they were in use.
    #[serde(default)]
    pub files_deferred: i32,

    /// The relative paths a run actually copied (or moved).
    ///
    /// Transient run detail so the UI can count **distinct** files across a burst of coalesced
    /// runs; never persisted — `status.json` keeps only the count.
    #[serde(skip)]
    pub copied_relative_paths: Vec<String>,

    /// The relative paths that were deferred. Transient run detail — logged so the culprit is
    /// identifiable; `status.json` keeps only the count.
    #[serde(skip)]
    pub deferred_relative_paths: Vec<String>,
}

impl DestinationSyncStatus {
    /// The "not yet run" status for a destination.
    pub fn never(destination_id: Uuid) -> Self {
        Self {
            destination_id,
            outcome: SyncOutcome::Never,
            last_run: None,
            files_copied: 0,
            error: None,
            files_deferred: 0,
            copied_relative_paths: Vec::new(),
            deferred_relative_paths: Vec::new(),
        }
    }

    /// A status with the given outcome and nothing else set.
    pub fn new(destination_id: Uuid, outcome: SyncOutcome) -> Self {
        Self {
            outcome,
            ..Self::never(destination_id)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcomes_persist_as_their_csharp_names() {
        assert_eq!(
            r#""NeedsConfirmation""#,
            serde_json::to_string(&SyncOutcome::NeedsConfirmation).unwrap()
        );
        assert_eq!(
            r#""Incomplete""#,
            serde_json::to_string(&SyncOutcome::Incomplete).unwrap()
        );
    }

    #[test]
    fn a_never_run_status_omits_its_nulls() {
        let status = DestinationSyncStatus::never(Uuid::nil());
        assert_eq!(
            concat!(
                r#"{"DestinationId":"00000000-0000-0000-0000-000000000000","Outcome":"Never","#,
                r#""FilesCopied":0,"FilesDeferred":0}"#
            ),
            serde_json::to_string(&status).unwrap()
        );
    }

    #[test]
    fn a_full_status_round_trips_and_drops_its_transient_paths() {
        let status = DestinationSyncStatus {
            last_run: Some(DateTime::parse_from_rfc3339("2026-08-09T02:00:12.418+08:00").unwrap()),
            files_copied: 128,
            error: Some("Failed to copy 'raw/img.dng': access denied.".into()),
            files_deferred: 2,
            copied_relative_paths: vec!["a.jpg".into()],
            deferred_relative_paths: vec!["b.jpg".into()],
            ..DestinationSyncStatus::new(Uuid::nil(), SyncOutcome::Failed)
        };

        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains(r#""FilesCopied":128"#));
        assert!(json.contains(r#""FilesDeferred":2"#));
        assert!(
            !json.contains("a.jpg"),
            "copied paths are run detail, not persisted state"
        );
        assert!(
            !json.contains("b.jpg"),
            "deferred paths are run detail, not persisted state"
        );

        let reloaded: DestinationSyncStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(status.last_run, reloaded.last_run);
        assert_eq!(status.error, reloaded.error);
        assert!(reloaded.copied_relative_paths.is_empty());
    }
}
