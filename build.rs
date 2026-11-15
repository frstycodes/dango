fn main() {
    match std::env::var("CARGO_CFG_TARGET_OS").as_deref() {
        // ScreenCaptureKit (the `screencapturekit` crate, for Go Live) links Swift's runtime libraries, which
        // macOS ships in /usr/lib/swift; without this rpath the binary fails to launch with "Library not loaded".
        Ok("macos") => println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift"),
        // The exe's icon: Explorer shows it, and gpui loads it for the window and taskbar.
        Ok("windows") => {
            println!("cargo:rerun-if-changed=assets/windows/app.ico");
            embed_resource::compile("assets/windows/app.rc", embed_resource::NONE).manifest_optional().expect("embed the app icon");
        }
        _ => {}
    }
}
