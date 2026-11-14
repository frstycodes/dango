// Media over UDP: mic → processing → Opus → DAVE → AEAD → socket and the reverse into the mixer;
// H.264 video received and decoded into frames; our camera and Go Live sent as H.264. Runs on
// plain threads: cpal, Opus, OpenH264, capture callbacks and blocking UDP are all synchronous.

use std::{
    collections::{HashMap, hash_map::Entry},
    io::ErrorKind,
    net::UdpSocket,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        mpsc::{self as std_mpsc, Receiver, RecvTimeoutError},
    },
    thread,
    time::{Duration, Instant},
};

use davey::{Codec, DaveSession, MediaType, OPUS_SILENCE_PACKET};
use futures::channel::mpsc::UnboundedSender;
use opus::{Application, Channels, Decoder, Encoder};
use serde_json::json;
use tokio_tungstenite::tungstenite::Message as WsMessage;

use super::{
    GoLiveOptions, InputMode, Role, ShareSource, Shared, Ssrcs, VideoKind, VideoSource, VoiceEvent,
    audio::{self, AudioSettings, FRAME_LEN, FRAME_SAMPLES, Mixer, SAMPLE_RATE},
    camera::{self, CameraCapture},
    rtp::{self, Cipher, Received},
    video::{self, Depacketizer, VideoDecoder, VideoEncoder, VideoFrame},
};

const DAVE_MARKER: [u8; 2] = [0xfa, 0xfa];
const MAX_FRAME_SAMPLES: usize = 5760;
const SILENCE_FRAMES: u8 = 5;
const BITRATE: i32 = 64_000;
// Automatic voice activity: speech must clear this level and the tracked noise floor by this margin.
const VOICE_THRESHOLD_DB: f32 = -40.0;
const NOISE_MARGIN_DB: f32 = 9.5;
const HANGOVER_FRAMES: u32 = 15;
const QUIET_AFTER: Duration = Duration::from_millis(250);
const POLL: Duration = Duration::from_millis(100);
const CAMERA_POLL: Duration = Duration::from_millis(200);
const KEYFRAME_REQUEST_GAP: Duration = Duration::from_secs(1);
const MAX_CONCEALED_FRAMES: u16 = 5;
// A silence frame this often while not talking keeps the UDP path (NAT mapping) open.
const KEEPALIVE: Duration = Duration::from_secs(5);
// Mic level events go out every this many 20 ms frames (25 per second).
const LEVEL_EVERY: u32 = 2;
pub const CAMERA_SIZE: (u32, u32) = (1280, 720);
pub const CAMERA_FPS: u32 = 30;
// `GoLiveOptions::resolution == 0` (source size) is still capped here.
const SOURCE_SIZE_CAP: (u32, u32) = (3840, 2160);
const VIDEO_MTU: usize = 1200;
const VIDEO_CLOCK_HZ: u128 = 90_000;
const IDLE_REPEAT: Duration = Duration::from_millis(500);
const STREAM_AUDIO_BITRATE: i32 = 128_000;

// 16:9 bounds for a Go Live height.
pub fn go_live_size(resolution: u32) -> (u32, u32) {
    match resolution {
        0 => SOURCE_SIZE_CAP,
        height => ((height * 16 / 9) & !1, height),
    }
}

// Roughly 0.09 bits per pixel, which keeps screen text sharp in H.264 baseline.
pub fn video_bitrate(width: u32, height: u32, fps: u32) -> u32 {
    ((width as f64 * height as f64 * fps as f64 * 0.09) as u32).clamp(300_000, 8_000_000)
}

pub struct Link {
    pub socket: UdpSocket,
    pub ssrc: u32,
    // Our outgoing video SSRCs, from the voice ready payload.
    pub video_ssrc: u32,
    pub rtx_ssrc: u32,
    pub cipher: Cipher,
    pub dave: Arc<Mutex<DaveSession>>,
    pub e2ee: Arc<AtomicBool>,
    pub shared: Arc<Shared>,
    pub role: Role,
    pub my_id: String,
    pub events: UnboundedSender<VoiceEvent>,
    pub ws: UnboundedSender<WsMessage>,
    // One nonce counter for every packet sealed with this key (RTP and RTCP): a repeat breaks AES-GCM.
    pub nonce: AtomicU32,
    // Set when a viewer asks for a keyframe (or when sending starts).
    pub keyframe: AtomicBool,
    pub counters: Arc<Counters>,
}

impl Link {
    fn next_nonce(&self) -> u32 {
        self.nonce.fetch_add(1, Ordering::Relaxed)
    }

    // Mixer queues are shared by the voice and watch connections, whose SSRCs may collide.
    fn mix_key(&self, ssrc: u32) -> u64 {
        let connection = if self.role == Role::Voice { 0 } else { 1 };
        (connection << 32) | ssrc as u64
    }
}

// Packet loss, both ways, for `Stats`.
#[derive(Default)]
pub struct Counters {
    received: AtomicU64,
    expected: AtomicU64,
    // Percent, as f32 bits, from the server's RTCP receiver reports about our audio.
    outbound_loss: AtomicU32,
}

impl Counters {
    fn count(&self, gap: u16) {
        self.received.fetch_add(1, Ordering::Relaxed);
        self.expected.fetch_add(gap as u64 + 1, Ordering::Relaxed);
    }

    // (inbound %, outbound %) since the last call.
    pub fn take_loss(&self) -> (f32, f32) {
        let received = self.received.swap(0, Ordering::Relaxed) as f32;
        let expected = self.expected.swap(0, Ordering::Relaxed) as f32;
        let inbound = if expected > 0.0 { (1.0 - received / expected).max(0.0) * 100.0 } else { 0.0 };
        (inbound, f32::from_bits(self.outbound_loss.load(Ordering::Relaxed)))
    }
}

pub type Users = Arc<Mutex<HashMap<u32, String>>>;

pub fn map_ssrc(users: &Users, ssrc: u32, user_id: &str) {
    if let Ok(mut users) = users.lock() {
        users.insert(ssrc, user_id.to_owned());
    }
}

pub struct Media {
    stop: Arc<AtomicBool>,
    link: Arc<Link>,
    users: Users,
    // Audio devices or the screen capture; dropping it stops them.
    _devices: Devices,
}

impl Drop for Media {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

// Binds a UDP socket to the voice server and asks it for our public address.
pub fn discover(ip: &str, port: u16, ssrc: u32) -> Result<(UdpSocket, (String, u16)), String> {
    let socket = UdpSocket::bind("0.0.0.0:0").map_err(|e| e.to_string())?;
    socket.connect((ip, port)).map_err(|e| e.to_string())?;
    socket.set_read_timeout(Some(Duration::from_secs(2))).map_err(|e| e.to_string())?;
    let mut buf = [0u8; 128];
    for _ in 0..3 {
        socket.send(&rtp::discovery_request(ssrc)).map_err(|e| e.to_string())?;
        let Ok(n) = socket.recv(&mut buf) else { continue };
        if let Some(address) = rtp::discovery_response(&buf[..n]) {
            return Ok((socket, address));
        }
    }
    Err("IP discovery got no answer".into())
}

impl Media {
    // `users` (SSRC → user id) is filled by the gateway, often before media starts.
    pub fn start(link: Link, users: Users) -> Result<Self, String> {
        let stop = Arc::new(AtomicBool::new(false));
        let rx_socket = link.socket.try_clone().map_err(|e| e.to_string())?;
        rx_socket.set_read_timeout(Some(POLL)).map_err(|e| e.to_string())?;
        let link = Arc::new(link);

        let devices: Devices = match link.role {
            Role::Broadcast(source, options) => {
                let ScreenFeed { capture, frames, sound } = start_capture(source, options)?;
                let (width, height) = go_live_size(options.resolution);
                let bitrate = video_bitrate(width, height, options.fps);
                let (l, s, fps) = (link.clone(), stop.clone(), options.fps);
                spawn("rustcord-golive-send", move || video_send_loop(&l, frames, fps, bitrate, &s))?;
                if let Some(sound) = sound {
                    let (l, s) = (link.clone(), stop.clone());
                    spawn("rustcord-golive-audio", move || stream_audio_loop(&l, sound, &s))?;
                }
                let (l, s) = (link.clone(), stop.clone());
                spawn("rustcord-golive-rtcp", move || rtcp_loop(&l, &rx_socket, &s))?;
                capture
            }
            Role::Voice => {
                let shared = &link.shared;
                let (engine, pcm) = audio::start(shared.audio.clone(), shared.mixer.clone(), true)?;
                let (l, s) = (link.clone(), stop.clone());
                spawn("rustcord-voice-send", move || send_loop(&l, pcm, &s))?;
                let (l, s) = (link.clone(), stop.clone());
                spawn("rustcord-camera-control", move || camera_loop(&l, &s))?;
                let (l, s, u) = (link.clone(), stop.clone(), users.clone());
                spawn("rustcord-voice-recv", move || recv_loop(&l, &rx_socket, &s, &u))?;
                Box::new(engine)
            }
            // Plays through the voice connection's mixer and speakers, so echo cancellation
            // sees everything that is played.
            Role::Watch => {
                let (l, s, u) = (link.clone(), stop.clone(), users.clone());
                spawn("rustcord-watch-recv", move || recv_loop(&l, &rx_socket, &s, &u))?;
                Box::new(())
            }
        };
        Ok(Self { stop, link, users, _devices: devices })
    }

    pub fn user_left(&self, user_id: &str) {
        let Ok(mut users) = self.users.lock() else { return };
        users.retain(|&ssrc, user| {
            let keep = user != user_id;
            if !keep {
                self.link.shared.mixer.remove(self.link.mix_key(ssrc));
            }
            keep
        });
    }
}

fn spawn(name: &str, f: impl FnOnce() + Send + 'static) -> Result<(), String> {
    thread::Builder::new().name(name.into()).spawn(f).map(|_| ()).map_err(|e| e.to_string())
}

fn send_loop(link: &Link, pcm: Receiver<Vec<f32>>, stop: &AtomicBool) {
    let mut encoder = match Encoder::new(SAMPLE_RATE, Channels::Stereo, Application::Voip) {
        Ok(encoder) => encoder,
        Err(e) => return eprintln!("voice: opus encoder: {e}"),
    };
    let _ = encoder.set_bitrate(opus::Bitrate::Bits(BITRATE));
    let mut out = RtpOut { sequence: rand::random(), timestamp: rand::random() };
    let mut pending: Vec<f32> = Vec::with_capacity(FRAME_LEN * 2);
    let mut opus_buf = [0u8; 4000];
    let (mut speaking, mut silence_left) = (false, 0u8);
    let mut gate = InputGate::default();
    let mut last_sent = Instant::now();
    let mut frames = 0u32;

    while !stop.load(Ordering::Relaxed) {
        match pcm.recv_timeout(POLL) {
            Ok(chunk) => pending.extend(chunk),
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => return,
        }
        while pending.len() >= FRAME_LEN {
            let mut frame: Vec<f32> = pending.drain(..FRAME_LEN).collect();
            let db = link.shared.audio.process_mic(&mut frame);
            let held = link.shared.talk_key_held.load(Ordering::Relaxed);
            let open = gate.update(link.shared.input_mode(), db, held);
            frames = frames.wrapping_add(1);
            if frames.is_multiple_of(LEVEL_EVERY) {
                let _ = link.events.unbounded_send(VoiceEvent::InputLevel { db, gate_open: open });
            }
            let wants = link.shared.may_transmit() && open;

            if wants != speaking {
                speaking = wants;
                silence_left = if speaking { 0 } else { SILENCE_FRAMES };
                let op = json!({ "op": 5, "d": { "speaking": u8::from(speaking), "delay": 0, "ssrc": link.ssrc } });
                let _ = link.ws.unbounded_send(WsMessage::Text(op.to_string()));
                let _ = link.events.unbounded_send(VoiceEvent::Speaking { user_id: link.my_id.clone(), speaking });
            }

            let opus: &[u8] = if speaking {
                match encoder.encode_float(&frame, &mut opus_buf) {
                    Ok(n) => &opus_buf[..n],
                    Err(e) => {
                        eprintln!("voice: opus encode: {e}");
                        &OPUS_SILENCE_PACKET
                    }
                }
            } else if silence_left > 0 {
                silence_left -= 1;
                &OPUS_SILENCE_PACKET
            } else if last_sent.elapsed() >= KEEPALIVE {
                &OPUS_SILENCE_PACKET
            } else {
                out.timestamp = out.timestamp.wrapping_add(FRAME_SAMPLES as u32);
                continue;
            };
            if let Some(packet) = seal(link, &mut out, opus) {
                let _ = link.socket.send(&packet);
                last_sent = Instant::now();
            }
        }
    }
}

// The mic test: the same mic chain as a call, played straight back, with levels reported.
pub fn mic_test_loop(audio: &AudioSettings, mixer: &Mixer, pcm: &Receiver<Vec<f32>>, levels: &UnboundedSender<f32>, stop: &AtomicBool) {
    let mut pending: Vec<f32> = Vec::with_capacity(FRAME_LEN * 2);
    let mut frames = 0u32;
    while !stop.load(Ordering::Relaxed) {
        match pcm.recv_timeout(POLL) {
            Ok(chunk) => pending.extend(chunk),
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => return,
        }
        while pending.len() >= FRAME_LEN {
            let mut frame: Vec<f32> = pending.drain(..FRAME_LEN).collect();
            let db = audio.process_mic(&mut frame);
            mixer.push(0, &frame, 1.0);
            frames = frames.wrapping_add(1);
            if frames.is_multiple_of(LEVEL_EVERY) && levels.unbounded_send(db).is_err() {
                return;
            }
        }
    }
}

struct RtpOut {
    sequence: u16,
    timestamp: u32,
}

// With E2EE in force a frame that can't be DAVE-encrypted yet is dropped, never sent in the clear.
fn seal(link: &Link, out: &mut RtpOut, opus: &[u8]) -> Option<Vec<u8>> {
    let payload = if link.e2ee.load(Ordering::Relaxed) {
        let mut dave = link.dave.lock().ok()?;
        dave.encrypt_opus(opus).ok()?.into_owned()
    } else {
        opus.to_vec()
    };
    let header = rtp::header(rtp::OPUS_PAYLOAD_TYPE, false, out.sequence, out.timestamp, link.ssrc);
    let packet = link.cipher.seal(&header, &payload, link.next_nonce())?;
    out.sequence = out.sequence.wrapping_add(1);
    out.timestamp = out.timestamp.wrapping_add(FRAME_SAMPLES as u32);
    Some(packet)
}

// Decides when the mic transmits, per `InputMode`. Automatic voice activity tracks the background
// noise floor (falls fast, rises over ~4 s); both voice-activity modes hold open briefly so word
// endings aren't clipped.
struct InputGate {
    noise_floor_db: f32,
    hangover: u32,
    last_held: Option<Instant>,
}

impl Default for InputGate {
    fn default() -> Self {
        Self { noise_floor_db: VOICE_THRESHOLD_DB - NOISE_MARGIN_DB, hangover: 0, last_held: None }
    }
}

impl InputGate {
    fn update(&mut self, mode: InputMode, db: f32, key_held: bool) -> bool {
        match mode {
            InputMode::PushToTalk { release_delay_ms } => {
                if key_held {
                    self.last_held = Some(Instant::now());
                }
                let delay = Duration::from_millis(release_delay_ms as u64);
                key_held || self.last_held.is_some_and(|at| at.elapsed() < delay)
            }
            InputMode::VoiceActivity { auto, threshold_db } => {
                let voiced = if auto { db > VOICE_THRESHOLD_DB.max(self.noise_floor_db + NOISE_MARGIN_DB) } else { db > threshold_db };
                let rate = if db < self.noise_floor_db { 0.5 } else { 0.005 };
                self.noise_floor_db += (db - self.noise_floor_db) * rate;
                self.hangover = if voiced { HANGOVER_FRAMES } else { self.hangover.saturating_sub(1) };
                self.hangover > 0
            }
        }
    }
}

// Follows `Shared::camera`: starts and stops the camera, its encoder and the op 12 announcement.
fn camera_loop(link: &Arc<Link>, stop: &AtomicBool) {
    let mut current: Option<(String, CameraCapture)> = None;
    while !stop.load(Ordering::Relaxed) {
        let want = link.shared.camera.lock().ok().and_then(|c| c.clone());
        if current.as_ref().map(|(id, _)| id) != want.as_ref() {
            let was_on = current.take().is_some();
            match want.map(|id| start_camera(link, &id).map(|capture| (id, capture))) {
                Some(Ok(started)) => current = Some(started),
                Some(Err(e)) => {
                    eprintln!("voice: camera: {e}");
                    if let Ok(mut camera) = link.shared.camera.lock() {
                        *camera = None;
                    }
                    link.shared.send_voice_state(true);
                }
                None => {}
            }
            if current.is_some() || was_on {
                let ssrcs = Ssrcs { audio: link.ssrc, video: link.video_ssrc, rtx: link.rtx_ssrc };
                let op = super::announce_video(&ssrcs, VideoKind::Camera, current.is_some());
                let _ = link.ws.unbounded_send(WsMessage::Text(op.to_string()));
            }
        }
        thread::sleep(CAMERA_POLL);
    }
}

// Frames go to the encoder (dropped if it lags) and back to the UI as our self-preview.
fn start_camera(link: &Arc<Link>, id: &str) -> Result<CameraCapture, String> {
    let (tx, frames) = std_mpsc::sync_channel::<Arc<VideoFrame>>(1);
    let (events, me) = (link.events.clone(), link.my_id.clone());
    let capture = camera::start(id, CAMERA_SIZE, move |frame| {
        let frame = Arc::new(frame);
        let _ = tx.try_send(frame.clone());
        let _ = events.unbounded_send(VoiceEvent::StreamFrame { user_id: me.clone(), source: VideoSource::Camera, frame });
    })?;
    link.keyframe.store(true, Ordering::Relaxed);
    let l = link.clone();
    let bitrate = video_bitrate(CAMERA_SIZE.0, CAMERA_SIZE.1, CAMERA_FPS);
    // The loop ends when the capture is dropped (its frame sender goes with it).
    spawn("rustcord-camera-send", move || video_send_loop(&l, frames, CAMERA_FPS, bitrate, &AtomicBool::new(false)))?;
    Ok(capture)
}

// ponytail: plays packets in arrival order (late ones are dropped) and conceals short gaps with
// Opus PLC; add a reorder buffer if remote audio crackles on links that reorder packets.
fn recv_loop(link: &Link, socket: &UdpSocket, stop: &AtomicBool, users: &Mutex<HashMap<u32, String>>) {
    let mut decoders: HashMap<u32, AudioIn> = HashMap::new();
    let mut videos: HashMap<u32, VideoIn> = HashMap::new();
    let mut heard: HashMap<String, Instant> = HashMap::new();
    let mut buf = [0u8; 2048];
    let mut pcm = vec![0f32; MAX_FRAME_SAMPLES * 2];
    let speaking_events = link.role == Role::Voice;

    while !stop.load(Ordering::Relaxed) {
        match socket.recv(&mut buf) {
            Ok(n) if rtp::is_rtcp(&buf[..n]) => handle_rtcp(link, &buf[..n]),
            Ok(n) => {
                let Some((user, packet)) = open(link, users, &buf[..n]) else { continue };
                match packet.payload_type {
                    rtp::OPUS_PAYLOAD_TYPE => {
                        let Some(opus) = dave_open(link, &user, MediaType::AUDIO, packet.payload) else { continue };
                        if opus == OPUS_SILENCE_PACKET {
                            continue;
                        }
                        if heard.insert(user.clone(), Instant::now()).is_none() && speaking_events {
                            let _ = link.events.unbounded_send(VoiceEvent::Speaking { user_id: user.clone(), speaking: true });
                        }
                        if let Some(audio) = audio_in(&mut decoders, packet.ssrc) {
                            let gain = link.shared.listening_gain(&user);
                            audio.play(link, packet.ssrc, packet.sequence, &opus, &mut pcm, gain);
                        }
                    }
                    rtp::H264_PAYLOAD_TYPE => {
                        let video = match videos.entry(packet.ssrc) {
                            Entry::Occupied(e) => e.into_mut(),
                            Entry::Vacant(e) => match VideoDecoder::new() {
                                Ok(decoder) => e.insert(VideoIn { depacketizer: Depacketizer::default(), decoder, last_keyframe_request: None }),
                                Err(err) => {
                                    eprintln!("voice: h264 decoder: {err}");
                                    continue;
                                }
                            },
                        };
                        if let Some(frame) = video.receive(link, &user, &packet) {
                            let event = VoiceEvent::StreamFrame { user_id: user, source: link.role.video_source(), frame: Arc::new(frame) };
                            let _ = link.events.unbounded_send(event);
                        }
                    }
                    _ => {}
                }
            }
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(e) => return eprintln!("voice: udp receive: {e}"),
        }
        heard.retain(|user, at| {
            let talking = at.elapsed() < QUIET_AFTER;
            if !talking && speaking_events {
                let _ = link.events.unbounded_send(VoiceEvent::Speaking { user_id: user.clone(), speaking: false });
            }
            talking
        });
    }
}

// Keyframe requests from viewers and the server's loss reports about what we send.
fn handle_rtcp(link: &Link, packet: &[u8]) {
    let Some(rtcp) = link.cipher.open_rtcp(packet) else { return };
    if rtp::is_pli(&rtcp) {
        link.keyframe.store(true, Ordering::Relaxed);
        if super::debug() {
            eprintln!("[{:?}] viewer asked for a keyframe", link.role);
        }
    }
    if let Some(loss) = rtp::reported_loss(&rtcp, link.ssrc) {
        link.counters.outbound_loss.store(loss.to_bits(), Ordering::Relaxed);
    }
}

struct VideoIn {
    depacketizer: Depacketizer,
    decoder: VideoDecoder,
    last_keyframe_request: Option<Instant>,
}

impl VideoIn {
    // DAVE encrypts whole encoded frames, so decryption happens after depacketization.
    fn receive(&mut self, link: &Link, user: &str, packet: &Received) -> Option<VideoFrame> {
        if !packet.marker {
            self.depacketizer.push(packet.sequence, false, &packet.payload);
            return None;
        }
        let unit = self.depacketizer.push(packet.sequence, true, &packet.payload);
        let Some(unit) = unit.and_then(|unit| dave_open(link, user, MediaType::VIDEO, unit)) else {
            self.decoder.frame_lost();
            self.request_keyframe(link, packet.ssrc);
            return None;
        };
        let frame = self.decoder.decode(&unit);
        if self.decoder.awaiting_keyframe() {
            self.request_keyframe(link, packet.ssrc);
        }
        frame
    }

    // ponytail: PLI only; NACK/RTX retransmission would recover single lost packets without a keyframe.
    fn request_keyframe(&mut self, link: &Link, media_ssrc: u32) {
        if self.last_keyframe_request.is_some_and(|at| at.elapsed() < KEYFRAME_REQUEST_GAP) {
            return;
        }
        self.last_keyframe_request = Some(Instant::now());
        let pli = rtp::pli(link.ssrc, media_ssrc);
        let (header, body) = pli.split_at(rtp::RTCP_HEADER_LEN);
        if let Some(packet) = link.cipher.seal(header, body, link.next_nonce()) {
            let _ = link.socket.send(&packet);
            if super::debug() {
                eprintln!("[{:?}] video: requested keyframe for ssrc {media_ssrc}", link.role);
            }
        }
    }
}

type Devices = Box<dyn std::any::Any + Send>;

struct ScreenFeed {
    capture: Devices,
    frames: Receiver<Arc<VideoFrame>>,
    sound: Option<Receiver<Vec<f32>>>,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn start_capture(source: ShareSource, options: GoLiveOptions) -> Result<ScreenFeed, String> {
    let (width, height) = go_live_size(options.resolution);
    let captured = super::capture::start(source, options.fps, width, height, options.audio)?;
    let sound = options.audio.then_some(captured.audio);
    Ok(ScreenFeed { capture: Box::new(captured.capture), frames: captured.frames, sound })
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn start_capture(_source: ShareSource, _options: GoLiveOptions) -> Result<ScreenFeed, String> {
    Err("Screen sharing is only implemented on macOS and Windows".into())
}

// The stream's sound: always on (not gated by mic mute or voice detection), music-tuned Opus.
fn stream_audio_loop(link: &Link, sound: Receiver<Vec<f32>>, stop: &AtomicBool) {
    let mut encoder = match Encoder::new(SAMPLE_RATE, Channels::Stereo, Application::Audio) {
        Ok(encoder) => encoder,
        Err(e) => return eprintln!("voice: opus encoder: {e}"),
    };
    let _ = encoder.set_bitrate(opus::Bitrate::Bits(STREAM_AUDIO_BITRATE));
    // Not verified: speaking flag 2 (soundshare) marks this SSRC as stream audio.
    let op = json!({ "op": 5, "d": { "speaking": 2, "delay": 0, "ssrc": link.ssrc } });
    let _ = link.ws.unbounded_send(WsMessage::Text(op.to_string()));
    let mut out = RtpOut { sequence: rand::random(), timestamp: rand::random() };
    let mut pending: Vec<f32> = Vec::with_capacity(FRAME_LEN * 2);
    let mut opus_buf = [0u8; 4000];
    while !stop.load(Ordering::Relaxed) {
        match sound.recv_timeout(POLL) {
            Ok(chunk) => pending.extend(chunk),
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => return,
        }
        while pending.len() >= FRAME_LEN {
            let frame: Vec<f32> = pending.drain(..FRAME_LEN).collect();
            let Ok(n) = encoder.encode_float(&frame, &mut opus_buf) else { continue };
            if let Some(packet) = seal(link, &mut out, &opus_buf[..n]) {
                let _ = link.socket.send(&packet);
            }
        }
    }
}

// Encodes frames as they come (camera or screen) and sends them on our video SSRC. Ends when
// `stop` is set or the frame source is dropped.
fn video_send_loop(link: &Link, frames: Receiver<Arc<VideoFrame>>, fps: u32, bitrate: u32, stop: &AtomicBool) {
    let mut encoder = match VideoEncoder::new(fps, bitrate) {
        Ok(encoder) => encoder,
        Err(e) => return eprintln!("voice: h264 encoder: {e}"),
    };
    let started = Instant::now();
    let mut sequence: u16 = rand::random();
    let mut last: Option<(Arc<VideoFrame>, Instant)> = None;
    while !stop.load(Ordering::Relaxed) {
        // ScreenCaptureKit only delivers frames when the screen changes; repeating the last one
        // keeps the stream alive and lets keyframe requests be answered on a still screen.
        let frame = match frames.recv_timeout(POLL) {
            // Our own Go Live comes back to the UI as its self-view, as the camera's does.
            Ok(frame) if matches!(link.role, Role::Broadcast(..)) => {
                let _ = link.events.unbounded_send(VoiceEvent::StreamFrame { user_id: link.my_id.clone(), source: VideoSource::GoLive, frame: frame.clone() });
                frame
            }
            Ok(frame) => frame,
            Err(RecvTimeoutError::Timeout) => match last.take() {
                Some((frame, at)) if at.elapsed() >= IDLE_REPEAT || link.keyframe.load(Ordering::Relaxed) => frame,
                kept => {
                    last = kept;
                    continue;
                }
            },
            Err(RecvTimeoutError::Disconnected) => return,
        };
        if link.keyframe.swap(false, Ordering::Relaxed) {
            encoder.force_keyframe();
        }
        let unit = encoder.encode(&frame);
        last = Some((frame, Instant::now()));
        let Some(unit) = unit else { continue };
        let Some(payload) = dave_seal_video(link, &unit) else { continue };
        let timestamp = (started.elapsed().as_micros() * VIDEO_CLOCK_HZ / 1_000_000) as u32;
        let packets = video::packetize(&payload, VIDEO_MTU);
        let last_index = packets.len().saturating_sub(1);
        for (i, payload) in packets.iter().enumerate() {
            let header = rtp::header(rtp::H264_PAYLOAD_TYPE, i == last_index, sequence, timestamp, link.video_ssrc);
            sequence = sequence.wrapping_add(1);
            if let Some(packet) = link.cipher.seal(&header, payload, link.next_nonce()) {
                let _ = link.socket.send(&packet);
            }
        }
    }
}

// Same rule as audio: under E2EE a frame that can't be DAVE-encrypted yet is dropped.
fn dave_seal_video(link: &Link, unit: &[u8]) -> Option<Vec<u8>> {
    if !link.e2ee.load(Ordering::Relaxed) {
        return Some(unit.to_vec());
    }
    let mut dave = link.dave.lock().ok()?;
    let sealed = dave.encrypt(MediaType::VIDEO, Codec::H264, unit);
    if let Err(e) = &sealed
        && super::debug()
    {
        eprintln!("[{:?}] dave encrypt: {e:?}", link.role);
    }
    Some(sealed.ok()?.into_owned())
}

// The broadcast socket only carries RTCP back to us: keyframe requests and loss reports.
fn rtcp_loop(link: &Link, socket: &UdpSocket, stop: &AtomicBool) {
    let mut buf = [0u8; 1500];
    while !stop.load(Ordering::Relaxed) {
        let Ok(n) = socket.recv(&mut buf) else { continue };
        if rtp::is_rtcp(&buf[..n]) {
            handle_rtcp(link, &buf[..n]);
        }
    }
}

fn open(link: &Link, users: &Mutex<HashMap<u32, String>>, packet: &[u8]) -> Option<(String, Received)> {
    let Some(received) = link.cipher.open(packet) else {
        if super::debug() {
            eprintln!("[{:?}] udp: {} bytes failed transport decryption", link.role, packet.len());
        }
        return None;
    };
    let user = users.lock().ok()?.get(&received.ssrc).cloned();
    if user.is_none() && super::debug() {
        eprintln!("[{:?}] udp: unmapped ssrc {} pt {}", link.role, received.ssrc, received.payload_type);
    }
    Some((user?, received))
}

// With E2EE in force only DAVE frames (and Opus silence) are accepted.
fn dave_open(link: &Link, user: &str, media: MediaType, data: Vec<u8>) -> Option<Vec<u8>> {
    if data.ends_with(&DAVE_MARKER) {
        let id = user.parse().ok()?;
        let result = link.dave.lock().ok()?.decrypt(id, media, &data);
        if let Err(e) = &result
            && super::debug()
        {
            eprintln!("[{:?}] dave decrypt for {user}: {e:?}", link.role);
        }
        return result.ok();
    }
    let allowed = !link.e2ee.load(Ordering::Relaxed) || data == OPUS_SILENCE_PACKET;
    allowed.then_some(data)
}

struct AudioIn {
    decoder: Decoder,
    last_sequence: Option<u16>,
}

fn audio_in(decoders: &mut HashMap<u32, AudioIn>, ssrc: u32) -> Option<&mut AudioIn> {
    match decoders.entry(ssrc) {
        Entry::Occupied(e) => Some(e.into_mut()),
        Entry::Vacant(e) => {
            let decoder = Decoder::new(SAMPLE_RATE, Channels::Stereo).ok()?;
            Some(e.insert(AudioIn { decoder, last_sequence: None }))
        }
    }
}

impl AudioIn {
    // `gain` is the listener's volume for this user (0 when muted or deafened).
    fn play(&mut self, link: &Link, ssrc: u32, sequence: u16, opus: &[u8], pcm: &mut [f32], gain: f32) {
        let gap = self.last_sequence.map_or(0, |last| sequence.wrapping_sub(last).wrapping_sub(1));
        if self.last_sequence.is_some() && gap >= u16::MAX / 2 {
            return; // late: older than what was already played
        }
        self.last_sequence = Some(sequence);
        link.counters.count(gap);
        let (mixer, key) = (&link.shared.mixer, link.mix_key(ssrc));
        // Short gaps get Opus loss concealment (exactly one frame each); longer ones are silence.
        if gap <= MAX_CONCEALED_FRAMES {
            for _ in 0..gap {
                if let Ok(samples) = self.decoder.decode_float(&[], &mut pcm[..FRAME_LEN], false) {
                    mixer.push(key, &pcm[..samples * 2], gain);
                }
            }
        }
        let Ok(samples) = self.decoder.decode_float(opus, pcm, false) else { return };
        mixer.push(key, &pcm[..samples * 2], gain);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AUTO: InputMode = InputMode::VoiceActivity { auto: true, threshold_db: 0.0 };

    #[test]
    fn auto_gate_learns_steady_noise_but_hears_speech() {
        let mut gate = InputGate::default();
        let noise = audio::level_db(&[0.03; 4]);
        let noisy: Vec<bool> = (0..300).map(|_| gate.update(AUTO, noise, false)).collect();
        assert!(noisy[0], "unknown noise first counts as voice");
        assert!(!noisy[299], "steady fan noise stops counting as speech");
        assert!(gate.update(AUTO, audio::level_db(&[0.3; 4]), false), "speech over the noise is heard");
        let held = (1..HANGOVER_FRAMES).all(|_| gate.update(AUTO, audio::SILENCE_DB, false));
        assert!(held, "hangover keeps word endings");
        assert!(!gate.update(AUTO, audio::SILENCE_DB, false));
    }

    #[test]
    fn manual_threshold_and_push_to_talk() {
        let mut gate = InputGate::default();
        let manual = InputMode::VoiceActivity { auto: false, threshold_db: -30.0 };
        assert!(!gate.update(manual, -35.0, false));
        assert!(gate.update(manual, -25.0, false));

        let ptt = InputMode::PushToTalk { release_delay_ms: 60_000 };
        let mut gate = InputGate::default();
        assert!(!gate.update(ptt, 0.0, false), "loud but key not held");
        assert!(gate.update(ptt, -100.0, true), "key held, even silent");
        assert!(gate.update(ptt, -100.0, false), "still within the release delay");
        let instant = InputMode::PushToTalk { release_delay_ms: 0 };
        assert!(!gate.update(instant, -100.0, false));
    }

    #[test]
    fn counters_report_inbound_loss() {
        let counters = Counters::default();
        counters.count(0);
        counters.count(2);
        let (inbound, outbound) = counters.take_loss();
        assert!((inbound - 50.0).abs() < 0.01, "2 of 4 expected packets lost: {inbound}");
        assert_eq!(outbound, 0.0);
        assert_eq!(counters.take_loss().0, 0.0, "counts reset per interval");
    }

    #[test]
    fn go_live_sizes_and_bitrates() {
        assert_eq!(go_live_size(720), (1280, 720));
        assert_eq!(go_live_size(1080), (1920, 1080));
        assert_eq!(go_live_size(480), (852, 480));
        assert_eq!(go_live_size(0), SOURCE_SIZE_CAP);
        assert_eq!(video_bitrate(1280, 720, 30), 2_488_320);
        assert_eq!(video_bitrate(16, 16, 1), 300_000);
    }

    #[test]
    fn opus_round_trip_at_discord_frame_size() {
        let mut encoder = Encoder::new(SAMPLE_RATE, Channels::Stereo, Application::Voip).unwrap();
        let mut decoder = Decoder::new(SAMPLE_RATE, Channels::Stereo).unwrap();
        let tone: Vec<f32> = (0..FRAME_LEN).map(|i| ((i / 2) as f32 * 0.05).sin() * 0.5).collect();
        let mut packet = [0u8; 4000];
        let n = encoder.encode_float(&tone, &mut packet).unwrap();
        let mut pcm = vec![0f32; MAX_FRAME_SAMPLES * 2];
        assert_eq!(decoder.decode_float(&packet[..n], &mut pcm, false).unwrap(), FRAME_SAMPLES);
    }
}
