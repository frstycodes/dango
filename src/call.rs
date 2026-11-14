// Everything around a voice call, after Discord: the voice panel over the user panel with its
// connection stages, the connection details popout, the call view with participant tiles and the
// control bar, device menus and the participant menu.
use std::time::{Duration, Instant};

use gpui_kit::{
    Animation, AnimationExt, AnyElement, Context, Div, FontWeight, Hsla, ImageSource, MouseButton, MouseDownEvent, Pixels, Point, SharedString, Stateful, Window,
    component::{ActiveTheme, Icon, tooltip::Tooltip},
    div, img, prelude::*, px, rgb,
};

use crate::{
    Rustcord,
    ui::{self, Tone},
    menu::{Entry, item, submenu},
    discord::{self, User, VoiceState},
    icons::Glyph,
    layout::RAIL_WIDTH,
    palette,
    settings::{Fill, Page},
    voice::{self, Stage},
};

// Discord's voice panel: 8px padding, a 40px status row, 12px gap, 32px buttons.
pub const VOICE_HEIGHT: f32 = 100.;
const GO_LIVE_HEIGHT: f32 = 57.;
// A warning line under the voice status: 16px of text plus the panel's 12px gap.
const WARNING_HEIGHT: f32 = 28.;
const PREVIEW_AGE: std::time::Duration = std::time::Duration::from_secs(60);
// Discord's call tiles (read from Discord web, 2026-11-14).
const TILE_RADIUS: f32 = 8.;
const TILE_GAP: f32 = 8.;
const STRIP_HEIGHT: f32 = 112.;
const STRIP_LIMIT: usize = 12;
const RECENT_SPEAKER: std::time::Duration = std::time::Duration::from_secs(10);
const CONTROLS_IDLE: Duration = Duration::from_secs(3);
const PIP_WIDTH: f32 = 240.;
const CHAT_WIDTH: f32 = 340.;
const PIP_INSET: f32 = 16.;
const PIP_DECELERATION: f32 = 0.995;
const PIP_SNAP: Duration = Duration::from_millis(320);
const HEADER_HEIGHT: f32 = 48.;
const CONTROLS_HEIGHT: f32 = 96.;

// How several watched streams share the stage: one large with the rest small, or an even grid.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StreamLayout {
    Focus,
    Split,
}

// Which corner of the call stage our stream's self-view sits in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Corner {
    // The corner a released self-view heads for: where its centre would coast to at its release
    // velocity (px/s) under scroll-like deceleration, so a flick throws it across the stage.
    pub fn thrown(center: (f32, f32), velocity: (f32, f32), stage: (f32, f32)) -> Corner {
        let coast = |v: f32| v / 1000. * PIP_DECELERATION / (1. - PIP_DECELERATION);
        let (x, y) = (center.0 + coast(velocity.0), center.1 + coast(velocity.1));
        match (y < stage.1 / 2., x < stage.0 / 2.) {
            (true, true) => Corner::TopLeft,
            (true, false) => Corner::TopRight,
            (false, true) => Corner::BottomLeft,
            (false, false) => Corner::BottomRight,
        }
    }

    // Top-left of a self-view sitting in this corner of a stage of `stage` size.
    fn spot(self, stage: (f32, f32)) -> (f32, f32) {
        let left = PIP_INSET;
        let right = stage.0 - PIP_WIDTH - PIP_INSET;
        let top = HEADER_HEIGHT + 8.;
        let bottom = stage.1 - CONTROLS_HEIGHT - PIP_WIDTH * 9. / 16. - 8.;
        match self {
            Corner::TopLeft => (left, top),
            Corner::TopRight => (right, top),
            Corner::BottomLeft => (left, bottom),
            Corner::BottomRight => (right, bottom),
        }
    }
}

// A self-view being dragged: where the pointer and view started, and recent pointer samples for
// the release velocity.
pub struct PipDrag {
    start_mouse: Point<Pixels>,
    start: (f32, f32),
    pos: (f32, f32),
    samples: Vec<(Point<Pixels>, Instant)>,
}

enum Tile {
    User(VoiceState),
    Stream(VoiceState),
}
const ERROR_HEIGHT: f32 = 56.;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeviceKind {
    Input,
    Output,
    Camera,
}

pub struct DeviceMenu {
    kind: DeviceKind,
    at: Point<Pixels>,
    devices: Vec<voice::Device>,
}

// Discord's names for each connection stage, and the colour its status line takes.
fn stage_text(stage: Stage) -> &'static str {
    match stage {
        Stage::AwaitingEndpoint => "Awaiting Endpoint",
        Stage::Authenticating => "Authenticating",
        Stage::RtcConnecting => "RTC Connecting",
        Stage::IceChecking => "ICE Checking",
        Stage::DtlsConnecting => "DTLS Connecting",
        Stage::Connected => "Voice Connected",
        Stage::NoRoute => "No Route",
        Stage::Disconnected => "RTC Disconnected",
    }
}

// Not verified: Discord's exact ping bands for its signal icon; these follow its "above 250 ms" warning.
fn signal(ping: Option<u32>) -> (Glyph, Hsla) {
    let good = palette::status(discord::Status::Online);
    match ping {
        None => (Glyph::ConnectionUnknown, good),
        Some(p) if p < 150 => (Glyph::ConnectionFine, good),
        Some(p) if p < 250 => (Glyph::ConnectionAverage, palette::status(discord::Status::Idle)),
        Some(_) => (Glyph::ConnectionBad, palette::status(discord::Status::Dnd)),
    }
}

impl Rustcord {
    // Discord's stream thumbnails (GET /streams/{key}/preview) for everyone live in the open server
    // and our call, refetched when older than a minute.
    pub(crate) fn refresh_stream_previews(&mut self, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        let wanted: Vec<String> = [self.guild_id.clone(), self.call.as_ref().and_then(|c| c.guild_id.clone())].into_iter().flatten().collect();
        let mut keys: Vec<(String, String)> = Vec::new();
        for guild in self.guilds.iter().filter(|g| wanted.contains(&g.id)) {
            for state in guild.voice_states.iter().filter(|v| v.streaming) {
                let Some(channel) = &state.channel_id else { continue };
                let fresh = self.stream_previews.get(&state.user_id).is_some_and(|(_, at)| at.elapsed() < PREVIEW_AGE);
                if !fresh {
                    keys.push((state.user_id.clone(), format!("guild:{}:{channel}:{}", guild.id, state.user_id)));
                }
            }
        }
        for (user_id, key) in keys {
            // Marked fetched now so a burst of events doesn't fire duplicate requests.
            let old = self.stream_previews.get(&user_id).map(|(url, _)| url.clone()).unwrap_or_default();
            self.stream_previews.insert(user_id.clone(), (old, std::time::Instant::now()));
            let client = client.clone();
            cx.spawn(async move |this, cx| {
                let result = client.call(reqwest::Method::GET, format!("/streams/{key}/preview"), None).await;
                if cfg!(debug_assertions) && std::env::var_os("RUSTCORD_STREAM_DEBUG").is_some() {
                    eprintln!("stream preview {key}: {:?}", result.as_ref().map(|b| b.to_string()));
                }
                let Ok(body) = result else { return };
                let Some(url) = body["url"].as_str().map(str::to_owned) else { return };
                this.update(cx, |this, cx| {
                    this.stream_previews.insert(user_id, (url, std::time::Instant::now()));
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
    }

    // Tiles without a camera take their avatar's colour, as Discord's do; fetched once per person.
    pub(crate) fn refresh_avatar_colors(&mut self, cx: &mut Context<Self>) {
        if self.call.is_none() {
            return;
        }
        let wanted: Vec<(String, String)> = self.participants().into_iter().filter(|p| !self.avatar_colors.contains_key(&p.user_id)).map(|p| (p.user_id.clone(), p.user.map(|u| u.avatar).unwrap_or_else(|| self.user_avatar(&p.user_id)))).collect();
        for (user_id, avatar) in wanted {
            self.avatar_colors.insert(user_id.clone(), None);
            let url = small_png(&avatar);
            cx.spawn(async move |this, cx| {
                let color = discord::runtime().spawn(async move { avatar_color(&url).await }).await.ok().flatten();
                this.update(cx, |this, cx| {
                    this.avatar_colors.insert(user_id, color);
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
    }

    // What everyone sees of our Go Live before watching, uploaded as Discord's own client does:
    // a 512px-wide JPEG shortly after going live, then every five minutes (a minute after a failure).
    pub(crate) fn upload_stream_previews(&mut self, source: voice::ShareSource, cx: &mut Context<Self>) {
        let (Some(client), Some(call), Some(me)) = (self.client.clone(), self.call.as_ref(), self.me.as_ref()) else { return };
        let key = match &call.guild_id {
            Some(guild) => format!("guild:{guild}:{}:{}", call.channel.id, me.id),
            None => format!("call:{}:{}", call.channel.id, me.id),
        };
        self.preview_generation += 1;
        let generation = self.preview_generation;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(3)).await;
            loop {
                let live = this.update(cx, |this, _| this.preview_generation == generation && this.call.as_ref().is_some_and(|c| c.sharing.is_some()));
                if !live.unwrap_or(false) {
                    return;
                }
                let thumbnail = cx.background_spawn(async move { voice::thumbnail(source, 512).ok().and_then(|frame| jpeg_data_url(&frame)) }).await;
                let sent = match thumbnail {
                    Some(url) => {
                        let result = client.call(reqwest::Method::POST, format!("/streams/{key}/preview"), Some(serde_json::json!({ "thumbnail": url }))).await;
                        if cfg!(debug_assertions) && std::env::var_os("RUSTCORD_STREAM_DEBUG").is_some() {
                            eprintln!("stream preview upload {key}: {result:?}");
                        }
                        result.is_ok()
                    }
                    None => false,
                };
                cx.background_executor().timer(Duration::from_secs(if sent { 300 } else { 60 })).await;
            }
        })
        .detach();
    }

    pub(crate) fn stream_preview(&self, user_id: &str) -> Option<String> {
        self.stream_previews.get(user_id).map(|(url, _)| url.clone()).filter(|url| !url.is_empty())
    }

    // Discord's in-call warnings: a mic that sends nothing, and Advanced suppression that had to
    // fall back to RNNoise because this computer can't keep up with it.
    fn voice_warning(&self) -> Option<&'static str> {
        let call = self.call.as_ref()?;
        let silent = self.prefs.silence_warning && !self.muted && !self.deafened && call.session.input_detected() == Some(false);
        match (silent, call.session.noise_suppression_degraded()) {
            (true, _) => Some("No audio detected from your mic"),
            (false, true) => Some("Advanced noise suppression fell back to Standard"),
            _ => None,
        }
    }

    pub(crate) fn voice_panel_height(&self) -> f32 {
        let warning = if self.voice_warning().is_some() { WARNING_HEIGHT } else { 0. };
        match (&self.call, &self.voice_error) {
            (Some(call), _) => VOICE_HEIGHT + warning + if call.sharing.is_some() { GO_LIVE_HEIGHT } else { 0. },
            (None, Some(_)) => ERROR_HEIGHT,
            (None, None) => 0.,
        }
    }

    pub(crate) fn dismiss_call_menus(&mut self) -> bool {
        std::mem::take(&mut self.connection_info) | self.device_menu.take().is_some()
    }

    // "Voice Connected" (or the stage it's at), the channel and server, the hang-up button, then
    // Discord's row of call buttons.
    pub(crate) fn voice_panel(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = cx.theme().clone();
        let Some(call) = &self.call else {
            let error = self.voice_error.clone()?;
            return Some(
                div()
                    .h(px(ERROR_HEIGHT))
                    .px_3()
                    .flex()
                    .items_center()
                    .gap_2()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(div().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).text_color(theme.danger).child("Voice Disconnected"))
                            .child(div().truncate().text_size(px(12.)).text_color(theme.muted_foreground).child(error)),
                    )
                    .child(ui::icon_button("dismiss-voice", Glyph::X, "Dismiss", cx).on_click(cx.listener(|this, _, _, cx| {
                        this.voice_error = None;
                        cx.notify();
                    })))
                    .into_any_element(),
            );
        };
        let connected = call.stage == Stage::Connected;
        // --text-feedback-positive / -warning / -critical, Discord's status text.
        let color: Hsla = match call.stage {
            Stage::Connected => rgb(0x41A063).into(),
            Stage::NoRoute | Stage::Disconnected => rgb(0xEB5F5E).into(),
            _ => rgb(0xCE8100).into(),
        };
        let ping = call.stats.as_ref().map(|s| s.ping_ms);
        let (glyph, quality) = signal(ping);
        let place = match call.guild_id.as_ref().and_then(|id| self.guilds.iter().find(|g| &g.id == id)) {
            Some(guild) => format!("{} / {}", call.channel.name, guild.name),
            None => call.channel.name.clone(),
        };
        let channel = call.channel.clone();
        // Discord's large ping: an 18px signal in a 32px tile tinted by connection quality.
        let ping_tile = div()
            .id("voice-ping")
            .size(px(32.))
            .flex_none()
            .rounded(px(8.))
            .bg(quality.opacity(0.08))
            .border_1()
            .border_color(palette::wash(0.16))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .text_color(quality)
            .child(Icon::new(glyph).size(px(18.)))
            .on_click(cx.listener(|this, _, _, cx| {
                this.connection_info = !this.connection_info;
                cx.notify();
            }));
        let status = div()
            .id("voice-status")
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .cursor_pointer()
            .child(div().truncate().h(px(20.)).text_size(px(16.)).line_height(px(20.)).font_weight(FontWeight::SEMIBOLD).text_color(color).child(stage_text(call.stage)))
            .child(
                div()
                    .id("voice-place")
                    .truncate()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(theme.muted_foreground)
                    .hover(|s| s.underline())
                    .child(place)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.show_call(channel.clone(), window, cx);
                    })),
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.connection_info = !this.connection_info;
                cx.notify();
            }));
        let noise = self.prefs.noise_suppression;
        let header = div()
            .h(px(40.))
            .flex()
            .items_center()
            .gap_2()
            .child(ping_tile)
            .child(status)
            .child(
                ui::icon_button("noise", Glyph::Waveform, if noise { "Disable Noise Suppression" } else { "Enable Noise Suppression" }, cx)
                    .when(noise, |b| b.text_color(theme.foreground))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_noise_suppression(cx))),
            )
            .child(panel_button_tinted("hang-up", Glyph::CallEnd, "Disconnect", Some(palette::danger()), cx).on_click(cx.listener(|this, _, _, cx| this.leave_voice(cx))));
        let sharing = call.sharing.is_some();
        let actions = div()
            .flex()
            .gap_2()
            .child(wide_button("panel-camera", if call.camera_on { Glyph::Video } else { Glyph::VideoOff }, if call.camera_on { "Turn Off Camera" } else { "Turn On Camera" }, call.camera_on, connected, cx).on_click(cx.listener(|this, _, _, cx| this.toggle_camera(cx))))
            .child(match sharing {
                true => wide_button("panel-share", Glyph::ShareScreen, "Stream Options", true, connected, cx).on_mouse_down(MouseButton::Left, cx.listener(|this, e: &MouseDownEvent, _, cx| this.open_stream_menu(e.position, cx))),
                false => wide_button("panel-share", Glyph::ShareScreen, "Share Your Screen", false, connected, cx).on_click(cx.listener(|this, _, _, cx| this.toggle_screen_share(cx))),
            })
            .child(wide_button("panel-activities", Glyph::Activities, "Start an Activity", false, connected, cx).on_click(cx.listener(|this, _, _, cx| this.show_toast("Activities aren't in Rustcord yet", cx))))
            .child(wide_button("panel-soundboard", Glyph::Soundboard, "Open Soundboard", false, connected, cx).on_click(cx.listener(|this, _, _, cx| this.show_toast("The soundboard isn't in Rustcord yet", cx))));
        let warning = self.voice_warning();
        let voice = div()
            .min_h(px(VOICE_HEIGHT))
            .p_2()
            .flex()
            .flex_col()
            .gap(px(12.))
            .border_b_1()
            .border_color(palette::wash(0.16))
            .child(header)
            .children(warning.map(|text| {
                div()
                    .id("voice-warning")
                    .h(px(16.))
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_size(px(12.))
                    .text_color(rgb(0xCE8100))
                    .cursor_pointer()
                    .child(Icon::new(Glyph::Help).size(px(14.)))
                    .child(div().min_w_0().truncate().child(text))
                    .on_click(cx.listener(|this, _, window, cx| this.open_settings(Page::VoiceVideo, window, cx)))
            }))
            .child(actions);
        Some(div().flex().flex_col().children(self.go_live_panel(cx)).child(voice).into_any_element())
    }

    // Above the voice panel while we share: our stream's live thumbnail, what it is, its quality,
    // who's watching and for how long. Clicking it opens the stream menu.
    fn go_live_panel(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let call = self.call.as_ref()?;
        call.sharing.as_ref()?;
        let theme = cx.theme().clone();
        let label = call.share_label.clone().unwrap_or_else(|| "Screen".into());
        // Discord colours quality above its free tier (720p 30fps) in Nitro pink.
        let boosted = self.prefs.stream_resolution == 0 || self.prefs.stream_resolution > 720 || self.prefs.stream_fps > 30;
        let quality_ink: Hsla = if boosted { rgb(0xFF73FA).into() } else { theme.muted_foreground };
        let viewers = self.own_viewers();
        let since = call.live_since.map(|t| clock(t.elapsed().as_secs())).unwrap_or_default();
        let thumb = match &call.self_view {
            Some((frame, _)) => img(ImageSource::Render(frame.clone())).size_full().object_fit(gpui_kit::ObjectFit::Cover).into_any_element(),
            None => div().size_full().bg(rgb(0x45A796)).into_any_element(),
        };
        let watching = match viewers {
            0 => String::new(),
            n => format!(" · {n} watching"),
        };
        Some(
            div()
                .id("go-live-card")
                .h(px(GO_LIVE_HEIGHT))
                .px_2()
                .flex()
                .items_center()
                .gap_3()
                .cursor_pointer()
                .border_b_1()
                .border_color(palette::wash(0.16))
                .hover(|s| s.bg(palette::wash(0.12)))
                .child(div().relative().w(px(64.)).h(px(36.)).flex_none().rounded(px(4.)).overflow_hidden().child(thumb).child(div().absolute().left(px(3.)).top(px(3.)).child(ui::live_badge().h(px(12.)).text_size(px(8.)))))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(div().truncate().text_size(px(14.)).font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(label))
                        .child(
                            div()
                                .truncate()
                                .text_size(px(12.))
                                .text_color(theme.muted_foreground)
                                .child(gpui_kit::StyledText::new(format!("{}{watching} · {since}", quality_label(self.prefs.stream_resolution, self.prefs.stream_fps))).with_highlights([(0..quality_label(self.prefs.stream_resolution, self.prefs.stream_fps).len(), gpui_kit::HighlightStyle { color: Some(quality_ink), ..Default::default() })])),
                        ),
                )
                .child(Icon::new(Glyph::ChevronRight).size(px(16.)).text_color(theme.muted_foreground))
                .on_mouse_down(MouseButton::Left, cx.listener(|this, e: &MouseDownEvent, _, cx| this.open_stream_menu(e.position, cx)))
                .with_animation("go-live-card-in", Animation::new(Duration::from_millis(220)), |d, t| d.opacity(t.min(1.)).mt(px((1. - crate::motion::back_out(t)) * 8.))),
        )
    }

    fn own_viewers(&self) -> usize {
        let Some(me) = &self.me else { return 0 };
        let suffix = format!(":{}", me.id);
        self.stream_viewers.iter().find(|(key, _)| key.ends_with(&suffix)).map_or(0, |(_, v)| v.iter().filter(|id| **id != me.id).count())
    }

    // Discord's stream menu, opened from the share button or the Go Live card while we're live.
    pub(crate) fn open_stream_menu(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        cx.stop_propagation();
        self.open_live_menu(at, |this| this.stream_menu(), cx);
    }

    fn stream_menu(&self) -> Vec<Entry> {
        let (res, fps, audio) = (self.prefs.stream_resolution, self.prefs.stream_fps, self.prefs.stream_audio);
        let mut quality: Vec<Entry> = [(720, "720p"), (1080, "1080p"), (1440, "1440p"), (0, "Source")]
            .into_iter()
            .map(|(value, label)| item(label, move |this, _, cx| this.set_stream_quality(Some(value), None, cx)).checked(res == value).entry())
            .collect();
        quality.push(Entry::Separator);
        quality.extend([15, 30, 60].into_iter().map(|value| item(format!("{value} FPS"), move |this, _, cx| this.set_stream_quality(None, Some(value), cx)).checked(fps == value).entry()));
        vec![
            item("Change Stream", |this, _, cx| this.open_share_picker(cx)).hint("Pick another window or screen").icon(Glyph::ShareScreen).entry(),
            submenu("Stream Quality", quality).hint(quality_label(res, fps)).entry(),
            item("Share Stream Audio", |this, _, cx| {
                this.update_prefs(cx, |p| p.stream_audio = !p.stream_audio);
                this.restart_share(cx);
            })
            .checked(audio)
            .entry(),
            item("Show My Stream", |this, _, cx| {
                this.pip_shown = !this.pip_shown;
                cx.notify();
            })
            .checked(self.pip_shown)
            .entry(),
            Entry::Separator,
            item("Stop Streaming", |this, _, cx| this.toggle_screen_share(cx)).icon(Glyph::ScreenOff).danger().entry(),
        ]
    }

    fn set_stream_quality(&mut self, resolution: Option<u32>, fps: Option<u32>, cx: &mut Context<Self>) {
        self.update_prefs(cx, |p| {
            p.stream_resolution = resolution.unwrap_or(p.stream_resolution);
            p.stream_fps = fps.unwrap_or(p.stream_fps);
        });
        self.restart_share(cx);
    }

    pub(crate) fn toggle_noise_suppression(&mut self, cx: &mut Context<Self>) {
        self.update_prefs(cx, |p| {
            p.noise_suppression = !p.noise_suppression;
            p.input_profile = crate::settings::InputProfile::Custom;
        });
    }

    pub(crate) fn toggle_camera(&mut self, cx: &mut Context<Self>) {
        let Some(call) = &mut self.call else { return };
        let on = !call.camera_on;
        let camera = on.then(|| self.prefs.camera.clone().or_else(|| voice::cameras().into_iter().next().map(|c| c.id))).flatten();
        if on && camera.is_none() {
            self.voice_error = Some("No camera found".into());
            return cx.notify();
        }
        match call.session.set_camera(camera) {
            Ok(()) => call.camera_on = on,
            Err(error) => self.voice_error = Some(error.into()),
        }
        if !on && let Some(me) = self.me.as_ref().map(|m| m.id.clone()) && let Some(frame) = call.cameras.remove(&me) {
            cx.drop_image(frame, None);
        }
        cx.notify();
    }

    // Opening a voice channel you're in shows its call, as clicking it in Discord does.
    pub(crate) fn show_call(&mut self, channel: discord::Channel, window: &mut Window, cx: &mut Context<Self>) {
        if self.call.as_ref().is_none_or(|c| c.channel.id != channel.id) {
            self.join_voice(channel.clone(), cx);
        }
        let guild_id = self.guilds.iter().find(|g| g.channels.iter().any(|c| c.id == channel.id)).map(|g| g.id.clone());
        if let Some(guild_id) = guild_id
            && self.guild_id.as_ref() != Some(&guild_id)
        {
            self.open_guild(Some(guild_id), window, cx);
        }
        self.call_view = true;
        cx.notify();
    }

    fn participants(&self) -> Vec<VoiceState> {
        let Some(call) = &self.call else { return Vec::new() };
        let states: Vec<VoiceState> = self
            .guilds
            .iter()
            .filter(|g| Some(&g.id) == call.guild_id.as_ref())
            .flat_map(|g| g.voice_states.iter())
            .filter(|v| v.channel_id.as_ref() == Some(&call.channel.id))
            .cloned()
            .collect();
        if states.is_empty()
            && let Some(me) = &self.me
        {
            return vec![VoiceState { guild_id: None, channel_id: Some(call.channel.id.clone()), user_id: me.id.clone(), user: Some(me.clone()), muted: self.muted, deafened: self.deafened, server_muted: false, server_deafened: false, streaming: false, camera: false }];
        }
        states
    }

    fn speaking(&self, user_id: &str) -> bool {
        let Some(call) = &self.call else { return false };
        match self.me.as_ref().is_some_and(|m| m.id == user_id) {
            true => call.level.1 && !self.muted && !self.deafened,
            false => call.speaking.contains(user_id),
        }
    }

    // Discord's order (its participant sort, read from Discord web 2026-11-14): streams first, then
    // whoever is speaking, then anyone who spoke in the last ten seconds, then cameras, then the rest
    // by most recently spoken and name.
    fn ordered_tiles(&self) -> Vec<Tile> {
        let Some(call) = &self.call else { return Vec::new() };
        let people = self.participants();
        let mut tiles: Vec<(String, Tile)> = Vec::new();
        for state in people.iter().filter(|s| s.streaming) {
            let name = state.user.as_ref().map(|u| u.name.clone()).unwrap_or_default();
            tiles.push((format!("\x02{name}"), Tile::Stream(state.clone())));
        }
        for state in people {
            let name = state.user.as_ref().map(|u| u.name.clone()).unwrap_or_default();
            let since = call.last_spoke.get(&state.user_id).map(|t| t.elapsed());
            let rank = match () {
                _ if self.speaking(&state.user_id) => 3,
                _ if since.is_some_and(|d| d < RECENT_SPEAKER) => 4,
                _ if state.camera => 5,
                _ => 6,
            };
            let ago = since.map_or(u64::MAX, |d| d.as_millis() as u64);
            tiles.push((format!("{}{ago:020}{name}", rank as u8 as char), Tile::User(state)));
        }
        tiles.sort_by(|a, b| a.0.cmp(&b.0));
        tiles.into_iter().map(|(_, tile)| tile).collect()
    }

    // The call view, with the voice channel's chat beside it when open (the stage shrinks; the
    // chat never covers the stream).
    pub(crate) fn render_call_with_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let call_channel = self.call.as_ref()?.channel.clone();
        let chat_open = self.call_chat && self.channel.as_ref().is_some_and(|c| c.id == call_channel.id);
        let drawer = chat_open.then(|| self.call_chat_drawer(&call_channel, window, cx));
        let call = self.render_call(window, cx)?;
        Some(div().size_full().flex().child(div().flex_1().min_w_0().h_full().child(call)).children(drawer).into_any_element())
    }

    fn call_chat_drawer(&mut self, channel: &discord::Channel, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let messages = self.render_messages(cx).into_any_element();
        let composer = self.render_composer(window, cx).into_any_element();
        div()
            .w(px(CHAT_WIDTH))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .bg(theme.background)
            .border_l_1()
            .border_color(theme.sidebar_border)
            .child(
                div()
                    .h(px(48.))
                    .flex_none()
                    .px_4()
                    .flex()
                    .items_center()
                    .gap_2()
                    .border_b_1()
                    .border_color(theme.sidebar_border)
                    .child(Icon::new(Glyph::VolumeHigh).size(px(20.)).text_color(theme.muted_foreground))
                    .child(div().flex_1().min_w_0().truncate().font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(channel.name.clone()))
                    .child(ui::icon_button("call-chat-close", Glyph::X, "Close Chat", cx).on_click(cx.listener(|this, _, _, cx| {
                        this.call_chat = false;
                        cx.notify();
                    }))),
            )
            .child(div().relative().flex_1().min_h_0().child(messages))
            .child(composer)
            .with_animation("call-chat-in", Animation::new(Duration::from_millis(220)), |d, t| d.opacity(t.min(1.)).ml(px((1. - crate::motion::back_out(t)) * 24.)))
            .into_any_element()
    }

    // Opens the voice channel's text chat beside the call, as Discord's chat button does.
    fn toggle_call_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.call_chat = !self.call_chat;
        if self.call_chat
            && let Some(channel) = self.call.as_ref().map(|c| c.channel.clone())
            && self.channel.as_ref().is_none_or(|c| c.id != channel.id)
        {
            self.open_channel(channel, window, cx);
            self.call_view = true;
        }
        cx.notify();
    }

    // The call view: a grid of 16:9 tiles, or the watched stream large over a collapsible strip of
    // everyone else. The header and control bar float over the stage and fade out while the pointer
    // is still, as Discord's do.
    pub(crate) fn render_call(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let call = self.call.as_ref()?;
        let tiles = self.ordered_tiles();
        let viewport = window.viewport_size();
        let chat = if self.call_chat && self.channel.as_ref().is_some_and(|c| c.id == call.channel.id) { CHAT_WIDTH } else { 0. };
        let (width, height) = match self.call_fullscreen {
            true => (f32::from(viewport.width) - 32. - chat, f32::from(viewport.height)),
            false => (f32::from(viewport.width) - RAIL_WIDTH - self.channels_width - 32. - chat, f32::from(viewport.height) - 32. - 16.),
        };
        let chrome = !self.call_idle();
        let stage_size = match self.call_fullscreen {
            true => (f32::from(viewport.width) - chat, f32::from(viewport.height)),
            false => (f32::from(viewport.width) - RAIL_WIDTH - self.channels_width - chat, f32::from(viewport.height) - crate::layout::TITLE_HEIGHT - 1.),
        };
        let ended = call.ended.clone().filter(|_| call.watching.is_empty());
        let stage: AnyElement = match (call.watching.is_empty(), ended) {
            (false, _) => self.stream_stage(&tiles, width, height, cx),
            (true, Some(streamer)) => self.ended_stage(&streamer, cx),
            (true, None) => {
                let grid_h = height - HEADER_HEIGHT - CONTROLS_HEIGHT;
                let n = tiles.len().max(1);
                // The column count that makes the biggest 16:9 tiles fit the stage.
                let size = |cols: usize| {
                    let rows = n.div_ceil(cols) as f32;
                    let w = (width - TILE_GAP * (cols as f32 - 1.)) / cols as f32;
                    w.min((grid_h - TILE_GAP * (rows - 1.)) / rows * 16. / 9.)
                };
                let cols = (1..=n).max_by(|a, b| size(*a).total_cmp(&size(*b))).unwrap_or(1);
                let tile_w = size(cols).max(124.);
                let rendered: Vec<AnyElement> = tiles.iter().map(|t| self.tile(t, tile_w, tile_w * 9. / 16., false, cx)).collect();
                div()
                    .size_full()
                    .pt(px(HEADER_HEIGHT))
                    .pb(px(CONTROLS_HEIGHT))
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_center()
                    .content_center()
                    .gap(px(TILE_GAP))
                    .children(rendered)
                    .into_any_element()
            }
        };
        // Discord's idle rule: the chrome fades over 200ms while the top bar rises 8px and the bottom bar drops 8px.
        let fade = |id: &'static str, rise: f32, el: AnyElement| {
            div().relative().with_animation(SharedString::from(format!("{id}-{chrome}")), Animation::new(Duration::from_millis(200)), move |d, t| {
                let shown = if chrome { t } else { 1. - t };
                d.opacity(shown).top(px(rise * (1. - shown)))
            })
            .child(el)
        };
        Some(
            div()
                .id("call-view")
                .relative()
                .size_full()
                .bg(gpui_kit::black())
                .on_mouse_move(cx.listener(|this, e: &gpui_kit::MouseMoveEvent, _, cx| {
                    let was_idle = this.call_idle();
                    this.call_pointer_at = Instant::now();
                    if this.drag_pip(e.position) || was_idle {
                        cx.notify();
                    }
                }))
                .on_mouse_up(MouseButton::Left, cx.listener(move |this, e: &gpui_kit::MouseUpEvent, _, cx| this.drop_pip(e.position, stage_size, cx)))
                .on_mouse_up_out(MouseButton::Left, cx.listener(move |this, e: &gpui_kit::MouseUpEvent, _, cx| this.drop_pip(e.position, stage_size, cx)))
                .child(div().size_full().px_4().child(stage))
                .children(self.self_view(stage_size, window, cx))
                .child(div().absolute().top_0().left_0().right_0().child(fade("call-header", -8., self.call_header(cx))))
                .child(div().absolute().bottom_0().left_0().right_0().child(fade("call-controls", 8., self.call_controls(cx).into_any_element())))
                .into_any_element(),
        )
    }

    // Our own stream in a corner of the stage, straight from the capture: it hangs from the pointer
    // while dragged and glides into the corner it's thrown toward. Clicking it watches it full size.
    fn self_view(&self, stage: (f32, f32), window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let call = self.call.as_ref()?;
        call.sharing.as_ref()?;
        let watching_self = self.me.as_ref().is_some_and(|m| call.watched(&m.id).is_some());
        if !self.pip_shown || watching_self {
            return None;
        }
        let target = self.pip_corner.spot(stage);
        let (x, y) = match (&self.pip_drag, self.pip_snap) {
            (Some(drag), _) => drag.pos,
            (None, Some((from, at))) if at.elapsed() < PIP_SNAP => {
                window.request_animation_frame();
                let t = crate::motion::back_out((at.elapsed().as_secs_f32() / PIP_SNAP.as_secs_f32()).min(1.));
                (from.0 + (target.0 - from.0) * t, from.1 + (target.1 - from.1) * t)
            }
            _ => target,
        };
        let frame: AnyElement = match &call.self_view {
            Some((frame, _)) => img(ImageSource::Render(frame.clone())).size_full().object_fit(gpui_kit::ObjectFit::Cover).into_any_element(),
            None => div().size_full().bg(palette::call_tile()).into_any_element(),
        };
        let viewers = self.own_viewers();
        let dragging = self.pip_drag.is_some();
        Some(
            div()
                .id("self-view")
                .group("self-view")
                .absolute()
                .left(px(x))
                .top(px(y))
                .w(px(PIP_WIDTH))
                .h(px(PIP_WIDTH * 9. / 16.))
                .rounded(px(10.))
                .overflow_hidden()
                .border_1()
                .border_color(gpui_kit::white().opacity(0.12))
                .shadow_lg()
                .cursor(if dragging { gpui_kit::CursorStyle::ClosedHand } else { gpui_kit::CursorStyle::OpenHand })
                .child(frame)
                .child(
                    div()
                        .absolute()
                        .left(px(8.))
                        .right(px(8.))
                        .bottom(px(8.))
                        .flex()
                        .items_center()
                        .gap_1()
                        .text_size(px(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(gpui_kit::white())
                        .child(ui::live_badge())
                        .child("Your stream")
                        .child(div().flex_1())
                        .when(viewers > 0, |d| d.child(div().px_1().rounded(px(6.)).bg(gpui_kit::black().opacity(0.56)).child(format!("{viewers} watching")))),
                )
                .child(
                    div()
                        .id("self-view-close")
                        .absolute()
                        .top(px(6.))
                        .right(px(6.))
                        .size(px(24.))
                        .rounded_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(gpui_kit::black().opacity(0.6))
                        .text_color(gpui_kit::white())
                        .opacity(0.)
                        .group_hover("self-view", |s| s.opacity(1.))
                        .hover(|s| s.bg(gpui_kit::black().opacity(0.85)))
                        .child(Icon::new(Glyph::X).size(px(14.)))
                        .tooltip(|window, cx| Tooltip::new("Hide Self-View").build(window, cx))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.pip_shown = false;
                            cx.notify();
                        })),
                )
                .on_mouse_down(MouseButton::Left, cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    this.pip_snap = None;
                    this.pip_drag = Some(PipDrag { start_mouse: e.position, start: (x, y), pos: (x, y), samples: vec![(e.position, Instant::now())] });
                    cx.notify();
                }))
                .with_animation("self-view-in", Animation::new(Duration::from_millis(220)), |d, t| d.opacity(t.min(1.)))
                .into_any_element(),
        )
    }

    // Moves a dragged self-view with the pointer; false when nothing is being dragged.
    fn drag_pip(&mut self, mouse: Point<Pixels>) -> bool {
        let Some(drag) = &mut self.pip_drag else { return false };
        drag.pos = (drag.start.0 + f32::from(mouse.x - drag.start_mouse.x), drag.start.1 + f32::from(mouse.y - drag.start_mouse.y));
        drag.samples.push((mouse, Instant::now()));
        if drag.samples.len() > 6 {
            drag.samples.remove(0);
        }
        true
    }

    // Lets go of the self-view: a click (barely moved) watches our stream full size; a drag glides
    // to the corner it was thrown toward.
    fn drop_pip(&mut self, mouse: Point<Pixels>, stage: (f32, f32), cx: &mut Context<Self>) {
        let Some(drag) = self.pip_drag.take() else { return };
        let moved = f32::from(mouse.x - drag.start_mouse.x).hypot(f32::from(mouse.y - drag.start_mouse.y));
        if moved < 4. {
            if let Some(me) = self.me.as_ref().map(|m| m.id.clone()) {
                self.watch_stream(me, cx);
            }
            return cx.notify();
        }
        let (first, last) = (drag.samples.first().copied(), drag.samples.last().copied());
        let velocity = match (first, last) {
            (Some((a, ta)), Some((b, tb))) if tb > ta => {
                let dt = (tb - ta).as_secs_f32();
                (f32::from(b.x - a.x) / dt, f32::from(b.y - a.y) / dt)
            }
            _ => (0., 0.),
        };
        let center = (drag.pos.0 + PIP_WIDTH / 2., drag.pos.1 + PIP_WIDTH * 9. / 32.);
        self.pip_corner = Corner::thrown(center, velocity, stage);
        self.pip_snap = Some((drag.pos, Instant::now()));
        cx.notify();
    }

    // Discord's full screen shows the call alone: no servers, channels or user panel.
    pub(crate) fn set_call_fullscreen(&mut self, on: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.call_fullscreen = on;
        if window.is_fullscreen() != on {
            window.toggle_fullscreen();
        }
        cx.notify();
    }

    // Still pointer for a while, and nothing open that belongs to the bar.
    // GAP: Discord's idle timeout isn't in the cached bundle chunks; 3 seconds by eye.
    fn call_idle(&self) -> bool {
        self.call_pointer_at.elapsed() > CONTROLS_IDLE && self.device_menu.is_none() && self.menu.is_none()
    }

    // The channel name, or while watching: whose screen, its quality, LIVE and who else is watching.
    fn call_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let call = self.call.as_ref().expect("in a call");
        let bar = div()
            .h(px(HEADER_HEIGHT))
            .px_4()
            .flex()
            .items_center()
            .gap_2()
            .bg(gpui_kit::linear_gradient(180., gpui_kit::linear_color_stop(gpui_kit::black().opacity(0.6), 0.), gpui_kit::linear_color_stop(gpui_kit::transparent_black(), 1.)))
            .text_color(gpui_kit::white());
        let Some(watching) = call.focused() else {
            let people = self.participants().len();
            return bar
                .child(Icon::new(Glyph::VolumeHigh).size(px(20.)).text_color(gpui_kit::white().opacity(0.7)))
                .child(div().font_weight(FontWeight::SEMIBOLD).child(call.channel.name.clone()))
                .child(div().text_size(px(14.)).text_color(gpui_kit::white().opacity(0.6)).child(format!("{people} in voice")))
                .into_any_element();
        };
        let several = call.watching.len() > 1;
        let suffix = format!(":{}", watching.streamer_id);
        let viewers: Vec<String> = self.stream_viewers.iter().find(|(key, _)| key.ends_with(&suffix)).map(|(_, v)| v.clone()).unwrap_or_default();
        // GAP: the frame rate isn't reported by the voice engine, so only the resolution shows.
        let quality = watching.frame.as_ref().map(|f| format!("{}P", f.size(0).height.0));
        let avatars: Vec<AnyElement> = viewers.iter().take(3).map(|id| img(self.user_avatar(id)).size(px(20.)).rounded_full().border_2().border_color(gpui_kit::black()).ml(px(-6.)).into_any_element()).collect();
        let title = match several {
            true => format!("Watching {} streams", call.watching.len()),
            false => format!("{}'s Screen", self.user_name(&watching.streamer_id)),
        };
        let layout = call.stream_layout;
        let segment = |id: &'static str, text: &'static str, value: StreamLayout| {
            ui::segment(id, text, layout == value, cx).on_click(cx.listener(move |this, _, _, cx| {
                if let Some(call) = &mut this.call {
                    call.stream_layout = value;
                }
                cx.notify();
            }))
        };
        bar.child(Icon::new(Glyph::Screen).size(px(20.)).text_color(gpui_kit::white().opacity(0.7)))
            .child(div().font_weight(FontWeight::SEMIBOLD).truncate().child(title))
            .when(!several, |b| {
                b.children(quality.map(|q| ui::badge(gpui_kit::white().opacity(0.16), gpui_kit::white()).child(q)))
                    .child(ui::live_badge())
                    .when(!watching.arrivals.is_empty(), |b| b.child({
                        let (health, fps) = stream_health(&watching.arrivals, Instant::now());
                        let (label, color): (&str, Hsla) = match health {
                            Health::Smooth => ("Smooth", rgb(0x41A063).into()),
                            Health::Choppy => ("Choppy", rgb(0xCE8100).into()),
                            Health::Stalled => ("Stalled", rgb(0xEB5F5E).into()),
                        };
                        div()
                            .id("stream-health")
                            .flex()
                            .items_center()
                            .gap_1()
                            .text_size(px(12.))
                            .text_color(gpui_kit::white().opacity(0.75))
                            .child(div().size(px(8.)).rounded_full().bg(color))
                            .child(label)
                            .tooltip(move |window, cx| Tooltip::new(format!("{label}: {fps:.0} frames per second")).build(window, cx))
                    }))
                    .when(!viewers.is_empty(), |b| {
                        b.child(div().pl(px(6.)).flex().items_center().children(avatars).when(viewers.len() > 3, |d| d.child(div().pl_1().text_size(px(12.)).font_weight(FontWeight::BOLD).child(format!("+{}", viewers.len() - 3)))))
                    })
            })
            .child(div().flex_1())
            .when(several, |b| b.child(ui::segmented().w(px(220.)).child(segment("layout-focus", "Focus", StreamLayout::Focus)).child(segment("layout-split", "Side by Side", StreamLayout::Split))))
            .child(ui::button("stop-watching", Tone::OnMedia, cx).child(if several { "Stop All" } else { "Stop Watching" }).on_click(cx.listener(|this, _, _, cx| this.stop_watching(cx))))
            .into_any_element()
    }

    // The streams we watch, laid out by `stream_layout`, over a strip of everyone else (folded away
    // with the bar's toggle). Unwatched streams sit in the strip with Watch Too.
    fn stream_stage(&self, tiles: &[Tile], width: f32, height: f32, cx: &mut Context<Self>) -> AnyElement {
        let call = self.call.as_ref().expect("in a call");
        let strip_w = STRIP_HEIGHT * 16. / 9.;
        let strip: Vec<AnyElement> = tiles
            .iter()
            .filter(|t| !matches!(t, Tile::Stream(s) if call.watched(&s.user_id).is_some()))
            .take(STRIP_LIMIT)
            .map(|t| self.tile(t, strip_w, STRIP_HEIGHT, true, cx))
            .collect();
        let strip_shown = !self.call_strip_hidden && !strip.is_empty();
        let main_h = (height - HEADER_HEIGHT - CONTROLS_HEIGHT - if strip_shown { STRIP_HEIGHT + TILE_GAP } else { 0. }).max(120.);
        let focused = call.focused().map(|w| w.streamer_id.clone()).unwrap_or_default();
        let main: AnyElement = match call.stream_layout {
            StreamLayout::Focus => {
                let others: Vec<AnyElement> = call
                    .watching
                    .iter()
                    .filter(|w| w.streamer_id != focused)
                    .map(|w| self.stream_video(w, width * 0.24, main_h * 0.3, true, cx))
                    .collect();
                let big = call.focused().map(|w| self.stream_video(w, width, main_h, false, cx));
                div()
                    .relative()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .children(big)
                    .when(!others.is_empty(), |d| d.child(div().absolute().right(px(12.)).bottom(px(12.)).flex().flex_col().gap(px(TILE_GAP)).children(others)))
                    .into_any_element()
            }
            StreamLayout::Split => {
                let n = call.watching.len();
                let cols = (n as f32).sqrt().ceil().max(1.) as usize;
                let rows = n.div_ceil(cols);
                let (cell_w, cell_h) = ((width - TILE_GAP * (cols as f32 - 1.)) / cols as f32, (main_h - TILE_GAP * (rows as f32 - 1.)) / rows as f32);
                let videos: Vec<AnyElement> = call.watching.iter().map(|w| self.stream_video(w, cell_w, cell_h, false, cx)).collect();
                div().size_full().flex().flex_wrap().items_center().justify_center().content_center().gap(px(TILE_GAP)).children(videos).into_any_element()
            }
        };
        div()
            .size_full()
            .pt(px(HEADER_HEIGHT))
            .pb(px(CONTROLS_HEIGHT))
            .flex()
            .flex_col()
            .gap(px(TILE_GAP))
            .child(div().flex_1().min_h_0().child(main))
            .when(strip_shown, |d| d.child(div().flex_none().flex().justify_center().gap(px(TILE_GAP)).children(strip)))
            .into_any_element()
    }

    // One watched stream fitted into `max_w` x `max_h`. Hovering shows its name and app, its volume,
    // and buttons to focus or stop it; a small one (Focus layout) swaps in on click.
    fn stream_video(&self, watch: &crate::Watching, max_w: f32, max_h: f32, small: bool, cx: &mut Context<Self>) -> AnyElement {
        let call = self.call.as_ref().expect("in a call");
        let (fw, fh) = watch.frame.as_ref().map_or((16., 9.), |f| (f.size(0).width.0 as f32, f.size(0).height.0 as f32));
        let scale = (max_w / fw).min(max_h / fh);
        let (w, h) = (fw * scale, fh * scale);
        let id = watch.streamer_id.clone();
        let name = self.user_name(&id);
        let several = call.watching.len() > 1;
        let body: AnyElement = match &watch.frame {
            Some(frame) => img(ImageSource::Render(frame.clone())).size_full().into_any_element(),
            None => div().size_full().bg(palette::call_tile()).flex().items_center().justify_center().text_color(gpui_kit::white().opacity(0.7)).child(format!("Joining {name}'s stream…")).into_any_element(),
        };
        let group = SharedString::from(format!("stream-{id}"));
        let app = self.activity_name(&id);
        let (focus_id, stop_id, volume_id) = (id.clone(), id.clone(), id.clone());
        let overlay = div()
            .absolute()
            .left_0()
            .right_0()
            .bottom_0()
            .p_2()
            .flex()
            .items_center()
            .gap_2()
            .text_color(gpui_kit::white())
            .bg(gpui_kit::linear_gradient(180., gpui_kit::linear_color_stop(gpui_kit::transparent_black(), 0.), gpui_kit::linear_color_stop(gpui_kit::black().opacity(0.75), 1.)))
            .opacity(0.)
            .group_hover(group.clone(), |s| s.opacity(1.))
            .child(div().min_w_0().truncate().font_weight(FontWeight::SEMIBOLD).child(match app {
                Some(app) => format!("{name} · {app}"),
                None => name.clone(),
            }))
            .child(div().flex_1())
            .child(
                ui::round_button(SharedString::from(format!("volume-{id}")), Glyph::VolumeHigh, "Stream Volume", 28.)
                    .on_mouse_down(MouseButton::Left, cx.listener(move |this, e: &MouseDownEvent, _, cx| this.open_stream_volume(volume_id.clone(), e.position, cx))),
            )
            .when(several && call.stream_layout == StreamLayout::Split, |d| {
                d.child(ui::button(SharedString::from(format!("focus-{id}")), Tone::OnMedia, cx).child("Focus").on_click(cx.listener(move |this, _, _, cx| this.focus_stream(&focus_id, cx))))
            })
            .child(ui::button(SharedString::from(format!("unwatch-{id}")), Tone::OnMedia, cx).child("Stop Watching").on_click(cx.listener(move |this, _, _, cx| this.unwatch_stream(&stop_id, cx))));
        let swap_id = id.clone();
        div()
            .id(SharedString::from(format!("stream-video-{id}-{small}")))
            .group(group)
            .relative()
            .w(px(w))
            .h(px(h))
            .flex_none()
            .rounded(px(TILE_RADIUS))
            .overflow_hidden()
            .bg(gpui_kit::black())
            .child(body)
            .when(several, |d| d.child(div().absolute().left(px(8.)).top(px(8.)).flex().gap_1().child(ui::live_badge()).child(ui::badge(gpui_kit::black().opacity(0.56), gpui_kit::white()).child(name.clone()))))
            .when(!small, |d| d.child(overlay))
            .when(small, |d| {
                d.cursor_pointer()
                    .border_1()
                    .border_color(gpui_kit::white().opacity(0.14))
                    .shadow_lg()
                    .on_click(cx.listener(move |this, _, _, cx| this.focus_stream(&swap_id, cx)))
            })
            .with_animation(SharedString::from(format!("stream-in-{id}-{small}")), Animation::new(Duration::from_millis(220)), |d, t| d.opacity(t.min(1.)))
            .into_any_element()
    }

    // What someone is playing or streaming, from their presence, for name tags.
    fn activity_name(&self, user_id: &str) -> Option<String> {
        let activity = self.presences.get(user_id)?.activity.as_ref()?;
        matches!(activity.kind, discord::ActivityKind::Playing | discord::ActivityKind::Streaming | discord::ActivityKind::Competing).then(|| activity.text.clone())
    }

    fn focus_stream(&mut self, streamer_id: &str, cx: &mut Context<Self>) {
        if let Some(call) = &mut self.call {
            call.focus = Some(streamer_id.to_owned());
            call.stream_layout = StreamLayout::Focus;
        }
        cx.notify();
    }

    fn open_stream_volume(&mut self, streamer_id: String, at: Point<Pixels>, cx: &mut Context<Self>) {
        cx.stop_propagation();
        self.open_live_menu(
            at,
            move |this| {
                let volume = this.stream_volumes.get(&streamer_id).copied().unwrap_or(1.);
                let id = streamer_id.clone();
                vec![crate::menu::slider("Stream Volume", format!("stream-volume-{streamer_id}"), volume / 2., move |this, f, cx| this.set_stream_volume(&id, f * 2., cx))]
            },
            cx,
        );
    }

    fn set_stream_volume(&mut self, streamer_id: &str, volume: f32, cx: &mut Context<Self>) {
        self.stream_volumes.insert(streamer_id.to_owned(), volume);
        if let Some(call) = &self.call {
            call.session.set_stream_volume(streamer_id, volume);
        }
        cx.notify();
    }

    // What the stage says when the stream we watched ends, in place of a blank or an error.
    fn ended_stage(&self, streamer: &str, cx: &mut Context<Self>) -> AnyElement {
        let mine = self.me.as_ref().is_some_and(|m| m.id == streamer);
        let title = if mine { "You stopped streaming".to_owned() } else { format!("{} stopped streaming", self.user_name(streamer)) };
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_3()
            .child(div().text_size(px(18.)).font_weight(FontWeight::BOLD).text_color(gpui_kit::white()).child(title))
            .child(ui::button("ended-back", Tone::OnMedia, cx).child("Back to Call").on_click(cx.listener(|this, _, _, cx| {
                if let Some(call) = &mut this.call {
                    call.ended = None;
                }
                cx.notify();
            })))
            .with_animation("ended-in", Animation::new(Duration::from_millis(240)), |d, t| d.opacity(t.min(1.)).mt(px((1. - crate::motion::back_out(t)) * 10.)))
            .into_any_element()
    }

    // A participant (camera or avatar, green ring while speaking, name and mute state in Discord's
    // overlay pill) or someone's Go Live (their preview dimmed, with Watch Stream).
    fn tile(&self, tile: &Tile, w: f32, h: f32, compact: bool, cx: &mut Context<Self>) -> AnyElement {
        let call = self.call.as_ref().expect("in a call");
        let (Tile::User(state) | Tile::Stream(state)) = tile;
        let user = state.user.clone().unwrap_or_else(|| User {
            id: state.user_id.clone(),
            name: self.user_name(&state.user_id),
            avatar: discord::avatar_url(&state.user_id, None),
            bot: false,
            tag: None,
            nameplate: None,
            decoration: None,
        });
        let me = self.me.as_ref().is_some_and(|m| m.id == state.user_id);
        let (muted, deafened) = if me { (self.muted || self.deafened, self.deafened) } else { (state.muted, state.deafened) };
        let is_stream = matches!(tile, Tile::Stream(_));
        let talking = !is_stream && self.speaking(&state.user_id);
        let margin = if compact { 4. } else { 12. };
        let face: AnyElement = match (is_stream, call.cameras.get(&state.user_id)) {
            (true, _) => div()
                .size_full()
                .bg(palette::call_tile())
                .children(self.stream_preview(&state.user_id).map(|url| img(url).size_full().object_fit(gpui_kit::ObjectFit::Cover).opacity(0.15)))
                .into_any_element(),
            (false, Some(frame)) => img(ImageSource::Render(frame.clone())).size_full().object_fit(gpui_kit::ObjectFit::Cover).into_any_element(),
            (false, None) => div()
                .size_full()
                .bg(self.avatar_colors.get(&state.user_id).copied().flatten().unwrap_or(palette::call_tile()))
                .flex()
                .items_center()
                .justify_center()
                .child(img(user.avatar.clone()).size(px((h * 0.4).clamp(32., 80.))).rounded_full())
                .into_any_element(),
        };
        let watching = call.watched(&state.user_id).is_some();
        let watchable = is_stream && !watching && (!me || call.sharing.is_some());
        let status = |glyph: Glyph| {
            let pad = if compact { 4. } else { 6. };
            div().size(px(20. + pad * 2.)).p(px(pad)).flex_none().rounded_full().bg(gpui_kit::black().opacity(0.56)).text_color(gpui_kit::white()).child(Icon::new(glyph).size_full())
        };
        let label = div()
            .absolute()
            .left(px(margin))
            .bottom(px(margin))
            .max_w(px(w - margin * 2.))
            .flex()
            .items_center()
            .gap_1()
            .child(
                div()
                    .min_w_0()
                    .px(px(12.))
                    .py(px(6.))
                    .rounded(px(TILE_RADIUS))
                    .bg(gpui_kit::black().opacity(0.56))
                    .text_color(gpui_kit::white())
                    .text_size(px(14.))
                    .font_weight(FontWeight::MEDIUM)
                    .flex()
                    .items_center()
                    .gap_1()
                    .when(is_stream, |d| d.child(Icon::new(Glyph::ShareScreen).size(px(16.)).flex_none()))
                    .child(div().flex_none().child(user.name.clone()))
                    .children(self.activity_name(&state.user_id).filter(|_| !compact).map(|app| div().min_w_0().truncate().font_weight(FontWeight::NORMAL).text_color(gpui_kit::white().opacity(0.7)).child(format!("· {app}")))),
            )
            .when(!is_stream && muted, |d| d.child(status(Glyph::MicOff)))
            .when(!is_stream && deafened, |d| d.child(status(Glyph::HeadphonesOff)));
        let streamer = state.user_id.clone();
        let menu_user = state.user_id.clone();
        div()
            .id(SharedString::from(format!("tile-{}-{is_stream}", state.user_id)))
            .relative()
            .w(px(w))
            .h(px(h))
            .flex_none()
            .rounded(px(TILE_RADIUS))
            .overflow_hidden()
            .child(face)
            .when(is_stream, |d| d.child(div().absolute().top(px(8.)).right(px(8.)).child(ui::live_badge())))
            .when(watchable, |d| {
                d.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(ui::button(SharedString::from(format!("watch-{streamer}")), Tone::OnMedia, cx).child(if call.watching.is_empty() { "Watch Stream" } else { "Watch Too" }).on_click(cx.listener(move |this, _, _, cx| this.watch_stream(streamer.clone(), cx)))),
                )
            })
            .child(label)
            // Speaking: a 2px green ring inside the tile edge, as Discord draws its tile border.
            .child(div().absolute().inset_0().rounded(px(TILE_RADIUS)).border_2().border_color(if talking { rgb(0x3D9E60).into() } else { gpui_kit::transparent_black() }))
            .on_mouse_down(MouseButton::Right, cx.listener(move |this, e: &MouseDownEvent, _, cx| this.voice_user_menu(&menu_user, e.position, cx)))
            .into_any_element()
    }

    // Discord's call bar: invite on the left; mic and camera with their device chevrons, share,
    // activities, soundboard and the red hang-up in the middle; strip and fullscreen on the right.
    fn call_controls(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let call = self.call.as_ref().expect("in a call");
        let muted = self.muted || self.deafened;
        let connected = call.stage == Stage::Connected;
        let sharing = call.sharing.is_some();
        let fullscreen = self.call_fullscreen;
        let invite = call.guild_id.clone().map(|guild| {
            let channel = call.channel.id.clone();
            ui::round_button("call-invite", Glyph::GroupPlus, "Invite to Voice", 40.).on_click(cx.listener(move |this, _, window, cx| this.show_invite(guild.clone(), channel.clone(), window, cx)))
        });
        let strip_toggle = (!call.watching.is_empty()).then(|| {
            let hidden = self.call_strip_hidden;
            ui::round_button("call-strip", if hidden { Glyph::ChevronUp } else { Glyph::ChevronDown }, if hidden { "Show Members" } else { "Hide Members" }, 40.).on_click(cx.listener(|this, _, _, cx| {
                this.call_strip_hidden = !this.call_strip_hidden;
                cx.notify();
            }))
        });
        let center = div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                // Button and chevron sit 1px apart: the stage shows through as their divider, so the
                // two translucent fills never overlap into a lighter strip.
                div()
                    .flex()
                    .gap(px(1.))
                    .child(
                        split_button("call-mic", if muted { Glyph::MicOff } else { Glyph::Mic }, if muted { "Unmute" } else { "Mute" }, muted, cx)
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_mute(cx))),
                    )
                    .child(chevron("call-mic-devices", cx).on_mouse_down(MouseButton::Left, cx.listener(|this, e: &MouseDownEvent, _, cx| this.open_device_menu(DeviceKind::Input, e.position, cx)))),
            )
            .child(
                // Button and chevron sit 1px apart: the stage shows through as their divider, so the
                // two translucent fills never overlap into a lighter strip.
                div()
                    .flex()
                    .gap(px(1.))
                    .child(
                        split_button("call-camera", if call.camera_on { Glyph::Video } else { Glyph::VideoOff }, if call.camera_on { "Turn Off Camera" } else { "Turn On Camera" }, !call.camera_on, cx)
                            .when(!connected, |b| b.opacity(0.5))
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_camera(cx))),
                    )
                    .child(chevron("call-camera-devices", cx).on_mouse_down(MouseButton::Left, cx.listener(|this, e: &MouseDownEvent, _, cx| this.open_device_menu(DeviceKind::Camera, e.position, cx)))),
            )
            .child(match sharing {
                true => split_button("call-share", Glyph::ShareScreen, "Stream Options", false, cx)
                    .rounded(px(12.))
                    .bg(Hsla::from(rgb(0x008545)).opacity(0.24))
                    .text_color(rgb(0x62B77C))
                    .on_mouse_down(MouseButton::Left, cx.listener(|this, e: &MouseDownEvent, _, cx| this.open_stream_menu(e.position, cx))),
                false => split_button("call-share", Glyph::ShareScreen, "Share Your Screen", false, cx)
                    .rounded(px(12.))
                    .when(!connected, |b| b.opacity(0.5))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_screen_share(cx))),
            })
            .child(
                split_button("call-activities", Glyph::Activities, "Start an Activity", false, cx)
                    .rounded(px(12.))
                    .when(!connected, |b| b.opacity(0.5))
                    .on_click(cx.listener(|this, _, _, cx| this.show_toast("Activities aren't in Rustcord yet", cx))),
            )
            .child(
                split_button("call-soundboard", Glyph::Soundboard, "Open Soundboard", false, cx)
                    .rounded(px(12.))
                    .when(!connected, |b| b.opacity(0.5))
                    .on_click(cx.listener(|this, _, _, cx| this.show_toast("The soundboard isn't in Rustcord yet", cx))),
            )
            .child(
                div()
                    .id("call-hang-up")
                    .ml_2()
                    .w(px(64.))
                    .h(px(48.))
                    .rounded(px(12.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .bg(palette::danger())
                    .text_color(gpui_kit::white())
                    .hover(|s| s.bg(palette::danger_hover()))
                    .child(Icon::new(Glyph::CallEnd).size(px(24.)))
                    .tooltip(|window, cx| Tooltip::new("Disconnect").build(window, cx))
                    .on_click(cx.listener(|this, _, _, cx| this.leave_voice(cx))),
            );
        div()
            .h(px(CONTROLS_HEIGHT))
            .px_4()
            .flex()
            .items_center()
            .bg(gpui_kit::linear_gradient(0., gpui_kit::linear_color_stop(gpui_kit::black().opacity(0.6), 0.), gpui_kit::linear_color_stop(gpui_kit::transparent_black(), 1.)))
            .child(div().flex_1().flex().children(invite))
            .child(center)
            .child(
                div()
                    .flex_1()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .child({
                        let open = self.call_chat;
                        ui::round_button("call-chat", Glyph::ChatRound, if open { "Hide Chat" } else { "Show Chat" }, 40.)
                            .when(open, |b| b.bg(gpui_kit::white()).text_color(gpui_kit::black()))
                            .on_click(cx.listener(|this, _, window, cx| this.toggle_call_chat(window, cx)))
                    })
                    .children(strip_toggle)
                    .child(
                        ui::round_button("call-fullscreen", if fullscreen { Glyph::FullscreenExit } else { Glyph::Fullscreen }, if fullscreen { "Exit Full Screen" } else { "Full Screen" }, 40.)
                            .on_click(cx.listener(move |this, _, window, cx| this.set_call_fullscreen(!fullscreen, window, cx))),
                    ),
            )
    }

    pub(crate) fn open_device_menu(&mut self, kind: DeviceKind, at: Point<Pixels>, cx: &mut Context<Self>) {
        cx.stop_propagation();
        let devices = match kind {
            DeviceKind::Input => voice::input_devices(),
            DeviceKind::Output => voice::output_devices(),
            DeviceKind::Camera => voice::cameras(),
        };
        self.device_menu = Some(DeviceMenu { kind, at, devices });
        cx.notify();
    }

    fn choose_device(&mut self, kind: DeviceKind, id: Option<String>, cx: &mut Context<Self>) {
        self.device_menu = None;
        self.update_prefs(cx, |p| match kind {
            DeviceKind::Input => p.input_device = id,
            DeviceKind::Output => p.output_device = id,
            DeviceKind::Camera => p.camera = id,
        });
        if kind == DeviceKind::Camera
            && let Some(call) = &self.call
            && call.camera_on
            && let Err(error) = call.session.set_camera(self.prefs.camera.clone())
        {
            self.voice_error = Some(error.into());
        }
    }

    pub(crate) fn render_call_overlays(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut out = Vec::new();
        out.extend(self.connection_overlay(cx));
        out.extend(self.device_overlay(window, cx));
        out
    }

    // Discord's connection popout: ping over time, the numbers behind it, and how the call is secured.
    fn connection_overlay(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.connection_info {
            return None;
        }
        let call = self.call.as_ref()?;
        let theme = cx.theme().clone();
        let stats = call.stats.clone().unwrap_or_default();
        let (_, color) = signal(call.stats.as_ref().map(|s| s.ping_ms));
        let peak = stats.last_pings.iter().copied().max().unwrap_or(1).max(100) as f32;
        let bars = div().h(px(64.)).flex().items_end().gap(px(2.)).children(stats.last_pings.iter().map(|&p| {
            let (_, c) = signal(Some(p));
            div().flex_1().h(px((p as f32 / peak * 60.).max(2.))).rounded_t(px(2.)).bg(c)
        }));
        let row = |label: &'static str, value: String| {
            div().flex().justify_between().gap_4().text_size(px(14.)).child(div().flex_none().text_color(theme.muted_foreground).child(label)).child(div().min_w_0().truncate().font_weight(FontWeight::MEDIUM).child(value))
        };
        let measured = call.stats.is_some();
        let dash = |value: String| if measured { value } else { "—".into() };
        let card = div()
            .id("connection-card")
            .absolute()
            .left(px(8.))
            .bottom(px(self.panel_stack_height() + 16.))
            .w(px(320.))
            .p_4()
            .rounded(px(8.))
            .bg(theme.popover)
            .border_1()
            .border_color(theme.input)
            .shadow_lg()
            .flex()
            .flex_col()
            .gap_3()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(div().flex().items_center().gap_2().text_color(color).child(Icon::new(signal(call.stats.as_ref().map(|s| s.ping_ms)).0).size(px(20.))).child(div().font_weight(FontWeight::SEMIBOLD).child(stage_text(call.stage))))
            .child(bars)
            .child(row("Average ping", dash(format!("{} ms", stats.avg_ping_ms))))
            .child(row("Last ping", dash(format!("{} ms", stats.ping_ms))))
            .child(row("Outbound packet loss rate", dash(format!("{:.1}%", stats.outbound_loss_pct))))
            .child(row("Inbound packet loss rate", dash(format!("{:.1}%", stats.inbound_loss_pct))))
            .child(div().h(px(1.)).bg(theme.border))
            .child(row("Hostname", dash(stats.endpoint.clone())))
            .child(row("Encryption", dash(stats.encryption.clone())))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_size(px(14.))
                    .text_color(if stats.dave { palette::status(discord::Status::Online) } else { theme.muted_foreground })
                    .child(Icon::new(Glyph::ShieldLock).size(px(16.)))
                    .child(if stats.dave { "End-to-end encrypted" } else { "Encrypted in transit" }),
            )
            .child(div().text_size(px(12.)).text_color(theme.muted_foreground).child("If your average ping stays above 250 ms, try another server region or check your network."));
        Some(call_backdrop("connection-backdrop", cx).child(card).into_any_element())
    }

    // The chevron menus: pick a device, set its volume, or jump to Voice & Video settings.
    fn device_overlay(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let menu = self.device_menu.as_ref()?;
        let theme = cx.theme().clone();
        let kind = menu.kind;
        let chosen = match kind {
            DeviceKind::Input => self.prefs.input_device.clone(),
            DeviceKind::Output => self.prefs.output_device.clone(),
            DeviceKind::Camera => self.prefs.camera.clone(),
        };
        let title = match kind {
            DeviceKind::Input => "Input Device",
            DeviceKind::Output => "Output Device",
            DeviceKind::Camera => "Camera",
        };
        let mut options: Vec<(Option<String>, String)> = Vec::new();
        if kind != DeviceKind::Camera {
            options.push((None, "Default".into()));
        }
        options.extend(menu.devices.iter().map(|d| (Some(d.id.clone()), d.name.clone())));
        let rows: Vec<AnyElement> = options
            .into_iter()
            .enumerate()
            .map(|(i, (id, name))| {
                let selected = id == chosen;
                menu_row(SharedString::from(format!("device-{i}")), name, selected, cx).on_click(cx.listener(move |this, _, _, cx| this.choose_device(kind, id.clone(), cx))).into_any_element()
            })
            .collect();
        let volume = match kind {
            DeviceKind::Input => Some(("Input Volume", self.prefs.input_volume)),
            DeviceKind::Output => Some(("Output Volume", self.prefs.output_volume)),
            DeviceKind::Camera => None,
        };
        let volume = volume.map(|(label, value)| {
            div()
                .px_2()
                .pt_2()
                .flex()
                .flex_col()
                .gap_1()
                .child(div().text_size(px(12.)).font_weight(FontWeight::BOLD).text_color(theme.muted_foreground).child(label))
                .child(self.range(
                    SharedString::from(format!("menu-{label}")),
                    value / 2.,
                    Fill::Accent,
                    move |this, f, cx| {
                        this.update_prefs(cx, |p| match kind {
                            DeviceKind::Output => p.output_volume = f.clamp(0., 1.) * 2.,
                            _ => p.input_volume = f.clamp(0., 1.) * 2.,
                        })
                    },
                    cx,
                ))
        });
        let viewport = window.viewport_size();
        let card = div()
            .id("device-menu")
            .absolute()
            .left(px((f32::from(menu.at.x) - 120.).clamp(8., f32::from(viewport.width) - 268.)))
            .bottom(px((f32::from(viewport.height) - f32::from(menu.at.y) + 12.).max(8.)))
            .w(px(260.))
            .p_2()
            .rounded(px(8.))
            .bg(theme.popover)
            .border_1()
            .border_color(theme.input)
            .shadow_lg()
            .flex()
            .flex_col()
            .gap_1()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(div().px_2().pb_1().text_size(px(12.)).font_weight(FontWeight::BOLD).text_color(theme.muted_foreground).child(title))
            .children(rows)
            .children(volume)
            .child(div().my_1().h(px(1.)).bg(theme.border))
            .child(
                menu_row("device-settings", "Voice Settings".into(), false, cx)
                    .child(Icon::new(Glyph::Settings).size(px(16.)))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.device_menu = None;
                        this.open_settings(Page::VoiceVideo, window, cx);
                    })),
            );
        Some(call_backdrop("device-backdrop", cx).child(card).into_any_element())
    }

    // Right-click on anyone in voice opens Discord's server user menu with its voice items.
    pub(crate) fn voice_user_menu(&mut self, user_id: &str, at: Point<Pixels>, cx: &mut Context<Self>) {
        let found = self.guilds.iter().find_map(|g| g.voice_states.iter().find(|v| v.user_id == user_id).map(|v| (g.id.clone(), v.user.clone())));
        let guild_id = found.as_ref().map(|(g, _)| g.clone()).or_else(|| self.call.as_ref().and_then(|c| c.guild_id.clone()));
        let user = found.and_then(|(_, u)| u).unwrap_or_else(|| User {
            id: user_id.to_owned(),
            name: self.user_name(user_id),
            avatar: discord::avatar_url(user_id, None),
            bot: false,
            tag: None,
            nameplate: None,
            decoration: None,
        });
        // Anyone in voice, server or DM call, gets Discord's voice items (volume, local mute).
        let surface = crate::user_menu::Surface::ServerVoice;
        self.open_user_menu(user, guild_id, surface, at, cx);
    }

    pub(crate) fn set_user_volume(&mut self, user_id: &str, volume: f32, cx: &mut Context<Self>) {
        if let Some(call) = &self.call {
            call.session.set_user_volume(user_id, volume);
        }
        let user_id = user_id.to_owned();
        self.update_prefs(cx, |p| {
            p.user_volumes.insert(user_id, volume);
        });
    }

    pub(crate) fn toggle_local_mute(&mut self, user_id: &str, cx: &mut Context<Self>) {
        let muted = !self.prefs.local_mutes.iter().any(|u| u == user_id);
        if let Some(call) = &self.call {
            call.session.set_user_muted(user_id, muted);
        }
        let user_id = user_id.to_owned();
        self.update_prefs(cx, |p| match muted {
            true => p.local_mutes.push(user_id),
            false => p.local_mutes.retain(|u| *u != user_id),
        });
    }
}

fn call_backdrop(id: &'static str, cx: &mut Context<Rustcord>) -> Stateful<Div> {
    div().id(id).absolute().inset_0().occlude().on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _, _, cx| {
            this.dismiss_call_menus();
            cx.notify();
        }),
    )
}

// `hover_ink` recolours the icon on hover, as the hang-up button turns red.
fn panel_button_tinted(id: &'static str, glyph: Glyph, label: &'static str, hover_ink: Option<Hsla>, cx: &mut Context<Rustcord>) -> Stateful<Div> {
    let theme = cx.theme().clone();
    let ink = hover_ink.unwrap_or(theme.foreground);
    div()
        .id(id)
        .size(px(32.))
        .flex_none()
        .rounded(px(8.))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .text_color(theme.foreground.opacity(0.7))
        .hover(move |s| s.bg(theme.foreground.opacity(0.1)).text_color(ink))
        .child(Icon::new(glyph).size(px(20.)))
        .tooltip(move |window, cx| Tooltip::new(label).build(window, cx))
}

// The panel's row of equal-width call buttons, from Discord's stylesheet: 32px, 8px radius, a 12%
// wash with a 16% border; while on (sharing, camera), green at 12% with a green icon.
fn wide_button(id: &'static str, glyph: Glyph, label: &'static str, on: bool, enabled: bool, _cx: &mut Context<Rustcord>) -> Stateful<Div> {
    let wash = |a: f32| palette::wash(a);
    let green = |a: f32| Hsla::from(rgb(0x008545)).opacity(a);
    div()
        .id(id)
        .flex_1()
        .h(px(32.))
        .rounded(px(8.))
        .border_1()
        .border_color(if on { green(0.12) } else { wash(0.16) })
        .bg(if on { green(0.12) } else { wash(0.12) })
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .text_color(if on { rgb(0x62B77C) } else { rgb(0xDCDCDF) })
        .hover(move |s| s.bg(if on { green(0.24) } else { wash(0.2) }))
        .when(!enabled, |b| b.opacity(0.3))
        .child(Icon::new(glyph).size(px(20.)))
        .tooltip(move |window, cx| Tooltip::new(label).build(window, cx))
}

// A call bar button: 56×48, light on the dark stage, red when that input is off.
fn split_button(id: &'static str, glyph: Glyph, label: &'static str, off: bool, cx: &mut Context<Rustcord>) -> Stateful<Div> {
    let danger = cx.theme().danger;
    div()
        .id(id)
        .w(px(56.))
        .h(px(48.))
        .rounded_l(px(12.))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .bg(gpui_kit::white().opacity(0.1))
        .text_color(if off { danger } else { gpui_kit::white() })
        .hover(|s| s.bg(gpui_kit::white().opacity(0.16)))
        .child(Icon::new(glyph).size(px(24.)))
        .tooltip(move |window, cx| Tooltip::new(label).build(window, cx))
}

fn chevron(id: &'static str, _cx: &mut Context<Rustcord>) -> Stateful<Div> {
    div()
        .id(id)
        .w(px(28.))
        .h(px(48.))
        .rounded_r(px(12.))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .bg(gpui_kit::white().opacity(0.1))
        .text_color(gpui_kit::white())
        .hover(|s| s.bg(gpui_kit::white().opacity(0.16)))
        .child(Icon::new(Glyph::ChevronDown).size(px(16.)))
}

fn menu_row(id: impl Into<SharedString>, label: String, checked: bool, cx: &mut Context<Rustcord>) -> Stateful<Div> {
    ui::menu_row(gpui_kit::ElementId::Name(id.into()), cx).child(div().flex_1().min_w_0().truncate().child(label)).when(checked, |d| d.child(Icon::new(Glyph::Checkmark).size(px(16.))))
}

// A BGRA frame as the data URL Discord's preview upload takes.
fn jpeg_data_url(frame: &voice::VideoFrame) -> Option<String> {
    use base64::Engine;
    let rgb: Vec<u8> = frame.bgra.as_chunks::<4>().0.iter().flat_map(|[b, g, r, _]| [*r, *g, *b]).collect();
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 80).encode(&rgb, frame.width, frame.height, image::ExtendedColorType::Rgb8).ok()?;
    Some(format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(jpeg)))
}

// Discord's CDN serves any avatar as a small PNG, whatever its original format.
fn small_png(avatar: &str) -> String {
    let base = avatar.split('?').next().unwrap_or(avatar);
    let stem = base.rsplit_once('.').map_or(base, |(stem, _)| stem);
    format!("{stem}.png?size=32")
}

async fn avatar_color(url: &str) -> Option<Hsla> {
    let bytes = reqwest::get(url).await.ok()?.bytes().await.ok()?;
    let rgba = image::load_from_memory(&bytes).ok()?.to_rgba8();
    let (r, g, b) = average_color(rgba.as_raw())?;
    let mut color = Hsla::from(gpui_kit::Rgba { r: r as f32 / 255., g: g as f32 / 255., b: b as f32 / 255., a: 1. });
    // Dark enough for white names and the avatar to stand out, as on Discord's tiles.
    color.l = color.l.clamp(0.2, 0.45);
    Some(color)
}

// The alpha-weighted mean colour of RGBA pixels; None when fully transparent.
fn average_color(rgba: &[u8]) -> Option<(u8, u8, u8)> {
    let (mut sum, mut weight) = ([0u64; 3], 0u64);
    for px in rgba.as_chunks::<4>().0 {
        let a = px[3] as u64;
        for (total, channel) in sum.iter_mut().zip(px) {
            *total += *channel as u64 * a;
        }
        weight += a;
    }
    (weight > 0).then(|| ((sum[0] / weight) as u8, (sum[1] / weight) as u8, (sum[2] / weight) as u8))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn avatar_colour_and_url() {
        assert_eq!(average_color(&[255, 0, 0, 255, 0, 0, 255, 255, 9, 9, 9, 0]), Some((127, 0, 127)));
        assert_eq!(average_color(&[1, 2, 3, 0]), None);
        assert_eq!(small_png("https://cdn.discordapp.com/avatars/1/a_b.gif?size=128"), "https://cdn.discordapp.com/avatars/1/a_b.png?size=32");
        assert_eq!(small_png("https://cdn.discordapp.com/embed/avatars/3.png"), "https://cdn.discordapp.com/embed/avatars/3.png?size=32");
        let frame = voice::VideoFrame { width: 2, height: 1, bgra: vec![0, 0, 255, 255, 255, 0, 0, 255] };
        assert!(jpeg_data_url(&frame).is_some_and(|url| url.starts_with("data:image/jpeg;base64,/9j/")));
    }
}

// "1080p 60" or "Source 30", how Rustcord names a stream's quality.
fn quality_label(resolution: u32, fps: u32) -> String {
    match resolution {
        0 => format!("Source {fps}"),
        r => format!("{r}p {fps}"),
    }
}

// A running time: "4:05", or "1:02:09" past an hour.
pub(crate) fn clock(secs: u64) -> String {
    match secs / 3600 {
        0 => format!("{}:{:02}", secs / 60, secs % 60),
        h => format!("{h}:{:02}:{:02}", secs / 60 % 60, secs % 60),
    }
}

#[cfg(test)]
mod pip_tests {
    use super::*;

    #[test]
    fn thrown_corner_follows_the_flick() {
        let stage = (1000., 600.);
        assert_eq!(Corner::thrown((900., 100.), (0., 0.), stage), Corner::TopRight);
        // Released on the right but flicked hard left: it coasts across.
        assert_eq!(Corner::thrown((700., 100.), (-3000., 0.), stage), Corner::TopLeft);
        assert_eq!(Corner::thrown((100., 280.), (0., 400.), stage), Corner::BottomLeft);
    }

    #[test]
    fn clocks_and_quality() {
        assert_eq!(clock(245), "4:05");
        assert_eq!(clock(3729), "1:02:09");
        assert_eq!(quality_label(1080, 60), "1080p 60");
        assert_eq!(quality_label(0, 30), "Source 30");
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Health {
    Smooth,
    Choppy,
    Stalled,
}

// How a watched stream is doing, from when its last frames arrived: no frame for a second is
// stalled; under 15 frames a second over the last two is choppy.
// ponytail: frame rate only; add decode lag once the engine reports it.
pub fn stream_health(arrivals: &std::collections::VecDeque<Instant>, now: Instant) -> (Health, f32) {
    let Some(last) = arrivals.back() else { return (Health::Stalled, 0.) };
    let window = Duration::from_secs(2);
    let recent = arrivals.iter().filter(|t| now.duration_since(**t) <= window).count();
    let fps = recent as f32 / window.as_secs_f32();
    let health = match () {
        _ if now.duration_since(*last) > Duration::from_secs(1) => Health::Stalled,
        _ if fps < 15. => Health::Choppy,
        _ => Health::Smooth,
    };
    (health, fps)
}

#[cfg(test)]
mod health_tests {
    use super::*;

    #[test]
    fn health_from_arrivals() {
        let now = Instant::now();
        let at = |ms: u64| now - Duration::from_millis(ms);
        let smooth: std::collections::VecDeque<Instant> = (0..60).map(|i| at(i * 33)).rev().collect();
        assert_eq!(stream_health(&smooth, now).0, Health::Smooth);
        let choppy: std::collections::VecDeque<Instant> = (0..10).map(|i| at(i * 200)).rev().collect();
        assert_eq!(stream_health(&choppy, now).0, Health::Choppy);
        let stalled: std::collections::VecDeque<Instant> = [at(1500)].into();
        assert_eq!(stream_health(&stalled, now).0, Health::Stalled);
        assert_eq!(stream_health(&Default::default(), now).0, Health::Stalled);
    }
}
