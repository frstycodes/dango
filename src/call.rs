// Everything around a voice call (design/voice.html): the voice panel over the user panel with its
// connection stages, the connection details popover, the call stage with its tiles, header and
// control bar, device menus and the participant menu.
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use gpui_kit::{
    Animation, AnimationExt, AnyElement, App, BoxShadow, Context, Div, ElementId, FontFeatures, FontWeight, Hsla, ImageSource, MouseButton, MouseDownEvent, Pixels, Point, SharedString, Stateful, Window,
    component::{ActiveTheme, Icon, tooltip::Tooltip},
    div, point, prelude::*, px, relative, rgb,
};

use crate::{
    Rustcord,
    discord::{self, User, VoiceState},
    icons::Glyph,
    layout::RAIL_WIDTH,
    menu::{Entry, item, submenu},
    palette,
    settings::Page,
    ui::{self, RADIUS_LG, RADIUS_MD, RADIUS_SM, RADIUS_XS},
    voice::{self, Stage},
};

// .voicebar: 10px padding, a 38px status block (a 20px line over an 18px one), 8px gap, 32px
// buttons, and its 1px hairline.
pub const VOICE_HEIGHT: f32 = 99.;
const PANEL_PAD: f32 = 10.;
// The error bar is the voicebar without its buttons.
const ERROR_HEIGHT: f32 = 59.;
const PREVIEW_AGE: std::time::Duration = std::time::Duration::from_secs(60);
// After a failed preview fetch, how long until the next try.
const PREVIEW_RETRY: std::time::Duration = std::time::Duration::from_secs(20);
// The stage (.call): 16px padding and gaps around a 32px header and 48px controls.
const STAGE_PAD: f32 = 16.;
const STAGE_GAP: f32 = 16.;
const HEAD_HEIGHT: f32 = 32.;
const CONTROLS_HEIGHT: f32 = 48.;
// .tiles: 10px apart, 8px once the grid is three across; avatars shrink in small tiles.
const TILE_GAP: f32 = 10.;
const TILE_GAP_DENSE: f32 = 8.;
const SMALL_TILE: f32 = 240.;
// .filmstrip under a focused stream.
const STRIP_TILE: f32 = 148.;
const STRIP_GAP: f32 = 8.;
const STRIP_LIMIT: usize = 12;
const RECENT_SPEAKER: std::time::Duration = std::time::Duration::from_secs(10);
const CONTROLS_IDLE: Duration = Duration::from_secs(3);
const CHAT_WIDTH: f32 = 340.;
// .pip: 30% of the tile area, 132 to 220px wide, 16:10, 12px from its corner.
const PIP_INSET: f32 = 12.;
const PIP_DECELERATION: f32 = 0.995;
const PIP_SNAP: Duration = ui::DUR_PANEL;
// The connection popover's colour bands (its legend): under 80 ms, 80 to 120, over 120.
const PING_FAIR: u32 = 80;
const PING_BAD: u32 = 120;
// When the panel says "Poor connection": a bad average ping, or this much packet loss.
const LOSS_POOR_PCT: f32 = 5.;
const METER_BARS: usize = 28;

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

const CORNERS: [Corner; 4] = [Corner::TopLeft, Corner::TopRight, Corner::BottomLeft, Corner::BottomRight];

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

    // Top-left of a self-view sitting in this corner of the stage's tile area.
    fn spot(self, stage: (f32, f32)) -> (f32, f32) {
        let (x, y, w, h) = tile_area(stage);
        let (pw, ph) = pip_size(stage);
        let (left, top) = (x + PIP_INSET, y + PIP_INSET);
        let (right, bottom) = (x + w - pw - PIP_INSET, y + h - ph - PIP_INSET);
        match self {
            Corner::TopLeft => (left, top),
            Corner::TopRight => (right, top),
            Corner::BottomLeft => (left, bottom),
            Corner::BottomRight => (right, bottom),
        }
    }
}

// The stage's tile area (left, top, width, height), between its header and controls.
fn tile_area(stage: (f32, f32)) -> (f32, f32, f32, f32) {
    let top = STAGE_PAD + HEAD_HEIGHT + STAGE_GAP;
    (STAGE_PAD, top, stage.0 - STAGE_PAD * 2., (stage.1 - top - CONTROLS_HEIGHT - STAGE_GAP - STAGE_PAD).max(0.))
}

fn pip_size(stage: (f32, f32)) -> (f32, f32) {
    let w = (tile_area(stage).2 * 0.3).clamp(132., 220.);
    (w, w * 10. / 16.)
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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeviceKind {
    Input,
    Output,
    Camera,
}

pub struct DeviceMenu {
    kind: DeviceKind,
    at: Point<Pixels>,
    // Opened from the call controls, so it takes the stage's dark tokens.
    on_stage: bool,
    inputs: Vec<voice::Device>,
    outputs: Vec<voice::Device>,
    cameras: Vec<voice::Device>,
}

// Where the connection stands, as the voice panel words and colours it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Link {
    Connecting,
    Connected,
    Poor,
    Lost,
}

impl Link {
    fn of(stage: Stage, stats: Option<&voice::Stats>) -> Self {
        match stage {
            Stage::Connected if stats.is_some_and(|s| s.avg_ping_ms > PING_BAD || loss(s) >= LOSS_POOR_PCT) => Link::Poor,
            Stage::Connected => Link::Connected,
            Stage::NoRoute | Stage::Disconnected => Link::Lost,
            _ => Link::Connecting,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Link::Connecting => "Connecting…",
            Link::Connected => "Voice connected",
            Link::Poor => "Poor connection",
            Link::Lost => "Reconnecting…",
        }
    }

    // .voicebar's --tone: muted while it connects, green when up, amber when something needs you.
    fn tone(self, cx: &App) -> Hsla {
        match self {
            Link::Connecting => palette::muted(cx),
            Link::Connected => palette::positive_ink(cx),
            Link::Poor | Link::Lost => palette::warning_ink(cx),
        }
    }
}

fn loss(stats: &voice::Stats) -> f32 {
    stats.inbound_loss_pct.max(stats.outbound_loss_pct)
}

// A ping's band in the connection graph.
fn ping_color(ping: u32, cx: &App) -> Hsla {
    match ping {
        p if p < PING_FAIR => palette::status_in(discord::Status::Online, cx),
        p if p <= PING_BAD => palette::status_in(discord::Status::Idle, cx),
        _ => palette::status_in(discord::Status::Dnd, cx),
    }
}

fn quality_word(ping: u32) -> &'static str {
    match ping {
        p if p < PING_FAIR => "Good",
        p if p <= PING_BAD => "Fair",
        _ => "Poor",
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
                let url = match &result {
                    Ok(body) => body["url"].as_str().map(str::to_owned),
                    Err(_) => None,
                };
                let Some(url) = url else {
                    // Often just too early: the streamer uploads its first preview a few seconds
                    // after going live. Ask again soon instead of waiting for the next voice event.
                    cx.background_executor().timer(PREVIEW_RETRY).await;
                    this.update(cx, |this, cx| {
                        this.expire_stream_preview(&user_id);
                        this.refresh_stream_previews(cx);
                    })
                    .ok();
                    return;
                };
                if std::env::var_os("RUSTCORD_STREAM_DEBUG").is_some() {
                    eprintln!("stream preview fetched for {key}");
                }
                this.update(cx, |this, cx| {
                    this.stream_previews.insert(user_id, (url, std::time::Instant::now()));
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
    }

    // Lets the next refresh fetch this streamer's preview again, keeping the picture shown so far.
    pub(crate) fn expire_stream_preview(&mut self, user_id: &str) {
        if let Some((_, at)) = self.stream_previews.get_mut(user_id)
            && let Some(old) = std::time::Instant::now().checked_sub(PREVIEW_AGE)
        {
            *at = old;
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
                        if let Err(e) = &result {
                            eprintln!("stream preview upload {key} failed: {e}");
                        } else {
                            if std::env::var_os("RUSTCORD_STREAM_DEBUG").is_some() {
                                eprintln!("stream preview uploaded for {key}");
                            }
                            // Show our own new preview right away rather than at the next refresh.
                            let _ = this.update(cx, |this, cx| {
                                if let Some(me) = this.me.as_ref().map(|m| m.id.clone()) {
                                    this.expire_stream_preview(&me);
                                }
                                this.refresh_stream_previews(cx);
                            });
                        }
                        result.is_ok()
                    }
                    None => {
                        eprintln!("stream preview: couldn't grab a still of {source:?}");
                        false
                    }
                };
                cx.background_executor().timer(Duration::from_secs(if sent { 300 } else { 60 })).await;
            }
        })
        .detach();
    }

    pub(crate) fn stream_preview(&self, user_id: &str) -> Option<String> {
        self.stream_previews.get(user_id).map(|(url, _)| url.clone()).filter(|url| !url.is_empty())
    }

    // In-call warnings: a mic that sends nothing, and Advanced suppression that had to fall back to
    // RNNoise because this computer can't keep up with it.
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
        match (&self.call, &self.voice_error) {
            (Some(_), _) => VOICE_HEIGHT,
            (None, Some(_)) => ERROR_HEIGHT,
            (None, None) => 0.,
        }
    }

    pub(crate) fn dismiss_call_menus(&mut self) -> bool {
        std::mem::take(&mut self.connection_info) | self.device_menu.take().is_some()
    }

    // .voicebar: the connection's state and where you are (or why it needs you), the one button
    // that leaves, then camera, share and activities.
    pub(crate) fn voice_panel(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let Some(call) = &self.call else { return self.voice_error_panel(cx) };
        let link = Link::of(call.stage, call.stats.as_ref());
        let tone = link.tone(cx);
        let connected = call.stage == Stage::Connected;
        let ping = call.stats.as_ref().map(|s| s.ping_ms);
        let place = match call.guild_id.as_ref().and_then(|id| self.guilds.iter().find(|g| &g.id == id)) {
            Some(guild) => format!("{} · {}", call.channel.name, guild.name),
            None => call.channel.name.clone(),
        };
        let channel = call.channel.clone();
        let second: AnyElement = match (link, call.stats.as_ref(), self.voice_warning()) {
            (Link::Poor, Some(stats), _) => why_line(format!("{} ms ping, {:.0}% packet loss", stats.avg_ping_ms, loss(stats)), tone).into_any_element(),
            (Link::Lost, _, _) => why_line("Lost the connection, retrying", tone).into_any_element(),
            (_, _, Some(warning)) => why_line(warning, palette::warning_ink(cx))
                .id("voice-warning")
                .cursor_pointer()
                .hover(|s| s.underline())
                .on_click(cx.listener(|this, _, window, cx| {
                    cx.stop_propagation();
                    this.open_settings(Page::VoiceVideo, window, cx);
                }))
                .into_any_element(),
            _ => div()
                .id("voice-place")
                .h(px(18.))
                .truncate()
                .text_size(px(12.))
                .text_color(palette::faint(cx))
                .hover(|s| s.underline())
                .child(place)
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.show_call(channel.clone(), window, cx);
                }))
                .into_any_element(),
        };
        let bars: AnyElement = match link {
            Link::Connecting | Link::Lost => ui::bars("voice-bars", tone, 12.),
            Link::Connected => still_bars(tone, 12., false).into_any_element(),
            Link::Poor => still_bars(tone, 12., true).into_any_element(),
        };
        let tip: SharedString = match (link, ping) {
            (Link::Connected | Link::Poor, Some(ms)) => format!("{ms} ms ping, {} connection. Click for details", quality_word(ms).to_lowercase()).into(),
            _ => "Click for connection details".into(),
        };
        let hover = palette::hover(cx);
        let status = div()
            .id("voice-status")
            .flex_1()
            .min_w_0()
            .mx(px(-6.))
            .my(px(-4.))
            .px(px(6.))
            .py(px(4.))
            .rounded(px(RADIUS_SM))
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .when(self.connection_info, |d| d.bg(hover))
            .child(div().h(px(20.)).flex().items_center().gap(px(6.)).text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).text_color(tone).child(bars).child(div().truncate().child(link.label())))
            .child(second)
            .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
            .on_click(cx.listener(|this, _, _, cx| {
                this.connection_info = !this.connection_info;
                cx.notify();
            }));
        let leave_tip = if link == Link::Connecting { "Cancel joining" } else { "Disconnect" };
        let top = div().flex().items_center().gap(px(8.)).child(status).child(ui::icon_button("hang-up", Glyph::CallEnd, leave_tip, cx).on_click(cx.listener(|this, _, _, cx| this.leave_voice(cx))));
        let wait = if link == Link::Lost { "Available once you're reconnected" } else { "Available once you're connected" };
        let ready = |label: &'static str| if connected { label } else { wait };
        let sharing = call.sharing.is_some();
        let camera = panel_action("panel-camera", Glyph::Video, ready(if call.camera_on { "Turn off camera" } else { "Turn on camera" }), connected, cx)
            .when(call.camera_on, |b| b.bg(palette::strong(cx)).text_color(palette::surface(palette::Level::Page, cx)))
            .when(connected, |b| b.on_click(cx.listener(|this, _, _, cx| this.toggle_camera(cx))));
        let share = match sharing {
            true => panel_action("panel-share", Glyph::ShareScreen, "Stream options", true, cx)
                .bg(palette::accent())
                .text_color(gpui_kit::white())
                .hover(|s| s.bg(palette::accent_hover()))
                .on_mouse_down(MouseButton::Left, cx.listener(|this, e: &MouseDownEvent, _, cx| this.open_stream_menu(e.position, cx))),
            false => panel_action("panel-share", Glyph::ShareScreen, ready("Share your screen"), connected, cx).when(connected, |b| b.on_click(cx.listener(|this, _, _, cx| this.toggle_screen_share(cx)))),
        };
        let activities = panel_action("panel-activities", Glyph::Activities, ready("Activities and soundboard"), connected, cx)
            .when(connected, |b| b.on_click(cx.listener(|this, _, _, cx| this.show_toast("Activities and the soundboard aren't in Wave yet", cx))));
        let voice = div()
            .h(px(VOICE_HEIGHT))
            .flex_none()
            .p(px(PANEL_PAD))
            .flex()
            .flex_col()
            .gap(px(8.))
            .rounded_t(px(RADIUS_MD))
            .bg(palette::surface(palette::Level::Rail, cx))
            .border_b_1()
            .border_color(palette::line(cx))
            .child(top)
            .child(div().flex().gap(px(6.)).child(camera).child(share).child(activities));
        Some(voice.into_any_element())
    }

    // The voicebar's danger state once a call is gone: what happened, and a way to dismiss it.
    fn voice_error_panel(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let error = self.voice_error.clone()?;
        let tone = palette::danger_ink(cx);
        Some(
            div()
                .h(px(ERROR_HEIGHT))
                .p(px(PANEL_PAD))
                .flex()
                .items_center()
                .gap(px(8.))
                .rounded_t(px(RADIUS_MD))
                .bg(palette::surface(palette::Level::Rail, cx))
                .border_b_1()
                .border_color(palette::line(cx))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(div().h(px(20.)).flex().items_center().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).text_color(tone).child("Voice disconnected"))
                        .child(why_line(error, tone)),
                )
                .child(ui::icon_button("dismiss-voice", Glyph::X, "Dismiss", cx).on_click(cx.listener(|this, _, _, cx| {
                    this.voice_error = None;
                    cx.notify();
                })))
                .into_any_element(),
        )
    }

    // The stream menu, opened from the share button while we're live.
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

    // .call: the stage, dark in both themes. The header, the tiles (or the watched streams over a
    // filmstrip) and the control bar; over a stream the chrome steps aside while the pointer rests.
    pub(crate) fn render_call(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let call = self.call.as_ref()?;
        let tiles = self.ordered_tiles();
        let viewport = window.viewport_size();
        let chat = if self.call_chat && self.channel.as_ref().is_some_and(|c| c.id == call.channel.id) { CHAT_WIDTH } else { 0. };
        let stage_size = match self.call_fullscreen {
            true => (f32::from(viewport.width) - chat, f32::from(viewport.height)),
            false => (f32::from(viewport.width) - RAIL_WIDTH - self.channels_width - chat, f32::from(viewport.height) - crate::layout::TITLE_HEIGHT - 1.),
        };
        let (_, _, width, height) = tile_area(stage_size);
        let watching = !call.watching.is_empty();
        let ended = call.ended.clone().filter(|_| !watching);
        let body: AnyElement = match (watching, ended) {
            (true, _) => self.stream_stage(&tiles, width, height, cx),
            (false, Some(streamer)) => self.ended_stage(&streamer, width, height, cx),
            (false, None) => self.tile_grid(&tiles, width, height, cx),
        };
        // .call.autohide: 260ms out, drifting 6px toward its edge; any move brings it back.
        let rest = watching && self.call_idle();
        let shown = gpui_kit::base::transition("call-chrome", if rest { 0. } else { 1. }, gpui_kit::base::Transition::new(ui::DUR_PANEL), window, cx);
        let drift = 6. * (1. - shown);
        Some(
            div()
                .id("call-view")
                .relative()
                .size_full()
                .p(px(STAGE_PAD))
                .flex()
                .flex_col()
                .gap(px(STAGE_GAP))
                .bg(stage::bg())
                .text_color(stage::text())
                .on_mouse_move(cx.listener(|this, e: &gpui_kit::MouseMoveEvent, _, cx| {
                    let was_idle = this.call_idle();
                    this.call_pointer_at = Instant::now();
                    if this.drag_pip(e.position) || was_idle {
                        cx.notify();
                    }
                }))
                .on_mouse_up(MouseButton::Left, cx.listener(move |this, e: &gpui_kit::MouseUpEvent, _, cx| this.drop_pip(e.position, stage_size, cx)))
                .on_mouse_up_out(MouseButton::Left, cx.listener(move |this, e: &gpui_kit::MouseUpEvent, _, cx| this.drop_pip(e.position, stage_size, cx)))
                .child(div().relative().top(px(-drift)).opacity(shown).child(self.call_header(cx)))
                .child(div().flex_1().min_h_0().child(body))
                .child(div().relative().top(px(drift)).opacity(shown).child(self.call_controls(cx)))
                .children(self.self_view(stage_size, window, cx))
                .into_any_element(),
        )
    }

    // .tiles: the size that makes the biggest 16:9 tiles fit, 10px apart (8px three across and up).
    fn tile_grid(&self, tiles: &[Tile], width: f32, height: f32, cx: &mut Context<Self>) -> AnyElement {
        let n = tiles.len().max(1);
        let gap = |cols: usize| if cols >= 3 { TILE_GAP_DENSE } else { TILE_GAP };
        let size = |cols: usize| {
            let rows = n.div_ceil(cols) as f32;
            let w = (width - gap(cols) * (cols as f32 - 1.)) / cols as f32;
            w.min((height - gap(cols) * (rows - 1.)) / rows * 16. / 9.)
        };
        let cols = (1..=n).max_by(|a, b| size(*a).total_cmp(&size(*b))).unwrap_or(1);
        let tile_w = size(cols).max(124.);
        let rendered: Vec<AnyElement> = tiles.iter().map(|t| self.tile(t, tile_w, cx)).collect();
        div().size_full().flex().flex_wrap().items_center().justify_center().content_center().gap(px(gap(cols))).children(rendered).into_any_element()
    }

    // Our own stream in a corner of the tile area, straight from the capture: it lifts and follows
    // the pointer while dragged, with the four corners showing as wells, and springs into the one
    // it's thrown toward. Clicking it watches it full size. Closed, a small control stays in its
    // corner to bring it back.
    fn self_view(&self, stage: (f32, f32), window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(call) = self.call.as_ref().filter(|c| c.sharing.is_some()) else { return Vec::new() };
        let watching_self = self.me.as_ref().is_some_and(|m| call.watched(&m.id).is_some());
        if watching_self {
            return Vec::new();
        }
        if !self.pip_shown {
            return vec![self.pip_restore(stage, cx)];
        }
        let (w, h) = pip_size(stage);
        let target = self.pip_corner.spot(stage);
        let (x, y) = match (&self.pip_drag, self.pip_snap) {
            (Some(drag), _) => drag.pos,
            (None, Some((from, at))) if at.elapsed() < PIP_SNAP => {
                window.request_animation_frame();
                let t = ease_spring((at.elapsed().as_secs_f32() / PIP_SNAP.as_secs_f32()).min(1.));
                (from.0 + (target.0 - from.0) * t, from.1 + (target.1 - from.1) * t)
            }
            _ => target,
        };
        let dragging = self.pip_drag.is_some();
        let near = Corner::thrown((x + w / 2., y + h / 2.), (0., 0.), stage);
        let wells = CORNERS.into_iter().filter(|_| dragging).map(|corner| {
            let (wx, wy) = corner.spot(stage);
            let lit = corner == near;
            div()
                .absolute()
                .left(px(wx))
                .top(px(wy))
                .w(px(w))
                .h(px(h))
                .rounded(px(RADIUS_LG))
                .bg(if lit { palette::accent().opacity(0.18) } else { stage::wash(0.05) })
                .border(px(if lit { 2. } else { 1.5 }))
                .border_color(if lit { palette::accent() } else { stage::wash(0.24) })
                .into_any_element()
        });
        let frame: AnyElement = match &call.self_view {
            Some((frame, _)) => ui::media(ImageSource::Render(frame.clone()), ui::all(RADIUS_LG)).size_full().object_fit(gpui_kit::ObjectFit::Cover).into_any_element(),
            None => div().size_full().rounded(px(RADIUS_LG)).bg(stage::page()).into_any_element(),
        };
        // .pip: 0 10px 28px black 55% under a 1px white edge; deeper while lifted.
        let lift = if dragging { (18., 40., 0.6, 0.18) } else { (10., 28., 0.55, 0.12) };
        let shadow = vec![
            BoxShadow { color: gpui_kit::black().opacity(lift.2), offset: point(px(0.), px(lift.0)), blur_radius: px(lift.1), spread_radius: px(0.), inset: false },
            BoxShadow { color: stage::wash(lift.3), offset: point(px(0.), px(0.)), blur_radius: px(0.), spread_radius: px(1.), inset: false },
        ];
        // PATCH(transform): scale the self-view to 1.04 while dragged (.pip.is-dragging), easing back on release.
        let pip = div()
            .id("self-view")
            .group("self-view")
            .absolute()
            .left(px(x))
            .top(px(y))
            .w(px(w))
            .h(px(h))
            .rounded(px(RADIUS_LG))
            .shadow(shadow)
            .cursor(if dragging { gpui_kit::CursorStyle::ClosedHand } else { gpui_kit::CursorStyle::OpenHand })
            .child(frame)
            .child(scrim_bottom(RADIUS_LG))
            .child(div().absolute().left(px(10.)).right(px(10.)).bottom(px(8.)).flex().items_center().gap(px(6.)).text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(gpui_kit::white()).child(div().truncate().child("You")))
            .child(
                div()
                    .id("self-view-close")
                    .absolute()
                    .top(px(4.))
                    .right(px(4.))
                    .size(px(24.))
                    .rounded(px(RADIUS_XS))
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(gpui_kit::black().opacity(0.5))
                    .text_color(gpui_kit::white())
                    .opacity(0.)
                    .when(!dragging, |d| d.group_hover("self-view", |s| s.opacity(1.)))
                    .hover(|s| s.bg(gpui_kit::black().opacity(0.7)))
                    .child(Icon::new(Glyph::X).size(px(14.)))
                    .tooltip(|window, cx| Tooltip::new("Hide self-view. Others still see your stream").build(window, cx))
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
            .into_any_element();
        wells.chain([pip]).collect()
    }

    // .pip-restore: "Show self-view", left in the corner the self-view closed from.
    fn pip_restore(&self, stage: (f32, f32), cx: &mut Context<Self>) -> AnyElement {
        let (x, y, w, h) = tile_area(stage);
        let (right_gap, bottom_gap) = (stage.0 - x - w + PIP_INSET, stage.1 - y - h + PIP_INSET);
        let ring = vec![
            BoxShadow { color: stage::wash(0.16), offset: point(px(0.), px(0.)), blur_radius: px(0.), spread_radius: px(1.), inset: false },
            BoxShadow { color: gpui_kit::black().opacity(0.35), offset: point(px(0.), px(6.)), blur_radius: px(16.), spread_radius: px(0.), inset: false },
        ];
        let button = div()
            .id("self-view-restore")
            .absolute()
            .h(px(32.))
            .pl(px(10.))
            .pr(px(12.))
            .rounded(px(RADIUS_MD))
            .flex()
            .items_center()
            .gap(px(6.))
            .cursor_pointer()
            .bg(stage::bg().opacity(0.78))
            .shadow(ring)
            .text_color(gpui_kit::white())
            .text_size(px(12.))
            .font_weight(FontWeight::SEMIBOLD)
            .whitespace_nowrap()
            .hover(|s| s.bg(stage::raised().opacity(0.9)))
            .child(Icon::new(Glyph::User).size(px(16.)))
            .child("Show self-view")
            .tooltip(|window, cx| Tooltip::new("Others still see your stream").build(window, cx))
            .on_click(cx.listener(|this, _, _, cx| {
                this.pip_shown = true;
                cx.notify();
            }));
        let placed = match self.pip_corner {
            Corner::TopLeft => button.left(px(x + PIP_INSET)).top(px(y + PIP_INSET)),
            Corner::TopRight => button.right(px(right_gap)).top(px(y + PIP_INSET)),
            Corner::BottomLeft => button.left(px(x + PIP_INSET)).bottom(px(bottom_gap)),
            Corner::BottomRight => button.right(px(right_gap)).bottom(px(bottom_gap)),
        };
        placed.with_animation("self-view-restore-in", Animation::new(ui::DUR_POP), |d, t| d.opacity(t)).into_any_element()
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

    // Lets go of the self-view: a click (barely moved) watches our stream full size; a drag springs
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
        let (w, h) = pip_size(stage);
        let center = (drag.pos.0 + w / 2., drag.pos.1 + h / 2.);
        self.pip_corner = Corner::thrown(center, velocity, stage);
        self.pip_snap = Some((drag.pos, Instant::now()));
        cx.notify();
    }

    // Full screen shows the call alone: no servers, channels or user panel.
    pub(crate) fn set_call_fullscreen(&mut self, on: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.call_fullscreen = on;
        if window.is_fullscreen() != on {
            window.toggle_fullscreen();
        }
        cx.notify();
    }

    // Still pointer for a while, and nothing open that belongs to the bar.
    fn call_idle(&self) -> bool {
        self.call_pointer_at.elapsed() > CONTROLS_IDLE && self.device_menu.is_none() && self.menu.is_none()
    }

    // .callhead: the room (or whose screen), how long the call has run, who's in it, then the view
    // controls. While watching one stream: its quality, how smoothly it plays and a way to stop.
    fn call_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let call = self.call.as_ref().expect("in a call");
        let people = self.participants();
        let watching = call.focused();
        let several = call.watching.len() > 1;
        let title = match (watching, several) {
            (None, _) => call.channel.name.clone(),
            (Some(_), true) => format!("Watching {} streams", call.watching.len()),
            (Some(w), false) => format!("{}'s screen", self.user_name(&w.streamer_id)),
        };
        let glyph = if watching.is_some() { Glyph::Screen } else { Glyph::VolumeHigh };
        let timer = div()
            .h(px(22.))
            .px(px(8.))
            .flex_none()
            .rounded(px(RADIUS_XS))
            .flex()
            .items_center()
            .gap(px(6.))
            .bg(stage::wash(0.08))
            .text_color(stage::muted())
            .text_size(px(12.))
            .font_weight(FontWeight::SEMIBOLD)
            .font_features(tabular())
            .child(div().size(px(6.)).rounded_full().bg(stage::online()))
            .child(clock(call.joined.elapsed().as_secs()));
        let faces: Vec<String> = people.iter().take(3).map(|p| p.user.as_ref().map(|u| u.avatar.clone()).unwrap_or_else(|| self.user_avatar(&p.user_id))).collect();
        let count = people.len();
        let who = div()
            .id("call-people")
            .h(px(32.))
            .pl(px(4.))
            .pr(px(8.))
            .flex_none()
            .rounded(px(RADIUS_SM))
            .flex()
            .items_center()
            .gap(px(6.))
            .text_color(stage::muted())
            .text_size(px(12.))
            .font_weight(FontWeight::SEMIBOLD)
            .child(face_stack(faces, 22., stage::bg()))
            .child(count.to_string())
            .tooltip(move |window, cx| Tooltip::new(format!("{count} in {}", if count == 1 { "the call alone" } else { "the call" })).build(window, cx));
        let health = watching.filter(|w| !several && !w.arrivals.is_empty()).map(|w| {
            let (health, fps) = stream_health(&w.arrivals, Instant::now());
            let (label, color) = match health {
                Health::Smooth => ("Smooth", stage::online()),
                Health::Choppy => ("Choppy", stage::idle()),
                Health::Stalled => ("Stalled", stage::dnd()),
            };
            // GAP: the frame rate isn't reported by the voice engine, so only the resolution shows.
            let quality = w.frame.as_ref().map(|f| format!("{}p · ", f.size(0).height.0)).unwrap_or_default();
            div()
                .id("stream-health")
                .flex_none()
                .flex()
                .items_center()
                .gap(px(6.))
                .text_size(px(12.))
                .text_color(stage::muted())
                .child(div().size(px(6.)).rounded_full().bg(color))
                .child(format!("{quality}{label}"))
                .tooltip(move |window, cx| Tooltip::new(format!("{label}: {fps:.0} frames per second")).build(window, cx))
        });
        let layout = call.stream_layout;
        let layout_button = several.then(|| {
            let (glyph, tip, next) = match layout {
                StreamLayout::Focus => (Glyph::Screen, "Layout: focus. Switch to side by side", StreamLayout::Split),
                StreamLayout::Split => (Glyph::Users, "Layout: side by side. Switch to focus", StreamLayout::Focus),
            };
            stage_icon_button("call-layout", glyph, tip, false).on_click(cx.listener(move |this, _, _, cx| {
                if let Some(call) = &mut this.call {
                    call.stream_layout = next;
                }
                cx.notify();
            }))
        });
        let stop = watching.map(|_| stage_button("stop-watching", if several { "Stop all" } else { "Stop watching" }).on_click(cx.listener(|this, _, _, cx| this.stop_watching(cx))));
        let fullscreen = self.call_fullscreen;
        let chat_open = self.call_chat;
        div()
            .h(px(HEAD_HEIGHT))
            .flex()
            .items_center()
            .gap(px(8.))
            .text_color(stage::strong())
            .child(Icon::new(glyph).size(px(20.)).text_color(stage::muted()))
            .child(div().min_w_0().truncate().text_size(px(16.)).font_weight(FontWeight::SEMIBOLD).child(title))
            .child(timer)
            .children(health)
            .child(div().flex_1())
            .children(stop)
            .child(who)
            .child(div().w(px(1.)).h(px(20.)).mx(px(2.)).flex_none().bg(stage::wash(0.14)))
            .children(layout_button)
            .child(
                stage_icon_button("call-fullscreen", if fullscreen { Glyph::FullscreenExit } else { Glyph::Fullscreen }, if fullscreen { "Exit full screen" } else { "Full screen" }, false)
                    .on_click(cx.listener(move |this, _, window, cx| this.set_call_fullscreen(!fullscreen, window, cx))),
            )
            .child(
                stage_icon_button("call-chat", Glyph::ChatRound, if chat_open { "Chat: hide the channel's messages" } else { "Chat: show the channel's messages beside the call" }, chat_open)
                    .on_click(cx.listener(|this, _, window, cx| this.toggle_call_chat(window, cx))),
            )
            .into_any_element()
    }

    // .focus: the streams we watch, laid out by `stream_layout`, over a filmstrip of everyone else
    // (folded away from More). Unwatched streams sit in the filmstrip with Watch too.
    fn stream_stage(&self, tiles: &[Tile], width: f32, height: f32, cx: &mut Context<Self>) -> AnyElement {
        let call = self.call.as_ref().expect("in a call");
        let strip: Vec<AnyElement> = tiles
            .iter()
            .filter(|t| !matches!(t, Tile::Stream(s) if call.watched(&s.user_id).is_some()))
            .take(STRIP_LIMIT)
            .map(|t| self.tile(t, STRIP_TILE, cx))
            .collect();
        let strip_shown = !self.call_strip_hidden && !strip.is_empty();
        let main_h = (height - if strip_shown { STRIP_TILE * 9. / 16. + STRIP_GAP } else { 0. }).max(120.);
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
                    .when(!others.is_empty(), |d| d.child(div().absolute().right(px(12.)).bottom(px(12.)).flex().flex_col().gap(px(STRIP_GAP)).children(others)))
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
            .flex()
            .flex_col()
            .gap(px(STRIP_GAP))
            .child(div().flex_1().min_h_0().child(main))
            .when(strip_shown, |d| d.child(div().flex_none().flex().justify_center().gap(px(STRIP_GAP)).children(strip)))
            .into_any_element()
    }

    // One watched stream fitted into `max_w` x `max_h`, as a media tile: LIVE and its viewers on top,
    // the name over the scrim. Hovering adds its volume and buttons to focus or stop it; a small one
    // (Focus layout) swaps in on click.
    fn stream_video(&self, watch: &crate::Watching, max_w: f32, max_h: f32, small: bool, cx: &mut Context<Self>) -> AnyElement {
        let call = self.call.as_ref().expect("in a call");
        let (fw, fh) = watch.frame.as_ref().map_or((16., 9.), |f| (f.size(0).width.0 as f32, f.size(0).height.0 as f32));
        let scale = (max_w / fw).min(max_h / fh);
        let (w, h) = (fw * scale, fh * scale);
        let id = watch.streamer_id.clone();
        let name = self.user_name(&id);
        let several = call.watching.len() > 1;
        let body: AnyElement = match &watch.frame {
            Some(frame) => ui::media(ImageSource::Render(frame.clone()), ui::all(RADIUS_LG)).size_full().into_any_element(),
            None => stream_fill(RADIUS_LG).size_full().flex().items_center().justify_center().text_size(px(14.)).text_color(stage::muted()).child(format!("Joining {name}'s stream…")).into_any_element(),
        };
        let group = SharedString::from(format!("stream-{id}"));
        let label = match self.activity_name(&id) {
            Some(app) => format!("{name} · {app}"),
            None => name.clone(),
        };
        let (focus_id, stop_id, volume_id) = (id.clone(), id.clone(), id.clone());
        let controls = div()
            .absolute()
            .right(px(8.))
            .bottom(px(6.))
            .flex()
            .items_center()
            .gap(px(6.))
            .opacity(0.)
            .group_hover(group.clone(), |s| s.opacity(1.))
            .child(
                ui::round_button(SharedString::from(format!("volume-{id}")), Glyph::VolumeHigh, "Stream volume", 28.)
                    .on_mouse_down(MouseButton::Left, cx.listener(move |this, e: &MouseDownEvent, _, cx| this.open_stream_volume(volume_id.clone(), e.position, cx))),
            )
            .when(several && call.stream_layout == StreamLayout::Split, |d| d.child(ui::button(SharedString::from(format!("focus-{id}")), ui::Tone::OnMedia, cx).small().child("Focus").on_click(cx.listener(move |this, _, _, cx| this.focus_stream(&focus_id, cx)))))
            .child(ui::button(SharedString::from(format!("unwatch-{id}")), ui::Tone::OnMedia, cx).small().child("Stop watching").on_click(cx.listener(move |this, _, _, cx| this.unwatch_stream(&stop_id, cx))));
        let swap_id = id.clone();
        let viewers = self.viewer_count(&id);
        div()
            .id(SharedString::from(format!("stream-video-{id}-{small}")))
            .group(group)
            .relative()
            .w(px(w))
            .h(px(h))
            .flex_none()
            .rounded(px(RADIUS_LG))
            .bg(gpui_kit::black())
            .child(body)
            .child(scrim_top(RADIUS_LG))
            .child(scrim_bottom(RADIUS_LG))
            .child(div().absolute().left(px(8.)).top(px(8.)).child(ui::live_badge()))
            .when(viewers > 0 && !small, |d| d.child(viewer_chip(viewers)))
            .child(tile_name(div().child(div().min_w_0().truncate().child(label))))
            .when(!small, |d| d.child(controls))
            .child(tile_ring(false, false))
            .when(small, |d| d.cursor_pointer().on_click(cx.listener(move |this, _, _, cx| this.focus_stream(&swap_id, cx))))
            .with_animation(SharedString::from(format!("stream-in-{id}-{small}")), Animation::new(ui::DUR_POP), |d, t| d.opacity(t.min(1.)))
            .into_any_element()
    }

    // Everyone watching a streamer, other than the streamer.
    fn viewer_count(&self, streamer_id: &str) -> usize {
        let suffix = format!(":{streamer_id}");
        self.stream_viewers.iter().find(|(key, _)| key.ends_with(&suffix)).map_or(0, |(_, v)| v.iter().filter(|id| *id != streamer_id).count())
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
                vec![crate::menu::slider("Stream volume", format!("stream-volume-{streamer_id}"), volume / 2., move |this, f, cx| this.set_stream_volume(&id, f * 2., cx))]
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

    // .tile.ended in the stream's place: that it ended, who stopped, and the way back to the call.
    fn ended_stage(&self, streamer: &str, width: f32, height: f32, cx: &mut Context<Self>) -> AnyElement {
        let mine = self.me.as_ref().is_some_and(|m| m.id == streamer);
        let name = if mine { "You".to_owned() } else { self.user_name(streamer) };
        let line = match self.activity_name(streamer) {
            Some(app) => format!("{name} stopped sharing {app}"),
            None => format!("{name} stopped sharing"),
        };
        let w = width.min(height * 16. / 9.).min(720.);
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .w(px(w))
                    .h(px(w * 9. / 16.))
                    .rounded(px(RADIUS_LG))
                    .bg(stage::ended())
                    .relative()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(4.))
                    .p(px(12.))
                    .child(div().text_size(px(16.)).font_weight(FontWeight::SEMIBOLD).text_color(stage::strong()).child("Stream ended"))
                    .child(div().text_size(px(12.)).text_color(stage::muted()).child(line))
                    .child(div().mt(px(6.)).child(stage_button("ended-back", "Back to call").on_click(cx.listener(|this, _, _, cx| {
                        if let Some(call) = &mut this.call {
                            call.ended = None;
                        }
                        cx.notify();
                    }))))
                    .child(tile_ring(false, false)),
            )
            .with_animation("ended-in", Animation::new(ui::DUR_PANEL), |d, t| d.opacity(t.min(1.)))
            .into_any_element()
    }

    // .tile: a person (camera, or their avatar on their own tint) or someone's Go Live (its preview
    // under LIVE and the viewer count). Speaking draws the accent ring above everything; the label
    // carries one mark for what's off.
    fn tile(&self, tile: &Tile, w: f32, cx: &mut Context<Self>) -> AnyElement {
        let call = self.call.as_ref().expect("in a call");
        let h = w * 9. / 16.;
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
        let local_muted = !me && self.prefs.local_mutes.contains(&state.user_id);
        let is_stream = matches!(tile, Tile::Stream(_));
        let talking = !is_stream && self.speaking(&state.user_id);
        let camera = (!is_stream).then(|| call.cameras.get(&state.user_id)).flatten();
        let media = is_stream || camera.is_some();
        let face: AnyElement = match (is_stream, camera) {
            (true, _) => stream_fill(RADIUS_LG)
                .size_full()
                .children(self.stream_preview(&state.user_id).map(|url| ui::media(url, ui::all(RADIUS_LG)).absolute().inset_0().size_full().object_fit(gpui_kit::ObjectFit::Cover)))
                .into_any_element(),
            (false, Some(frame)) => ui::media(ImageSource::Render(frame.clone()), ui::all(RADIUS_LG)).size_full().object_fit(gpui_kit::ObjectFit::Cover).into_any_element(),
            (false, None) => div()
                .size_full()
                .rounded(px(RADIUS_LG))
                .bg(self.tile_fill(&state.user_id))
                .flex()
                .items_center()
                .justify_center()
                .child(ui::avatar(user.avatar.clone(), if w < SMALL_TILE { 40. } else { 64. }).when(local_muted, |a| a.opacity(0.5)))
                .into_any_element(),
        };
        let watching = call.watched(&state.user_id).is_some();
        let watchable = is_stream && !watching && (!me || call.sharing.is_some());
        let label = match (is_stream, self.activity_name(&state.user_id)) {
            (true, Some(app)) => format!("{} · {app}", user.name),
            _ => user.name.clone(),
        };
        let name = tile_name(
            div()
                .when(talking, |d| d.child(still_bars(gpui_kit::white(), 10., false)))
                .when(!is_stream && state.server_muted, |d| d.child(vmark(Glyph::MicOff, None, MarkKind::Server)))
                .when(!is_stream && !state.server_muted && muted && !deafened, |d| d.child(Icon::new(Glyph::MicOff).size(px(14.)).text_color(stage::off_ink())))
                .when(!is_stream && deafened, |d| d.child(Icon::new(Glyph::HeadphonesOff).size(px(14.)).text_color(stage::off_ink())))
                .child(div().min_w_0().truncate().child(label))
                .when(local_muted, |d| d.child(vmark(Glyph::VolumeOff, Some("Muted for you"), MarkKind::Local))),
        );
        let streamer = state.user_id.clone();
        let menu_user = state.user_id.clone();
        let group = SharedString::from(format!("tile-{}-{is_stream}", state.user_id));
        let viewers = if is_stream { self.viewer_count(&state.user_id) } else { 0 };
        let watch_label = if call.watching.is_empty() { "Watch stream" } else { "Watch too" };
        div()
            .id(group.clone())
            .group(group.clone())
            .relative()
            .w(px(w))
            .h(px(h))
            .flex_none()
            .rounded(px(RADIUS_LG))
            .child(face)
            .when(media, |d| d.child(scrim_top(RADIUS_LG)).child(scrim_bottom(RADIUS_LG)))
            .when(is_stream, |d| d.child(div().absolute().left(px(8.)).top(px(8.)).child(ui::live_badge())))
            .when(viewers > 0, |d| d.child(viewer_chip(viewers)))
            .child(name)
            .when(watchable, |d| {
                let streamer = streamer.clone();
                d.cursor_pointer()
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .rounded(px(RADIUS_LG))
                            .bg(gpui_kit::black().opacity(0.45))
                            .flex()
                            .items_center()
                            .justify_center()
                            .opacity(0.)
                            .group_hover(group.clone(), |s| s.opacity(1.))
                            .child(watch_button(watch_label)),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.watch_stream(streamer.clone(), cx)))
            })
            .child(tile_ring(talking, local_muted).when(watchable, |r| r.group_hover(group.clone(), |s| s.border_color(stage::wash(0.2)))))
            .on_mouse_down(MouseButton::Right, cx.listener(move |this, e: &MouseDownEvent, _, cx| this.voice_user_menu(&menu_user, e.position, cx)))
            .into_any_element()
    }

    // A person's tile tint (oklch 0.24 0.02, their hue): their avatar's hue once fetched, else one
    // from their id so the grid isn't one grey.
    fn tile_fill(&self, user_id: &str) -> Hsla {
        let hue = match self.avatar_colors.get(user_id).copied().flatten() {
            Some(color) => oklch_hue(color),
            None => user_id.bytes().fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32)) as f32 % 360.,
        };
        oklch(0.24, 0.02, hue)
    }

    // .controls: mic and camera as split buttons with their device menus, share, activities, more,
    // then the one red fill that leaves.
    fn call_controls(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let call = self.call.as_ref().expect("in a call");
        let muted = self.muted || self.deafened;
        let connected = call.stage == Stage::Connected;
        let sharing = call.sharing.is_some();
        let menu_open = |kind: DeviceKind| self.device_menu.as_ref().is_some_and(|m| m.on_stage && m.kind == kind);
        let mic_state = if muted { Control::Off } else { Control::Rest };
        let camera_state = if call.camera_on { Control::On } else { Control::Rest };
        let mic = split_control(
            control("call-mic", if muted { Glyph::MicOff } else { Glyph::Mic }, if muted { "Unmute" } else { "Mute" }, mic_state, true).on_click(cx.listener(|this, _, _, cx| this.toggle_mute(cx))),
            control("call-mic-devices", Glyph::ChevronUp, "Microphone options", if menu_open(DeviceKind::Input) { Control::Open } else { mic_state }, true)
                .on_mouse_down(MouseButton::Left, cx.listener(|this, e: &MouseDownEvent, _, cx| this.open_stage_device_menu(DeviceKind::Input, e.position, cx))),
        );
        let camera = split_control(
            control("call-camera", if call.camera_on { Glyph::Video } else { Glyph::VideoOff }, if call.camera_on { "Turn off camera" } else { "Turn on camera" }, camera_state, connected)
                .when(connected, |b| b.on_click(cx.listener(|this, _, _, cx| this.toggle_camera(cx)))),
            control("call-camera-devices", Glyph::ChevronUp, "Camera options", if menu_open(DeviceKind::Camera) { Control::Open } else { camera_state }, true)
                .on_mouse_down(MouseButton::Left, cx.listener(|this, e: &MouseDownEvent, _, cx| this.open_stage_device_menu(DeviceKind::Camera, e.position, cx))),
        );
        let share = match sharing {
            true => control("call-share", Glyph::ShareScreen, "Stream options", Control::Active, true).on_mouse_down(MouseButton::Left, cx.listener(|this, e: &MouseDownEvent, _, cx| this.open_stream_menu(e.position, cx))),
            false => control("call-share", Glyph::ShareScreen, "Share your screen", Control::Rest, connected).when(connected, |b| b.on_click(cx.listener(|this, _, _, cx| this.toggle_screen_share(cx)))),
        };
        let activities = control("call-activities", Glyph::Activities, "Activities and soundboard", Control::Rest, connected)
            .when(connected, |b| b.on_click(cx.listener(|this, _, _, cx| this.show_toast("Activities and the soundboard aren't in Wave yet", cx))));
        let more = control("call-more", Glyph::More, "More options", Control::Rest, true).on_mouse_down(MouseButton::Left, cx.listener(|this, e: &MouseDownEvent, _, cx| this.open_call_more(e.position, cx)));
        let leave = control("call-hang-up", Glyph::CallEnd, "Leave call", Control::Leave, true).w(px(68.)).on_click(cx.listener(|this, _, _, cx| this.leave_voice(cx)));
        div()
            .h(px(CONTROLS_HEIGHT))
            .flex()
            .justify_center()
            .items_center()
            .gap(px(10.))
            .child(mic)
            .child(camera)
            .child(share)
            .child(activities)
            .child(more)
            .child(div().w(px(6.)))
            .child(leave)
    }

    // More options: what didn't earn a button of its own.
    fn open_call_more(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        cx.stop_propagation();
        self.open_live_menu(
            at,
            |this| {
                let Some(call) = &this.call else { return Vec::new() };
                let mut entries = Vec::new();
                if let Some(guild) = call.guild_id.clone() {
                    let channel = call.channel.id.clone();
                    entries.push(item("Invite to voice", move |this, window, cx| this.show_invite(guild.clone(), channel.clone(), window, cx)).icon(Glyph::GroupPlus).entry());
                }
                entries.push(item("Soundboard", |this, _, cx| this.show_toast("The soundboard isn't in Wave yet", cx)).icon(Glyph::Soundboard).entry());
                if !call.watching.is_empty() {
                    let hidden = this.call_strip_hidden;
                    entries.push(
                        item(if hidden { "Show everyone" } else { "Hide everyone" }, |this, _, cx| {
                            this.call_strip_hidden = !this.call_strip_hidden;
                            cx.notify();
                        })
                        .hint("The filmstrip under the stream")
                        .icon(Glyph::Users)
                        .entry(),
                    );
                }
                if call.sharing.is_some() {
                    entries.push(
                        item("Show self-view", |this, _, cx| {
                            this.pip_shown = !this.pip_shown;
                            cx.notify();
                        })
                        .checked(this.pip_shown)
                        .entry(),
                    );
                }
                entries
            },
            cx,
        );
    }

    pub(crate) fn open_device_menu(&mut self, kind: DeviceKind, at: Point<Pixels>, cx: &mut Context<Self>) {
        self.show_device_menu(kind, at, false, cx);
    }

    fn open_stage_device_menu(&mut self, kind: DeviceKind, at: Point<Pixels>, cx: &mut Context<Self>) {
        self.show_device_menu(kind, at, true, cx);
    }

    fn show_device_menu(&mut self, kind: DeviceKind, at: Point<Pixels>, on_stage: bool, cx: &mut Context<Self>) {
        cx.stop_propagation();
        let (inputs, outputs, cameras) = match kind {
            DeviceKind::Input => (voice::input_devices(), voice::output_devices(), Vec::new()),
            DeviceKind::Output => (Vec::new(), voice::output_devices(), Vec::new()),
            DeviceKind::Camera => (Vec::new(), Vec::new(), voice::cameras()),
        };
        self.device_menu = Some(DeviceMenu { kind, at, on_stage, inputs, outputs, cameras });
        cx.notify();
    }

    fn choose_device(&mut self, kind: DeviceKind, id: Option<String>, cx: &mut Context<Self>) {
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

    // .connpop: how the connection is doing, ping over the last few minutes, the numbers behind it,
    // and how the call is secured.
    fn connection_overlay(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.connection_info {
            return None;
        }
        let call = self.call.as_ref()?;
        let stats = call.stats.clone().unwrap_or_default();
        let measured = call.stats.is_some();
        let (strong, muted, faint) = (palette::strong(cx), palette::muted(cx), palette::faint(cx));
        let link = Link::of(call.stage, call.stats.as_ref());
        let verdict = match (link, measured) {
            (Link::Connected | Link::Poor, true) => quality_word(stats.avg_ping_ms),
            (Link::Lost, _) => "Reconnecting",
            _ => "Connecting",
        };
        let verdict_ink = match verdict {
            "Good" => palette::positive_ink(cx),
            "Connecting" => muted,
            _ => palette::warning_ink(cx),
        };
        let peak = stats.last_pings.iter().copied().max().unwrap_or(0).max(160) as f32;
        let graph = div()
            .h(px(56.))
            .p(px(6.))
            .rounded(px(RADIUS_SM))
            .bg(palette::surface(palette::Level::Page, cx))
            .flex()
            .items_end()
            .gap(px(2.))
            .children(stats.last_pings.iter().map(|&p| div().flex_1().min_w(px(2.)).h(relative((p as f32 / peak).clamp(0.06, 1.))).rounded_t(px(2.)).rounded_b(px(1.)).bg(ping_color(p, cx))));
        let swatch = |color: Hsla, text: &'static str| div().flex().items_center().gap(px(4.)).child(div().size(px(8.)).rounded(px(2.)).bg(color)).child(text);
        let legend = div()
            .flex()
            .flex_wrap()
            .gap(px(12.))
            .text_size(px(12.))
            .text_color(muted)
            .child(swatch(ping_color(0, cx), "Under 80 ms"))
            .child(swatch(ping_color(PING_FAIR, cx), "80–120"))
            .child(swatch(ping_color(PING_BAD + 1, cx), "Over 120"));
        let dash = |value: String| if measured { value } else { "—".into() };
        let row = |label: &'static str, value: String| {
            div()
                .min_h(px(26.))
                .flex()
                .items_center()
                .justify_between()
                .gap(px(12.))
                .child(div().flex_none().text_color(muted).child(label))
                .child(div().min_w_0().truncate().text_color(strong).font_weight(FontWeight::SEMIBOLD).font_features(tabular()).child(value))
        };
        let (lock_ink, secured) = match stats.dave {
            true => (palette::positive_ink(cx), "Voice and video in this call are end-to-end encrypted. Only the people in it can hear them."),
            false => (muted, "Voice and video are encrypted on the way to Discord's servers, not end to end."),
        };
        let card = div()
            .id("connection-card")
            .absolute()
            .left(px(8.))
            .bottom(px(self.panel_stack_height() + 16.))
            .w(px(300.))
            .p(px(16.))
            .rounded(px(RADIUS_LG))
            .bg(palette::surface(palette::Level::Overlay, cx))
            .shadow(pop_shadow(palette::line_strong(cx), cx))
            .flex()
            .flex_col()
            .gap(px(12.))
            .text_size(px(14.))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(div().flex().items_baseline().justify_between().gap(px(8.)).child(div().text_color(strong).font_weight(FontWeight::SEMIBOLD).child("Connection")).child(div().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(verdict_ink).child(verdict)))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(graph)
                    // GAP: the engine keeps its last 20 heartbeat pings, about 13.75s apart, so the graph spans roughly five minutes.
                    .child(div().flex().justify_between().text_size(px(12.)).text_color(faint).child("5 min ago").child("Now"))
                    .child(legend),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(row("Average ping", dash(format!("{} ms", stats.avg_ping_ms))))
                    .child(row("Packet loss", dash(format!("{:.1}%", loss(&stats)))))
                    .child(row("Server", dash(stats.endpoint.clone()))),
            )
            .child(
                div()
                    .pt(px(12.))
                    .border_t_1()
                    .border_color(palette::line(cx))
                    .flex()
                    .items_start()
                    .gap(px(8.))
                    .text_size(px(12.))
                    .text_color(muted)
                    .child(Icon::new(Glyph::HashtagLock).size(px(16.)).flex_none().text_color(lock_ink))
                    .child(div().flex_1().min_w_0().child(secured)),
            )
            .child(div().text_size(px(12.)).text_color(faint).child("Updated every 2 seconds"))
            .with_animation("connection-in", Animation::new(ui::DUR_POP), |d, t| d.opacity(t));
        Some(call_backdrop("connection-backdrop", cx).child(card).into_any_element())
    }

    // .devmenu, opened above its chevron: the devices to pick from, volume (with the live level for
    // the mic), noise suppression, and the way to the full settings. Over the stage it stays dark.
    fn device_overlay(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let menu = self.device_menu.as_ref()?;
        let ink = Inks::of(menu.on_stage, cx);
        let kind = menu.kind;
        let mut body: Vec<AnyElement> = Vec::new();
        let group = |body: &mut Vec<AnyElement>, title: &'static str, which: DeviceKind, devices: &[voice::Device], chosen: Option<String>, cx: &mut Context<Self>| {
            body.push(menu_label(title, &ink).into_any_element());
            let mut options: Vec<(Option<String>, String)> = Vec::new();
            if which != DeviceKind::Camera {
                options.push((None, "Default".into()));
            }
            options.extend(devices.iter().map(|d| (Some(d.id.clone()), d.name.clone())));
            body.extend(options.into_iter().enumerate().map(|(i, (id, name))| {
                let on = id == chosen;
                choice_row(SharedString::from(format!("device-{title}-{i}")), &ink)
                    .child(radio(on, &ink))
                    .child(div().flex_1().min_w_0().truncate().font_weight(FontWeight::MEDIUM).text_color(ink.strong).child(name))
                    .on_click(cx.listener(move |this, _, _, cx| this.choose_device(which, id.clone(), cx)))
                    .into_any_element()
            }));
        };
        match kind {
            DeviceKind::Input => {
                group(&mut body, "Input device", DeviceKind::Input, &menu.inputs, self.prefs.input_device.clone(), cx);
                group(&mut body, "Output device", DeviceKind::Output, &menu.outputs, self.prefs.output_device.clone(), cx);
            }
            DeviceKind::Output => group(&mut body, "Output device", DeviceKind::Output, &menu.outputs, self.prefs.output_device.clone(), cx),
            DeviceKind::Camera => group(&mut body, "Camera", DeviceKind::Camera, &menu.cameras, self.prefs.camera.clone(), cx),
        }
        body.push(menu_rule(&ink).into_any_element());
        if kind != DeviceKind::Camera {
            let (label, value, key) = match kind {
                DeviceKind::Output => ("Output volume", self.prefs.output_volume, "menu-output-volume"),
                _ => ("Input volume", self.prefs.input_volume, "menu-input-volume"),
            };
            let level = (kind == DeviceKind::Input).then(|| self.call.as_ref().map(|c| c.level.0)).flatten();
            body.push(
                self.call_menu_slider(SharedString::from(key), label, value / 2., &ink, move |this, f, cx| {
                    this.update_prefs(cx, |p| match kind {
                        DeviceKind::Output => p.output_volume = f.clamp(0., 1.) * 2.,
                        _ => p.input_volume = f.clamp(0., 1.) * 2.,
                    })
                }, cx)
                .children(level.map(|db| meter((db + 100.) / 100., &ink)))
                .into_any_element(),
            );
        }
        if kind == DeviceKind::Input {
            let noise = self.prefs.noise_suppression;
            body.push(
                choice_row("device-noise", &ink)
                    .child(div().flex_1().min_w_0().child(div().font_weight(FontWeight::MEDIUM).text_color(ink.strong).child("Noise suppression")).child(div().text_size(px(12.)).text_color(ink.muted).child("Filters fans and keys on this device")))
                    .child(ui::switch_live("device-noise-switch", noise, window, cx))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_noise_suppression(cx)))
                    .into_any_element(),
            );
            body.push(menu_rule(&ink).into_any_element());
        }
        let settings_label = if kind == DeviceKind::Camera { "Video settings" } else { "Voice settings" };
        body.push(
            choice_row("device-settings", &ink)
                .child(Icon::new(Glyph::Settings).size(px(18.)).text_color(ink.muted))
                .child(div().flex_1().font_weight(FontWeight::MEDIUM).text_color(ink.text).child(settings_label))
                .child(Icon::new(Glyph::ChevronRight).size(px(14.)).text_color(ink.faint))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.device_menu = None;
                    this.open_settings(Page::VoiceVideo, window, cx);
                }))
                .into_any_element(),
        );
        let viewport = window.viewport_size();
        let card = div()
            .id("device-menu")
            .absolute()
            .left(px((f32::from(menu.at.x) - 150.).clamp(8., f32::from(viewport.width) - 308.)))
            .bottom(px((f32::from(viewport.height) - f32::from(menu.at.y) + 12.).max(8.)))
            .w(px(300.))
            .p(px(6.))
            .rounded(px(RADIUS_LG))
            .bg(ink.overlay)
            .shadow(pop_shadow(ink.line_strong, cx))
            .flex()
            .flex_col()
            .gap(px(1.))
            .text_size(px(14.))
            .text_color(ink.text)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .children(body)
            .with_animation("device-menu-in", Animation::new(ui::DUR_POP), |d, t| d.opacity(t));
        Some(call_backdrop("device-backdrop", cx).child(card).into_any_element())
    }

    // .mslider: the label and its percentage over the slider; click or drag anywhere on it.
    fn call_menu_slider(&self, key: SharedString, label: &'static str, fraction: f32, ink: &Inks, on_set: impl Fn(&mut Rustcord, f32, &mut Context<Rustcord>) + Clone + 'static, cx: &mut Context<Self>) -> Div {
        let bounds = self.slider_bounds.clone();
        let record = bounds.clone();
        let (down_key, move_key, record_key) = (key.clone(), key.clone(), key.clone());
        let set = move |this: &mut Rustcord, key: &SharedString, x: Pixels, cx: &mut Context<Rustcord>| {
            let Some(b) = bounds.borrow().get(key).copied() else { return };
            on_set(this, ((x - b.left()) / b.size.width).clamp(0., 1.), cx);
        };
        let set_drag = set.clone();
        let slider = div()
            .id(SharedString::from(format!("menu-slider-{key}")))
            .relative()
            .w_full()
            .cursor_pointer()
            .child(gpui_kit::canvas(move |b, _, _| { record.borrow_mut().insert(record_key.clone(), b); }, |_, _, _, _| {}).absolute().size_full())
            .child(ui::slider_track(fraction, cx))
            .on_mouse_down(MouseButton::Left, cx.listener(move |this, e: &MouseDownEvent, _, cx| set(this, &down_key, e.position.x, cx)))
            .on_drag(MenuSliderDrag(key.clone()), |drag, _, _, cx| cx.new(|_| drag.clone()))
            .on_drag_move::<MenuSliderDrag>(cx.listener(move |this, e: &gpui_kit::DragMoveEvent<MenuSliderDrag>, _, cx| {
                if e.drag(cx).0 == move_key {
                    set_drag(this, &move_key, e.event.position.x, cx);
                }
            }));
        div()
            .px(px(10.))
            .pt(px(6.))
            .pb(px(10.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(div().flex().justify_between().font_weight(FontWeight::MEDIUM).text_color(ink.text).child(label).child(div().text_color(ink.muted).font_features(tabular()).child(format!("{:.0}%", fraction * 100.))))
            .child(slider)
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

// The call stage is dark in both themes (design/css/voice.css `.call`), so whatever sits on it reads
// these instead of the theme's tokens. Converted from the CSS's OKLCH.
// MOVE(ui): stage scope, as palette::stage(..) beside the theme ramps.
mod stage {
    use gpui_kit::{Hsla, rgb};

    fn hex(value: u32) -> Hsla {
        rgb(value).into()
    }

    pub fn bg() -> Hsla {
        hex(0x0A0608)
    }

    pub fn page() -> Hsla {
        hex(0x1C1619)
    }

    pub fn raised() -> Hsla {
        hex(0x261F22)
    }

    pub fn overlay() -> Hsla {
        hex(0x2C2528)
    }

    pub fn ended() -> Hsla {
        hex(0x171214)
    }

    pub fn strong() -> Hsla {
        hex(0xF7F4F5)
    }

    pub fn text() -> Hsla {
        hex(0xDCD5D8)
    }

    pub fn muted() -> Hsla {
        hex(0xABA2A5)
    }

    pub fn faint() -> Hsla {
        hex(0x958C90)
    }

    pub fn online() -> Hsla {
        hex(0x3EC87D)
    }

    pub fn idle() -> Hsla {
        hex(0xF2B036)
    }

    pub fn dnd() -> Hsla {
        hex(0xF05136)
    }

    // Translucent white: hover .05, press .09, selected .08, line .08, strong line .14.
    pub fn wash(alpha: f32) -> Hsla {
        gpui_kit::white().opacity(alpha)
    }

    // Off states (muted, camera off) are soft coral, apart from the red that leaves.
    pub fn off(hover: bool) -> Hsla {
        hex(0xF05136).opacity(if hover { 0.3 } else { 0.2 })
    }

    pub fn off_ink() -> Hsla {
        hex(0xFF9079)
    }

    // On states (camera on) are a light fill with dark ink.
    pub fn on(hover: bool) -> Hsla {
        hex(if hover { 0xDBD6D8 } else { 0xF1EDEF })
    }

    pub fn on_ink() -> Hsla {
        hex(0x1A1417)
    }
}

// The few tokens a menu reads, from the stage or from the theme.
struct Inks {
    overlay: Hsla,
    strong: Hsla,
    text: Hsla,
    muted: Hsla,
    faint: Hsla,
    hover: Hsla,
    line: Hsla,
    line_strong: Hsla,
}

impl Inks {
    fn of(on_stage: bool, cx: &App) -> Self {
        match on_stage {
            true => Inks { overlay: stage::overlay(), strong: stage::strong(), text: stage::text(), muted: stage::muted(), faint: stage::faint(), hover: stage::wash(0.05), line: stage::wash(0.08), line_strong: stage::wash(0.14) },
            false => Inks {
                overlay: palette::surface(palette::Level::Overlay, cx),
                strong: palette::strong(cx),
                text: palette::text(cx),
                muted: palette::muted(cx),
                faint: palette::faint(cx),
                hover: palette::hover(cx),
                line: palette::line(cx),
                line_strong: palette::line_strong(cx),
            },
        }
    }
}

// --shadow-pop: a soft drop plus a 1px ring in the strong line colour.
// MOVE(ui): pop shadow, also used by the Go live picker and the stream card.
pub(crate) fn pop_shadow(ring: Hsla, cx: &App) -> Vec<BoxShadow> {
    let drop = if cx.theme().is_dark() { gpui_kit::black().opacity(0.45) } else { Hsla::from(rgb(0x3D2A22)).opacity(0.16) };
    vec![
        BoxShadow { color: drop, offset: point(px(0.), px(12.)), blur_radius: px(32.), spread_radius: px(0.), inset: false },
        BoxShadow { color: ring, offset: point(px(0.), px(0.)), blur_radius: px(0.), spread_radius: px(1.), inset: false },
    ]
}

// The mini wave at rest (.bars.still): 3px bars, 2px apart. `weak` dims all but the first.
// MOVE(ui): bars_still at the .bars size (ui's is the LIVE badge's 2px).
fn still_bars(color: Hsla, height: f32, weak: bool) -> Div {
    div().h(px(height)).flex_none().flex().items_center().gap(px(2.)).children([0.55_f32, 1., 0.7].into_iter().enumerate().map(move |(i, base)| {
        let ink = if weak && i > 0 { color.opacity(0.3) } else { color };
        div().w(px(3.)).h(px(height * base)).rounded_full().bg(ink)
    }))
}

// .voicebar .why: the second line, in the status's tone, saying what needs you.
fn why_line(text: impl Into<SharedString>, tone: Hsla) -> Div {
    div().h(px(18.)).truncate().text_size(px(12.)).text_color(tone).child(text.into())
}

// .voicebar .acts .btn: equal-width 32px buttons; disabled until connected, saying why.
fn panel_action(id: &'static str, glyph: Glyph, tip: &'static str, enabled: bool, cx: &App) -> Stateful<Div> {
    let (fill, press, ink) = (palette::selected(cx), palette::press(cx), palette::strong(cx));
    div()
        .id(id)
        .flex_1()
        .min_w_0()
        .h(px(32.))
        .rounded(px(RADIUS_SM))
        .flex()
        .items_center()
        .justify_center()
        .bg(fill)
        .text_color(ink)
        .child(Icon::new(glyph).size(px(18.)))
        .when(enabled, |b| b.cursor_pointer().hover(move |s| s.bg(press)))
        .when(!enabled, |b| b.opacity(0.45).cursor(gpui_kit::CursorStyle::OperationNotAllowed))
        .tooltip(move |window, cx| Tooltip::new(tip).build(window, cx))
}

// .iconbtn on the stage: muted until hovered; `pressed` holds the selected fill.
fn stage_icon_button(id: &'static str, glyph: Glyph, tip: &'static str, pressed: bool) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(32.))
        .flex_none()
        .rounded(px(RADIUS_SM))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .text_color(if pressed { stage::strong() } else { stage::muted() })
        .when(pressed, |b| b.bg(stage::wash(0.08)))
        .hover(|s| s.bg(stage::wash(0.05)).text_color(stage::strong()))
        .active(|s| s.bg(stage::wash(0.09)))
        .child(Icon::new(glyph).size(px(20.)))
        .tooltip(move |window, cx| Tooltip::new(tip).build(window, cx))
}

// .btn.btn--sm on the stage: the quiet fill.
fn stage_button(id: &'static str, label: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(30.))
        .px(px(10.))
        .flex_none()
        .rounded(px(RADIUS_SM))
        .flex()
        .items_center()
        .gap(px(8.))
        .cursor_pointer()
        .bg(stage::wash(0.08))
        .hover(|s| s.bg(stage::wash(0.09)))
        .text_color(stage::strong())
        .text_size(px(14.))
        .font_weight(FontWeight::SEMIBOLD)
        .whitespace_nowrap()
        .child(label)
}

// .preview .watch: the light button over a dimmed stream.
pub(crate) fn watch_button(label: &'static str) -> Div {
    div()
        .h(px(30.))
        .px(px(10.))
        .rounded(px(RADIUS_SM))
        .flex()
        .items_center()
        .gap(px(8.))
        .bg(stage::strong())
        .hover(|s| s.bg(gpui_kit::white()))
        .text_color(stage::on_ink())
        .text_size(px(14.))
        .font_weight(FontWeight::SEMIBOLD)
        .child(Icon::new(Glyph::Play).size(px(16.)))
        .child(label)
}

// A call control's state: .cbtn at rest, .off (coral), .on (light), .active (accent), the open
// chevron ([aria-expanded]), or .leave (the one red fill).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Control {
    Rest,
    Off,
    On,
    Active,
    Open,
    Leave,
}

// .cbtn: 52x48, a squircle; icons 22px (the chevron 14px).
// GAP: the design's icons here are Phosphor's fill weight; Wave bundles the regular ones only.
// PATCH(transform): scale to 0.92 while pressed (.cbtn:active), on the touch spring.
fn control(id: &'static str, glyph: Glyph, tip: &'static str, state: Control, enabled: bool) -> Stateful<Div> {
    let (fill, hover, ink) = match state {
        Control::Rest => (stage::wash(0.1), stage::wash(0.16), gpui_kit::white()),
        Control::Off => (stage::off(false), stage::off(true), stage::off_ink()),
        Control::On => (stage::on(false), stage::on(true), stage::on_ink()),
        Control::Active => (palette::accent(), palette::accent_hover(), gpui_kit::white()),
        Control::Open => (stage::wash(0.2), stage::wash(0.2), gpui_kit::white()),
        Control::Leave => (palette::danger(), palette::danger_hover(), gpui_kit::white()),
    };
    let chevron = matches!(glyph, Glyph::ChevronUp);
    div()
        .id(id)
        .w(px(if chevron { 26. } else { 52. }))
        .h(px(CONTROLS_HEIGHT))
        .flex_none()
        .rounded(px(12.))
        .flex()
        .items_center()
        .justify_center()
        .bg(fill)
        .text_color(ink)
        .child(Icon::new(glyph).size(px(if chevron { 14. } else { 22. })))
        .when(enabled, |b| b.cursor_pointer().hover(move |s| s.bg(hover)))
        .when(!enabled, |b| b.opacity(0.4).cursor(gpui_kit::CursorStyle::OperationNotAllowed))
        .tooltip(move |window, cx| Tooltip::new(tip).build(window, cx))
}

// .csplit: the main half and its chevron 2px apart, their facing corners small.
fn split_control(main: Stateful<Div>, chevron: Stateful<Div>) -> Div {
    div().flex().gap(px(2.)).child(main.rounded_tr(px(4.)).rounded_br(px(4.))).child(chevron.rounded_tl(px(4.)).rounded_bl(px(4.)))
}

// .tile .name: 12px semibold white along the tile's foot.
// GAP: gpui has no text-shadow, so the 1px shadow under the label is left to the scrim.
fn tile_name(content: Div) -> Div {
    content.absolute().left(px(10.)).right(px(10.)).bottom(px(8.)).flex().items_center().gap(px(6.)).min_w_0().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(gpui_kit::white())
}

// The tile's edge, drawn above its media so a stream can't hide it: a faint 1px line, or while
// speaking a 2px accent ring with a dark inner line (neutral when they're muted for you).
fn tile_ring(speaking: bool, local_muted: bool) -> Div {
    let ring = div().absolute().inset_0().rounded(px(RADIUS_LG));
    if !speaking {
        return ring.border_1().border_color(stage::wash(0.06));
    }
    let color = if local_muted { stage::wash(0.42) } else { palette::accent() };
    ring.border(px(2.)).border_color(color).child(div().absolute().inset_0().rounded(px(RADIUS_LG - 2.)).border(px(2.)).border_color(stage::bg().opacity(0.6)))
}

// .tile.has-media::before: a smooth scrim under the labels, never a hard strip.
fn scrim_top(radius: f32) -> Div {
    ui::edge(div().absolute().top_0().left_0().right_0().h(relative(0.3)), ui::top(radius)).bg(gpui_kit::linear_gradient(180., gpui_kit::linear_color_stop(gpui_kit::black().opacity(0.35), 0.), gpui_kit::linear_color_stop(gpui_kit::transparent_black(), 1.)))
}

fn scrim_bottom(radius: f32) -> Div {
    ui::edge(div().absolute().bottom_0().left_0().right_0().h(relative(0.38)), ui::bottom(radius)).bg(gpui_kit::linear_gradient(180., gpui_kit::linear_color_stop(gpui_kit::transparent_black(), 0.), gpui_kit::linear_color_stop(gpui_kit::black().opacity(0.55), 1.)))
}

// .tile .viewers: the watch count, top right.
// GAP: the design's eye glyph isn't bundled; the people glyph stands in.
fn viewer_chip(viewers: usize) -> Div {
    div()
        .absolute()
        .right(px(8.))
        .top(px(8.))
        .h(px(20.))
        .px(px(7.))
        .rounded(px(RADIUS_XS))
        .flex()
        .items_center()
        .gap(px(4.))
        .bg(gpui_kit::black().opacity(0.45))
        .text_color(gpui_kit::white())
        .text_size(px(11.))
        .font_weight(FontWeight::BOLD)
        .font_features(tabular())
        .child(Icon::new(Glyph::Users).size(px(12.)))
        .child(viewers.to_string())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MarkKind {
    // A moderator's lock: solid red.
    Server,
    // Your own choice, so neutral.
    Local,
}

// .vmark: why you can't hear someone, as a chip; on tiles it can carry the words.
fn vmark(glyph: Glyph, words: Option<&'static str>, kind: MarkKind) -> Div {
    let base = div()
        .h(px(18.))
        .min_w(px(18.))
        .flex_none()
        .rounded(px(4.))
        .flex()
        .items_center()
        .justify_center()
        .gap(px(4.))
        .text_size(px(11.))
        .font_weight(FontWeight::BOLD)
        .whitespace_nowrap()
        .text_color(gpui_kit::white())
        .when(words.is_some(), |d| d.pl(px(4.)).pr(px(6.)))
        .when(words.is_none(), |d| d.px(px(3.)))
        .child(Icon::new(glyph).size(px(12.)))
        .children(words);
    match kind {
        MarkKind::Server => base.bg(palette::danger()),
        MarkKind::Local => base.bg(stage::bg().opacity(0.72)).border_1().border_color(stage::wash(0.16)),
    }
}

// .stackav: avatars overlapping by 6px, each ringed in the surface so they read apart.
// MOVE(ui): avatar_stack measures narrower than its avatars under negative margins, so its
// neighbours slide underneath; this one is sized up front.
fn face_stack(urls: Vec<String>, size: f32, ring: Hsla) -> Div {
    let step = size - 6.;
    let width = if urls.is_empty() { 0. } else { size + step * (urls.len() - 1) as f32 };
    div().relative().flex_none().w(px(width)).h(px(size)).children(urls.into_iter().enumerate().map(move |(i, url)| ui::avatar(url, size).absolute().top_0().left(px(step * i as f32)).border_2().border_color(ring)))
}

// .tile.stream: a stream before its first preview arrives.
pub(crate) fn stream_fill(radius: f32) -> Div {
    let glow = gpui_kit::linear_gradient(135., gpui_kit::linear_color_stop(Hsla::from(rgb(0x0097A1)).opacity(0.7), 0.), gpui_kit::linear_color_stop(Hsla::from(rgb(0x9D2398)).opacity(0.7), 1.));
    div().relative().rounded(px(radius)).bg(rgb(0x1D1E39)).child(div().absolute().inset_0().rounded(px(radius)).bg(glow))
}

// .mlabel
fn menu_label(text: &'static str, ink: &Inks) -> Div {
    div().pt(px(6.)).px(px(10.)).pb(px(2.)).text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(ink.muted).child(text)
}

// .devmenu hr
fn menu_rule(ink: &Inks) -> Div {
    div().h(px(1.)).mx(px(4.)).my(px(4.)).flex_none().bg(ink.line)
}

// .choice.vrow: a 34px menu row holding a radio or a switch.
fn choice_row(id: impl Into<ElementId>, ink: &Inks) -> Stateful<Div> {
    let hover = ink.hover;
    div().id(id).min_h(px(34.)).px(px(10.)).py(px(6.)).rounded(px(RADIUS_SM)).flex().items_center().gap(px(10.)).cursor_pointer().hover(move |s| s.bg(hover))
}

// .vrow .radio: 18px; on is a 5px accent band.
// MOVE(ui): radio with a size and a ring colour, so a dark scope can use it.
fn radio(on: bool, ink: &Inks) -> Div {
    div().size(px(18.)).flex_none().rounded_full().when(on, |d| d.border(px(5.)).border_color(palette::accent())).when(!on, |d| d.border(px(1.5)).border_color(ink.line_strong))
}

// .meter: the live level as a row of pills.
// MOVE(ui): meter.
fn meter(level: f32, ink: &Inks) -> Div {
    let lit = (level.clamp(0., 1.) * METER_BARS as f32).round() as usize;
    div().h(px(14.)).flex().items_center().gap(px(2.)).children((0..METER_BARS).map(|i| div().flex_1().min_w(px(3.)).h_full().rounded_full().bg(if i < lit { stage::online() } else { ink.line_strong })))
}

#[derive(Clone)]
struct MenuSliderDrag(SharedString);

impl gpui_kit::Render for MenuSliderDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

// --ease-spring: CSS linear() stops for a small overshoot that settles.
// MOVE(ui): ease_spring beside the motion tokens.
fn ease_spring(t: f32) -> f32 {
    use gpui_kit::base::{Easing, LinearStop};
    let stops = [LinearStop::new(0.), LinearStop::at(0.42, 0.12), LinearStop::at(0.82, 0.25), LinearStop::at(1.06, 0.4), LinearStop::at(1.1, 0.48), LinearStop::at(1.04, 0.62), LinearStop::at(0.99, 0.78), LinearStop::new(1.)];
    Easing::linear_stops(stops).map_or(t, |e| e.sample(t))
}

fn tabular() -> FontFeatures {
    FontFeatures(Arc::new(vec![("tnum".into(), 1)]))
}

// MOVE(ui): oklch <-> sRGB, as palette already does privately for role colours.
fn oklch(l: f32, c: f32, h: f32) -> Hsla {
    let (a, b) = (c * h.to_radians().cos(), c * h.to_radians().sin());
    let l_ = (l + 0.396_337_8 * a + 0.215_803_76 * b).powi(3);
    let m_ = (l - 0.105_561_35 * a - 0.063_854_17 * b).powi(3);
    let s_ = (l - 0.089_484_18 * a - 1.291_485_5 * b).powi(3);
    let channel = |v: f32| {
        let v = v.clamp(0., 1.);
        if v <= 0.003_130_8 { 12.92 * v } else { 1.055 * v.powf(1. / 2.4) - 0.055 }
    };
    let r = channel(4.076_741_7 * l_ - 3.307_711_6 * m_ + 0.230_969_94 * s_);
    let g = channel(-1.268_438 * l_ + 2.609_757_4 * m_ - 0.341_319_38 * s_);
    let bl = channel(-0.004_196_086 * l_ - 0.703_418_6 * m_ + 1.707_614_7 * s_);
    gpui_kit::Rgba { r, g, b: bl, a: 1. }.into()
}

fn oklch_hue(color: Hsla) -> f32 {
    let rgba = gpui_kit::Rgba::from(color);
    let lin = |c: f32| if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
    let (r, g, b) = (lin(rgba.r), lin(rgba.g), lin(rgba.b));
    let l = (0.412_221_47 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    let (a, bb) = (1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s, 0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s);
    bb.atan2(a).to_degrees().rem_euclid(360.)
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
