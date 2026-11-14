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
const PREVIEW_AGE: std::time::Duration = std::time::Duration::from_secs(60);
// Discord's call tiles (read from Discord web, 2026-11-14).
const TILE_RADIUS: f32 = 8.;
const TILE_GAP: f32 = 8.;
const STRIP_HEIGHT: f32 = 112.;
const STRIP_LIMIT: usize = 12;
const RECENT_SPEAKER: std::time::Duration = std::time::Duration::from_secs(10);
const CONTROLS_IDLE: Duration = Duration::from_secs(3);
const HEADER_HEIGHT: f32 = 48.;
const CONTROLS_HEIGHT: f32 = 96.;
const TILE_BG: gpui_kit::Rgba = gpui_kit::Rgba { r: 0x1E as f32 / 255., g: 0x1F as f32 / 255., b: 0x22 as f32 / 255., a: 1. };

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

    pub(crate) fn voice_panel_height(&self) -> f32 {
        match (&self.call, &self.voice_error) {
            (Some(call), _) => VOICE_HEIGHT + if call.sharing.is_some() { GO_LIVE_HEIGHT } else { 0. },
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
                    .child(panel_button("dismiss-voice", Glyph::X, "Dismiss", cx).on_click(cx.listener(|this, _, _, cx| {
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
            .border_color(Hsla::from(rgb(0x97979F)).opacity(0.16))
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
                panel_button("noise", Glyph::Waveform, if noise { "Disable Noise Suppression" } else { "Enable Noise Suppression" }, cx)
                    .when(noise, |b| b.text_color(theme.foreground))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_noise_suppression(cx))),
            )
            .child(panel_button_tinted("hang-up", Glyph::CallEnd, "Disconnect", Some(rgb(0xDA3E44).into()), cx).on_click(cx.listener(|this, _, _, cx| this.leave_voice(cx))));
        let sharing = call.sharing.is_some();
        let actions = div()
            .flex()
            .gap_2()
            .child(wide_button("panel-camera", if call.camera_on { Glyph::Video } else { Glyph::VideoOff }, if call.camera_on { "Turn Off Camera" } else { "Turn On Camera" }, call.camera_on, connected, cx).on_click(cx.listener(|this, _, _, cx| this.toggle_camera(cx))))
            .child(wide_button("panel-share", if sharing { Glyph::ScreenOff } else { Glyph::ShareScreen }, if sharing { "Stop Streaming" } else { "Share Your Screen" }, sharing, connected, cx).on_click(cx.listener(|this, _, _, cx| this.toggle_screen_share(cx))))
            .child(wide_button("panel-activities", Glyph::Activities, "Start an Activity", false, connected, cx).on_click(cx.listener(|this, _, _, cx| this.show_toast("Activities aren't in Rustcord yet", cx))))
            .child(wide_button("panel-soundboard", Glyph::Soundboard, "Open Soundboard", false, connected, cx).on_click(cx.listener(|this, _, _, cx| this.show_toast("The soundboard isn't in Rustcord yet", cx))));
        let voice = div().h(px(VOICE_HEIGHT)).p_2().flex().flex_col().gap(px(12.)).border_b_1().border_color(Hsla::from(rgb(0x97979F)).opacity(0.16)).child(header).child(actions);
        Some(div().flex().flex_col().children(self.go_live_panel(cx)).child(voice).into_any_element())
    }

    // Discord's Go Live panel above the voice panel while we share: the source, its quality, and
    // a button to stop.
    fn go_live_panel(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let call = self.call.as_ref()?;
        call.sharing.as_ref()?;
        let theme = cx.theme().clone();
        let label = call.share_label.clone().unwrap_or_else(|| "Screen".into());
        let initial: String = label.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
        let resolution = match self.prefs.stream_resolution {
            0 => "Source".to_owned(),
            r => format!("{r}p"),
        };
        // Discord colours quality above its free tier (720p 30fps) in Nitro pink.
        let boosted = self.prefs.stream_resolution == 0 || self.prefs.stream_resolution > 720 || self.prefs.stream_fps > 30;
        let quality: Hsla = if boosted { rgb(0xFF73FA).into() } else { theme.muted_foreground };
        Some(
            div()
                .h(px(GO_LIVE_HEIGHT))
                .px_2()
                .flex()
                .items_center()
                .gap_3()
                .border_b_1()
                .border_color(Hsla::from(rgb(0x97979F)).opacity(0.16))
                .child(
                    div()
                        .relative()
                        .size(px(40.))
                        .flex_none()
                        .rounded(px(8.))
                        .bg(rgb(0x45A796))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(gpui_kit::white())
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(initial)
                        .child(
                            div()
                                .absolute()
                                .right(px(-4.))
                                .bottom(px(-4.))
                                .size(px(20.))
                                .rounded(px(6.))
                                .bg(theme.popover)
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(Icon::new(Glyph::Video).size(px(14.)).text_color(gpui_kit::white())),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(div().truncate().text_size(px(16.)).font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(label))
                        .child(div().text_size(px(12.)).font_weight(FontWeight::MEDIUM).text_color(quality).child(format!("{resolution} {}FPS", self.prefs.stream_fps))),
                )
                .child(panel_button("stop-streaming", Glyph::ScreenOff, "Stop Streaming", cx).on_click(cx.listener(|this, _, _, cx| this.toggle_screen_share(cx)))),
        )
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

    // The call view: a grid of 16:9 tiles, or the watched stream large over a collapsible strip of
    // everyone else. The header and control bar float over the stage and fade out while the pointer
    // is still, as Discord's do.
    pub(crate) fn render_call(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let call = self.call.as_ref()?;
        let tiles = self.ordered_tiles();
        let viewport = window.viewport_size();
        let width = f32::from(viewport.width) - RAIL_WIDTH - self.channels_width - 32.;
        let height = f32::from(viewport.height) - 32. - 16.;
        let chrome = !self.call_idle();
        let stage: AnyElement = match &call.watching {
            Some(watching) => {
                let strip_shown = !self.call_strip_hidden && tiles.len() > 1;
                let main_h = (height - CONTROLS_HEIGHT - if strip_shown { STRIP_HEIGHT + TILE_GAP } else { 0. }).max(120.);
                let main = match &watching.frame {
                    Some(frame) => {
                        let (w, h) = (frame.size(0).width.0 as f32, frame.size(0).height.0 as f32);
                        let scale = (width / w).min(main_h / h);
                        img(ImageSource::Render(frame.clone())).w(px(w * scale)).h(px(h * scale)).rounded(px(TILE_RADIUS)).into_any_element()
                    }
                    None => div().text_color(gpui_kit::white().opacity(0.7)).child(format!("Joining {}'s stream…", self.user_name(&watching.streamer_id))).into_any_element(),
                };
                let strip_w = STRIP_HEIGHT * 16. / 9.;
                let strip: Vec<AnyElement> = tiles
                    .iter()
                    .filter(|t| !matches!(t, Tile::Stream(s) if s.user_id == watching.streamer_id))
                    .take(STRIP_LIMIT)
                    .map(|t| self.tile(t, strip_w, STRIP_HEIGHT, true, cx))
                    .collect();
                div()
                    .size_full()
                    .pb(px(CONTROLS_HEIGHT))
                    .flex()
                    .flex_col()
                    .gap(px(TILE_GAP))
                    .child(div().flex_1().min_h_0().flex().items_center().justify_center().child(main))
                    .when(strip_shown, |d| d.child(div().flex_none().flex().justify_center().gap(px(TILE_GAP)).children(strip)))
                    .into_any_element()
            }
            None => {
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
        let fade = |id: &'static str, el: AnyElement| {
            div().with_animation(SharedString::from(format!("{id}-{chrome}")), Animation::new(Duration::from_millis(if chrome { 150 } else { 300 })), move |d, t| d.opacity(if chrome { t } else { 1. - t })).child(el)
        };
        Some(
            div()
                .id("call-view")
                .relative()
                .size_full()
                .bg(gpui_kit::black())
                .on_mouse_move(cx.listener(|this, _, _, cx| {
                    let was_idle = this.call_idle();
                    this.call_pointer_at = Instant::now();
                    if was_idle {
                        cx.notify();
                    }
                }))
                .child(div().size_full().px_4().child(stage))
                .child(div().absolute().top_0().left_0().right_0().child(fade("call-header", self.call_header(cx))))
                .child(div().absolute().bottom_0().left_0().right_0().child(fade("call-controls", self.call_controls(window, cx).into_any_element())))
                .into_any_element(),
        )
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
        let Some(watching) = &call.watching else {
            let people = self.participants().len();
            return bar
                .child(Icon::new(Glyph::VolumeHigh).size(px(20.)).text_color(gpui_kit::white().opacity(0.7)))
                .child(div().font_weight(FontWeight::SEMIBOLD).child(call.channel.name.clone()))
                .child(div().text_size(px(14.)).text_color(gpui_kit::white().opacity(0.6)).child(format!("{people} in voice")))
                .into_any_element();
        };
        let suffix = format!(":{}", watching.streamer_id);
        let viewers: Vec<String> = self.stream_viewers.iter().find(|(key, _)| key.ends_with(&suffix)).map(|(_, v)| v.clone()).unwrap_or_default();
        // GAP: the frame rate isn't reported by the voice engine, so only the resolution shows.
        let quality = watching.frame.as_ref().map(|f| format!("{}P", f.size(0).height.0));
        let avatars: Vec<AnyElement> = viewers.iter().take(3).map(|id| img(self.user_avatar(id)).size(px(20.)).rounded_full().border_2().border_color(gpui_kit::black()).ml(px(-6.)).into_any_element()).collect();
        bar.child(Icon::new(Glyph::Screen).size(px(20.)).text_color(gpui_kit::white().opacity(0.7)))
            .child(div().font_weight(FontWeight::SEMIBOLD).truncate().child(format!("{}'s Screen", self.user_name(&watching.streamer_id))))
            .children(quality.map(|q| div().h(px(18.)).px_1().rounded(px(4.)).bg(gpui_kit::white().opacity(0.16)).text_size(px(11.)).font_weight(FontWeight::BOLD).flex().items_center().child(q)))
            .child(live_badge())
            .when(!viewers.is_empty(), |b| {
                b.child(div().pl(px(6.)).flex().items_center().children(avatars).when(viewers.len() > 3, |d| d.child(div().pl_1().text_size(px(12.)).font_weight(FontWeight::BOLD).child(format!("+{}", viewers.len() - 3)))))
            })
            .child(div().flex_1())
            .child(pill_button("stop-watching", "Stop Watching").on_click(cx.listener(|this, _, _, cx| this.stop_watching(cx))))
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
                .bg(TILE_BG)
                .children(self.stream_preview(&state.user_id).map(|url| img(url).size_full().object_fit(gpui_kit::ObjectFit::Cover).opacity(0.15)))
                .into_any_element(),
            (false, Some(frame)) => img(ImageSource::Render(frame.clone())).size_full().object_fit(gpui_kit::ObjectFit::Cover).into_any_element(),
            (false, None) => div()
                .size_full()
                .bg(self.avatar_colors.get(&state.user_id).copied().flatten().unwrap_or(TILE_BG.into()))
                .flex()
                .items_center()
                .justify_center()
                .child(img(user.avatar.clone()).size(px((h * 0.4).clamp(32., 80.))).rounded_full())
                .into_any_element(),
        };
        let watching = call.watching.as_ref().is_some_and(|w| w.streamer_id == state.user_id);
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
                    .child(div().truncate().child(user.name.clone())),
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
            .when(is_stream, |d| d.child(div().absolute().top(px(8.)).right(px(8.)).child(live_badge())))
            .when(watchable, |d| {
                d.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(pill_button(SharedString::from(format!("watch-{streamer}")), "Watch Stream").on_click(cx.listener(move |this, _, _, cx| this.watch_stream(streamer.clone(), cx)))),
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
    fn call_controls(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let call = self.call.as_ref().expect("in a call");
        let muted = self.muted || self.deafened;
        let connected = call.stage == Stage::Connected;
        let sharing = call.sharing.is_some();
        let fullscreen = window.is_fullscreen();
        let invite = call.guild_id.clone().map(|guild| {
            let channel = call.channel.id.clone();
            round_button("call-invite", Glyph::GroupPlus, "Invite to Voice").on_click(cx.listener(move |this, _, window, cx| this.show_invite(guild.clone(), channel.clone(), window, cx)))
        });
        let strip_toggle = call.watching.is_some().then(|| {
            let hidden = self.call_strip_hidden;
            round_button("call-strip", if hidden { Glyph::ChevronUp } else { Glyph::ChevronDown }, if hidden { "Show Members" } else { "Hide Members" }).on_click(cx.listener(|this, _, _, cx| {
                this.call_strip_hidden = !this.call_strip_hidden;
                cx.notify();
            }))
        });
        let center = div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .flex()
                    .child(
                        split_button("call-mic", if muted { Glyph::MicOff } else { Glyph::Mic }, if muted { "Unmute" } else { "Mute" }, muted, cx)
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_mute(cx))),
                    )
                    .child(chevron("call-mic-devices", cx).on_mouse_down(MouseButton::Left, cx.listener(|this, e: &MouseDownEvent, _, cx| this.open_device_menu(DeviceKind::Input, e.position, cx)))),
            )
            .child(
                div()
                    .flex()
                    .child(
                        split_button("call-camera", if call.camera_on { Glyph::Video } else { Glyph::VideoOff }, if call.camera_on { "Turn Off Camera" } else { "Turn On Camera" }, !call.camera_on, cx)
                            .when(!connected, |b| b.opacity(0.5))
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_camera(cx))),
                    )
                    .child(chevron("call-camera-devices", cx).on_mouse_down(MouseButton::Left, cx.listener(|this, e: &MouseDownEvent, _, cx| this.open_device_menu(DeviceKind::Camera, e.position, cx)))),
            )
            .child(
                split_button("call-share", if sharing { Glyph::ScreenOff } else { Glyph::ShareScreen }, if sharing { "Stop Streaming" } else { "Share Your Screen" }, false, cx)
                    .rounded(px(12.))
                    .when(sharing, |b| b.bg(Hsla::from(rgb(0x008545)).opacity(0.24)).text_color(rgb(0x62B77C)))
                    .when(!connected, |b| b.opacity(0.5))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_screen_share(cx))),
            )
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
                    .bg(rgb(0xDA3E44))
                    .text_color(gpui_kit::white())
                    .hover(|s| s.bg(rgb(0xA2212B)))
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
                    .children(strip_toggle)
                    .child(
                        round_button("call-fullscreen", if fullscreen { Glyph::FullscreenExit } else { Glyph::Fullscreen }, if fullscreen { "Exit Full Screen" } else { "Full Screen" })
                            .on_click(|_, window, _| window.toggle_fullscreen()),
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

fn panel_button(id: &'static str, glyph: Glyph, label: &'static str, cx: &mut Context<Rustcord>) -> Stateful<Div> {
    panel_button_tinted(id, glyph, label, None, cx)
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
    let wash = |a: f32| Hsla::from(rgb(0x97979F)).opacity(a);
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
        .ml(px(-11.))
        .w(px(24.))
        .h(px(48.))
        .rounded_r(px(12.))
        .border_l_1()
        .border_color(gpui_kit::black().opacity(0.4))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .bg(gpui_kit::white().opacity(0.1))
        .text_color(gpui_kit::white())
        .hover(|s| s.bg(gpui_kit::white().opacity(0.16)))
        .child(Icon::new(Glyph::ChevronDown).size(px(16.)))
}

fn pill_button(id: impl Into<SharedString>, text: &'static str) -> Stateful<Div> {
    div()
        .id(gpui_kit::ElementId::Name(id.into()))
        .h(px(32.))
        .px_3()
        .rounded(px(8.))
        .flex()
        .items_center()
        .cursor_pointer()
        .bg(gpui_kit::black().opacity(0.6))
        .text_color(gpui_kit::white())
        .text_size(px(14.))
        .font_weight(FontWeight::MEDIUM)
        .hover(|s| s.bg(gpui_kit::black().opacity(0.8)))
        .child(text)
}

fn menu_row(id: impl Into<SharedString>, label: String, checked: bool, cx: &mut Context<Rustcord>) -> Stateful<Div> {
    let theme = cx.theme().clone();
    div()
        .id(gpui_kit::ElementId::Name(id.into()))
        .h(px(32.))
        .px_2()
        .rounded(px(4.))
        .flex()
        .items_center()
        .justify_between()
        .gap_2()
        .cursor_pointer()
        .text_size(px(14.))
        .font_weight(FontWeight::MEDIUM)
        .hover(|s| s.bg(palette::blurple()).text_color(gpui_kit::white()))
        .child(div().flex_1().min_w_0().truncate().child(label))
        .when(checked, |d| d.child(Icon::new(Glyph::Checkmark).size(px(16.)).text_color(theme.foreground)))
}

// A secondary call bar button: a 40px circle, white icon on a dark wash.
fn round_button(id: &'static str, glyph: Glyph, label: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(40.))
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .bg(gpui_kit::white().opacity(0.1))
        .text_color(gpui_kit::white())
        .hover(|s| s.bg(gpui_kit::white().opacity(0.16)))
        .child(Icon::new(glyph).size(px(20.)))
        .tooltip(move |window, cx| Tooltip::new(label).build(window, cx))
}

fn live_badge() -> Div {
    div().h(px(18.)).px_1().rounded(px(4.)).bg(rgb(0xED4245)).text_color(gpui_kit::white()).text_size(px(11.)).font_weight(FontWeight::BOLD).flex().items_center().child("LIVE")
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
