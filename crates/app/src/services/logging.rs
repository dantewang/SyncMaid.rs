//! The activity and error log.
//!
//! One file beside the config, rolled at ~5 MB with a single previous copy kept. It is the run
//! history the UI does not show — the destination rows only display the latest result — so its
//! format matches the C# build's line for line, and a log from either build reads the same.
//!
//! Logging never throws into the app. An I/O failure here is swallowed, which is the one
//! sanctioned exception to "never swallow an exception": there is nowhere better to report it,
//! and a full disk must not take the app down with it.

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

        let previous = self.path.with_extension("log.1");
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

    #[test]
    fn levels_abbreviate_the_way_the_csharp_logger_does() {
        assert_eq!("INF", abbreviate(Level::INFO));
        assert_eq!("WRN", abbreviate(Level::WARN));
        assert_eq!("ERR", abbreviate(Level::ERROR));
        assert_eq!("DBG", abbreviate(Level::DEBUG));
        assert_eq!("TRC", abbreviate(Level::TRACE));
    }
}
