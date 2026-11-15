use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use rodio::Source;

// Wave's sound cues (assets/sounds, made by tools/sound_cues.py), played where Discord plays its own.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, serde::Serialize, serde::Deserialize)]
pub enum Cue {
    Mute,
    Unmute,
    Deafen,
    Undeafen,
    UserJoin,
    UserLeave,
    UserMoved,
    Disconnect,
    StreamStarted,
    StreamEnded,
    StreamUserJoined,
    StreamUserLeft,
    Message,
    Mention,
    PttStart,
    PttStop,
    CallCalling,
    CallRinging,
}

impl Cue {
    fn bytes(self) -> &'static [u8] {
        match self {
            Cue::Mute => include_bytes!("../assets/sounds/mute.mp3"),
            Cue::Unmute => include_bytes!("../assets/sounds/unmute.mp3"),
            Cue::Deafen => include_bytes!("../assets/sounds/deafen.mp3"),
            Cue::Undeafen => include_bytes!("../assets/sounds/undeafen.mp3"),
            Cue::UserJoin => include_bytes!("../assets/sounds/user_join.mp3"),
            Cue::UserLeave => include_bytes!("../assets/sounds/user_leave.mp3"),
            Cue::UserMoved => include_bytes!("../assets/sounds/user_moved.mp3"),
            Cue::Disconnect => include_bytes!("../assets/sounds/disconnect.mp3"),
            Cue::StreamStarted => include_bytes!("../assets/sounds/stream_started.mp3"),
            Cue::StreamEnded => include_bytes!("../assets/sounds/stream_ended.mp3"),
            Cue::StreamUserJoined => include_bytes!("../assets/sounds/stream_user_joined.mp3"),
            Cue::StreamUserLeft => include_bytes!("../assets/sounds/stream_user_left.mp3"),
            Cue::Message => include_bytes!("../assets/sounds/message.mp3"),
            Cue::Mention => include_bytes!("../assets/sounds/mention.mp3"),
            Cue::PttStart => include_bytes!("../assets/sounds/ptt_start.mp3"),
            Cue::PttStop => include_bytes!("../assets/sounds/ptt_stop.mp3"),
            Cue::CallCalling => include_bytes!("../assets/sounds/call_calling.mp3"),
            Cue::CallRinging => include_bytes!("../assets/sounds/call_ringing.mp3"),
        }
    }
}

static SILENCED: std::sync::Mutex<Vec<Cue>> = std::sync::Mutex::new(Vec::new());

// The cues turned off in Settings; `play` skips them, `preview` doesn't.
pub fn silence(cues: &[Cue]) {
    if let Ok(mut silenced) = SILENCED.lock() {
        *silenced = cues.to_vec();
    }
}

fn silenced(cue: Cue) -> bool {
    SILENCED.lock().is_ok_and(|s| s.contains(&cue))
}

pub fn play(cue: Cue) {
    if silenced(cue) {
        return;
    }
    preview(cue);
}

pub fn preview(cue: Cue) {
    send(cue, None);
}

// The ringtone playing now, if any; setting its flag ends the loop.
static RINGING: std::sync::Mutex<Option<Arc<AtomicBool>>> = std::sync::Mutex::new(None);

// Loops a ringtone until `stop_ring`, replacing any ringing already.
pub fn ring(cue: Cue) {
    stop_ring();
    if silenced(cue) {
        return;
    }
    let stop = Arc::new(AtomicBool::new(false));
    if let Ok(mut ringing) = RINGING.lock() {
        *ringing = Some(stop.clone());
    }
    send(cue, Some(stop));
}

pub fn stop_ring() {
    if let Some(stop) = RINGING.lock().ok().and_then(|mut ringing| ringing.take()) {
        stop.store(true, Ordering::Relaxed);
    }
}

// One audio thread owns the output device for the life of the app; cues are queued to it. A cue sent
// with a stop flag loops until the flag is set.
fn send(cue: Cue, looping: Option<Arc<AtomicBool>>) {
    type Request = (Cue, Option<Arc<AtomicBool>>);
    static PLAYER: std::sync::OnceLock<std::sync::mpsc::Sender<Request>> = std::sync::OnceLock::new();
    let player = PLAYER.get_or_init(|| {
        let (send, receive) = std::sync::mpsc::channel::<Request>();
        std::thread::spawn(move || {
            let Ok(mut sink) = rodio::DeviceSinkBuilder::open_default_sink() else { return };
            sink.log_on_drop(false);
            for (cue, looping) in receive {
                let Ok(sound) = rodio::Decoder::try_from(std::io::Cursor::new(cue.bytes())) else { continue };
                match looping {
                    None => sink.mixer().add(sound),
                    Some(stop) => sink.mixer().add(sound.buffered().repeat_infinite().stoppable().periodic_access(Duration::from_millis(50), move |s| {
                        if stop.load(Ordering::Relaxed) {
                            s.stop();
                        }
                    })),
                }
            }
        });
        send
    });
    let _ = player.send((cue, looping));
}
