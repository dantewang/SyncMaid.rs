//! A bounded retry for the failures that are worth retrying.
//!
//! Deliberately narrow. A missing file will still be missing next time, a verification failure
//! means the bytes were wrong, and a busy source needs the *next run*, not another attempt a
//! fraction of a second later — retrying that would only race the writer again.

use std::time::Duration;

use crate::sync::OperationError;

/// How hard to try.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryOptions {
    pub max_attempts: u32,
    /// Multiplied by the attempt number, so waits grow linearly.
    pub base_delay: Duration,
}

impl RetryOptions {
    /// Three attempts, 200 ms apart and growing — enough to ride out a virus scanner or a
    /// momentary share hiccup without making a real failure take visibly longer.
    pub const DEFAULT: Self = Self {
        max_attempts: 3,
        base_delay: Duration::from_millis(200),
    };
    /// One attempt, no waiting. For tests that assert on the first failure.
    pub const NONE: Self = Self {
        max_attempts: 1,
        base_delay: Duration::ZERO,
    };
}

impl Default for RetryOptions {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Runs `action`, retrying transient failures.
pub fn execute<T>(
    options: RetryOptions,
    action: impl FnMut() -> Result<T, OperationError>,
) -> Result<T, OperationError> {
    execute_with(options, std::thread::sleep, action)
}

/// [`execute`] with the wait injected, so tests do not have to spend the time.
pub fn execute_with<T>(
    options: RetryOptions,
    mut sleep: impl FnMut(Duration),
    mut action: impl FnMut() -> Result<T, OperationError>,
) -> Result<T, OperationError> {
    let mut attempt = 1;
    loop {
        match action() {
            Ok(value) => return Ok(value),
            Err(error) if attempt >= options.max_attempts || !error.is_transient() => {
                return Err(error)
            }
            Err(_) => {
                sleep(options.base_delay * attempt);
                attempt += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::*;

    #[test]
    fn a_transient_failure_that_clears_itself_succeeds() {
        let mut remaining = 2;
        let result = execute_with(
            RetryOptions::DEFAULT,
            |_| {},
            || {
                if remaining > 0 {
                    remaining -= 1;
                    Err(OperationError::Io(io::Error::from(
                        io::ErrorKind::PermissionDenied,
                    )))
                } else {
                    Ok(42)
                }
            },
        );

        assert_eq!(42, result.unwrap());
    }

    #[test]
    fn attempts_are_bounded() {
        let mut attempts = 0;
        let result: Result<(), _> = execute_with(
            RetryOptions::DEFAULT,
            |_| {},
            || {
                attempts += 1;
                Err(OperationError::Io(io::Error::other("still broken")))
            },
        );

        assert!(result.is_err());
        assert_eq!(3, attempts);
    }

    #[test]
    fn waits_grow_with_the_attempt() {
        let mut waits = Vec::new();
        let _: Result<(), _> = execute_with(
            RetryOptions::DEFAULT,
            |wait| waits.push(wait),
            || Err(OperationError::Io(io::Error::other("still broken"))),
        );

        assert_eq!(
            vec![Duration::from_millis(200), Duration::from_millis(400)],
            waits
        );
    }

    #[test]
    fn a_missing_file_is_not_retried() {
        let mut attempts = 0;
        let _: Result<(), _> = execute_with(
            RetryOptions::DEFAULT,
            |_| {},
            || {
                attempts += 1;
                Err(OperationError::Io(io::Error::from(io::ErrorKind::NotFound)))
            },
        );

        assert_eq!(
            1, attempts,
            "a source deleted between plan and apply will not come back"
        );
    }

    #[test]
    fn a_busy_source_and_a_verification_failure_are_not_retried() {
        for error in [
            OperationError::SourceBusy {
                path: r"C:\src\a.txt".into(),
            },
            OperationError::verification("xxHash mismatch"),
        ] {
            let mut attempts = 0;
            let _: Result<(), _> = execute_with(
                RetryOptions::DEFAULT,
                |_| {},
                || {
                    attempts += 1;
                    Err(match &error {
                        OperationError::SourceBusy { path } => {
                            OperationError::SourceBusy { path: path.clone() }
                        }
                        other => OperationError::verification(other.to_string()),
                    })
                },
            );
            assert_eq!(
                1, attempts,
                "{error:?} must reach the caller on the first attempt"
            );
        }
    }

    #[test]
    fn a_sharing_violation_is_retried_before_it_is_deferred() {
        // It is an I/O failure, so the retry gets its chance; the engine is what turns a
        // still-locked file into a deferral rather than a failure.
        let mut attempts = 0;
        let _: Result<(), _> = execute_with(
            RetryOptions::DEFAULT,
            |_| {},
            || {
                attempts += 1;
                Err(OperationError::Io(io::Error::from_raw_os_error(32)))
            },
        );

        assert_eq!(3, attempts);
    }
}
