//! Handing a file to whatever the user has chosen to open it with.
//!
//! `ShellExecuteW` with the default verb, which is the same thing double-clicking the file in
//! Explorer does — so "open the log" lands in whatever the user actually reads `.log` files in,
//! not in whatever this app guessed. Spawning `notepad.exe` would be guessing; going through
//! `cmd /c start` would work but flashes a console window on the way past.
//!
//! A file extension nobody has claimed makes Windows show its own "How do you want to open
//! this?" picker. That is the right answer to the question, so it is left to happen.

use std::path::Path;

/// Opens `path` with its default application. `false` when the shell refused.
pub fn open(path: &Path) -> bool {
    platform::open(path)
}

#[cfg(windows)]
mod platform {
    use std::os::windows::ffi::OsStrExt as _;
    use std::path::Path;

    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    /// Below this, `ShellExecuteW`'s return value is an error code rather than an instance
    /// handle. The documented sentinel, spelled out because it reads as a magic number.
    const SUCCESS_ABOVE: isize = 32;

    pub(super) fn open(path: &Path) -> bool {
        let file = wide(path.as_os_str());

        // SAFETY: both strings are NUL-terminated and outlive the call, and every other
        // argument is a documented "none": no owner window, default verb, no parameters, no
        // working directory.
        let result = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                std::ptr::null(),
                file.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        };

        result as isize > SUCCESS_ABOVE
    }

    fn wide(text: &std::ffi::OsStr) -> Vec<u16> {
        text.encode_wide().chain(std::iter::once(0)).collect()
    }
}

#[cfg(not(windows))]
mod platform {
    use std::path::Path;

    pub(super) fn open(_path: &Path) -> bool {
        false
    }
}
