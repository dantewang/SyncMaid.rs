//! First-match-wins routing for a Move task.
//!
//! A file can only be moved once, so a Move task's destinations are not independent filters —
//! they are an ordered rule list. Each source file is assigned to the first destination whose
//! filters include it and to no other; files nothing matches stay in the source. Independent
//! per-destination filtering would hand the same file to two destinations, and the second
//! would fail on a source that is already gone.
//!
//! Overlap between rules is deliberately legal. Requiring disjoint rules was considered and
//! rejected: the commonest routing pair — `*.pdf` and `invoices/` — overlaps and is perfectly
//! sensible. Order is what resolves it, which is why destination order is semantic and must
//! never be reordered as a side effect of anything.

use std::collections::BTreeMap;

use crate::io::ListedFile;
use crate::model::Destination;

/// Who gets which file, computed once per run before any planning.
#[derive(Debug, Clone, Default)]
pub struct MoveRouting {
    per_destination: Vec<Vec<ListedFile>>,
    unmatched: Vec<ListedFile>,
    contested: BTreeMap<String, Vec<usize>>,
}

impl MoveRouting {
    /// Assigns every file to the first destination that includes it.
    pub fn route(destinations: &[Destination], files: &[ListedFile]) -> Self {
        let mut per_destination = vec![Vec::new(); destinations.len()];
        let mut unmatched = Vec::new();
        let mut contested = BTreeMap::new();

        for file in files {
            let matches: Vec<usize> = destinations
                .iter()
                .enumerate()
                .filter(|(_, destination)| destination.includes(&file.relative_path))
                .map(|(index, _)| index)
                .collect();

            match matches.first() {
                Some(&winner) => {
                    per_destination[winner].push(file.clone());
                    if matches.len() > 1 {
                        // Not a problem — the editor shows it so the user can see that the
                        // order is doing real work.
                        contested.insert(file.relative_path.clone(), matches);
                    }
                }
                None => unmatched.push(file.clone()),
            }
        }

        Self {
            per_destination,
            unmatched,
            contested,
        }
    }

    /// The files assigned to the destination at `index`.
    pub fn for_destination(&self, index: usize) -> &[ListedFile] {
        self.per_destination.get(index).map_or(&[], Vec::as_slice)
    }

    /// Files no rule matched. They stay in the source, which is a normal outcome.
    pub fn unmatched(&self) -> &[ListedFile] {
        &self.unmatched
    }

    /// Files more than one rule matched, with every matching index — the first of which won.
    pub fn contested(&self) -> &BTreeMap<String, Vec<usize>> {
        &self.contested
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;
    use crate::filtering::FilterRule;
    use crate::io::FileStamp;
    use crate::model::SyncStrategy;

    fn file(relative_path: &str) -> ListedFile {
        ListedFile {
            relative_path: relative_path.into(),
            stamp: FileStamp::new(1, Utc::now()),
        }
    }

    fn rule(name: &str, filters: Vec<FilterRule>) -> Destination {
        Destination::new(name, format!(r"D:\{name}"), filters, SyncStrategy::Move)
    }

    fn paths(files: &[ListedFile]) -> Vec<&str> {
        files.iter().map(|f| f.relative_path.as_str()).collect()
    }

    #[test]
    fn each_file_goes_to_the_first_rule_that_matches_it() {
        let destinations = vec![
            rule("Books", vec![FilterRule::extension("pdf")]),
            rule("Bills", vec![FilterRule::path("invoices")]),
        ];
        let files = vec![
            file("a.pdf"),
            file("invoices/b.pdf"),
            file("invoices/c.txt"),
        ];

        let routing = MoveRouting::route(&destinations, &files);

        assert_eq!(
            vec!["a.pdf", "invoices/b.pdf"],
            paths(routing.for_destination(0))
        );
        assert_eq!(vec!["invoices/c.txt"], paths(routing.for_destination(1)));
    }

    #[test]
    fn an_overlap_is_recorded_rather_than_refused() {
        let destinations = vec![
            rule("Books", vec![FilterRule::extension("pdf")]),
            rule("Bills", vec![FilterRule::path("invoices")]),
        ];
        let files = vec![file("invoices/b.pdf")];

        let routing = MoveRouting::route(&destinations, &files);

        assert_eq!(vec![0, 1], routing.contested()["invoices/b.pdf"]);
        assert_eq!(
            vec!["invoices/b.pdf"],
            paths(routing.for_destination(0)),
            "the first rule takes it, and only the first"
        );
        assert!(routing.for_destination(1).is_empty());
    }

    #[test]
    fn reordering_the_rules_reassigns_the_contested_file() {
        let files = vec![file("invoices/b.pdf")];
        let books_first = vec![
            rule("Books", vec![FilterRule::extension("pdf")]),
            rule("Bills", vec![FilterRule::path("invoices")]),
        ];
        let bills_first = vec![
            rule("Bills", vec![FilterRule::path("invoices")]),
            rule("Books", vec![FilterRule::extension("pdf")]),
        ];

        assert_eq!("Books", winner_name(&books_first, &files));
        assert_eq!(
            "Bills",
            winner_name(&bills_first, &files),
            "destination order is semantic, so nothing may reorder it as a side effect"
        );
    }

    #[test]
    fn files_nothing_matches_stay_in_the_source() {
        let destinations = vec![rule("Books", vec![FilterRule::extension("pdf")])];
        let files = vec![file("a.pdf"), file("notes.txt")];

        let routing = MoveRouting::route(&destinations, &files);

        assert_eq!(vec!["notes.txt"], paths(routing.unmatched()));
    }

    #[test]
    fn a_catch_all_rule_empties_the_source_completely() {
        let destinations = vec![
            rule("Books", vec![FilterRule::extension("pdf")]),
            rule("To sort", vec![FilterRule::AllFiles]),
        ];
        let files = vec![file("a.pdf"), file("notes.txt")];

        let routing = MoveRouting::route(&destinations, &files);

        assert!(routing.unmatched().is_empty());
        assert_eq!(vec!["notes.txt"], paths(routing.for_destination(1)));
    }

    #[test]
    fn a_destination_with_no_rules_takes_nothing() {
        let destinations = vec![rule("Empty", vec![])];
        let files = vec![file("a.pdf")];

        let routing = MoveRouting::route(&destinations, &files);

        assert!(routing.for_destination(0).is_empty());
        assert_eq!(vec!["a.pdf"], paths(routing.unmatched()));
    }

    fn winner_name<'a>(destinations: &'a [Destination], files: &[ListedFile]) -> &'a str {
        let routing = MoveRouting::route(destinations, files);
        let index = (0..destinations.len())
            .find(|&index| !routing.for_destination(index).is_empty())
            .expect("some rule matched");
        &destinations[index].name
    }
}
