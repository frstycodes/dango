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

