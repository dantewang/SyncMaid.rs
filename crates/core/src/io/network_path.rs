//! Detects whether a path lives on a mounted network location.
//!
//! Network-ness is a **runtime capability, not a type**: the same `LocalDestination` is a
//! local disk on one machine and a share on another. Three behaviours turn on it — network
//! shares have no Recycle Bin (removals there are permanent), Windows change notifications are
//! unreliable on them (so the watcher polls instead), and content verification re-reads every
//! copied file over the wire (so the editor warns before enabling it).

use std::path::{Component, Path};

/// True when `path` is a UNC path or resolves to a mapped network drive.
pub fn is_network(path: &Path) -> bool {
    let text = path.to_string_lossy();
    if text.trim().is_empty() {
        return false;
    }

    if text.starts_with(r"\\") || text.starts_with("//") {
        return true;
    }

    drive_root(path).is_some_and(|root| is_network_drive(&root))
}

/// `C:\foo\bar` → `C:\`. `None` when the path has no drive prefix to ask about.
fn drive_root(path: &Path) -> Option<String> {
    let absolute = std::path::absolute(path).ok()?;
    match absolute.components().next()? {
        Component::Prefix(prefix) => Some(format!("{}\\", prefix.as_os_str().to_string_lossy())),
        _ => None,
    }
}

#[cfg(windows)]
fn is_network_drive(root: &str) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDriveTypeW;

    /// `DRIVE_REMOTE` from winbase.h — windows-sys does not re-export the drive-type constants.
    const DRIVE_REMOTE: u32 = 4;

    let wide: Vec<u16> = std::ffi::OsStr::new(root)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    // SAFETY: `wide` is a NUL-terminated UTF-16 buffer that outlives the call, which is the
    // whole of GetDriveTypeW's contract. It cannot fail — an unknown root reports DRIVE_UNKNOWN.
    unsafe { GetDriveTypeW(wide.as_ptr()) == DRIVE_REMOTE }
}

#[cfg(not(windows))]
fn is_network_drive(_root: &str) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unc_paths_are_network() {
        assert!(is_network(Path::new(r"\\server\share")));
        assert!(is_network(Path::new(r"\\server\share\sub\file.txt")));
    }

    #[test]
    fn blank_input_is_not_network() {
        assert!(!is_network(Path::new("")));
        assert!(!is_network(Path::new("   ")));
    }

    #[test]
    #[cfg(windows)]
    fn a_local_drive_is_not_network() {
        // The system drive is by definition local on any machine that can run these tests.
        let system_drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
        assert!(!is_network(Path::new(&format!(r"{system_drive}\Windows"))));
    }
}
