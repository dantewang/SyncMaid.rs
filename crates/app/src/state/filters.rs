//! The two-level shape the filter editor presents, and its round trip to the stored rules.
//!
//! A destination stores a **flat list of rules with OR semantics**. The editor presents groups
//! instead — each group is "any rule" or "all rules", and the groups themselves combine by any
//! or all — because that covers the two shapes people actually ask for (`docs/ and jpg`,
//! `(docs/ or photos/) and jpg`) without a parser or an unbounded tree.
//!
//! Two rules govern the round trip:
//!
//! - **Collapse trivial shapes on save.** One group needs no wrapper; one rule in a group needs
//!   no group. A simple config keeps persisting the way it always did.
//! - **Never discard what the editor cannot represent.** Deeper nesting can only come from a
//!   hand-edited `tasks.json`; it is carried through untouched and shown as a read-only row
//!   rather than silently rewritten into something the user did not ask for.

use syncmaid_core::filtering::FilterRule;

/// Which of the three rule kinds the editor offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FilterKind {
    /// A folder relative to the source root, e.g. `photos/2024`.
    #[default]
    Path,
    /// A file extension, anywhere in the tree, e.g. `jpg`.
    Extension,
    /// A glob against the whole relative path, e.g. `**/ChatGPT*.png`.
    Wildcard,
}

impl FilterKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Path => "Path",
            Self::Extension => "Extension",
            Self::Wildcard => "Wildcard",
        }
    }

    pub fn index(self) -> usize {
        match self {
            Self::Path => 0,
            Self::Extension => 1,
            Self::Wildcard => 2,
        }
    }

    pub fn from_index(index: usize) -> Self {
        match index {
            1 => Self::Extension,
            2 => Self::Wildcard,
            _ => Self::Path,
        }
    }
}

/// One row in a group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterEntry {
    pub kind: FilterKind,
    pub pattern: String,
    /// Wrapped in a `Not` when stored.
    pub excluded: bool,
}

impl FilterEntry {
    pub fn new(kind: FilterKind, pattern: impl Into<String>) -> Self {
        Self {
            kind,
            pattern: pattern.into(),
            excluded: false,
        }
    }

    pub fn excluded(mut self) -> Self {
        self.excluded = true;
        self
    }

    fn to_rule(&self) -> FilterRule {
        let rule = match self.kind {
            FilterKind::Path => FilterRule::path(&self.pattern),
            FilterKind::Extension => FilterRule::extension(&self.pattern),
            FilterKind::Wildcard => FilterRule::wildcard(&self.pattern),
        };
        if self.excluded {
            FilterRule::not(rule)
        } else {
            rule
        }
    }

    /// What the preview line calls this rule.
    fn describe(&self) -> String {
        let body = match self.kind {
            FilterKind::Path => format!("{}/", self.pattern.trim_end_matches('/')),
            FilterKind::Extension | FilterKind::Wildcard => self.pattern.clone(),
        };
        if self.excluded {
            format!("not {body}")
        } else {
            body
        }
    }

    /// Reads one stored rule back, if the editor can represent it.
    fn of(rule: &FilterRule) -> Option<Self> {
        match rule {
            FilterRule::Not(inner) => {
                let mut entry = Self::of(&inner.rule)?;
                if entry.excluded {
                    return None; // A double negation is not a shape the editor draws.
                }
                entry.excluded = true;
                Some(entry)
            }
            FilterRule::Path(filter) => Some(Self::new(FilterKind::Path, filter.prefix())),
            FilterRule::Extension(filter) => {
                Some(Self::new(FilterKind::Extension, filter.extension()))
            }
            FilterRule::Wildcard(filter) => Some(Self::new(FilterKind::Wildcard, filter.pattern())),
            _ => None,
        }
    }
}

/// A set of rules combined by any or all.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FilterGroup {
    /// True for "all rules", false for "any rule".
    pub match_all: bool,
    pub rules: Vec<FilterEntry>,
}

impl FilterGroup {
    pub fn any(rules: Vec<FilterEntry>) -> Self {
        Self {
            match_all: false,
            rules,
        }
    }

    pub fn all(rules: Vec<FilterEntry>) -> Self {
        Self {
            match_all: true,
            rules,
        }
    }

    fn to_rule(&self) -> Option<FilterRule> {
        match self.rules.len() {
            0 => None,
            // One rule needs no group around it.
            1 => Some(self.rules[0].to_rule()),
            _ => {
                let rules: Vec<FilterRule> = self.rules.iter().map(FilterEntry::to_rule).collect();
                Some(if self.match_all {
                    FilterRule::all_of(rules)
                } else {
                    FilterRule::any_of(rules)
                })
            }
        }
    }

    fn describe(&self, parenthesise: bool) -> String {
        let joiner = if self.match_all { " and " } else { " or " };
        let body = self
            .rules
            .iter()
            .map(FilterEntry::describe)
            .collect::<Vec<_>>()
            .join(joiner);
        if parenthesise && self.rules.len() > 1 {
            format!("({body})")
        } else {
            body
        }
    }
}

/// What a set of rules selects, in words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Summary {
    /// No rule selects anything, so the destination would sync nothing.
    Nothing,
    /// What is selected, e.g. `docs/ and (jpg or png)`.
    Selection(String),
}

impl Summary {
    /// The selection, or a short stand-in saying there is none.
    pub fn text(&self) -> &str {
        match self {
            Self::Nothing => "nothing yet",
            Self::Selection(what) => what,
        }
    }
}

/// What the filter section of the destination editor is showing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterModel {
    /// "All files" rather than "Only matching".
    pub all_files: bool,
    /// True for "match all groups", false for "match any group".
    pub match_all_groups: bool,
    pub groups: Vec<FilterGroup>,
    /// Rules from a hand-edited config that the editor cannot draw. Kept verbatim and written
    /// back untouched.
    pub opaque: Vec<FilterRule>,
}

impl Default for FilterModel {
    fn default() -> Self {
        Self::all()
    }
}

impl FilterModel {
    /// Everything.
    pub fn all() -> Self {
        Self {
            all_files: true,
            match_all_groups: false,
            groups: Vec::new(),
            opaque: Vec::new(),
        }
    }

    /// Reads a destination's stored rules into the editor's shape.
    pub fn of(rules: &[FilterRule]) -> Self {
        if matches!(rules, [rule] if rule.is_all_files()) {
            return Self::all();
        }

        let mut model = Self {
            all_files: false,
            match_all_groups: false,
            groups: Vec::new(),
            opaque: Vec::new(),
        };

        // `[AllOf([...])]` is written by two different editor shapes — one all-group, or
        // several groups combined by all — and the two mean exactly the same thing. The
        // children decide which one it is read back as: a composite child can only have come
        // from a group, so its presence means several groups. All-bare children are the
        // commoner shape, one group whose rules are ANDed, and that is what they read back as.
        if let [FilterRule::AllOf(composite)] = rules {
            let holds_a_group = composite
                .rules
                .iter()
                .any(|rule| matches!(rule, FilterRule::AllOf(_) | FilterRule::AnyOf(_)));
            if holds_a_group {
                if let Some(groups) = read_groups(&composite.rules) {
                    model.match_all_groups = true;
                    model.groups = groups;
                    return model;
                }
            } else if let Some(entries) = read_entries(&composite.rules) {
                model.groups = vec![FilterGroup::all(entries)];
                return model;
            }
        }

        match read_groups(rules) {
            Some(groups) => model.groups = groups,
            None => model.opaque = rules.to_vec(),
        }
        if model.groups.is_empty() && model.opaque.is_empty() {
            // A destination with no rules at all: show one empty group to type into rather than
            // an editor with nowhere to start.
            model.groups.push(FilterGroup::default());
        }
        model
    }

    /// Writes the editor's shape back out, collapsing anything trivial.
    pub fn to_rules(&self) -> Vec<FilterRule> {
        if self.all_files {
            return vec![FilterRule::AllFiles];
        }
        if !self.opaque.is_empty() {
            return self.opaque.clone();
        }

        let groups: Vec<FilterRule> = self
            .groups
            .iter()
            .filter_map(FilterGroup::to_rule)
            .collect();
        if groups.len() > 1 && self.match_all_groups {
            return vec![FilterRule::all_of(groups)];
        }
        // "Match any group" is the flat list's own semantics, so it needs no wrapper at all.
        groups
    }

    /// The plain-language line under the editor.
    ///
    /// The single best guard against getting a filter wrong: it says, in words, what will be
    /// synced, and it updates as the rules do.
    pub fn describe(&self) -> String {
        match self.summary() {
            Summary::Nothing => "No rules yet — nothing will be synced.".to_owned(),
            Summary::Selection(what) => format!("Syncs: {what}"),
        }
    }

    /// What is selected, without the leading verb — the same words, for somewhere the verb is
    /// already on screen (a workspace row's one-line summary).
    pub fn summary(&self) -> Summary {
        if self.all_files {
            return Summary::Selection("all files".to_owned());
        }
        if !self.opaque.is_empty() {
            return Summary::Selection(
                "rules this editor can't show (kept as they are)".to_owned(),
            );
        }

        let described: Vec<String> = self
            .groups
            .iter()
            .filter(|group| !group.rules.is_empty())
            .map(|group| group.describe(self.groups.len() > 1))
            .collect();

        if described.is_empty() {
            return Summary::Nothing;
        }

        let joiner = if self.match_all_groups {
            " and "
        } else {
            " or "
        };
        Summary::Selection(described.join(joiner))
    }

    /// True when the destination would select nothing, which blocks the save.
    pub fn selects_nothing(&self) -> bool {
        !self.all_files
            && self.opaque.is_empty()
            && self.groups.iter().all(|group| group.rules.is_empty())
    }
}

/// Reads a flat OR list as one group per entry, or gives up.
fn read_groups(rules: &[FilterRule]) -> Option<Vec<FilterGroup>> {
    let mut groups = Vec::new();
    for rule in rules {
        match rule {
            FilterRule::AllOf(composite) => {
                groups.push(FilterGroup::all(read_entries(&composite.rules)?));
            }
            FilterRule::AnyOf(composite) => {
                groups.push(FilterGroup::any(read_entries(&composite.rules)?));
            }
            // A bare rule is a group of one; it came from the collapse on the way out.
            other => groups.push(FilterGroup::any(vec![FilterEntry::of(other)?])),
        }
    }
    Some(groups)
}

fn read_entries(rules: &[FilterRule]) -> Option<Vec<FilterEntry>> {
    rules.iter().map(FilterEntry::of).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(pattern: &str) -> FilterEntry {
        FilterEntry::new(FilterKind::Path, pattern)
    }

    fn extension(pattern: &str) -> FilterEntry {
        FilterEntry::new(FilterKind::Extension, pattern)
    }

    fn only(groups: Vec<FilterGroup>) -> FilterModel {
        FilterModel {
            all_files: false,
            match_all_groups: false,
            groups,
            opaque: Vec::new(),
        }
    }

    #[test]
    fn all_files_round_trips_as_the_lone_all_rule() {
        let model = FilterModel::all();

        assert_eq!(vec![FilterRule::AllFiles], model.to_rules());
        assert_eq!(model, FilterModel::of(&model.to_rules()));
        assert_eq!("Syncs: all files", model.describe());
    }

    #[test]
    fn a_single_rule_persists_bare_the_way_it_always_did() {
        let model = only(vec![FilterGroup::any(vec![extension("jpg")])]);

        assert_eq!(
            vec![FilterRule::extension("jpg")],
            model.to_rules(),
            "a simple config keeps the shape older versions wrote"
        );
        assert_eq!(model, FilterModel::of(&model.to_rules()));
    }

    #[test]
    fn several_bare_rules_are_the_flat_lists_own_or() {
        let model = only(vec![
            FilterGroup::any(vec![extension("jpg")]),
            FilterGroup::any(vec![extension("png")]),
        ]);

        assert_eq!(
            vec![FilterRule::extension("jpg"), FilterRule::extension("png")],
            model.to_rules()
        );
        assert_eq!("Syncs: jpg or png", model.describe());
    }

    #[test]
    fn a_group_of_several_rules_wraps_in_any_or_all() {
        let any = only(vec![FilterGroup::any(vec![path("docs"), path("photos")])]);
        let all = only(vec![FilterGroup::all(vec![path("docs"), extension("pdf")])]);

        assert_eq!(
            vec![FilterRule::any_of([
                FilterRule::path("docs"),
                FilterRule::path("photos")
            ])],
            any.to_rules()
        );
        assert_eq!(
            vec![FilterRule::all_of([
                FilterRule::path("docs"),
                FilterRule::extension("pdf")
            ])],
            all.to_rules()
        );
        assert_eq!(any, FilterModel::of(&any.to_rules()));
        assert_eq!(all, FilterModel::of(&all.to_rules()));
    }

    #[test]
    fn matching_all_groups_wraps_the_lot_and_round_trips() {
        let model = FilterModel {
            all_files: false,
            match_all_groups: true,
            groups: vec![
                FilterGroup::any(vec![path("docs"), path("photos")]),
                FilterGroup::any(vec![extension("jpg")]),
            ],
            opaque: Vec::new(),
        };

        let rules = model.to_rules();
        assert_eq!(1, rules.len());
        assert_eq!(model, FilterModel::of(&rules));
        assert_eq!("Syncs: (docs/ or photos/) and jpg", model.describe());
    }

    #[test]
    fn an_exclude_becomes_a_not_and_reads_back() {
        let model = only(vec![FilterGroup::all(vec![
            path("docs"),
            extension("tmp").excluded(),
        ])]);

        assert_eq!(
            vec![FilterRule::all_of([
                FilterRule::path("docs"),
                FilterRule::not(FilterRule::extension("tmp")),
            ])],
            model.to_rules()
        );
        assert_eq!(model, FilterModel::of(&model.to_rules()));
        assert_eq!("Syncs: docs/ and not tmp", model.describe());
    }

    #[test]
    fn a_lone_all_of_is_read_by_what_its_children_are() {
        // The two shapes persist identically and mean the same thing, so the read-back has to
        // pick one. All-bare children are the commoner "one group, ANDed" shape; a composite
        // child can only have come from a group, so it means several groups.
        let bare = vec![FilterRule::all_of([
            FilterRule::path("docs"),
            FilterRule::extension("pdf"),
        ])];
        let nested = vec![FilterRule::all_of([
            FilterRule::any_of([FilterRule::path("docs"), FilterRule::path("photos")]),
            FilterRule::extension("jpg"),
        ])];

        let one_group = FilterModel::of(&bare);
        assert!(!one_group.match_all_groups);
        assert_eq!(1, one_group.groups.len());
        assert!(one_group.groups[0].match_all);

        let several = FilterModel::of(&nested);
        assert!(several.match_all_groups);
        assert_eq!(2, several.groups.len());

        // Whichever way each is read, saving it puts the same rules back.
        assert_eq!(bare, one_group.to_rules());
        assert_eq!(nested, several.to_rules());
    }

    #[test]
    fn a_legacy_flat_list_loads_as_one_group_per_rule() {
        let legacy = vec![FilterRule::extension("jpg"), FilterRule::path("photos")];

        let model = FilterModel::of(&legacy);

        assert!(!model.all_files);
        assert_eq!(2, model.groups.len());
        assert_eq!(
            legacy,
            model.to_rules(),
            "loading and saving changes nothing"
        );
    }

    #[test]
    fn nesting_the_editor_cannot_draw_is_carried_through_untouched() {
        // Only a hand-edited tasks.json produces this.
        let deep = vec![FilterRule::any_of([FilterRule::all_of([
            FilterRule::extension("jpg"),
            FilterRule::path("photos"),
        ])])];

        let model = FilterModel::of(&deep);

        assert!(
            !model.opaque.is_empty(),
            "the editor recognises it cannot draw this"
        );
        assert_eq!(
            deep,
            model.to_rules(),
            "saving must not rewrite what the user asked for into something else"
        );
        assert!(model.describe().contains("can't show"));
    }

    #[test]
    fn a_double_negation_counts_as_something_the_editor_cannot_draw() {
        let doubled = vec![FilterRule::not(FilterRule::not(FilterRule::extension(
            "tmp",
        )))];

        let model = FilterModel::of(&doubled);

        assert_eq!(doubled, model.to_rules());
    }

    #[test]
    fn no_rules_at_all_says_so_and_blocks_the_save() {
        let empty = only(vec![FilterGroup::any(Vec::new())]);

        assert_eq!("No rules yet — nothing will be synced.", empty.describe());
        assert!(empty.selects_nothing());
        assert!(empty.to_rules().is_empty());
        assert!(!FilterModel::all().selects_nothing());
    }

    #[test]
    fn a_path_rule_reads_with_its_trailing_slash_once() {
        assert_eq!("docs/", path("docs").describe());
        assert_eq!("docs/", path("docs/").describe());
    }
}
