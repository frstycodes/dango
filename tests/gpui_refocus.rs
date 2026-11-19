// App keys (⌘K, ⌘F) sit on the root's key listener, which only hears keys while focus is inside it. Esc
// closing a focused popup leaves nothing focused; main.rs takes focus back on focus lost. This is that
// shape: without the refocus the second ⌘K goes nowhere.
use gpui_kit::prelude::*;
use gpui_kit::{Context, FocusHandle, KeyDownEvent, TestAppContext, Window, div};

struct Root {
    focus: FocusHandle,
    popup: Option<FocusHandle>,
    opened: usize,
}

impl Root {
    fn new(refocus: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        if refocus {
            cx.on_focus_lost(window, |this, window, cx| this.focus.focus(window, cx)).detach();
        }
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        Self { focus, popup: None, opened: 0 }
    }

    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "k" && event.keystroke.modifiers.platform {
            let popup = cx.focus_handle();
            popup.focus(window, cx);
            self.popup = Some(popup);
            self.opened += 1;
        }
        if event.keystroke.key == "escape" {
            self.popup = None;
        }
        cx.notify();
    }
}

impl Render for Root {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().track_focus(&self.focus).size_full().on_key_down(cx.listener(Self::on_key)).children(self.popup.as_ref().map(|popup| div().track_focus(popup).size_full()))
    }
}

fn opens_after_escape(refocus: bool, cx: &mut TestAppContext) -> usize {
    let (root, cx) = cx.add_window_view(|window, cx| Root::new(refocus, window, cx));
    cx.simulate_keystrokes("cmd-k escape cmd-k");
    root.read_with(cx, |root, _| root.opened)
}

#[gpui_kit::test]
fn escape_from_a_popup_keeps_app_keys_working(cx: &mut TestAppContext) {
    assert_eq!(opens_after_escape(true, cx), 2);
}

#[gpui_kit::test]
fn without_refocus_the_second_key_is_lost(cx: &mut TestAppContext) {
    assert_eq!(opens_after_escape(false, cx), 1, "the bug this guards against");
}
