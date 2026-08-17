//! Starting with Windows.
//!
//! A per-user `Run` value, and nothing else. The alternatives were weighed and declined:
//! `HKLM` needs elevation and affects every user; a Startup-folder shortcut needs COM to
//! create and buys nothing; a scheduled task is both heavier and *more* likely to be flagged.
//!
//! The Run key's virtue is that it is completely visible — Task Manager lists it under Startup
//! apps, where the user can turn it off. Antivirus heuristics target *covert* persistence, so
//! the obvious mechanism is the safe one.
//!
//! **The registry value is the single source of truth.** Nothing about autostart is mirrored
//! into `settings.json`, so the two can never disagree.

/// Whether SyncMaid starts with Windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoStartState {
    Disabled,
    Enabled,
    /// The Run value is there, but Windows has it switched off.
    ///
    /// Task Manager's "disable" does not delete the value — it writes a status blob elsewhere.
    /// Windows' switch wins, and the settings page says so rather than fighting it.
    DisabledByWindows,
}

/// Reads and writes the autostart registration.
pub trait AutoStart: Send + Sync {
    fn state(&self) -> AutoStartState;
    fn enable(&self) -> anyhow::Result<()>;
    fn disable(&self) -> anyhow::Result<()>;
}

/// The shipping implementation for this platform.
pub fn auto_start() -> Box<dyn AutoStart> {
    #[cfg(windows)]
    {
        Box::new(windows_impl::WindowsAutoStart)
    }
    #[cfg(not(windows))]
    {
        Box::new(Unsupported)
    }
}

/// Somewhere without a Run key. Reports disabled and ignores the rest, so callers never branch.
#[cfg_attr(windows, allow(dead_code))]
pub struct Unsupported;

impl AutoStart for Unsupported {
    fn state(&self) -> AutoStartState {
        AutoStartState::Disabled
    }

    fn enable(&self) -> anyhow::Result<()> {
        Ok(())
    }

    fn disable(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

#[cfg(windows)]
mod windows_impl {
    use anyhow::Context as _;
    use windows_registry::CURRENT_USER;

    use super::{AutoStart, AutoStartState};

    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const APPROVED_KEY: &str =
        r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
    const VALUE_NAME: &str = "SyncMaid";

    pub struct WindowsAutoStart;

    impl AutoStart for WindowsAutoStart {
        fn state(&self) -> AutoStartState {
            let registered = CURRENT_USER
                .open(RUN_KEY)
                .ok()
                .and_then(|key| key.get_string(VALUE_NAME).ok())
                .is_some_and(|value| !value.is_empty());
            if !registered {
                return AutoStartState::Disabled;
            }

            // Read only, never write. Overriding Windows' own switch is exactly the kind of
            // behaviour antivirus heuristics look for — and the user meant it when they used it.
            let disabled_by_windows = CURRENT_USER
                .open(APPROVED_KEY)
                .ok()
                .and_then(|key| key.get_value(VALUE_NAME).ok())
                // A status blob, not a string: the first byte is odd when the entry is off.
                .is_some_and(|flag| flag.as_ref().first().is_some_and(|first| first & 1 == 1));

            if disabled_by_windows {
                AutoStartState::DisabledByWindows
            } else {
                AutoStartState::Enabled
            }
        }

        fn enable(&self) -> anyhow::Result<()> {
            let executable = std::env::current_exe().context("find this executable")?;
            let key = CURRENT_USER.create(RUN_KEY).context("open the Run key")?;
            // Quoted, so a space in the install location does not split the command.
            key.set_string(VALUE_NAME, format!("\"{}\"", executable.display()))
                .context("write the Run value")?;
            Ok(())
        }

        fn disable(&self) -> anyhow::Result<()> {
            if let Ok(key) = CURRENT_USER.open(RUN_KEY) {
                // Absent is the goal, so "it was not there" is success.
                let _ = key.remove_value(VALUE_NAME);
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unsupported_platform_reports_disabled_and_does_nothing() {
        let auto_start = Unsupported;

        assert_eq!(AutoStartState::Disabled, auto_start.state());
        assert!(auto_start.enable().is_ok());
        assert!(auto_start.disable().is_ok());
        assert_eq!(AutoStartState::Disabled, auto_start.state());
    }

    #[test]
    #[cfg(windows)]
    fn reading_the_current_state_never_writes_anything() {
        // Deliberately read-only: the settings page asks on every open, and asking must not
        // register the app. Registry writes in CI are hostile, so the write paths stay
        // manual-verification only — this pins the one that has to be safe to call.
        let before = auto_start().state();
        let again = auto_start().state();
        assert_eq!(before, again);
    }
}
