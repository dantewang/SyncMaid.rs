//! The routing preview: one walk of the source, then exactly the assignment a run would make.
//!
//! Rules are easy to write and hard to be sure of. This reads the source and reports where each
//! file would go, using the engine's own first-match-wins routing rather than a second
//! implementation of it — so what the preview shows is what a run does. It reads only: nothing
//! is written and no plan is applied.

use std::collections::HashMap;
use std::sync::Arc;

use syncmaid_core::io::{FileSystem, ListedFile};
use syncmaid_core::model::{Destination, SyncTask, SyncTaskKind};
use syncmaid_core::sync::MoveRouting;
use uuid::Uuid;

/// How many file names a sample carries. Enough to recognize what was caught, short enough to
/// read at a glance.
const SAMPLE_SIZE: usize = 8;

/// How many file types the chips offer. The long tail of a real folder is not worth a row.
const EXTENSION_CHIPS: usize = 12;

/// What one destination would take.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DestinationPreview {
    pub count: usize,
    pub sample: Vec<String>,
}

/// One file type present in the source, offered as a one-click rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionChip {
    /// The bare extension, e.g. `pdf`.
    pub extension: String,
    pub count: usize,
}

impl ExtensionChip {
    /// What the chip reads, e.g. `pdf (12)`.
    pub fn label(&self) -> String {
        format!("{} ({})", self.extension, self.count)
    }
}

/// A file more than one rule matched, and the rules that wanted it. **Information, not a
/// problem**: the ordering resolved it, and seeing which rule won is the point.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContestedFile {
    pub relative_path: String,
    /// Rule positions, 0-based, in order. The first one wins.
    pub rules: Vec<usize>,
}

/// Everything one scan found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scan {
    pub file_count: usize,
    pub per_destination: HashMap<Uuid, DestinationPreview>,
    /// Files no rule claims, which therefore stay in the source.
    pub unmatched: DestinationPreview,
    pub contested: Vec<ContestedFile>,
    pub extensions: Vec<ExtensionChip>,
}

/// Walks the source and assigns every file. Blocking — the caller runs it off the UI thread.
pub fn scan(
    file_system: &Arc<dyn FileSystem>,
    task: &SyncTask,
    destinations: &[Destination],
) -> std::io::Result<Scan> {
    let files = file_system
        .list_tree(std::path::Path::new(&task.source_path))?
        .files;

    let mut result = Scan {
        file_count: files.len(),
        extensions: extensions(&files),
        ..Default::default()
    };
    for destination in destinations {
        result
            .per_destination
            .insert(destination.id, DestinationPreview::default());
    }

    match task.kind() {
        SyncTaskKind::Move => route(&mut result, destinations, &files),
        SyncTaskKind::Sync => claim(&mut result, destinations, &files),
    }

    Ok(result)
}

/// Move: one owner per file, decided by rule order.
fn route(result: &mut Scan, destinations: &[Destination], files: &[ListedFile]) {
    let routing = MoveRouting::route(destinations, files);

    for (index, destination) in destinations.iter().enumerate() {
        let assigned = routing.for_destination(index);
        result.per_destination.insert(
            destination.id,
            DestinationPreview {
                count: assigned.len(),
                sample: sample_of(assigned),
            },
        );
    }

    result.unmatched = DestinationPreview {
        count: routing.unmatched().len(),
        sample: sample_of(routing.unmatched()),
    };
    result.contested = routing
        .contested()
        .iter()
        .take(SAMPLE_SIZE)
        .map(|(path, rules)| ContestedFile {
            relative_path: path.clone(),
            rules: rules.clone(),
        })
        .collect();
}

/// Sync: destinations are independent, so a file can land in several of them, and "unmatched"
/// means no destination wanted it at all.
fn claim(result: &mut Scan, destinations: &[Destination], files: &[ListedFile]) {
    for file in files {
        let mut claimed = false;
        for destination in destinations {
            if !destination.includes(&file.relative_path) {
                continue;
            }
            claimed = true;
            let entry = result.per_destination.entry(destination.id).or_default();
            entry.count += 1;
            if entry.sample.len() < SAMPLE_SIZE {
                entry.sample.push(file.relative_path.clone());
            }
        }

        if !claimed {
            result.unmatched.count += 1;
            if result.unmatched.sample.len() < SAMPLE_SIZE {
                result.unmatched.sample.push(file.relative_path.clone());
            }
        }
    }
}

fn sample_of(files: &[ListedFile]) -> Vec<String> {
    files
        .iter()
        .take(SAMPLE_SIZE)
        .map(|file| file.relative_path.clone())
        .collect()
}

/// The file types the source actually holds, commonest first. Authoring a rule is then picking
/// from what is there rather than guessing at globs.
fn extensions(files: &[ListedFile]) -> Vec<ExtensionChip> {
    let mut counts: HashMap<String, (String, usize)> = HashMap::new();
    for file in files {
        let Some(extension) = file
            .relative_path
            .rsplit('/')
            .next()
            .and_then(|leaf| leaf.rsplit_once('.'))
            .map(|(_, extension)| extension)
            .filter(|extension| !extension.is_empty())
        else {
            continue;
        };
        // Counted case-insensitively, shown as first seen: `JPG` and `jpg` are one type, and a
        // rule for either matches both.
        let entry = counts
            .entry(extension.to_lowercase())
            .or_insert_with(|| (extension.to_owned(), 0));
        entry.1 += 1;
    }

    let mut chips: Vec<_> = counts
        .into_values()
        .map(|(extension, count)| ExtensionChip { extension, count })
        .collect();
    // Commonest first, then alphabetical, so the order is stable across scans of the same tree.
    chips.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| a.extension.to_lowercase().cmp(&b.extension.to_lowercase()))
    });
    chips.truncate(EXTENSION_CHIPS);
    chips
}

#[cfg(test)]
mod tests {
    use syncmaid_core::filtering::FilterRule;
    use syncmaid_core::io::InMemoryFileSystem;
    use syncmaid_core::model::{Destination, SyncStrategy};
    use syncmaid_core::triggers::Trigger;

    use super::*;

    const SOURCE: &str = r"C:\src";

    fn file_system(paths: &[&str]) -> Arc<dyn FileSystem> {
        let memory = InMemoryFileSystem::new();
        memory.add_directory(SOURCE);
        for path in paths {
            memory.add_file(format!(r"{SOURCE}\{}", path.replace('/', r"\")), b"x");
        }
        Arc::new(memory)
    }

    fn task(kind: SyncTaskKind, destinations: Vec<Destination>) -> SyncTask {
        let mut task = SyncTask::new("T", SOURCE, Trigger::Manual, destinations);
        task.set_kind(kind);
        task
    }

    fn rule(name: &str, filters: Vec<FilterRule>) -> Destination {
        Destination::new(name, format!(r"D:\{name}"), filters, SyncStrategy::Move)
    }

    #[test]
    fn a_move_preview_gives_each_file_to_the_first_rule_that_wants_it() {
        let invoices = rule("invoices", vec![FilterRule::path("invoices")]);
        let pdfs = rule("pdfs", vec![FilterRule::extension("pdf")]);
        let task = task(SyncTaskKind::Move, vec![invoices.clone(), pdfs.clone()]);
        let files = file_system(&["invoices/march.pdf", "notes/read.pdf", "photo.jpg"]);

        let scan = scan(&files, &task, &task.destinations).expect("scan the source");

        assert_eq!(3, scan.file_count);
        assert_eq!(1, scan.per_destination[&invoices.id].count);
        assert_eq!(
            1, scan.per_destination[&pdfs.id].count,
            "march.pdf is taken"
        );
        assert_eq!(1, scan.unmatched.count);
        assert_eq!(vec!["photo.jpg".to_owned()], scan.unmatched.sample);
    }

    #[test]
    fn a_contested_file_is_reported_with_every_rule_that_wanted_it() {
        let task = task(
            SyncTaskKind::Move,
            vec![
                rule("invoices", vec![FilterRule::path("invoices")]),
                rule("pdfs", vec![FilterRule::extension("pdf")]),
            ],
        );
        let files = file_system(&["invoices/march.pdf"]);

        let scan = scan(&files, &task, &task.destinations).expect("scan the source");

        assert_eq!(
            vec![ContestedFile {
                relative_path: "invoices/march.pdf".into(),
                rules: vec![0, 1],
            }],
            scan.contested,
            "the order resolved it; seeing which rule won is the whole point"
        );
    }

    #[test]
    fn sync_destinations_are_independent_so_one_file_can_reach_several() {
        let jpgs = Destination::new(
            "jpgs",
            r"D:\a",
            vec![FilterRule::extension("jpg")],
            SyncStrategy::AddOnly,
        );
        let photos = Destination::new(
            "photos",
            r"D:\b",
            vec![FilterRule::path("photos")],
            SyncStrategy::AddOnly,
        );
        let task = task(SyncTaskKind::Sync, vec![jpgs.clone(), photos.clone()]);
        let files = file_system(&["photos/a.jpg", "notes.txt"]);

        let scan = scan(&files, &task, &task.destinations).expect("scan the source");

        assert_eq!(1, scan.per_destination[&jpgs.id].count);
        assert_eq!(1, scan.per_destination[&photos.id].count);
        assert_eq!(1, scan.unmatched.count, "notes.txt reaches neither");
        assert!(
            scan.contested.is_empty(),
            "nothing is contested when nothing competes"
        );
    }

    #[test]
    fn extension_chips_are_commonest_first_and_fold_case() {
        let task = task(SyncTaskKind::Sync, vec![]);
        let files = file_system(&["a.JPG", "b.jpg", "c.txt", "README", "d.tar.gz"]);

        let scan = scan(&files, &task, &task.destinations).expect("scan the source");
        let labels: Vec<_> = scan
            .extensions
            .iter()
            .map(|chip| chip.label().to_lowercase())
            .collect();

        assert_eq!(
            vec![
                "jpg (2)".to_owned(),
                "gz (1)".to_owned(),
                "txt (1)".to_owned()
            ],
            labels,
            "an extensionless file offers no rule to pick"
        );
    }

    #[test]
    fn a_source_that_cannot_be_read_is_an_error_rather_than_an_empty_preview() {
        // An empty preview would read as "no files match your rules", which is a different
        // thing entirely and would send the user off to rewrite rules that are fine.
        let task = task(SyncTaskKind::Move, vec![]);
        let memory: Arc<dyn FileSystem> = Arc::new(InMemoryFileSystem::new());

        assert!(scan(&memory, &task, &task.destinations).is_err());
    }
}
