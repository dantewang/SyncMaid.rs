//! One running copy per installed copy.
//!
//! Two SyncMaids over the same `Data` folder would fight: both write `tasks.json`, both start
//! the same watchers, and both plan the same run. The original had no guard at all — this is a
//! known gap being closed rather than a behaviour being ported.
//!
//! **The identity is the folder the executable lives in, not the user.** SyncMaid is portable,
//! so a copy on a USB stick and a copy on `C:` are two different installs with two different
//! `Data` folders, and refusing to run the second would be wrong. Two launches of the *same*
//! folder are the case worth stopping.
//!
//! A second launch is a request, not an error: the copy already running is told to show its
//! window, which is what the user almost always meant.

use std::path::Path;

/// What a launch found.
pub enum Launch {
    /// This process is the one. Holds the claim for as long as it lives.
    First(SingleInstance),
    /// A copy of this install is already running, and has been asked to come forward.
    Another,
}

/// The claim on this install, plus the channel a later launch knocks on.
pub struct SingleInstance {
    /// Fires when another launch of this same install asked us to show ourselves.
    pub knock: flume::Receiver<()>,
    /// The named mutex that marks this install as taken; releasing it is this field dropping.
    /// `None` when the claim could not be made and we chose to start unguarded.
    #[cfg(windows)]
    _claim: Option<std::os::windows::io::OwnedHandle>,
}

/// Claims this install, or hands off to the copy that already has it.
pub fn acquire(executable_directory: &Path) -> Launch {
    #[cfg(windows)]
    {
        windows_impl::acquire(executable_directory)
    }
    #[cfg(not(windows))]
    {
        // Nothing to coordinate through here yet. Being the only copy is the safe assumption:
        // refusing to start would be worse than the double-run this cannot detect.
        let _ = executable_directory;
        let (_sender, knock) = flume::unbounded();
        std::mem::forget(_sender);
        Launch::First(SingleInstance { knock })
    }
}

/// A stable, case-insensitive fingerprint of the install folder.
///
/// FNV-1a rather than the standard hasher, whose output is explicitly not promised to be stable
/// — and this name has to mean the same thing to a build from six months ago.
fn fingerprint(directory: &Path) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x1000_0000_01b3;

    let mut hash = OFFSET;
    // Windows paths are case-insensitive, so `C:\App` and `c:\app` are one install.
    for byte in directory.to_string_lossy().to_uppercase().bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

#[cfg(windows)]
mod windows_impl {
    use std::os::windows::io::{AsRawHandle, HandleOrNull, OwnedHandle};
    use std::path::Path;

    use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
    use windows_sys::Win32::System::Threading::{
        CreateEventW, CreateMutexW, SetEvent, WaitForSingleObject, INFINITE,
    };

    use super::{fingerprint, Launch, SingleInstance};

    pub fn acquire(executable_directory: &Path) -> Launch {
        let id = fingerprint(executable_directory);
        // `Local\` rather than `Global\`: the claim belongs to this logon session, and naming it
        // globally would need privileges an ordinary app has no business asking for.
        let mutex_name = wide(&format!("Local\\SyncMaid-{id:016x}"));
        let event_name = wide(&format!("Local\\SyncMaid-{id:016x}-show"));

        let Some((mutex, already_claimed)) = claim(&mutex_name) else {
            // Without the mutex there is nothing to coordinate through. Starting anyway is the
            // lesser failure: at worst the old behaviour, where two copies could run.
            tracing::warn!("could not claim single-instance; starting anyway");
            return Launch::First(SingleInstance {
                knock: flume::unbounded().1,
                _claim: None,
            });
        };

        if already_claimed {
            // Let go before knocking: the claim belongs to the copy that got there first.
            drop(mutex);
            knock(&event_name);
            return Launch::Another;
        }

        let (sender, knock) = flume::unbounded();
        if let Some(event) = show_event(&event_name) {
            std::thread::Builder::new()
                .name("syncmaid-instance".into())
                .spawn(move || {
                    // Ends with the process. A closed channel means the window is gone, which
                    // is the same moment the process is on its way out.
                    //
                    // SAFETY: this thread owns `event` for the whole loop, so the handle is
                    // live for every call, and waiting only reads it.
                    while unsafe { WaitForSingleObject(event.as_raw_handle(), INFINITE) } == 0 {
                        if sender.send(()).is_err() {
                            return;
                        }
                    }
                })
                .ok();
        }

        Launch::First(SingleInstance {
            knock,
            _claim: Some(mutex),
        })
    }

    /// Takes this install's mutex, and says whether someone was already holding it.
    ///
    /// `None` when Windows would not hand one out at all.
    fn claim(mutex_name: &[u16]) -> Option<(OwnedHandle, bool)> {
        // SAFETY: `mutex_name` is a NUL-terminated UTF-16 buffer that outlives the call, and the
        // handle comes back ours alone, wanting nothing but the `CloseHandle` that `OwnedHandle`
        // does on drop. `GetLastError` is read inside the same block, so nothing can run in
        // between and overwrite it.
        let (handle, error) = unsafe {
            let handle = CreateMutexW(std::ptr::null(), 0, mutex_name.as_ptr());
            (HandleOrNull::from_raw_handle(handle), GetLastError())
        };
        // Null is Windows refusing, and an `OwnedHandle` cannot hold one — so the conversion is
        // the failure check.
        let mutex = OwnedHandle::try_from(handle).ok()?;
        Some((mutex, error == ERROR_ALREADY_EXISTS))
    }

    /// Tells the copy already running to show itself. Best effort: if the event is not there,
    /// the other copy is mid-exit and this launch has simply lost a race.
    fn knock(event_name: &[u16]) {
        let Some(event) = show_event(event_name) else {
            return;
        };
        // SAFETY: `event` stays live until it drops at the end of this function, and signalling
        // is all this does to it.
        unsafe { SetEvent(event.as_raw_handle()) };
    }

    /// Opens this install's "show yourself" event, creating it if nobody has yet.
    ///
    /// Auto-reset and initially unset: each knock wakes the wait exactly once.
    fn show_event(event_name: &[u16]) -> Option<OwnedHandle> {
        // SAFETY: `event_name` is a NUL-terminated UTF-16 buffer that outlives the call, and the
        // handle it returns is ours to close — which is the whole of `OwnedHandle`'s job.
        let handle = unsafe {
            HandleOrNull::from_raw_handle(CreateEventW(std::ptr::null(), 0, 0, event_name.as_ptr()))
        };
        // As in `claim`: null means refused, and the conversion is the check.
        OwnedHandle::try_from(handle).ok()
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fingerprint_ignores_case_but_not_the_folder() {
        // Two portable copies in different folders are two different apps, and both may run.
        assert_eq!(
            fingerprint(Path::new(r"C:\Apps\SyncMaid")),
            fingerprint(Path::new(r"c:\apps\syncmaid"))
        );
        assert_ne!(
            fingerprint(Path::new(r"C:\Apps\SyncMaid")),
            fingerprint(Path::new(r"E:\Portable\SyncMaid"))
        );
    }

    #[test]
    fn the_fingerprint_is_the_same_every_time() {
        // It names a kernel object that a build from six months ago also has to find.
        assert_eq!(
            fingerprint(Path::new(r"C:\Apps\SyncMaid")),
            fingerprint(Path::new(r"C:\Apps\SyncMaid"))
        );
    }

    #[test]
    #[cfg(windows)]
    fn a_second_launch_of_the_same_folder_stands_down() {
        let folder = std::env::temp_dir().join("syncmaid-single-instance-test");

        let first = acquire(&folder);
        assert!(matches!(first, Launch::First(_)));

        let second = acquire(&folder);
        assert!(matches!(second, Launch::Another));

        // The knock reaches the copy that is running.
        let Launch::First(instance) = first else {
            unreachable!()
        };
        assert_eq!(
            Ok(()),
            instance
                .knock
                .recv_timeout(std::time::Duration::from_secs(5))
        );
    }

    #[test]
    #[cfg(windows)]
    fn a_different_folder_is_a_different_app() {
        let one = acquire(&std::env::temp_dir().join("syncmaid-instance-a"));
        let two = acquire(&std::env::temp_dir().join("syncmaid-instance-b"));

        assert!(matches!(one, Launch::First(_)));
        assert!(matches!(two, Launch::First(_)));
    }
}
