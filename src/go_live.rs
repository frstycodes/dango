// Go Live's memory: the last source we shared, found again next time even when its window is gone,
// and what the Toggle Screen Share keybind does with it.
use gpui_kit::{AppContext, Context};
use serde::{Deserialize, Serialize};

use crate::{
    Rustcord,
    voice::{self, ShareSource, ShareableScreen, ShareableWindow},
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum LastShare {
    Screen { id: u32 },
    // A window id dies with its window and can come back as another app's after a restart, so the
    // app is kept too, to check the id against and to find a sibling window by.
    Window { id: u32, pid: i32, app_name: String },
}

impl LastShare {
    pub fn of(source: ShareSource, windows: &[ShareableWindow]) -> Option<Self> {
        match source {
            ShareSource::Screen(id) => Some(Self::Screen { id }),
            ShareSource::Window(id) => windows.iter().find(|w| w.id == id).map(|w| Self::Window { id, pid: w.pid, app_name: w.app_name.clone() }),
            ShareSource::App(_) => None,
        }
    }

    // What's still on offer: the same screen or window, else another window of the same process, else
    // of the same app. None when the app has no windows left or the screen is unplugged.
    pub fn find(&self, windows: &[ShareableWindow], screens: &[ShareableScreen]) -> Option<(ShareSource, String)> {
        match self {
            Self::Screen { id } => screens.iter().find(|s| s.id == *id).map(screen_choice),
            Self::Window { id, pid, app_name } => {
                let same_app = |w: &&ShareableWindow| &w.app_name == app_name;
                windows
                    .iter()
                    .filter(same_app)
                    .find(|w| w.id == *id)
                    .or_else(|| windows.iter().filter(same_app).find(|w| w.pid == *pid))
                    .or_else(|| windows.iter().find(same_app))
                    .map(window_choice)
            }
        }
    }
}

// The picker's opening selection: the remembered source as `find` resolves it, else Screen 1.
pub fn preselect(last: Option<&LastShare>, windows: &[ShareableWindow], screens: &[ShareableScreen]) -> Option<(ShareSource, String)> {
    last.and_then(|l| l.find(windows, screens)).or_else(|| screens.first().map(screen_choice))
}

fn screen_choice(screen: &ShareableScreen) -> (ShareSource, String) {
    (ShareSource::Screen(screen.id), screen.name.clone())
}

fn window_choice(window: &ShareableWindow) -> (ShareSource, String) {
    (ShareSource::Window(window.id), window.title.clone())
}

#[derive(Debug, PartialEq)]
pub enum KeybindMove {
    Nothing,
    Stop,
    // Go live again with the remembered source, if it can still be found.
    Resume,
    Pick,
}

pub fn keybind_move(in_call: bool, live: bool, remembered: bool) -> KeybindMove {
    if !in_call {
        return KeybindMove::Nothing;
    }
    match (live, remembered) {
        (true, _) => KeybindMove::Stop,
        (false, true) => KeybindMove::Resume,
        (false, false) => KeybindMove::Pick,
    }
}

impl Rustcord {
    pub(crate) fn toggle_share_keybind(&mut self, cx: &mut Context<Self>) {
        let in_call = self.call.as_ref().is_some_and(|c| c.connected);
        let live = self.call.as_ref().is_some_and(|c| c.sharing.is_some());
        match keybind_move(in_call, live, self.prefs.last_share.is_some()) {
            KeybindMove::Nothing => {}
            KeybindMove::Stop => self.toggle_screen_share(cx),
            KeybindMove::Resume => self.resume_share(cx),
            KeybindMove::Pick => self.open_share_picker(cx),
        }
    }

    // Listing windows takes tens of milliseconds, so it runs off the UI thread; the call may have
    // changed by the time it's back.
    fn resume_share(&mut self, cx: &mut Context<Self>) {
        let Some(last) = self.prefs.last_share.clone() else { return };
        let listing = cx.background_spawn(async { (voice::shareable_windows().unwrap_or_default(), voice::screens().unwrap_or_default()) });
        cx.spawn(async move |this, cx| {
            let (windows, screens) = listing.await;
            this.update(cx, |this, cx| {
                let ready = this.call.as_ref().is_some_and(|c| c.connected && c.sharing.is_none());
                if !ready {
                    return;
                }
                match last.find(&windows, &screens) {
                    Some((source, label)) => this.share_and_remember(source, label, &windows, cx),
                    None => this.open_share_picker(cx),
                }
            })
            .ok();
        })
        .detach();
    }

    // Goes live through `start_share`, then remembers the source once the stream is really up.
    pub(crate) fn share_and_remember(&mut self, source: ShareSource, label: String, windows: &[ShareableWindow], cx: &mut Context<Self>) {
        let last = LastShare::of(source, windows);
        self.share_choice_label = Some(label);
        self.start_share(source, cx);
        let live = self.call.as_ref().is_some_and(|c| c.sharing.is_some());
        if live && last.is_some() && self.prefs.last_share != last {
            self.prefs.last_share = last;
            self.prefs.save();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(id: u32, pid: i32, app: &str) -> ShareableWindow {
        ShareableWindow { id, title: format!("{app} {id}"), app_name: app.into(), pid }
    }

    fn screens() -> Vec<ShareableScreen> {
        vec![ShareableScreen { id: 7, name: "Screen 1".into(), width: 0, height: 0 }, ShareableScreen { id: 9, name: "Screen 2".into(), width: 0, height: 0 }]
    }

    #[test]
    fn remembered_window_falls_back_to_its_app() {
        let last = LastShare::Window { id: 10, pid: 100, app_name: "Figma".into() };
        let pick = |windows: &[ShareableWindow]| last.find(windows, &screens()).map(|(source, _)| source);
        assert_eq!(pick(&[window(11, 100, "Figma"), window(10, 100, "Figma")]), Some(ShareSource::Window(10)), "the same window first");
        assert_eq!(pick(&[window(20, 200, "Figma"), window(12, 100, "Figma")]), Some(ShareSource::Window(12)), "then the same process");
        assert_eq!(pick(&[window(1, 300, "Safari"), window(21, 200, "Figma")]), Some(ShareSource::Window(21)), "then the same app");
        assert_eq!(pick(&[window(10, 300, "Safari")]), None, "a reused id from another app isn't ours");
        assert_eq!(pick(&[]), None);
    }

    #[test]
    fn picker_preselects_the_remembered_source_or_screen_one() {
        let windows = [window(10, 100, "Figma")];
        let screen = LastShare::Screen { id: 9 };
        assert_eq!(preselect(Some(&screen), &windows, &screens()), Some((ShareSource::Screen(9), "Screen 2".into())));
        let unplugged = LastShare::Screen { id: 4 };
        assert_eq!(preselect(Some(&unplugged), &windows, &screens()), Some((ShareSource::Screen(7), "Screen 1".into())));
        let closed = LastShare::Window { id: 30, pid: 300, app_name: "Notes".into() };
        assert_eq!(preselect(Some(&closed), &windows, &screens()).map(|(s, _)| s), Some(ShareSource::Screen(7)));
        assert_eq!(preselect(None, &windows, &screens()).map(|(s, _)| s), Some(ShareSource::Screen(7)));
        assert_eq!(preselect(None, &windows, &[]), None);
        assert_eq!(LastShare::of(ShareSource::Window(10), &windows), Some(LastShare::Window { id: 10, pid: 100, app_name: "Figma".into() }));
    }

    #[test]
    fn keybind_stops_resumes_or_asks() {
        assert_eq!(keybind_move(false, false, true), KeybindMove::Nothing);
        assert_eq!(keybind_move(false, true, true), KeybindMove::Nothing);
        assert_eq!(keybind_move(true, true, false), KeybindMove::Stop);
        assert_eq!(keybind_move(true, false, true), KeybindMove::Resume);
        assert_eq!(keybind_move(true, false, false), KeybindMove::Pick);
    }
}
