# Tasks and destinations

A **task** is one source folder plus a trigger. A **destination** is one place that source
goes, with its own strategy and filters. Every task can have several destinations.

## What a task does

You choose this when you create the task, and it decides which destinations the task will
accept. It is locked once the task has any, since switching would invalidate all of them —
empty the task first if you need to change your mind.

**Sync** copies the source and leaves it where it is. Its destinations are **Mirror** or
**Add-only**, each filtering the whole source independently, so the same file can go to
several of them. Each destination is synced separately and has its own status.

**Move** files the source away. Its destinations are all **Move**, and they form an *ordered
list of rules*: each file goes to the **first rule that matches it** and nowhere else, since
a file can only be moved once. Files no rule matches stay in the source. This is the
"downloads land here, file them there" task — PDFs to `Books\`, anything under `invoices\`
to `Bills\`, and (if you add a last rule for all files) whatever is left to `To sort\`.

Because the first match wins, **rules may overlap** — you don't have to make them mutually
exclusive. `report.pdf` under `invoices\` matches both rules above; the one listed first
takes it. Reordering the rules is therefore a real edit, not cosmetic.

## The task card

Each task is a card in the main window:

- **Name** and the source folder.
- A **trigger badge**: **Manual**, **Watching**, or **Scheduled · `0 2 * * *`**. A
  scheduled task also shows **next run in 2 h** (hover for the exact local time).
- A **health summary** of its destinations: **All synced**, **Partly synced**,
  **2 of 3 failed**, **4 in use**, or **No destinations**.
- Buttons: **Run now** (becomes **Stop** while running), **Add destination**,
  **Edit task**, **Delete task**.

**Run all** at the top runs every task. The sidebar lists your tasks; selecting one scrolls
its card into view.

## Sync strategies

Set per destination, in the destination editor.

### Mirror

> Keep the destination identical — copies new and changed files, deletes extras.

Mirror reproduces the source exactly: new and changed files are copied, files the source no
longer has are removed, and the directory structure is replicated — including empty
directories. When no run is in progress, comparing the two trees shows no difference.

Because that identity is the whole contract, **Mirror takes no file filters**: a filtered
mirror would be identical to nothing in particular. The destination editor hides the filter
section when you choose Mirror.

Deletions are what make Mirror powerful and what make it worth understanding. They go to the
Recycle Bin by default, and a run that would delete an unusual proportion of the destination
stops and asks first — see [Safety](safety.md#mirror-deletions).

### Add-only

> Copy new and changed files; never delete from the destination.

The safe accumulator. Files removed from the source stay in the destination forever, and
nothing SyncMaid does under this strategy can lose data at the destination. Use it for
backups where you'd rather keep too much than too little, and for destinations you also
write to by hand.

### Move

> Move matching files to the destination, removing them from the source.

The strategy every destination of a **Move task** uses; it is not offered to a Sync task.
Files that match the rule are moved out of the source, so each one is claimed by exactly one
rule — the first that matches it.

Things worth knowing:

- **Move and the copying strategies never share a task.** Move empties the source; Mirror
  and Add-only treat the source as the truth. Combining them has no sensible meaning, which
  is why the choice is made once, as the task's kind.
- **The folders a run empties are cleaned up.** A folder the moves left empty is removed
  (following the destination's delete mode); a folder that was already empty, and the source
  folder itself, are left alone.
- **Files can land flat.** By default a file keeps the folders it sat in (`2026\a.pdf` →
  `Books\2026\a.pdf`). Switch the destination to put every file directly in the folder and
  it becomes `Books\a.pdf` — which can collide, so that option comes with a choice: leave
  the file where it is and report it (the default: nothing is renamed or overwritten), or
  move it under a numbered name (`a (2).pdf`).
- Files another program is holding open are skipped, not failed — the task reports them as
  *in use* and moves them on a later run.

## Rules SyncMaid enforces

These are refused in the editor with an explanation, and re-checked when a run starts, so
they hold even for hand-edited configuration.

**A destination is never inside its own source, and never contains it.**
Sibling folders under a common parent are fine, but nesting is not:
`D:\Photos` → `D:\Photos\Backup` is rejected. A destination inside the source would feed the
app's own output back in as input; a source inside a destination would make Mirror see your
live files as extras to delete.

**Two tasks never share a source, and never share a destination.**
Not the same folder, and not nested in one another either. Tasks run independently and
concurrently, so overlapping destinations would race on the same files and overlapping
sources would process the same files twice.

**Two destinations of the same task never overlap either.**
Same rule, same reason: a Mirror destination would see whatever the destination beside it
wrote into its subtree as extras, and delete them.

**Chaining is allowed.** One task's destination *may* be another task's source — task A
moves files into a folder that task B backs up. This is a supported layout: the runs settle
in sequence.

## Editing and deleting

**Edit task** changes the name, source, or trigger; **Edit** on a destination row changes
everything about that destination. Changes take effect from the next run.

Deleting a task or a destination asks for confirmation first and cannot be undone — but it
only removes the task from SyncMaid. **Files already synced are left alone**, at both ends.
