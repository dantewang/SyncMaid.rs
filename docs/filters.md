# Filters

Filters decide **which source files reach a particular destination**. They are set per
destination, under **Files to sync**.

Two choices to start with:

- **All files** — everything under the source. This is the default.
- **Only matching** — the file has to match the rules you build below.

> **Mirror destinations have no filter section.** Mirror's job is to make the destination
> identical to the source, so it always syncs everything. Choose Add-only or Move if you
> need a subset.

## Rules

Each rule is one of:

- **Path** — files under a folder, relative to the source root. `photos/2024` matches
  everything under `<source>\photos\2024`.
- **Extension** — files of a type. `jpg` matches every `.jpg` anywhere under the source.
- **Wildcard** — a pattern matched against the file's path relative to the source. This is
  the one that can look at a file's *name*.

Type the value into the box next to the rule (the placeholder shows the shape:
`e.g. photos/2024, jpg, or **/ChatGPT*.png`).

### Wildcard patterns

| Symbol | Matches |
|---|---|
| `*` | any run of characters, but never across a folder boundary |
| `?` | exactly one character, likewise |
| `**` | any number of folder levels, including none |

| Pattern | Matches |
|---|---|
| `**/ChatGPT*.png` | `ChatGPT Image 1.png`, `saved/ChatGPT-2.png`, at any depth |
| `photos/*.raw` | `photos/a.raw`, but **not** `photos/2024/a.raw` |
| `photos/**/*.raw` | every `.raw` under `photos\`, however deep |
| `photos/**` | everything under `photos\` (the same as a Path rule) |
| `IMG_????.jpg` | `IMG_0042.jpg` at the source root |

Patterns are case-insensitive, and `\` works the same as `/`. A leading `**/` is what makes
a pattern apply at any depth: `ChatGPT*.png` on its own only matches files sitting directly
in the source folder.

## Groups: any, all, and exclude

Rules live in **groups**, and a group can require **any rule** (OR) or **all rules** (AND):

- **Match `any rule`** — a file matches when at least one rule in the group matches.
- **Match `all rules`** — a file matches only when every rule matches.

Each rule also has an **exclude** toggle, which flips it: *sync everything except files
matching this rule*. An excluded rule shows as **Exclude — Extension: tmp**.

With more than one group, **Add group** gives you a second level, and you choose how the
groups combine:

- **Match any group** — sync files matching at least one group.
- **Match all groups** — sync only files matching every group.

That's enough to express the combinations people actually want:

| You want | Build it as |
|---|---|
| Everything under `docs/`, plus every `.jpg` anywhere | One group, **any rule**: Path `docs`, Extension `jpg` |
| Only `.jpg` files that are under `photos/` | One group, **all rules**: Path `photos`, Extension `jpg` |
| `.jpg` or `.png`, but only under `photos/` | **Match all groups** — group 1 (any rule): Extension `jpg`, Extension `png`; group 2: Path `photos` |
| Everything except `.tmp` files | One group: **All files**, plus a rule Extension `tmp` with **exclude** on |
| The PNGs whose name starts with ChatGPT | One group: Wildcard `**/ChatGPT*.png` |

## The preview line

Under the rules, SyncMaid shows what you have actually built, in words:

```
Syncs: (docs/ or photos/) and jpg
```

Read it before saving. It is the quickest way to catch an *any* that should have been an
*all* — a mistake that silently syncs far more, or far less, than you meant.

Two previews worth recognising:

- **`Syncs: all files`** — no filtering; everything goes.
- **`No rules yet — nothing will be synced.`** — you chose **Only matching** but haven't
  added a rule. The destination will receive nothing until you do.

## How filters interact with the strategies

- **Add-only** — filters select what gets copied. Files that stop matching are simply not
  copied any more; whatever already reached the destination stays.
- **Move** — filters select what gets moved out of the source. Anything that doesn't match
  any of the task's rules is left where it is. Where a file matches more than one rule, the
  first rule in the list takes it — see
  [Tasks and destinations](tasks-and-destinations.md#what-a-task-does).
- **Mirror** — no filters, by design.

Changing a filter does not retroactively clean up a destination. Under Add-only nothing is
ever removed, so narrowing a filter leaves the old files in place; delete them yourself if
you don't want them.
