// App shortcuts: ⌘R / ⌘Q on macOS, Ctrl+R / Ctrl+Q on Windows and Linux; ⌘F / Ctrl+F in a channel or DM
// (the "Chat" key context, so Settings can bind its own). Alt+F4 on Windows stays
// the window's close, which hides Dango to the tray like Discord (tray.rs).
use gpui_kit::{App, KeyBinding, actions};

actions!(dango, [Quit, Restart, FocusSearch]);

pub fn init(cx: &mut App) {
    // `secondary` is ⌘ on macOS and Ctrl elsewhere.
    cx.bind_keys([KeyBinding::new("secondary-q", Quit, None), KeyBinding::new("secondary-r", Restart, None), KeyBinding::new("secondary-f", FocusSearch, Some("Chat"))]);
    cx.on_action(|_: &Quit, cx| cx.quit());
    // In a call it asks first, then leaves voice (restart.rs).
    cx.on_action(|_: &Restart, cx| crate::restart::restart_app(cx));
}
