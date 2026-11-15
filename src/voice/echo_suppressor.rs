// Residual echo suppressor after AEC3. While the near end talks, AEC3's suppressor goes transparent
// (and holds that for 200 ms after each word), leaving only its linear filter, which reaches ~15 dB
// in double talk: the far end hears itself in the gaps between our words. This models the echo's
// power spectrum at the mic from what was played (a non-negative filter over the last 0.5 s of
// render power in each frequency bin, which covers both the playback delay and the reverb tail,
// learned whenever only the far end talks) and attenuates the bins of AEC3's output that the
// modelled echo could account for. Echo underneath our own speech it leaves to masking: cutting
// those bins would cut our voice with it.
//
// Real-time: everything is allocated in `new`. Adds `HOP` samples (5 ms) of latency.

use std::sync::Arc;

use rustfft::{Fft, FftPlanner, num_complex::Complex32};

use super::dsp::BLOCK;

const FFT_LEN: usize = 480;
const HOP: usize = FFT_LEN / 2;
const BINS: usize = FFT_LEN / 2 + 1;
const _: () = assert!(BLOCK.is_multiple_of(HOP), "whole hops per 10 ms block");
// Render history the echo model spans: playback delay plus reverb tail, 5 ms per tap.
const TAPS: usize = 100;
// Model adaptation step (normalised).
const STEP: f32 = 0.05;
// Only the far end is talking when AEC3 took out more than ~17 dB; while the near end talks, its
// output stays within ~10 dB of the mic.
const FAR_ONLY_RATIO: f32 = 0.02;
// Render power (sum over bins, per hop) below which nothing is playing worth modelling.
const RENDER_ACTIVE: f32 = 1e-6;
// Share of the modelled echo power assumed to survive AEC3, and the deepest cut per bin (-30 dB).
// Tuned with the offline echo check on recorded speech: stronger costs the near end more than it
// removes echo.
const RESIDUAL: f32 = 0.05;
const FLOOR: f32 = 0.03;
// Per-hop recovery of a bin's gain after a cut: an instant snap back pumps the residual noise
// enough to read as speech to the voice-activity gate.
const RELEASE: f32 = 0.3;

pub struct EchoSuppressor {
    fft: Arc<dyn Fft<f32>>,
    ifft: Arc<dyn Fft<f32>>,
    window: [f32; FFT_LEN],
    scratch: Vec<Complex32>,
    spectrum: Vec<Complex32>,
    // Sliding analysis frames: what was played, the raw mic, and AEC3's output.
    render_frame: [f32; FFT_LEN],
    mic_frame: [f32; FFT_LEN],
    out_frame: [f32; FFT_LEN],
    // Render power per bin, newest at `head`.
    history: Vec<[f32; BINS]>,
    head: usize,
    weights: Vec<[f32; BINS]>,
    overlap: [f32; HOP],
    gain: [f32; BINS],
}

impl EchoSuppressor {
    pub fn new() -> Self {
        let mut planner = FftPlanner::new();
        let (fft, ifft) = (planner.plan_fft_forward(FFT_LEN), planner.plan_fft_inverse(FFT_LEN));
        let scratch_len = fft.get_inplace_scratch_len().max(ifft.get_inplace_scratch_len());
        // Periodic sqrt-Hann on analysis and synthesis: at 50% overlap the two sum to one.
        let window = std::array::from_fn(|i| (std::f32::consts::PI * i as f32 / FFT_LEN as f32).sin());
        Self {
            fft,
            ifft,
            window,
            scratch: vec![Complex32::default(); scratch_len],
            spectrum: vec![Complex32::default(); FFT_LEN],
            render_frame: [0.0; FFT_LEN],
            mic_frame: [0.0; FFT_LEN],
            out_frame: [0.0; FFT_LEN],
            history: vec![[0.0; BINS]; TAPS],
            head: 0,
            weights: vec![[0.0; BINS]; TAPS],
            overlap: [0.0; HOP],
            gain: [1.0; BINS],
        }
    }

    // One 10 ms block of what is being played (mono, as AEC3 gets it).
    pub fn render(&mut self, block: &[f32; BLOCK]) {
        for hop in block.as_chunks::<HOP>().0 {
            slide(&mut self.render_frame, hop);
            self.head = (self.head + 1) % TAPS;
            let mut power = [0.0; BINS];
            self.analyse(Frame::Render, &mut power);
            self.history[self.head] = power;
        }
    }

    // Cleans one 10 ms block of AEC3 output in place; `mic` is the same block before AEC3.
    pub fn capture(&mut self, mic: &[f32; BLOCK], out: &mut [f32; BLOCK]) {
        for (mic, out) in mic.as_chunks::<HOP>().0.iter().zip(out.as_chunks_mut::<HOP>().0) {
            slide(&mut self.mic_frame, mic);
            slide(&mut self.out_frame, out);
            self.hop(out);
        }
    }

    fn hop(&mut self, out: &mut [f32; HOP]) {
        let (mut mic_power, mut out_power) = ([0.0; BINS], [0.0; BINS]);
        self.analyse(Frame::Mic, &mut mic_power);
        self.analyse(Frame::Out, &mut out_power);
        let echo = self.estimate();
        let far_only = out_power.iter().sum::<f32>() < FAR_ONLY_RATIO * mic_power.iter().sum::<f32>();
        if far_only && self.history[self.head].iter().sum::<f32>() > RENDER_ACTIVE {
            self.learn(&mic_power, &echo);
        }
        let target = gains(&echo, &out_power);
        for (gain, target) in self.gain.iter_mut().zip(target) {
            *gain = if target < *gain { target } else { *gain + RELEASE * (target - *gain) };
        }
        self.synthesise(out);
    }

    // Echo power at the mic per bin, from the render history.
    fn estimate(&self) -> [f32; BINS] {
        let mut echo = [0.0; BINS];
        for (tap, weights) in self.weights.iter().enumerate() {
            let past = &self.history[(self.head + TAPS - tap) % TAPS];
            echo.iter_mut().zip(weights).zip(past).for_each(|((e, w), x)| *e += w * x);
        }
        echo
    }

    // Normalised LMS on power, weights kept non-negative.
    fn learn(&mut self, mic_power: &[f32; BINS], echo: &[f32; BINS]) {
        let mut norm = [1e-12; BINS];
        for tap in 0..TAPS {
            let past = &self.history[(self.head + TAPS - tap) % TAPS];
            norm.iter_mut().zip(past).for_each(|(n, x)| *n += x * x);
        }
        for tap in 0..TAPS {
            let past = self.history[(self.head + TAPS - tap) % TAPS];
            for k in 0..BINS {
                let step = STEP * (mic_power[k] - echo[k]) * past[k] / norm[k];
                self.weights[tap][k] = (self.weights[tap][k] + step).max(0.0);
            }
        }
    }

    fn analyse(&mut self, frame: Frame, power: &mut [f32; BINS]) {
        let samples = match frame {
            Frame::Render => &self.render_frame,
            Frame::Mic => &self.mic_frame,
            Frame::Out => &self.out_frame,
        };
        self.spectrum.iter_mut().zip(samples.iter().zip(&self.window)).for_each(|(c, (s, w))| *c = Complex32::new(s * w, 0.0));
        self.fft.process_with_scratch(&mut self.spectrum, &mut self.scratch);
        power.iter_mut().zip(&self.spectrum).for_each(|(p, c)| *p = c.norm_sqr());
    }

    // `spectrum` still holds AEC3's output from the last `analyse(Frame::Out)`.
    fn synthesise(&mut self, out: &mut [f32; HOP]) {
        for (k, c) in self.spectrum.iter_mut().enumerate() {
            *c *= self.gain[k.min(FFT_LEN - k)];
        }
        self.ifft.process_with_scratch(&mut self.spectrum, &mut self.scratch);
        let scale = 1.0 / FFT_LEN as f32;
        let frame: [f32; FFT_LEN] = std::array::from_fn(|i| self.spectrum[i].re * scale * self.window[i]);
        out.iter_mut().zip(&self.overlap).zip(&frame[..HOP]).for_each(|((o, prev), now)| *o = prev + now);
        self.overlap.copy_from_slice(&frame[HOP..]);
    }
}

// Per bin: what is left once the modelled residual echo is taken out of AEC3's output.
fn gains(echo: &[f32; BINS], out_power: &[f32; BINS]) -> [f32; BINS] {
    std::array::from_fn(|k| {
        // Window leakage spreads a bin's echo into its neighbours.
        let nearby = echo[k.saturating_sub(1)].max(echo[k]).max(echo[(k + 1).min(BINS - 1)]);
        (1.0 - RESIDUAL * nearby / (out_power[k] + 1e-12)).clamp(FLOOR, 1.0)
    })
}

enum Frame {
    Render,
    Mic,
    Out,
}

fn slide(frame: &mut [f32; FFT_LEN], hop: &[f32; HOP]) {
    frame.copy_within(HOP.., 0);
    frame[FFT_LEN - HOP..].copy_from_slice(hop);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voice::fixtures::{scaled_to, talker};

    // Far end only, echoing back 30 ms later at half amplitude: the model should predict the mic's
    // power, and the output should be cut well below the input.
    #[test]
    fn learns_a_delayed_echo() {
        let far = scaled_to(talker(6.0, 197.0, 1), 0.05);
        let delay = 1440;
        let mut s = EchoSuppressor::new();
        let (mut mic_energy, mut out_energy, mut predicted, mut actual) = (0.0, 0.0, 0.0, 0.0);
        for (b, block) in far.as_chunks::<BLOCK>().0.iter().enumerate() {
            s.render(block);
            let mic: [f32; BLOCK] = std::array::from_fn(|i| {
                let t = b * BLOCK + i;
                if t >= delay { 0.5 * far[t - delay] } else { 0.0 }
            });
            // AEC3 has taken out 40 dB.
            let mut out = mic.map(|x| 0.01 * x);
            s.capture(&mic, &mut out);
            if b > 400 {
                mic_energy += mic.iter().map(|x| x * x).sum::<f32>();
                out_energy += out.iter().map(|x| x * x).sum::<f32>();
                predicted += s.estimate().iter().sum::<f32>();
                let mut power = [0.0; BINS];
                s.analyse(Frame::Mic, &mut power);
                actual += power.iter().sum::<f32>();
            }
        }
        println!("predicted/actual {:.2}, cut {:.1} dB", predicted / actual, 10.0 * (mic_energy / out_energy).log10());
        assert!((0.5..2.0).contains(&(predicted / actual)), "echo model tracks the mic: {}", predicted / actual);
    }
}
