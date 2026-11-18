// DM and group calls (design/flows-call.html): the strip that holds a voice call above the DM's chat,
// the banner that rings anywhere else in Dango, and the gateway's CALL_* events behind them. Video
// still opens the call stage (call.rs).
//
// GAP: the mock's Add people popover, resizable video stage and mini player aren't built yet.
use std::time::Duration;

use gpui_kit::{Animation, AnimationExt, AnyElement, Context, FontWeight, Hsla, SharedString, component::Icon, div, prelude::*, px};

use crate::{
    Rustcord,
    discord::{Channel, ChannelKind, VoiceState},
    icons::Glyph,
    palette::{self, Level},
    sounds,
    ui::{self, RADIUS_LG, Squircle, Tone},
};

pub(crate) const STRIP_HEIGHT: f32 = 56.;
const FACE: f32 = 32.;

// What the strip in a DM shows, from the gateway's view of its call and ours.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum StripState {
    // It rings us: Decline or Join.
    Incoming,
    // We're in and ringing them; nobody else has joined.
    Ringing,
    Live,
    // We're in, nobody else is, and it no longer rings anyone.
    Alone,
    // A call goes on here without us (we declined a group call, or joined late).
    Joinable,
}

pub(crate) fn strip_state(ringing_me: bool, in_it: bool, others: usize, ringing: bool, active: bool) -> Option<StripState> {
    match (ringing_me, in_it) {
        (true, false) => Some(StripState::Incoming),
        (_, true) if others > 0 => Some(StripState::Live),
        (_, true) if ringing => Some(StripState::Ringing),
        (_, true) => Some(StripState::Alone),
        (false, false) if active => Some(StripState::Joinable),
        (false, false) => None,
    }
}

impl Rustcord {
    pub(crate) fn on_dm_call(&mut self, channel_id: String, ringing: Vec<String>, cx: &mut Context<Self>) {
        let rings_me = ringing.iter().any(|id| id == self.my_id());
        let in_it = self.call.as_ref().is_some_and(|c| c.channel.id == channel_id);
        // Nobody left to ring: they answered, declined, or it rang out.
        if ringing.is_empty() && self.ringing_out.as_ref() == Some(&channel_id) {
            self.stop_ringing_out();
        }
        let was_ringing_me = self.ringing_in.as_ref() == Some(&channel_id);
        if rings_me && !in_it && !was_ringing_me {
            self.ringing_in = Some(channel_id.clone());
            sounds::ring(sounds::Cue::CallRinging);
            if !self.window_active {
                let caller = self.dm_caller(&channel_id);
                // Not verified: whether macOS shows an unbundled binary's notification; the main window doesn't take focus.
                std::thread::spawn(move || {
                    let _ = notify_rust::Notification::new().summary(&caller).body("Incoming call").show();
                });
            }
        } else if was_ringing_me && (!rings_me || in_it) {
            self.stop_ringing_in();
        }
        self.dm_calls.insert(channel_id, ringing);
        cx.notify();
    }

    pub(crate) fn on_dm_call_ended(&mut self, channel_id: &str) {
        self.dm_calls.remove(channel_id);
        self.dm_voice.retain(|v| v.channel_id.as_deref() != Some(channel_id));
        if self.ringing_in.as_deref() == Some(channel_id) {
            self.stop_ringing_in();
        }
    }

    pub(crate) fn stop_ringing_in(&mut self) {
        if self.ringing_in.take().is_some() {
            sounds::stop_ring();
        }
    }

    // Joins a DM's call, ringing the others when nobody is in it yet. `video` turns the camera on once connected.
    pub(crate) fn start_dm_call(&mut self, channel: Channel, video: bool, cx: &mut Context<Self>) {
        if self.call.as_ref().is_some_and(|c| c.channel.id == channel.id) {
            return;
        }
        if self.ringing_in.as_ref() == Some(&channel.id) {
            self.stop_ringing_in();
        }
        if self.dm_others(&channel.id).is_empty() {
            self.ring_on_connect = Some(channel.id.clone());
        }
        self.join_voice_in(None, channel, cx);
        self.camera_on_connect = video;
    }

    // Join from the banner or a call line: opens the DM too, so the strip is in view.
    pub(crate) fn answer_dm_call(&mut self, channel_id: &str, window: &mut gpui_kit::Window, cx: &mut Context<Self>) {
        let Some(dm) = self.dms.iter().find(|d| d.id == channel_id).cloned() else { return };
        if self.channel.as_ref().is_none_or(|c| c.id != dm.id) {
            self.guild_id = None;
            self.open_channel(dm.clone(), window, cx);
        }
        self.start_dm_call(dm, false, cx);
    }

    // Stops it ringing for us only; a group call goes on.
    pub(crate) fn decline_dm_call(&mut self, channel_id: String, cx: &mut Context<Self>) {
        self.stop_ringing_in();
        let (Some(client), me) = (self.client.clone(), self.my_id().to_owned()) else { return };
        // Not verified: the decline endpoint and body, taken from Discord's client traffic as documented by third parties.
        crate::discord::runtime().spawn(async move {
            let _ = client.call(reqwest::Method::POST, format!("/channels/{channel_id}/call/stop-ringing"), Some(serde_json::json!({ "recipients": [me] }))).await;
        });
        cx.notify();
    }

    // Rings everyone in the DM who isn't in its call, with our ringtone until one joins.
    pub(crate) fn ring_dm(&mut self, channel_id: String, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        self.ring_out(channel_id.clone(), cx);
        crate::discord::runtime().spawn(async move {
            let _ = client.call(reqwest::Method::POST, format!("/channels/{channel_id}/call/ring"), Some(serde_json::json!({ "recipients": null }))).await;
        });
    }

    // Everyone else in the DM's call.
    pub(crate) fn dm_others(&self, channel_id: &str) -> Vec<&VoiceState> {
        let me = self.my_id();
        self.dm_voice.iter().filter(|v| v.channel_id.as_deref() == Some(channel_id) && v.user_id != me).collect()
    }

    fn dm_caller(&self, channel_id: &str) -> String {
        match self.dm_others(channel_id).first() {
            Some(v) => v.user.as_ref().map_or_else(|| self.user_name(&v.user_id), |u| u.name.clone()),
            None => self.dms.iter().find(|d| d.id == channel_id).map(|d| d.name.clone()).unwrap_or_else(|| "Someone".into()),
        }
    }

    pub(crate) fn dm_strip_state(&self, channel_id: &str) -> Option<StripState> {
        let me = self.my_id();
        let ringing = self.dm_calls.get(channel_id).is_some_and(|r| r.iter().any(|id| id != me)) || self.ringing_out.as_deref() == Some(channel_id);
        let others = self.dm_others(channel_id).len();
        strip_state(self.ringing_in.as_deref() == Some(channel_id), self.call.as_ref().is_some_and(|c| c.channel.id == channel_id), others, ringing, others > 0)
    }

    // The faces in the strip: who's in, then who it still rings (pulsing), us left out.
    fn strip_faces(&self, channel: &Channel, state: StripState, cx: &mut Context<Self>) -> AnyElement {
        let me = self.my_id().to_owned();
        let page = palette::surface(Level::Raised, cx);
        let mut faces: Vec<AnyElement> = Vec::new();
        for v in self.dm_others(&channel.id) {
            let url = v.user.as_ref().map_or_else(|| self.user_avatar(&v.user_id), |u| u.avatar.clone());
            let talking = state == StripState::Live && self.speaking(&v.user_id);
            faces.push(div().size(px(FACE)).rounded_full().when(talking, |d| d.shadow(ui::speaking_ring(page))).child(ui::avatar(url, FACE)).into_any_element());
        }
        let rung: Vec<String> = match state {
            StripState::Ringing | StripState::Alone => match channel.recipient.as_ref() {
                Some(r) => vec![r.id.clone()],
                None => self.dm_calls.get(&channel.id).cloned().unwrap_or_default().into_iter().filter(|id| *id != me).collect(),
            },
            _ => Vec::new(),
        };
        for (i, id) in rung.into_iter().enumerate() {
            let face = div().size(px(FACE)).rounded_full().child(ui::avatar(self.user_avatar(&id), FACE));
            let face = match state {
                StripState::Ringing => face
                    .with_animation(SharedString::from(format!("strip-ring-{i}")), Animation::new(Duration::from_millis(1400)).repeat(), |d, t| d.opacity(0.55 + 0.45 * (t * std::f32::consts::TAU).cos().abs()))
                    .into_any_element(),
                _ => face.opacity(0.5).into_any_element(),
            };
            faces.push(face);
        }
        if faces.is_empty() {
            let url = channel.recipient.as_ref().map(|u| u.avatar.clone()).unwrap_or_else(|| crate::discord::avatar_url(&channel.id, None));
            faces.push(ui::avatar(url, FACE).into_any_element());
        }
        ui::stacked(faces, FACE, page).into_any_element()
    }

    // .dmcall: a voice call in a DM is a 56px strip over the chat, never a stage; the chat stays the screen.
    pub(crate) fn dm_call_strip(&self, channel: &Channel, cx: &mut Context<Self>) -> Option<AnyElement> {
        if channel.kind != ChannelKind::Dm {
            return None;
        }
        let state = self.dm_strip_state(&channel.id)?;
        let elapsed = self.call.as_ref().filter(|c| c.channel.id == channel.id).map(|c| crate::call::clock(c.joined.elapsed().as_secs()));
        let (title, line, tone): (String, String, Hsla) = match state {
            StripState::Incoming => (format!("{} is calling", self.dm_caller(&channel.id)), "Incoming call".into(), palette::accent_ink(cx)),
            StripState::Ringing => (format!("Ringing {}…", channel.name), elapsed.unwrap_or_default(), palette::faint(cx)),
            StripState::Live => (channel.name.clone(), format!("In call · {}", elapsed.unwrap_or_default()), palette::positive_ink(cx)),
            StripState::Alone => ("No one else is here".into(), format!("{} didn't pick up", channel.name), palette::faint(cx)),
            StripState::Joinable => ("Call in progress".into(), format!("{} in the call", self.dm_others(&channel.id).len()), palette::positive_ink(cx)),
        };
        let text = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .child(div().truncate().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(title))
            .child(div().truncate().text_size(px(12.)).text_color(tone).child(line));
        let id = channel.id.clone();
        let controls = div().flex_none().flex().items_center().gap(px(4.)).children(match state {
            StripState::Incoming => vec![
                ui::button("strip-decline", Tone::Danger, cx).small().child("Decline").on_click(cx.listener(move |this, _, _, cx| this.decline_dm_call(id.clone(), cx))).into_any_element(),
                self.join_button("strip-join", channel.id.clone(), cx),
            ],
            StripState::Joinable => vec![self.join_button("strip-join", channel.id.clone(), cx)],
            StripState::Alone => vec![
                ui::button("strip-ring-again", Tone::Secondary, cx).small().child("Ring again").on_click(cx.listener(move |this, _, _, cx| this.ring_dm(id.clone(), cx))).into_any_element(),
                self.mic_button(cx),
                self.hang_up_button("Leave call", cx),
            ],
            StripState::Ringing => vec![self.mic_button(cx), self.camera_button(cx), self.hang_up_button("Cancel call", cx)],
            StripState::Live => vec![
                self.mic_button(cx),
                self.camera_button(cx),
                self.share_button(cx),
                ui::icon_button("strip-stage", Glyph::Fullscreen, "Open call view", cx).on_click(cx.listener(|this, _, _, cx| {
                    this.call_view = true;
                    cx.notify();
                })).into_any_element(),
                self.hang_up_button("Leave call", cx),
            ],
        });
        let strip = div()
            .id("dm-call-strip")
            .occlude()
            .h(px(STRIP_HEIGHT))
            .px(px(12.))
            .flex()
            .items_center()
            .gap(px(12.))
            .squircle(RADIUS_LG)
            .bg(palette::surface(Level::Raised, cx))
            .border_1()
            .border_color(palette::line(cx))
            .child(self.strip_faces(channel, state, cx))
            .child(text)
            .child(controls);
        Some(strip.into_any_element())
    }

    fn join_button(&self, id: &'static str, channel_id: String, cx: &mut Context<Self>) -> AnyElement {
        ui::button(id, Tone::Primary, cx)
            .small()
            .child(Icon::new(Glyph::Phone).size(px(16.)))
            .child("Join")
            .on_click(cx.listener(move |this, _, window, cx| this.answer_dm_call(&channel_id, window, cx)))
            .into_any_element()
    }

    fn mic_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let (glyph, tip) = if self.muted { (Glyph::MicOff, "Unmute") } else { (Glyph::Mic, "Mute") };
        ui::icon_button("strip-mic", glyph, tip, cx).off(self.muted, cx).on_click(cx.listener(|this, _, _, cx| this.toggle_mute(cx))).into_any_element()
    }

    fn camera_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(call) = &self.call else { return div().into_any_element() };
        let (glyph, tip) = match (call.connected, call.camera_on) {
            (false, _) => (Glyph::Video, "Available once you're connected"),
            (true, true) => (Glyph::VideoOff, "Turn off camera"),
            (true, false) => (Glyph::Video, "Turn on camera"),
        };
        ui::icon_button("strip-camera", glyph, tip, cx).disabled(!call.connected).on_click(cx.listener(|this, _, _, cx| this.toggle_camera(cx))).into_any_element()
    }

    fn share_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let sharing = self.call.as_ref().is_some_and(|c| c.sharing.is_some());
        ui::icon_button("strip-share", Glyph::ShareScreen, if sharing { "Stop sharing" } else { "Share your screen" }, cx)
            .when(sharing, |b| b.text_color(palette::accent_ink(cx)))
            .on_click(cx.listener(|this, _, _, cx| this.toggle_screen_share(cx)))
            .into_any_element()
    }

    fn hang_up_button(&self, tip: &'static str, cx: &mut Context<Self>) -> AnyElement {
        ui::icon_button("strip-hang-up", Glyph::CallEnd, tip, cx).danger(cx).on_click(cx.listener(|this, _, _, cx| this.leave_voice(cx))).into_any_element()
    }

    // Rings anywhere but the DM itself: drops from the top of the pane and never takes focus.
    pub(crate) fn incoming_call_banner(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let channel_id = self.ringing_in.clone()?;
        if self.channel.as_ref().is_some_and(|c| c.id == channel_id) && !self.call_view {
            return None;
        }
        let dm = self.dms.iter().find(|d| d.id == channel_id)?;
        let caller = self.dm_caller(&channel_id);
        let face = match self.dm_others(&channel_id).first() {
            Some(v) => v.user.as_ref().map_or_else(|| self.user_avatar(&v.user_id), |u| u.avatar.clone()),
            None => dm.recipient.as_ref().map(|u| u.avatar.clone()).unwrap_or_else(|| crate::discord::avatar_url(&dm.id, None)),
        };
        let place = if dm.recipient.is_some() { "Incoming call".to_owned() } else { format!("Calling {}", dm.name) };
        let decline_id = channel_id.clone();
        let card = div()
            .id("incoming-call")
            .occlude()
            .w(px(360.))
            .p(px(12.))
            .flex()
            .items_center()
            .gap(px(12.))
            .squircle(RADIUS_LG)
            .bg(palette::surface(Level::Overlay, cx))
            .shadow(ui::shadow_pop(cx))
            .child(
                div()
                    .size(px(40.))
                    .flex_none()
                    .rounded_full()
                    .child(ui::avatar(face, 40.))
                    .with_animation("incoming-pulse", Animation::new(Duration::from_millis(1400)).repeat(), |d, t| d.opacity(0.6 + 0.4 * (t * std::f32::consts::TAU).cos().abs())),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(div().truncate().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(caller))
                    .child(div().truncate().text_size(px(12.)).text_color(palette::accent_ink(cx)).child(place)),
            )
            .child(ui::button("incoming-decline", Tone::Danger, cx).small().child("Decline").on_click(cx.listener(move |this, _, _, cx| this.decline_dm_call(decline_id.clone(), cx))))
            .child(self.join_button("incoming-join", channel_id, cx))
            .with_animation("incoming-in", Animation::new(Duration::from_millis(240)).with_easing(|t| crate::motion::ease_out().sample(t)), |d, t| d.mt(px(-12. * (1. - t))).opacity(t));
        Some(div().absolute().top(px(12.)).left_0().right_0().flex().justify_center().child(card).into_any_element())
    }

    // The DM row's second line while a call goes on there.
    pub(crate) fn dm_call_line(&self, channel_id: &str, cx: &mut Context<Self>) -> Option<(Glyph, &'static str, Hsla)> {
        if self.ringing_in.as_deref() == Some(channel_id) {
            return Some((Glyph::Phone, "Calling you…", palette::accent_ink(cx)));
        }
        if self.call.as_ref().is_some_and(|c| c.channel.id == channel_id) {
            return Some((Glyph::Phone, "In call", palette::positive_ink(cx)));
        }
        (!self.dm_others(channel_id).is_empty()).then(|| (Glyph::Phone, "Call in progress", palette::positive_ink(cx)))
    }
}

// "lasted 12 minutes": Discord's wording for a finished call.
pub(crate) fn lasted(secs: i64) -> String {
    match secs {
        ..60 => "a few seconds".into(),
        60..120 => "a minute".into(),
        120..3600 => format!("{} minutes", secs / 60),
        3600..7200 => "an hour".into(),
        _ => format!("{} hours", secs / 3600),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_states() {
        assert_eq!(strip_state(true, false, 1, false, true), Some(StripState::Incoming));
        assert_eq!(strip_state(false, true, 0, true, false), Some(StripState::Ringing));
        assert_eq!(strip_state(false, true, 1, true, true), Some(StripState::Live), "someone joined while others still ring");
        assert_eq!(strip_state(false, true, 0, false, false), Some(StripState::Alone));
        assert_eq!(strip_state(false, false, 2, false, true), Some(StripState::Joinable), "declined a group call that goes on");
        assert_eq!(strip_state(false, false, 0, false, false), None);
        assert_eq!(strip_state(true, true, 1, true, true), Some(StripState::Live), "answered on this device");
    }

    #[test]
    fn lasted_reads_like_discord() {
        assert_eq!(lasted(5), "a few seconds");
        assert_eq!(lasted(90), "a minute");
        assert_eq!(lasted(12 * 60 + 30), "12 minutes");
        assert_eq!(lasted(3700), "an hour");
        assert_eq!(lasted(3 * 3600), "3 hours");
    }
}
