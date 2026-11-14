// Settings, ported from design/settings.html: the whole window, the nav on the sidebar tone, one page
// of titled groups on the right. Only settings Wave honours are shown.
use std::{collections::HashMap, time::Duration};

use gpui_kit::{
    Animation, AnimationExt, AnyElement, App, BoxShadow, Context, Div, ElementId, FontWeight, Hsla, MouseButton, Pixels, Radians, ScrollHandle, SharedString, Stateful, Window, canvas,
    component::{Icon, input::Input},
    div, img, point, prelude::*, px, relative, rgb,
};
use serde::{Deserialize, Serialize};

use crate::{
    Rustcord,
    hotkeys::{self, Action, Keybind},
    icons::Glyph,
    layout::TITLE_HEIGHT,
    palette::{self, Level},
    sounds::{self, Cue},
    motion,
    ui::{self, Squircle, Tone},
    voice,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Account,
    Notifications,
    VoiceVideo,
    Keybinds,
    Appearance,
}

impl Page {
    fn label(self) -> &'static str {
        match self {
            Page::Account => "Account",
            Page::Notifications => "Notifications",
            Page::VoiceVideo => "Voice and video",
            Page::Keybinds => "Keybinds",
            Page::Appearance => "Appearance",
        }
    }

    fn glyph(self) -> Glyph {
        match self {
            Page::Account => Glyph::UserCircle,
            Page::Notifications => Glyph::Bell,
            Page::VoiceVideo => Glyph::Mic,
            Page::Keybinds => Glyph::Keyboard,
            Page::Appearance => Glyph::Palette,
        }
    }

    // In-page anchors listed under the open page.
    fn sections(self) -> &'static [&'static str] {
        match self {
            Page::VoiceVideo => &["Voice", "Camera", "Sounds", "Advanced"],
            _ => &[],
        }
    }
}

const NAV: [(Option<&str>, &[Page]); 2] =
    [(None, &[Page::Account, Page::Notifications]), (Some("Experience"), &[Page::VoiceVideo, Page::Keybinds, Page::Appearance])];

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum ThemePref {
    #[default]
    System,
    Dark,
    Light,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum InputProfile {
    VoiceIsolation,
    Studio,
    #[default]
    Custom,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum NoiseMode {
    #[default]
    Advanced,
    Standard,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub theme: ThemePref,
    pub app_icon: crate::app_icon::AppIcon,
    pub input_device: Option<String>,
    pub output_device: Option<String>,
    pub camera: Option<String>,
    // 0..=2, 1 is 100%.
    pub input_volume: f32,
    pub output_volume: f32,
    pub input_profile: InputProfile,
    pub sensitivity_auto: bool,
    // -100..=0 dB.
    pub sensitivity_db: f32,
    // Discord's "Advanced Voice Activity": a neural speech detector drives automatic sensitivity.
    pub advanced_voice_activity: bool,
    pub noise_suppression: bool,
    // Which suppressor while it's on: DeepFilterNet (Discord's Krisp slot) or WebRTC's.
    pub noise_mode: NoiseMode,
    pub echo_cancellation: bool,
    pub auto_gain: bool,
    // Global Attenuation: percent other apps are lowered (0 = off), and when.
    pub attenuation: u8,
    pub attenuate_self: bool,
    pub attenuate_others: bool,
    pub silence_warning: bool,
    pub qos: bool,
    pub push_to_talk: bool,
    pub ptt_release_ms: u32,
    pub silenced_cues: Vec<Cue>,
    pub keybinds: Vec<Keybind>,
    // Per-user listening volume (0..=2) and local mutes, kept across calls as Discord does.
    pub user_volumes: HashMap<String, f32>,
    pub local_mutes: Vec<String>,
    // Discord's per-user "Mute Soundboard" and "Disable Video", kept locally like the volume.
    pub soundboard_mutes: Vec<String>,
    pub hidden_video: Vec<String>,
    // How often we've reacted with each emoji, for the "frequently used" rows.
    pub emoji_uses: Vec<(crate::discord::Emoji, u32)>,
    // Go Live quality: height (0 = source), frame rate, and whether the source's sound goes too.
    pub stream_resolution: u32,
    pub stream_fps: u32,
    pub stream_audio: bool,
    // What we last went live with: preselected in the picker, reused by the Toggle Screen Share keybind.
    pub last_share: Option<crate::go_live::LastShare>,
    // Generations of default keybinds this file has been given. A file from before the field reads 0
    // (the field's own default, not `Prefs::default`'s), so it gets newer defaults once; deleting one
    // afterwards sticks.
    #[serde(default)]
    pub keybinds_version: u32,
}

const KEYBINDS_VERSION: u32 = 1;
const STUDIO_THRESHOLD_DB: f32 = -84.;

impl Default for Prefs {
    fn default() -> Self {
        Self {
            theme: ThemePref::System,
            app_icon: crate::app_icon::AppIcon::default(),
            input_device: None,
            output_device: None,
            camera: None,
            input_volume: 1.,
            output_volume: 1.,
            input_profile: InputProfile::Custom,
            sensitivity_auto: true,
            sensitivity_db: -60.,
            advanced_voice_activity: true,
            noise_suppression: true,
            noise_mode: NoiseMode::Advanced,
            echo_cancellation: true,
            auto_gain: true,
            attenuation: 0,
            attenuate_self: false,
            attenuate_others: true,
            silence_warning: true,
            qos: false,
            push_to_talk: false,
            ptt_release_ms: 20,
            silenced_cues: Vec::new(),
            keybinds: vec![Keybind { action: Action::PushToTalk, keys: Some("`".into()) }, share_keybind()],
            user_volumes: HashMap::new(),
            local_mutes: Vec::new(),
            soundboard_mutes: Vec::new(),
            hidden_video: Vec::new(),
            emoji_uses: Vec::new(),
            stream_resolution: 720,
            stream_fps: 30,
            stream_audio: true,
            last_share: None,
            keybinds_version: KEYBINDS_VERSION,
        }
    }
}

fn share_keybind() -> Keybind {
    Keybind { action: Action::ToggleScreenShare, keys: Some(hotkeys::SHARE_KEYS.into()) }
}

impl Prefs {
    pub fn load() -> Self {
        let mut prefs: Self = prefs_file().and_then(|path| std::fs::read_to_string(path).ok()).and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
        prefs.add_new_keybinds();
        // 480p left the picker; the nearest offered quality takes its place.
        if prefs.stream_resolution == 480 {
            prefs.stream_resolution = 720;
        }
        prefs
    }

    fn add_new_keybinds(&mut self) {
        if self.keybinds_version >= KEYBINDS_VERSION {
            return;
        }
        if !self.keybinds.iter().any(|k| k.action == Action::ToggleScreenShare) {
            self.keybinds.push(share_keybind());
        }
        self.keybinds_version = KEYBINDS_VERSION;
    }

    pub(crate) fn save(&self) {
        let Some(path) = prefs_file() else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, text);
        }
    }

    // Discord's input profiles overlay the stored switches (MediaEngineStore `e7`) rather than
    // overwrite them, so going back to Custom restores what was there.
    fn set_profile(&mut self, profile: InputProfile) {
        self.input_profile = profile;
    }
}

impl Prefs {
    pub fn voice_settings(&self) -> voice::VoiceSettings {
        voice::VoiceSettings {
            input_device: self.input_device.clone(),
            output_device: self.output_device.clone(),
            input_volume: self.input_volume,
            output_volume: self.output_volume,
            input_mode: self.input_mode(),
            echo_cancellation: self.echo_cancellation(),
            noise_suppression: self.noise(),
            auto_gain: self.auto_gain(),
            attenuation: self.attenuation(),
            qos: self.qos,
        }
    }

    // Voice Isolation: Krisp-tier suppression, echo cancellation, AGC, automatic sensitivity with
    // Advanced Voice Activity. Studio: everything off, voice activity at a fixed -84 dB.
    fn noise(&self) -> voice::NoiseSuppression {
        match (self.input_profile, self.noise_suppression, self.noise_mode) {
            (InputProfile::VoiceIsolation, ..) => voice::NoiseSuppression::Advanced,
            (InputProfile::Studio, ..) | (InputProfile::Custom, false, _) => voice::NoiseSuppression::Off,
            (InputProfile::Custom, true, NoiseMode::Advanced) => voice::NoiseSuppression::Advanced,
            (InputProfile::Custom, true, NoiseMode::Standard) => voice::NoiseSuppression::Standard,
        }
    }

    fn echo_cancellation(&self) -> bool {
        self.profile_switch(self.echo_cancellation)
    }

    fn auto_gain(&self) -> bool {
        self.profile_switch(self.auto_gain)
    }

    fn profile_switch(&self, custom: bool) -> bool {
        match self.input_profile {
            InputProfile::VoiceIsolation => true,
            InputProfile::Studio => false,
            InputProfile::Custom => custom,
        }
    }

    fn input_mode(&self) -> voice::InputMode {
        let ptt = voice::InputMode::PushToTalk { release_delay_ms: self.ptt_release_ms };
        match self.input_profile {
            InputProfile::Studio => voice::InputMode::VoiceActivity { auto: false, threshold_db: STUDIO_THRESHOLD_DB, advanced: false },
            _ if self.push_to_talk => ptt,
            InputProfile::VoiceIsolation => voice::InputMode::VoiceActivity { auto: true, threshold_db: self.sensitivity_db, advanced: true },
            InputProfile::Custom => {
                voice::InputMode::VoiceActivity { auto: self.sensitivity_auto, threshold_db: self.sensitivity_db, advanced: self.advanced_voice_activity }
            }
        }
    }

    fn attenuation(&self) -> voice::Attenuation {
        voice::Attenuation { strength: self.attenuation.min(100), when_self_speaks: self.attenuate_self, when_others_speak: self.attenuate_others }
    }

    pub fn apply_to(&self, session: &voice::VoiceSession) {
        session.set_input_device(self.input_device.clone());
        session.set_output_device(self.output_device.clone());
        session.set_input_volume(self.input_volume);
        session.set_output_volume(self.output_volume);
        session.set_input_mode(self.input_mode());
        session.set_echo_cancellation(self.echo_cancellation());
        session.set_noise_suppression(self.noise());
        session.set_auto_gain(self.auto_gain());
        session.set_attenuation(self.attenuation());
    }
}

impl Prefs {
    // Most-used first, topped up with Discord's own defaults when we haven't reacted enough yet.
    // ponytail: counts only; Discord's frecency (time-decayed, synced in settings-proto) is the upgrade.
    pub fn frequent_emojis(&self, n: usize, pad: &[&str]) -> Vec<crate::discord::Emoji> {
        let mut uses = self.emoji_uses.clone();
        uses.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
        let mut out: Vec<crate::discord::Emoji> = uses.into_iter().map(|(e, _)| e).take(n).collect();
        for name in pad {
            if out.len() >= n {
                break;
            }
            let emoji = crate::discord::Emoji::unicode(name);
            if !out.contains(&emoji) {
                out.push(emoji);
            }
        }
        out
    }

    pub fn record_emoji(&mut self, emoji: &crate::discord::Emoji) {
        match self.emoji_uses.iter_mut().find(|(e, _)| e == emoji) {
            Some((_, count)) => *count += 1,
            None => self.emoji_uses.push((emoji.clone(), 1)),
        }
    }
}

fn prefs_file() -> Option<std::path::PathBuf> {
    Some(crate::data_dir()?.join("settings.json"))
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Slider {
    InputVolume,
    OutputVolume,
    Sensitivity,
    ReleaseDelay,
    Attenuation,
}

impl Slider {
    fn fraction(self, prefs: &Prefs) -> f32 {
        match self {
            Slider::InputVolume => prefs.input_volume / 2.,
            Slider::OutputVolume => prefs.output_volume / 2.,
            Slider::Sensitivity => (prefs.sensitivity_db + 100.) / 100.,
            Slider::ReleaseDelay => prefs.ptt_release_ms as f32 / 2000.,
            Slider::Attenuation => f32::from(prefs.attenuation) / 100.,
        }
    }

    fn set(self, prefs: &mut Prefs, fraction: f32) {
        let f = fraction.clamp(0., 1.);
        match self {
            Slider::InputVolume => prefs.input_volume = f * 2.,
            Slider::OutputVolume => prefs.output_volume = f * 2.,
            Slider::Sensitivity => prefs.sensitivity_db = (f * 100. - 100.).round(),
            Slider::ReleaseDelay => prefs.ptt_release_ms = (f * 2000.).round() as u32,
            Slider::Attenuation => prefs.attenuation = (f * 100.).round() as u8,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Select {
    Microphone,
    Speaker,
    Camera,
    Action(usize),
}

// The live drag payload for a slider; drawn as nothing.
#[derive(Clone)]
struct SliderDrag(SharedString);

impl Render for SliderDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

pub struct SettingsView {
    page: Page,
    section: usize,
    select: Option<Select>,
    recording: Option<usize>,
    more_sounds: bool,
    advanced: bool,
    // Log out and Reset ask in place before acting.
    asking: Option<Ask>,
    // What Reset replaced, kept while its Undo shows.
    reset_undo: Option<Prefs>,
    copied: bool,
    scroll: ScrollHandle,
    devices: Devices,
    // From the running mic test: whether the voice-activity gate is open, and Discord's
    // "isn't detecting any input" state.
    mic_speaking: bool,
    mic_input: Option<bool>,
}

#[derive(Default)]
struct Devices {
    inputs: Vec<(String, String)>,
    outputs: Vec<(String, String)>,
    cameras: Vec<(String, String)>,
}

impl Devices {
    fn list() -> Self {
        let pairs = |devices: Vec<voice::Device>| devices.into_iter().map(|d| (d.id, d.name)).collect();
        Self { inputs: pairs(voice::input_devices()), outputs: pairs(voice::output_devices()), cameras: pairs(voice::cameras()) }
    }
}

impl SettingsView {
    // Demo scenes open the first keybind's action list.
    #[cfg(debug_assertions)]
    pub fn open_first_select(&mut self) {
        self.select = Some(Select::Action(0));
    }

    pub fn new(page: Page) -> Self {
        Self {
            page,
            section: 0,
            select: None,
            recording: None,
            more_sounds: false,
            advanced: false,
            asking: None,
            reset_undo: None,
            copied: false,
            scroll: ScrollHandle::new(),
            devices: Devices::list(),
            mic_speaking: false,
            mic_input: None,
        }
    }
}

impl Rustcord {
    pub(crate) fn open_settings(&mut self, page: Page, window: &mut Window, cx: &mut Context<Self>) {
        self.settings = Some(SettingsView::new(page));
        self.settings_search.update(cx, |state, cx| state.set_value("", window, cx));
        cx.notify();
    }

    pub(crate) fn close_settings(&mut self, cx: &mut Context<Self>) {
        self.settings = None;
        cx.notify();
    }

    // Saves, then pushes every setting that has a live effect: sounds, hotkeys, the voice engine.
    pub(crate) fn update_prefs(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut Prefs)) {
        change(&mut self.prefs);
        self.prefs.save();
        self.apply_prefs();
        cx.notify();
    }

    pub(crate) fn apply_prefs(&mut self) {
        sounds::silence(&self.prefs.silenced_cues);
        if let Some(hotkeys) = &mut self.hotkeys {
            // A push-to-talk key is only taken from other apps while push to talk is on.
            let active: Vec<Keybind> = self.prefs.keybinds.iter().filter(|k| self.prefs.push_to_talk || k.action != Action::PushToTalk).cloned().collect();
            self.keybind_errors = hotkeys.bind(&active);
        }
        if let Some(call) = &self.call {
            self.prefs.apply_to(&call.session);
        }
    }

    pub(crate) fn start_hotkeys(&mut self, cx: &mut Context<Self>) {
        let Some((hotkeys, mut events)) = hotkeys::Hotkeys::new() else { return };
        self.hotkeys = Some(hotkeys);
        self.apply_prefs();
        cx.spawn(async move |this, cx| {
            while let Some((id, pressed)) = futures::StreamExt::next(&mut events).await {
                if this.update(cx, |this, cx| this.on_hotkey(id, pressed, cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    fn on_hotkey(&mut self, id: u32, pressed: bool, cx: &mut Context<Self>) {
        let Some(action) = self.hotkeys.as_ref().and_then(|h| h.action(id)) else { return };
        if !pressed && !action.held() {
            return;
        }
        match action {
            Action::PushToTalk if self.prefs.push_to_talk => self.set_talk_key(pressed),
            Action::PushToTalk => {}
            Action::PushToMute => {
                if let Some(call) = &self.call {
                    call.session.set_muted(pressed || self.muted || self.deafened);
                }
            }
            Action::ToggleMute => self.toggle_mute(cx),
            Action::ToggleDeafen => self.toggle_deafen(cx),
            Action::ToggleVoiceMode => self.update_prefs(cx, |p| p.push_to_talk = !p.push_to_talk),
            Action::ToggleScreenShare => self.toggle_share_keybind(cx),
            Action::Disconnect => self.leave_voice(cx),
            Action::Unassigned => {}
        }
        cx.notify();
    }

    pub(crate) fn toggle_mute(&mut self, cx: &mut Context<Self>) {
        // Unmuting while deafened undeafens too, as in Discord.
        let muted = !(self.muted || self.deafened);
        self.muted = muted;
        if !muted {
            self.deafened = false;
        }
        sounds::play(if muted { Cue::Mute } else { Cue::Unmute });
        self.sync_voice_flags();
        cx.notify();
    }

    pub(crate) fn toggle_deafen(&mut self, cx: &mut Context<Self>) {
        self.deafened = !self.deafened;
        sounds::play(if self.deafened { Cue::Deafen } else { Cue::Undeafen });
        self.sync_voice_flags();
        cx.notify();
    }

    // Discord's Mic Test: our processed mic, looped back to the speaker, metered.
    fn toggle_mic_test(&mut self, cx: &mut Context<Self>) {
        if self.mic_test.take().is_some() {
            self.mic_level = crate::SILENCE_DB;
            return cx.notify();
        }
        match voice::mic_test(None, &self.prefs.voice_settings()) {
            Ok((test, mut levels)) => {
                let task = cx.spawn(async move |this, cx| {
                    while let Some(level) = futures::StreamExt::next(&mut levels).await {
                        if this.update(cx, |this, cx| {
                            this.mic_level = level.db;
                            if let Some(view) = &mut this.settings {
                                (view.mic_speaking, view.mic_input) = (level.speaking, level.input_detected);
                            }
                            cx.notify();
                        })
                        .is_err()
                        {
                            break;
                        }
                    }
                });
                self.mic_test = Some((test, task));
            }
            Err(error) => self.voice_error = Some(error.into()),
        }
        cx.notify();
    }

    // While a keybind is recording, the next keystroke with a key in it becomes the binding.
    pub(crate) fn record_keystroke(&mut self, keystroke: &gpui_kit::Keystroke, cx: &mut Context<Self>) -> bool {
        let Some(index) = self.settings.as_ref().and_then(|s| s.recording) else { return false };
        if keystroke.key == "escape" && !keystroke.modifiers.modified() {
            if let Some(s) = &mut self.settings {
                s.recording = None;
            }
            cx.notify();
            return true;
        }
        let Some(keys) = hotkeys::from_keystroke(keystroke) else { return true };
        if let Some(settings) = &mut self.settings {
            settings.recording = None;
        }
        self.update_prefs(cx, |prefs| {
            if let Some(bind) = prefs.keybinds.get_mut(index) {
                bind.keys = Some(keys);
            }
        });
        true
    }

    // Settings take the whole window: the nav on the sidebar tone, one page on the right with a big
    // title, and a close button pinned top right (design/settings.html, .set).
    pub(crate) fn render_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let view = self.settings.as_ref()?;
        let (page, scroll) = (view.page, view.scroll.clone());
        let sections = match page {
            Page::Account => self.account_page(cx),
            Page::Notifications => self.notifications_page(window, cx),
            Page::VoiceVideo => self.voice_page(window, cx),
            Page::Keybinds => self.keybinds_page(window, cx),
            Page::Appearance => self.appearance_page(window, cx),
        };
        // The scroll list's first item is the page title, so a section's item index is one past its own.
        let anchors: Vec<usize> = page.sections().iter().map(|name| sections.iter().position(|s| s.anchor == Some(*name)).map_or(0, |i| i + 1)).collect();
        let title = (page != Page::Keybinds).then(|| page_title(page.label(), cx).flex_none());
        let body = div()
            .id("settings-scroll")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&scroll)
            .pt(px(32.))
            .px(px(40.))
            .pb(px(64.))
            .flex()
            .flex_col()
            .gap(px(32.))
            .children(title)
            .children(sections.into_iter().map(|s| div().w_full().max_w(px(PAGE_WIDTH)).flex_none().child(s.body)));
        let main = div()
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .bg(palette::surface(Level::Page, cx))
            .child(div().size_full().relative().child(body).with_animation(SharedString::from(format!("set-page-{page:?}")), Animation::new(ui::DUR_POP).with_easing(|t| motion::ease_out().sample(t)), |el, t| el.top(px(-4. * (1. - t))).opacity(t)))
            .child(self.close_button(cx));
        Some(
            div()
                .id("settings")
                .absolute()
                .inset_0()
                .occlude()
                .flex()
                .text_size(px(14.))
                .text_color(palette::text(cx))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(self.settings_nav(&anchors, window, cx))
                .child(main)
                .into_any_element(),
        )
    }

    // Pinned top right as the page scrolls: a ringed circle with "Esc" under it.
    fn close_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (muted, strong, hover) = (palette::muted(cx), palette::strong(cx), palette::hover(cx));
        // The Windows caption buttons take the window's top right corner.
        let top = if cfg!(target_os = "windows") { TITLE_HEIGHT + 8. } else { 32. };
        div()
            .absolute()
            .top(px(top))
            .right(px(24.))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(4.))
            .text_size(px(12.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(muted)
            .child(
                ui::pressable(div().id("settings-close"), 0.94)
                    .size(px(36.))
                    .rounded_full()
                    .border(px(1.5))
                    .border_color(palette::line_strong(cx))
                    .bg(palette::surface(Level::Page, cx))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .text_color(muted)
                    .hover(move |s| s.bg(hover).text_color(strong))
                    .child(Icon::new(Glyph::X).size(px(18.)))
                    .on_click(cx.listener(|this, _, _, cx| this.close_settings(cx))),
            )
            .child("Esc")
    }

    fn settings_nav(&self, anchors: &[usize], window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = self.settings.as_ref().expect("settings are open");
        let query = self.settings_search.read(cx).value().to_lowercase();
        let searching = !query.is_empty();
        let sidebar = palette::surface(Level::Sidebar, cx);
        let me = self.me.clone().map(|me| {
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .px(px(6.))
                .pb(px(12.))
                .min_w_0()
                .child(
                    div()
                        .relative()
                        .flex_none()
                        .child(ui::avatar(me.avatar.clone(), 40.))
                        .child(div().absolute().right(px(-2.)).bottom(px(-2.)).child(ui::status_mark(self.my_status, 16., sidebar, cx))),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .line_height(px(18.))
                        .child(div().truncate().font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(me.name.clone()))
                        .child(div().text_size(px(12.)).text_color(palette::muted(cx)).child(self.my_status.label())),
                )
        });
        let focused = gpui_kit::Focusable::focus_handle(self.settings_search.read(cx), cx).is_focused(window);
        let search = div()
            .h(px(34.))
            .mb(px(8.))
            .pl(px(10.))
            .pr(px(4.))
            .flex_none()
            .squircle(ui::RADIUS_MD)
            .bg(palette::surface(Level::Page, cx))
            .border_1()
            .border_color(if focused { palette::accent() } else { palette::line_strong(cx) })
            .when(focused, |d| d.shadow(vec![ring(palette::accent_soft(cx), 3.)]))
            .flex()
            .items_center()
            .gap(px(8.))
            .text_color(palette::faint(cx))
            .child(Icon::new(Glyph::Search).size(px(16.)))
            .child(div().flex_1().min_w_0().text_size(px(14.)).child(Input::new(&self.settings_search).appearance(false)));
        let mut list: Vec<AnyElement> = Vec::new();
        for (heading, pages) in NAV {
            let pages: Vec<Page> = pages.iter().copied().filter(|p| !searching || p.label().to_lowercase().contains(&query)).collect();
            if pages.is_empty() {
                continue;
            }
            if let Some(heading) = heading {
                list.push(div().pt(px(14.)).px(px(10.)).pb(px(4.)).text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(palette::muted(cx)).child(heading).into_any_element());
            }
            for page in pages {
                let current = view.page == page;
                list.push(nav_item(page, current && !searching, cx).into_any_element());
                if current && !searching && !page.sections().is_empty() {
                    list.push(sub_items(page, view.section, anchors, cx).into_any_element());
                }
            }
        }
        let asking = view.asking == Some(Ask::LogOut);
        div()
            .id("settings-nav")
            .w(px(240.))
            .flex_none()
            .h_full()
            .overflow_y_scroll()
            .bg(sidebar)
            .pt(px(if cfg!(target_os = "macos") { 0. } else { 16. }))
            .px(px(8.))
            .pb(px(12.))
            .flex()
            .flex_col()
            .gap(px(1.))
            // On macOS the traffic lights sit over the nav's top, as in System Settings; the strip drags the window.
            .when(cfg!(target_os = "macos"), |d| d.child(div().h(px(TITLE_HEIGHT + 12.)).flex_none().window_control_area(gpui_kit::WindowControlArea::Drag)))
            .children(me)
            .child(search)
            .children(list)
            .child(div().h(px(1.)).mx(px(10.)).my(px(8.)).flex_none().bg(palette::line(cx)))
            .child(self.log_out_item(cx))
            .children(asking.then(|| self.log_out_ask(cx)))
            .child(
                div()
                    .mt_auto()
                    .pt(px(16.))
                    .px(px(10.))
                    .pb(px(4.))
                    .text_size(px(12.))
                    .line_height(px(18.))
                    .text_color(palette::muted(cx))
                    .child(format!("Wave {} ({})", env!("CARGO_PKG_VERSION"), os_name())),
            )
    }

    fn log_out_item(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tint = palette::danger().opacity(0.12);
        div()
            .id("nav-log-out")
            .min_h(px(32.))
            .px(px(10.))
            .flex_none()
            .squircle(ui::RADIUS_SM)
            .flex()
            .items_center()
            .gap(px(10.))
            .cursor_pointer()
            .font_weight(FontWeight::MEDIUM)
            .text_color(palette::danger_ink(cx))
            .hover(move |s| s.bg(tint))
            .child(Icon::new(Glyph::DoorExit).size(px(18.)))
            .child("Log out")
            .on_click(cx.listener(|this, _, _, cx| this.set_ask(Some(Ask::LogOut), cx)))
    }

    // Log out asks in place: what happens, then Stay or Log out.
    fn log_out_ask(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let buttons = div()
            .flex()
            .gap(px(8.))
            .child(ui::button("log-out-stay", Tone::Secondary, cx).small().flex_1().child("Stay").on_click(cx.listener(|this, _, _, cx| this.set_ask(None, cx))))
            .child(ui::button("log-out-yes", Tone::Danger, cx).small().flex_1().child("Log out").on_click(cx.listener(|this, _, _, cx| this.sign_out(cx))));
        reveal(
            "log-out-ask",
            div()
                .my(px(4.))
                .p(px(10.))
                .squircle(ui::RADIUS_MD)
                .bg(palette::danger().opacity(0.1))
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(
                    div()
                        .text_size(px(14.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::strong(cx))
                        .child("Log out of Wave on this computer?")
                        .child(div().mt(px(2.)).text_size(px(12.)).font_weight(FontWeight::NORMAL).text_color(palette::muted(cx)).child("You'll scan a new code with your phone to sign back in.")),
                )
                .child(buttons),
        )
    }

    fn set_ask(&mut self, ask: Option<Ask>, cx: &mut Context<Self>) {
        if let Some(view) = &mut self.settings {
            view.asking = ask;
        }
        cx.notify();
    }

    fn account_page(&self, cx: &mut Context<Self>) -> Vec<Section> {
        let Some(me) = self.me.clone() else { return Vec::new() };
        let raised = palette::surface(Level::Raised, cx);
        let banner = ui::edge(div().h(px(96.)).bg(gpui_kit::linear_gradient(135., gpui_kit::linear_color_stop(rgb(BANNER.0), 0.), gpui_kit::linear_color_stop(rgb(BANNER.1), 1.))), ui::top(ui::RADIUS_LG - 1.)).corner_shape(ui::SQUIRCLE);
        let avatar = div()
            .relative()
            .flex_none()
            .child(ui::avatar(me.avatar.clone(), 100.).border(px(6.)).border_color(raised))
            .child(div().absolute().right(px(4.)).bottom(px(4.)).p(px(2.)).rounded_full().bg(raised).child(ui::status_mark(self.my_status, 28., raised, cx)));
        let identity = div()
            .mt(px(-42.))
            .px(px(10.))
            .flex()
            .items_end()
            .gap(px(16.))
            .child(avatar)
            .child(div().flex_1().min_w_0().pb(px(12.)).truncate().text_size(px(20.)).line_height(px(24.)).font_weight(FontWeight::BOLD).text_color(palette::strong(cx)).child(me.name.clone()));
        let copied = self.settings.as_ref().is_some_and(|s| s.copied);
        let id = me.id.clone();
        let copy = ui::button("copy-user-id", Tone::Secondary, cx).small()
            .min_w(px(96.))
            .when(copied, |b| b.text_color(palette::positive_ink(cx)))
            .child(Icon::new(if copied { Glyph::Checkmark } else { Glyph::Copy }).size(px(16.)))
            .child(if copied { "Copied" } else { "Copy ID" })
            .on_click(cx.listener(move |this, _, _, cx| this.copy_user_id(&id, cx)));
        let mut lines = Hairlines::default();
        let fields = div()
            .pt(px(12.))
            .px(px(4.))
            .pb(px(4.))
            .flex()
            .flex_col()
            .child(ui::settings_row("acct-name", lines.next(), cx).child(field_text("Display name", me.name.clone(), cx)))
            .child(ui::settings_row("acct-id", lines.next(), cx).child(field_text("User ID", me.id.clone(), cx)).child(copy));
        let card = div().squircle(ui::RADIUS_LG).bg(raised).border_1().border_color(palette::line(cx)).flex().flex_col().child(banner).child(identity).child(fields);
        vec![Section::new(ui::settings_group("Account info", cx).child(card))]
    }

    fn copy_user_id(&mut self, id: &str, cx: &mut Context<Self>) {
        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(id.to_owned()));
        if let Some(view) = &mut self.settings {
            view.copied = true;
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(COPIED_FOR).await;
            this.update(cx, |this, cx| {
                if let Some(view) = &mut this.settings {
                    view.copied = false;
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn notifications_page(&self, window: &mut Window, cx: &mut Context<Self>) -> Vec<Section> {
        let mut lines = Hairlines::default();
        let rows: Vec<AnyElement> = NOTIFY_CUES.iter().map(|&(id, label, cue)| self.cue_row(id, label, cue, lines.next(), window, cx)).collect();
        vec![Section::new(
            ui::settings_group("Sounds", cx)
                .child(ui::settings_card(cx).children(rows))
                .child(ui::settings_caption("Sounds Wave plays for new direct messages and for mentions. Voice sounds are under Voice and video.", cx)),
        )]
    }

    fn appearance_page(&self, window: &Window, cx: &mut Context<Self>) -> Vec<Section> {
        let current = self.prefs.theme;
        let os_dark = matches!(window.appearance(), gpui_kit::WindowAppearance::Dark | gpui_kit::WindowAppearance::VibrantDark);
        let pick = |id: &'static str, pref: ThemePref, preview: AnyElement, label: &'static str, note: Option<&'static str>, cx: &mut Context<Self>| {
            let on = current == pref;
            ui::pressable(div().id(id), 0.97)
                .flex_1()
                .min_w_0()
                .pt(px(6.))
                .px(px(6.))
                .pb(px(10.))
                .squircle(ui::RADIUS_LG)
                .flex()
                .flex_col()
                .gap(px(8.))
                .cursor_pointer()
                .font_weight(FontWeight::MEDIUM)
                .text_color(if on { palette::strong(cx) } else { palette::text(cx) })
                .when(on, |d| d.bg(palette::selected(cx)))
                .when(!on, |d| {
                    let hover = palette::hover(cx);
                    d.hover(move |s| s.bg(hover))
                })
                .child(
                    div()
                        .squircle(ui::RADIUS_MD)
                        .shadow(vec![ring(if on { palette::accent() } else { palette::line_strong(cx) }, if on { 2. } else { 1. })])
                        .child(preview),
                )
                .child(
                    div()
                        .px(px(4.))
                        .flex()
                        .flex_col()
                        .gap(px(1.))
                        .child(label)
                        .children(note.map(|n| div().text_size(px(12.)).font_weight(FontWeight::NORMAL).text_color(palette::muted(cx)).child(n))),
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.update_prefs(cx, |prefs| prefs.theme = pref);
                    palette::sync(window, cx);
                }))
        };
        let picks = div()
            .flex()
            .gap(px(12.))
            .child(pick("theme-light", ThemePref::Light, mini_window(&MINI_LIGHT).into_any_element(), "Light", None, cx))
            .child(pick("theme-dark", ThemePref::Dark, mini_window(&MINI_DARK).into_any_element(), "Midnight", None, cx))
            .child(pick("theme-system", ThemePref::System, mini_sync(), "Sync with OS", Some(if os_dark { "Midnight now" } else { "Light now" }), cx));
        // GAP: the design's Neutrals, Message density and Chat font size groups have no setting in the
        // app yet; they appear here once Prefs carries them.
        vec![
            Section::new(ui::settings_group("Theme", cx).child(picks).child(ui::settings_caption("Midnight is the darkest theme, the one Wave is built on.", cx))),
            Section::new(ui::settings_group("App icon", cx).child(self.app_icon_grid(cx)).child(ui::settings_caption("How Wave looks in your Dock: the same voice wave in different media. Changes right away; no restart.", cx))),
        ]
    }

    fn app_icon_grid(&self, cx: &mut Context<Self>) -> AnyElement {
        let current = self.prefs.app_icon;
        let tiles = crate::app_icon::AppIcon::ALL.map(|icon| {
            let on = icon == current;
            let hover = palette::hover(cx);
            ui::pressable(div().id(SharedString::from(format!("app-icon-{}", icon.label()))), 0.95)
                .w(px(88.))
                .pt(px(10.))
                .px(px(4.))
                .pb(px(8.))
                .squircle(ui::RADIUS_LG)
                .flex()
                .flex_col()
                .items_center()
                .gap(px(6.))
                .cursor_pointer()
                .text_size(px(12.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(if on { palette::strong(cx) } else { palette::text(cx) })
                .when(on, |d| d.bg(palette::selected(cx)).shadow(vec![inset_ring(palette::accent(), 2.)]))
                .when(!on, |d| d.hover(move |s| s.bg(hover)))
                .child(img(icon.asset()).size(px(56.)))
                .child(icon.label())
                .children((icon == crate::app_icon::AppIcon::default()).then(|| div().mt(px(-4.)).font_weight(FontWeight::NORMAL).text_color(palette::muted(cx)).child("Default")))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.update_prefs(cx, |prefs| prefs.app_icon = icon);
                    icon.apply();
                }))
        });
        div().flex().flex_wrap().gap(px(8.)).children(tiles).into_any_element()
    }

    fn voice_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Vec<Section> {
        let prefs = self.prefs.clone();
        let view = self.settings.as_ref().expect("settings are open");
        let (inputs, outputs, cameras) = (view.devices.inputs.clone(), view.devices.outputs.clone(), view.devices.cameras.clone());
        let advanced = view.advanced;
        let (custom, studio) = (prefs.input_profile == InputProfile::Custom, prefs.input_profile == InputProfile::Studio);
        let mut out = vec![Section::anchored("Voice", self.devices_group(&inputs, &outputs, window, cx))];

        let mut lines = Hairlines::default();
        let profiles = [
            ("profile-isolation", "Voice isolation", "Just your beautiful voice: Wave cuts through the noise around you", InputProfile::VoiceIsolation),
            ("profile-studio", "Studio", "Pure audio: an open mic with no processing", InputProfile::Studio),
            ("profile-custom", "Custom", "Advanced mode: every button and dial, set your way", InputProfile::Custom),
        ];
        let profile_rows: Vec<Stateful<Div>> = profiles
            .into_iter()
            .map(|(id, title, detail, value)| {
                radio_row(id, lines.next(), title, detail, prefs.input_profile == value, cx).on_click(cx.listener(move |this, _, _, cx| this.update_prefs(cx, |p| p.set_profile(value))))
            })
            .collect();
        out.push(Section::new(
            ui::settings_group("Input profile", cx)
                .child(ui::settings_card(cx).children(profile_rows))
                .when(!custom, |g| g.child(ui::settings_caption("This profile sets noise suppression, echo cancellation and sensitivity for you. Pick Custom to change them.", cx))),
        ));
        if !studio {
            out.push(Section::new(ui::settings_group("Input mode", cx).child(self.input_mode_card(custom, window, cx))));
        }
        if custom {
            let mut lines = Hairlines::default();
            let noise = self.noise_row(lines.next(), window, cx);
            let echo = self.switch_row(SwitchRow { id: "set-echo", title: "Echo cancellation", detail: Some("Stops others hearing themselves through your speakers"), on: prefs.echo_cancellation, enabled: true, field: |p| &mut p.echo_cancellation }, lines.next(), window, cx);
            out.push(Section::new(ui::settings_group("Voice processing", cx).child(ui::settings_card(cx).child(noise).child(echo))));
        }
        let expander = expander_row("voice-advanced", false, if advanced { "Hide advanced voice settings" } else { "Show advanced voice settings" }, &advanced_summary(custom), advanced, window, cx).on_click(cx.listener(|this, _, _, cx| {
            if let Some(s) = &mut this.settings {
                s.advanced = !s.advanced;
            }
            cx.notify();
        }));
        let advanced_rows = advanced.then(|| reveal("voice-advanced-body", self.advanced_voice_rows(custom, window, cx)));
        out.push(Section::new(ui::settings_card(cx).child(expander).children(advanced_rows)));

        let camera = div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(field_label("Camera", cx))
            .child(self.device_select(Select::Camera, Glyph::Video, &cameras, prefs.camera.as_deref(), 40., window, cx));
        // GAP: the design's camera preview tile (Test video, mirrored self view) needs a local camera
        // preview the settings page doesn't have yet.
        out.push(Section::anchored("Camera", ui::settings_group("Camera", cx).child(card_pad(cx).child(camera))));
        out.push(Section::anchored("Sounds", self.sounds_group(window, cx)));
        out.push(Section::anchored("Advanced", ui::settings_group("Advanced", cx).child(ui::settings_card(cx).child(self.reset_row(cx)))));
        out
    }

    fn devices_group(&mut self, inputs: &[(String, String)], outputs: &[(String, String)], window: &mut Window, cx: &mut Context<Self>) -> Div {
        let prefs = self.prefs.clone();
        let view = self.settings.as_ref().expect("settings are open");
        let testing = self.mic_test.is_some();
        // The mic test's "isn't detecting any input", or in a call the No audio detected warning.
        let in_call_silent = prefs.silence_warning && self.call.as_ref().is_some_and(|c| c.session.input_detected() == Some(false));
        let no_input = if testing { view.mic_input == Some(false) } else { in_call_silent };
        let column = |label: &'static str, field: AnyElement, cx: &App| div().flex_1().min_w_0().flex().flex_col().gap(px(6.)).child(field_label(label, cx)).child(field);
        let percent = |v: f32| format!("{}%", (v * 100.).round() as i32);
        let devices = div()
            .flex()
            .gap(px(24.))
            .child(column("Microphone", self.device_select(Select::Microphone, Glyph::Mic, inputs, prefs.input_device.as_deref(), 40., window, cx), cx))
            .child(column("Speaker", self.device_select(Select::Speaker, Glyph::Headphones, outputs, prefs.output_device.as_deref(), 40., window, cx), cx));
        let volumes = div()
            .flex()
            .gap(px(24.))
            .child(div().flex_1().min_w_0().child(self.volume("Microphone volume", &percent(prefs.input_volume), Slider::InputVolume, cx)))
            .child(div().flex_1().min_w_0().child(self.volume("Speaker volume", &percent(prefs.output_volume), Slider::OutputVolume, cx)));
        let test_button = ui::button("mic-test", Tone::Primary, cx)
            .min_w(px(112.))
            .child(Icon::new(if testing { Glyph::Stop } else { Glyph::Mic }).size(px(16.)))
            .child(if testing { "Stop testing" } else { "Test mic" })
            .on_click(cx.listener(|this, _, _, cx| this.toggle_mic_test(cx)));
        let mic_test = div().flex().items_center().gap(px(16.)).child(test_button).child(mic_wave(self.mic_level, testing, cx));
        let speaker = outputs.iter().find(|(id, _)| Some(id.as_str()) == prefs.output_device.as_deref()).map_or("your speaker", |(_, name)| name.as_str());
        let mic = inputs.iter().find(|(id, _)| Some(id.as_str()) == prefs.input_device.as_deref()).map_or("your microphone", |(_, name)| name.as_str());
        let notice = no_input.then(|| self.no_input_notice(mic, cx));
        ui::settings_group("Devices", cx).child(
            card_pad(cx)
                .child(div().flex().flex_col().gap(px(16.)).child(devices).child(volumes))
                .child(mic_test)
                .children(notice)
                .child(ui::settings_caption(&format!("Talk normally. You'll hear yourself back through {speaker}."), cx).px_0()),
        )
    }

    // Names the device and every likely fix, with the fixes one click away.
    fn no_input_notice(&self, mic: &str, cx: &mut Context<Self>) -> Div {
        let actions = div()
            .mt(px(8.))
            .flex()
            .flex_wrap()
            .gap(px(8.))
            .child(ui::button("nomic-pick", Tone::Secondary, cx).small().child("Choose another input").on_click(cx.listener(|this, _, _, cx| {
                if let Some(s) = &mut this.settings {
                    s.select = Some(Select::Microphone);
                }
                cx.notify();
            })))
            .when(cfg!(target_os = "macos"), |d| {
                d.child(ui::button("nomic-system", Tone::Ghost, cx).small().child("Open System Settings").on_click(|_, _, cx| cx.open_url(MAC_MIC_PRIVACY)))
            });
        ui::callout_with(
            ui::Notice::Warning,
            Glyph::MicOff,
            "No audio detected from your mic",
            format!("Wave hears nothing from {mic}. Turn up Microphone volume above, pick another input, or let Wave use the microphone in System Settings."),
            actions,
            cx,
        )
    }

    fn volume(&self, label: &'static str, value: &str, slider: Slider, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .justify_between()
                    .text_size(px(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(palette::muted(cx))
                    .child(label)
                    .child(div().text_color(palette::strong(cx)).child(value.to_owned())),
            )
            .child(self.slider(slider, None, cx))
    }

    // Voice activity or push to talk; push to talk shows its shortcut and release delay, voice
    // activity under Custom its sensitivity.
    fn input_mode_card(&self, custom: bool, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let prefs = &self.prefs;
        let ptt = prefs.push_to_talk;
        let mut lines = Hairlines::default();
        let mode = |id: &'static str, title: &'static str, detail: &'static str, on_ptt: bool, line: bool, cx: &mut Context<Self>| {
            radio_row(id, line, title, detail, ptt == on_ptt, cx).on_click(cx.listener(move |this, _, _, cx| this.update_prefs(cx, |p| p.push_to_talk = on_ptt)))
        };
        let mut card = ui::settings_card(cx).child(mode("mode-vad", "Voice activity", "Transmits when you speak above the sensitivity", false, lines.next(), cx)).child(mode(
            "mode-ptt",
            "Push to talk",
            "Transmits while you hold a key",
            true,
            lines.next(),
            cx,
        ));
        if ptt {
            let ptt_index = prefs.keybinds.iter().position(|k| k.action == Action::PushToTalk);
            card = card
                .child(ui::settings_row("ptt-shortcut", lines.next(), cx).child(row_text("Shortcut", Some("Also listed under Keybinds"), cx)).children(ptt_index.map(|i| self.keybind_recorder(i, cx).flex_basis(px(300.)).flex_shrink(1.))))
                .child(
                    stack_row("ptt-delay", lines.next(), cx)
                        .child(numbered("Push to talk release delay", "Keeps transmitting this long after you let go, so word endings aren't cut", &format!("{} ms", prefs.ptt_release_ms), cx))
                        .child(self.slider(Slider::ReleaseDelay, None, cx)),
                );
        } else if custom {
            let auto = prefs.sensitivity_auto;
            card = card
                .child(self.switch_row(
                    SwitchRow { id: "set-auto-sensitivity", title: "Set sensitivity automatically", detail: Some("Wave adjusts to the noise around you. When the bar fills, you're transmitting."), on: auto, enabled: true, field: |p| &mut p.sensitivity_auto },
                    lines.next(),
                    window,
                    cx,
                ))
                .child(
                    stack_row("input-sensitivity", lines.next(), cx)
                        .child(row_text("Input sensitivity", Some(if auto { "Your mic transmits when Wave hears your voice" } else { "Your mic transmits when the level passes the handle" }), cx))
                        .child(if auto { sensitivity_track(None, self.mic_level, cx).into_any_element() } else { self.slider(Slider::Sensitivity, Some(self.mic_level), cx) })
                        .when(!auto, |d| d.child(scale(&["\u{2212}100 dB", "0 dB"], cx))),
                );
        }
        card
    }

    fn sounds_group(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let more = self.settings.as_ref().is_some_and(|s| s.more_sounds);
        let mut lines = Hairlines::default();
        let first: Vec<AnyElement> = VOICE_CUES.iter().map(|&(id, label, cue)| self.cue_row(id, label, cue, lines.next(), window, cx)).collect();
        let expander = expander_row(
            "more-sounds",
            lines.next(),
            &if more { format!("Hide {} sounds", MORE_CUES.len()) } else { format!("Show {} more sounds", MORE_CUES.len()) },
            "Voice disconnected, PTT activate, User join, and more",
            more,
            window,
            cx,
        )
        .on_click(cx.listener(|this, _, _, cx| {
            if let Some(s) = &mut this.settings {
                s.more_sounds = !s.more_sounds;
            }
            cx.notify();
        }));
        let mut body = Hairlines(true);
        let rest = more.then(|| reveal("more-sounds-body", div().flex().flex_col().children(MORE_CUES.iter().map(|&(id, label, cue)| self.cue_row(id, label, cue, body.next(), window, cx)))));
        ui::settings_group("Sounds", cx).child(ui::settings_card(cx).children(first).child(expander).children(rest)).child(ui::settings_caption("Message and mention sounds are under Notifications.", cx))
    }

    // Reset asks in place, naming what happens; once done it offers Undo for a few seconds.
    fn reset_row(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let view = self.settings.as_ref().expect("settings are open");
        let (asking, done) = (view.asking == Some(Ask::Reset), view.reset_undo.is_some());
        let trigger = (!asking && !done).then(|| {
            ui::button("reset-voice", Tone::Secondary, cx).small().text_color(palette::danger_ink(cx)).child("Reset voice settings").on_click(cx.listener(|this, _, _, cx| this.set_ask(Some(Ask::Reset), cx)))
        });
        let ask = asking.then(|| {
            reveal(
                "reset-ask",
                div()
                    .w_full()
                    .py(px(10.))
                    .px(px(12.))
                    .squircle(ui::RADIUS_MD)
                    .bg(palette::danger().opacity(0.1))
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(8.))
                    .child(div().flex_1().min_w(px(200.)).font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child("Reset every voice and video setting to its default?"))
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .child(ui::button("reset-keep", Tone::Secondary, cx).small().child("Keep my settings").on_click(cx.listener(|this, _, _, cx| this.set_ask(None, cx))))
                            .child(ui::button("reset-yes", Tone::Danger, cx).small().child("Reset voice settings").on_click(cx.listener(|this, _, _, cx| this.reset_voice(cx)))),
                    ),
            )
        });
        let done = done.then(|| {
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette::positive_ink(cx))
                .child(Icon::new(Glyph::CheckCircle).size(px(16.)))
                .child("Voice and video settings reset")
                .child(
                    div()
                        .id("reset-undo")
                        .ml(px(4.))
                        .cursor_pointer()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::accent_ink(cx))
                        .hover(|s| s.underline())
                        .child("Undo")
                        .on_click(cx.listener(|this, _, _, cx| this.undo_reset(cx))),
                )
        });
        ui::settings_row("reset", false, cx)
            .flex_wrap()
            .child(row_text("Reset voice and video settings", Some("Puts devices, levels, processing and sounds back to their defaults. Your keybinds and theme stay."), cx))
            .children(trigger)
            .children(done)
            .children(ask)
    }

    fn reset_voice(&mut self, cx: &mut Context<Self>) {
        let before = self.prefs.clone();
        self.update_prefs(cx, |p| {
            let keep = (p.theme, p.keybinds.clone(), p.app_icon);
            *p = Prefs::default();
            (p.theme, p.keybinds, p.app_icon) = keep;
        });
        if let Some(view) = &mut self.settings {
            (view.asking, view.reset_undo) = (None, Some(before));
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(UNDO_FOR).await;
            this.update(cx, |this, cx| {
                if let Some(view) = &mut this.settings {
                    view.reset_undo = None;
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn undo_reset(&mut self, cx: &mut Context<Self>) {
        let Some(before) = self.settings.as_mut().and_then(|v| v.reset_undo.take()) else { return };
        self.update_prefs(cx, |p| *p = before);
    }

    fn keybinds_page(&self, window: &mut Window, cx: &mut Context<Self>) -> Vec<Section> {
        let header = div()
            .flex()
            .items_start()
            .gap(px(16.))
            .child(div().flex_1().min_w_0().flex().flex_col().gap(px(4.)).child(page_title("Keybinds", cx)).child(div().text_color(palette::muted(cx)).child("Shortcuts that work everywhere, even while another app is in front.")))
            .child(ui::button("add-keybind", Tone::Primary, cx).child(Icon::new(Glyph::Plus).size(px(16.))).child("Add a keybind").on_click(cx.listener(|this, _, _, cx| {
                this.update_prefs(cx, |p| p.keybinds.push(Keybind { action: Action::Unassigned, keys: None }));
                let index = this.prefs.keybinds.len() - 1;
                if let Some(s) = &mut this.settings {
                    s.recording = Some(index);
                }
            })));
        let mut lines = Hairlines::default();
        let rows: Vec<AnyElement> = self.prefs.keybinds.iter().enumerate().map(|(index, keybind)| self.keybind_row(index, keybind, lines.next(), window, cx)).collect();
        let yours = ui::settings_group("Your keybinds", cx).when(!rows.is_empty(), |g| g.child(ui::settings_card(cx).children(rows)));
        let mut lines = Hairlines::default();
        let defaults = DEFAULT_KEYBINDS.iter().map(|&(action, combos)| {
            ui::settings_row(SharedString::from(format!("default-{action}")), lines.next(), cx).min_h(px(48.)).child(row_text(action, None::<&str>, cx)).child(combo_caps(combos, cx))
        });
        let platform = if cfg!(target_os = "macos") { "Built in and always on." } else { "Built in and always on. Ctrl reads \u{2318} on a Mac." };
        vec![
            Section::new(header),
            Section::new(yours),
            Section::new(ui::settings_group("Default keybinds", cx).child(ui::settings_card(cx).children(defaults)).child(ui::settings_caption(platform, cx))),
        ]
    }

    // Action picker, recorder, and a remove button that's always there.
    fn keybind_row(&self, index: usize, keybind: &Keybind, line: bool, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let refused = keybind.keys.as_ref().is_some_and(|k| self.keybind_errors.contains(k));
        let actions: Vec<(String, String)> = Action::ALL.iter().map(|a| (format!("{a:?}"), sentence(a.label()))).collect();
        let action = self.select_field(Select::Action(index), None, &actions, &format!("{:?}", keybind.action), sentence(keybind.action.label()).into(), 36., window, cx);
        let (danger, hover, press) = (palette::danger_ink(cx), palette::hover(cx), palette::press(cx));
        // The design's .iconbtn.rm: ui::icon_button, but hovering to the danger ink.
        let remove = div()
            .id(SharedString::from(format!("keybind-delete-{index}")))
            .size(px(32.))
            .flex_none()
            .squircle(ui::RADIUS_SM)
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .text_color(palette::muted(cx))
            .hover(move |s| s.bg(hover).text_color(danger))
            .active(move |s| s.bg(press))
            .child(Icon::new(Glyph::Trash).size(px(20.)))
            .tooltip(|window, cx| gpui_kit::component::tooltip::Tooltip::new("Remove keybind").build(window, cx))
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(s) = &mut this.settings {
                    s.recording = None;
                }
                this.update_prefs(cx, |p| {
                    if index < p.keybinds.len() {
                        p.keybinds.remove(index);
                    }
                });
            }));
        ui::settings_row(SharedString::from(format!("keybind-{index}")), line, cx)
            .flex_wrap()
            .gap(px(12.))
            .child(div().flex_grow(1.).flex_basis(px(0.)).min_w_0().child(action))
            .child(self.keybind_recorder(index, cx).flex_grow(1.25).flex_basis(px(0.)))
            .child(remove)
            .children(refused.then(|| error_line("Another app already uses this shortcut. Record a different one, or change it in that app.", cx).w_full()))
            .into_any_element()
    }

    // The binding as keycaps, and a button that starts, stops or edits recording. Recording rings the
    // field in the accent, pulsing.
    fn keybind_recorder(&self, index: usize, cx: &mut Context<Self>) -> Div {
        let recording = self.settings.as_ref().and_then(|s| s.recording) == Some(index);
        let keys = self.prefs.keybinds.get(index).and_then(|k| k.keys.clone());
        let refused = keys.as_ref().is_some_and(|k| self.keybind_errors.contains(k));
        let value = match (&keys, recording) {
            (_, true) => div()
                .flex()
                .items_center()
                .gap(px(6.))
                .text_color(palette::muted(cx))
                .child(div().text_color(palette::accent_ink(cx)).child(Icon::new(Glyph::Record).size(px(14.))))
                .child("Press a key\u{2026}")
                .into_any_element(),
            (Some(keys), false) => key_caps(keys, cx).into_any_element(),
            (None, false) => div().text_color(palette::muted(cx)).font_weight(FontWeight::NORMAL).child("No keybind set").into_any_element(),
        };
        let border = match (recording, refused) {
            (true, _) => palette::accent(),
            (false, true) => palette::danger_ink(cx),
            (false, false) => palette::line_strong(cx),
        };
        let label = if recording {
            "Stop recording"
        } else if keys.is_some() {
            "Edit keybind"
        } else {
            "Record keybind"
        };
        div()
            .relative()
            .h(px(40.))
            .min_w_0()
            .pl(px(12.))
            .pr(px(4.))
            .squircle(ui::RADIUS_MD)
            .bg(palette::surface(Level::Page, cx))
            .border_1()
            .border_color(border)
            .flex()
            .items_center()
            .gap(px(8.))
            .children(recording.then(|| pulse_ring(SharedString::from(format!("record-ring-{index}")), cx)))
            .child(div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(value))
            .child(ui::button(SharedString::from(format!("record-{index}")), Tone::Secondary, cx).small().child(label).on_click(cx.listener(move |this, _, window, cx| {
                if let Some(s) = &mut this.settings {
                    s.recording = if s.recording == Some(index) { None } else { Some(index) };
                }
                this.focus.focus(window, cx);
                cx.notify();
            })))
    }

    // Discord's "Show advanced voice settings" rows, in its order; the first two only under Custom.
    fn advanced_voice_rows(&self, custom: bool, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let prefs = &self.prefs;
        let vad_enabled = !prefs.push_to_talk && prefs.sensitivity_auto;
        let mut lines = Hairlines(true);
        let mut rows: Vec<AnyElement> = Vec::new();
        if custom {
            rows.push(
                self.switch_row(SwitchRow { id: "set-agc", title: "Automatic gain control", detail: Some("Adjusts your mic volume to keep you clear and steady"), on: prefs.auto_gain, enabled: true, field: |p| &mut p.auto_gain }, lines.next(), window, cx)
                    .into_any_element(),
            );
            let detail = if vad_enabled { "Turn this off if automatic sensitivity misses your voice" } else { WHY_VAD };
            rows.push(
                self.switch_row(
                    SwitchRow { id: "set-advanced-vad", title: "Advanced voice activity", detail: Some(detail), on: prefs.advanced_voice_activity, enabled: vad_enabled, field: |p| &mut p.advanced_voice_activity },
                    lines.next(),
                    window,
                    cx,
                )
                .into_any_element(),
            );
        }
        rows.push(
            self.switch_row(
                SwitchRow { id: "set-silence-warning", title: "No audio detected warning", detail: Some("Warns you in a call when Wave hears nothing from your mic"), on: prefs.silence_warning, enabled: true, field: |p| &mut p.silence_warning },
                lines.next(),
                window,
                cx,
            )
            .into_any_element(),
        );
        if voice::ATTENUATION_SUPPORTED {
            rows.push(
                stack_row("attenuation", lines.next(), cx)
                    .child(row_text("Global attenuation", Some("Lowers other apps by this much while someone speaks. 0% turns it off."), cx))
                    .child(self.volume("Attenuation strength", &format!("{}%", prefs.attenuation), Slider::Attenuation, cx))
                    .into_any_element(),
            );
            rows.push(
                self.switch_row(SwitchRow { id: "set-attenuate-self", title: "When I speak", detail: Some("Lower other apps while you talk"), on: prefs.attenuate_self, enabled: true, field: |p| &mut p.attenuate_self }, lines.next(), window, cx)
                    .into_any_element(),
            );
            rows.push(
                self.switch_row(
                    SwitchRow { id: "set-attenuate-others", title: "When others speak", detail: Some("Lower other apps while anyone else talks"), on: prefs.attenuate_others, enabled: true, field: |p| &mut p.attenuate_others },
                    lines.next(),
                    window,
                    cx,
                )
                .into_any_element(),
            );
        }
        rows.push(
            self.switch_row(
                SwitchRow {
                    id: "set-qos",
                    title: "Quality of service",
                    detail: Some("Asks your router to send Wave's voice first. Turn it off if calls drop; some routers handle it badly."),
                    on: prefs.qos,
                    enabled: true,
                    field: |p| &mut p.qos,
                },
                lines.next(),
                window,
                cx,
            )
            .into_any_element(),
        );
        div().flex().flex_col().children(rows)
    }

    // Three options fit in a row, so noise suppression is a segmented control. Krisp is licensed to
    // Discord, so Advanced is DeepFilterNet, an open neural suppressor, in its place.
    fn noise_row(&self, line: bool, window: &mut Window, cx: &mut Context<Self>) -> Stateful<Div> {
        let selected = match (self.prefs.noise_suppression, self.prefs.noise_mode) {
            (false, _) => 0,
            (true, NoiseMode::Standard) => 1,
            (true, NoiseMode::Advanced) => 2,
        };
        let degraded = selected == 2 && self.call.as_ref().is_some_and(|c| c.session.noise_suppression_degraded());
        let detail = if degraded {
            div()
                .flex()
                .items_center()
                .gap(px(4.))
                .text_size(px(12.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette::warning_ink(cx))
                .child(Icon::new(Glyph::WarningBold).size(px(14.)))
                .child("Your computer couldn't run DeepFilterNet in real time, so this call uses Standard.")
        } else {
            div().text_size(px(12.)).text_color(palette::muted(cx)).child("Reduces background noise from your mic. Advanced is powered by DeepFilterNet.")
        };
        let this = cx.entity().downgrade();
        let control = ui::segmented_live(
            "set-noise",
            &["None", "Standard", "Advanced"],
            selected,
            move |i, _, cx| {
                this.update(cx, |this, cx| {
                    this.update_prefs(cx, |p| match i {
                        0 => p.noise_suppression = false,
                        1 => (p.noise_suppression, p.noise_mode) = (true, NoiseMode::Standard),
                        _ => (p.noise_suppression, p.noise_mode) = (true, NoiseMode::Advanced),
                    })
                })
                .ok();
            },
            window,
            cx,
        );
        ui::settings_row("noise", line, cx).child(row_text("Noise suppression", None::<&str>, cx).child(detail)).child(control.flex_none())
    }

    // A sound cue: its preview button beside its switch.
    fn cue_row(&self, id: &'static str, title: &'static str, cue: Cue, line: bool, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let on = !self.prefs.silenced_cues.contains(&cue);
        let (selected, press) = (palette::selected(cx), palette::press(cx));
        let play = div()
            .id(SharedString::from(format!("preview-{id}")))
            .size(px(36.))
            .flex_none()
            .squircle(ui::RADIUS_MD)
            .bg(selected)
            .text_color(palette::strong(cx))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .hover(move |s| s.bg(press))
            .child(Icon::new(Glyph::Play).size(px(16.)))
            .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(format!("Preview {title}")).build(window, cx))
            .on_click(move |_, _, cx| {
                cx.stop_propagation();
                sounds::preview(cue);
            });
        let switch = ui::switch_live(id, on, window, cx).on_click(cx.listener(move |this, _, _, cx| {
            this.update_prefs(cx, |p| match p.silenced_cues.iter().position(|c| *c == cue) {
                Some(i) => {
                    p.silenced_cues.remove(i);
                }
                None => p.silenced_cues.push(cue),
            })
        }));
        // GAP: the play button turns into the mini wave while its sound plays; sounds.rs doesn't report
        // playback yet.
        ui::settings_row(SharedString::from(format!("cue-{id}")), line, cx)
            .min_h(px(52.))
            .child(row_text(title, None::<&str>, cx))
            .child(div().flex().items_center().gap(px(8.)).flex_none().child(play).child(switch))
            .into_any_element()
    }

    // A switch row toggles from anywhere on it. A disabled one dims its switch and says why.
    fn switch_row(&self, spec: SwitchRow, line: bool, window: &mut Window, cx: &mut Context<Self>) -> Stateful<Div> {
        let SwitchRow { id, title, detail, on, enabled, field } = spec;
        let text = if enabled {
            row_text(title, detail, cx)
        } else {
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(div().truncate().font_weight(FontWeight::MEDIUM).text_color(palette::muted(cx)).child(title))
                .child(div().flex().items_center().gap(px(4.)).text_size(px(12.)).text_color(palette::muted(cx)).child(Icon::new(Glyph::Info).size(px(14.))).children(detail))
        };
        let switch = ui::switch_live(id, on, window, cx);
        let row = ui::settings_row(SharedString::from(format!("row-{id}")), line, cx).child(text);
        if !enabled {
            return row.child(div().opacity(0.45).child(switch));
        }
        choice(row, cx).child(switch).on_click(cx.listener(move |this, _, _, cx| {
            this.update_prefs(cx, |p| {
                let value = field(p);
                *value = !*value;
            })
        }))
    }

    #[allow(clippy::too_many_arguments)]
    fn device_select(&self, select: Select, glyph: Glyph, devices: &[(String, String)], chosen: Option<&str>, height: f32, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let mut options = vec![(String::new(), "Default".to_owned())];
        options.extend(devices.iter().cloned());
        let value = chosen.and_then(|id| devices.iter().find(|(d, _)| d == id)).map_or("Default".to_owned(), |(_, name)| name.clone());
        self.select_field(select, Some(glyph), &options, chosen.unwrap_or(""), value.into(), height, window, cx)
    }

    // A dropdown trigger whose list rises from it; the chosen option carries a check.
    #[allow(clippy::too_many_arguments)]
    fn select_field(&self, select: Select, glyph: Option<Glyph>, options: &[(String, String)], chosen: &str, value: SharedString, height: f32, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let open = self.settings.as_ref().and_then(|s| s.select) == Some(select);
        let caret = turn(format!("caret-{select:?}"), if open { 180. } else { 0. }, window, cx);
        let (muted, faint) = (palette::muted(cx), palette::faint(cx));
        let hover_border = faint.opacity(0.6);
        let field = div()
            .id(SharedString::from(format!("select-{select:?}")))
            .h(px(height))
            .px(px(12.))
            .w_full()
            .squircle(ui::RADIUS_MD)
            .bg(palette::surface(Level::Page, cx))
            .border_1()
            .border_color(if open { palette::accent() } else { palette::line_strong(cx) })
            .when(!open, |d| d.hover(move |s| s.border_color(hover_border)))
            .flex()
            .items_center()
            .gap(px(8.))
            .cursor_pointer()
            .text_size(px(14.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(palette::strong(cx))
            .children(glyph.map(|g| div().text_color(muted).child(Icon::new(g).size(px(18.)))))
            .child(div().flex_1().min_w_0().truncate().child(value))
            .child(div().text_color(faint).child(Icon::new(Glyph::ChevronDown).size(px(14.)).rotate(caret)))
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(s) = &mut this.settings {
                    s.select = if s.select == Some(select) { None } else { Some(select) };
                }
                cx.notify();
            }));
        let list = open.then(|| {
            let accent_ink = palette::accent_ink(cx);
            let rows: Vec<AnyElement> = options
                .iter()
                .map(|(id, name)| {
                    let (id, selected) = (id.clone(), id == chosen);
                    ui::menu_row(SharedString::from(format!("option-{select:?}-{id}")), cx)
                        .when(selected, |d| d.text_color(palette::strong(cx)))
                        .child(div().flex_1().min_w_0().truncate().child(name.clone()))
                        .child(div().text_color(accent_ink).when(!selected, |d| d.invisible()).child(Icon::new(Glyph::Checkmark).size(px(16.))))
                        .on_click(cx.listener(move |this, _, _, cx| this.choose(select, &id, cx)))
                        .into_any_element()
                })
                .collect();
            let menu = ui::surface(cx).id(SharedString::from(format!("options-{select:?}"))).squircle(ui::RADIUS_LG).max_h(px(280.)).overflow_y_scroll().gap(px(1.)).occlude().children(rows);
            // The list rises from the trigger: 0.95 scale about its top centre, with the fade.
            let list = div().absolute().top(px(height + 4.)).left_0().right_0().min_w(px(200.)).child(menu);
            ui::rise(list.transform_origin(gpui_kit::point(relative(0.5), relative(0.))), SharedString::from(format!("options-in-{select:?}")), Duration::ZERO)
        });
        // Deferred so the open list paints above the settings below it.
        div().relative().w_full().child(field).children(list.map(|list| gpui_kit::deferred(list).with_priority(2))).into_any_element()
    }

    fn choose(&mut self, select: Select, id: &str, cx: &mut Context<Self>) {
        let device = (!id.is_empty()).then(|| id.to_owned());
        if let Some(s) = &mut self.settings {
            s.select = None;
        }
        self.update_prefs(cx, |p| match select {
            Select::Microphone => p.input_device = device,
            Select::Speaker => p.output_device = device,
            Select::Camera => p.camera = device,
            Select::Action(index) => {
                let action = Action::ALL.into_iter().find(|a| format!("{a:?}") == id).unwrap_or(Action::Unassigned);
                if let Some(bind) = p.keybinds.get_mut(index) {
                    bind.action = action;
                }
            }
        });
    }

    fn slider(&self, slider: Slider, level: Option<f32>, cx: &mut Context<Self>) -> AnyElement {
        let fill = match slider {
            Slider::Sensitivity => Fill::Threshold(level),
            _ => Fill::Accent,
        };
        self.range(format!("{slider:?}").into(), slider.fraction(&self.prefs), fill, move |this, f, cx| this.update_prefs(cx, |p| slider.set(p, f)), cx)
    }

    // Wave's slider: the done part, a gap, a tall narrow thumb, a gap, the rest; click or drag anywhere
    // on it. The sensitivity slider draws the live level under its handle instead.
    pub(crate) fn range(
        &self,
        key: SharedString,
        fraction: f32,
        fill: Fill,
        on_set: impl Fn(&mut Rustcord, f32, &mut Context<Rustcord>) + Clone + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let bounds = self.slider_bounds.clone();
        let record = bounds.clone();
        let track = match fill {
            Fill::Accent => ui::slider_track(fraction, cx).into_any_element(),
            Fill::Threshold(level) => sensitivity_track(Some(fraction), level.unwrap_or(crate::SILENCE_DB), cx).into_any_element(),
        };
        let (down_key, drag_key, move_key, record_key) = (key.clone(), key.clone(), key.clone(), key.clone());
        let set = move |this: &mut Rustcord, key: &SharedString, x: Pixels, cx: &mut Context<Rustcord>| {
            let Some(b) = bounds.borrow().get(key).copied() else { return };
            on_set(this, (x - b.left()) / b.size.width, cx);
        };
        let set_drag = set.clone();
        div()
            .id(SharedString::from(format!("slider-{key}")))
            .relative()
            .h(px(24.))
            .w_full()
            .cursor_pointer()
            .child(canvas(move |b, _, _| { record.borrow_mut().insert(record_key.clone(), b); }, |_, _, _, _| {}).absolute().size_full())
            .child(track)
            .on_mouse_down(MouseButton::Left, cx.listener(move |this, e: &gpui_kit::MouseDownEvent, _, cx| set(this, &down_key, e.position.x, cx)))
            .on_drag(SliderDrag(drag_key), |drag, _, _, cx| cx.new(|_| drag.clone()))
            .on_drag_move::<SliderDrag>(cx.listener(move |this, e: &gpui_kit::DragMoveEvent<SliderDrag>, _, cx| {
                if e.drag(cx).0 == move_key {
                    set_drag(this, &move_key, e.event.position.x, cx);
                }
            }))
            .into_any_element()
    }
}

pub(crate) enum Fill {
    Accent,
    Threshold(Option<f32>),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ask {
    LogOut,
    Reset,
}

// A group on a settings page, with the nav anchor (Voice, Camera, …) that scrolls to it.
struct Section {
    anchor: Option<&'static str>,
    body: AnyElement,
}

impl Section {
    fn new(body: impl IntoElement) -> Self {
        Self { anchor: None, body: body.into_any_element() }
    }

    fn anchored(anchor: &'static str, body: impl IntoElement) -> Self {
        Self { anchor: Some(anchor), body: body.into_any_element() }
    }
}

struct SwitchRow {
    id: &'static str,
    title: &'static str,
    detail: Option<&'static str>,
    on: bool,
    enabled: bool,
    field: fn(&mut Prefs) -> &mut bool,
}

// Rows in a card are split by inset hairlines: every row but the first draws one above itself.
#[derive(Default)]
struct Hairlines(bool);

impl Hairlines {
    fn next(&mut self) -> bool {
        std::mem::replace(&mut self.0, true)
    }
}

const PAGE_WIDTH: f32 = 680.;
const COPIED_FOR: Duration = Duration::from_secs(2);
const UNDO_FOR: Duration = Duration::from_secs(8);
const WHY_VAD: &str = "Works with automatic sensitivity in voice activity mode";
const MAC_MIC_PRIVACY: &str = "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone";
// The account banner's gradient: oklch(0.6 0.14 300) to oklch(0.45 0.15 340).
const BANNER: (u32, u32) = (0x8E6AC7, 0x88296F);

const NOTIFY_CUES: [(&str, &str, Cue); 2] = [("cue-message", "Message", Cue::Message), ("cue-mention", "Mention", Cue::Mention)];
const VOICE_CUES: [(&str, &str, Cue); 4] = [("cue-deafen", "Deafen", Cue::Deafen), ("cue-undeafen", "Undeafen", Cue::Undeafen), ("cue-mute", "Mute", Cue::Mute), ("cue-unmute", "Unmute", Cue::Unmute)];
const MORE_CUES: [(&str, &str, Cue); 9] = [
    ("cue-disconnect", "Voice disconnected", Cue::Disconnect),
    ("cue-ptt-start", "PTT activate", Cue::PttStart),
    ("cue-ptt-stop", "PTT deactivate", Cue::PttStop),
    ("cue-user-join", "User join", Cue::UserJoin),
    ("cue-user-leave", "User leave", Cue::UserLeave),
    ("cue-stream-started", "Stream started", Cue::StreamStarted),
    ("cue-stream-stopped", "Stream stopped", Cue::StreamEnded),
    ("cue-viewer-join", "Viewer join", Cue::StreamUserJoined),
    ("cue-viewer-leave", "Viewer leave", Cue::StreamUserLeft),
];

// Built-in shortcuts, each as one or more combos read "or".
const DEFAULT_KEYBINDS: [(&str, &[&str]); 5] = [
    ("Quick switcher", &["cmd+k"]),
    ("Navigate between channels", &["alt+ArrowUp", "ArrowDown"]),
    ("Mark channel as read", &["escape"]),
    ("Previous and next in the image viewer", &["ArrowLeft", "ArrowRight"]),
    ("Close a popup", &["escape"]),
];

fn os_name() -> &'static str {
    match std::env::consts::OS {
        "macos" => "macOS",
        "windows" => "Windows",
        "linux" => "Linux",
        other => other,
    }
}

fn nav_item(page: Page, selected: bool, cx: &mut Context<Rustcord>) -> impl IntoElement {
    let (strong, hover) = (palette::strong(cx), palette::hover(cx));
    div()
        .id(SharedString::from(format!("nav-{page:?}")))
        .min_h(px(32.))
        .px(px(10.))
        .flex_none()
        .squircle(ui::RADIUS_SM)
        .flex()
        .items_center()
        .gap(px(10.))
        .cursor_pointer()
        .whitespace_nowrap()
        .font_weight(FontWeight::MEDIUM)
        .text_color(if selected { strong } else { palette::text(cx) })
        .when(selected, |d| d.bg(palette::selected(cx)))
        .when(!selected, |d| d.hover(move |s| s.bg(hover).text_color(strong)))
        .child(div().text_color(if selected { strong } else { palette::muted(cx) }).child(Icon::new(page.glyph()).size(px(18.))))
        .child(div().flex_1().min_w_0().truncate().child(page.label()))
        .on_click(cx.listener(move |this, _, window, cx| {
            if let Some(s) = &mut this.settings {
                (s.page, s.section, s.select, s.recording, s.asking) = (page, 0, None, None, None);
                s.scroll.set_offset(gpui_kit::point(px(0.), px(0.)));
            }
            this.settings_search.update(cx, |state, cx| state.set_value("", window, cx));
            cx.notify();
        }))
}

// The open page's sections, listed under it in the nav.
fn sub_items(page: Page, current: usize, anchors: &[usize], cx: &mut Context<Rustcord>) -> impl IntoElement {
    let (strong, muted, hover) = (palette::strong(cx), palette::muted(cx), palette::hover(cx));
    let anchors = anchors.to_vec();
    div().pt(px(2.)).pb(px(4.)).flex().flex_col().gap(px(1.)).children(page.sections().iter().enumerate().map(move |(i, name)| {
        let item = anchors.get(i).copied().unwrap_or(0);
        div()
            .id(SharedString::from(format!("sub-{name}")))
            .min_h(px(28.))
            .pl(px(38.))
            .pr(px(10.))
            .squircle(ui::RADIUS_SM)
            .flex()
            .items_center()
            .cursor_pointer()
            .font_weight(FontWeight::MEDIUM)
            .text_color(if i == current { strong } else { muted })
            .hover(move |s| s.bg(hover).text_color(strong))
            .child(*name)
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(s) = &mut this.settings {
                    s.section = i;
                    s.scroll.scroll_to_top_of_item(item);
                }
                cx.notify();
            }))
    }))
}

fn page_title(text: &'static str, cx: &App) -> Div {
    div().text_size(px(28.)).line_height(px(34.)).font_weight(FontWeight::BOLD).text_color(palette::strong(cx)).truncate().child(text)
}

fn card_pad(cx: &App) -> Div {
    ui::settings_card(cx).p(px(16.)).gap(px(16.))
}

// A whole-row control: the row fills on hover.
fn choice(row: Stateful<Div>, cx: &App) -> Stateful<Div> {
    let hover = palette::hover(cx);
    row.cursor_pointer().hover(move |s| s.bg(hover))
}

// A row whose control sits under its text, full width.
fn stack_row(id: &'static str, line: bool, cx: &App) -> Stateful<Div> {
    ui::settings_row(id, line, cx).flex_col().items_stretch().gap(px(12.))
}

fn row_text(title: impl Into<SharedString>, detail: Option<impl Into<SharedString>>, cx: &App) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(2.))
        .child(div().truncate().font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(title.into()))
        .children(detail.map(|d| div().text_size(px(12.)).line_height(px(17.)).text_color(palette::muted(cx)).child(d.into())))
}

// A label over its value, as Account info lists them.
fn field_text(label: &'static str, value: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(2.))
        .child(div().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(palette::muted(cx)).child(label))
        .child(div().font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(value.into()))
}

fn field_label(text: &'static str, cx: &App) -> Div {
    div().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(palette::muted(cx)).child(text)
}

// A title and description with the value it sets on the right.
fn numbered(title: &'static str, detail: &'static str, value: &str, cx: &App) -> Div {
    div().flex().items_start().justify_between().gap(px(16.)).child(row_text(title, Some(detail), cx)).child(div().whitespace_nowrap().font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(value.to_owned()))
}

fn scale(stops: &[&'static str], cx: &App) -> Div {
    div().flex().justify_between().text_size(px(12.)).text_color(palette::muted(cx)).children(stops.iter().copied())
}

fn radio_row(id: &'static str, line: bool, title: &'static str, detail: &'static str, on: bool, cx: &App) -> Stateful<Div> {
    choice(ui::settings_row(id, line, cx), cx).items_start().child(div().mt(px(2.)).child(ui::radio(on, cx))).child(row_text(title, Some(detail), cx))
}

// A row that reveals more rows under it; closed, it names what's inside.
#[allow(clippy::too_many_arguments)]
fn expander_row(id: &'static str, line: bool, title: &str, summary: &str, open: bool, window: &mut Window, cx: &mut App) -> Stateful<Div> {
    let angle = turn(format!("chevron-{id}"), if open { 90. } else { 0. }, window, cx);
    let chev = div()
        .size(px(32.))
        .flex_none()
        .squircle(ui::RADIUS_SM)
        .bg(palette::selected(cx))
        .text_color(palette::strong(cx))
        .flex()
        .items_center()
        .justify_center()
        .child(Icon::new(Glyph::ChevronRight).size(px(16.)).rotate(angle));
    choice(ui::settings_row(id, line, cx), cx).child(row_text(title.to_owned(), (!open).then(|| summary.to_owned()), cx)).child(chev)
}

fn error_line(text: &'static str, cx: &App) -> Div {
    div()
        .px(px(2.))
        .flex()
        .items_start()
        .gap(px(6.))
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(palette::danger_ink(cx))
        .child(div().mt(px(1.)).flex_none().child(Icon::new(Glyph::WarningCircle).size(px(14.))))
        .child(text)
}

// An icon's angle in degrees, eased over the pop duration whenever `degrees` changes.
fn turn(id: String, degrees: f32, window: &mut Window, cx: &mut App) -> Radians {
    let degrees = gpui_kit::base::transition(id, degrees, gpui_kit::base::Transition::new(ui::DUR_POP).easing(motion::ease_out()), window, cx);
    Radians(degrees.to_radians())
}

// The mic test draws your voice as the wave: 40 bars swelling from the middle, accent while live.
fn mic_wave(level_db: f32, live: bool, cx: &App) -> Div {
    const BARS: usize = 40;
    let level = ((level_db + 100.) / 100.).clamp(0., 1.);
    // Quieter than a whisper reads as silence, so the wave stays flat in a quiet room.
    let voice = ((level - 0.35) / 0.55).clamp(0., 1.);
    let (rest, on) = (palette::line_strong(cx), palette::accent());
    div().flex_1().min_w_0().h(px(40.)).flex().items_center().gap(px(3.)).children((0..BARS).map(move |i| {
        let shape = (i as f32 / BARS as f32 * std::f32::consts::PI).sin();
        let jitter = 0.6 + 0.6 * ((i as f32 * 12.9898 + level_db * 78.233).sin() * 0.5 + 0.5);
        let l = if live { (voice * shape * jitter).clamp(0.1, 1.) } else { 0.1 };
        div().flex_1().min_w(px(2.)).h(px(40. * l)).rounded_full().bg(if live && l > 0.15 { on } else { rest })
    }))
}

// The input sensitivity track: the live level lit over a bar split at the threshold, the handle on
// the threshold. With no threshold (automatic) the whole bar lights green up to the level.
fn sensitivity_track(threshold: Option<f32>, level_db: f32, cx: &App) -> Div {
    let level = ((level_db + 100.) / 100.).clamp(0., 1.);
    let (rest, online) = (palette::line_strong(cx), palette::status_in(crate::discord::Status::Online, cx));
    let part = |from: f32, to: f32, fill: Hsla| {
        let (from, to) = (from.clamp(0., 1.), to.clamp(0., 1.));
        div().absolute().top_0().bottom_0().left(relative(from)).w(relative((to - from).max(0.))).bg(fill).when(from == 0., |d| d.rounded_l_full()).when(to >= 1., |d| d.rounded_r_full())
    };
    let bar = div().absolute().left_0().right_0().top(px(8.)).h(px(8.));
    let bar = match threshold {
        None => bar.child(part(0., 1., rest)).child(part(0., level, online)),
        Some(th) => bar
            .child(part(0., th, rest))
            .child(part(th, 1., online.opacity(0.35)))
            .child(part(0., level.min(th), palette::faint(cx)))
            .when(level > th, |d| d.child(part(th, level, online))),
    };
    div()
        .relative()
        .w_full()
        .h(px(24.))
        .child(bar)
        .children(threshold.map(|th| div().absolute().top_0().left(relative(th.clamp(0., 1.))).ml(px(-3.)).w(px(6.)).h(px(24.)).rounded(px(3.)).bg(palette::accent())))
}

// The recorder's listening ring: 3px of accent tint 4px out, breathing.
fn pulse_ring(id: SharedString, cx: &App) -> AnyElement {
    let ring = div().absolute().top(px(-5.)).bottom(px(-5.)).left(px(-5.)).right(px(-5.)).squircle(ui::RADIUS_MD + 4.).border(px(3.)).border_color(palette::accent_soft(cx));
    if cx.reduce_motion() {
        return ring.into_any_element();
    }
    ring.with_animation(id, Animation::new(Duration::from_millis(2400)).repeat(), |ring, t| ring.opacity(0.2 + 0.8 * (0.5 - 0.5 * (t * std::f32::consts::TAU).cos())))
        .into_any_element()
}

// Content arriving in place: a fade down from 4px above, on the house ease-out.
fn reveal(id: impl Into<ElementId>, el: impl IntoElement) -> AnyElement {
    div()
        .relative()
        .child(el)
        .with_animation(id, Animation::new(ui::DUR_POP).with_easing(|t| motion::ease_out().sample(t)), |el, t| el.top(px(-4. * (1. - t))).opacity(t))
        .into_any_element()
}

fn ring(color: Hsla, width: f32) -> BoxShadow {
    BoxShadow { color, offset: point(px(0.), px(0.)), blur_radius: px(0.), spread_radius: px(width), inset: false }
}

fn inset_ring(color: Hsla, width: f32) -> BoxShadow {
    BoxShadow { inset: true, ..ring(color, width) }
}

// Keycaps for a recorded binding, with the platform's own key names.
fn key_caps(keys: &str, cx: &App) -> Div {
    div().flex().items_center().gap(px(4.)).children(keys.split('+').map(|key| ui::kbd(key_name(key), cx)))
}

fn combo_caps(combos: &[&str], cx: &App) -> Div {
    let muted = palette::muted(cx);
    div().flex().items_center().gap(px(4.)).flex_none().children(combos.iter().enumerate().flat_map(|(i, combo)| {
        let or = (i > 0).then(|| div().px(px(2.)).text_size(px(12.)).text_color(muted).child("or").into_any_element());
        or.into_iter().chain([key_caps(combo, cx).into_any_element()])
    }))
}

// ⌘ ⇧ ⌥ ⌃ on macOS, printed names elsewhere; arrows as arrows, letters upper-case.
fn key_name(key: &str) -> SharedString {
    let mac = cfg!(target_os = "macos");
    let name = match key {
        "cmd" if mac => "\u{2318}",
        "cmd" => "Win",
        "shift" if mac => "\u{21E7}",
        "shift" => "Shift",
        "alt" if mac => "\u{2325}",
        "alt" => "Alt",
        "ctrl" if mac => "\u{2303}",
        "ctrl" => "Ctrl",
        "ArrowUp" => "\u{2191}",
        "ArrowDown" => "\u{2193}",
        "ArrowLeft" => "\u{2190}",
        "ArrowRight" => "\u{2192}",
        "escape" => "Esc",
        "enter" => "Return",
        key => {
            let key = key.strip_prefix("Key").or_else(|| key.strip_prefix("Digit")).filter(|k| k.chars().count() == 1).unwrap_or(key);
            let mut chars = key.chars();
            return chars.next().map_or(SharedString::default(), |first| format!("{}{}", first.to_uppercase(), chars.as_str()).into());
        }
    };
    name.into()
}

// Discord's labels in Title Case, read as sentences.
fn sentence(label: &str) -> String {
    let mut chars = label.chars();
    chars.next().map_or(String::new(), |first| first.to_uppercase().chain(chars.as_str().to_lowercase().chars()).collect())
}

// A small window in each theme, on fixed colours, so a light preview reads light in the dark theme.
struct Mini {
    rail: u32,
    side: u32,
    page: u32,
    ink: u32,
    faint: u32,
}

const MINI_DARK: Mini = Mini { rail: 0x0A0B0D, side: 0x111213, page: 0x17181A, ink: 0xD6D7DA, faint: 0x4C4D4F };
const MINI_LIGHT: Mini = Mini { rail: 0xDEE0E3, side: 0xECEEF0, page: 0xF8F9FB, ink: 0x2D2E30, faint: 0xBCBEC0 };

fn mini_window(m: &Mini) -> Div {
    let bar = |color: u32, width: f32| div().h(px(4.)).w(relative(width)).rounded_full().bg(rgb(color));
    let rail = div()
        .w(relative(0.12))
        .h_full()
        .bg(rgb(m.rail))
        .pt(px(6.))
        .flex()
        .flex_col()
        .items_center()
        .gap(px(4.))
        .child(div().w(relative(0.6)).h(px(14.)).squircle(4.).bg(palette::accent()))
        .child(div().w(relative(0.6)).h(px(14.)).squircle(4.).bg(rgb(m.faint)));
    let side = div().w(relative(0.28)).h_full().bg(rgb(m.side)).py(px(8.)).px(px(6.)).flex().flex_col().gap(px(5.)).child(bar(m.faint, 1.)).child(bar(m.ink, 0.8)).child(bar(m.faint, 0.6)).child(bar(m.faint, 1.));
    let content = div()
        .flex_1()
        .h_full()
        .py(px(10.))
        .px(px(8.))
        .flex()
        .flex_col()
        .justify_end()
        .gap(px(5.))
        .child(bar(m.ink, 0.4))
        .child(bar(m.faint, 0.85))
        .child(bar(m.ink, 0.4))
        .child(bar(m.faint, 0.85))
        .child(div().mt(px(4.)).h(px(12.)).w_full().squircle(4.).bg(rgb(m.faint)));
    div().w_full().h(px(MINI_HEIGHT)).squircle(ui::RADIUS_MD).bg(rgb(m.page)).flex().child(rail).child(side).child(content)
}

const MINI_HEIGHT: f32 = 130.;

// Sync with OS: the light window on the left half, the dark one on the right.
// GAP: the design splits the two along the diagonal (clip-path). gpui clips only to rectangles, and a
// clip inside a rotated element is its axis-aligned box, so rotation can't cut the diagonal either.
fn mini_sync() -> AnyElement {
    let half = |left: bool, m: &Mini| {
        div()
            .absolute()
            .top_0()
            .bottom_0()
            .w(relative(0.5))
            .when(left, |d| d.left_0())
            .when(!left, |d| d.right_0())
            .overflow_hidden()
            .child(div().absolute().top_0().w(relative(2.)).when(left, |d| d.left_0()).when(!left, |d| d.right_0()).child(mini_window(m)))
    };
    div().relative().w_full().h(px(MINI_HEIGHT)).child(half(true, &MINI_LIGHT)).child(half(false, &MINI_DARK)).into_any_element()
}

// The collapsed accordion lists its first settings: three under Custom, two otherwise (Discord `tI`).
fn advanced_summary(custom: bool) -> String {
    let custom_only = ["Automatic gain control", "Advanced voice activity"];
    let always = ["No audio detected warning", "Global attenuation", "Quality of service"];
    let shown = custom_only.iter().filter(|_| custom).chain(always.iter().filter(|t| **t != "Global attenuation" || voice::ATTENUATION_SUPPORTED));
    shown.take(if custom { 3 } else { 2 }).copied().collect::<Vec<_>>().join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefs_round_trip_and_profiles() {
        let mut prefs = Prefs { push_to_talk: true, noise_mode: NoiseMode::Standard, ..Prefs::default() };
        prefs.set_profile(InputProfile::Studio);
        let studio = prefs.voice_settings();
        assert_eq!(studio.noise_suppression, voice::NoiseSuppression::Off);
        assert!(!studio.echo_cancellation && !studio.auto_gain);
        assert_eq!(studio.input_mode, voice::InputMode::VoiceActivity { auto: false, threshold_db: -84., advanced: false }, "Studio forces voice activity");
        prefs.set_profile(InputProfile::VoiceIsolation);
        assert_eq!(prefs.voice_settings().noise_suppression, voice::NoiseSuppression::Advanced);
        prefs.set_profile(InputProfile::Custom);
        assert_eq!(prefs.voice_settings().noise_suppression, voice::NoiseSuppression::Standard, "Custom gets its own switches back");
        assert!(matches!(prefs.voice_settings().input_mode, voice::InputMode::PushToTalk { .. }));
        prefs.push_to_talk = false;
        let text = serde_json::to_string(&prefs).unwrap();
        assert_eq!(serde_json::from_str::<Prefs>(&text).unwrap(), prefs);
        let mut empty = serde_json::from_str::<Prefs>("{}").unwrap();
        empty.add_new_keybinds();
        assert_eq!(empty, Prefs::default());
        Slider::Sensitivity.set(&mut prefs, 0.4);
        assert_eq!(prefs.sensitivity_db, -60.);
        assert!((Slider::Sensitivity.fraction(&prefs) - 0.4).abs() < 1e-6);
    }

    #[test]
    fn defaults_match_discord_and_old_files_migrate() {
        let settings = Prefs::default().voice_settings();
        assert_eq!(settings.input_mode, voice::InputMode::VoiceActivity { auto: true, threshold_db: -60., advanced: true });
        assert_eq!(settings.noise_suppression, voice::NoiseSuppression::Advanced);
        assert!(settings.echo_cancellation && settings.auto_gain && !settings.qos);
        assert_eq!(settings.attenuation, voice::Attenuation { strength: 0, when_self_speaks: false, when_others_speak: true });

        // A settings file from before this change: no attenuation, VAD, warning or QoS fields.
        let old = r#"{"theme":"Dark","input_profile":"Custom","sensitivity_auto":false,"sensitivity_db":-42.0,
            "noise_suppression":true,"noise_mode":"Standard","echo_cancellation":false,"auto_gain":true,"push_to_talk":false}"#;
        let prefs: Prefs = serde_json::from_str(old).unwrap();
        assert_eq!((prefs.sensitivity_db, prefs.noise_mode, prefs.echo_cancellation), (-42., NoiseMode::Standard, false), "kept");
        assert!(prefs.advanced_voice_activity && prefs.attenuate_others && prefs.silence_warning, "new fields take Discord's defaults");
        assert_eq!((prefs.attenuation, prefs.attenuate_self, prefs.qos), (0, false, false));
        assert_eq!(prefs.voice_settings().input_mode, voice::InputMode::VoiceActivity { auto: false, threshold_db: -42., advanced: true });
        let mut p = prefs;
        Slider::Attenuation.set(&mut p, 0.355);
        assert_eq!(p.attenuation, 36);
    }

    #[test]
    fn advanced_summary_lists_the_first_visible_settings() {
        assert_eq!(advanced_summary(true), "Automatic gain control, Advanced voice activity, No audio detected warning");
        let second = if voice::ATTENUATION_SUPPORTED { "Global attenuation" } else { "Quality of service" };
        assert_eq!(advanced_summary(false), format!("No audio detected warning, {second}"));
    }

    #[test]
    fn older_prefs_get_the_share_keybind_once() {
        let shares = |p: &Prefs| p.keybinds.iter().filter(|k| **k == share_keybind()).count();
        let mut old = serde_json::from_str::<Prefs>(r#"{"keybinds":[{"action":"ToggleMute","keys":"alt+m"}]}"#).unwrap();
        assert_eq!(old.keybinds_version, 0);
        old.add_new_keybinds();
        assert_eq!(shares(&old), 1);
        old.keybinds.retain(|k| k.action != Action::ToggleScreenShare);
        let mut reread = serde_json::from_str::<Prefs>(&serde_json::to_string(&old).unwrap()).unwrap();
        reread.add_new_keybinds();
        assert_eq!(shares(&reread), 0, "a deleted default stays deleted");
    }
}
