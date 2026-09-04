//! Build-time work: the Windows resources and the string tables.

use std::collections::BTreeMap;
use std::path::Path;

#[path = "build/strings.rs"]
mod strings;

fn main() {
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
        // `FILEVERSION`, `PRODUCTVERSION` and the `FileVersion` string all default to
        // `package.version`, so Explorer and the Settings screen cannot disagree.
        if let Err(error) = resource.compile() {
            println!("cargo:warning=could not embed the Windows resources: {error}");
        }
    }
}

/// Reads one `lang/*.json` as an ordered key/value table.
fn read_table(path: &Path) -> BTreeMap<String, String> {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|error| panic!("parse {}: {error}", path.display()))
}
