use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::filtering::FilterRule;
use crate::model::{DeleteMode, DestinationLocation, FileNameCollisionPolicy, SyncStrategy};

/// Fraction of the destination a single Mirror run may delete before it is treated as a
/// likely mistake and held for confirmation.
pub const DEFAULT_MASS_DELETE_THRESHOLD: f64 = 0.5;

/// A sync target: where filtered source files go, which files are selected, and how the
/// destination is reconciled.
///
/// Field order matches the C# declaration order so the persisted JSON is key-for-key
/// identical.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Destination {
    /// Display name.
    pub name: String,

    /// Where this destination's files live (local/mounted today; cloud/SFTP later).
    pub target: DestinationLocation,

    /// The rules selecting which source files sync here. A file is included when it matches
    /// **any** rule; with no rules, nothing is selected.
    pub filters: Vec<FilterRule>,

    /// How the destination is reconciled with the filtered source.
    pub strategy: SyncStrategy,

    /// Stable identity, preserved across edits so the destination's sync status survives
    /// renames.
    #[serde(default = "Uuid::new_v4")]
    pub id: Uuid,

    /// When true, every copied file is read back and its xxHash compared to the source before
    /// it is committed — guarding against silent corruption the basic length check can't see.
    ///
    /// Off by default; on a mounted network path this re-reads each file over the network, so
    /// the editor warns when enabling it there.
    #[serde(default)]
    pub verify_contents: bool,

    /// How Mirror removes files no longer in the source.
    #[serde(default)]
    pub delete_mode: DeleteMode,

    /// Move only. When true the file lands directly in the destination root, dropping the
    /// folders it happened to sit in. Off by default: the source-relative layout is what a
    /// backup needs, and it is the shape that cannot collide.
    #[serde(default)]
    pub flatten_structure: bool,

    /// How a flattening Move destination handles a name that is already taken. Ignored
    /// unless [`Destination::flatten_structure`].
    #[serde(default)]
    pub collision_policy: FileNameCollisionPolicy,

    /// Fraction (0–1) of the destination a single Mirror run may delete before it is aborted
    /// as a likely mistake. Set 0 to disable the ratio guard — the empty-source guard always
    /// applies regardless.
    #[serde(default = "default_mass_delete_threshold")]
    pub mass_delete_threshold: f64,
}

fn default_mass_delete_threshold() -> f64 {
    DEFAULT_MASS_DELETE_THRESHOLD
}

impl Destination {
    /// A destination at a local/mounted `path`, with everything else at its default.
    pub fn new(
        name: impl Into<String>,
        path: impl Into<String>,
        filters: impl IntoIterator<Item = FilterRule>,
        strategy: SyncStrategy,
    ) -> Self {
        Self::at(name, DestinationLocation::local(path), filters, strategy)
    }

    /// A destination at an arbitrary `target` location.
    pub fn at(
        name: impl Into<String>,
        target: DestinationLocation,
        filters: impl IntoIterator<Item = FilterRule>,
        strategy: SyncStrategy,
    ) -> Self {
        Self {
            name: name.into(),
            target,
            filters: filters.into_iter().collect(),
            strategy,
            id: Uuid::new_v4(),
            verify_contents: false,
            delete_mode: DeleteMode::default(),
            flatten_structure: false,
            collision_policy: FileNameCollisionPolicy::default(),
            mass_delete_threshold: DEFAULT_MASS_DELETE_THRESHOLD,
        }
    }

    /// The destination path when [`Destination::target`] is a local/mounted location; empty
    /// otherwise. For display and the editor, which are path-based in phase 1.
    pub fn local_path(&self) -> &str {
        self.target.local_path()
    }

    /// True when the source file at `relative_path` should sync to this destination.
    ///
    /// Rules are evaluated in order and the first match wins, which keeps behaviour
    /// predictable now that exclude-style rules exist. With no rules, nothing is selected.
    pub fn includes(&self, relative_path: &str) -> bool {
        self.filters.iter().any(|rule| rule.matches(relative_path))
    }

    /// True when this destination carries exactly the lone all-files filter a Mirror
    /// destination is allowed to have — not zero rules, not a duplicated one.
    pub fn has_only_the_all_files_filter(&self) -> bool {
        matches!(self.filters.as_slice(), [rule] if rule.is_all_files())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Destination {
        Destination::new(
            "Backup",
            r"D:\Backup",
            [FilterRule::AllFiles],
            SyncStrategy::Mirror,
        )
    }

    #[test]
    fn includes_matches_any_rule_and_nothing_without_rules() {
        let mut destination = Destination::new(
            "D",
            r"D:\d",
            [FilterRule::extension("jpg")],
            SyncStrategy::AddOnly,
        );
        assert!(destination.includes("a.jpg"));
        assert!(!destination.includes("a.png"));

        destination.filters.clear();
        assert!(
            !destination.includes("a.jpg"),
            "an empty filter list selects nothing — that is a half-built destination, not a wildcard"
        );
    }

    #[test]
    fn only_a_lone_all_files_filter_counts_as_the_mirror_filter() {
        let mut destination = sample();
        assert!(destination.has_only_the_all_files_filter());

        destination.filters = vec![];
        assert!(!destination.has_only_the_all_files_filter());

        destination.filters = vec![FilterRule::AllFiles, FilterRule::AllFiles];
        assert!(!destination.has_only_the_all_files_filter());

        destination.filters = vec![FilterRule::extension("jpg")];
        assert!(!destination.has_only_the_all_files_filter());
    }

    #[test]
    fn a_destination_persists_in_the_csharp_key_order_with_every_knob() {
        let destination = Destination {
            id: Uuid::nil(),
            ..sample()
        };
        let json = serde_json::to_string(&destination).unwrap();
        assert_eq!(
            concat!(
                r#"{"Name":"Backup","Target":{"kind":"local","Path":"D:\\Backup"},"#,
                r#""Filters":[{"kind":"all"}],"Strategy":"Mirror","#,
                r#""Id":"00000000-0000-0000-0000-000000000000","VerifyContents":false,"#,
                r#""DeleteMode":"Recycle","FlattenStructure":false,"CollisionPolicy":"Skip","#,
                r#""MassDeleteThreshold":0.5}"#
            ),
            json
        );
        assert_eq!(
            destination,
            serde_json::from_str::<Destination>(&json).unwrap()
        );
    }

    #[test]
    fn a_legacy_destination_without_the_later_knobs_loads_with_their_defaults() {
        let json = r#"{"Name":"D","Target":{"kind":"local","Path":"D:\\d"},
                       "Filters":[{"kind":"all"}],"Strategy":"Move"}"#;
        let destination: Destination = serde_json::from_str(json).unwrap();

        assert_eq!(SyncStrategy::Move, destination.strategy);
        assert!(!destination.verify_contents);
        assert_eq!(DeleteMode::Recycle, destination.delete_mode);
        assert!(!destination.flatten_structure);
        assert_eq!(FileNameCollisionPolicy::Skip, destination.collision_policy);
        assert_eq!(
            DEFAULT_MASS_DELETE_THRESHOLD,
            destination.mass_delete_threshold
        );
        assert_ne!(Uuid::nil(), destination.id, "a missing id gets a fresh one");
    }

    #[test]
    fn a_destination_missing_a_required_member_is_corrupt_rather_than_defaulted() {
        // `Filters` and `Strategy` are positional record parameters in C# with
        // RespectRequiredConstructorParameters on — absent means the file is corrupt, which is
        // what makes the .bak fallback fire instead of silently syncing nothing.
        let json = r#"{"Name":"D","Target":{"kind":"local","Path":"D:\\d"}}"#;
        assert!(serde_json::from_str::<Destination>(json).is_err());
    }
}
