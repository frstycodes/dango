// Dango's app icon choices: the three dumplings in many media, picked in Settings › Appearance.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppIcon {
    Brand,
    Graphite,
    Sticker,
    Peek,
    // Last because it also takes any icon no longer offered (glass, riso, …), so an old pick can't fail
    // the whole prefs file, and serde wants that catch-all at the end.
    #[default]
    #[serde(other)]
    Bone,
}

impl AppIcon {
    pub const ALL: [AppIcon; 5] = [AppIcon::Bone, AppIcon::Brand, AppIcon::Graphite, AppIcon::Sticker, AppIcon::Peek];

    pub fn label(self) -> &'static str {
        match self {
            AppIcon::Bone => "Bone",
            AppIcon::Brand => "Chili",
            AppIcon::Graphite => "Graphite",
            AppIcon::Sticker => "Sticker",
            AppIcon::Peek => "Peek",
        }
    }

    // The 512px PNG's asset path, served by `icons::Assets`.
    pub fn asset(self) -> &'static str {
        BRAND_ICONS[self as usize].0
    }

    pub fn bytes(self) -> &'static [u8] {
        BRAND_ICONS[self as usize].1
    }

    // Shows this icon in the Dock right away. On other systems the icon comes from the build.
    // GAP: on Windows the exe, window and taskbar keep the build's icon (assets/windows/app.ico); following
    // the pick there needs WM_SETICON on the window.
    pub fn apply(self) {
        #[cfg(target_os = "macos")]
        set_dock_icon(self.bytes());
        // The tray icon follows the pick; the taskbar's still comes from the build (see GAP above).
        #[cfg(target_os = "windows")]
        crate::tray::set_icon(self.bytes());
    }
}

// Order matches `AppIcon`.
pub const BRAND_ICONS: [(&str, &[u8]); 5] = [
    ("brand/icons/brand.png", include_bytes!("../assets/brand/icons/brand.png")),
    ("brand/icons/graphite.png", include_bytes!("../assets/brand/icons/graphite.png")),
    ("brand/icons/sticker.png", include_bytes!("../assets/brand/icons/sticker.png")),
    ("brand/icons/peek.png", include_bytes!("../assets/brand/icons/peek.png")),
    ("brand/icons/bone.png", include_bytes!("../assets/brand/icons/bone.png")),
];

#[cfg(target_os = "macos")]
fn set_dock_icon(png: &[u8]) {
    use objc2::{
        msg_send,
        runtime::{AnyClass, AnyObject},
    };
    // SAFETY: plain AppKit messages on the main thread; a nil image is checked before use.
    unsafe {
        let (Some(data_class), Some(image_class), Some(app_class)) = (AnyClass::get(c"NSData"), AnyClass::get(c"NSImage"), AnyClass::get(c"NSApplication")) else {
            return;
        };
        let data: *mut AnyObject = msg_send![data_class, dataWithBytes: png.as_ptr() as *const std::ffi::c_void, length: png.len()];
        let image: *mut AnyObject = msg_send![image_class, alloc];
        let image: *mut AnyObject = msg_send![image, initWithData: data];
        if image.is_null() {
            return;
        }
        let app: *mut AnyObject = msg_send![app_class, sharedApplication];
        let _: () = msg_send![app, setApplicationIconImage: image];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_has_its_png_in_order() {
        for icon in AppIcon::ALL {
            let (path, bytes) = BRAND_ICONS[icon as usize];
            assert!(path.ends_with(&format!("{}.png", serde_json::to_string(&icon).unwrap().trim_matches('"'))), "{path}");
            assert_eq!(&bytes[1..4], b"PNG", "{path} is a PNG");
        }
        let saved: AppIcon = serde_json::from_str("\"peek\"").unwrap();
        assert_eq!(saved, AppIcon::Peek);
    }

    #[test]
    fn a_retired_icon_falls_back_to_the_default() {
        let saved: AppIcon = serde_json::from_str("\"riso\"").unwrap();
        assert_eq!(saved, AppIcon::default());
    }
}
