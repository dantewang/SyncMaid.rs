//! Hiding the main window without destroying it.
//!
//! This is the whole reason SyncMaid can live in the tray on gpui. gpui breaks its message
//! loop when the last window is *closed* (zed-industries/zed#22008), so `window.remove_window()`
//! would take the process with it and the triggers would stop. Hiding the native window instead
//! leaves it — and therefore the loop — alive, with nothing on screen and nothing in the
//! taskbar. Closing to tray is a hide; the tray's Exit is a real quit.
//!
//! Hiding is not quite enough on its own: gpui un-hides the window behind our back whenever a
//! display disappears. [`keep_hidden_across_display_changes`] is the answer to that.

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

/// Keeps a window we hid on purpose hidden when displays come and go.
///
/// gpui reads "the monitor my window was on is gone" as "Windows minimized me", and answers
/// `WM_DISPLAYCHANGE` with `ShowWindow(SW_SHOWNORMAL)` — which un-hides, and activates, a
/// window that was sitting quietly in the tray. Monitors going to sleep look exactly like a
/// disconnect: the link drops and the `HMONITOR` goes with it. The result is a tray-resident
/// SyncMaid that walks back onto the screen, taking the keyboard with it, on every wake.
///
/// So we sit in front of gpui's window procedure and put the window back afterwards. `hidden`
/// seeds what the guard believes about the window it is handed: `true` when the app started
/// minimized to the tray, since nothing has called [`hide`] in that case.
pub fn keep_hidden_across_display_changes(window: &Window, hidden: bool) -> bool {
    platform::install_guard(window, hidden)
}

/// Stands in for a monitor disappearing. Used by `--self-test-tray`, and nothing else.
///
/// A real disconnect is gpui un-hiding the window from *inside* its `WM_DISPLAYCHANGE`
/// handler, and short of unplugging a monitor we cannot make it take that branch — as far as
/// it can tell, every display is still there. So the gate un-hides the window itself and then
/// delivers the message, which exercises the half that is ours: that the guard notices and
/// puts the window back.
pub fn simulate_display_disconnect(window: &Window) -> bool {
    platform::simulate_display_disconnect(window)
}

fn set_visible(window: &Window, visible: bool) -> bool {
    platform::set_visible(window, visible)
}

#[cfg(windows)]
mod platform {
    use std::sync::atomic::{AtomicBool, Ordering};

    use gpui::Window;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        IsWindowVisible, SendMessageW, SetForegroundWindow, ShowWindow, SW_HIDE, SW_SHOW,
        SW_SHOWNORMAL, WM_DISPLAYCHANGE, WM_NCDESTROY,
    };

    /// Whether nothing is on screen because we put it away, as opposed to the user minimizing
    /// it or Windows moving it off a dying monitor. SyncMaid has exactly one native window, so
    /// one flag says it all.
    static HIDDEN_ON_PURPOSE: AtomicBool = AtomicBool::new(false);

    /// Ours among whatever else may be subclassing the window; only unique per window matters.
    const GUARD_SUBCLASS_ID: usize = 1;

    pub(super) fn set_visible(window: &Window, visible: bool) -> bool {
        let Some(hwnd) = hwnd_of(window) else {
            return false;
        };
        HIDDEN_ON_PURPOSE.store(!visible, Ordering::Relaxed);
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

    pub(super) fn install_guard(window: &Window, hidden: bool) -> bool {
        let Some(hwnd) = hwnd_of(window) else {
            return false;
        };
        HIDDEN_ON_PURPOSE.store(hidden, Ordering::Relaxed);
        // SAFETY: `hwnd` is the live window we were handed, and `guard_proc` has the signature
        // `SUBCLASSPROC` calls for. Windows holds the subclass until the window is destroyed,
        // at which point `guard_proc` takes itself back out of the chain.
        unsafe { SetWindowSubclass(hwnd, Some(guard_proc), GUARD_SUBCLASS_ID, 0) != 0 }
    }

    pub(super) fn simulate_display_disconnect(window: &Window) -> bool {
        let Some(hwnd) = hwnd_of(window) else {
            return false;
        };
        // SAFETY: as above. `SendMessageW` runs the window procedure inline, so by the time it
        // returns the guard has already had its say.
        unsafe {
            ShowWindow(hwnd, SW_SHOWNORMAL);
            SendMessageW(hwnd, WM_DISPLAYCHANGE, 0, 0);
        }
        true
    }

    /// Sits in front of gpui's window procedure, undoing the one thing it gets wrong about a
    /// window that is hidden rather than minimized. See `keep_hidden_across_display_changes`.
    unsafe extern "system" fn guard_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _subclass_id: usize,
        _reference_data: usize,
    ) -> LRESULT {
        if message == WM_NCDESTROY {
            // SAFETY: the window is going away; step out of its chain before it does.
            unsafe { RemoveWindowSubclass(hwnd, Some(guard_proc), GUARD_SUBCLASS_ID) };
            // SAFETY: handing the message on to the rest of the chain, as every subclass must.
            return unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
        }

        // gpui goes first: on a display it no longer recognizes it un-hides the window *and*
        // re-reads which monitor the window now lives on. That second half is bookkeeping we
        // want to keep, so let it run and undo only the un-hiding.
        // SAFETY: as above.
        let result = unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
        if message == WM_DISPLAYCHANGE && HIDDEN_ON_PURPOSE.load(Ordering::Relaxed) {
            // SAFETY: `hwnd` is live — we are running inside its own window procedure.
            unsafe { ShowWindow(hwnd, SW_HIDE) };
        }
        result
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

    pub(super) fn install_guard(_window: &Window, _hidden: bool) -> bool {
        false
    }

    pub(super) fn simulate_display_disconnect(_window: &Window) -> bool {
        false
    }
}
