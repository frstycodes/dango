// Window material: Mica behind the sidebar on Windows; a solid sidebar everywhere else.
use gpui_kit::WindowBackgroundAppearance;

pub fn window_background() -> WindowBackgroundAppearance {
    if cfg!(target_os = "windows") {
        return WindowBackgroundAppearance::MicaBackdrop;
    }
    WindowBackgroundAppearance::Opaque
}

// Whether GPUI leaves the sidebar unpainted so the native material shows through.
pub const NATIVE_SIDEBAR: bool = cfg!(target_os = "windows");

#[cfg(not(target_os = "macos"))]
pub fn system_reduce_motion() -> bool {
    false
}

#[cfg(target_os = "macos")]
pub fn system_reduce_motion() -> bool {
    use objc2::{
        msg_send,
        runtime::{AnyClass, AnyObject},
    };
    // SAFETY: a read-only NSWorkspace property.
    unsafe {
        let class = AnyClass::get(c"NSWorkspace").expect("NSWorkspace");
        let workspace: *mut AnyObject = msg_send![class, sharedWorkspace];
        msg_send![workspace, accessibilityDisplayShouldReduceMotion]
    }
}

// Rustcord's own icon (a Rust crab on a blurple chat bubble), set at runtime so `cargo run` shows it in the
// Dock too. NSImage reads SVG on macOS 14 and later; older systems keep the default icon.
#[cfg(target_os = "macos")]
pub fn set_app_icon() {
    use objc2::{
        msg_send,
        runtime::{AnyClass, AnyObject},
    };
    const ICON: &[u8] = include_bytes!("../assets/brand/chord-icon.svg");
    // SAFETY: plain AppKit messages; a nil image (unsupported SVG) is checked before use.
    unsafe {
        let (Some(data_class), Some(image_class), Some(app_class)) = (AnyClass::get(c"NSData"), AnyClass::get(c"NSImage"), AnyClass::get(c"NSApplication")) else {
            return;
        };
        let data: *mut AnyObject = msg_send![data_class, dataWithBytes: ICON.as_ptr() as *const std::ffi::c_void, length: ICON.len()];
        let image: *mut AnyObject = msg_send![image_class, alloc];
        let image: *mut AnyObject = msg_send![image, initWithData: data];
        if image.is_null() {
            return;
        }
        let app: *mut AnyObject = msg_send![app_class, sharedApplication];
        let _: () = msg_send![app, setApplicationIconImage: image];
    }
}

#[cfg(not(target_os = "macos"))]
pub fn set_app_icon() {}
