// Discord voice and video: main-gateway handshake, voice gateway v8, UDP transport, DAVE E2EE,
// Opus audio with echo/noise/gain processing, H.264 cameras and Go Live (watching and, on macOS,
// streaming). Voice, watching a Go Live and the stream ops 19/20 were checked against Discord
// live; going live (main ops 18/22, voice op 12, soundshare speaking flag) and sending a camera
// are from memory of what official clients send — Not verified.
// `RUSTCORD_VOICE_DEBUG=1` logs gateway traffic and undecodable packets to stderr.

mod audio;
mod camera;
#[cfg(target_os = "macos")]
mod capture;
#[cfg(target_os = "windows")]
#[path = "capture_windows.rs"]
mod capture;
mod dave;
mod dsp;
mod media;
mod rtp;
mod video;
#[cfg(target_os = "windows")]
mod video_mf;

use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use futures::{SinkExt, StreamExt, channel::mpsc, channel::oneshot};
use serde_json::{Value, json};
use tokio::sync::broadcast;
use tokio_tungstenite::tungstenite::Message as WsMessage;

pub use audio::{Device, input_devices, output_devices};
pub use camera::cameras;
pub use video::VideoFrame;

const SERVER_TIMEOUT: Duration = Duration::from_secs(10);
const STATS_EVERY: Duration = Duration::from_secs(2);
const PINGS_KEPT: usize = 20;

pub enum VoiceEvent {
    Connected,
    Stage(Stage),
    Stats(Stats),
    // Our mic level after processing, about 25 times a second while in a call.
    InputLevel { db: f32, gate_open: bool },
    Speaking { user_id: String, speaking: bool },
    // A user in our channel turned Go Live or their camera on/off. Users already live when we
    // joined are in GUILD_CREATE `voice_states[].self_stream` / `self_video`, which the UI owns.
    VideoState { user_id: String, streaming: bool, camera: bool },
    // Remote cameras and Go Lives, and our own camera (our user id) for the self-preview.
    StreamFrame { user_id: String, source: VideoSource, frame: Arc<VideoFrame> },
    StreamEnded { user_id: String, reason: String },
    Disconnected(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VideoSource {
    Camera,
    GoLive,
}

// Connection progress. Discord's desktop voice runs over plain UDP, not WebRTC, so the ICE and
// DTLS stages stand for their UDP counterparts: IceChecking = IP discovery, DtlsConnecting =
// waiting for the encryption key after choosing a cipher.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    AwaitingEndpoint,
    Authenticating,
    RtcConnecting,
    IceChecking,
    DtlsConnecting,
    Connected,
    NoRoute,
    Disconnected,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stats {
    pub ping_ms: u32,
    pub avg_ping_ms: u32,
    pub last_pings: Vec<u32>,
    pub outbound_loss_pct: f32,
    pub inbound_loss_pct: f32,
    pub endpoint: String,
    pub server_ip: String,
    pub encryption: String,
    pub dave: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum InputMode {
    // `auto` learns the background noise; otherwise the mic opens above `threshold_db` (-100..0).
    VoiceActivity { auto: bool, threshold_db: f32 },
    // Transmits while the talk key is held, plus `release_delay_ms` after release.
    PushToTalk { release_delay_ms: u32 },
}

// Everything that shapes our audio, applied from the first packet. All of it can also be changed
// live on the session.
#[derive(Clone, Debug, PartialEq)]
pub struct VoiceSettings {
    // Device ids from `input_devices` / `output_devices`; None = system default.
    pub input_device: Option<String>,
    pub output_device: Option<String>,
    // 0..=2.0
    pub input_volume: f32,
    pub output_volume: f32,
    pub input_mode: InputMode,
    pub echo_cancellation: bool,
    pub noise_suppression: bool,
    pub auto_gain: bool,
}

impl Default for VoiceSettings {
    fn default() -> Self {
        Self {
            input_device: None,
            output_device: None,
            input_volume: 1.0,
            output_volume: 1.0,
            input_mode: InputMode::VoiceActivity { auto: true, threshold_db: -60.0 },
            echo_cancellation: true,
            noise_suppression: true,
            auto_gain: true,
        }
    }
}

impl VoiceSettings {
    fn audio(&self) -> audio::AudioSettings {
        let processor = dsp::Processor::new(self.echo_cancellation, self.noise_suppression, self.auto_gain);
        audio::AudioSettings::new(self.input_device.clone(), self.output_device.clone(), self.input_volume, self.output_volume, processor)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GoLiveOptions {
    // Stream height: 480, 720 or 1080; 0 = the source's own size.
    pub resolution: u32,
    // 15, 30 or 60.
    pub fps: u32,
    // Send the shared app's / system's sound.
    pub audio: bool,
}

impl Default for GoLiveOptions {
    fn default() -> Self {
        Self { resolution: 720, fps: 30, audio: true }
    }
}

// What a voice-gateway connection is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    // The channel itself: our mic and camera out, everyone's audio and cameras in.
    Voice,
    // Someone else's Go Live: their screen and its audio in.
    Watch,
    // Our Go Live: our screen (or one app / window) out.
    Broadcast(ShareSource, GoLiveOptions),
}

impl Role {
    fn video_source(self) -> VideoSource {
        match self {
            Role::Voice => VideoSource::Camera,
            Role::Watch | Role::Broadcast(..) => VideoSource::GoLive,
        }
    }

    fn broadcasts(self) -> bool {
        matches!(self, Role::Broadcast(..))
    }
}

// What a Go Live shares. Audio follows the source, as in Discord: the whole screen sends all
// system audio except Rustcord's own; an app or window sends only that app's sound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShareSource {
    Screen,
    // Every window of the app with this process id.
    App(i32),
    // One window, by `ShareableWindow::id`.
    Window(u32),
}

// A window that can be shared, for the UI's picker. `pid` doubles as `ShareSource::App`.
#[derive(Clone, Debug)]
pub struct ShareableWindow {
    pub id: u32,
    pub title: String,
    pub app_name: String,
    pub pid: i32,
}

// Visible, titled windows of other apps (macOS and Windows); Err elsewhere.
pub fn shareable_windows() -> Result<Vec<ShareableWindow>, String> {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    return capture::shareable_windows();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    Err("Screen sharing is only implemented on macOS and Windows".into())
}

// The voice module's view of the main gateway. `src/discord.rs` writes every string from the
// receiver returned by `new` to its socket and feeds VOICE_* / STREAM_* dispatches into `dispatch`.
#[derive(Clone)]
pub struct GatewayHandle {
    outbound: mpsc::UnboundedSender<String>,
    dispatches: broadcast::Sender<(String, Value)>,
}

impl GatewayHandle {
    pub fn new() -> (Self, mpsc::UnboundedReceiver<String>) {
        let (outbound, rx) = mpsc::unbounded();
        (Self { outbound, dispatches: broadcast::channel(64).0 }, rx)
    }

    pub fn dispatch(&self, kind: &str, d: &Value) {
        if kind.starts_with("VOICE_") || kind.starts_with("STREAM_") {
            if debug() {
                let brief = json!({ "user": d["user_id"], "session": d["session_id"], "channel": d["channel_id"], "stream": d["self_stream"], "key": d["stream_key"] });
                eprintln!("main {kind}: {brief}");
            }
            let _ = self.dispatches.send((kind.to_owned(), d.clone()));
        }
    }

    fn send(&self, op: u8, d: Value) {
        let _ = self.outbound.unbounded_send(json!({ "op": op, "d": d }).to_string());
    }
}

pub struct VoiceSession {
    shared: Arc<Shared>,
    // Dropping this sender is the leave signal.
    _leave: oneshot::Sender<()>,
}

// Dropping it stops watching.
pub struct StreamWatch {
    _stop: oneshot::Sender<()>,
}

// Our Go Live; dropping it ends the stream.
pub struct GoLive {
    _stop: oneshot::Sender<()>,
}

struct Shared {
    gateway: GatewayHandle,
    user_id: String,
    guild_id: Option<String>,
    channel_id: String,
    events: mpsc::UnboundedSender<VoiceEvent>,
    session_id: Mutex<Option<String>>,
    muted: AtomicBool,
    deafened: AtomicBool,
    talk_key_held: AtomicBool,
    input_mode: Mutex<InputMode>,
    audio: Arc<audio::AudioSettings>,
    // Everything we play, from the voice connection and any stream we watch.
    mixer: Arc<audio::Mixer>,
    // Local per-user listening volume (0..=2) and mute, by user id.
    listening: Mutex<HashMap<String, (f32, bool)>>,
    // The camera the user wants on (None = off); the voice connection follows it.
    camera: Mutex<Option<String>>,
}

impl Shared {
    fn send_voice_state(&self, in_channel: bool) {
        let camera_on = self.camera.lock().is_ok_and(|c| c.is_some());
        self.gateway.send(
            4,
            json!({
                "guild_id": self.guild_id,
                "channel_id": in_channel.then_some(&self.channel_id),
                "self_mute": self.muted.load(Ordering::Relaxed),
                "self_deaf": self.deafened.load(Ordering::Relaxed),
                "self_video": in_channel && camera_on,
            }),
        );
    }

    fn may_transmit(&self) -> bool {
        !self.muted.load(Ordering::Relaxed) && !self.deafened.load(Ordering::Relaxed)
    }

    fn input_mode(&self) -> InputMode {
        self.input_mode.lock().map(|m| *m).unwrap_or(InputMode::VoiceActivity { auto: true, threshold_db: -60.0 })
    }

    // The gain to play `user_id` at: their local volume, 0 when locally muted or deafened.
    fn listening_gain(&self, user_id: &str) -> f32 {
        if self.deafened.load(Ordering::Relaxed) {
            return 0.0;
        }
        let local = self.listening.lock().ok().and_then(|l| l.get(user_id).copied());
        match local {
            Some((_, true)) => 0.0,
            Some((volume, false)) => volume,
            None => 1.0,
        }
    }

    fn server_id(&self) -> &str {
        self.guild_id.as_deref().unwrap_or(&self.channel_id)
    }

    fn emit(&self, event: VoiceEvent) {
        let _ = self.events.unbounded_send(event);
    }
}

// `guild_id` is None for DM / group calls. Safe to call from any thread; the work runs on
// `crate::discord::runtime()`.
pub fn join(
    user_id: String,
    guild_id: Option<String>,
    channel_id: String,
    gateway: GatewayHandle,
    settings: VoiceSettings,
) -> (VoiceSession, mpsc::UnboundedReceiver<VoiceEvent>) {
    let (events, rx) = mpsc::unbounded();
    let shared = Arc::new(Shared {
        gateway,
        user_id,
        guild_id,
        channel_id,
        events,
        session_id: Mutex::new(None),
        muted: AtomicBool::new(false),
        deafened: AtomicBool::new(false),
        talk_key_held: AtomicBool::new(false),
        input_mode: Mutex::new(settings.input_mode),
        audio: Arc::new(settings.audio()),
        mixer: Arc::default(),
        listening: Mutex::new(HashMap::new()),
        camera: Mutex::new(None),
    });
    let (leave_tx, leave_rx) = oneshot::channel();
    crate::discord::runtime().spawn(run(shared.clone(), leave_rx));
    (VoiceSession { shared, _leave: leave_tx }, rx)
}

impl VoiceSession {
    pub fn set_muted(&self, muted: bool) {
        self.shared.muted.store(muted, Ordering::Relaxed);
        self.shared.send_voice_state(true);
    }

    pub fn set_deafened(&self, deafened: bool) {
        self.shared.deafened.store(deafened, Ordering::Relaxed);
        self.shared.send_voice_state(true);
    }

    // Device ids from `input_devices` / `output_devices`; None = system default. Switches live.
    pub fn set_input_device(&self, id: Option<String>) {
        if let Ok(mut device) = self.shared.audio.input_device.lock() {
            *device = id;
        }
    }

    pub fn set_output_device(&self, id: Option<String>) {
        if let Ok(mut device) = self.shared.audio.output_device.lock() {
            *device = id;
        }
    }

    // 0..=2.0
    pub fn set_input_volume(&self, volume: f32) {
        self.shared.audio.set_input_volume(volume);
    }

    pub fn set_output_volume(&self, volume: f32) {
        self.shared.audio.set_output_volume(volume);
    }

    // How loud `user_id` plays for us, 0..=2.0. Local only.
    pub fn set_user_volume(&self, user_id: &str, volume: f32) {
        if let Ok(mut listening) = self.shared.listening.lock() {
            listening.entry(user_id.to_owned()).or_insert((1.0, false)).0 = volume.clamp(0.0, audio::MAX_VOLUME);
        }
    }

    // Stop hearing `user_id`. Local only.
    pub fn set_user_muted(&self, user_id: &str, muted: bool) {
        if let Ok(mut listening) = self.shared.listening.lock() {
            listening.entry(user_id.to_owned()).or_insert((1.0, false)).1 = muted;
        }
    }

    pub fn set_input_mode(&self, mode: InputMode) {
        if let Ok(mut current) = self.shared.input_mode.lock() {
            *current = mode;
        }
    }

    // The push-to-talk key; only matters in `InputMode::PushToTalk`.
    pub fn set_talk_key(&self, held: bool) {
        self.shared.talk_key_held.store(held, Ordering::Relaxed);
    }

    pub fn set_echo_cancellation(&self, on: bool) {
        self.shared.audio.processor.set(Some(on), None, None);
    }

    pub fn set_noise_suppression(&self, on: bool) {
        self.shared.audio.processor.set(None, Some(on), None);
    }

    pub fn set_auto_gain(&self, on: bool) {
        self.shared.audio.processor.set(None, None, Some(on));
    }

    // Camera id from `cameras()`; None turns the camera off. Our own frames come back as
    // `StreamFrame { source: Camera }` with our user id.
    pub fn set_camera(&self, id: Option<String>) -> Result<(), String> {
        if let Some(id) = &id
            && !cameras().iter().any(|c| &c.id == id)
        {
            return Err("That camera is not connected".into());
        }
        if let Ok(mut camera) = self.shared.camera.lock() {
            *camera = id;
        }
        self.shared.send_voice_state(true);
        Ok(())
    }

    // Watch `streamer_id`'s Go Live. Frames arrive as `StreamFrame { source: GoLive, .. }` on this
    // session's event stream, and `StreamEnded` when it stops. Needs `Connected` first.
    pub fn watch_stream(&self, streamer_id: &str) -> Result<StreamWatch, String> {
        Ok(StreamWatch { _stop: self.start_stream(streamer_id, Role::Watch)? })
    }

    // Share the main display (macOS) at 720p30 with sound. Our own `StreamEnded` reports when it stops.
    pub fn go_live(&self) -> Result<GoLive, String> {
        self.go_live_with_options(ShareSource::Screen, GoLiveOptions::default())
    }

    // Share one app or window, with only its audio; see `shareable_windows` for the choices.
    pub fn go_live_with(&self, source: ShareSource) -> Result<GoLive, String> {
        self.go_live_with_options(source, GoLiveOptions::default())
    }

    pub fn go_live_with_options(&self, source: ShareSource, options: GoLiveOptions) -> Result<GoLive, String> {
        Ok(GoLive { _stop: self.start_stream(&self.shared.user_id, Role::Broadcast(source, options))? })
    }

    fn start_stream(&self, streamer_id: &str, role: Role) -> Result<oneshot::Sender<()>, String> {
        let session_id = self.shared.session_id.lock().ok().and_then(|id| id.clone()).ok_or("Not connected to voice yet")?;
        let key = match &self.shared.guild_id {
            Some(guild) => format!("guild:{guild}:{}:{streamer_id}", self.shared.channel_id),
            None => format!("call:{}:{streamer_id}", self.shared.channel_id),
        };
        let (stop_tx, stop_rx) = oneshot::channel();
        crate::discord::runtime().spawn(stream(self.shared.clone(), role, streamer_id.to_owned(), key, session_id, stop_rx));
        Ok(stop_tx)
    }

    pub fn leave(self) {}
}

// Discord's "Let's Check": plays the processed mic back and reports its level. Drop to stop.
pub struct MicTest {
    stop: Arc<AtomicBool>,
    _engine: audio::Engine,
}

impl Drop for MicTest {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

// Works outside a call. `device` overrides `settings.input_device`. Levels (dBFS) arrive about
// 25 times a second.
pub fn mic_test(device: Option<String>, settings: &VoiceSettings) -> Result<(MicTest, mpsc::UnboundedReceiver<f32>), String> {
    let settings = VoiceSettings { input_device: device.or_else(|| settings.input_device.clone()), ..settings.clone() };
    let audio = Arc::new(settings.audio());
    let mixer = Arc::new(audio::Mixer::default());
    let (engine, pcm) = audio::start(audio.clone(), mixer.clone(), true)?;
    let (levels_tx, levels) = mpsc::unbounded();
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    thread::Builder::new()
        .name("rustcord-mic-test".into())
        .spawn(move || media::mic_test_loop(&audio, &mixer, &pcm, &levels_tx, &flag))
        .map_err(|e| e.to_string())?;
    Ok((MicTest { stop, _engine: engine }, levels))
}

async fn run(shared: Arc<Shared>, leave: oneshot::Receiver<()>) {
    let mut dispatches = shared.gateway.dispatches.subscribe();
    shared.send_voice_state(true);
    shared.emit(VoiceEvent::Stage(Stage::AwaitingEndpoint));
    let reason = tokio::select! {
        result = join_voice(&shared, &mut dispatches) => result.err().unwrap_or_else(|| "Voice connection closed".to_owned()),
        _ = leave => "Left".to_owned(),
    };
    shared.send_voice_state(false);
    shared.emit(VoiceEvent::Stage(Stage::Disconnected));
    shared.emit(VoiceEvent::Disconnected(reason));
}

async fn join_voice(shared: &Arc<Shared>, dispatches: &mut broadcast::Receiver<(String, Value)>) -> Result<(), String> {
    let (session_id, server) = tokio::time::timeout(SERVER_TIMEOUT, await_voice_server(shared, dispatches))
        .await
        .map_err(|_| "Voice server did not respond".to_owned())??;
    if let Ok(mut id) = shared.session_id.lock() {
        *id = Some(session_id.clone());
    }
    let target = Target {
        server_id: shared.server_id().to_owned(),
        dave_channel: shared.channel_id.clone(),
        session_id,
        server,
        role: Role::Voice,
    };
    serve(shared, target, dispatches, |kind, d| voice_dispatch(shared, kind, d)).await
}

// Follows our channel's voice states while connected; ends the connection if we were moved out.
fn voice_dispatch(shared: &Shared, kind: &str, d: &Value) -> Option<String> {
    if kind != "VOICE_STATE_UPDATE" {
        return None;
    }
    let user_id = d["user_id"].as_str()?;
    let in_channel = d["channel_id"].as_str() == Some(&shared.channel_id);
    if user_id == shared.user_id && !in_channel {
        return Some("Removed from the voice channel".into());
    }
    let streaming = in_channel && d["self_stream"].as_bool().unwrap_or(false);
    let camera = in_channel && d["self_video"].as_bool().unwrap_or(false);
    shared.emit(VoiceEvent::VideoState { user_id: user_id.to_owned(), streaming, camera });
    None
}

#[derive(Clone)]
struct Server {
    token: String,
    endpoint: String,
}

struct Target {
    server_id: String,
    // DAVE's MLS group id: the voice channel, or a Go Live's `rtc_channel_id`.
    dave_channel: String,
    session_id: String,
    server: Server,
    role: Role,
}

async fn await_voice_server(shared: &Shared, dispatches: &mut broadcast::Receiver<(String, Value)>) -> Result<(String, Server), String> {
    let (mut session_id, mut server) = (None, None);
    loop {
        let (kind, d) = dispatches.recv().await.map_err(|e| e.to_string())?;
        let for_us = d["guild_id"].as_str().or(d["channel_id"].as_str()) == Some(shared.server_id());
        match kind.as_str() {
            "VOICE_STATE_UPDATE" if d["user_id"].as_str() == Some(&shared.user_id) && for_us => {
                session_id = d["session_id"].as_str().map(str::to_owned);
            }
            "VOICE_SERVER_UPDATE" if for_us => server = parse_server(&d).or(server),
            _ => {}
        }
        if let (Some(id), Some(server)) = (&session_id, &server) {
            return Ok((id.clone(), server.clone()));
        }
    }
}

// A null endpoint means the server is being reallocated; another update follows.
fn parse_server(d: &Value) -> Option<Server> {
    Some(Server { token: d["token"].as_str()?.to_owned(), endpoint: d["endpoint"].as_str()?.to_owned() })
}

// Main-gateway stream ops: 18 create (go live), 19 delete (stop streaming or watching),
// 20 watch, 22 set paused.
async fn stream(shared: Arc<Shared>, role: Role, streamer: String, key: String, session_id: String, stop: oneshot::Receiver<()>) {
    let mut dispatches = shared.gateway.dispatches.subscribe();
    if role.broadcasts() {
        let kind = if shared.guild_id.is_some() { "guild" } else { "call" };
        let create = json!({ "type": kind, "guild_id": shared.guild_id, "channel_id": shared.channel_id, "preferred_region": null });
        shared.gateway.send(18, create);
    } else {
        shared.gateway.send(20, json!({ "stream_key": key }));
    }
    let reason = tokio::select! {
        result = connect_stream(&shared, role, &key, session_id, &mut dispatches) => result.err().unwrap_or_else(|| "Stream connection closed".to_owned()),
        _ = stop => "Stopped".to_owned(),
    };
    shared.gateway.send(19, json!({ "stream_key": key }));
    shared.emit(VoiceEvent::StreamEnded { user_id: streamer, reason });
}

async fn connect_stream(
    shared: &Arc<Shared>,
    role: Role,
    key: &str,
    session_id: String,
    dispatches: &mut broadcast::Receiver<(String, Value)>,
) -> Result<(), String> {
    let (rtc, server) = tokio::time::timeout(SERVER_TIMEOUT, await_stream_server(key, dispatches))
        .await
        .map_err(|_| "Stream server did not respond".to_owned())??;
    if role.broadcasts() {
        shared.gateway.send(22, json!({ "stream_key": key, "paused": false }));
    }
    let target = Target { server_id: rtc.server_id, dave_channel: rtc.channel_id, session_id, server, role };
    serve(shared, target, dispatches, |kind, d| {
        let ended = kind == "STREAM_DELETE" && d["stream_key"].as_str() == Some(key);
        ended.then(|| d["reason"].as_str().unwrap_or("Stream ended").to_owned())
    })
    .await
}

#[derive(Clone)]
struct RtcIds {
    server_id: String,
    channel_id: String,
}

async fn await_stream_server(key: &str, dispatches: &mut broadcast::Receiver<(String, Value)>) -> Result<(RtcIds, Server), String> {
    let (mut rtc, mut server) = (None, None);
    loop {
        let (kind, d) = dispatches.recv().await.map_err(|e| e.to_string())?;
        if d["stream_key"].as_str() != Some(key) {
            continue;
        }
        match kind.as_str() {
            "STREAM_CREATE" => {
                if let (Some(server_id), Some(channel_id)) = (d["rtc_server_id"].as_str(), d["rtc_channel_id"].as_str()) {
                    rtc = Some(RtcIds { server_id: server_id.to_owned(), channel_id: channel_id.to_owned() });
                }
            }
            "STREAM_SERVER_UPDATE" => server = parse_server(&d).or(server),
            "STREAM_DELETE" => return Err(d["reason"].as_str().unwrap_or("Stream unavailable").to_owned()),
            _ => {}
        }
        if let (Some(rtc), Some(server)) = (&rtc, &server) {
            return Ok((rtc.clone(), server.clone()));
        }
    }
}

// Round-trip times from heartbeat acks.
#[derive(Default)]
struct Pings(VecDeque<u32>);

impl Pings {
    fn record(&mut self, sent_ms: i64) {
        let rtt = (chrono::Utc::now().timestamp_millis() - sent_ms).clamp(0, u32::MAX as i64) as u32;
        self.0.push_back(rtt);
        if self.0.len() > PINGS_KEPT {
            self.0.pop_front();
        }
    }

    fn stats(&self) -> (u32, u32, Vec<u32>) {
        let last = self.0.back().copied().unwrap_or(0);
        let avg = (self.0.iter().map(|&p| p as u64).sum::<u64>() / self.0.len().max(1) as u64) as u32;
        (last, avg, self.0.iter().copied().collect())
    }
}

// One voice-gateway connection: the channel's own (audio + cameras) or a Go Live stream's.
async fn serve(
    shared: &Arc<Shared>,
    target: Target,
    dispatches: &mut broadcast::Receiver<(String, Value)>,
    on_dispatch: impl Fn(&str, &Value) -> Option<String>,
) -> Result<(), String> {
    let is_voice = target.role == Role::Voice;
    let stage = |stage| {
        if is_voice {
            shared.emit(VoiceEvent::Stage(stage));
        }
    };
    let host = target.server.endpoint.trim_start_matches("wss://").trim_end_matches('/').to_owned();
    let (ws, _) = tokio_tungstenite::connect_async(format!("wss://{host}/?v=8")).await.map_err(|e| e.to_string())?;
    let (mut sink, mut stream) = ws.split();
    let (out, mut out_rx) = mpsc::unbounded::<WsMessage>();

    let my_id: u64 = shared.user_id.parse().map_err(|_| "Invalid user id".to_owned())?;
    let channel: u64 = target.dave_channel.parse().map_err(|_| "Invalid channel id".to_owned())?;
    let mut dave = dave::Dave::new(my_id, channel, out.clone())?;
    let mut media: Option<media::Media> = None;
    let users = media::Users::default();
    let counters = Arc::new(media::Counters::default());
    let mut ready: Option<(Ssrcs, std::net::UdpSocket)> = None;
    let mut seq_ack: i64 = -1;
    let mut heartbeat = tokio::time::interval(Duration::from_secs(3600));
    heartbeat.tick().await;
    let mut stats_tick = tokio::time::interval(STATS_EVERY);
    let mut pings = Pings::default();
    let mut stats = Stats { endpoint: host.clone(), ..Stats::default() };

    let identify = json!({ "op": 0, "d": {
        "server_id": target.server_id,
        "user_id": shared.user_id,
        "session_id": target.session_id,
        "token": target.server.token,
        "video": true,
        "streams": [{ "type": if target.role.broadcasts() { "screen" } else { "video" }, "rid": "100", "quality": 100 }],
        "max_dave_protocol_version": davey::DAVE_PROTOCOL_VERSION,
    }});
    sink.send(WsMessage::Text(identify.to_string())).await.map_err(|e| e.to_string())?;
    stage(Stage::Authenticating);

    loop {
        let frame = tokio::select! {
            _ = heartbeat.tick() => {
                let nonce = chrono::Utc::now().timestamp_millis();
                let beat = json!({ "op": 3, "d": { "t": nonce, "seq_ack": seq_ack } });
                sink.send(WsMessage::Text(beat.to_string())).await.map_err(|e| e.to_string())?;
                continue;
            }
            _ = stats_tick.tick(), if is_voice && media.is_some() => {
                (stats.ping_ms, stats.avg_ping_ms, stats.last_pings) = pings.stats();
                (stats.inbound_loss_pct, stats.outbound_loss_pct) = counters.take_loss();
                stats.dave = dave.e2ee().load(Ordering::Relaxed);
                shared.emit(VoiceEvent::Stats(stats.clone()));
                continue;
            }
            Some(msg) = out_rx.next() => {
                sink.send(msg).await.map_err(|e| e.to_string())?;
                continue;
            }
            Ok((kind, d)) = dispatches.recv() => match on_dispatch(&kind, &d) {
                Some(reason) => return Err(reason),
                None => continue,
            },
            frame = stream.next() => match frame {
                None => return Ok(()),
                Some(frame) => frame.map_err(|e| e.to_string())?,
            },
        };

        let payload = match frame {
            WsMessage::Binary(bytes) => {
                let Some((seq, op, body)) = dave::parse_binary(&bytes) else { continue };
                if debug() {
                    eprintln!("[{:?}] voice binary op {op} ({} bytes)", target.role, body.len());
                }
                seq_ack = seq as i64;
                dave.handle_binary(op, body);
                continue;
            }
            WsMessage::Text(text) => serde_json::from_str::<Value>(&text).map_err(|e| e.to_string())?,
            WsMessage::Close(close) => {
                return Err(close.map(|c| format!("Voice gateway closed: {} {}", u16::from(c.code), c.reason)).unwrap_or_default());
            }
            _ => continue,
        };
        if let Some(seq) = payload["seq"].as_i64() {
            seq_ack = seq;
        }
        let d = &payload["d"];
        if debug() && payload["op"] != 6 {
            eprintln!("[{:?}] voice op {}: {:.300}", target.role, payload["op"], d.to_string());
        }
        match payload["op"].as_u64() {
            Some(8) => {
                let ms = d["heartbeat_interval"].as_f64().unwrap_or(13_750.0) as u64;
                heartbeat = tokio::time::interval(Duration::from_millis(ms));
                heartbeat.tick().await;
            }
            Some(6) => {
                if let Some(sent) = d["t"].as_i64() {
                    pings.record(sent);
                }
            }
            Some(2) => {
                stage(Stage::RtcConnecting);
                let ssrc = d["ssrc"].as_u64().ok_or("Voice ready without ssrc")? as u32;
                let ip = d["ip"].as_str().ok_or("Voice ready without ip")?.to_owned();
                let port = d["port"].as_u64().ok_or("Voice ready without port")? as u16;
                stats.server_ip = format!("{ip}:{port}");
                let modes: Vec<&str> = d["modes"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
                let mode = rtp::MODES.into_iter().find(|m| modes.contains(m)).ok_or("No supported encryption mode")?;
                stage(Stage::IceChecking);
                let discovered = tokio::task::spawn_blocking(move || media::discover(&ip, port, ssrc)).await.map_err(|e| e.to_string())?;
                let (socket, (my_ip, my_port)) = discovered.inspect_err(|_| stage(Stage::NoRoute))?;
                let video = &d["streams"][0];
                let ssrcs = Ssrcs {
                    audio: ssrc,
                    video: video["ssrc"].as_u64().unwrap_or(0) as u32,
                    rtx: video["rtx_ssrc"].as_u64().unwrap_or(0) as u32,
                };
                ready = Some((ssrcs, socket));
                sink.send(WsMessage::Text(select_protocol(&my_ip, my_port, mode).to_string())).await.map_err(|e| e.to_string())?;
                stage(Stage::DtlsConnecting);
            }
            Some(4) => {
                let (ssrcs, socket) = ready.take().ok_or("Session description before ready")?;
                let key: Vec<u8> = d["secret_key"].as_array().into_iter().flatten().filter_map(|b| b.as_u64().map(|b| b as u8)).collect();
                let mode = d["mode"].as_str().unwrap_or_default();
                stats.encryption = mode.to_owned();
                let cipher = rtp::Cipher::new(mode, &key).ok_or("Server picked an unsupported mode")?;
                dave.set_protocol_version(d["dave_protocol_version"].as_u64().unwrap_or(0) as u16);
                if let Role::Broadcast(_, options) = target.role {
                    sink.send(WsMessage::Text(announce_video(&ssrcs, VideoKind::Screen(options), true).to_string())).await.map_err(|e| e.to_string())?;
                }
                media = Some(media::Media::start(
                    media::Link {
                        socket,
                        ssrc: ssrcs.audio,
                        video_ssrc: ssrcs.video,
                        rtx_ssrc: ssrcs.rtx,
                        cipher,
                        dave: dave.session(),
                        e2ee: dave.e2ee(),
                        shared: shared.clone(),
                        role: target.role,
                        my_id: shared.user_id.clone(),
                        events: shared.events.clone(),
                        ws: out.clone(),
                        nonce: Default::default(),
                        keyframe: AtomicBool::new(true),
                        counters: counters.clone(),
                    },
                    users.clone(),
                )?);
                // Not verified: op 15 (media sink wants) asks the SFU to forward video at full quality.
                sink.send(WsMessage::Text(json!({ "op": 15, "d": { "any": 100 } }).to_string())).await.map_err(|e| e.to_string())?;
                if is_voice {
                    stage(Stage::Connected);
                    shared.emit(VoiceEvent::Connected);
                }
            }
            Some(5) => {
                if let (Some(user), Some(ssrc)) = (d["user_id"].as_str(), d["ssrc"].as_u64()) {
                    media::map_ssrc(&users, ssrc as u32, user);
                }
            }
            Some(12) => {
                if let Some(user) = d["user_id"].as_str() {
                    video_ssrcs(d).for_each(|ssrc| media::map_ssrc(&users, ssrc, user));
                }
            }
            Some(11) => dave.users_joined(d["user_ids"].as_array().into_iter().flatten().filter_map(Value::as_str)),
            Some(13) => {
                if let Some(user) = d["user_id"].as_str() {
                    dave.user_left(user);
                    if let Some(media) = &media {
                        media.user_left(user);
                    }
                }
            }
            Some(op @ (21 | 22 | 24)) => dave.handle_json(op, d),
            _ => {}
        }
    }
}

// RUSTCORD_VOICE_DEBUG=1 logs voice-gateway traffic and unroutable UDP packets to stderr.
pub(crate) fn debug() -> bool {
    static DEBUG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *DEBUG.get_or_init(|| std::env::var_os("RUSTCORD_VOICE_DEBUG").is_some())
}

struct Ssrcs {
    audio: u32,
    video: u32,
    rtx: u32,
}

// What our outgoing video is, for op 12.
#[derive(Clone, Copy)]
enum VideoKind {
    Camera,
    Screen(GoLiveOptions),
}

// Op 12 tells the SFU and viewers which SSRC carries our video and its limits. Shape follows
// what official clients send — Not verified.
fn announce_video(ssrcs: &Ssrcs, kind: VideoKind, active: bool) -> Value {
    let (kind, (width, height), fps) = match kind {
        VideoKind::Camera => ("video", media::CAMERA_SIZE, media::CAMERA_FPS),
        VideoKind::Screen(options) => ("screen", media::go_live_size(options.resolution), options.fps),
    };
    json!({ "op": 12, "d": {
        "audio_ssrc": ssrcs.audio,
        "video_ssrc": if active { ssrcs.video } else { 0 },
        "rtx_ssrc": if active { ssrcs.rtx } else { 0 },
        "streams": [{
            "type": kind, "rid": "100", "ssrc": ssrcs.video, "rtx_ssrc": ssrcs.rtx, "active": active, "quality": 100,
            "max_bitrate": media::video_bitrate(width, height, fps), "max_framerate": fps,
            "max_resolution": { "type": "fixed", "width": width, "height": height },
        }],
    }})
}

// We decode only H.264, so it is the only video codec offered.
fn select_protocol(address: &str, port: u16, mode: &str) -> Value {
    json!({ "op": 1, "d": {
        "protocol": "udp",
        "data": { "address": address, "port": port, "mode": mode },
        "codecs": [
            { "name": "opus", "type": "audio", "priority": 1000, "payload_type": rtp::OPUS_PAYLOAD_TYPE },
            { "name": "H264", "type": "video", "priority": 1000, "payload_type": rtp::H264_PAYLOAD_TYPE,
              "rtx_payload_type": rtp::H264_RTX_PAYLOAD_TYPE, "encode": true, "decode": true },
        ],
    }})
}

// Op 12 names a user's audio, video and per-stream SSRCs; 0 means "none".
fn video_ssrcs(d: &Value) -> impl Iterator<Item = u32> + '_ {
    let streams = d["streams"].as_array().into_iter().flatten().map(|s| &s["ssrc"]);
    [&d["audio_ssrc"], &d["video_ssrc"]]
        .into_iter()
        .chain(streams)
        .filter_map(Value::as_u64)
        .filter(|&ssrc| ssrc != 0)
        .map(|ssrc| ssrc as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shared() -> (Shared, mpsc::UnboundedReceiver<VoiceEvent>) {
        let (gateway, _out) = GatewayHandle::new();
        let (events, rx) = mpsc::unbounded();
        let shared = Shared {
            gateway,
            user_id: "me".into(),
            guild_id: Some("g".into()),
            channel_id: "c".into(),
            events,
            session_id: Mutex::new(None),
            muted: AtomicBool::new(false),
            deafened: AtomicBool::new(false),
            talk_key_held: AtomicBool::new(false),
            input_mode: Mutex::new(VoiceSettings::default().input_mode),
            audio: Arc::new(VoiceSettings::default().audio()),
            mixer: Arc::default(),
            listening: Mutex::new(HashMap::new()),
            camera: Mutex::new(None),
        };
        (shared, rx)
    }

    #[test]
    fn op12_ssrcs_skip_zero() {
        let d = json!({ "user_id": "1", "audio_ssrc": 5, "video_ssrc": 0, "streams": [{ "ssrc": 6 }, { "ssrc": 0 }] });
        assert_eq!(video_ssrcs(&d).collect::<Vec<_>>(), [5, 6]);
    }

    #[test]
    fn voice_state_tracks_video_and_removal() {
        let (shared, mut rx) = shared();
        assert_eq!(voice_dispatch(&shared, "VOICE_STATE_UPDATE", &json!({ "user_id": "u", "channel_id": "c", "self_stream": true })), None);
        let Ok(VoiceEvent::VideoState { user_id, streaming: true, camera: false }) = rx.try_recv() else { panic!() };
        assert_eq!(user_id, "u");
        assert!(voice_dispatch(&shared, "VOICE_STATE_UPDATE", &json!({ "user_id": "me", "channel_id": null })).is_some());
    }

    #[test]
    fn listening_gain_follows_volume_mute_and_deafen() {
        let (shared, _rx) = shared();
        assert_eq!(shared.listening_gain("u"), 1.0);
        shared.listening.lock().unwrap().insert("u".into(), (1.5, false));
        assert_eq!(shared.listening_gain("u"), 1.5);
        shared.listening.lock().unwrap().insert("u".into(), (1.5, true));
        assert_eq!(shared.listening_gain("u"), 0.0, "locally muted");
        shared.deafened.store(true, Ordering::Relaxed);
        assert_eq!(shared.listening_gain("v"), 0.0, "deafened hears no one");
    }

    #[test]
    fn pings_average_and_cap() {
        let mut pings = Pings::default();
        let now = chrono::Utc::now().timestamp_millis();
        (0..25).for_each(|_| pings.record(now - 40));
        let (last, avg, all) = pings.stats();
        assert!((40..60).contains(&last) && (40..60).contains(&avg), "{last} {avg}");
        assert_eq!(all.len(), PINGS_KEPT);
    }
}
