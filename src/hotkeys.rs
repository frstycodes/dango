// Discord's Keybinds: system-wide shortcuts that work while another app is in front.
// They are Carbon hotkeys on macOS, which take the key from every other app while bound, so a bare
// key such as ` stops typing elsewhere; Discord listens with an event tap instead.
// ponytail: an event tap (needs Input Monitoring permission) if a bare push-to-talk key bites.
use futures::channel::mpsc;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState, hotkey::HotKey};
use gpui_kit::Keystroke;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Action {
    PushToTalk,
    PushToMute,
    ToggleMute,
    ToggleDeafen,
    ToggleVoiceMode,
    Disconnect,
    ToggleScreenShare,
    Unassigned,
}

// Toggle Screen Share's default: ⌘⇧L on macOS, Ctrl+Shift+L elsewhere.
pub const SHARE_KEYS: &str = if cfg!(target_os = "macos") { "shift+cmd+l" } else { "ctrl+shift+l" };

impl Action {
    pub const ALL: [Action; 8] = [
        Action::Unassigned,
        Action::PushToTalk,
        Action::PushToMute,
        Action::ToggleVoiceMode,
        Action::ToggleMute,
        Action::ToggleDeafen,
        Action::Disconnect,
        Action::ToggleScreenShare,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Action::PushToTalk => "Push to Talk (Normal)",
            Action::PushToMute => "Push to Mute",
            Action::ToggleMute => "Toggle Mute",
            Action::ToggleDeafen => "Toggle Deafen",
            Action::ToggleVoiceMode => "Toggle Voice Activity Mode",
            Action::Disconnect => "Disconnect From Voice",
            Action::ToggleScreenShare => "Toggle Screen Share",
            Action::Unassigned => "Unassigned",
        }
    }

    // Held actions act on press and release; the rest only on press.
    pub fn held(self) -> bool {
        matches!(self, Action::PushToTalk | Action::PushToMute)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Keybind {
    pub action: Action,
    // global-hotkey's own syntax, e.g. "cmd+shift+KeyM"; None until recorded.
    pub keys: Option<String>,
}

pub struct Hotkeys {
    manager: GlobalHotKeyManager,
    bound: Vec<(HotKey, Action)>,
}

impl Hotkeys {
    // Must run on the main thread (AppKit). Events arrive as (hotkey id, pressed); `action` maps the id.
    pub fn new() -> Option<(Self, mpsc::UnboundedReceiver<(u32, bool)>)> {
        let manager = GlobalHotKeyManager::new().ok()?;
        let (tx, rx) = mpsc::unbounded();
        GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
            let _ = tx.unbounded_send((event.id(), event.state() == HotKeyState::Pressed));
        }));
        Some((Self { manager, bound: Vec::new() }, rx))
    }

    // Replaces every binding; returns the keybinds the system refused (taken by another app).
    pub fn bind(&mut self, keybinds: &[Keybind]) -> Vec<String> {
        let old: Vec<HotKey> = self.bound.drain(..).map(|(hotkey, _)| hotkey).collect();
        let _ = self.manager.unregister_all(&old);
        let mut refused = Vec::new();
        for keybind in keybinds.iter().filter(|k| k.action != Action::Unassigned) {
            let Some(hotkey) = keybind.keys.as_deref().and_then(|k| k.parse::<HotKey>().ok()) else { continue };
            if self.bound.iter().any(|(h, _)| h.id() == hotkey.id()) {
                continue;
            }
            match self.manager.register(hotkey) {
                Ok(()) => self.bound.push((hotkey, keybind.action)),
                Err(_) => refused.extend(keybind.keys.clone()),
            }
        }
        refused
    }

    pub fn action(&self, id: u32) -> Option<Action> {
        self.bound.iter().find(|(hotkey, _)| hotkey.id() == id).map(|(_, action)| *action)
    }
}

// A recorded gpui keystroke in global-hotkey's syntax, or None for a key it can't bind.
pub fn from_keystroke(keystroke: &Keystroke) -> Option<String> {
    let m = &keystroke.modifiers;
    let key = match keystroke.key.as_str() {
        "up" | "down" | "left" | "right" => format!("Arrow{}{}", keystroke.key[..1].to_uppercase(), &keystroke.key[1..]),
        "pageup" => "PageUp".into(),
        "pagedown" => "PageDown".into(),
        key => key.to_owned(),
    };
    let mods = [(m.control, "ctrl"), (m.alt, "alt"), (m.shift, "shift"), (m.platform, "cmd")];
    let text: Vec<&str> = mods.iter().filter(|(on, _)| *on).map(|(_, name)| *name).chain([key.as_str()]).collect();
    let text = text.join("+");
    text.parse::<HotKey>().is_ok().then_some(text)
}

// How Discord prints a keybind: upper-case keys joined by " + ".
pub fn display(keys: &str) -> String {
    keys.split('+')
        .map(|part| match part {
            // The platform key and Alt go by their own names on each system, as Discord shows them.
            "cmd" if cfg!(target_os = "macos") => "CMD".to_owned(),
            "cmd" => "WIN".to_owned(),
            "alt" if cfg!(target_os = "macos") => "OPTION".to_owned(),
            "alt" => "ALT".to_owned(),
            "ctrl" => "CTRL".to_owned(),
            p => p.trim_start_matches("Arrow").to_uppercase(),
        })
        .collect::<Vec<_>>()
        .join(" + ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keystrokes_round_trip() {
        let k = |s: &str| from_keystroke(&Keystroke::parse(s).unwrap());
        assert_eq!(k("cmd-shift-m").as_deref(), Some("shift+cmd+m"));
        assert_eq!(k("`").as_deref(), Some("`"));
        assert_eq!(k("alt-up").as_deref(), Some("alt+ArrowUp"));
        let (cmd, alt) = if cfg!(target_os = "macos") { ("CMD", "OPTION") } else { ("WIN", "ALT") };
        assert_eq!(display("shift+cmd+m"), format!("SHIFT + {cmd} + M"));
        assert_eq!(display("alt+ArrowUp"), format!("{alt} + UP"));
        let recorded = k(if cfg!(target_os = "macos") { "cmd-shift-l" } else { "ctrl-shift-l" });
        assert_eq!(recorded.as_deref(), Some(SHARE_KEYS), "the default reads as if it had been recorded");
    }
}
