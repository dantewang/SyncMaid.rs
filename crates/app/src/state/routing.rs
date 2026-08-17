//! Reads one routing rule against an earlier one to answer a single question: can the later
//! rule ever match anything?
//!
//! Under first-match-wins, overlapping rules are normal and wanted — `*.pdf` beside `invoices/`
//! is the routine case and not a problem at all. The mistake worth catching is the overlap that
//! *hides* a rule completely.
//!
//! **Deliberately partial.** Deciding disjointness over the whole filter algebra is possible,
//! but it reports "overlap" for pairs that never collide in a real tree, and a warning that
//! cries wolf is worse than no warning. So this proves subsumption only for the shapes where it
//! is unarguable — an all-files rule, a shorter extension, an ancestor path — and stays quiet
//! otherwise. **A silent verdict means "no proof", never "no overlap".**

use syncmaid_core::filtering::FilterRule;
use syncmaid_core::model::Destination;

/// True when `earlier` provably takes every file `later` would, so `later` can never match.
pub fn subsumes(earlier: &Destination, later: &Destination) -> bool {
    if matches!(earlier.filters.as_slice(), [FilterRule::AllFiles]) {
        return true;
    }

    // A rule with no filters selects nothing, so it is unreachable for its own reasons — not
    // something an earlier rule did to it.
    if later.filters.is_empty() {
        return false;
    }

    let (Some(covering), Some(covered)) = (leaves(earlier), leaves(later)) else {
        return false;
    };

    covered
        .iter()
        .all(|rule| covering.iter().any(|candidate| covers(candidate, rule)))
}

/// Only the shape the editor writes for a simple rule: a flat OR list of path and extension
/// leaves. Anything with and/or/not structure is left alone rather than half-understood.
fn leaves(destination: &Destination) -> Option<Vec<&FilterRule>> {
    let mut leaves = Vec::with_capacity(destination.filters.len());
    for filter in &destination.filters {
        match filter {
            FilterRule::Path(_) | FilterRule::Extension(_) => leaves.push(filter),
            _ => return None,
        }
    }
    (!leaves.is_empty()).then_some(leaves)
}

fn covers(earlier: &FilterRule, later: &FilterRule) -> bool {
    match (earlier, later) {
        // "gz" also matches "archive.tar.gz", so the shorter extension covers the longer one.
        // The comparison includes the dot, so "gz" does not cover "targz".
        (FilterRule::Extension(first), FilterRule::Extension(second)) => {
            let first = format!(".{}", first.extension()).to_lowercase();
            format!(".{}", second.extension())
                .to_lowercase()
                .ends_with(&first)
        }

        // An ancestor folder covers everything under it, itself included.
        (FilterRule::Path(first), FilterRule::Path(second)) => {
            let first = first.prefix().to_lowercase();
            let second = second.prefix().to_lowercase();
            second == first || second.starts_with(&format!("{first}/"))
        }

        // A folder does not cover a file type, nor the other way round.
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use syncmaid_core::model::SyncStrategy;

    use super::*;

    fn rule(filters: Vec<FilterRule>) -> Destination {
        Destination::new("R", r"D:\r", filters, SyncStrategy::Move)
    }

    #[test]
    fn an_all_files_rule_hides_everything_after_it() {
        assert!(subsumes(
            &rule(vec![FilterRule::AllFiles]),
            &rule(vec![FilterRule::extension("pdf")])
        ));
    }

    #[test]
    fn a_shorter_extension_covers_a_longer_one_but_only_at_a_dot() {
        assert!(subsumes(
            &rule(vec![FilterRule::extension("gz")]),
            &rule(vec![FilterRule::extension("tar.gz")])
        ));
        assert!(
            !subsumes(
                &rule(vec![FilterRule::extension("gz")]),
                &rule(vec![FilterRule::extension("targz")])
            ),
            "'targz' is a different type, not a longer form of 'gz'"
        );
    }

    #[test]
    fn an_ancestor_folder_covers_what_is_under_it() {
        assert!(subsumes(
            &rule(vec![FilterRule::path("photos")]),
            &rule(vec![FilterRule::path("Photos/2024")])
        ));
        assert!(!subsumes(
            &rule(vec![FilterRule::path("photos/2024")]),
            &rule(vec![FilterRule::path("photos")])
        ));
        assert!(
            !subsumes(
                &rule(vec![FilterRule::path("photo")]),
                &rule(vec![FilterRule::path("photos")])
            ),
            "a prefix of a folder's name is not an ancestor of it"
        );
    }

    #[test]
    fn a_folder_and_a_file_type_never_cover_each_other() {
        assert!(!subsumes(
            &rule(vec![FilterRule::path("invoices")]),
            &rule(vec![FilterRule::extension("pdf")])
        ));
        assert!(!subsumes(
            &rule(vec![FilterRule::extension("pdf")]),
            &rule(vec![FilterRule::path("invoices")])
        ));
    }

    #[test]
    fn every_leaf_of_the_later_rule_has_to_be_covered() {
        let earlier = rule(vec![FilterRule::extension("jpg")]);
        assert!(subsumes(
            &earlier,
            &rule(vec![FilterRule::extension("jpg")])
        ));
        assert!(
            !subsumes(
                &earlier,
                &rule(vec![FilterRule::extension("jpg"), FilterRule::path("raw")])
            ),
            "the later rule still reaches raw/, so it is not hidden"
        );
    }

    #[test]
    fn a_shape_the_analysis_cannot_read_stays_quiet() {
        // Not "these do not overlap" — "no proof either way", which is the only honest answer
        // for a composite the editor never writes.
        let earlier = rule(vec![FilterRule::any_of([FilterRule::extension("jpg")])]);
        assert!(!subsumes(
            &earlier,
            &rule(vec![FilterRule::extension("jpg")])
        ));
        assert!(!subsumes(
            &rule(vec![FilterRule::extension("jpg")]),
            &rule(vec![FilterRule::wildcard("**/*.jpg")])
        ));
    }

    #[test]
    fn a_rule_that_selects_nothing_is_not_reported_as_hidden() {
        // It is unreachable for its own reasons, and blaming the rule above it would send the
        // user off to change the wrong thing.
        assert!(!subsumes(
            &rule(vec![FilterRule::extension("pdf")]),
            &rule(vec![])
        ));
    }
}
