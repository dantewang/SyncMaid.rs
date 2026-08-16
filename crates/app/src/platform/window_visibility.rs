//! Hiding the main window without destroying it.
//!
//! This is the whole reason SyncMaid can live in the tray on gpui. gpui breaks its message
//! loop when the last window is *closed* (zed-industries/zed#22008), so `window.remove_window()`
//! would take the process with it and the triggers would stop. Hiding the native window instead
//! leaves it — and therefore the loop — alive, with nothing on screen and nothing in the
//! taskbar. Closing to tray is a hide; the tray's Exit is a real quit.

use gpui::Window;

/// Hides the window: gone from screen and taskbar, still alive.
pub fn hide(window: &Window) -> bool {
    set_visible(window, false)
}

/// Shows the window again and brings it to the foreground.
pub fn show(window: &Window) -> bool {
    if !set_visible(window, true) {
        return false;
    }
    window.activate_window();
    true
}

/// Whether the OS currently shows the window.
pub fn is_visible(window: &Window) -> bool {
    platform::is_visible(window)
}

fn set_visible(window: &Window, visible: bool) -> bool {
    platform::set_visible(window, visible)
}

#[cfg(windows)]
mod platform {
    use gpui::Window;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        IsWindowVisible, SetForegroundWindow, ShowWindow, SW_HIDE, SW_SHOW,
    };

    pub(super) fn set_visible(window: &Window, visible: bool) -> bool {
        let Some(hwnd) = hwnd_of(window) else {
            return false;
        };
        // SAFETY: `hwnd` came from the live window we were handed, so it is a valid handle for
        // the duration of this call. Both calls are infallible in the sense that matters here —
        // their return value reports the *previous* state, not an error.
        unsafe {
            ShowWindow(hwnd, if visible { SW_SHOW } else { SW_HIDE });
            if visible {
                SetForegroundWindow(hwnd);
            }
        }
        true
    }

    pub(super) fn is_visible(window: &Window) -> bool {
        let Some(hwnd) = hwnd_of(window) else {
            return false;
        };
        // SAFETY: as above.
        unsafe { IsWindowVisible(hwnd) != 0 }
    }

    fn hwnd_of(window: &Window) -> Option<HWND> {
        // Spelled out because gpui's `Window` also has an inherent `window_handle()` that
        // returns its own `AnyWindowHandle`, which would silently win here.
        match HasWindowHandle::window_handle(window).ok()?.as_raw() {
            RawWindowHandle::Win32(handle) => Some(handle.hwnd.get() as HWND),
            _ => None,
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use gpui::Window;

    pub(super) fn set_visible(_window: &Window, _visible: bool) -> bool {
        false
    }

    pub(super) fn is_visible(_window: &Window) -> bool {
        true
    }
}
