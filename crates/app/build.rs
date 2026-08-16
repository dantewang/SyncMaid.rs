fn main() {
    #[cfg(windows)]
    {
        println!("cargo:rerun-if-changed=assets/syncmaid.ico");

        // Embeds the multi-size icon as Win32 resource 1. It is both the executable's icon
        // (Explorer, taskbar, Alt-Tab) and what the tray loads, so the two can never drift.
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon_with_id("assets/syncmaid.ico", "1");
        resource.set("FileDescription", "SyncMaid");
        resource.set("ProductName", "SyncMaid");
        if let Err(error) = resource.compile() {
            println!("cargo:warning=could not embed the Windows resources: {error}");
        }
    }
}
