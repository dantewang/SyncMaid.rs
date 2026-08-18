# Settings

Open **Settings** from the bottom of the task sidebar. It takes over the window; the arrow at
top left goes back to your tasks. Changes apply immediately unless noted.

## Language

**System default** follows Windows. You can also pick English, 简体中文, 繁體中文, or 日本語
directly. The whole window switches over as soon as you choose — no restart.

## Startup

**Start SyncMaid when Windows starts** — opens the main window when you sign in.

SyncMaid registers itself the standard, visible way (a per-user entry Windows shows under
**Task Manager → Startup apps**), so no administrator rights are needed and you can always
see it's there.

If you turn SyncMaid off in Task Manager, the setting reports:

> Startup is turned off for SyncMaid in Windows Task Manager (Startup apps). Turn it back on
> there to allow starting with Windows.

Windows' own switch wins, and SyncMaid deliberately doesn't override it — re-enable it in
Task Manager.

Pair this with **Start minimized** below if you want SyncMaid running from sign-in without a
window appearing.

## Window

**Close to the system tray instead of exiting** — closing the window hides it instead of
quitting, so scheduled and watched tasks keep running. Restore it by clicking the tray icon,
or right-click the icon → **Show main window**. **Exit** in the same menu really quits.

With this off, closing the window exits the app and no automatic syncing happens until you
start it again.

**Start minimized to the system tray** — SyncMaid launches hidden, with only the tray icon.
Tasks run as usual. Takes effect on the next launch.

## Storage

SyncMaid is **portable**. Everything it saves — your tasks, settings, status and log — lives
in a `Data` folder beside `SyncMaid.exe`, so moving that folder takes your tasks with it and
the machine it ran on keeps nothing. The page shows the exact path and opens it for you.

There is nothing to choose here, and that is the point: put the app where you want its data,
and the two never come apart.

Two consequences worth knowing:

- **Put it somewhere you can write to.** A copy under `Program Files` cannot create its own
  `Data` folder without administrator rights, so keep it in your user folder, on another
  drive, or on a stick.
- **"Start with Windows" points at the app's current location.** If you move the folder,
  re-enable it there.

Two copies in *different* folders are two different installs with two different `Data`
folders, and both may run. Two launches of the *same* folder are not: the second one brings
the window of the copy already running to the front instead of starting a second app over the
same files.

## About

The installed version.

## Where things are on disk

Inside the `Data` folder beside `SyncMaid.exe`:

| File | What it is |
|---|---|
| `tasks.json` | your tasks and destinations |
| `status.json` | the last result per destination |
| `settings.json` | the settings on this page |
| `*.bak` | the previous version of each of the above, kept automatically |
| `logs\syncmaid.log` | the activity and error log |

All of these are plain text; back them up by copying the folder. If you hand-edit
`tasks.json`, SyncMaid validates it on load and refuses task layouts that break the
[rules](tasks-and-destinations.md#rules-syncmaid-enforces) — and if the file is there but
cannot be read at all, it says so and stops saving rather than replacing your tasks with an
empty list.
