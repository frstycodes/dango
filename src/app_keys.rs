// App-wide shortcuts: ⌘R / ⌘Q on macOS, Ctrl+R / Ctrl+Q on Windows and Linux. Alt+F4 on Windows stays
// the window's close, which hides Dango to the tray like Discord (tray.rs).
use gpui_kit::{App, KeyBinding, actions};

actions!(dango, [Quit, Restart]);

pub fn init(cx: &mut App) {
    // `secondary` is ⌘ on macOS and Ctrl elsewhere.
    cx.bind_keys([KeyBinding::new("secondary-q", Quit, None), KeyBinding::new("secondary-r", Restart, None)]);
    cx.on_action(|_: &Quit, cx| cx.quit());
    // Relaunches this build with the same arguments once this process has exited.
    cx.on_action(|_: &Restart, cx| cx.restart());
}
