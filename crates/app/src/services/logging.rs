//! The activity and error log.
//!
//! One file beside the config, rolled at ~5 MB with a single previous copy kept. It is the run
//! history the UI does not show — the destination rows only display the latest result — so its
//! format matches the C# build's line for line, and a log from either build reads the same.
//!
//! Logging never throws into the app. An I/O failure here is swallowed, which is the one
//! sanctioned exception to "never swallow an exception": there is nowhere better to report it,
//! and a full disk must not take the app down with it.
//!
//! The log is read back as well as written: a destination's status text opens a window onto its
//! own lines. [`destination_tag`] is what makes that possible, and it lives here rather than
//! beside the reader because the two have to agree on one sentence forever — a wording changed
//! at one end and not the other quietly stops matching every line already on disk.

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tracing::{Event, Level, Subscriber};
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields};
use tracing_subscriber::registry::LookupSpan;

/// Roll once the file passes this, keeping one previous copy.
const MAX_BYTES: u64 = 5 * 1024 * 1024;

/// The needle that finds one destination's lines — and the opening of every line about it.
///
/// `Sync 'Photos' → 'NAS backup'`. Both names, because a destination name is only unique inside
/// its task: two tasks may each have a "NAS backup". Two tasks that share a name *and* have a
/// same-named destination still collide, which is the price of a log whose format is a sentence
/// rather than a record — and that format is what lets a file from either build read the same.
pub fn destination_tag(task: &str, destination: &str) -> String {
    format!("Sync '{task}' → '{destination}'")
}

/// The last `limit` lines mentioning `tag`, oldest first. Empty when there is no log yet.
///
/// Reads whole files rather than seeking back from the end: matching lines are sparse and may be
/// old, so a fixed tail of *bytes* would silently drop the very line being looked up. The roll
/// caps each read at [`MAX_BYTES`], and this only runs on a click.
pub fn tail_matching(path: &Path, tag: &str, limit: usize) -> Vec<String> {
    // The previous file first, so what comes out is a true tail. The roll happens at a size
    // nobody chose, and the line explaining a failure is as likely to have just crossed it as not.
    let mut matched = matching_lines(&previous_path(path), tag);
    matched.extend(matching_lines(path, tag));

    let excess = matched.len().saturating_sub(limit);
    matched.drain(..excess);
    matched
}

/// One file's matching lines. A file that is not there has none, which is not an error.
fn matching_lines(path: &Path, tag: &str) -> Vec<String> {
    let Ok(bytes) = fs::read(path) else {
        return Vec::new();
    };
    // Lossy on purpose: a half-written line at the end of a file being appended to must not cost
    // the reader the nineteen good lines above it.
    String::from_utf8_lossy(&bytes)
        .lines()
        .filter(|line| line.contains(tag))
        .map(str::to_owned)
        .collect()
}

/// Where the roll puts the previous copy. Shared, so the reader looks where the writer wrote.
fn previous_path(path: &Path) -> PathBuf {
    path.with_extension("log.1")
}

/// Installs the file log. Returns quietly if it cannot be opened — the app runs regardless.
pub fn install(log_path: &Path) {
    let Some(writer) = RollingFile::open(log_path) else {
        return;
    };

    let subscriber = tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .event_format(SyncMaidFormat)
        .with_writer(writer)
        .finish();

    // A second install would mean two subscribers fighting over the same file; the first wins.
    let _ = tracing::subscriber::set_global_default(subscriber);
}

/// `2026-08-09 02:00:12.418 [INF] TaskNode: Sync 'Photos' → 'NAS backup': Success · 128 copied`
struct SyncMaidFormat;

impl<S, N> FormatEvent<S, N> for SyncMaidFormat
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        context: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let metadata = event.metadata();
        write!(
            writer,
            "{} [{}] {}: ",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"),
            abbreviate(*metadata.level()),
            // The last segment only, the way the C# logger shortens its category.
            metadata
                .target()
                .rsplit("::")
                .next()
                .unwrap_or(metadata.target()),
        )?;
        context
            .field_format()
            .format_fields(writer.by_ref(), event)?;
        writeln!(writer)
    }
}

fn abbreviate(level: Level) -> &'static str {
    match level {
        Level::TRACE => "TRC",
        Level::DEBUG => "DBG",
        Level::INFO => "INF",
        Level::WARN => "WRN",
        Level::ERROR => "ERR",
    }
}

/// Appends to one file, rolling it aside once it grows past [`MAX_BYTES`].
#[derive(Clone)]
struct RollingFile {
    state: Arc<Mutex<RollingState>>,
}

struct RollingState {
    path: PathBuf,
    file: Option<File>,
    written: u64,
}

impl RollingFile {
    fn open(path: &Path) -> Option<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).ok()?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .ok()?;
        let written = file.metadata().map(|metadata| metadata.len()).unwrap_or(0);

        Some(Self {
            state: Arc::new(Mutex::new(RollingState {
                path: path.to_path_buf(),
                file: Some(file),
                written,
            })),
        })
    }
}

impl RollingState {
    fn roll_if_needed(&mut self) {
        if self.written < MAX_BYTES {
            return;
        }

        let previous = previous_path(&self.path);
        self.file = None;
        let _ = fs::remove_file(&previous);
        if fs::rename(&self.path, &previous).is_err() {
            // Something holds the file; keep appending rather than losing the line.
            self.file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)
                .ok();
            return;
        }

        self.file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .ok();
        self.written = 0;
    }
}

impl Write for RollingFile {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let Ok(mut state) = self.state.lock() else {
            return Ok(buffer.len());
        };

        state.roll_if_needed();
        if let Some(file) = state.file.as_mut() {
            // Swallowed on purpose: logging must never throw into the app.
            if file.write_all(buffer).is_ok() {
                state.written += buffer.len() as u64;
            }
        }
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if let Ok(mut state) = self.state.lock() {
            if let Some(file) = state.file.as_mut() {
                let _ = file.flush();
            }
        }
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for RollingFile {
    type Writer = Self;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_is_appended_and_the_file_grows() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("logs").join("syncmaid.log");
        let mut writer = RollingFile::open(&path).unwrap();

        writer.write_all(b"first\n").unwrap();
        writer.write_all(b"second\n").unwrap();
        writer.flush().unwrap();

        assert_eq!("first\nsecond\n", fs::read_to_string(&path).unwrap());
    }

    #[test]
    fn the_file_rolls_aside_once_and_keeps_exactly_one_previous_copy() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("syncmaid.log");
        fs::write(&path, vec![b'x'; MAX_BYTES as usize + 1]).unwrap();
        let mut writer = RollingFile::open(&path).unwrap();

        writer.write_all(b"after the roll\n").unwrap();
        writer.flush().unwrap();

        assert_eq!("after the roll\n", fs::read_to_string(&path).unwrap());
        assert_eq!(
            MAX_BYTES as usize + 1,
            fs::read(directory.path().join("syncmaid.log.1"))
                .unwrap()
                .len()
        );
    }

    /// The one line that has to keep matching what `log_destination` writes.
    #[test]
    fn the_tag_is_the_opening_of_a_real_log_line() {
        let line = "2026-08-09 02:00:12.418 [WRN] main_view:                     Sync 'Photos' → 'NAS backup': Failed · access denied";

        assert!(line.contains(&destination_tag("Photos", "NAS backup")));
    }

    #[test]
    fn only_this_destinations_lines_come_back_and_only_the_last_few() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("syncmaid.log");
        fs::write(
            &path,
            "Sync 'Photos' → 'NAS backup': Success · 1
             Sync 'Photos' → 'USB stick': Success · 2
             Sync 'Docs' → 'NAS backup': Success · 3
             Sync 'Photos' → 'NAS backup': Failed · access denied
",
        )
        .unwrap();

        let lines = tail_matching(&path, &destination_tag("Photos", "NAS backup"), 20);

        assert_eq!(2, lines.len(), "{lines:?}");
        assert!(lines[0].ends_with("Success · 1"), "{lines:?}");
        assert!(
            lines[1].ends_with("access denied"),
            "oldest first: {lines:?}"
        );
    }

    #[test]
    fn the_limit_keeps_the_newest_lines_not_the_oldest() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("syncmaid.log");
        let tag = destination_tag("Photos", "NAS backup");
        let body: String = (0..30)
            .map(|run| {
                format!(
                    "{tag}: Success · {run}
"
                )
            })
            .collect();
        fs::write(&path, body).unwrap();

        let lines = tail_matching(&path, &tag, 20);

        assert_eq!(20, lines.len());
        assert!(lines[0].ends_with("· 10"), "{}", lines[0]);
        assert!(lines[19].ends_with("· 29"), "{}", lines[19]);
    }

    /// A line that has just rolled off is exactly the one a failure is being looked up for.
    #[test]
    fn the_tail_reaches_back_across_the_roll() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("syncmaid.log");
        let tag = destination_tag("Photos", "NAS backup");
        fs::write(
            directory.path().join("syncmaid.log.1"),
            format!(
                "{tag}: yesterday
"
            ),
        )
        .unwrap();
        fs::write(
            &path,
            format!(
                "{tag}: today
"
            ),
        )
        .unwrap();

        let lines = tail_matching(&path, &tag, 20);

        assert_eq!(2, lines.len(), "{lines:?}");
        assert!(
            lines[0].ends_with("yesterday"),
            "the older file comes first: {lines:?}"
        );
    }

    #[test]
    fn no_log_file_yet_is_no_lines_rather_than_an_error() {
        let directory = tempfile::tempdir().unwrap();

        assert!(tail_matching(&directory.path().join("nothing.log"), "Sync", 20).is_empty());
    }

    #[test]
    fn levels_abbreviate_the_way_the_csharp_logger_does() {
        assert_eq!("INF", abbreviate(Level::INFO));
        assert_eq!("WRN", abbreviate(Level::WARN));
        assert_eq!("ERR", abbreviate(Level::ERROR));
        assert_eq!("DBG", abbreviate(Level::DEBUG));
        assert_eq!("TRC", abbreviate(Level::TRACE));
    }
}
