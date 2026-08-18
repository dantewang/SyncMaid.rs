# Agent guidelines

Guidance for AI agents working in this repository.

This is the Rust port of [SyncMaid](https://github.com/dantewang/SyncMaid) (C# / Avalonia). The
product rules below are carried over unchanged and are the reason the port exists in the shape
it does — they are **product decisions, not implementation details**. Enforce them; do not
engineer around them.

## Layout

- `crates/core` (`syncmaid-core`) — the engine. No window, no display strings, no platform
  services. Everything with a rule in it lives here so it can be tested against an in-memory
  filesystem with fault injection.
- `crates/app` (`syncmaid`) — the GPUI shell. A library as well as a binary, so the parts that
  carry real rules (the run gate, the card summary, the workspace's persistence guards) are
  testable end to end without a window.
- `crates/app/src/state` — what the UI shows, kept apart from how it is drawn. Prefer putting
  logic here over putting it in a `Render` impl; a rule the user can feel should be testable
  without a renderer.

## UI implementation

- **Pick the right dialog host.** `Window::open_dialog` — `gpui-component`'s dialog layer — is for
  flows the user starts from the visible main window (editors, delete confirms). Anything that
  can appear while the app is hidden in the tray — the mirror-delete confirmation — must be an
  independent top-level window, or nobody sees it.
- **`Root` holds the dialog stack; it does not draw it.** The application's own root view has to
  render `Root::render_dialog_layer` and `render_notification_layer` as children, or
  `open_dialog` succeeds and shows nothing.
- **Do not open a dialog from inside `WindowHandle::update`.** That closure already holds `Root`
  borrowed, and `open_dialog` wants it too. `Window::defer` moves the call one tick later.
- **The title bar is the system's.** `TitlebarOptions { appears_transparent: false, .. }`, so
  dragging, the system menu, double-click to maximize and Windows 11's snap-layout flyout are
  the OS's to provide rather than ours to re-implement.
- **`WindowOptions::window_bounds` is in logical pixels.** GPUI applies the display scale
  itself; scaling the request as well opens a window that is scale-times too big with the UI
  painted into one corner. `Window::viewport_size()` is likewise already in layout units and
  must not be divided by `scale_factor()`.
- **Colour comes from the theme, never from a constant.** `cx.theme().danger`, not a hex literal:
  the palette is `gpui-component`'s Ayu Light, loaded from a compiled-in JSON. A hard-coded
  colour is a piece of the UI that stops following the theme forward.
- **Icons are Lucide, reached through `Glyph`.** Most resolve to an `IconName`; the dozen with no
  variant (Play, Stop, Refresh, Trash, Pencil, Clock, Funnel …) resolve to an SVG under
  `assets/icons`. Add a glyph in one place, not at the call site.
- **Measure screenshots with a DPI-aware process.** Windows virtualizes `GetWindowRect` and
  `PrintWindow` for DPI-unaware callers, dividing everything back down — which hides exactly
  the class of bug above. Call `SetProcessDpiAwarenessContext(-4)` first.

## Localization

- **Never hardcode user-facing text.** Every display string lives in `crates/app/lang/en.json`
  with full translations in `zh-Hans.json`, `zh-Hant.json` and `ja.json`. The build fails if
  the four files are not key-identical. The product name and the cron placeholder pattern are
  the only deliberate literals.
- `build/strings.rs` generates one typed accessor per key, so a key that does not exist is a
  **compile error** and so is calling one with the wrong number of arguments. Plain keys become
  `strings::some_key()`, `{0}`-bearing keys take that many `Display` arguments, and a
  `.One`/`.Other` pair collapses into one function taking a count.
- Engine messages stay English — `syncmaid-core` carries no display strings. Localize only the
  sentence wrapped around them.
- Only the UI language switches. Dates and numbers keep following the system's regional
  settings, which the user has already expressed separately.

## Task shape conventions

Each is validated in the editor (blocked with a hint) **and** in the engine (the run fails
without touching files), so hand-edited config is covered too.

- **A task's source and destinations never nest.** A destination must not equal the source, be
  inside it, or contain it — either direction, every strategy. Sibling folders under a common
  parent are fine. A destination inside the source turns output into input; a source inside a
  destination makes Mirror treat the live source as orphaned content and delete it. Do **not**
  add code to make nested layouts work (excluding a nested subtree from planning, say) —
  reject the layout.
- **A task's kind decides its strategies.** `Sync` means Mirror and Add-only destinations;
  `Move` means Move destinations only. Move's postcondition (an emptied source) contradicts
  every other strategy's precondition (the source is the truth), so mixtures have no coherent
  semantics. The kind is chosen while the task is empty and locked afterwards; it persists as
  an `Option` so config written before it existed stays distinguishable from an explicit
  `Sync` and derives its kind from its destinations.
- **A Move task's destinations are an ordered rule list; first match wins.** Each source file
  goes to the first destination whose filters include it and to no other; files nothing
  matches stay in the source. Order is therefore semantic — persist it, and never reorder as a
  side effect of anything. Overlap is deliberately legal: the common routing pair, `*.pdf` and
  `invoices/`, overlaps and is perfectly sensible.
- **Mirror takes no file filters.** Its contract is tree identity — whenever no task is
  running, a tree compare of source and destination reports identical, empty directories
  included — and a filtered subset contradicts that by definition. The editor hides the filter
  section for Mirror and persists a lone all-files filter; the engine refuses a hand-edited
  Mirror destination whose filter list is anything else.
- **Tasks never share same-kind paths.** Across tasks, a source may not equal or nest with
  another task's source, nor a destination with another task's destination. Chaining — task A
  files into a folder task B backs up — is explicitly allowed: chained runs converge via
  trigger coalescing and idempotent planning.
- **Destinations never overlap each other, including two of the same task.** Same rationale
  one level down: a Mirror destination deletes as orphans whatever the sibling writing into
  its subtree just put there.

## Sync safety

Stated priority: **avoid file loss at all costs.** These are invariants, not defaults — never
add a faster path that skips them.

- **Every write is temp → verify → atomic rename** (`sync::safe_transfer`). Nothing overwrites
  a destination file in place, and Move deletes the source only once the destination verifies.
  Byte-moving code goes through it, not through `FileSystem` directly.
- **Mirror deletions pass `MirrorGuard` first.** An empty or unavailable source emits zero
  deletes and is **not** overridable; a mass delete needs one-shot user confirmation that is
  never persisted. Deletes go to the Recycle Bin by default.
- **Safety logic lives in UI-free core** so `InMemoryFileSystem` can fault-inject it. New
  safety behaviour ships with a fault-injection test.
- **A run never re-triggers itself into a loop.** Runs of a task are serialized by `RunGate`;
  while one is active, further requests coalesce into a single follow-up rather than starting
  a second writer. The trigger stays **live** across a run — nothing calls `stop` around one —
  so a run that mutates its own source (only Move does) fires the trigger once afterwards.
  That follow-up is a no-op, because planning is idempotent. Do not "fix" it by suppressing
  the trigger around runs without re-reading `tests/self_triggering_run.rs`, which pins the
  cost at exactly one extra run and no cascade.
- Notifications deliver outside the owner's state gate (`TriggerNotifier`): a subscriber must
  never block a watcher callback, and a source's own I/O must not hold that gate either, or
  `stop` blocks behind a dead network share.

## Persistence

- **Byte-compatible with the C# build.** `tasks.json`, `status.json` and `settings.json` are
  read and written in exactly the shape `System.Text.Json` produced, CRLF included, so the two
  builds can share a `Data` folder. `examples/compat_check.rs` checks a real config round-trips
  byte-identically.
- **Config writes go through `AtomicFile` / `JsonConfigFile`** — temp → rename, previous
  version kept as `.bak` and loaded as a fallback. Never write over `tasks.json` in place.
- **Config that exists but cannot be read switches saving off.** Writing then would replace a
  file the user still has with one we invented.
- **Old config keeps loading.** Normalize legacy shapes on save; never silently discard what
  the current editor cannot represent — carry it through untouched instead.

## Platform-specific services

- Put the implementation behind a trait in `platform`, `#[cfg(windows)]` the real one, and
  provide a no-op fallback so callers never branch on the platform.
- **Never write `StartupApproved\Run`.** Windows' own startup switch wins; overriding it is
  exactly the behaviour antivirus heuristics look for, and the user meant it when they used
  it. The registry is the single source of truth for autostart — nothing about it is mirrored
  into `settings.json`, so the two can never disagree.

## Errors

- **Never swallow an error.** The file logger's own I/O failures are the single exemption.
  Surface the failure where the user is: trigger failures as the card's amber badge, per-file
  failures with the path and the verb. Cancellation propagates untouched and **is not a
  failure** — what landed stays, and the rows go back to what they said before.

## Commits

- Do **not** add a "Co-Authored-By: Claude" trailer, or any AI co-author or attribution
  trailer, to commit messages.
- Write clear, conventional commit messages describing the change and its intent.
