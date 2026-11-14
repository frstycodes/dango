// Everything around a voice call, after Discord: the voice panel over the user panel with its
// connection stages, the connection details popout, the call view with participant tiles and the
// control bar, device menus and the participant menu.
use gpui_kit::{
    AnyElement, Context, Div, FontWeight, Hsla, ImageSource, MouseButton, MouseDownEvent, Pixels, Point, SharedString, Stateful, Window,
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

pub const VOICE_HEIGHT: f32 = 92.;
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
    pub(crate) fn voice_panel_height(&self) -> f32 {
        match (&self.call, &self.voice_error) {
            (Some(_), _) => VOICE_HEIGHT,
            (None, Some(_)) => ERROR_HEIGHT,
            (None, None) => 0.,
        }
    }

    pub(crate) fn dismiss_call_menus(&mut self) -> bool {
        std::mem::take(&mut self.connection_info) | self.participant_menu.take().is_some() | self.device_menu.take().is_some()
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
        let color = match call.stage {
            Stage::Connected => palette::status(discord::Status::Online),
            Stage::NoRoute | Stage::Disconnected => theme.danger,
            _ => theme.warning,
        };
        let (glyph, _) = signal(call.stats.as_ref().map(|s| s.ping_ms));
        let place = match call.guild_id.as_ref().and_then(|id| self.guilds.iter().find(|g| &g.id == id)) {
            Some(guild) => format!("{} / {}", call.channel.name, guild.name),
            None => call.channel.name.clone(),
        };
        let channel = call.channel.clone();
        let status = div()
            .id("voice-status")
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .cursor_pointer()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .text_color(color)
                    .child(Icon::new(glyph).size(px(16.)))
                    .child(div().truncate().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).child(stage_text(call.stage))),
            )
            .child(
                div()
                    .id("voice-place")
                    .truncate()
                    .text_size(px(12.))
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
            .flex()
            .items_center()
            .gap_1()
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
            .child(wide_button("panel-share", Glyph::ShareScreen, if sharing { "Stop Streaming" } else { "Share Your Screen" }, sharing, connected, cx).on_click(cx.listener(|this, _, _, cx| this.toggle_screen_share(cx))));
        Some(div().h(px(VOICE_HEIGHT)).p_2().flex().flex_col().gap_2().border_b_1().border_color(theme.border).child(header).child(actions).into_any_element())
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
            return vec![VoiceState { guild_id: None, channel_id: Some(call.channel.id.clone()), user_id: me.id.clone(), user: Some(me.clone()), muted: self.muted, deafened: self.deafened, streaming: false, camera: false }];
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

    // The call view: a grid of tiles, or the watched stream large with everyone in a strip below,
    // then Discord's control bar.
    pub(crate) fn render_call(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let call = self.call.as_ref()?;
        let theme = cx.theme().clone();
        let people = self.participants();
        let viewport = window.viewport_size();
        let width = f32::from(viewport.width) - RAIL_WIDTH - self.channels_width - 48.;
        let height = f32::from(viewport.height) - 32. - 48. - 96.;
        let header = div()
            .h(px(48.))
            .flex_none()
            .px_4()
            .flex()
            .items_center()
            .gap_2()
            .text_color(gpui_kit::white())
            .child(Icon::new(Glyph::VolumeHigh).size(px(20.)).text_color(gpui_kit::white().opacity(0.7)))
            .child(div().font_weight(FontWeight::SEMIBOLD).child(call.channel.name.clone()))
            .child(div().text_size(px(14.)).text_color(gpui_kit::white().opacity(0.6)).child(format!("{} in voice", people.len())));
        let stage: AnyElement = match &call.watching {
            Some(watching) => {
                let strip_h = 120.;
                let main_h = (height - strip_h - 16.).max(120.);
                let main = match &watching.frame {
                    Some(frame) => {
                        let (w, h) = (frame.size(0).width.0 as f32, frame.size(0).height.0 as f32);
                        let scale = (width / w).min(main_h / h);
                        img(ImageSource::Render(frame.clone())).w(px(w * scale)).h(px(h * scale)).rounded(px(8.)).into_any_element()
                    }
                    None => div().text_color(gpui_kit::white().opacity(0.7)).child(format!("Joining {}'s stream…", self.user_name(&watching.streamer_id))).into_any_element(),
                };
                let tiles: Vec<AnyElement> = people.iter().map(|p| self.tile(p, strip_h * 16. / 9., strip_h, cx)).collect();
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_h_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(main)
                            .child(div().absolute().top_2().right_2().child(pill_button("stop-watching", "Stop Watching").on_click(cx.listener(|this, _, _, cx| this.stop_watching(cx))))),
                    )
                    .child(div().flex_none().flex().justify_center().gap_2().children(tiles))
                    .into_any_element()
            }
            None => {
                let n = people.len().max(1);
                let cols = (n as f32).sqrt().ceil() as usize;
                let rows = n.div_ceil(cols);
                let gap = 8.;
                let tile_w = ((width - gap * (cols as f32 - 1.)) / cols as f32).min((height - gap * (rows as f32 - 1.)) / rows as f32 * 16. / 9.).max(160.);
                let tiles: Vec<AnyElement> = people.iter().map(|p| self.tile(p, tile_w, tile_w * 9. / 16., cx)).collect();
                div().size_full().flex().flex_wrap().items_center().justify_center().content_center().gap(px(gap)).children(tiles).into_any_element()
            }
        };
        Some(
            div()
                .id("call-view")
                .size_full()
                .bg(gpui_kit::black())
                .flex()
                .flex_col()
                .child(header)
                .child(div().flex_1().min_h_0().px_6().child(stage))
                .child(self.call_controls(cx))
                .when(!theme.is_dark(), |d| d.border_t_1().border_color(theme.border))
                .into_any_element(),
        )
    }

    // A participant: camera if it's on, otherwise their avatar on a dark tile; green ring while
    // speaking; name and mute state bottom left; a LIVE tile action when they stream.
    fn tile(&self, state: &VoiceState, w: f32, h: f32, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let call = self.call.as_ref().expect("in a call");
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
        let talking = self.speaking(&state.user_id);
        let face: AnyElement = match call.cameras.get(&state.user_id) {
            Some(frame) => img(ImageSource::Render(frame.clone())).size_full().object_fit(gpui_kit::ObjectFit::Cover).into_any_element(),
            None => img(user.avatar.clone()).size(px((h * 0.4).clamp(40., 96.))).rounded_full().into_any_element(),
        };
        let watchable = state.streaming && !me && call.watching.as_ref().is_none_or(|w| w.streamer_id != state.user_id);
        let streamer = state.user_id.clone();
        let menu_user = state.user_id.clone();
        div()
            .id(SharedString::from(format!("tile-{}", state.user_id)))
            .group("tile")
            .relative()
            .w(px(w))
            .h(px(h))
            .flex_none()
            .rounded(px(8.))
            .overflow_hidden()
            .bg(rgb(0x1A1A1E))
            .flex()
            .items_center()
            .justify_center()
            .child(face)
            .child(div().absolute().inset_0().rounded(px(8.)).border_2().border_color(if talking { palette::status(discord::Status::Online) } else { gpui_kit::transparent_black() }))
            .child(
                div()
                    .absolute()
                    .left_2()
                    .bottom_2()
                    .max_w(px(w - 16.))
                    .h(px(24.))
                    .px_2()
                    .rounded(px(4.))
                    .bg(gpui_kit::black().opacity(0.5))
                    .text_color(gpui_kit::white())
                    .text_size(px(14.))
                    .flex()
                    .items_center()
                    .gap_1()
                    .when(muted, |d| d.child(Icon::new(Glyph::MicOff).size(px(14.)).text_color(theme.danger)))
                    .when(deafened, |d| d.child(Icon::new(Glyph::HeadphonesOff).size(px(14.)).text_color(theme.danger)))
                    .child(div().truncate().child(user.name.clone())),
            )
            .when(state.streaming, |d| d.child(div().absolute().top_2().left_2().h(px(18.)).px_1().rounded(px(4.)).bg(rgb(0xED4245)).text_color(gpui_kit::white()).text_size(px(11.)).font_weight(FontWeight::BOLD).flex().items_center().child("LIVE")))
            .when(watchable, |d| {
                d.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .invisible()
                        .group_hover("tile", |s| s.visible())
                        .child(pill_button(SharedString::from(format!("watch-{streamer}")), "Watch Stream").on_click(cx.listener(move |this, _, _, cx| this.watch_stream(streamer.clone(), cx)))),
                )
            })
            .on_mouse_down(MouseButton::Right, cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                this.participant_menu = Some((menu_user.clone(), e.position));
                cx.notify();
            }))
            .into_any_element()
    }

    // Discord's call bar: mic and camera with their device chevrons, share screen, then the red hang-up.
    fn call_controls(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let call = self.call.as_ref().expect("in a call");
        let muted = self.muted || self.deafened;
        let connected = call.stage == Stage::Connected;
        let sharing = call.sharing.is_some();
        div()
            .h(px(96.))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .gap_3()
            .child(
                split_button("call-mic", if muted { Glyph::MicOff } else { Glyph::Mic }, if muted { "Unmute" } else { "Mute" }, muted, cx)
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_mute(cx)))
                    .into_any_element(),
            )
            .child(chevron("call-mic-devices", cx).on_mouse_down(MouseButton::Left, cx.listener(|this, e: &MouseDownEvent, _, cx| this.open_device_menu(DeviceKind::Input, e.position, cx))))
            .child(
                split_button("call-camera", if call.camera_on { Glyph::Video } else { Glyph::VideoOff }, if call.camera_on { "Turn Off Camera" } else { "Turn On Camera" }, !call.camera_on, cx)
                    .when(!connected, |b| b.opacity(0.5))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_camera(cx)))
                    .into_any_element(),
            )
            .child(chevron("call-camera-devices", cx).on_mouse_down(MouseButton::Left, cx.listener(|this, e: &MouseDownEvent, _, cx| this.open_device_menu(DeviceKind::Camera, e.position, cx))))
            .child(
                split_button("call-share", if sharing { Glyph::ScreenOff } else { Glyph::ShareScreen }, if sharing { "Stop Streaming" } else { "Share Your Screen" }, false, cx)
                    .rounded(px(12.))
                    .when(sharing, |b| b.bg(palette::status(discord::Status::Online)))
                    .when(!connected, |b| b.opacity(0.5))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_screen_share(cx)))
                    .into_any_element(),
            )
            .child(
                div()
                    .id("call-hang-up")
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
        out.extend(self.participant_overlay(window, cx));
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

    // Right-click on someone in the call: profile, mention, message, their volume, local mute.
    fn participant_overlay(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (user_id, at) = self.participant_menu.clone()?;
        let theme = cx.theme().clone();
        let me = self.me.as_ref().is_some_and(|m| m.id == user_id);
        let user = self.guilds.iter().flat_map(|g| g.voice_states.iter()).chain(self.participants().iter()).find(|p| p.user_id == user_id).and_then(|p| p.user.clone());
        let volume = self.prefs.user_volumes.get(&user_id).copied().unwrap_or(1.);
        let locally_muted = self.prefs.local_mutes.contains(&user_id);
        let (vol_id, mute_id, dm_id, copy_id) = (user_id.clone(), user_id.clone(), user_id.clone(), user_id.clone());
        let profile_user = user.clone();
        let mention_user = user.clone();
        let viewport = window.viewport_size();
        let card = div()
            .id("participant-menu")
            .absolute()
            .left(px(f32::from(at.x).min(f32::from(viewport.width) - 236.)))
            .top(px(f32::from(at.y).min(f32::from(viewport.height) - 320.)))
            .w(px(220.))
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
            .children(profile_user.map(|user| {
                menu_row("participant-profile", "Profile".into(), false, cx).on_click(cx.listener(move |this, e: &gpui_kit::ClickEvent, _, cx| {
                    this.participant_menu = None;
                    this.open_profile(user.clone(), e.position(), cx);
                }))
            }))
            .children(mention_user.map(|user| {
                menu_row("participant-mention", "Mention".into(), false, cx).on_click(cx.listener(move |this, _, window, cx| {
                    this.participant_menu = None;
                    this.mention_user(&user, window, cx);
                }))
            }))
            .when(!me, |d| {
                d.child(menu_row("participant-message", "Message".into(), false, cx).on_click(cx.listener(move |this, _, window, cx| {
                    this.participant_menu = None;
                    this.message_user(dm_id.clone(), window, cx);
                })))
                .child(div().my_1().h(px(1.)).bg(theme.border))
                .child(
                    div()
                        .px_2()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(div().flex().justify_between().text_size(px(12.)).font_weight(FontWeight::BOLD).text_color(theme.muted_foreground).child("User Volume").child(format!("{:.0}%", volume * 100.)))
                        .child(self.range(SharedString::from(format!("user-volume-{vol_id}")), volume / 2., Fill::Accent, move |this, f, cx| this.set_user_volume(&vol_id, f.clamp(0., 1.) * 2., cx), cx)),
                )
                .child(menu_row("participant-mute", "Mute".into(), locally_muted, cx).on_click(cx.listener(move |this, _, _, cx| this.toggle_local_mute(&mute_id, cx))))
            })
            .child(div().my_1().h(px(1.)).bg(theme.border))
            .child(menu_row("participant-copy", "Copy User ID".into(), false, cx).on_click(cx.listener(move |this, _, _, cx| {
                this.participant_menu = None;
                this.copy_text(copy_id.clone(), cx);
            })));
        Some(call_backdrop("participant-backdrop", cx).child(card).into_any_element())
    }

    fn set_user_volume(&mut self, user_id: &str, volume: f32, cx: &mut Context<Self>) {
        if let Some(call) = &self.call {
            call.session.set_user_volume(user_id, volume);
        }
        let user_id = user_id.to_owned();
        self.update_prefs(cx, |p| {
            p.user_volumes.insert(user_id, volume);
        });
    }

    fn toggle_local_mute(&mut self, user_id: &str, cx: &mut Context<Self>) {
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

// The panel's row of equal-width call buttons; lit green while that thing is on.
fn wide_button(id: &'static str, glyph: Glyph, label: &'static str, on: bool, enabled: bool, cx: &mut Context<Rustcord>) -> Stateful<Div> {
    let theme = cx.theme().clone();
    let on_color = palette::status(discord::Status::Online);
    div()
        .id(id)
        .flex_1()
        .h(px(32.))
        .rounded(px(8.))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .bg(if on { on_color.opacity(0.2) } else { theme.foreground.opacity(0.08) })
        .text_color(if on { on_color } else { theme.foreground.opacity(0.85) })
        .hover(|s| s.bg(theme.foreground.opacity(0.14)))
        .when(!enabled, |b| b.opacity(0.5))
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
