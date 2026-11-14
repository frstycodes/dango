// Discord's Settings: a modal 40px in from the window with the nav on the left and one page on the
// right, measured from Discord web (2026-11-14). Only settings Rustcord honours are shown.
use std::collections::HashMap;

use gpui_kit::{
    AnyElement, App, Context, Div, FontWeight, Hsla, MouseButton, Pixels, ScrollHandle, SharedString, Stateful, Window, canvas,
    component::{ActiveTheme, Icon, input::Input},
    div, img, prelude::*, px, rgb,
};
use serde::{Deserialize, Serialize};

use crate::{
    Rustcord,
    hotkeys::{self, Action, Keybind},
    icons::Glyph,
    palette::{self, wash},
    sounds::{self, Cue},
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
            Page::VoiceVideo => "Voice & Video",
            Page::Keybinds => "Keybinds",
            Page::Appearance => "Appearance",
        }
    }

    fn glyph(self) -> Glyph {
        match self {
            Page::Account => Glyph::User,
            Page::Notifications => Glyph::Bell,
            Page::VoiceVideo => Glyph::Mic,
            Page::Keybinds => Glyph::Keyboard,
            Page::Appearance => Glyph::Palette,
        }
    }

    // In-page anchors listed under the open page, as Discord does for Voice & Video.
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
    Noise,
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

    pub(crate) fn render_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let view = self.settings.as_ref()?;
        let theme = cx.theme().clone();
        let page = view.page;
        let content = match page {
            Page::Account => self.account_page(cx),
            Page::Notifications => self.notifications_page(cx),
            Page::VoiceVideo => self.voice_page(window, cx),
            Page::Keybinds => self.keybinds_page(cx),
            Page::Appearance => self.appearance_page(cx),
        };
        let scroll = self.settings.as_ref()?.scroll.clone();
        let header = div()
            .h(px(48.))
            .flex_none()
            .px_4()
            .flex()
            .items_center()
            .justify_between()
            .border_b_1()
            .border_color(theme.border)
            .child(div().text_size(px(16.)).font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(page.label()))
            .child(
                icon_button("settings-close", Glyph::X, cx).on_click(cx.listener(|this, _, _, cx| this.close_settings(cx))),
            );
        let body = div()
            .id("settings-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&scroll)
            .pt(px(32.))
            .pb(px(80.))
            .children(content.into_iter().map(|section| div().w_full().max_w(px(696.)).mx_auto().px_4().flex().flex_col().child(section)));
        let modal = div()
            .id("settings")
            .absolute()
            .inset(px(40.))
            .rounded(px(12.))
            .border_1()
            .border_color(wash(0.2))
            .bg(theme.background)
            .overflow_hidden()
            .flex()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(self.settings_nav(cx))
            .child(div().flex_1().min_w_0().flex().flex_col().child(header).child(body));
        Some(
            div()
                .id("settings-backdrop")
                .absolute()
                .inset_0()
                .occlude()
                .bg(gpui_kit::black().opacity(0.7))
                .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.close_settings(cx)))
                .child(modal)
                .into_any_element(),
        )
    }

    fn settings_nav(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let view = self.settings.as_ref().expect("settings are open");
        let query = self.settings_search.read(cx).value().to_lowercase();
        let me = self.me.clone();
        let profile = me.map(|me| {
            div()
                .flex()
                .items_center()
                .gap_3()
                .px_2()
                .child(img(me.avatar.clone()).size(px(48.)).rounded_full().flex_none())
                .child(
                    div()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(div().truncate().text_size(px(16.)).font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(me.name.clone()))
                        .child(div().text_size(px(14.)).text_color(theme.muted_foreground).child(self.my_status.label())),
                )
        });
        let search = div()
            .h(px(40.))
            .px_3()
            .rounded(px(8.))
            .border_1()
            .border_color(wash(0.24))
            .bg(field_fill(cx))
            .flex()
            .items_center()
            .gap_2()
            .child(Icon::new(Glyph::Search).size(px(16.)).text_color(theme.muted_foreground))
            .child(div().flex_1().child(Input::new(&self.settings_search).appearance(false)));
        let mut groups: Vec<AnyElement> = Vec::new();
        for (section, pages) in NAV {
            let pages: Vec<Page> = pages.iter().copied().filter(|p| query.is_empty() || p.label().to_lowercase().contains(&query)).collect();
            if pages.is_empty() {
                continue;
            }
            let mut group = div().flex().flex_col().gap_1().pb_2();
            if let Some(section) = section {
                group = group.child(div().h(px(30.)).px_2().flex().items_center().text_size(px(14.)).font_weight(FontWeight::MEDIUM).text_color(theme.muted_foreground).child(section));
            }
            for page in pages {
                group = group.child(nav_item(page, view.page == page, cx));
                if view.page == page {
                    group = group.children((!page.sections().is_empty()).then(|| sub_items(page, view.section, cx)));
                }
            }
            groups.push(group.into_any_element());
        }
        let log_out = div()
            .id("nav-log-out")
            .h(px(36.))
            .p_2()
            .rounded(px(8.))
            .flex()
            .items_center()
            .gap_3()
            .cursor_pointer()
            .text_color(theme.danger)
            .font_weight(FontWeight::MEDIUM)
            .hover(|s| s.bg(wash(0.08)))
            .child(Icon::new(Glyph::DoorExit).size(px(20.)))
            .child("Log Out")
            .on_click(cx.listener(|this, _, _, cx| this.sign_out(cx)));
        div()
            .id("settings-nav")
            .w(px(253.))
            .flex_none()
            .h_full()
            .overflow_y_scroll()
            .border_r_1()
            .border_color(theme.border)
            .pt(px(24.))
            .pb_4()
            .px_4()
            .flex()
            .flex_col()
            .gap_4()
            .children(profile)
            .child(search)
            .child(div().flex().flex_col().children(groups).child(div().h(px(1.)).mx_2().mb_2().bg(theme.border)).child(log_out))
    }

    fn account_page(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = cx.theme().clone();
        let Some(me) = self.me.clone() else { return Vec::new() };
        let row = |label: &'static str, value: SharedString| {
            div()
                .min_h(px(56.))
                .flex()
                .items_center()
                .gap_4()
                .child(div().w(px(160.)).flex_none().text_size(px(16.)).font_weight(FontWeight::MEDIUM).child(label))
                .child(div().flex_1().min_w_0().truncate().text_color(theme.muted_foreground).child(value))
        };
        vec![
            h2("Account Info", cx),
            div()
                .mb_4()
                .p_4()
                .rounded(px(16.))
                .bg(theme.muted)
                .flex()
                .items_center()
                .gap_4()
                .child(img(me.avatar.clone()).size(px(80.)).rounded_full())
                .child(div().text_size(px(20.)).font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(me.name.clone()))
                .into_any_element(),
            row("Display Name", me.name.clone().into()).into_any_element(),
            row("User ID", me.id.clone().into()).into_any_element(),
        ]
    }

    fn notifications_page(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut out = vec![h2("Sounds", cx), description("Sounds Wave plays for new messages in direct messages and for mentions.", cx).mb_4().into_any_element()];
        out.extend([("Message", Cue::Message), ("Mention", Cue::Mention)].into_iter().map(|(label, cue)| self.cue_row(label, cue, cx)));
        out
    }

    fn appearance_page(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = cx.theme().clone();
        let current = self.prefs.theme;
        let swatch = |id: &'static str, label: &'static str, pref: ThemePref, fill: AnyElement| {
            let selected = current == pref;
            div()
                .id(id)
                .flex()
                .flex_col()
                .items_center()
                .gap_2()
                .cursor_pointer()
                .child(
                    div()
                        .relative()
                        .size(px(60.))
                        .rounded_full()
                        .border_2()
                        .border_color(if selected { palette::accent() } else { wash(0.24) })
                        .p(px(3.))
                        .child(fill)
                        .children(selected.then(|| {
                            div()
                                .absolute()
                                .top(px(-4.))
                                .right(px(-4.))
                                .size(px(20.))
                                .rounded_full()
                                .bg(palette::accent())
                                .text_color(gpui_kit::white())
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(Icon::new(Glyph::Checkmark).size(px(14.)))
                        })),
                )
                .child(div().text_size(px(14.)).text_color(theme.muted_foreground).child(label))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.update_prefs(cx, |prefs| prefs.theme = pref);
                    palette::sync(window, cx);
                }))
        };
        let disc = |color: Hsla| div().size_full().rounded_full().bg(color).border_1().border_color(wash(0.3)).into_any_element();
        let split = div()
            .size_full()
            .rounded_full()
            .overflow_hidden()
            .flex()
            .child(div().w_1_2().h_full().rounded_l_full().bg(gpui_kit::white()))
            .child(div().w_1_2().h_full().rounded_r_full().bg(gpui_kit::black()))
            .into_any_element();
        vec![
            h2("Theme", cx),
            description("Midnight is Discord's darkest theme, the one Wave is built on.", cx).mb_4().into_any_element(),
            div()
                .flex()
                .gap_6()
                .child(swatch("theme-light", "Light", ThemePref::Light, disc(gpui_kit::white())))
                .child(swatch("theme-dark", "Midnight", ThemePref::Dark, disc(gpui_kit::black())))
                .child(swatch("theme-system", "Sync with OS", ThemePref::System, split))
                .into_any_element(),
            divider(&theme).into_any_element(),
            h2("App Icon", cx),
            description("How Wave looks in your Dock. The same voice wave in different media.", cx).mb_4().into_any_element(),
            self.app_icon_grid(cx),
        ]
    }

    fn app_icon_grid(&self, cx: &mut Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let current = self.prefs.app_icon;
        let tiles = crate::app_icon::AppIcon::ALL.map(|icon| {
            let selected = icon == current;
            div()
                .id(SharedString::from(format!("app-icon-{}", icon.label())))
                .flex()
                .flex_col()
                .items_center()
                .gap_2()
                .cursor_pointer()
                .child(
                    div()
                        .size(px(76.))
                        .rounded(px(20.))
                        .border_2()
                        .border_color(if selected { palette::accent() } else { gpui_kit::transparent_black() })
                        .p(px(3.))
                        .hover(|s| s.bg(wash(0.12)))
                        .child(img(icon.asset()).size_full()),
                )
                .child(div().text_size(px(13.)).text_color(if selected { palette::strong(cx) } else { muted }).child(icon.label()))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.update_prefs(cx, |prefs| prefs.app_icon = icon);
                    icon.apply();
                }))
        });
        div().flex().flex_wrap().gap_4().children(tiles).into_any_element()
    }

    fn voice_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = cx.theme().clone();
        let prefs = self.prefs.clone();
        let view = self.settings.as_ref().expect("settings are open");
        let (inputs, outputs, cameras) = (view.devices.inputs.clone(), view.devices.outputs.clone(), view.devices.cameras.clone());
        let (more_sounds, advanced) = (view.more_sounds, view.advanced);
        let ptt_index = prefs.keybinds.iter().position(|k| k.action == Action::PushToTalk);

        let devices = div()
            .flex()
            .gap_4()
            .child(div().flex_1().flex().flex_col().gap_2().child(label("Microphone", cx)).child(self.device_select(Select::Microphone, &inputs, prefs.input_device.as_deref(), cx)))
            .child(div().flex_1().flex().flex_col().gap_2().child(label("Speaker", cx)).child(self.device_select(Select::Speaker, &outputs, prefs.output_device.as_deref(), cx)));
        let volumes = div()
            .mt_6()
            .flex()
            .gap_4()
            .child(div().flex_1().flex().flex_col().gap_2().child(label("Microphone Volume", cx)).child(self.slider(Slider::InputVolume, None, cx)))
            .child(div().flex_1().flex().flex_col().gap_2().child(label("Speaker Volume", cx)).child(self.slider(Slider::OutputVolume, None, cx)));
        let level = self.mic_level;
        let testing = self.mic_test.is_some();
        // Discord's mic test notice, and in a call its "No Audio Detected" warning when that's on.
        let in_call_silent = prefs.silence_warning && self.call.as_ref().is_some_and(|c| c.session.input_detected() == Some(false));
        let no_input = if testing { view.mic_input == Some(false) } else { in_call_silent };
        let speaking = if testing { view.mic_speaking } else { self.call.as_ref().is_some_and(|c| c.level.1) };
        let mic_test = div()
            .mt_6()
            .flex()
            .items_center()
            .gap_4()
            .child(primary_button("mic-test", if testing { "Stop Testing" } else { "Mic Test" }).on_click(cx.listener(|this, _, _, cx| this.toggle_mic_test(cx))))
            .child(meter(level, cx));
        let no_input_notice = no_input.then(|| {
            div()
                .mt_2()
                .flex()
                .flex_col()
                .child(label("Wave isn\u{2019}t detecting any input from your mic.", cx))
                .child(description("Please check that you\u{2019}ve selected the right input device.", cx))
        });
        let profile = |id: &'static str, title: &'static str, detail: &'static str, value: InputProfile, cx: &mut Context<Self>| {
            radio_row(id, title, detail, prefs.input_profile == value, cx).on_click(cx.listener(move |this, _, _, cx| this.update_prefs(cx, |p| p.set_profile(value))))
        };
        let custom = prefs.input_profile == InputProfile::Custom;
        let studio = prefs.input_profile == InputProfile::Studio;
        let sensitivity = self.sensitivity_section(speaking, level, cx);
        let ptt = (prefs.push_to_talk && !studio).then(|| {
            div()
                .flex()
                .gap_4()
                .child(
                    div().flex_1().flex().flex_col().gap_2().child(label("Shortcut", cx)).children(ptt_index.map(|i| self.keybind_recorder(i, cx))),
                )
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(label("Push to Talk Release Delay", cx))
                        .child(self.slider(Slider::ReleaseDelay, None, cx))
                        .child(description(&format!("{} ms", prefs.ptt_release_ms), cx)),
                )
        });
        let advanced_rows = advanced.then(|| self.advanced_voice_rows(custom, cx));
        let voice = div()
            .flex()
            .flex_col()
            .child(h2("Voice", cx))
            .child(devices)
            .child(volumes)
            .child(mic_test)
            .children(no_input_notice)
            .child(divider(&theme))
            .child(label("Input Profile", cx))
            .child(
                div()
                    .mt_2()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(profile("profile-isolation", "Voice Isolation", "Just your beautiful voice: let Wave cut through the noise", InputProfile::VoiceIsolation, cx))
                    .child(profile("profile-studio", "Studio", "Pure audio: open mic with no processing", InputProfile::Studio, cx))
                    .child(profile("profile-custom", "Custom", "Advanced mode: give me all the buttons and dials!", InputProfile::Custom, cx)),
            )
            .children(custom.then_some(sensitivity))
            .child(
                div()
                    .mt_4()
                    .flex()
                    .flex_col()
                    .when(custom, |d| {
                        d.child(self.noise_row(cx))
                            .child(self.toggle_row("echo", "Echo Cancellation", None, prefs.echo_cancellation, |p| &mut p.echo_cancellation, cx))
                    })
                    .when(!studio, |d| d.child(self.toggle_row("ptt", "Push to Talk", None, prefs.push_to_talk, |p| &mut p.push_to_talk, cx)))
                    .children(ptt),
            )
            .child(expander("voice-advanced", if advanced { "Hide Advanced Voice Settings" } else { "Show Advanced Voice Settings" }, &advanced_summary(custom), advanced, &theme, cx).on_click(cx.listener(|this, _, _, cx| {
                if let Some(s) = &mut this.settings {
                    s.advanced = !s.advanced;
                }
                cx.notify();
            })))
            .children(advanced_rows)
            .child(divider(&theme));
        let camera = div()
            .flex()
            .flex_col()
            .child(h2("Camera", cx))
            .child(div().flex().flex_col().gap_2().child(label("Camera", cx)).child(self.device_select(Select::Camera, &cameras, prefs.camera.as_deref(), cx)))
            .child(divider(&theme));
        let first = [("Deafen", Cue::Deafen), ("Undeafen", Cue::Undeafen), ("Mute", Cue::Mute), ("Unmute", Cue::Unmute)];
        let rest = [
            ("Voice Disconnected", Cue::Disconnect),
            ("PTT Activate", Cue::PttStart),
            ("PTT Deactivate", Cue::PttStop),
            ("User Join", Cue::UserJoin),
            ("User Leave", Cue::UserLeave),
            ("Stream Started", Cue::StreamStarted),
            ("Stream Stopped", Cue::StreamEnded),
            ("Viewer Join", Cue::StreamUserJoined),
            ("Viewer Leave", Cue::StreamUserLeft),
        ];
        let sounds_section = div()
            .flex()
            .flex_col()
            .child(h2("Sounds", cx))
            .children(first.into_iter().map(|(l, c)| self.cue_row(l, c, cx)))
            .child(
                expander("more-sounds", &format!("Show {} more sounds", rest.len()), "Voice Disconnected, PTT Activate, User Join, and more", more_sounds, &theme, cx).on_click(
                    cx.listener(|this, _, _, cx| {
                        if let Some(s) = &mut this.settings {
                            s.more_sounds = !s.more_sounds;
                        }
                        cx.notify();
                    }),
                ),
            )
            .children(more_sounds.then(|| rest.into_iter().map(|(l, c)| self.cue_row(l, c, cx)).collect::<Vec<_>>()).into_iter().flatten())
            .child(divider(&theme));
        let advanced_section = div()
            .flex()
            .flex_col()
            .child(h2("Advanced", cx))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_4()
                    .child(div().flex_1().flex().flex_col().gap_1().child(label("Reset all Voice & Video settings", cx)).child(description("Return to the default set of Voice & Video settings.", cx)))
                    .child(danger_button("reset-voice", "Reset").on_click(cx.listener(|this, _, _, cx| {
                        this.update_prefs(cx, |p| {
                            let keep = (p.theme, p.keybinds.clone());
                            *p = Prefs::default();
                            (p.theme, p.keybinds) = keep;
                        })
                    }))),
            );
        let _ = window;
        vec![voice.into_any_element(), camera.into_any_element(), sounds_section.into_any_element(), advanced_section.into_any_element()]
    }

    fn keybinds_page(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = cx.theme().clone();
        let header = div()
            .flex()
            .items_start()
            .gap_4()
            .child(div().flex_1().flex().flex_col().gap_2().child(h2("Keybinds", cx)).child(description("Shortcuts that work everywhere, even while another app is in front.", cx)))
            .child(primary_button("add-keybind", "Add a Keybind").on_click(cx.listener(|this, _, _, cx| {
                this.update_prefs(cx, |p| p.keybinds.push(Keybind { action: Action::Unassigned, keys: None }));
                let index = this.prefs.keybinds.len() - 1;
                if let Some(s) = &mut this.settings {
                    s.recording = Some(index);
                }
            })));
        let mut out = vec![header.into_any_element()];
        for (index, keybind) in self.prefs.keybinds.iter().enumerate() {
            out.push(self.keybind_row(index, keybind, cx));
        }
        out.push(divider(&theme).into_any_element());
        out.push(h2("Default Keybinds", cx));
        let defaults = [
            ("Quick Switcher", if cfg!(target_os = "macos") { "CMD + K" } else { "CTRL + K" }),
            ("Navigate Between Channels", if cfg!(target_os = "macos") { "OPTION + UP / DOWN" } else { "ALT + UP / DOWN" }),
            ("Mark Channel as Read", "ESCAPE"),
            ("Previous and Next in the Image Viewer", "LEFT / RIGHT"),
            ("Close a Popup", "ESCAPE"),
        ];
        out.extend(defaults.into_iter().map(|(action, keys)| {
            div()
                .h(px(48.))
                .flex()
                .items_center()
                .justify_between()
                .border_b_1()
                .border_color(theme.border)
                .child(div().text_size(px(16.)).child(action))
                .child(key_caps(keys, &theme))
                .into_any_element()
        }));
        out
    }

    fn keybind_row(&self, index: usize, keybind: &Keybind, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let refused = keybind.keys.as_ref().is_some_and(|k| self.keybind_errors.contains(k));
        let column = |title: &'static str, field: AnyElement| {
            div().flex_1().min_w_0().flex().flex_col().gap_2().child(div().text_size(px(12.)).font_weight(FontWeight::BOLD).text_color(theme.muted_foreground).child(title)).child(field)
        };
        let actions: Vec<(String, String)> = Action::ALL.iter().map(|a| (format!("{a:?}"), a.label().to_owned())).collect();
        let action = self.select_field(Select::Action(index), &actions, &format!("{:?}", keybind.action), keybind.action.label().into(), cx);
        div()
            .id(SharedString::from(format!("keybind-{index}")))
            .group("keybind")
            .relative()
            .mt_4()
            .p_4()
            .rounded(px(8.))
            .border_1()
            .border_color(wash(0.2))
            .bg(theme.muted)
            .flex()
            .flex_col()
            .gap_2()
            .child(div().flex().gap_4().child(column("ACTION", action)).child(column("KEYBIND", self.keybind_recorder(index, cx).into_any_element())))
            .children(refused.then(|| div().text_size(px(14.)).text_color(theme.danger).child("Another app already uses this shortcut.")))
            .child(
                div()
                    .id(SharedString::from(format!("keybind-delete-{index}")))
                    .absolute()
                    .top(px(-10.))
                    .right(px(-10.))
                    .size(px(24.))
                    .rounded_full()
                    .border_1()
                    .border_color(theme.danger)
                    .bg(theme.background)
                    .text_color(theme.danger)
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .invisible()
                    .group_hover("keybind", |s| s.visible())
                    .child(Icon::new(Glyph::X).size(px(14.)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(s) = &mut this.settings {
                            s.recording = None;
                        }
                        this.update_prefs(cx, |p| {
                            if index < p.keybinds.len() {
                                p.keybinds.remove(index);
                            }
                        });
                    })),
            )
            .into_any_element()
    }

    // Discord's recorder: the binding in caps, a button that toggles recording, red while it listens.
    fn keybind_recorder(&self, index: usize, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme().clone();
        let recording = self.settings.as_ref().and_then(|s| s.recording) == Some(index);
        let keys = self.prefs.keybinds.get(index).and_then(|k| k.keys.clone());
        let text: SharedString = match (&keys, recording) {
            (_, true) => "Press a key…".into(),
            (Some(keys), _) => hotkeys::display(keys).into(),
            (None, _) => "No Keybind Set".into(),
        };
        let accent = if recording { theme.danger } else { wash(0.24) };
        div()
            .h(px(40.))
            .pl_3()
            .pr_1()
            .rounded(px(8.))
            .border_1()
            .border_color(accent)
            .bg(field_fill(cx))
            .when(recording, |d| d.shadow_md())
            .flex()
            .items_center()
            .gap_2()
            .child(div().flex_1().min_w_0().truncate().text_size(px(14.)).font_weight(FontWeight::BOLD).text_color(if keys.is_some() || recording { theme.foreground } else { theme.muted_foreground }).child(text))
            .child(
                div()
                    .id(SharedString::from(format!("record-{index}")))
                    .h(px(32.))
                    .px_3()
                    .rounded(px(6.))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .text_size(px(14.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(gpui_kit::white())
                    .bg(if recording { theme.danger } else { track(cx) })
                    .hover(|s| s.opacity(0.9))
                    .child(if recording { "Stop Recording" } else if keys.is_some() { "Edit Keybind" } else { "Record Keybind" })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if let Some(s) = &mut this.settings {
                            s.recording = if s.recording == Some(index) { None } else { Some(index) };
                        }
                        this.focus.focus(window, cx);
                        cx.notify();
                    })),
            )
    }

    // Discord's input sensitivity: automatic shows a bar lit while the gate is open, manual a threshold
    // slider over the live level. Both are disabled under push to talk.
    fn sensitivity_section(&self, speaking: bool, level: f32, cx: &mut Context<Self>) -> Div {
        let prefs = &self.prefs;
        let (auto, enabled) = (prefs.sensitivity_auto, !prefs.push_to_talk);
        let detail = if auto {
            "Controls how much sound Wave transmits from your mic. Speak to test your mic \u{2013} if the meter is filled, Wave is transmitting your beautiful voice."
        } else {
            "Controls how much sound Wave transmits from your mic."
        };
        let control = if auto {
            speaking_bar(speaking && enabled, cx).into_any_element()
        } else {
            div().flex().flex_col().gap_2().child(self.slider(Slider::Sensitivity, Some(level), cx)).child(description(&format!("{} dB", prefs.sensitivity_db as i32), cx)).into_any_element()
        };
        div()
            .mt_6()
            .flex()
            .flex_col()
            .gap_2()
            .child(label("Input Sensitivity", cx))
            .child(self.switch_row("auto-sensitivity", "Automatically Adjust Input Sensitivity", Some(detail), auto, enabled, |p| &mut p.sensitivity_auto, cx))
            .child(div().when(!enabled, |d| d.opacity(0.5)).child(control))
    }

    // Discord's "Show Advanced Voice Settings" rows, in its order; the first two only under Custom.
    fn advanced_voice_rows(&self, custom: bool, cx: &mut Context<Self>) -> Div {
        let prefs = &self.prefs;
        let vad_enabled = !prefs.push_to_talk && prefs.sensitivity_auto;
        let attenuation = voice::ATTENUATION_SUPPORTED.then(|| {
            div()
                .py_3()
                .flex()
                .flex_col()
                .gap_2()
                .child(label("Global Attenuation", cx))
                .child(description("Lower the volume of other applications by this percent when someone is speaking. Set to 0% to completely disable global attenuation.", cx))
                .child(label("Attenuation Strength", cx))
                .child(self.slider(Slider::Attenuation, None, cx))
                .child(description(&format!("{}%", prefs.attenuation), cx))
                .child(self.toggle_row("attenuate-self", "When I speak", None, prefs.attenuate_self, |p| &mut p.attenuate_self, cx))
                .child(self.toggle_row("attenuate-others", "When others speak", None, prefs.attenuate_others, |p| &mut p.attenuate_others, cx))
        });
        div()
            .flex()
            .flex_col()
            .when(custom, |d| {
                d.child(self.toggle_row(
                    "agc",
                    "Automatic Gain Control",
                    Some("Automatically adjust microphone volume to keep it clear and consistent."),
                    prefs.auto_gain,
                    |p| &mut p.auto_gain,
                    cx,
                ))
                .child(self.switch_row(
                    "advanced-vad",
                    "Advanced Voice Activity",
                    Some("Turning this off may help if your voice is not being detected by the automatic input sensitivity."),
                    prefs.advanced_voice_activity,
                    vad_enabled,
                    |p| &mut p.advanced_voice_activity,
                    cx,
                ))
            })
            .child(self.toggle_row(
                "silence-warning",
                "No Audio Detected Warning",
                Some("Show a warning when Wave is not detecting audio from your mic"),
                prefs.silence_warning,
                |p| &mut p.silence_warning,
                cx,
            ))
            .children(attenuation)
            .child(self.toggle_row(
                "qos",
                "Quality of Service",
                Some("Hints to your router that the packets Wave is transmitting are high priority. Some routers or internet service providers may misbehave when this is set."),
                prefs.qos,
                |p| &mut p.qos,
                cx,
            ))
    }

    // Discord's Noise Suppression dropdown (Krisp / Standard / None). Krisp is licensed to Discord, so
    // the first option is DeepFilterNet, an open neural suppressor, in its place.
    fn noise_row(&self, cx: &mut Context<Self>) -> AnyElement {
        let options: Vec<(String, String)> = vec![("advanced".into(), "Advanced".into()), ("standard".into(), "Standard".into()), ("off".into(), "None".into())];
        let chosen = match (self.prefs.noise_suppression, self.prefs.noise_mode) {
            (false, _) => "off",
            (true, NoiseMode::Advanced) => "advanced",
            (true, NoiseMode::Standard) => "standard",
        };
        let current = options.iter().find(|(id, _)| id == chosen).map(|(_, l)| l.clone()).unwrap_or_default();
        let degraded = chosen == "advanced" && self.call.as_ref().is_some_and(|c| c.session.noise_suppression_degraded());
        let detail = if degraded {
            "Your computer couldn't run DeepFilterNet in real time, so this call uses a lighter suppressor."
        } else {
            "Reduces background noise from your mic. Powered by DeepFilterNet."
        };
        div()
            .py_3()
            .flex()
            .items_center()
            .gap_4()
            .child(div().flex_1().flex().flex_col().gap_1().child(label("Noise Suppression", cx)).child(description(detail, cx)))
            .child(div().w(px(220.)).child(self.select_field(Select::Noise, &options, chosen, current.into(), cx)))
            .into_any_element()
    }

    fn cue_row(&self, title: &'static str, cue: Cue, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let on = !self.prefs.silenced_cues.contains(&cue);
        div()
            .id(SharedString::from(format!("cue-{title}")))
            .py_3()
            .flex()
            .items_center()
            .gap_4()
            .child(
                div().flex_1().flex().flex_col().gap_1().child(label(title, cx)).child(
                    div()
                        .id(SharedString::from(format!("preview-{title}")))
                        .text_size(px(14.))
                        .text_color(theme.link)
                        .cursor_pointer()
                        .hover(|s| s.underline())
                        .child("Preview Sound")
                        .on_click(move |_, _, _| sounds::preview(cue)),
                ),
            )
            .child(switch(on, &theme))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.update_prefs(cx, |p| match p.silenced_cues.iter().position(|c| *c == cue) {
                    Some(i) => {
                        p.silenced_cues.remove(i);
                    }
                    None => p.silenced_cues.push(cue),
                })
            }))
            .into_any_element()
    }

    fn toggle_row(
        &self,
        id: &'static str,
        title: &'static str,
        detail: Option<&'static str>,
        on: bool,
        field: fn(&mut Prefs) -> &mut bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        self.switch_row(id, title, detail, on, true, field, cx)
    }

    // A toggle row that can be disabled (dimmed, not clickable), as Discord greys out settings that
    // don't apply in the current mode.
    #[allow(clippy::too_many_arguments)]
    fn switch_row(
        &self,
        id: &'static str,
        title: &'static str,
        detail: Option<&'static str>,
        on: bool,
        enabled: bool,
        field: fn(&mut Prefs) -> &mut bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = cx.theme().clone();
        let row = div()
            .id(id)
            .py_3()
            .flex()
            .items_center()
            .gap_4()
            .child(div().flex_1().flex().flex_col().gap_1().child(label(title, cx)).children(detail.map(|d| description(d, cx))))
            .child(switch(on, &theme));
        if !enabled {
            return row.opacity(0.5);
        }
        row.cursor_pointer().on_click(cx.listener(move |this, _, _, cx| {
            this.update_prefs(cx, |p| {
                let value = field(p);
                *value = !*value;
            })
        }))
    }

    fn device_select(&self, select: Select, devices: &[(String, String)], chosen: Option<&str>, cx: &mut Context<Self>) -> AnyElement {
        let mut options = vec![(String::new(), "Default".to_owned())];
        options.extend(devices.iter().cloned());
        let value = chosen.and_then(|id| devices.iter().find(|(d, _)| d == id)).map_or("Default".to_owned(), |(_, name)| name.clone());
        self.select_field(select, &options, chosen.unwrap_or(""), value.into(), cx)
    }

    // Discord's select: a 40px field with a chevron; the list opens below it and closes on pick.
    fn select_field(&self, select: Select, options: &[(String, String)], chosen: &str, value: SharedString, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let open = self.settings.as_ref().and_then(|s| s.select) == Some(select);
        let field = div()
            .id(SharedString::from(format!("select-{select:?}")))
            .h(px(40.))
            .pl_3()
            .pr_2()
            .rounded(px(8.))
            .border_1()
            .border_color(if open { palette::accent() } else { wash(0.24) })
            .bg(field_fill(cx))
            .flex()
            .items_center()
            .gap_2()
            .cursor_pointer()
            .child(div().flex_1().min_w_0().truncate().font_weight(FontWeight::MEDIUM).child(value))
            .child(Icon::new(Glyph::ChevronDown).size(px(18.)))
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(s) = &mut this.settings {
                    s.select = if s.select == Some(select) { None } else { Some(select) };
                }
                cx.notify();
            }));
        let list = open.then(|| {
            let rows: Vec<AnyElement> = options
                .iter()
                .map(|(id, name)| {
                    let (id, selected) = (id.clone(), id == chosen);
                    div()
                        .id(SharedString::from(format!("option-{select:?}-{id}")))
                        .h(px(40.))
                        .px_3()
                        .rounded(px(4.))
                        .flex()
                        .items_center()
                        .gap_2()
                        .cursor_pointer()
                        .when(selected, |d| d.bg(wash(0.2)))
                        .hover(|s| s.bg(wash(0.12)))
                        .child(div().flex_1().min_w_0().truncate().child(name.clone()))
                        .children(selected.then(|| Icon::new(Glyph::Checkmark).size(px(16.)).text_color(palette::accent())))
                        .on_click(cx.listener(move |this, _, _, cx| this.choose(select, &id, cx)))
                        .into_any_element()
                })
                .collect();
            div()
                .id(SharedString::from(format!("options-{select:?}")))
                .absolute()
                .top(px(44.))
                .left_0()
                .right_0()
                .max_h(px(240.))
                .overflow_y_scroll()
                .p_1()
                .rounded(px(8.))
                .border_1()
                .border_color(wash(0.24))
                .bg(theme.popover)
                .shadow_lg()
                .occlude()
                .children(rows)
        });
        // Deferred so the open list paints above the settings below it.
        div().relative().child(field).children(list.map(|list| gpui_kit::deferred(list).with_priority(2))).into_any_element()
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
            Select::Noise => match id {
                "off" => p.noise_suppression = false,
                "standard" => (p.noise_suppression, p.noise_mode) = (true, NoiseMode::Standard),
                _ => (p.noise_suppression, p.noise_mode) = (true, NoiseMode::Advanced),
            },
            Select::Action(index) => {
                let action = Action::ALL.into_iter().find(|a| format!("{a:?}") == id).unwrap_or(Action::Unassigned);
                if let Some(bind) = p.keybinds.get_mut(index) {
                    bind.action = action;
                }
            }
        });
    }

    // Discord's slider: 4px track, accent fill, 16px white knob. The sensitivity slider instead paints
    // below-threshold yellow and above green, with the live mic level over it.
    fn slider(&self, slider: Slider, level: Option<f32>, cx: &mut Context<Self>) -> AnyElement {
        let fill = match slider {
            Slider::Sensitivity => Fill::Threshold(level),
            _ => Fill::Accent,
        };
        self.range(format!("{slider:?}").into(), slider.fraction(&self.prefs), fill, move |this, f, cx| this.update_prefs(cx, |p| slider.set(p, f)), cx)
    }

    // Discord's slider: 4px track, accent fill, 16px white knob; click or drag anywhere on it.
    pub(crate) fn range(
        &self,
        key: SharedString,
        fraction: f32,
        fill: Fill,
        on_set: impl Fn(&mut Rustcord, f32, &mut Context<Rustcord>) + Clone + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        let bounds = self.slider_bounds.clone();
        let record = bounds.clone();
        let bar = || div().absolute().left_0().right_0().top(px(10.)).h(px(4.)).rounded(px(4.));
        let part = |to: f32| div().absolute().left_0().top_0().h_full().w(gpui_kit::relative(to.clamp(0., 1.)));
        let track = match fill {
            // Below the threshold is yellow (gated), above green, with the live mic level shading it.
            Fill::Threshold(level) => bar()
                .bg(theme.success)
                .child(part(fraction).bg(theme.warning))
                .children(level.map(|db| part((db + 100.) / 100.).bg(gpui_kit::black().opacity(0.35)))),
            Fill::Accent => bar().bg(track(cx)).child(part(fraction).rounded(px(4.)).bg(palette::accent())),
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
            .child(div().absolute().top(px(4.)).left(gpui_kit::relative(fraction.clamp(0., 1.))).ml(px(-8.)).size(px(16.)).rounded_full().bg(gpui_kit::white()).border_1().border_color(wash(0.24)).shadow_sm())
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

// Inputs sit a shade below the page: black 12% on Midnight, a lighter touch on white.
fn field_fill(cx: &App) -> Hsla {
    gpui_kit::black().opacity(if cx.theme().is_dark() { 0.12 } else { 0.03 })
}

// Unfilled slider tracks, meter bars and secondary buttons.
fn track(cx: &App) -> Hsla {
    rgb(if cx.theme().is_dark() { 0x4E5058 } else { 0xC4C9CE }).into()
}

fn nav_item(page: Page, selected: bool, cx: &mut Context<Rustcord>) -> impl IntoElement {
    let theme = cx.theme().clone();
    div()
        .id(SharedString::from(format!("nav-{page:?}")))
        .h(px(36.))
        .p_2()
        .rounded(px(8.))
        .flex()
        .items_center()
        .gap_3()
        .cursor_pointer()
        .font_weight(FontWeight::MEDIUM)
        .text_color(if selected { palette::strong(cx) } else { theme.muted_foreground })
        .when(selected, |d| d.bg(wash(0.2)))
        .when(!selected, |d| d.hover(|s| s.bg(wash(0.08)).text_color(theme.foreground)))
        .child(Icon::new(page.glyph()).size(px(20.)))
        .child(page.label())
        .on_click(cx.listener(move |this, _, _, cx| {
            if let Some(s) = &mut this.settings {
                (s.page, s.section, s.select, s.recording) = (page, 0, None, None);
                s.scroll.set_offset(gpui_kit::point(px(0.), px(0.)));
            }
            cx.notify();
        }))
}

// The open page's anchors, hung off a hairline with a bar beside the current one.
fn sub_items(page: Page, current: usize, cx: &mut Context<Rustcord>) -> impl IntoElement {
    let theme = cx.theme().clone();
    div().ml(px(17.)).my_1().border_l_1().border_color(theme.border).flex().flex_col().py_2().gap_4().children(page.sections().iter().enumerate().map(|(i, name)| {
        let selected = i == current;
        div()
            .id(SharedString::from(format!("sub-{name}")))
            .relative()
            .pl(px(19.))
            .h(px(20.))
            .flex()
            .items_center()
            .cursor_pointer()
            .text_color(if selected { palette::strong(cx) } else { theme.muted_foreground })
            .hover(|s| s.text_color(theme.foreground))
            .children(selected.then(|| div().absolute().left(px(-1.)).top_0().w(px(2.)).h_full().rounded_full().bg(palette::strong(cx))))
            .child(*name)
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(s) = &mut this.settings {
                    s.section = i;
                    s.scroll.scroll_to_top_of_item(i);
                }
                cx.notify();
            }))
    }))
}

fn h2(text: &'static str, cx: &App) -> AnyElement {
    div().mb_4().text_size(px(24.)).line_height(px(32.)).font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(text).into_any_element()
}

fn label(text: &'static str, cx: &App) -> Div {
    div().text_size(px(16.)).line_height(px(20.)).font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(text)
}

fn description(text: &str, cx: &App) -> Div {
    div().text_size(px(14.)).line_height(px(18.)).text_color(cx.theme().muted_foreground).child(text.to_owned())
}

fn divider(theme: &gpui_kit::component::Theme) -> Div {
    div().my(px(32.)).h(px(1.)).bg(theme.border)
}

pub(crate) fn switch(on: bool, theme: &gpui_kit::component::Theme) -> Div {
    div()
        .w(px(40.))
        .h(px(24.))
        .flex_none()
        .p(px(3.))
        .rounded_full()
        .border_1()
        .border_color(if on { palette::accent() } else { wash(0.3) })
        .bg(if on { palette::accent() } else { theme.input })
        .flex()
        .items_center()
        .when(on, |d| d.justify_end())
        .child(div().size(px(16.)).rounded_full().bg(gpui_kit::white()))
}

fn radio_row(id: &'static str, title: &'static str, detail: &'static str, selected: bool, cx: &App) -> Stateful<Div> {
    let theme = cx.theme();
    div()
        .id(id)
        .py_2()
        .flex()
        .items_start()
        .gap_3()
        .cursor_pointer()
        .child(
            div()
                .mt(px(1.))
                .size(px(18.))
                .flex_none()
                .rounded_full()
                .border_2()
                .border_color(if selected { palette::accent() } else { theme.muted_foreground })
                .flex()
                .items_center()
                .justify_center()
                .children(selected.then(|| div().size(px(8.)).rounded_full().bg(gpui_kit::white())))
                .when(selected, |d| d.bg(palette::accent())),
        )
        .child(div().flex().flex_col().gap_1().child(div().text_size(px(16.)).child(title)).child(div().text_size(px(14.)).text_color(theme.muted_foreground).child(detail)))
}

fn expander(id: &'static str, title: &str, detail: &str, open: bool, theme: &gpui_kit::component::Theme, cx: &App) -> Stateful<Div> {
    div()
        .id(id)
        .py_3()
        .flex()
        .items_center()
        .gap_4()
        .cursor_pointer()
        .child(div().flex_1().flex().flex_col().gap_1().child(div().text_size(px(16.)).font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(title.to_owned())).child(
            div().text_size(px(14.)).text_color(theme.muted_foreground).child(detail.to_owned()),
        ))
        .child(
            div()
                .size(px(32.))
                .rounded(px(8.))
                .bg(theme.muted)
                .flex()
                .items_center()
                .justify_center()
                .child(Icon::new(if open { Glyph::ChevronDown } else { Glyph::ChevronRight }).size(px(18.))),
        )
}

fn primary_button(id: &'static str, text: &'static str) -> Stateful<Div> {
    button(id, text, palette::accent(), palette::accent_hover())
}

fn danger_button(id: &'static str, text: &'static str) -> Stateful<Div> {
    button(id, text, palette::danger(), palette::danger_hover())
}

fn button(id: &'static str, text: &'static str, fill: Hsla, hover: Hsla) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(38.))
        .px(px(16.))
        .flex_none()
        .rounded(px(8.))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .bg(fill)
        .text_color(gpui_kit::white())
        .font_weight(FontWeight::MEDIUM)
        .hover(move |s| s.bg(hover))
        .child(text)
}

fn icon_button(id: &'static str, glyph: Glyph, cx: &App) -> Stateful<Div> {
    let theme = cx.theme();
    div()
        .id(id)
        .size(px(32.))
        .rounded(px(8.))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .text_color(theme.muted_foreground)
        .hover(|s| s.bg(wash(0.12)).text_color(theme.foreground))
        .child(Icon::new(glyph).size(px(20.)))
}

// Discord's mic test meter: 50 thin bars, lit green up to the current level.
fn meter(level: f32, cx: &App) -> Div {
    let (lit_color, off) = (cx.theme().success, track(cx));
    const BARS: usize = 50;
    let lit = (((level + 100.) / 100.).clamp(0., 1.) * BARS as f32) as usize;
    div().flex_1().h(px(24.)).flex().items_center().justify_between().children((0..BARS).map(|i| {
        div().w(px(4.)).h(px(24.)).rounded(px(2.)).bg(if i < lit { lit_color } else { off })
    }))
}

// Discord's automatic-sensitivity meter: a bar that fills green while you're transmitting.
fn speaking_bar(speaking: bool, cx: &App) -> Div {
    div().w_full().h(px(8.)).rounded(px(4.)).bg(if speaking { cx.theme().success } else { track(cx) })
}

// The collapsed accordion lists its first settings: three under Custom, two otherwise (Discord `tI`).
fn advanced_summary(custom: bool) -> String {
    let custom_only = ["Automatic Gain Control", "Advanced Voice Activity"];
    let always = ["No Audio Detected Warning", "Global Attenuation", "Quality of Service"];
    let shown = custom_only.iter().filter(|_| custom).chain(always.iter().filter(|t| **t != "Global Attenuation" || voice::ATTENUATION_SUPPORTED));
    shown.take(if custom { 3 } else { 2 }).copied().collect::<Vec<_>>().join(", ")
}

fn key_caps(keys: &'static str, theme: &gpui_kit::component::Theme) -> Div {
    div().flex().items_center().gap_1().children(keys.split(" + ").map(|key| {
        div()
            .px_2()
            .h(px(24.))
            .rounded(px(4.))
            .bg(theme.muted)
            .border_1()
            .border_color(theme.border)
            .flex()
            .items_center()
            .text_size(px(12.))
            .font_weight(FontWeight::BOLD)
            .child(key)
    }))
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
        assert_eq!(advanced_summary(true), "Automatic Gain Control, Advanced Voice Activity, No Audio Detected Warning");
        let second = if voice::ATTENUATION_SUPPORTED { "Global Attenuation" } else { "Quality of Service" };
        assert_eq!(advanced_summary(false), format!("No Audio Detected Warning, {second}"));
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
