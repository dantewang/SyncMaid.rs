//! Build-time work: the Windows resources, the version stamp, and the string tables.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

#[path = "build/strings.rs"]
mod strings;

fn main() {
    let version = released_version();
    println!("cargo:rustc-env=SYNCMAID_VERSION={version}");

    strings::generate();

    #[cfg(windows)]
    {
        println!("cargo:rerun-if-changed=assets/syncmaid.ico");

        // Embeds the multi-size icon as Win32 resource 1. It is both the executable's icon
        // (Explorer, taskbar, Alt-Tab) and what the tray loads, so the two can never drift.
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon_with_id("assets/syncmaid.ico", "1");
        resource.set("FileDescription", "SyncMaid");
        resource.set("ProductName", "SyncMaid");
        resource.set("FileVersion", &version);
        resource.set("ProductVersion", &version);
        if let Err(error) = resource.compile() {
            println!("cargo:warning=could not embed the Windows resources: {error}");
        }
    }
}

/// The version to show and to stamp on the executable.
///
/// The git tag is the single source of truth, the way MinVer was for the original — so a
/// release is made by tagging and nothing has to be edited to match. A build from a tarball, a
/// shallow clone or a working tree with no tags falls back to `Cargo.toml`, which is never
/// wrong, only less specific.
fn released_version() -> String {
    // The tag is what changes the version, so a new one has to re-run this.
    println!("cargo:rerun-if-changed=../../.git/refs/tags");
    println!("cargo:rerun-if-env-changed=SYNCMAID_VERSION");

    if let Ok(forced) = std::env::var("SYNCMAID_VERSION") {
        return forced;
    }

    let described = Command::new("git")
        .args(["describe", "--tags", "--dirty", "--match", "v[0-9]*"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok());

    match described {
        Some(text) => text.trim().trim_start_matches('v').to_owned(),
        None => std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into()),
    }
}

/// Reads one `lang/*.json` as an ordered key/value table.
fn read_table(path: &Path) -> BTreeMap<String, String> {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|error| panic!("parse {}: {error}", path.display()))
}
