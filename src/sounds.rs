// Discord's own sound cues (assets/discord/sounds, personal use only), played where Discord plays them.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, serde::Serialize, serde::Deserialize)]
pub enum Cue {
    Mute,
    Unmute,
    Deafen,
    Undeafen,
    UserJoin,
    UserLeave,
    Disconnect,
    StreamStarted,
    StreamEnded,
    Message,
    Mention,
    PttStart,
    PttStop,
}

impl Cue {
    fn bytes(self) -> &'static [u8] {
        match self {
            Cue::Mute => include_bytes!("../assets/discord/sounds/mute.mp3"),
            Cue::Unmute => include_bytes!("../assets/discord/sounds/unmute.mp3"),
            Cue::Deafen => include_bytes!("../assets/discord/sounds/deafen.mp3"),
            Cue::Undeafen => include_bytes!("../assets/discord/sounds/undeafen.mp3"),
            Cue::UserJoin => include_bytes!("../assets/discord/sounds/user_join.mp3"),
            Cue::UserLeave => include_bytes!("../assets/discord/sounds/user_leave.mp3"),
            Cue::Disconnect => include_bytes!("../assets/discord/sounds/disconnect.mp3"),
            Cue::StreamStarted => include_bytes!("../assets/discord/sounds/stream_started.mp3"),
            Cue::StreamEnded => include_bytes!("../assets/discord/sounds/stream_ended.mp3"),
            Cue::Message => include_bytes!("../assets/discord/sounds/message1.mp3"),
            Cue::Mention => include_bytes!("../assets/discord/sounds/mention1.mp3"),
            Cue::PttStart => include_bytes!("../assets/discord/sounds/ptt_start.mp3"),
            Cue::PttStop => include_bytes!("../assets/discord/sounds/ptt_stop.mp3"),
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

#[cfg(target_os = "macos")]
pub fn preview(cue: Cue) {
    use std::{cell::RefCell, collections::HashMap};

    use objc2::{
        msg_send,
        runtime::{AnyClass, AnyObject},
    };

    // One NSSound per cue, created on first use and kept: replaying restarts it instead of leaking a copy.
    thread_local! {
        static PLAYERS: RefCell<HashMap<Cue, usize>> = RefCell::new(HashMap::new());
    }
    PLAYERS.with(|players| {
        let mut players = players.borrow_mut();
        let sound = *players.entry(cue).or_insert_with(|| {
            let bytes = cue.bytes();
            // SAFETY: plain AppKit messages; the NSData copies the bytes, and a nil NSSound is checked below.
            unsafe {
                let (Some(data_class), Some(sound_class)) = (AnyClass::get(c"NSData"), AnyClass::get(c"NSSound")) else { return 0 };
                let data: *mut AnyObject = msg_send![data_class, dataWithBytes: bytes.as_ptr() as *const std::ffi::c_void, length: bytes.len()];
                let sound: *mut AnyObject = msg_send![sound_class, alloc];
                let sound: *mut AnyObject = msg_send![sound, initWithData: data];
                sound as usize
            }
        });
        if sound == 0 {
            return;
        }
        // SAFETY: `sound` is a live NSSound retained for the life of the process.
        unsafe {
            let sound = sound as *mut AnyObject;
            let _: bool = msg_send![sound, stop];
            let _: bool = msg_send![sound, play];
        }
    });
}

// ponytail: cues are macOS-only for now; rodio is the upgrade for Windows and Linux.
#[cfg(not(target_os = "macos"))]
pub fn preview(_: Cue) {}
