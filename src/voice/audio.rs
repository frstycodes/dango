// Audio devices: listing, live-switchable capture/playback streams, volumes, the mixer, and the
// mic chain (volume → echo/noise/gain processing → level) shared by calls and the mic test.

use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

use cpal::{
    SampleFormat, Stream, StreamConfig, SupportedStreamConfig,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};

use super::dsp::{BLOCKS_PER_FRAME, Block, Processor};

pub const SAMPLE_RATE: u32 = 48_000;
pub const FRAME_SAMPLES: usize = 960;
// Interleaved stereo samples in one 20 ms frame.
pub const FRAME_LEN: usize = FRAME_SAMPLES * 2;
// Per-user backlog cap: anything beyond ~200 ms is dropped from the front so latency can't grow.
const MAX_QUEUE: usize = FRAME_LEN * 10;
// How often the audio thread checks for a device change.
const DEVICE_POLL: Duration = Duration::from_millis(200);
pub const MAX_VOLUME: f32 = 2.0;
pub const SILENCE_DB: f32 = -100.0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub default: bool,
}

pub fn input_devices() -> Vec<Device> {
    list_devices(true)
}

pub fn output_devices() -> Vec<Device> {
    list_devices(false)
}

fn list_devices(input: bool) -> Vec<Device> {
    let host = cpal::default_host();
    let default = default_device(&host, input).and_then(|d| d.id().ok()).map(|id| id.to_string());
    all_devices(&host, input)
        .into_iter()
        .filter_map(|d| {
            let id = d.id().ok()?.to_string();
            let name = d.description().ok()?.name().to_owned();
            Some(Device { default: Some(&id) == default.as_ref(), id, name })
        })
        .collect()
}

fn default_device(host: &cpal::Host, input: bool) -> Option<cpal::Device> {
    if input { host.default_input_device() } else { host.default_output_device() }
}

fn all_devices(host: &cpal::Host, input: bool) -> Vec<cpal::Device> {
    let devices = if input { host.input_devices().map(Iterator::collect) } else { host.output_devices().map(Iterator::collect) };
    devices.unwrap_or_default()
}

// The chosen device, or the system default when none is chosen or it has gone away.
fn find_device(input: bool, id: Option<&str>) -> Option<cpal::Device> {
    let host = cpal::default_host();
    let chosen = id.and_then(|id| all_devices(&host, input).into_iter().find(|d| d.id().is_ok_and(|d| d.to_string() == id)));
    chosen.or_else(|| default_device(&host, input))
}

// Live audio settings, shared by the session (which changes them) and the audio threads.
pub struct AudioSettings {
    pub input_device: Mutex<Option<String>>,
    pub output_device: Mutex<Option<String>>,
    input_volume: AtomicU32,
    output_volume: AtomicU32,
    pub processor: Processor,
}

impl AudioSettings {
    pub fn new(input_device: Option<String>, output_device: Option<String>, input_volume: f32, output_volume: f32, processor: Processor) -> Self {
        Self {
            input_device: Mutex::new(input_device),
            output_device: Mutex::new(output_device),
            input_volume: AtomicU32::new(input_volume.clamp(0.0, MAX_VOLUME).to_bits()),
            output_volume: AtomicU32::new(output_volume.clamp(0.0, MAX_VOLUME).to_bits()),
            processor,
        }
    }

    pub fn set_input_volume(&self, volume: f32) {
        self.input_volume.store(volume.clamp(0.0, MAX_VOLUME).to_bits(), Ordering::Relaxed);
    }

    pub fn set_output_volume(&self, volume: f32) {
        self.output_volume.store(volume.clamp(0.0, MAX_VOLUME).to_bits(), Ordering::Relaxed);
    }

    fn input_volume(&self) -> f32 {
        f32::from_bits(self.input_volume.load(Ordering::Relaxed))
    }

    fn output_volume(&self) -> f32 {
        f32::from_bits(self.output_volume.load(Ordering::Relaxed))
    }

    // The mic chain for one 20 ms frame, in place.
    pub fn process_mic(&self, frame: &mut [f32; FRAME_LEN]) -> Mic {
        let raw_db = level_db(frame);
        let volume = self.input_volume();
        if volume != 1.0 {
            frame.iter_mut().for_each(|s| *s = (*s * volume).clamp(-1.0, 1.0));
        }
        let blocks = self.processor.capture(frame);
        Mic { db: level_db(frame), raw_db, blocks }
    }
}

pub struct Mic {
    // dBFS after processing, and as captured.
    pub db: f32,
    pub raw_db: f32,
    // Voice-activity analysis of each 10 ms block.
    pub blocks: [Block; BLOCKS_PER_FRAME],
}

pub fn level_db(frame: &[f32]) -> f32 {
    let rms = (frame.iter().map(|s| s * s).sum::<f32>() / frame.len().max(1) as f32).sqrt();
    (20.0 * rms.log10()).max(SILENCE_DB)
}

// Mixes every remote speaker into the output device. One queue per source key (an SSRC tagged
// with its connection).
#[derive(Default)]
pub struct Mixer(Mutex<HashMap<u64, VecDeque<f32>>>);

impl Mixer {
    // `gain` is the listener's volume for this speaker (0 = locally muted).
    pub fn push(&self, key: u64, stereo: &[f32], gain: f32) {
        let Ok(mut queues) = self.0.lock() else { return };
        let queue = queues.entry(key).or_default();
        queue.extend(stereo.iter().map(|s| s * gain));
        let excess = queue.len().saturating_sub(MAX_QUEUE);
        queue.drain(..excess);
    }

    pub fn remove(&self, key: u64) {
        if let Ok(mut queues) = self.0.lock() {
            queues.remove(&key);
        }
    }

    // Interleaved 48 kHz stereo.
    fn fill(&self, out: &mut [f32]) {
        out.fill(0.0);
        let Ok(mut queues) = self.0.lock() else { return };
        for frame in out.as_chunks_mut::<2>().0 {
            for queue in queues.values_mut() {
                frame[0] += queue.pop_front().unwrap_or(0.0);
                frame[1] += queue.pop_front().unwrap_or(0.0);
            }
            frame.iter_mut().for_each(|s| *s = s.clamp(-1.0, 1.0));
        }
    }
}

// Keeps the cpal streams on their own thread (cpal streams are not Send on every host) and
// reopens a stream when its chosen device changes. Dropping it stops audio.
pub struct Engine {
    stop: mpsc::Sender<()>,
}

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.stop.send(());
    }
}

// Starts playback of `mixer`, and capture when `capture` is set. Captured audio arrives raw
// (before volume and processing) as interleaved 48 kHz stereo chunks.
pub fn start(settings: Arc<AudioSettings>, mixer: Arc<Mixer>, capture: bool) -> Result<(Engine, mpsc::Receiver<Vec<f32>>), String> {
    let (pcm_tx, pcm_rx) = mpsc::channel();
    let (ready_tx, ready_rx) = mpsc::channel();
    let (stop, stop_rx) = mpsc::channel::<()>();
    thread::Builder::new()
        .name("rustcord-audio".into())
        .spawn(move || run_engine(&settings, &mixer, capture.then_some(pcm_tx), &ready_tx, &stop_rx))
        .map_err(|e| e.to_string())?;
    ready_rx.recv().map_err(|e| e.to_string())??;
    Ok((Engine { stop }, pcm_rx))
}

// A stream and the device choice it was opened for (None stream = opening failed; not retried
// until the choice changes).
type Open = Option<(Option<String>, Option<Stream>)>;

fn run_engine(
    settings: &Arc<AudioSettings>,
    mixer: &Arc<Mixer>,
    pcm_tx: Option<mpsc::Sender<Vec<f32>>>,
    ready: &mpsc::Sender<Result<(), String>>,
    stop: &mpsc::Receiver<()>,
) {
    let (mut output, mut input): (Open, Open) = (None, None);
    let mut first = true;
    loop {
        let want = settings.output_device.lock().map(|d| d.clone()).unwrap_or_default();
        if output.as_ref().is_none_or(|(id, _)| *id != want) {
            output = Some((want.clone(), reopen(first, ready, open_output(settings.clone(), mixer.clone(), want.as_deref()))));
            if first && output.as_ref().is_some_and(|(_, s)| s.is_none()) {
                return;
            }
        }
        if let Some(pcm_tx) = &pcm_tx {
            let want = settings.input_device.lock().map(|d| d.clone()).unwrap_or_default();
            if input.as_ref().is_none_or(|(id, _)| *id != want) {
                input = Some((want.clone(), reopen(first, ready, open_input(pcm_tx.clone(), want.as_deref()))));
                if first && input.as_ref().is_some_and(|(_, s)| s.is_none()) {
                    return;
                }
            }
        }
        if first {
            let _ = ready.send(Ok(()));
            first = false;
        }
        match stop.recv_timeout(DEVICE_POLL) {
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            _ => return,
        }
    }
}

// The first open reports failure to the caller; later ones (a device switch) only log, so a
// bad choice doesn't end the call.
fn reopen(first: bool, ready: &mpsc::Sender<Result<(), String>>, opened: Result<Stream, String>) -> Option<Stream> {
    match opened {
        Ok(stream) => Some(stream),
        Err(e) if first => {
            let _ = ready.send(Err(e));
            None
        }
        Err(e) => {
            eprintln!("voice: switching audio device failed: {e}");
            None
        }
    }
}

fn open_output(settings: Arc<AudioSettings>, mixer: Arc<Mixer>, id: Option<&str>) -> Result<Stream, String> {
    let output = find_device(false, id).ok_or("No speaker found")?;
    let config = pick_config(output.supported_output_configs().map_err(|e| e.to_string())?, output.default_output_config().ok())?;
    let channels = (config.channels as usize).max(1);
    let mut to_device = Resampler::new(SAMPLE_RATE, config.sample_rate);
    let mut pending: VecDeque<f32> = VecDeque::new();
    let mut chunk = vec![0f32; FRAME_LEN / 2];
    let stream = output
        .build_output_stream(
            config,
            move |data: &mut [f32], _: &_| {
                let frames = data.len() / channels;
                while pending.len() < frames * 2 {
                    mixer.fill(&mut chunk);
                    let volume = settings.output_volume();
                    chunk.iter_mut().for_each(|s| *s = (*s * volume).clamp(-1.0, 1.0));
                    settings.processor.render(&chunk);
                    pending.extend(to_device.process(&chunk));
                }
                for frame in data.chunks_mut(channels) {
                    let (l, r) = (pending.pop_front().unwrap_or(0.0), pending.pop_front().unwrap_or(0.0));
                    match frame {
                        [mono] => *mono = (l + r) * 0.5,
                        [left, right, rest @ ..] => {
                            (*left, *right) = (l, r);
                            rest.fill(0.0);
                        }
                        [] => {}
                    }
                }
            },
            |e| eprintln!("voice: output stream error: {e}"),
            None,
        )
        .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(stream)
}

fn open_input(pcm_tx: mpsc::Sender<Vec<f32>>, id: Option<&str>) -> Result<Stream, String> {
    let input = find_device(true, id).ok_or("No microphone found")?;
    let config = pick_config(input.supported_input_configs().map_err(|e| e.to_string())?, input.default_input_config().ok())?;
    let channels = config.channels as usize;
    let mut from_device = Resampler::new(config.sample_rate, SAMPLE_RATE);
    let stream = input
        .build_input_stream(
            config,
            move |data: &[f32], _: &_| {
                let _ = pcm_tx.send(from_device.process(&to_stereo(data, channels)));
            },
            |e| eprintln!("voice: input stream error: {e}"),
            None,
        )
        .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(stream)
}

// 48 kHz when the device offers it; otherwise its default rate, resampled. Bluetooth headsets
// in call mode (AirPods: 16/24 kHz) are the usual reason.
fn pick_config(
    ranges: impl Iterator<Item = cpal::SupportedStreamConfigRange>,
    default: Option<SupportedStreamConfig>,
) -> Result<StreamConfig, String> {
    let native = ranges
        .filter(|r| r.sample_format() == SampleFormat::F32)
        .filter_map(|r| r.try_with_sample_rate(SAMPLE_RATE))
        .min_by_key(|c: &SupportedStreamConfig| c.channels().abs_diff(2));
    native
        .or(default.filter(|c| c.sample_format() == SampleFormat::F32))
        .map(|c| c.config())
        .ok_or_else(|| "Audio device has no float mode".into())
}

// Streaming linear-interpolation resampler for interleaved stereo. Plenty for speech; a
// windowed-sinc resampler would be the upgrade for music.
pub struct Resampler {
    step: f64,
    position: f64,
    previous: [f32; 2],
}

impl Resampler {
    pub fn new(from_hz: u32, to_hz: u32) -> Self {
        Self { step: from_hz as f64 / to_hz.max(1) as f64, position: 0.0, previous: [0.0; 2] }
    }

    pub fn process(&mut self, stereo: &[f32]) -> Vec<f32> {
        if self.step == 1.0 {
            return stereo.to_vec();
        }
        let mut out = Vec::with_capacity((stereo.len() as f64 / self.step) as usize + 4);
        for frame in stereo.as_chunks::<2>().0 {
            while self.position < 1.0 {
                let t = self.position as f32;
                out.push(self.previous[0] + (frame[0] - self.previous[0]) * t);
                out.push(self.previous[1] + (frame[1] - self.previous[1]) * t);
                self.position += self.step;
            }
            self.position -= 1.0;
            self.previous = [frame[0], frame[1]];
        }
        out
    }
}

fn to_stereo(data: &[f32], channels: usize) -> Vec<f32> {
    match channels {
        0 => Vec::new(),
        1 => data.iter().flat_map(|&s| [s, s]).collect(),
        n => data.chunks_exact(n).flat_map(|f| [f[0], f[1]]).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resampler_converts_rates_and_keeps_level() {
        let mut up = Resampler::new(24_000, 48_000);
        let out: Vec<f32> = (0..10).flat_map(|_| up.process(&[0.5, -0.5, 0.5, -0.5])).collect();
        assert_eq!(out.len(), 10 * 2 * 2 * 2, "24 kHz -> 48 kHz doubles the frame count");
        assert!(out[out.len() - 4..].iter().all(|s| s.abs() == 0.5), "a constant signal stays constant: {out:?}");

        let mut down = Resampler::new(48_000, 16_000);
        let n: usize = (0..30).map(|_| down.process(&[0.1; FRAME_LEN]).len()).sum();
        assert_eq!(n, 30 * FRAME_LEN / 3);
        assert_eq!(Resampler::new(48_000, 48_000).process(&[0.3, 0.4]), [0.3, 0.4]);
    }

    #[test]
    fn stereo_conversion() {
        assert_eq!(to_stereo(&[0.1, 0.2], 1), [0.1, 0.1, 0.2, 0.2]);
        assert_eq!(to_stereo(&[0.1, 0.2, 0.3, 0.4, 0.5, 0.6], 3), [0.1, 0.2, 0.4, 0.5]);
    }

    #[test]
    fn mixer_sums_clamps_caps_and_applies_user_gain() {
        let mixer = Mixer::default();
        mixer.push(1, &[0.5, -0.5, 0.9, 0.9], 1.0);
        mixer.push(2, &[0.5, -0.5], 0.5);
        mixer.push(3, &[1.0, 1.0], 0.0);
        let mut out = [9.0; 4];
        mixer.fill(&mut out);
        assert_eq!(out, [0.75, -0.75, 0.9, 0.9], "user 2 at half volume, user 3 locally muted");

        mixer.push(4, &vec![1.0; MAX_QUEUE + 100], 1.0);
        assert_eq!(mixer.0.lock().unwrap()[&4].len(), MAX_QUEUE);
        mixer.remove(4);
        assert!(!mixer.0.lock().unwrap().contains_key(&4));
    }

    #[test]
    fn mic_chain_applies_volume_and_reports_level() {
        let settings = AudioSettings::new(None, None, 0.5, 1.0, Processor::new(false, crate::voice::NoiseSuppression::Off, false));
        let mut frame = [0.2; FRAME_LEN];
        let mic = settings.process_mic(&mut frame);
        let db = mic.db;
        assert!((mic.raw_db - level_db(&[0.2; 4])).abs() < 1e-4, "raw level is before the volume");
        assert!(frame.iter().all(|&s| (s - 0.1).abs() < 1e-6));
        assert!((db - -20.0).abs() < 0.01, "0.1 RMS is -20 dBFS, got {db}");
        assert_eq!(level_db(&[0.0; 4]), SILENCE_DB);
    }
}
