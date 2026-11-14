// Microphone processing: echo cancellation (AEC3), noise suppression and automatic gain (AGC2)
// from `sonora`, a pure-Rust port of WebRTC's audio processing, plus RNNoise (`nnnoiseless`) as
// the ML noise suppressor. The speaker path feeds what is played (`render`) so the echo canceller
// knows what to remove from the mic (`capture`). Works in 10 ms mono blocks at 48 kHz, which is
// exactly one RNNoise frame; the mic is mono in practice, so stereo is folded down.
// ponytail: no `set_stream_delay_ms`; AEC3 estimates the speaker-to-mic delay itself.

use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicU8, Ordering},
};

use nnnoiseless::DenoiseState;
use sonora::{
    AudioProcessing, Config, StreamConfig,
    config::{AdaptiveDigital, EchoCanceller, GainController2},
};

use super::audio::SAMPLE_RATE;

// Interleaved stereo samples in one 10 ms block.
const BLOCK: usize = SAMPLE_RATE as usize / 100 * 2;
const _: () = assert!(BLOCK / 2 == DenoiseState::FRAME_SIZE, "a mono 10 ms block is one RNNoise frame");
// RNNoise works on 16-bit sample values held in floats.
const I16_SCALE: f32 = 32768.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NoiseSuppression {
    // RNNoise, the ML suppressor (Discord's Krisp slot).
    #[default]
    Advanced,
    // WebRTC's spectral noise suppressor.
    Standard,
    Off,
}

pub struct Processor {
    state: Mutex<State>,
    echo: AtomicBool,
    noise: AtomicU8,
    gain: AtomicBool,
}

struct State {
    apm: AudioProcessing,
    rnnoise: Box<DenoiseState<'static>>,
}

impl Processor {
    pub fn new(echo: bool, noise: NoiseSuppression, gain: bool) -> Self {
        let mono = StreamConfig::new(SAMPLE_RATE, 1);
        let apm = AudioProcessing::builder().config(config(echo, noise, gain)).capture_config(mono).render_config(mono).build();
        Self {
            state: Mutex::new(State { apm, rnnoise: DenoiseState::new() }),
            echo: AtomicBool::new(echo),
            noise: AtomicU8::new(noise as u8),
            gain: AtomicBool::new(gain),
        }
    }

    pub fn set(&self, echo: Option<bool>, noise: Option<NoiseSuppression>, gain: Option<bool>) {
        let update = |flag: &AtomicBool, value: Option<bool>| value.map(|v| flag.store(v, Ordering::Relaxed));
        update(&self.echo, echo);
        update(&self.gain, gain);
        if let Some(noise) = noise {
            self.noise.store(noise as u8, Ordering::Relaxed);
        }
        if let Ok(mut state) = self.state.lock() {
            state.apm.apply_config(config(self.flag(&self.echo), self.noise(), self.flag(&self.gain)));
        }
    }

    // What is about to be played, as interleaved 48 kHz stereo (any length of whole blocks).
    pub fn render(&self, stereo: &[f32]) {
        if !self.flag(&self.echo) {
            return;
        }
        let Ok(mut state) = self.state.lock() else { return };
        for block in stereo.as_chunks::<BLOCK>().0 {
            let mono = fold(block);
            let mut out = vec![0f32; mono.len()];
            let _ = state.apm.process_render_f32(&[&mono], &mut [&mut out]);
        }
    }

    // Cleans a mic frame in place (interleaved 48 kHz stereo, whole 10 ms blocks).
    pub fn capture(&self, stereo: &mut [f32]) {
        let noise = self.noise();
        if !(self.flag(&self.echo) || noise != NoiseSuppression::Off || self.flag(&self.gain)) {
            return;
        }
        let Ok(mut state) = self.state.lock() else { return };
        let State { apm, rnnoise } = &mut *state;
        for block in stereo.as_chunks_mut::<BLOCK>().0 {
            let mut mono = fold(block);
            let mut out = vec![0f32; mono.len()];
            if apm.process_capture_f32(&[&mono], &mut [&mut out]).is_err() {
                continue;
            }
            // RNNoise after AEC so it never sees the far end's echo as speech.
            if noise == NoiseSuppression::Advanced {
                mono.iter_mut().zip(&out).for_each(|(dst, s)| *dst = s * I16_SCALE);
                rnnoise.process_frame(&mut out, &mono);
                out.iter_mut().for_each(|s| *s = (*s / I16_SCALE).clamp(-1.0, 1.0));
            }
            block.iter_mut().zip(out.iter().flat_map(|&s| [s, s])).for_each(|(dst, s)| *dst = s);
        }
    }

    fn flag(&self, flag: &AtomicBool) -> bool {
        flag.load(Ordering::Relaxed)
    }

    fn noise(&self) -> NoiseSuppression {
        match self.noise.load(Ordering::Relaxed) {
            0 => NoiseSuppression::Advanced,
            1 => NoiseSuppression::Standard,
            _ => NoiseSuppression::Off,
        }
    }
}

fn config(echo: bool, noise: NoiseSuppression, gain: bool) -> Config {
    Config {
        echo_canceller: echo.then(EchoCanceller::default),
        noise_suppression: (noise == NoiseSuppression::Standard).then(sonora::config::NoiseSuppression::default),
        gain_controller2: gain.then(|| GainController2 { adaptive_digital: Some(AdaptiveDigital::default()), ..Default::default() }),
        ..Default::default()
    }
}

fn fold(stereo: &[f32]) -> Vec<f32> {
    stereo.as_chunks::<2>().0.iter().map(|[l, r]| (l + r) * 0.5).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_suppression_quiets_steady_noise() {
        let rms = |s: &[f32]| (s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32).sqrt();
        let mut seed = 1u32;
        let mut noise = || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as f32 / (1 << 24) as f32 * 0.1 - 0.05
        };
        let mut attenuation = |mode: NoiseSuppression| {
            let processor = Processor::new(false, mode, false);
            let mut last = (0.0, 0.0);
            for _ in 0..200 {
                let mut frame: Vec<f32> = (0..BLOCK * 2).map(|_| noise()).collect();
                let before = rms(&frame);
                processor.capture(&mut frame);
                last = (before, rms(&frame));
            }
            last.1 / last.0
        };
        let (standard, advanced) = (attenuation(NoiseSuppression::Standard), attenuation(NoiseSuppression::Advanced));
        assert!(standard < 0.5, "WebRTC attenuates white noise after adapting: {standard}");
        assert!(advanced < 0.5, "RNNoise attenuates white noise: {advanced}");

        let mut untouched = vec![0.25; BLOCK * 2];
        Processor::new(false, NoiseSuppression::Off, false).capture(&mut untouched);
        assert!(untouched.iter().all(|&s| s == 0.25), "all processing off leaves audio alone");

        let switched = Processor::new(false, NoiseSuppression::Off, false);
        switched.set(None, Some(NoiseSuppression::Standard), None);
        assert_eq!(switched.noise(), NoiseSuppression::Standard, "switches live");
    }
}
