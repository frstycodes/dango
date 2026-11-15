// Deterministic test signals for the audio chain.

use super::audio::SAMPLE_RATE;

pub struct Noise(pub u32);

impl Noise {
    // Uniform white noise in -amplitude..amplitude.
    pub fn next(&mut self, amplitude: f32) -> f32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        ((self.0 >> 8) as f32 / (1 << 24) as f32 * 2.0 - 1.0) * amplitude
    }
}

// Voiced only: noise suppressors rightly treat breath and fricatives as noise.
pub fn speech(seconds: f32) -> Vec<f32> {
    voice(seconds, 140.0, 0, false)
}

pub fn talker(seconds: f32, f0: f32, voice_id: usize) -> Vec<f32> {
    voice(seconds, f0, voice_id, true)
}

// A speech-like fixture: a glottal pulse train (f0 gliding ±30 Hz around `f0`) through two vowel
// formants that change every syllable, in ~4 Hz syllables separated by pauses. `breathy` adds pitch
// jitter, breath noise and a fricative (high-passed noise) every third syllable. `voice` picks a different vowel order,
// syllable rate, pitch contour, pause rhythm and noise, so two talkers stay uncorrelated (locked
// pitch contours would line their harmonics up). Echo cancellers refuse to adapt on purely tonal
// audio, so they need the breathy kind.
fn voice(seconds: f32, f0: f32, voice: usize, breathy: bool) -> Vec<f32> {
    let rate = SAMPLE_RATE as f32;
    let vowels = [(730.0, 1090.0), (270.0, 2290.0), (530.0, 1840.0), (300.0, 870.0)];
    let mut resonators = [Resonator::default(), Resonator::default(), Resonator::default()];
    let mut noise = Noise(voice as u32 + 1);
    let mut phase = 0.0f32;
    let (syllable_rate, glide_rate) = (4.0 - 0.7 * voice as f32, 1.3 + 0.37 * voice as f32);
    (0..(seconds * rate) as usize)
        .map(|i| {
            let t = i as f32 / rate;
            let syllable = (t * syllable_rate) as usize + voice * 3;
            let within = t * syllable_rate - (t * syllable_rate).floor();
            let envelope = if syllable % (5 + voice) == 4 { 0.0 } else { (std::f32::consts::PI * within).sin().powi(2) };
            if breathy && syllable % 3 == 2 {
                return 0.3 * resonators[2].tick(noise.next(1.0), 5000.0, 2500.0) * envelope;
            }
            let jitter = if breathy { noise.next(0.02) } else { 0.0 };
            phase += (f0 + 30.0 * (t * glide_rate + voice as f32 * 2.0).sin()) / rate * (1.0 + jitter);
            let pulse = if phase >= 1.0 {
                phase -= 1.0;
                1.0
            } else {
                0.0
            };
            let excitation = pulse + if breathy { noise.next(0.08) } else { 0.0 };
            let (f1, f2) = vowels[(syllable + voice) % vowels.len()];
            let voiced = resonators[0].tick(excitation, f1, 80.0) + 0.5 * resonators[1].tick(excitation, f2, 120.0);
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

pub fn rms(s: &[f32]) -> f32 {
    (s.iter().map(|x| x * x).sum::<f32>() / s.len().max(1) as f32).sqrt()
}

pub fn scaled_to(signal: Vec<f32>, target_rms: f32) -> Vec<f32> {
    let gain = target_rms / rms(&signal);
    signal.into_iter().map(|s| s * gain).collect()
}
