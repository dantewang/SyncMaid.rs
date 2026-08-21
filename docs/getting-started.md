# Getting started

SyncMaid keeps one or more **destination** folders in sync with a **source** folder, one
way: the source is the truth, and the destinations follow it. It can run when you click
**Run now**, on a schedule, or whenever the source changes.

## Install

SyncMaid ships as a single `SyncMaid-rs.exe`. There is no installer and no runtime to install —
copy it anywhere you can write to (your user folder, an external drive) and run it.

It is **portable**: everything it saves lives in a `Data` folder beside the executable, so a
copy on a USB stick carries its configuration with it and leaves nothing behind on the
machine it ran on. See [Storage](settings.md#storage).

## Your first task

1. Click **New task**.
2. Give it a **Name** — anything that will remind you what it does ("Photos to NAS").
3. Pick the **Source folder** with **Browse**. This is the folder SyncMaid reads *from*;
   it is never modified, except by the Move strategy.
4. Leave the **Trigger** on **Manual** for now — you can add automation once you have
   watched a run do what you expect.
5. **Save task**.

The task appears as a card with the badge **Manual** and the note **No destinations**. It
doesn't do anything yet, because you haven't said where the files should go.

## Add a destination

1. Click **Add destination** on the task card.
2. Name it, and choose the **Destination folder**. It doesn't have to exist yet — SyncMaid
   creates it on the first run. It must, however, be a separate folder: not inside your
   source, and not a folder containing it.
3. Choose a **Sync strategy**:
   - **Mirror** — keep the destination identical to the source, deleting anything the
     source no longer has.
   - **Add-only** — copy new and changed files, never delete.
   - **Move** — move files out of the source into the destination.

   If you're unsure, start with **Add-only**: it never deletes anything.
   [More on strategies](tasks-and-destinations.md#sync-strategies).
4. **Save destination**.

## Run it

Click **Run now** on the task card. Each destination row shows its progress as it goes
(`Copying photos/2024/img_0042.jpg (3/120)`) and finishes with a status like
**Synced just now · 120 files**. The task card summarises the destinations as **All
synced**.

That's a complete task. From here:

- Make it automatic → [Triggers](triggers.md)
- Send only some files to a destination → [Filters](filters.md)
- Understand what protects your files → [Safety](safety.md)
- Something looks wrong → [Troubleshooting](troubleshooting.md)

## A task can have several destinations

Click **Add destination** again to fan the same source out to more than one place — for
example a full **Mirror** to an external drive and an **Add-only** copy of just the raw
files to a NAS. Each destination has its own strategy, its own filters, and its own status.

A **Move** task fans out too, but differently: its destinations are rules matched in order,
and each file goes to the first one that matches it — so several destinations share the
source out rather than each taking a copy of it.

## Where your configuration lives

Tasks, settings and the log live in a `Data` folder beside `SyncMaid-rs.exe` — see
[Storage](settings.md#storage).

