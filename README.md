<div align="center">

<img src="assets/syncmaid.png" alt="SyncMaid" width="128" />

# SyncMaid

**This repo is a boring re-write of https://github.com/dantewang/SyncMaid in Rust + GPUI**

**One-way file sync for Windows, done for you.**

SyncMaid watches a source folder and keeps one or more destinations in sync with it — each
destination with its own filters and strategy (mirror, add-only, or move), triggered manually,
on a schedule, or by watching for changes. It lives in the system tray and keeps working while
you don't.

</div>

This is the Rust port of [SyncMaid](https://github.com/dantewang/SyncMaid), rebuilt on
[GPUI](https://www.gpui.rs/) with the same features, the same safety rules and the same window.
Two differences are deliberate:

- **Portable only.** Everything SyncMaid saves lives in a `Data` folder beside the executable,
  so the whole app is a folder you can copy to a USB stick and the machine keeps nothing. The
  C# build could also store its config in `%APPDATA%`; this one does not offer the choice.
- **No acrylic window background.** SyncMaid wears `gpui-component`'s Ayu Light theme, flat
  throughout, with the system title bar rather than a drawn one.

Config is **byte-compatible** with the C# build, so the two can share a `Data` folder: point
this one at an existing `tasks.json` and it loads, saves and reads back identically.

## Documentation

- **Using SyncMaid** — the guides in [`docs/`](docs).
- **Design records and rationale** — the [wiki](https://github.com/dantewang/SyncMaid/wiki) of
  the original, which this port follows.
- **Contributing / working with agents** — [AGENT.md](AGENT.md) holds the standing rules.

## Requirements

- Windows 10 or later (the app is Windows-only)
- The Rust toolchain pinned in [`rust-toolchain.toml`](rust-toolchain.toml), which `rustup`
  installs on the first build
- The MSVC toolchain (the `x86_64-pc-windows-msvc` target's linker)

## Build & run

```powershell
cargo run -p syncmaid --bin SyncMaid
```

A release build:

```powershell
cargo build --release
```

`SyncMaid-rs.exe --show <dialog>` opens straight into one editor or confirmation — `task`,
`task-edit`, `workspace`, `routing`, `destination`, `destination-edit`, `settings`, `log`,
`confirm` or `mirror-delete`. A development affordance: several of them sit three clicks deep.

## Tests

```powershell
cargo test --workspace
```

Just the engine, which is where the rules live and where the tests are fastest:

```powershell
cargo test -p syncmaid-core
```

The engine's tests run against an in-memory filesystem with fault injection — failed writes,
corrupted writes, locked files, failed enumerations, drifting timestamps — alongside
integration tests that repeat the same scenarios against a real temporary directory, so the two
filesystems cannot quietly disagree.

## Publish a user-ready build

```powershell
cargo build --release --target x86_64-pc-windows-msvc -p syncmaid --bin SyncMaid-rs
```

The executable lands in `target/x86_64-pc-windows-msvc/release/SyncMaid-rs.exe` and **is the whole
app**: the icon, the string tables and the C runtime are all linked into it, so every DLL it
imports ships with Windows itself. Nothing to install, nothing to copy alongside it — hand
someone that one file and it runs.

### Cutting a release

`Cargo.toml` is the only place a version is written. `[workspace.package] version` is what the
Settings screen shows, what Explorer reads off the executable, and what the release is named
after — the tag records which commit declared it, never the other way round.

```powershell
# Bump [workspace.package] version in Cargo.toml, then:
cargo check                     # refresh Cargo.lock with the new version
git commit -am "chore: release 1.2.3"
git tag v1.2.3
git push origin master --tags
```

The tag runs the release workflow, which refuses to build unless the tag and the manifest
agree, then packages exactly the executable above, publishes a SHA-256 sidecar beside it, and
attests the build's provenance.

`cargo install cargo-release` collapses that block into `cargo release 1.2.3` if the manual
steps start to grate.


