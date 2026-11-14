// Microphone processing in Discord's order (docs/discord-audio.md): echo cancellation (AEC3) and
// the Standard suppressor (WebRTC NS) from `sonora`, then the Krisp-tier suppressor (DeepFilterNet3,
// RNNoise while it loads or if it can't keep up), then automatic gain (AGC2 with Discord's
// config). Each 10 ms block is also analysed for voice activity: RNNoise's speech probability
// (Advanced Voice Activity, Discord's Krisp VAD slot) and WebRTC's GMM VAD (automatic sensitivity
// without it).
//
// Real-time: `render` runs on the output device callback and only copies a block into a bounded,
// preallocated channel. `capture` runs on the voice send thread with preallocated buffers.
// DeepFilterNet's tract graph allocates its own tensors every hop, so it runs on a worker thread of
// its own, never on a device callback.

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU8, Ordering},
        mpsc::{Receiver, SyncSender, TryRecvError, sync_channel},
    },
    thread,
    time::{Duration, Instant},
};

use df::tract::{DfParams, DfTract, RuntimeParams};
use earshot::{VoiceActivityDetector, VoiceActivityProfile};
use ndarray::{ArrayView2, ArrayViewMut2};
use nnnoiseless::DenoiseState;
use sonora::{
    AudioProcessing, Config, StreamConfig,
    config::{AdaptiveDigital, EchoCanceller, GainController2},
};

use super::audio::{FRAME_LEN, SAMPLE_RATE, SILENCE_DB, level_db};

// Mono samples in one 10 ms block: one RNNoise frame, one DeepFilterNet hop, one WebRTC block.
pub const BLOCK: usize = SAMPLE_RATE as usize / 100;
// 10 ms blocks in one 20 ms Opus frame.
pub const BLOCKS_PER_FRAME: usize = FRAME_LEN / 2 / BLOCK;
const _: () = assert!(BLOCK == DenoiseState::FRAME_SIZE, "a 10 ms block is one RNNoise frame");
// RNNoise and the WebRTC VAD work on 16-bit sample values.
const I16_SCALE: f32 = 32768.0;
// Played audio waiting for the echo canceller: 500 ms, after which the oldest is dropped.
const RENDER_QUEUE_BLOCKS: usize = 50;
// Discord reports KRISP_CPU_OVERUSE when its suppressor averages over 8 ms per 10 ms frame across
// more than 50 frames; past that we fall back to RNNoise, as Discord falls back to Standard.
const OVERRUN_PER_BLOCK: Duration = Duration::from_millis(8);
const OVERRUN_WINDOW: u32 = 100;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NoiseSuppression {
    // DeepFilterNet3: Discord's Krisp slot.
    #[default]
    Advanced,
    // WebRTC's spectral noise suppressor.
    Standard,
    Off,
}

// What one 10 ms block of processed mic audio holds, for the voice-activity gate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Block {
    // dBFS after processing.
    pub db: f32,
    // RNNoise's speech probability, 0..=1.
    pub speech: f32,
    // WebRTC's VAD in its very aggressive mode.
    pub vad: bool,
}

impl Default for Block {
    fn default() -> Self {
        Self { db: SILENCE_DB, speech: 0.0, vad: false }
    }
}

pub struct Processor {
    state: Mutex<State>,
    render_tx: SyncSender<[f32; BLOCK]>,
    echo: AtomicBool,
    noise: AtomicU8,
    gain: AtomicBool,
}

struct State {
    render_rx: Receiver<[f32; BLOCK]>,
    // AEC3 and, when chosen, the Standard suppressor.
    front: AudioProcessing,
    // AGC2 alone, so it runs after the neural suppressor.
    back: AudioProcessing,
    neural: Neural,
    vad: Box<VoiceActivityDetector>,
    mono: [f32; BLOCK],
    out: [f32; BLOCK],
    scaled: [f32; BLOCK],
    pcm: [i16; BLOCK],
}

impl Processor {
    pub fn new(echo: bool, noise: NoiseSuppression, gain: bool) -> Self {
        let mono = StreamConfig::new(SAMPLE_RATE, 1);
        let front = AudioProcessing::builder().config(front_config(echo, noise)).capture_config(mono).render_config(mono).build();
        let back = AudioProcessing::builder().config(back_config(gain)).capture_config(mono).render_config(mono).build();
        let (render_tx, render_rx) = sync_channel(RENDER_QUEUE_BLOCKS);
        let mut neural = Neural::new();
        if noise == NoiseSuppression::Advanced {
            neural.load();
        }
        let state = State {
            render_rx,
            front,
            back,
            neural,
            vad: Box::new(VoiceActivityDetector::new(VoiceActivityProfile::VERY_AGGRESSIVE)),
            mono: [0.0; BLOCK],
            out: [0.0; BLOCK],
            scaled: [0.0; BLOCK],
            pcm: [0; BLOCK],
        };
        Self { state: Mutex::new(state), render_tx, echo: AtomicBool::new(echo), noise: AtomicU8::new(noise as u8), gain: AtomicBool::new(gain) }
    }

    pub fn set(&self, echo: Option<bool>, noise: Option<NoiseSuppression>, gain: Option<bool>) {
        let update = |flag: &AtomicBool, value: Option<bool>| value.map(|v| flag.store(v, Ordering::Relaxed));
        update(&self.echo, echo);
        update(&self.gain, gain);
        if let Some(noise) = noise {
            self.noise.store(noise as u8, Ordering::Relaxed);
        }
        if let Ok(mut state) = self.state.lock() {
            state.front.apply_config(front_config(self.echo(), self.noise()));
            state.back.apply_config(back_config(self.gain()));
        }
    }

    pub fn gain(&self) -> bool {
        self.gain.load(Ordering::Relaxed)
    }

    // True once DeepFilterNet fell behind real time and RNNoise took over for this session.
    pub fn degraded(&self) -> bool {
        self.state.lock().is_ok_and(|s| s.neural.degraded)
    }

    // What is about to be played, as interleaved 48 kHz stereo (whole 10 ms blocks). Called from the
    // output device callback: no locks, no allocation; a full queue drops the block.
    pub fn render(&self, stereo: &[f32]) {
        if !self.echo() {
            return;
        }
        for block in stereo.as_chunks::<{ BLOCK * 2 }>().0 {
            let mut mono = [0.0; BLOCK];
            fold(block, &mut mono);
            let _ = self.render_tx.try_send(mono);
        }
    }

    // Cleans one 20 ms mic frame in place (interleaved 48 kHz stereo) and analyses each 10 ms block.
    pub fn capture(&self, frame: &mut [f32; FRAME_LEN]) -> [Block; BLOCKS_PER_FRAME] {
        let mut blocks = [Block::default(); BLOCKS_PER_FRAME];
        let (echo, noise, gain) = (self.echo(), self.noise(), self.gain());
        let Ok(mut state) = self.state.lock() else { return blocks };
        let state = &mut *state;
        state.drain_render(echo);
        for (stereo, block) in frame.as_chunks_mut::<{ BLOCK * 2 }>().0.iter_mut().zip(&mut blocks) {
            fold(stereo, &mut state.mono);
            let touched = state.process(echo, noise, gain, block);
            if touched {
                stereo.as_chunks_mut::<2>().0.iter_mut().zip(&state.mono).for_each(|(pair, &s)| *pair = [s, s]);
            }
            block.db = level_db(&state.mono);
        }
        blocks
    }

    fn echo(&self) -> bool {
        self.echo.load(Ordering::Relaxed)
    }

    fn noise(&self) -> NoiseSuppression {
        match self.noise.load(Ordering::Relaxed) {
            0 => NoiseSuppression::Advanced,
            1 => NoiseSuppression::Standard,
            _ => NoiseSuppression::Off,
        }
    }

    #[cfg(test)]
    fn wait_for_model(&self) {
        let deadline = Instant::now() + Duration::from_secs(60);
        while self.state.lock().is_ok_and(|s| !s.neural.ready()) {
            assert!(Instant::now() < deadline, "DeepFilterNet did not load");
            thread::sleep(Duration::from_millis(20));
        }
    }
}

impl State {
    fn drain_render(&mut self, echo: bool) {
        loop {
            match self.render_rx.try_recv() {
                Ok(block) if echo => {
                    let _ = self.front.process_render_f32(&[&block], &mut [&mut self.out]);
                }
                Ok(_) => {}
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return,
            }
        }
    }

    // One 10 ms block of `self.mono` through the chain; false when nothing changed the audio.
    fn process(&mut self, echo: bool, noise: NoiseSuppression, gain: bool, block: &mut Block) -> bool {
        if (echo || noise == NoiseSuppression::Standard) && self.front.process_capture_f32(&[&self.mono], &mut [&mut self.out]).is_ok() {
            self.mono = self.out;
        }
        // Speech probability comes from the echo-cancelled signal, before any suppression.
        self.mono.iter().zip(&mut self.scaled).for_each(|(s, dst)| *dst = s * I16_SCALE);
        block.speech = self.neural.rnnoise.process_frame(&mut self.out, &self.scaled);
        if noise == NoiseSuppression::Advanced {
            if !self.neural.denoise(&self.mono, &mut self.out) {
                self.out.iter_mut().for_each(|s| *s /= I16_SCALE);
            }
            self.mono.iter_mut().zip(&self.out).for_each(|(dst, s)| *dst = s.clamp(-1.0, 1.0));
        }
        if gain && self.back.process_capture_f32(&[&self.mono], &mut [&mut self.out]).is_ok() {
            self.mono = self.out;
        }
        self.mono.iter().zip(&mut self.pcm).for_each(|(s, dst)| *dst = (s * I16_SCALE).clamp(-I16_SCALE, I16_SCALE - 1.0) as i16);
        block.vad = self.vad.predict_48khz(&self.pcm).unwrap_or(false);
        echo || noise != NoiseSuppression::Off || gain
    }
}

// The Krisp-tier suppressor. DeepFilterNet's tract graph is not `Send`, so it lives on a worker
// thread of its own, which loads the model (it takes a moment to optimise) and then cleans one block
// per request. RNNoise covers until it is ready, and for good once it falls behind real time.
struct Neural {
    rnnoise: Box<DenoiseState<'static>>,
    dfn: Option<Worker>,
    overrun: Overrun,
    degraded: bool,
}

struct Worker {
    ready: Arc<AtomicBool>,
    requests: SyncSender<[f32; BLOCK]>,
    replies: Receiver<Option<[f32; BLOCK]>>,
}

impl Neural {
    fn new() -> Self {
        Self { rnnoise: DenoiseState::new(), dfn: None, overrun: Overrun::default(), degraded: false }
    }

    fn load(&mut self) {
        if self.dfn.is_some() || self.degraded {
            return;
        }
        let ready = Arc::new(AtomicBool::new(false));
        let (requests, inbox) = sync_channel(1);
        let (outbox, replies) = sync_channel(1);
        let flag = ready.clone();
        let spawned = thread::Builder::new().name("rustcord-deepfilternet".into()).spawn(move || run_worker(&flag, &inbox, &outbox));
        if spawned.is_ok() {
            self.dfn = Some(Worker { ready, requests, replies });
        }
    }

    #[cfg(test)]
    fn ready(&self) -> bool {
        self.dfn.as_ref().is_some_and(|w| w.ready.load(Ordering::Acquire))
    }

    // DeepFilterNet into `out` (true), or false when RNNoise's output, already in `out` at 16-bit
    // scale, stands in.
    fn denoise(&mut self, mono: &[f32; BLOCK], out: &mut [f32; BLOCK]) -> bool {
        self.load();
        let Some(worker) = &self.dfn else { return false };
        if !worker.ready.load(Ordering::Acquire) {
            return false;
        }
        let started = Instant::now();
        let enhanced = worker.requests.send(*mono).ok().and_then(|()| worker.replies.recv().ok().flatten());
        let Some(enhanced) = enhanced else {
            self.dfn = None;
            self.degraded = true;
            return false;
        };
        if self.overrun.record(started.elapsed()) {
            eprintln!("voice: DeepFilterNet can't keep up on this CPU; switching to RNNoise");
            self.dfn = None;
            self.degraded = true;
        }
        *out = enhanced;
        true
    }
}

// Owns the model; ends when the processor drops its side of the channels.
fn run_worker(ready: &AtomicBool, inbox: &Receiver<[f32; BLOCK]>, outbox: &SyncSender<Option<[f32; BLOCK]>>) {
    let mut model = match DfTract::new(DfParams::default(), &RuntimeParams::default_with_ch(1)) {
        Ok(model) => model,
        Err(e) => return eprintln!("voice: DeepFilterNet failed to load, using RNNoise: {e:?}"),
    };
    ready.store(true, Ordering::Release);
    for noisy in inbox {
        let mut enhanced = [0.0; BLOCK];
        let processed = match (ArrayView2::from_shape((1, BLOCK), &noisy[..]), ArrayViewMut2::from_shape((1, BLOCK), &mut enhanced[..])) {
            (Ok(noisy), Ok(clean)) => model.process(noisy, clean).is_ok(),
            _ => false,
        };
        if outbox.send(processed.then_some(enhanced)).is_err() {
            return;
        }
    }
}

// Average suppressor time per 10 ms block over a window, as Discord's CPU-overuse check does.
#[derive(Default)]
struct Overrun {
    spent: Duration,
    blocks: u32,
}

impl Overrun {
    fn record(&mut self, spent: Duration) -> bool {
        self.spent += spent;
        self.blocks += 1;
        if self.blocks < OVERRUN_WINDOW {
            return false;
        }
        let over = self.spent / self.blocks > OVERRUN_PER_BLOCK;
        *self = Self::default();
        over
    }
}

fn front_config(echo: bool, noise: NoiseSuppression) -> Config {
    Config {
        echo_canceller: echo.then(EchoCanceller::default),
        noise_suppression: (noise == NoiseSuppression::Standard).then(sonora::config::NoiseSuppression::default),
        ..Default::default()
    }
}

// Discord's AGC2 (MediaEngineStore `eQ`): adaptive digital only, 5 dB headroom, 50 dB max gain,
// 15 dB initial, 6 dB/s, -50 dBFS max output noise. `AdaptiveDigital::default()` is exactly that.
fn back_config(gain: bool) -> Config {
    Config {
        gain_controller2: gain.then(|| GainController2 { adaptive_digital: Some(AdaptiveDigital::default()), ..Default::default() }),
        ..Default::default()
    }
}

fn fold(stereo: &[f32; BLOCK * 2], mono: &mut [f32; BLOCK]) {
    stereo.as_chunks::<2>().0.iter().zip(mono).for_each(|([l, r], dst)| *dst = (l + r) * 0.5);
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Noise(u32);

    impl Noise {
        // Uniform white noise in -amplitude..amplitude.
        fn next(&mut self, amplitude: f32) -> f32 {
            self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            ((self.0 >> 8) as f32 / (1 << 24) as f32 * 2.0 - 1.0) * amplitude
        }
    }

    // A speech-like fixture: a glottal pulse train (f0 gliding 110-170 Hz) through two vowel
    // formants that change every syllable, in 4 Hz syllables separated by pauses.
    fn speech(seconds: f32) -> Vec<f32> {
        let rate = SAMPLE_RATE as f32;
        let vowels = [(730.0, 1090.0), (270.0, 2290.0), (530.0, 1840.0), (300.0, 870.0)];
        let mut resonators = [Resonator::default(), Resonator::default()];
        let mut phase = 0.0f32;
        (0..(seconds * rate) as usize)
            .map(|i| {
                let t = i as f32 / rate;
                let syllable = (t * 4.0) as usize;
                let within = t * 4.0 - syllable as f32;
                let envelope = if syllable % 5 == 4 { 0.0 } else { (std::f32::consts::PI * within).sin().powi(2) };
                let f0 = 140.0 + 30.0 * (t * 1.3).sin();
                phase += f0 / rate;
                let pulse = if phase >= 1.0 {
                    phase -= 1.0;
                    1.0
                } else {
                    0.0
                };
                let (f1, f2) = vowels[syllable % vowels.len()];
                let voiced = resonators[0].tick(pulse, f1, 80.0) + 0.5 * resonators[1].tick(pulse, f2, 120.0);
                voiced * envelope
            })
            .collect()
    }

    #[derive(Default)]
    struct Resonator(f32, f32);

    impl Resonator {
        fn tick(&mut self, x: f32, freq: f32, bandwidth: f32) -> f32 {
            let rate = SAMPLE_RATE as f32;
            let r = (-std::f32::consts::PI * bandwidth / rate).exp();
            let y = x + 2.0 * r * (2.0 * std::f32::consts::PI * freq / rate).cos() * self.0 - r * r * self.1;
            (self.1, self.0) = (self.0, y);
            y
        }
    }

    fn rms(s: &[f32]) -> f32 {
        (s.iter().map(|x| x * x).sum::<f32>() / s.len().max(1) as f32).sqrt()
    }

    fn scaled_to(signal: Vec<f32>, target_rms: f32) -> Vec<f32> {
        let gain = target_rms / rms(&signal);
        signal.into_iter().map(|s| s * gain).collect()
    }

    // Runs mono audio through a processor in 20 ms frames.
    fn run(processor: &Processor, mono: &[f32]) -> (Vec<f32>, Vec<Block>) {
        let (mut out, mut blocks) = (Vec::with_capacity(mono.len()), Vec::new());
        for chunk in mono.as_chunks::<{ FRAME_LEN / 2 }>().0 {
            let mut frame = [0.0; FRAME_LEN];
            frame.as_chunks_mut::<2>().0.iter_mut().zip(chunk).for_each(|(pair, &s)| *pair = [s, s]);
            blocks.extend(processor.capture(&mut frame));
            out.extend(frame.as_chunks::<2>().0.iter().map(|[l, _]| *l));
        }
        (out, blocks)
    }

    // SNR of `output` against `clean`, after finding the processing delay (on one second) and the
    // best gain (on everything after the first second, which lets adaptation settle).
    fn snr_db(clean: &[f32], output: &[f32]) -> f32 {
        const MAX_DELAY: usize = 2400;
        let rate = SAMPLE_RATE as usize;
        let len = clean.len().min(output.len()) - rate - MAX_DELAY;
        let dot = |delay: usize, n: usize| -> f32 { clean[rate..rate + n].iter().zip(&output[rate + delay..]).map(|(a, b)| a * b).sum() };
        let delay = (0..MAX_DELAY).max_by(|&a, &b| dot(a, rate).total_cmp(&dot(b, rate))).unwrap_or(0);
        let (c, o) = (&clean[rate..rate + len], &output[rate + delay..rate + delay + len]);
        let gain = dot(delay, len) / c.iter().map(|a| a * a).sum::<f32>();
        let error: f32 = c.iter().zip(o).map(|(a, b)| (b - gain * a).powi(2)).sum();
        let signal: f32 = c.iter().map(|a| (gain * a).powi(2)).sum();
        10.0 * (signal / error).log10()
    }

    #[test]
    fn deep_filter_net_removes_noise_from_speech() {
        let clean = scaled_to(speech(6.0), 0.1);
        let mut noise = Noise(7);
        let noisy: Vec<f32> = clean.iter().map(|s| s + noise.next(0.1 * 3f32.sqrt())).collect();
        let before = snr_db(&clean, &noisy);

        let processor = Processor::new(false, NoiseSuppression::Advanced, false);
        processor.wait_for_model();
        let (enhanced, _) = run(&processor, &noisy);
        let after = snr_db(&clean, &enhanced);
        println!("DeepFilterNet: SNR {before:.1} dB -> {after:.1} dB");
        assert!(before.abs() < 1.0, "fixture is mixed at 0 dB SNR: {before}");
        assert!(after - before > 6.0, "DeepFilterNet improves SNR by more than 6 dB: {before:.1} -> {after:.1}");
    }

    #[test]
    fn suppressors_quiet_steady_noise_and_off_leaves_audio_alone() {
        let mut noise = Noise(1);
        let white: Vec<f32> = (0..SAMPLE_RATE as usize * 2).map(|_| noise.next(0.05)).collect();
        let tail = |s: &[f32]| rms(&s[s.len() - SAMPLE_RATE as usize / 2..]);
        for mode in [NoiseSuppression::Standard, NoiseSuppression::Advanced] {
            let processor = Processor::new(false, mode, false);
            if mode == NoiseSuppression::Advanced {
                processor.wait_for_model();
            }
            let (out, _) = run(&processor, &white);
            let ratio = tail(&out) / tail(&white);
            assert!(ratio < 0.5, "{mode:?} attenuates white noise: {ratio}");
        }

        let mut untouched = [0.25; FRAME_LEN];
        Processor::new(false, NoiseSuppression::Off, false).capture(&mut untouched);
        assert!(untouched.iter().all(|&s| s == 0.25), "all processing off leaves audio alone");

        let switched = Processor::new(false, NoiseSuppression::Off, false);
        switched.set(None, Some(NoiseSuppression::Standard), None);
        assert_eq!(switched.noise(), NoiseSuppression::Standard, "switches live");
    }

    #[test]
    fn analysis_tells_speech_from_silence() {
        let processor = Processor::new(false, NoiseSuppression::Off, false);
        let (_, silent) = run(&processor, &vec![0.0; SAMPLE_RATE as usize]);
        assert!(silent.iter().all(|b| !b.vad && b.speech < 0.1 && b.db == SILENCE_DB));
        let (_, voiced) = run(&processor, &scaled_to(speech(3.0), 0.1));
        let heard = |f: fn(&Block) -> bool| voiced.iter().filter(|b| f(b)).count() as f32 / voiced.len() as f32;
        assert!(heard(|b| b.vad) > 0.3, "WebRTC VAD hears the syllables");
        assert!(voiced.iter().any(|b| b.speech > 0.5), "RNNoise gives speech a high probability");
    }

    #[test]
    fn agc_matches_discords_config() {
        let discord = AdaptiveDigital {
            headroom_db: 5.0,
            max_gain_db: 50.0,
            initial_gain_db: 15.0,
            max_gain_change_db_per_second: 6.0,
            max_output_noise_level_dbfs: -50.0,
        };
        let config = back_config(true).gain_controller2.unwrap();
        assert_eq!(config.adaptive_digital, Some(discord));
        assert!(!config.input_volume_controller, "Discord: enableAnalog false");
    }

    #[test]
    fn overrun_trips_only_on_a_slow_average() {
        let mut meter = Overrun::default();
        assert!(!(0..OVERRUN_WINDOW).any(|_| meter.record(Duration::from_millis(3))));
        let tripped = (0..OVERRUN_WINDOW).map(|_| meter.record(Duration::from_millis(9))).last();
        assert_eq!(tripped, Some(true));
    }

    // Per-frame cost of the full chain; run with `--ignored --nocapture` to see the numbers.
    #[test]
    #[ignore = "timing benchmark"]
    fn bench_per_frame_cost() {
        let mut noise = Noise(3);
        let audio: Vec<f32> = scaled_to(speech(10.0), 0.1).iter().map(|s| s + noise.next(0.05)).collect();
        for (label, echo, mode, gain) in [
            ("off", false, NoiseSuppression::Off, false),
            ("standard + AEC + AGC", true, NoiseSuppression::Standard, true),
            ("deepfilternet", false, NoiseSuppression::Advanced, false),
            ("deepfilternet + AEC + AGC", true, NoiseSuppression::Advanced, true),
        ] {
            let loading = Instant::now();
            let processor = Processor::new(echo, mode, gain);
            if mode == NoiseSuppression::Advanced {
                processor.wait_for_model();
                println!("{label}: model ready after {:?}", loading.elapsed());
            }
            let frames = audio.len() / (FRAME_LEN / 2);
            let started = Instant::now();
            run(&processor, &audio);
            let per_frame = started.elapsed() / frames as u32;
            println!("{label}: {per_frame:?} per 20 ms frame ({:.1}% of real time)", per_frame.as_secs_f64() / 0.020 * 100.0);
        }
    }
}
