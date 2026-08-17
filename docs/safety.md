# Safety

SyncMaid's design priority is simple: **never lose a file**. Nothing below needs configuring
— it is how every run already behaves — but knowing it makes the app's occasional refusals
and prompts make sense.

## Every copy is complete, or it never happened

A file is never written over in place. Each copy goes to a temporary file next to its
destination, is flushed to disk, is checked, and only then atomically replaces the previous
version.

So if the power cuts, the disk fills, the network drops, or you press **Stop** mid-copy, the
destination still holds the *previous* complete version of that file. You never end up with a
half-written file where a good one used to be. A leftover temporary file is cleaned up on the
next run.

**Move** applies the same rule in the direction that matters: the source file is deleted only
after the destination copy exists and has been verified.

## Verification

Every copy is checked for the failure that actually happens — a truncated or partial write.

For more than that, turn on **Verify file contents (xxHash) after each copy** in the
destination editor. SyncMaid then reads each copied file back and compares a hash against the
source, catching silent corruption from failing RAM, cables, or controllers that a size check
can't see.

It costs a full re-read of everything copied. On a local disk that's usually cheap. On a
network location the editor warns you, because the re-read crosses the network:

> This is a network location. Content verification re-reads every copied file over the
> network (slower, more bandwidth). It guards against silent/hardware corruption the network
> protocol does not.

Worth turning on for irreplaceable data (photo archives, documents) and for destinations on
older or flaky hardware. Not worth it for a scratch folder you re-sync all day.

## Mirror deletions

Mirror deletes destination files the source no longer has. Two guards sit in front of that.

**A missing or empty source never deletes anything.** If the source folder is unavailable
when the run starts — an external drive not plugged in, a network share not mounted, a
permission problem — Mirror would technically be "correct" to empty the destination. It
doesn't. The destination is left untouched and the run reports:

> Source is empty or unavailable; skipped deletions to avoid wiping the destination.

This one can't be overridden, because there is no version of it that isn't a mistake. Fix the
source and run again.

**A large deletion asks first.** If a single run would delete more than half of the files in
the destination (the default), the run stops before deleting anything and the destination row
shows **Needs confirmation** with a **Review deletions** button. That opens a window listing
how many files, and a sample of which:

> Syncing "Photos backup" will move 412 files to the Recycle Bin — they are no longer in the
> source.

**Keep them** cancels the deletions; **Move to Recycle Bin** (or **Delete N files**) approves
them for that run only — it is never remembered as a preference. The window is independent of
the main one, so it still appears when SyncMaid is running from the tray.

Both the threshold and the prompt are per destination, under **Confirm before a large
deletion**: **Ask when deleting more than `50` % of the destination**. Turn the checkbox off
to disable the ratio check entirely (the empty-source guard still applies). The ratio is only
applied once a destination holds a meaningful number of files — deleting "most" of five files
isn't alarming.

## Deleted files are recoverable

Under **When removing extra files**, Mirror defaults to the **Recycle Bin**, so anything it
removes can be restored. **Delete permanently** is available when you don't want a Recycle
Bin filling with churn.

Network shares have no Recycle Bin. There, removals are permanent whatever this is set to —
worth remembering when mirroring to a NAS.

## Files in use, and other transient failures

A file another program is holding open isn't an error. SyncMaid retries briefly, and if it's
still busy, leaves it for next time: the status reads **Synced 5 minutes ago · 340 files, 2
in use**, and the task card shows **2 in use**. The next run picks them up once the other
program is done.

Genuine errors — a full disk, a permission denial — fail that destination with the file named
in the message, and leave the other destinations of the task to finish.

## What SyncMaid does not protect against

- **A file deleted from the source before a sync.** One-way sync means the destination
  follows the source, and under Mirror that includes deletions. Use **Add-only** for a
  destination that should never lose anything.
- **Corruption that happens after a successful sync** — bit-rot on the destination drive,
  or someone editing the files later. SyncMaid verifies data as it moves it, not forever
  after.
- **Being your only backup.** One-way sync propagates your mistakes faithfully. Keep a
  destination that isn't a mirror, or versioned backups, for anything you can't recreate.
