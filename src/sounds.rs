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

pub fn play(cue: Cue) {
    if SILENCED.lock().is_ok_and(|s| s.contains(&cue)) {
        return;
    }
    preview(cue);
}

// One audio thread owns the output device for the life of the app; cues are queued to it.
pub fn preview(cue: Cue) {
    static PLAYER: std::sync::OnceLock<std::sync::mpsc::Sender<Cue>> = std::sync::OnceLock::new();
    let player = PLAYER.get_or_init(|| {
        let (send, receive) = std::sync::mpsc::channel::<Cue>();
        std::thread::spawn(move || {
            let Ok(mut sink) = rodio::DeviceSinkBuilder::open_default_sink() else { return };
            sink.log_on_drop(false);
            for cue in receive {
                if let Ok(sound) = rodio::Decoder::try_from(std::io::Cursor::new(cue.bytes())) {
                    sink.mixer().add(sound);
                }
            }
        });
        send
    });
    let _ = player.send(cue);
}
