//! The system tray icon, on its own thread.
//!
//! Windows requires the tray icon to be created on a thread that runs a win32 message pump,
//! but not that it be the main one — so the tray gets a thread of its own with its own
//! `GetMessage` loop, and talks to the UI over a channel. That keeps it entirely out of gpui's
//! event loop, which owns the main thread and has no seam to borrow it from.

use std::sync::mpsc;

use anyhow::{Context as _, Result};

/// What the user asked for from the tray.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCommand {
    /// Left click, or the "Show main window" item.
    ShowMainWindow,
    /// The "Exit" item. A real quit, not a hide.
    Exit,
}

/// Text for the tray, passed in so the localizer stays the only source of display strings.
pub struct TrayLabels {
    pub tooltip: String,
    pub show_main_window: String,
    pub exit: String,
}

/// Starts the tray and returns the channel its commands arrive on.
///
/// Fails only if the tray itself could not be created; the caller decides whether that is fatal
/// (it is not — the app is still usable, it just cannot be reached once hidden).
pub fn start(labels: TrayLabels) -> Result<flume::Receiver<TrayCommand>> {
    let (commands, receiver) = flume::unbounded();
    let (started, startup) = mpsc::channel();

    std::thread::Builder::new()
        .name("syncmaid-tray".into())
        .spawn(move || {
            match platform::install(&labels, commands) {
                Ok(tray) => {
                    let _ = started.send(Ok(()));
                    // The tray must outlive the pump: dropping it removes the icon.
                    platform::run_message_loop();
                    drop(tray);
                }
                Err(error) => {
                    let _ = started.send(Err(error));
                }
            }
        })
        .context("spawn the tray thread")?;

    startup
        .recv()
        .context("the tray thread stopped before reporting")??;
    Ok(receiver)
}

#[cfg(windows)]
mod platform {
    use anyhow::{Context as _, Result};
    use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{
        Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, TranslateMessage, MSG,
    };

    use super::{TrayCommand, TrayLabels};

    /// Resource id the build script embeds `assets/syncmaid.ico` under, shared with the
    /// executable's own icon so the two can never drift apart.
    const ICON_RESOURCE_ID: u16 = 1;

    pub(super) fn install(
        labels: &TrayLabels,
        commands: flume::Sender<TrayCommand>,
    ) -> Result<TrayIcon> {
        let show = MenuItem::new(&labels.show_main_window, true, None);
        let exit = MenuItem::new(&labels.exit, true, None);
        let show_id = show.id().clone();
        let exit_id = exit.id().clone();

        let menu = Menu::new();
        menu.append_items(&[&show, &PredefinedMenuItem::separator(), &exit])
            .context("build the tray menu")?;

        let icon = Icon::from_resource(ICON_RESOURCE_ID, None).context("load the tray icon")?;
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip(&labels.tooltip)
            .with_icon(icon)
            .build()
            .context("create the tray icon")?;

        let menu_commands = commands.clone();
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            let command = if event.id == show_id {
                TrayCommand::ShowMainWindow
            } else if event.id == exit_id {
                TrayCommand::Exit
            } else {
                return;
            };
            let _ = menu_commands.send(command);
        }));

        TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
            // Only a completed left click, so the menu's own right-click does not also
            // summon the window behind it.
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let _ = commands.send(TrayCommand::ShowMainWindow);
            }
        }));

        Ok(tray)
    }

    pub(super) fn run_message_loop() {
        // SAFETY: a textbook win32 pump. `message` is a live, zeroed MSG for every call, and
        // this thread owns the tray's hidden window, so it is the thread that must pump it.
        unsafe {
            let mut message: MSG = std::mem::zeroed();
            while GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use anyhow::{bail, Result};

    use super::{TrayCommand, TrayLabels};

    pub(super) fn install(
        _labels: &TrayLabels,
        _commands: flume::Sender<TrayCommand>,
    ) -> Result<()> {
        bail!("the tray is only implemented for Windows")
    }

    pub(super) fn run_message_loop() {}
}
