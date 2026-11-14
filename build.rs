// ScreenCaptureKit (the `screencapturekit` crate, for Go Live) links Swift's runtime libraries, which
// macOS ships in /usr/lib/swift; without this rpath the binary fails to launch with "Library not loaded".
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    }
}
