# Troubleshooting

## Reading the statuses

Each destination row carries its own status:

| Status | Meaning |
|---|---|
| **Never run** | Added but not yet synced. |
| **Syncing…** | Running now; the row shows the current file and progress. |
| **Synced 5 minutes ago · 120 files** | Finished; that many files are in the destination. |
| **Synced 5 minutes ago · 118 files, 2 in use** | Finished, but some files were open in another program and were skipped. They'll sync on a later run. |
| **Needs confirmation** | A Mirror run stopped before a large deletion. Click **Review deletions** — see [Safety](safety.md#mirror-deletions). |
| **Failed · <reason>** | That destination failed. The message names the file and what went wrong. |

The task card summarises them: **All synced**, **Partly synced**, **2 of 3 failed**,
**4 in use**, or **No destinations**.

## Common situations

### "Trigger error" on a task card

The amber badge means the task's automatic trigger isn't working — usually a source folder
that has gone away (unplugged drive, disconnected share). Hover it for the reason. The task
still runs when you click **Run now**. Reconnect the folder and the trigger recovers; if it
never started, edit the task and save it again.

### The task doesn't run automatically

- Check the trigger badge: a task set to **Manual** only runs when you click.
- With a **Scheduled** task, check the **next run in …** badge. If a run's time passes while
  the computer is asleep or SyncMaid is closed, that occurrence is missed — not queued.
- SyncMaid has to be running. If closing the window exits the app, turn on
  [**Close to the system tray**](settings.md#window) so it keeps working in the background.
- With a **Watch** task, remember the quiet period: the run starts a number of seconds after
  the last change, not immediately.

### A destination says "Failed"

The message names the file and the operation (`Failed to copy 'photos/img.jpg': …`). The
usual causes are a full disk, a permission denial, or a destination that vanished mid-run.
Other destinations of the same task carry on independently, so a single failure doesn't stop
the rest.

### Files aren't arriving at a destination

- Check the destination's **Files to sync**. The preview line spells out what it actually
  matches; **`No rules yet — nothing will be synced.`** means **Only matching** is selected
  with no rules. See [Filters](filters.md).
- Check the file is under the source folder you think it is.
- Under **Move**, files another program is holding open are skipped until it lets go.

### "This folder doesn't exist"

A hint, not an error — you can still save.

- On a **source**: nothing will sync and a watch trigger can't start until the folder exists.
  Usually a typo or a drive that isn't connected.
- On a **destination**: it will be created on the first run. Worth re-reading for typos, so
  you don't create `D:\Bakcup`.

### The editor won't let me save

SyncMaid refuses layouts that can't work, each with a hint saying why:

- *Destination must be a separate folder outside the source (and not contain it).*
- *This folder overlaps "…" in this task — destinations never overlap each other.*
- *This folder overlaps the source of task "…" — tasks never share sources.*

The reasoning is in [Tasks and destinations](tasks-and-destinations.md#rules-syncmaid-enforces).
The fix is always to pick a different folder, or to restructure into separate tasks.

### A run refused to start

> This task's paths overlap task "Photos"; fix the overlap and run again — no files were
> changed.

Two tasks ended up sharing a source or a destination — typically after hand-editing
`tasks.json`, or after editing one task's folder to something another task already uses. No
files were touched. Edit one of them to a different folder.

### Deletions I didn't expect

Only **Mirror** deletes, and only files the source no longer has. If a Mirror destination
lost files you wanted, they're in the Recycle Bin unless the destination is set to
**Delete permanently** or is a network share (which has no Recycle Bin). Switch that
destination to **Add-only** if it should never lose anything.

## The log

`Data\logs\syncmaid.log`, beside `SyncMaid-rs.exe` (see
[where things are on disk](settings.md#where-things-are-on-disk)). It is the place to look when a status
message isn't specific enough, and it rolls over at about 5 MB, keeping one previous file.

Every run writes one line per destination, so the log is also the history the window doesn't
show — the row only ever displays the *latest* result:

```
2026-08-09 02:00:12.418 [INF] TaskNodeViewModel: Sync 'Photos' → 'NAS backup': Success · 128 copied (12.4s)
2026-08-09 02:00:12.702 [INF] TaskNodeViewModel: Sync 'Photos' → 'USB': Incomplete · 126 copied, 2 in use (9.1s)
2026-08-09 03:00:01.233 [WRN] TaskNodeViewModel: Sync 'Photos' → 'NAS backup': Failed · Failed to copy 'raw/img.dng': access denied. (0.8s)
```

Failures and runs awaiting confirmation are logged as warnings, so searching for `[WRN]`
finds the runs that need you. The log also names the files a run left in use — which the
status row has no room for — and records trigger problems and stopped runs.

## Starting over

Deleting a task removes it from SyncMaid only — **synced files are left alone at both ends**.

To reset SyncMaid completely, exit it and delete `tasks.json`, `status.json` and
`settings.json` from the configuration folder — along with their `.bak` siblings, which
SyncMaid otherwise falls back to automatically when a file can't be read.

