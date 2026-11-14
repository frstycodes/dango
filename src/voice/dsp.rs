// Microphone processing: echo cancellation (AEC3), noise suppression and automatic gain (AGC2)
// from `sonora`, a pure-Rust port of WebRTC's audio processing. The speaker path feeds what is
// played (`render`) so the echo canceller knows what to remove from the mic (`capture`).
// Works in 10 ms mono blocks at 48 kHz; the mic is mono in practice, so stereo is folded down.
// ponytail: no `set_stream_delay_ms`; AEC3 estimates the speaker-to-mic delay itself.

use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};

use sonora::{
    AudioProcessing, Config, StreamConfig,
    config::{AdaptiveDigital, EchoCanceller, GainController2, NoiseSuppression},
};

use super::audio::SAMPLE_RATE;

// Interleaved stereo samples in one 10 ms block.
const BLOCK: usize = SAMPLE_RATE as usize / 100 * 2;

pub struct Processor {
    apm: Mutex<AudioProcessing>,
    echo: AtomicBool,
    noise: AtomicBool,
    gain: AtomicBool,
}

impl Processor {
    pub fn new(echo: bool, noise: bool, gain: bool) -> Self {
        let mono = StreamConfig::new(SAMPLE_RATE, 1);
        let apm = AudioProcessing::builder().config(config(echo, noise, gain)).capture_config(mono).render_config(mono).build();
        Self { apm: Mutex::new(apm), echo: AtomicBool::new(echo), noise: AtomicBool::new(noise), gain: AtomicBool::new(gain) }
    }

    pub fn set(&self, echo: Option<bool>, noise: Option<bool>, gain: Option<bool>) {
        let update = |flag: &AtomicBool, value: Option<bool>| value.map(|v| flag.store(v, Ordering::Relaxed));
        update(&self.echo, echo);
        update(&self.noise, noise);
        update(&self.gain, gain);
        if let Ok(mut apm) = self.apm.lock() {
            apm.apply_config(config(self.flag(&self.echo), self.flag(&self.noise), self.flag(&self.gain)));
        }
    }

    // What is about to be played, as interleaved 48 kHz stereo (any length of whole blocks).
    pub fn render(&self, stereo: &[f32]) {
        if !self.flag(&self.echo) {
            return;
        }
        let Ok(mut apm) = self.apm.lock() else { return };
        for block in stereo.as_chunks::<BLOCK>().0 {
            let mono = fold(block);
            let mut out = vec![0f32; mono.len()];
            let _ = apm.process_render_f32(&[&mono], &mut [&mut out]);
        }
    }

    // Cleans a mic frame in place (interleaved 48 kHz stereo, whole 10 ms blocks).
    pub fn capture(&self, stereo: &mut [f32]) {
        if !(self.flag(&self.echo) || self.flag(&self.noise) || self.flag(&self.gain)) {
            return;
        }
        let Ok(mut apm) = self.apm.lock() else { return };
        for block in stereo.as_chunks_mut::<BLOCK>().0 {
            let mono = fold(block);
            let mut out = vec![0f32; mono.len()];
            if apm.process_capture_f32(&[&mono], &mut [&mut out]).is_ok() {
                block.iter_mut().zip(out.iter().flat_map(|&s| [s, s])).for_each(|(dst, s)| *dst = s);
            }
        }
    }

    fn flag(&self, flag: &AtomicBool) -> bool {
        flag.load(Ordering::Relaxed)
    }
}

fn config(echo: bool, noise: bool, gain: bool) -> Config {
    Config {
        echo_canceller: echo.then(EchoCanceller::default),
        noise_suppression: noise.then(NoiseSuppression::default),
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
        let processor = Processor::new(false, true, false);
        let mut seed = 1u32;
        let mut noise = || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as f32 / (1 << 24) as f32 * 0.1 - 0.05
        };
        let rms = |s: &[f32]| (s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32).sqrt();
        let mut last = (0.0, 0.0);
        for _ in 0..200 {
            let mut frame: Vec<f32> = (0..BLOCK * 2).map(|_| noise()).collect();
            let before = rms(&frame);
            processor.capture(&mut frame);
            last = (before, rms(&frame));
        }
        assert!(last.1 < last.0 * 0.5, "white noise is attenuated after adapting: {last:?}");

        let mut untouched = vec![0.25; BLOCK * 2];
        Processor::new(false, false, false).capture(&mut untouched);
        assert!(untouched.iter().all(|&s| s == 0.25), "all processing off leaves audio alone");
    }
}
