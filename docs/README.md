# SyncMaid user guide

How to use SyncMaid — one-way file sync for Windows: a source folder, one or more
destinations, each with its own filters and strategy, run manually, on a schedule, or
whenever the source changes.

New here? Start with [Getting started](getting-started.md); it takes about five minutes to
have a working task.

## Guides

- **[Getting started](getting-started.md)** — install, create your first task, add a
  destination, run it.
- **[Tasks and destinations](tasks-and-destinations.md)** — the Mirror, Add-only and Move
  strategies, several destinations per task, and the folder layouts SyncMaid enforces.
- **[Filters](filters.md)** — choosing which files reach a destination, from a single
  extension to `(docs or photos) and jpg`.
- **[Triggers](triggers.md)** — running manually, on a cron schedule, or by watching the
  source for changes.
- **[Settings](settings.md)** — language, starting with Windows, the system tray, and where
  your data is kept.
- **[Safety](safety.md)** — what protects your files during a sync, what the mirror-delete
  confirmation is asking, and what SyncMaid deliberately won't do.
- **[Troubleshooting](troubleshooting.md)** — what each status means, why a task might not
  be running, and where the log is.

## Elsewhere

Design records and the reasoning behind SyncMaid's behaviour live in the
[wiki](https://github.com/dantewang/SyncMaid/wiki) of the original C# build, which this Rust
port follows. Rules for working on the code are in [AGENT.md](../AGENT.md).

