//! The persisted domain model.
//!
//! Every type here round-trips through the exact JSON shape the C# app writes: PascalCase
//! property names, a lowercase `"kind"` discriminator on the closed hierarchies, enums as
//! strings, and nulls omitted on write. Old config keeps loading; legacy shapes are
//! normalized on save rather than rejected.

mod destination;
mod settings;
mod status;
mod task;

pub use destination::{Destination, DEFAULT_MASS_DELETE_THRESHOLD};
pub use settings::AppSettings;
pub use status::{DestinationSyncStatus, SyncOutcome};
pub use task::SyncTask;

use serde::{Deserialize, Serialize};

/// What a task does, and so which destinations it accepts.
///
/// Move's postcondition (an emptied source) contradicts every other strategy's precondition
/// (the source is the truth), so mixtures have no coherent semantics. The kind is chosen while
/// the task is empty and locked afterwards.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncTaskKind {
    /// Destinations are Mirror or Add-only; each filters the whole source independently.
    Sync,
    /// Destinations are Move, and form an ordered rule list where the first match wins.
    Move,
}

/// How a destination is reconciled with the filtered source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncStrategy {
    /// The destination becomes identical to the source, empty directories included.
    /// Extras are deleted. Takes no file filters — a filtered subset contradicts tree identity.
    Mirror,
    /// New and changed files are copied; nothing is ever deleted. The safe accumulator.
    AddOnly,
    /// Matching files are moved out of the source.
    Move,
}

/// How Mirror removes files that are no longer in the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum DeleteMode {
    /// To the Recycle Bin, so deletions are recoverable. The default.
    ///
    /// Network shares have no Recycle Bin, so removals there are permanent regardless.
    #[default]
    Recycle,
    /// Deleted outright.
    Permanent,
}

/// How a flattening Move destination handles a file name that is already taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum FileNameCollisionPolicy {
    /// Leave the file in the source and report it. Nothing is renamed or overwritten.
    #[default]
    Skip,
    /// Move it under a numbered name — `report (2).pdf`.
    Suffix,
}

/// Where a destination's files live.
///
/// Open by design: cloud and SFTP locations are the reason the engine talks to a
/// `DestinationProvider` rather than to a path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all_fields = "PascalCase")]
pub enum DestinationLocation {
    /// A local or already-mounted network path.
    #[serde(rename = "local")]
    Local { path: String },
}

impl DestinationLocation {
    /// A local/mounted destination at `path`.
    pub fn local(path: impl Into<String>) -> Self {
        Self::Local { path: path.into() }
    }

    /// The path when this is a local/mounted location; empty otherwise. For display and the
    /// editor, which are path-based in phase 1.
    pub fn local_path(&self) -> &str {
        match self {
            Self::Local { path } => path,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enums_persist_as_their_csharp_names() {
        assert_eq!(
            r#""AddOnly""#,
            serde_json::to_string(&SyncStrategy::AddOnly).unwrap()
        );
        assert_eq!(
            r#""Mirror""#,
            serde_json::to_string(&SyncStrategy::Mirror).unwrap()
        );
        assert_eq!(
            r#""Move""#,
            serde_json::to_string(&SyncStrategy::Move).unwrap()
        );
        assert_eq!(
            r#""Recycle""#,
            serde_json::to_string(&DeleteMode::Recycle).unwrap()
        );
        assert_eq!(
            r#""Permanent""#,
            serde_json::to_string(&DeleteMode::Permanent).unwrap()
        );
        assert_eq!(
            r#""Skip""#,
            serde_json::to_string(&FileNameCollisionPolicy::Skip).unwrap()
        );
        assert_eq!(
            r#""Suffix""#,
            serde_json::to_string(&FileNameCollisionPolicy::Suffix).unwrap()
        );
        assert_eq!(
            r#""Sync""#,
            serde_json::to_string(&SyncTaskKind::Sync).unwrap()
        );
        assert_eq!(
            r#""Move""#,
            serde_json::to_string(&SyncTaskKind::Move).unwrap()
        );
    }

    #[test]
    fn a_local_destination_round_trips() {
        let location = DestinationLocation::local(r"D:\Backup");
        let json = r#"{"kind":"local","Path":"D:\\Backup"}"#;
        assert_eq!(json, serde_json::to_string(&location).unwrap());
        assert_eq!(
            location,
            serde_json::from_str::<DestinationLocation>(json).unwrap()
        );
        assert_eq!(r"D:\Backup", location.local_path());
    }
}
