use std::io;

use crate::sync::SyncOperation;

/// Why a single operation failed.
///
/// The three cases are kept apart because the engine treats them completely differently:
/// a busy source is deferred and is **not** a failure, a verification failure is never worth
/// retrying, and only an I/O failure can be transient.
#[derive(Debug, thiserror::Error)]
pub enum OperationError {
    /// The source changed under us between planning and applying — something is still
    /// writing it. Deliberately not an I/O error, so the retry never touches it: retrying
    /// would just race the writer again. The next run picks the file up.
    #[error("Source '{path}' is still being written; deferred to the next run.")]
    SourceBusy { path: String },

    /// A copy did not survive the trip: wrong length, or an xxHash mismatch on read-back.
    /// Never retried — the destination is untouched and the run should say so.
    #[error("{0}")]
    Verification(String),

    /// Anything the filesystem reported.
    #[error("{0}")]
    Io(#[from] io::Error),
}

impl OperationError {
    /// A verification failure with the given message.
    pub fn verification(message: impl Into<String>) -> Self {
        Self::Verification(message.into())
    }

    /// True when this means "another process is holding the file right now", either because
    /// the source moved under us or because the filesystem reported a sharing violation.
    ///
    /// A busy file is not an error: the run defers it, reports `Incomplete`, and the next run
    /// picks it up once the writer is done.
    pub fn is_busy(&self) -> bool {
        match self {
            Self::SourceBusy { .. } => true,
            Self::Io(error) => crate::io::is_busy(error),
            Self::Verification(_) => false,
        }
    }

    /// True when retrying might work: an I/O or permission failure that is not "it isn't
    /// there". A missing file will still be missing on the next attempt, and a verification
    /// failure or a busy source are not the retry's business.
    pub fn is_transient(&self) -> bool {
        match self {
            Self::Io(error) => error.kind() != io::ErrorKind::NotFound,
            Self::SourceBusy { .. } | Self::Verification(_) => false,
        }
    }
}

/// An operation failure, prefixed with what was being done to which file.
///
/// This string is what the destination row shows and what the log records, so it names both
/// the path and the verb: `Failed to copy 'photos/img.jpg': access denied.`
#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct SyncOperationError {
    message: String,
    /// The operation that failed, for callers that want more than the sentence.
    pub operation: SyncOperation,
    #[source]
    pub cause: OperationError,
}

impl SyncOperationError {
    pub fn new(operation: SyncOperation, cause: OperationError) -> Self {
        Self {
            message: format!("{}: {cause}", operation.describe_failure()),
            operation,
            cause,
        }
    }

    /// The user-facing sentence.
    pub fn message(&self) -> &str {
        &self.message
    }
}
