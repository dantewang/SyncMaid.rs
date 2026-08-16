//! Recognizes "another process is holding this file right now", as opposed to something being
//! wrong with it.
//!
//! A busy file is not an error: the engine defers it and the next run picks it up once the
//! writer is done. That distinction is why a locked file costs one deferral rather than
//! failing the whole destination.

use std::error::Error;
use std::io;

/// `ERROR_SHARING_VIOLATION` — the file is open in another process with incompatible sharing.
const ERROR_SHARING_VIOLATION: i32 = 32;
/// `ERROR_LOCK_VIOLATION` — a byte range of the file is locked.
const ERROR_LOCK_VIOLATION: i32 = 33;

/// True when `error` reports a file held open by another process, directly or as a source.
pub fn is_busy(error: &io::Error) -> bool {
    if is_sharing_or_lock_violation(error) {
        return true;
    }

    // An io::Error built by wrapping another one keeps the payload behind `get_ref`, not in
    // the `source` chain, so both routes have to be walked.
    if let Some(wrapped) = error
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<io::Error>())
    {
        if is_busy(wrapped) {
            return true;
        }
    }

    let mut source: Option<&(dyn Error + 'static)> = error.source();
    while let Some(current) = source {
        if let Some(inner) = current.downcast_ref::<io::Error>() {
            if is_sharing_or_lock_violation(inner) {
                return true;
            }
        }
        source = current.source();
    }

    false
}

fn is_sharing_or_lock_violation(error: &io::Error) -> bool {
    matches!(
        error.raw_os_error(),
        Some(ERROR_SHARING_VIOLATION) | Some(ERROR_LOCK_VIOLATION)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sharing_and_lock_violations_are_busy() {
        assert!(is_busy(&io::Error::from_raw_os_error(
            ERROR_SHARING_VIOLATION
        )));
        assert!(is_busy(&io::Error::from_raw_os_error(ERROR_LOCK_VIOLATION)));
    }

    #[test]
    fn other_failures_are_not_busy() {
        assert!(!is_busy(&io::Error::from(io::ErrorKind::NotFound)));
        assert!(!is_busy(&io::Error::from(io::ErrorKind::PermissionDenied)));
        assert!(!is_busy(&io::Error::from_raw_os_error(5)));
    }

    #[test]
    fn a_wrapped_violation_is_still_busy() {
        let wrapped = io::Error::other(io::Error::from_raw_os_error(ERROR_SHARING_VIOLATION));
        assert!(is_busy(&wrapped));
    }
}
