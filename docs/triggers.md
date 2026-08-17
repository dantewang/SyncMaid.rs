# Triggers

A task's **trigger** decides when it runs. One per task, set in the task editor. Whatever
you choose, **Run now** always works.

## Manual

The task runs only when you click **Run now** (or **Run all**). Nothing runs in the
background. Good while you're setting a task up, and for anything you want to think about
before it happens.

## Scheduled

The task runs on a **cron expression**, in your computer's local time.

Standard five-field cron — minute, hour, day of month, month, day of week:

| Expression | Runs |
|---|---|
| `*/5 * * * *` | every 5 minutes |
| `0 * * * *` | every hour, on the hour |
| `0 2 * * *` | every day at 02:00 |
| `0 2 * * 1` | every Monday at 02:00 |
| `30 22 1 * *` | 22:30 on the 1st of each month |

As you type, the editor confirms the next occurrence — **Next run (local time): …** — so
you can check the expression means what you think. It refuses to save an invalid one, and
warns if a valid expression has no upcoming runs at all (`0 0 30 2 *` — February 30th).

The task card shows a live **next run in 2 h** badge; hover it for the absolute time.

If the computer is asleep or SyncMaid isn't running at the scheduled moment, that run is
missed, not queued — the next occurrence happens normally.

## Watch

The task runs whenever the source folder changes.

Changes rarely arrive one at a time: saving a single document can touch several files over
several seconds, and copying a folder in is a long burst. So the watch trigger waits for
the source to go **quiet** before running:

> **Quiet period** — Run after `10` seconds without changes.

Every new change restarts the wait, so one burst of activity becomes one sync run. Ten
seconds is the default; the range is 1–600.

**Choosing a value.** Set it longer than the slowest save your source sees. Too short, and
a long copy triggers several runs while it's still going; too long, and syncing feels
sluggish. For a scratch folder, 5 seconds is fine; for a folder that receives large files
over the network, 60 or more.

**Network sources.** On a mapped drive or UNC path, Windows change notifications are
unreliable, so SyncMaid polls the folder every few seconds instead and applies the same
quiet period. It works the same way from your side; it just notices changes a little later.

**A task never re-triggers itself.** Its watch is suspended while its own run is in
progress and resumes afterwards, so a Move task doesn't fire again on the files it just
removed.

## What happens during a run

The **Run now** button becomes **Stop** while a run is in progress, and each destination row
shows live progress:

```
Copying photos/2024/img_0042.jpg (3/120)
```

Pressing **Stop** cancels the run. Files already copied stay copied, the file in flight is
abandoned safely (the destination keeps whatever it had before), and each destination
returns to the status it had before the run — a cancelled run isn't a failure.

Runs of the same task never overlap. If a trigger fires while the task is already running,
the run in progress finishes first.
