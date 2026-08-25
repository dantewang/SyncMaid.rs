//! The activity and error log.
//!
//! One file beside the config, rolled at ~5 MB with a single previous copy kept. It is the run
//! history the UI does not show — the destination rows only display the latest result — so its
//! shape follows the C# build's, and a log holding lines from both builds reads as one file.
//!
//! One deliberate divergence: a run line names its task's id as well as its name. The names
//! alone do not identify a destination, and the window that reads these lines back has to be
//! able to. Lines without it still read, and are still found — see [`DestinationTag`].
//!
//! Logging never throws into the app. An I/O failure here is swallowed, which is the one
//! sanctioned exception to "never swallow an exception": there is nowhere better to report it,
//! and a full disk must not take the app down with it.
//!
//! The log is read back as well as written: a destination's status text opens a window onto its
//! own lines. [`DestinationTag`] is what makes that possible, and it lives here rather than
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
use uuid::Uuid;

/// Roll once the file passes this, keeping one previous copy.
const MAX_BYTES: u64 = 5 * 1024 * 1024;

/// How one destination's lines open — in both spellings the log has ever used.
///
/// `Sync 'Photos' (1a2b…) → 'NAS backup'` is what a run writes now. The task's id is in it
/// because the two names alone are not unique: a destination name is unique only inside its
/// task, and two tasks may share a name. The id is, so the pair always is.
///
/// The name-only spelling is still matched. It is every line written before the id was in them,
/// and every line the C# build writes, and dropping it would blank this window on upgrade day
/// for history that is still on disk and still the reason the row says "Failed". It carries the
/// ambiguity the id was added to fix — but only for lines already written, which is the half of
/// the problem that cannot be fixed anyway.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DestinationTag {
    current: String,
    legacy: String,
}

impl DestinationTag {
    pub fn new(task_id: Uuid, task: &str, destination: &str) -> Self {
        Self {
            current: format!("Sync '{task}' ({task_id}) → '{destination}'"),
            legacy: format!("Sync '{task}' → '{destination}'"),
        }
    }

    /// The opening of every line a run writes about this destination.
    pub fn line_prefix(&self) -> &str {
        &self.current
    }

    /// Which spelling this line uses, or `None` when it is not about this destination.
    ///
    /// The two cannot both match: the id sits between the task name and the arrow, so a line
    /// with one spelling contains neither the other's text nor a prefix of it.
    fn matched_in(&self, line: &str) -> Option<&str> {
        [&self.current, &self.legacy]
            .into_iter()
            .find(|tag| line.contains(tag.as_str()))
            .map(String::as_str)
    }
}

/// One log line, split into the parts the window shows in their own places.
///
/// The tag is taken off: the window names the task and the destination once in its header, so
/// repeating them on all twenty rows would leave no width for what the rows actually say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogLine {
    /// `2026-08-25 02:00:11.004`. Empty when the line does not begin with one.
    pub time: String,
    /// `INF`, `WRN`, `ERR` … Empty when the line has no level.
    pub level: String,
    /// What is left with the timestamp, the level, the target and the tag taken off.
    pub message: String,
}

/// The last `limit` lines about `tag`, oldest first. Empty when there is no log yet.
///
/// Reads whole files rather than seeking back from the end: matching lines are sparse and may be
/// old, so a fixed tail of *bytes* would silently drop the very line being looked up. The roll
/// caps each read at [`MAX_BYTES`], and this only runs on a click.
pub fn tail_matching(path: &Path, tag: &DestinationTag, limit: usize) -> Vec<LogLine> {
    // The previous file first, so what comes out is a true tail. The roll happens at a size
    // nobody chose, and the line explaining a failure is as likely to have just crossed it as not.
    let mut matched = matching_lines(&previous_path(path), tag);
    matched.extend(matching_lines(path, tag));

    let excess = matched.len().saturating_sub(limit);
    matched.drain(..excess);
    matched
}

/// One file's matching lines. A file that is not there has none, which is not an error.
fn matching_lines(path: &Path, tag: &DestinationTag) -> Vec<LogLine> {
    let Ok(bytes) = fs::read(path) else {
        return Vec::new();
    };
    // Lossy on purpose: a half-written line at the end of a file being appended to must not cost
    // the reader the nineteen good lines above it.
    String::from_utf8_lossy(&bytes)
        .lines()
        .filter_map(|line| Some(split(line, tag.matched_in(line)?)))
        .collect()
}

/// Takes a line apart, given the tag that matched it.
///
/// Anything that will not parse is shown whole rather than shown wrong: a log line is evidence,
/// and an untidy row costs less than one with the wrong half trimmed off it.
fn split(line: &str, tag: &str) -> LogLine {
    // `2026-08-25 02:00:11.004 [WRN] main_view: {tag}: Failed · access denied`
    let (time, level) = match line.split_once(" [") {
        Some((time, rest)) => (time, rest.split_once(']').map_or("", |(level, _)| level)),
        None => ("", ""),
    };
    let message = line
        .split_once(tag)
        .and_then(|(_, rest)| rest.strip_prefix(": "))
        .unwrap_or(line);

    LogLine {
        time: time.to_owned(),
        level: level.to_owned(),
        message: message.to_owned(),
    }
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

/// `2026-08-09 02:00:12.418 [INF] main_view: Sync 'Photos' (1a2b…) → 'NAS backup': Success · …`
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

    const PHOTOS: Uuid = Uuid::from_u128(0x1a2b);
    const DOCS: Uuid = Uuid::from_u128(0x3c4d);

    fn tag_for(task_id: Uuid, task: &str, destination: &str) -> DestinationTag {
        DestinationTag::new(task_id, task, destination)
    }

    fn messages(lines: &[LogLine]) -> Vec<&str> {
        lines.iter().map(|line| line.message.as_str()).collect()
    }

    /// The one line that has to keep matching what `log_destination` writes.
    #[test]
    fn the_tag_is_the_opening_of_a_real_log_line() {
        let tag = tag_for(PHOTOS, "Photos", "NAS backup");
        let line = format!(
            "2026-08-09 02:00:12.418 [WRN] main_view: {}: Failed · access denied",
            tag.line_prefix()
        );

        assert_eq!(Some(tag.line_prefix()), tag.matched_in(&line));
    }

    /// The whole reason the id is in the line: the two names alone do not identify a
    /// destination, because a destination name is unique only inside its task.
    #[test]
    fn two_tasks_with_one_name_no_longer_read_each_others_lines() {
        let mine = tag_for(PHOTOS, "Photos", "NAS backup");
        let theirs = tag_for(DOCS, "Photos", "NAS backup");
        let line = format!(
            "[INF] main_view: {}: Success · 1 copied",
            theirs.line_prefix()
        );

        assert!(mine.matched_in(&line).is_none(), "{line}");
        assert_eq!(Some(theirs.line_prefix()), theirs.matched_in(&line));
    }

    /// Lines written before the id existed — and every line the C# build writes — still read.
    /// Losing them would blank this window on upgrade day for history that still explains why
    /// the row says "Failed".
    #[test]
    fn lines_written_without_a_task_id_are_still_found() {
        let tag = tag_for(PHOTOS, "Photos", "NAS backup");
        let line = "2026-08-09 02:00:12.418 [WRN] main_view: \
                    Sync 'Photos' → 'NAS backup': Failed · access denied";

        assert_eq!(Some("Sync 'Photos' → 'NAS backup'"), tag.matched_in(line));
    }

    #[test]
    fn only_this_destinations_lines_come_back() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("syncmaid.log");
        let tag = tag_for(PHOTOS, "Photos", "NAS backup");
        fs::write(
            &path,
            format!(
                "{}: Success · 1 copied\n\
                 {}: Success · 2 copied\n\
                 {}: Success · 3 copied\n\
                 {}: Failed · access denied\n",
                tag.line_prefix(),
                tag_for(PHOTOS, "Photos", "USB stick").line_prefix(),
                tag_for(DOCS, "Docs", "NAS backup").line_prefix(),
                tag.line_prefix(),
            ),
        )
        .unwrap();

        let lines = tail_matching(&path, &tag, 20);

        assert_eq!(
            vec!["Success · 1 copied", "Failed · access denied"],
            messages(&lines),
            "oldest first, and only this pair"
        );
    }

    /// The header names the task and the destination once, so the rows must not repeat them.
    #[test]
    fn a_line_is_split_into_its_time_its_level_and_what_it_actually_says() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("syncmaid.log");
        let tag = tag_for(PHOTOS, "Photos", "NAS backup");
        fs::write(
            &path,
            format!(
                "2026-08-25 02:00:11.004 [WRN] main_view: {}: Failed · access denied\n",
                tag.line_prefix()
            ),
        )
        .unwrap();

        let lines = tail_matching(&path, &tag, 20);

        assert_eq!(1, lines.len());
        assert_eq!("2026-08-25 02:00:11.004", lines[0].time);
        assert_eq!("WRN", lines[0].level);
        assert_eq!("Failed · access denied", lines[0].message);
    }

    /// A line that will not parse is shown whole rather than shown wrong.
    #[test]
    fn a_line_that_does_not_parse_keeps_all_of_itself() {
        let tag = tag_for(PHOTOS, "Photos", "NAS backup");
        let line = format!("{} something else entirely", tag.line_prefix());

        let split = split(&line, tag.line_prefix());

        assert_eq!("", split.time);
        assert_eq!(line, split.message);
    }

    #[test]
    fn the_limit_keeps_the_newest_lines_not_the_oldest() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("syncmaid.log");
        let tag = tag_for(PHOTOS, "Photos", "NAS backup");
        let body: String = (0..30)
            .map(|run| format!("{}: Success · {run}\n", tag.line_prefix()))
            .collect();
        fs::write(&path, body).unwrap();

        let lines = tail_matching(&path, &tag, 20);

        assert_eq!(20, lines.len());
        assert_eq!("Success · 10", lines[0].message);
        assert_eq!("Success · 29", lines[19].message);
    }

    /// A line that has just rolled off is exactly the one a failure is being looked up for.
    #[test]
    fn the_tail_reaches_back_across_the_roll() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("syncmaid.log");
        let tag = tag_for(PHOTOS, "Photos", "NAS backup");
        fs::write(
            directory.path().join("syncmaid.log.1"),
            format!("{}: yesterday\n", tag.line_prefix()),
        )
        .unwrap();
        fs::write(&path, format!("{}: today\n", tag.line_prefix())).unwrap();

        let lines = tail_matching(&path, &tag, 20);

        assert_eq!(
            vec!["yesterday", "today"],
            messages(&lines),
            "the older file comes first"
        );
    }

    #[test]
    fn no_log_file_yet_is_no_lines_rather_than_an_error() {
        let directory = tempfile::tempdir().unwrap();
        let tag = tag_for(PHOTOS, "Photos", "NAS backup");

        assert!(tail_matching(&directory.path().join("nothing.log"), &tag, 20).is_empty());
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
