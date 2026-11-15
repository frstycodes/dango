// Offline echo check: a far-end talker goes through the real speaker path (mixer → `Playback`, which
// feeds the echo canceller) into a simulated room (device buffers and latency, speaker, direct path,
// reverb, mic clock), is mixed with a near-end talker at the mic, and comes back through the real mic
// path (device chunks → 20 ms frames → `process_mic` → voice-activity gate). Reports echo return
// loss enhancement (ERLE) with the near end silent (single talk) and talking (double talk).
//
// cargo test --release echo_check -- --nocapture --include-ignored --test-threads=1
// With recorded voices: ECHO_FAR=far.wav ECHO_NEAR=near.wav (48 kHz mono float WAV; macOS:
// `say -o far.wav --data-format=LEF32@48000 "..."`). Synthetic talkers are kinder to AEC3 than
// recorded speech: double talk reads ~23 dB with them and ~13 dB with `say` voices.

use rustfft::{FftPlanner, num_complex::Complex32};

use super::{
    DEFAULT_INPUT_MODE, NoiseSuppression,
    audio::{AudioSettings, FRAME_LEN, Mixer, Playback, Resampler, SAMPLE_RATE, to_stereo},
    dsp::Processor,
    fixtures::{Noise, scaled_to, talker},
    vad::Gate,
};

const RATE: usize = SAMPLE_RATE as usize;
const SECONDS: usize = 16;
// Far end alone for the first half, both talking in the second.
const DOUBLE_TALK_FROM: usize = 8 * RATE;
// Skipped at the start of each half while the canceller adapts.
const SETTLE: usize = 3 * RATE;
// Talker levels with the fixture's ~22 dB crest factor in mind: peaks stay under full scale at the
// mic, as a sensibly set input level does (a clipped mic makes echo cancellers stop adapting).
const FAR_RMS: f32 = 0.06;
const NEAR_RMS: f32 = 0.04;
// Far-end audio is pushed into the mixer this far ahead of playback, like a jitter buffer.
const NETWORK_LEAD: usize = FRAME_LEN;

pub struct Report {
    pub single_talk_erle_db: f32,
    pub double_talk_erle_db: f32,
    // ERLE over the gaps between the near end's words during double talk, where nothing masks the
    // echo.
    pub gaps_erle_db: f32,
    // Near-end level kept through the chain in double talk (0 dB = untouched).
    pub near_kept_db: f32,
    // Share of 20 ms frames in single talk where the echo alone opened the voice gate.
    pub gate_open_single_talk: f32,
}

// The acoustic and clock conditions of one simulated call.
#[derive(Clone, Copy, Debug)]
pub struct Setup {
    // Speaker and mic devices both run at this nominal rate; 48 kHz skips our resampler.
    pub device_rate: u32,
    // The mic's real clock: a few Hz off `device_rate` is two devices drifting apart.
    pub mic_clock: u32,
    // Room reverberation time in seconds.
    pub rt60: f32,
    // Frames per device callback (macOS CoreAudio's usual 512; WASAPI and Bluetooth often more),
    // with one buffer of latency each way plus one more on output.
    pub output_buffer: usize,
    pub input_buffer: usize,
    // Speaker saturation: 0 is a clean speaker; higher drives a small one into soft clipping.
    pub distortion: f32,
}

impl Setup {
    pub const LAPTOP: Self = Self { device_rate: 48_000, mic_clock: 48_000, rt60: 0.35, output_buffer: 512, input_buffer: 512, distortion: 0.0 };
}

// Laptop speakers at arm's length in a furnished room: a 1 ms direct path, then a diffuse tail
// decaying by 60 dB over `rt60`, loud enough that the echo at the mic matches the near-end talker.
fn room(rate: usize, rt60: f32) -> Vec<(usize, f32)> {
    let mut noise = Noise(11);
    let direct = rate / 1000;
    let mut taps = vec![(direct, 0.6)];
    let decay_per_sample = (1e-3f32).ln() / (rt60 * rate as f32);
    let mut at = direct + rate / 500;
    while (at as f32) < rt60 * rate as f32 {
        taps.push((at, 0.12 * (decay_per_sample * at as f32).exp() * noise.next(1.0)));
        at += rate / 2000 + (noise.next(1.0).abs() * (rate / 2000) as f32) as usize;
    }
    taps
}

// The chain's output for the near end alone is the reference the double-talk output is compared
// against: it has been through the same filters (AEC3 always high-passes) and suppressors.
pub fn run(setup: Setup, echo: bool, noise: NoiseSuppression, gain: bool) -> Report {
    let with_echo = simulate(setup, echo, noise, gain, true);
    let reference = simulate(setup, echo, noise, gain, false);
    measure(&with_echo, &reference.out)
}

struct Run {
    near_at_mic: Vec<f32>,
    echo_at_mic: Vec<f32>,
    out: Vec<f32>,
    gate_open: Vec<bool>,
}

// What a mic chunk goes through between the room and `process_mic`: the mic's own clock, then our
// device-rate-to-48 kHz conversion. The near end and echo go through copies of it separately so
// the measurement sees them exactly as they reach the chain.
struct MicPath {
    clock: Resampler,
    to_48k: Resampler,
}

impl MicPath {
    fn new(setup: Setup) -> Self {
        Self { clock: Resampler::new(setup.device_rate, setup.mic_clock), to_48k: Resampler::new(setup.device_rate, SAMPLE_RATE) }
    }

    fn process(&mut self, mono: &[f32]) -> Vec<f32> {
        self.to_48k.process(&self.clock.process(&to_stereo(mono, 1)))
    }
}

fn simulate(setup: Setup, echo: bool, noise: NoiseSuppression, gain: bool, far_on: bool) -> Run {
    let processor = Processor::new(echo, noise, gain);
    if noise == NoiseSuppression::Advanced {
        processor.wait_for_model();
    }
    let settings = AudioSettings::new(None, None, 1.0, 1.0, processor);
    let mixer = Mixer::default();
    let rate = setup.device_rate as usize;
    let mut playback = Playback::new(setup.device_rate, 2);
    let (mut mic_path, mut near_path, mut echo_path) = (MicPath::new(setup), MicPath::new(setup), MicPath::new(setup));
    let mut gate = Gate::default();
    let mut mic_noise = Noise(5);

    let (far, near) = talkers();
    let far = if far_on { far } else { vec![0.0; far.len()] };
    let near: Vec<f32> = Resampler::new(SAMPLE_RATE, setup.device_rate).process(&to_stereo(&near, 1)).as_chunks::<2>().0.iter().map(|[l, _]| *l).collect();
    let taps = room(rate, setup.rt60);
    let len = SECONDS * rate;

    let output_latency = 2 * setup.output_buffer;
    let mut played = vec![0.0f32; len + output_latency + setup.output_buffer];
    let (mut near_at_mic, mut echo_at_mic) = (Vec::with_capacity(len), Vec::with_capacity(len));
    let (mut out, mut gate_open) = (Vec::with_capacity(len), Vec::new());
    let (mut pushed, mut pending) = (0, Vec::new());
    let mut device = vec![0.0f32; setup.output_buffer * 2];
    let (mut next_output, mut next_input) = (0, 2 * setup.input_buffer);

    // Device callbacks in time order: an output callback fills a buffer that plays
    // `output_latency` later; an input callback delivers the buffer that ended one buffer ago.
    while next_input < len {
        if next_output <= next_input {
            let now_48k = next_output * RATE / rate;
            while pushed < far.len() && pushed <= now_48k + NETWORK_LEAD {
                let frame = &far[pushed..(pushed + FRAME_LEN / 2).min(far.len())];
                mixer.push(1, &to_stereo(frame, 1), 1.0);
                pushed += FRAME_LEN / 2;
            }
            playback.fill(&settings, &mixer, &mut device);
            for (i, [l, r]) in device.as_chunks::<2>().0.iter().enumerate() {
                played[next_output + output_latency + i] = speaker((l + r) * 0.5, setup.distortion);
            }
            next_output += setup.output_buffer;
            continue;
        }
        let chunk = next_input - 2 * setup.input_buffer..next_input - setup.input_buffer;
        next_input += setup.input_buffer;
        let echo: Vec<f32> = chunk.clone().map(|t| taps.iter().filter(|&&(d, _)| d <= t).map(|&(d, g)| g * played[t - d]).sum()).collect();
        let mic: Vec<f32> = chunk.clone().zip(&echo).map(|(t, e)| near[t] + e + mic_noise.next(1e-3)).collect();
        near_at_mic.extend(left(&near_path.process(&near[chunk])));
        echo_at_mic.extend(left(&echo_path.process(&echo)));
        pending.extend(mic_path.process(&mic));
        while pending.len() >= FRAME_LEN {
            let mut frame = [0.0; FRAME_LEN];
            frame.copy_from_slice(&pending[..FRAME_LEN]);
            pending.drain(..FRAME_LEN);
            let processed = settings.process_mic(&mut frame);
            gate_open.push(gate.update(DEFAULT_INPUT_MODE, gain, &processed.blocks, false));
            out.extend(left(&frame));
        }
    }
    Run { near_at_mic, echo_at_mic, out, gate_open }
}

fn speaker(sample: f32, distortion: f32) -> f32 {
    if distortion == 0.0 {
        return sample;
    }
    (sample * distortion).tanh() / distortion
}

fn left(stereo: &[f32]) -> impl Iterator<Item = f32> + '_ {
    stereo.as_chunks::<2>().0.iter().map(|[l, _]| *l)
}

// Synthetic talkers, or recordings (48 kHz mono float WAV, e.g. from `say --data-format=LEF32@48000`)
// named by ECHO_FAR / ECHO_NEAR.
fn talkers() -> (Vec<f32>, Vec<f32>) {
    let voice = |var: &str, synthetic: fn() -> Vec<f32>| std::env::var(var).ok().and_then(|path| wav(&path)).unwrap_or_else(synthetic);
    let far = scaled_to(voice("ECHO_FAR", || talker(SECONDS as f32, 197.0, 1)), FAR_RMS);
    let mut near = scaled_to(voice("ECHO_NEAR", || talker(SECONDS as f32, 118.0, 0)), NEAR_RMS);
    near.rotate_right(DOUBLE_TALK_FROM);
    near[..DOUBLE_TALK_FROM].fill(0.0);
    (far, near)
}

// The samples of a float WAV, looped or cut to the run's length.
fn wav(path: &str) -> Option<Vec<f32>> {
    let bytes = std::fs::read(path).ok()?;
    let at = bytes.windows(4).position(|w| w == b"data")? + 8;
    let samples: Vec<f32> = bytes[at..].as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect();
    Some(samples.iter().cycle().take(SECONDS * RATE).copied().collect())
}

fn measure(run: &Run, reference: &[f32]) -> Report {
    let power = |s: &[f32]| s.iter().map(|x| x * x).sum::<f32>().max(1e-12);
    let single = SETTLE..DOUBLE_TALK_FROM;
    let double = DOUBLE_TALK_FROM + SETTLE..run.out.len().min(reference.len()).min(run.echo_at_mic.len());
    let delay = chain_delay(&run.near_at_mic, reference, double.start);
    let echo = &run.echo_at_mic[double.start - delay..double.end - delay];
    let split = split(&reference[double.clone()], echo, &run.out[double.clone()]);
    // Echo-to-voice ratio at the mic over the same ratio at the output: what the far end hears of
    // themselves relative to us, so AGC lifting both doesn't read as better or worse cancellation.
    let echo_to_near_in = spectral_power(echo) / spectral_power(&run.near_at_mic[double.start - delay..double.end - delay]);
    let frames = |r: &std::ops::Range<usize>| r.start / (FRAME_LEN / 2)..r.end / (FRAME_LEN / 2);
    let opened = &run.gate_open[frames(&single)];
    let chain_gain = split.near / spectral_power(&run.near_at_mic[double.start - delay..double.end - delay]);
    let gaps = gaps(&run.near_at_mic[double.start - delay..double.end - delay]);
    let in_gaps = |s: &[f32]| gaps.iter().map(|g| power(&s[g.clone()])).sum::<f32>();
    let gaps_erle_db = 10.0 * (chain_gain * in_gaps(echo) / in_gaps(&run.out[double.clone()])).log10();
    Report {
        gaps_erle_db,
        single_talk_erle_db: 10.0 * (power(&run.echo_at_mic[single.clone()]) / power(&run.out[single])).log10(),
        double_talk_erle_db: 10.0 * (echo_to_near_in * split.near / split.echo).log10(),
        near_kept_db: 10.0 * (split.near / spectral_power(&reference[double])).log10(),
        gate_open_single_talk: opened.iter().filter(|&&o| o).count() as f32 / opened.len() as f32,
    }
}

// 10 ms stretches where the near end is 30 dB below its average.
fn gaps(near: &[f32]) -> Vec<std::ops::Range<usize>> {
    let block = RATE / 100;
    let mean = near.iter().map(|x| x * x).sum::<f32>() / near.len() as f32;
    (0..near.len() / block)
        .map(|b| b * block..(b + 1) * block)
        .filter(|r| near[r.clone()].iter().map(|x| x * x).sum::<f32>() / (block as f32) < mean * 1e-3)
        .collect()
}

// Samples the chain delays audio by (DeepFilterNet looks ahead): the lag that best lines up the
// near end at the mic with the chain's output for it.
fn chain_delay(input: &[f32], output: &[f32], from: usize) -> usize {
    let dot = |d: usize| -> f32 { (from..from + RATE).map(|t| input[t - d] * output[t]).sum() };
    (0..RATE / 10).max_by(|&a, &b| dot(a).total_cmp(&dot(b))).unwrap_or(0)
}

const FFT: usize = 512;
const HOP: usize = FFT / 2;
// Bins per band: about AEC3's suppression resolution, coarse enough that the fit can't soak up
// much of one talker into the other's term.
const BAND: usize = 16;

// Spectral energy of the output that is the near end, that is the echo, and neither.
struct Split {
    near: f32,
    echo: f32,
}

// Fits the output, per band and 5 ms hop, as a complex gain times the near end (as the chain renders
// it alone) plus a complex gain times the echo at the mic, so suppression and AGC reshaping either
// one don't count against the other. Whatever fits neither (distortion) is left out of both.
fn split(near: &[f32], echo: &[f32], out: &[f32]) -> Split {
    let (near, echo, out) = (stft(near), stft(echo), stft(out));
    let mut split = Split { near: 0.0, echo: 1e-12 };
    for ((n, e), o) in near.iter().zip(&echo).zip(&out) {
        for ((n, e), o) in n.chunks(BAND).zip(e.chunks(BAND)).zip(o.chunks(BAND)) {
            let dot = |a: &[Complex32], b: &[Complex32]| -> Complex32 { a.iter().zip(b).map(|(x, y)| x.conj() * y).sum() };
            let (nn, ee, ne) = (dot(n, n).re, dot(e, e).re, dot(n, e));
            let (no, eo) = (dot(n, o), dot(e, o));
            let det = nn * ee - ne.norm_sqr();
            if det <= 1e-9 * (nn * ee).max(1e-30) {
                continue;
            }
            let gn = (ee * no - ne * eo) / det;
            let ge = (nn * eo - ne.conj() * no) / det;
            split.near += gn.norm_sqr() * nn;
            split.echo += ge.norm_sqr() * ee;
        }
    }
    split
}

fn spectral_power(signal: &[f32]) -> f32 {
    stft(signal).iter().flatten().map(|x| x.norm_sqr()).sum::<f32>().max(1e-12)
}

// Hann-windowed frames, positive-frequency bins only.
fn stft(signal: &[f32]) -> Vec<Vec<Complex32>> {
    let fft = FftPlanner::new().plan_fft_forward(FFT);
    let window: Vec<f32> = (0..FFT).map(|i| (std::f32::consts::PI * i as f32 / FFT as f32).sin().powi(2)).collect();
    (0..signal.len().saturating_sub(FFT) / HOP)
        .map(|f| {
            let mut frame: Vec<Complex32> = signal[f * HOP..f * HOP + FFT].iter().zip(&window).map(|(s, w)| Complex32::new(s * w, 0.0)).collect();
            fft.process(&mut frame);
            frame.truncate(FFT / 2);
            frame
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn print(label: &str, r: &Report) {
        println!(
            "{label}: ERLE single talk {:.1} dB, double talk {:.1} dB (in the near end's gaps {:.1} dB), near end kept {:+.1} dB, gate opened by echo {:.0}%",
            r.single_talk_erle_db,
            r.double_talk_erle_db,
            r.gaps_erle_db,
            r.near_kept_db,
            r.gate_open_single_talk * 100.0
        );
    }

    // Thresholds are for the synthetic talkers; with recordings it only reports.
    #[test]
    fn echo_is_removed_while_either_side_talks() {
        let none = run(Setup::LAPTOP, false, NoiseSuppression::Off, false);
        let aec = run(Setup::LAPTOP, true, NoiseSuppression::Off, false);
        let chain = run(Setup::LAPTOP, true, NoiseSuppression::Advanced, true);
        print("no AEC", &none);
        print("AEC", &aec);
        print("AEC + DeepFilterNet + AGC", &chain);
        if std::env::var_os("ECHO_FAR").is_some() || std::env::var_os("ECHO_NEAR").is_some() {
            return;
        }
        assert!(none.gate_open_single_talk > 0.5, "the simulated echo is loud enough to open the gate");
        assert!(aec.single_talk_erle_db > 45.0);
        assert!(aec.gate_open_single_talk < 0.15, "echo alone rarely opens the gate");
        assert!(aec.double_talk_erle_db > 18.0);
        assert!(aec.near_kept_db > -4.0, "the near end survives double talk");
        assert!(chain.single_talk_erle_db > 45.0);
        assert!(chain.double_talk_erle_db > 15.0);
        assert!(chain.near_kept_db > -6.0);
    }

    #[test]
    #[ignore = "every simulated condition: slow outside --release"]
    fn conditions() {
        let setups = [
            ("dry room", Setup { rt60: 0.1, ..Setup::LAPTOP }),
            ("44.1 kHz devices", Setup { device_rate: 44_100, mic_clock: 44_100, ..Setup::LAPTOP }),
            ("mic clock +42 ppm", Setup { mic_clock: 48_002, ..Setup::LAPTOP }),
            ("2048-frame buffers", Setup { output_buffer: 2048, input_buffer: 2048, ..Setup::LAPTOP }),
            ("1024 out / 441 in", Setup { output_buffer: 1024, input_buffer: 441, ..Setup::LAPTOP }),
            ("loud small speaker", Setup { distortion: 4.0, ..Setup::LAPTOP }),
            ("very loud small speaker", Setup { distortion: 8.0, ..Setup::LAPTOP }),
        ];
        for (label, setup) in setups {
            print(&format!("{label}, AEC"), &run(setup, true, NoiseSuppression::Off, false));
        }
    }

    // The split counts a known leak at its true level (less a fixed ~1 dB the fit shares out).
    #[test]
    fn measurement_reads_a_known_leak() {
        let (far, near) = talkers();
        let reference: Vec<f32> = near.iter().map(|s| 0.5 * s).collect();
        for leak in [0.1f32, 0.01] {
            let out = near.iter().zip(&far).map(|(n, f)| 0.5 * n + leak * f).collect();
            let run = Run { near_at_mic: near.clone(), echo_at_mic: far.clone(), out, gate_open: vec![false; SECONDS * 50] };
            let r = measure(&run, &reference);
            // Echo down by `leak`, near end halved: the echo-to-voice ratio improves by leak - 6 dB.
            let expected = -20.0 * leak.log10() - 6.0;
            assert!((r.double_talk_erle_db - expected).abs() < 2.0, "{} vs {expected}", r.double_talk_erle_db);
            assert!((r.single_talk_erle_db + 20.0 * leak.log10()).abs() < 0.1);
        }
    }
}
