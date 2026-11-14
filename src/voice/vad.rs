// Decides when the mic transmits, per `InputMode`, the way Discord's media engine is configured
// (docs/discord-audio.md): automatic sensitivity listens to Advanced Voice Activity (a neural
// speech probability) or else WebRTC's VAD; a manual threshold compares the level. Every voiced
// 10 ms block holds the gate open for `TRAILING_BLOCKS` more (Discord `vadTrailing: 25`), and an
// onset also sends the `LEADING_BLOCKS` before it (Discord `vadLeading: 5`).
// Not verified: the native engine is closed, so "blocks" being 10 ms and leading being pre-roll
// (rather than an onset debounce) are inferred, not read.

use std::time::{Duration, Instant};

use super::{
    InputMode,
    dsp::{BLOCKS_PER_FRAME, Block},
};

pub const LEADING_BLOCKS: usize = 5;
pub const TRAILING_BLOCKS: u32 = 25;
// Whole 20 ms frames that cover the leading blocks.
pub const PRE_ROLL_FRAMES: usize = LEADING_BLOCKS.div_ceil(BLOCKS_PER_FRAME);
// Discord's `vadKrispActivationThreshold`: 0.5, raised to 0.8 while automatic gain control is on
// (AGC lifts quiet noise toward speech level).
const ACTIVATION: f32 = 0.5;
const ACTIVATION_WITH_AGC: f32 = 0.8;

#[derive(Default)]
pub struct Gate {
    trailing: u32,
    last_held: Option<Instant>,
}

impl Gate {
    // Whether this 20 ms frame transmits. `agc` is whether automatic gain control is on.
    pub fn update(&mut self, mode: InputMode, agc: bool, blocks: &[Block; BLOCKS_PER_FRAME], key_held: bool) -> bool {
        let InputMode::VoiceActivity { auto, threshold_db, advanced } = mode else {
            return self.push_to_talk(mode, key_held);
        };
        let activation = if agc { ACTIVATION_WITH_AGC } else { ACTIVATION };
        let mut open = false;
        for block in blocks {
            let voiced = match (auto, advanced) {
                (true, true) => block.speech >= activation,
                (true, false) => block.vad,
                (false, _) => block.db > threshold_db,
            };
            if voiced {
                self.trailing = TRAILING_BLOCKS;
                open = true;
            } else if self.trailing > 0 {
                self.trailing -= 1;
                open = true;
            }
        }
        open
    }

    fn push_to_talk(&mut self, mode: InputMode, key_held: bool) -> bool {
        let InputMode::PushToTalk { release_delay_ms } = mode else { return false };
        self.trailing = 0;
        if key_held {
            self.last_held = Some(Instant::now());
        }
        let delay = Duration::from_millis(release_delay_ms as u64);
        key_held || self.last_held.is_some_and(|at| at.elapsed() < delay)
    }
}

// The last `PRE_ROLL_FRAMES` processed frames, oldest first, so an onset can send what led up to it.
pub struct PreRoll<const N: usize> {
    frames: [[f32; N]; PRE_ROLL_FRAMES],
    len: usize,
    next: usize,
}

impl<const N: usize> Default for PreRoll<N> {
    fn default() -> Self {
        Self { frames: [[0.0; N]; PRE_ROLL_FRAMES], len: 0, next: 0 }
    }
}

impl<const N: usize> PreRoll<N> {
    pub fn push(&mut self, frame: &[f32; N]) {
        self.frames[self.next] = *frame;
        self.next = (self.next + 1) % PRE_ROLL_FRAMES;
        self.len = (self.len + 1).min(PRE_ROLL_FRAMES);
    }

    // Takes the held frames, oldest first, leaving the buffer empty.
    pub fn drain(&mut self) -> impl Iterator<Item = &[f32; N]> {
        let (start, len) = ((self.next + PRE_ROLL_FRAMES - self.len) % PRE_ROLL_FRAMES, self.len);
        self.len = 0;
        let frames = &self.frames;
        (0..len).map(move |i| &frames[(start + i) % PRE_ROLL_FRAMES])
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn clear(&mut self) {
        self.len = 0;
    }
}

// Discord's "No Audio Detected" signal: the engine reports input at or below -100 dBFS
// (`setNoInputThreshold(-100)`) and the client debounces it, 1.5 s to call input detected and 5 s
// to call it lost (MediaEngineStore `handleSilence`).
#[derive(Default)]
pub struct InputWatch {
    detected: Option<bool>,
    candidate: bool,
    frames: u32,
}

const NO_INPUT_DB: f32 = -100.0;
const DETECTED_AFTER_FRAMES: u32 = 1500 / 20;
const LOST_AFTER_FRAMES: u32 = 5000 / 20;

impl InputWatch {
    // Feed the raw mic level of each 20 ms frame; returns whether input is detected (None at first).
    pub fn update(&mut self, raw_db: f32) -> Option<bool> {
        let input = raw_db > NO_INPUT_DB;
        if input != self.candidate {
            (self.candidate, self.frames) = (input, 0);
        }
        self.frames = self.frames.saturating_add(1);
        let needed = if input { DETECTED_AFTER_FRAMES } else { LOST_AFTER_FRAMES };
        if self.frames >= needed {
            self.detected = Some(input);
        }
        self.detected
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_watch_debounces_like_discord() {
        let mut watch = InputWatch::default();
        assert_eq!((0..74).map(|_| watch.update(-40.0)).last().unwrap(), None);
        assert_eq!(watch.update(-40.0), Some(true), "1.5 s of input");
        assert_eq!((0..249).map(|_| watch.update(-100.0)).last().unwrap(), Some(true), "short silence is ignored");
        assert_eq!(watch.update(-100.0), Some(false), "5 s of digital silence");
    }

    const ADVANCED: InputMode = InputMode::VoiceActivity { auto: true, threshold_db: -60.0, advanced: true };
    const WEBRTC: InputMode = InputMode::VoiceActivity { auto: true, threshold_db: -60.0, advanced: false };

    fn blocks(speech: f32, vad: bool, db: f32) -> [Block; BLOCKS_PER_FRAME] {
        [Block { db, speech, vad }; BLOCKS_PER_FRAME]
    }

    #[test]
    fn hangover_holds_for_discords_25_trailing_blocks() {
        let mut gate = Gate::default();
        let silent = blocks(0.0, false, -100.0);
        assert!(!gate.update(ADVANCED, false, &silent, false));
        assert!(gate.update(ADVANCED, false, &blocks(0.9, false, -100.0), false), "speech opens the gate");
        // 25 blocks of hangover = 12.5 frames: frames 1..=12 stay open, the 13th holds on its first block.
        let held: Vec<bool> = (0..14).map(|_| gate.update(ADVANCED, false, &silent, false)).collect();
        assert!(held[..13].iter().all(|&o| o), "250 ms of hangover keeps word endings: {held:?}");
        assert!(!held[13], "then it closes");

        let mut gate = Gate::default();
        gate.update(ADVANCED, false, &blocks(0.9, false, -100.0), false);
        (0..5).for_each(|_| _ = gate.update(ADVANCED, false, &silent, false));
        gate.update(ADVANCED, false, &blocks(0.9, false, -100.0), false);
        assert!((0..12).all(|_| gate.update(ADVANCED, false, &silent, false)), "speech inside the hangover restarts it");
    }

    #[test]
    fn automatic_modes_use_their_detector_and_agc_raises_activation() {
        let mut gate = Gate::default();
        assert!(gate.update(ADVANCED, false, &blocks(0.6, false, 0.0), false), "0.6 clears the 0.5 activation");
        let mut gate = Gate::default();
        assert!(!gate.update(ADVANCED, true, &blocks(0.6, false, 0.0), false), "with AGC on Discord activates at 0.8");
        assert!(gate.update(ADVANCED, true, &blocks(0.85, false, 0.0), false));

        let mut gate = Gate::default();
        assert!(!gate.update(WEBRTC, false, &blocks(1.0, false, 0.0), false), "without Advanced Voice Activity only WebRTC's VAD counts");
        assert!(gate.update(WEBRTC, false, &blocks(0.0, true, -90.0), false));
    }

    #[test]
    fn manual_threshold_and_push_to_talk() {
        let manual = InputMode::VoiceActivity { auto: false, threshold_db: -30.0, advanced: true };
        let mut gate = Gate::default();
        assert!(!gate.update(manual, false, &blocks(1.0, true, -35.0), false), "manual ignores the detectors");
        assert!(gate.update(manual, false, &blocks(0.0, false, -25.0), false));

        let ptt = InputMode::PushToTalk { release_delay_ms: 60_000 };
        let mut gate = Gate::default();
        let loud = blocks(1.0, true, 0.0);
        assert!(!gate.update(ptt, false, &loud, false), "loud but key not held");
        assert!(gate.update(ptt, false, &blocks(0.0, false, -100.0), true), "key held, even silent");
        assert!(gate.update(ptt, false, &loud, false), "still within the release delay");
        assert!(!gate.update(InputMode::PushToTalk { release_delay_ms: 0 }, false, &loud, false));
    }

    #[test]
    fn pre_roll_keeps_the_frames_before_an_onset() {
        let mut roll = PreRoll::<2>::default();
        assert_eq!(roll.drain().count(), 0);
        (1..=5).for_each(|i| roll.push(&[i as f32; 2]));
        let held: Vec<f32> = roll.drain().map(|f| f[0]).collect();
        assert_eq!(held, [3.0, 4.0, 5.0], "the last 3 frames (5 leading blocks), oldest first");
        assert_eq!(roll.drain().count(), 0, "draining empties it");
        roll.push(&[9.0; 2]);
        assert_eq!(roll.len(), 1);
        roll.clear();
        assert_eq!(roll.drain().count(), 0);
        assert_eq!(PRE_ROLL_FRAMES, 3);
    }
}
