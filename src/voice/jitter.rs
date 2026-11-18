// Per-speaker adaptive jitter buffer, after WebRTC's NetEQ. Opus packets wait here in sequence
// order and the output device pulls one 20 ms frame at a time, so playout follows the device
// clock and network jitter is absorbed by a delay that tracks the measured arrival spread.

use std::{
    collections::{BTreeMap, VecDeque},
    time::Duration,
};

use davey::OPUS_SILENCE_PACKET;
use opus::{Channels, Decoder};

use super::audio::{FRAME_LEN, SAMPLE_RATE, level_db};

const FRAME_MS: f32 = 20.0;
const MIN_DELAY_MS: f32 = 40.0;
const MAX_DELAY_MS: f32 = 240.0;
// Arrival spread is measured over the last ~2 s of packets, at this quantile.
const WINDOW: usize = 100;
const QUANTILE: f32 = 0.95;
// An underrun raises the target at once by this much; a lower target is approached by this
// fraction per packet (time constant ~10 s), so a calm link shrinks the delay slowly.
const UNDERRUN_STEP_MS: f32 = 40.0;
const SHRINK_RATE: f32 = 0.002;
// Concealed frames in a row: the first ones at full level, then faded to silence by the last.
const PLC_FULL: u32 = 2;
const MAX_CONCEALED: u32 = 5;
// Past target by this much, a quiet frame is dropped to shrink the buffer; past the larger
// margin, any frame is (sender clock running fast while someone talks without pause).
const QUIET_DROP_MS: f32 = 40.0;
const FORCE_DROP_MS: f32 = 100.0;
const QUIET_DB: f32 = -45.0;
// Crossfade over a dropped frame, in interleaved samples (5 ms).
const CROSSFADE: usize = 480;
// Packets held at most (1 s); older ones go when nothing pulls, e.g. no output device yet.
const MAX_PACKETS: usize = 50;

// What one pulled frame was made of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Playout {
    Normal,
    // Rebuilt from the in-band FEC copy carried by the following packet.
    Fec,
    // Opus loss concealment (fading out after PLC_FULL frames).
    Plc,
    // Nothing to play: buffering, between talkspurts, or concealment ran out.
    Silence,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct JitterStats {
    pub target_ms: u32,
    // Audio waiting to be played.
    pub buffered_ms: u32,
    // The buffer ran dry mid-speech.
    pub underruns: u32,
    pub concealed: u32,
    pub fec: u32,
    // Arrived after their slot was played.
    pub late: u32,
    // Dropped to shrink the delay.
    pub dropped: u32,
}

struct Packet {
    opus: Vec<u8>,
}

pub struct JitterBuffer {
    decoder: Decoder,
    // By extended (unwrapped) sequence number.
    packets: BTreeMap<u64, Packet>,
    highest: Option<u64>,
    // The next sequence to play; None while buffering before a talkspurt.
    next: Option<u64>,
    // The last sequence played or concealed; anything at or before it is too late.
    played: Option<u64>,
    // Concealed frames in a row.
    run: u32,
    // Frames pulled while buffering with packets waiting (a talkspurt shorter than the target).
    waited: u32,
    last_was_silence: bool,
    // Arrival minus media time (ms) of recent packets, and the first packet's pair as origin.
    delays: VecDeque<f32>,
    origin: Option<(Duration, u32)>,
    target_ms: f32,
    stats: JitterStats,
    scratch: Vec<f32>,
}

impl JitterBuffer {
    pub fn new() -> Option<Self> {
        Some(Self {
            decoder: Decoder::new(SAMPLE_RATE, Channels::Stereo).ok()?,
            packets: BTreeMap::new(),
            highest: None,
            next: None,
            played: None,
            run: 0,
            waited: 0,
            last_was_silence: false,
            delays: VecDeque::with_capacity(WINDOW + 1),
            origin: None,
            target_ms: MIN_DELAY_MS,
            stats: JitterStats::default(),
            scratch: vec![0.0; FRAME_LEN],
        })
    }

    pub fn stats(&self) -> JitterStats {
        JitterStats { target_ms: self.target_ms.round() as u32, buffered_ms: self.buffered_ms().round() as u32, ..self.stats }
    }

    // `arrival` is on any monotonic clock, the same one for every packet of this speaker.
    pub fn insert(&mut self, sequence: u16, timestamp: u32, opus: Vec<u8>, arrival: Duration) {
        let seq = self.extend(sequence);
        self.highest = Some(self.highest.map_or(seq, |h| h.max(seq)));
        if self.played.is_some_and(|p| seq <= p) {
            self.stats.late += 1;
            return;
        }
        self.measure(timestamp, arrival);
        self.packets.insert(seq, Packet { opus });
        while self.packets.len() > MAX_PACKETS {
            let Some((oldest, _)) = self.packets.pop_first() else { break };
            self.played = Some(oldest);
            if self.next.is_some_and(|n| n <= oldest) {
                self.next = Some(oldest + 1);
            }
        }
    }

    // Fills one 20 ms frame of interleaved 48 kHz stereo.
    pub fn pull(&mut self, out: &mut [f32]) -> Playout {
        out.fill(0.0);
        let next = match self.next {
            Some(next) => next,
            None => match self.start() {
                Some(first) => first,
                None => return Playout::Silence,
            },
        };
        if let Some(packet) = self.packets.remove(&next) {
            self.play(next, &packet, out);
            return Playout::Normal;
        }
        if self.packets.is_empty() {
            return self.underrun(out);
        }
        // Lost, or still on its way while later ones are here: conceal and move on.
        self.next = Some(next + 1);
        self.played = Some(next);
        match self.packets.get(&(next + 1)).map(|p| p.opus.clone()) {
            Some(following) if self.decoder.decode_float(&following, &mut out[..FRAME_LEN], true).is_ok() => {
                self.run += 1;
                self.stats.fec += 1;
                Playout::Fec
            }
            _ => self.conceal(out),
        }
    }

    // Playout starts once the target delay's worth of audio is waiting, or once what is
    // waiting has waited that long.
    fn start(&mut self) -> Option<u64> {
        let (&first, _) = self.packets.first_key_value()?;
        self.waited += 1;
        if self.buffered_ms() < self.target_ms && (self.waited as f32) * FRAME_MS < self.target_ms {
            return None;
        }
        self.waited = 0;
        self.next = Some(first);
        Some(first)
    }

    fn play(&mut self, seq: u64, packet: &Packet, out: &mut [f32]) {
        let fade_in = self.run > PLC_FULL;
        self.run = 0;
        self.played = Some(seq);
        self.next = Some(seq + 1);
        self.last_was_silence = packet.opus == OPUS_SILENCE_PACKET;
        let Ok(n) = self.decoder.decode_float(&packet.opus, &mut self.scratch, false) else { return };
        let n = (n * 2).min(FRAME_LEN);
        let excess = self.buffered_ms() - self.target_ms;
        let quiet = self.last_was_silence || level_db(&self.scratch[..n]) < QUIET_DB;
        let shrink = excess >= FORCE_DROP_MS || (excess >= QUIET_DROP_MS && quiet);
        if shrink && let Some(following) = self.packets.remove(&(seq + 1)) {
            self.drop_frame(seq + 1, &following, out);
            return;
        }
        out[..n].copy_from_slice(&self.scratch[..n]);
        if fade_in {
            ramp(&mut out[..CROSSFADE], 0.0, 1.0);
        }
    }

    // Plays `seq` in place of the frame already decoded into `scratch`, crossfading from the
    // dropped frame's start (which continues what was last played) into the kept one.
    fn drop_frame(&mut self, seq: u64, packet: &Packet, out: &mut [f32]) {
        self.stats.dropped += 1;
        self.played = Some(seq);
        self.next = Some(seq + 1);
        self.last_was_silence = packet.opus == OPUS_SILENCE_PACKET;
        let Ok(n) = self.decoder.decode_float(&packet.opus, &mut out[..FRAME_LEN], false) else { return };
        let fade = CROSSFADE.min(n * 2);
        for (i, (kept, dropped)) in out[..fade].iter_mut().zip(&self.scratch[..fade]).enumerate() {
            let w = i as f32 / fade as f32;
            *kept = *kept * w + dropped * (1.0 - w);
        }
    }

    // Nothing buffered. After a talkspurt's closing silence frames that is just the end of
    // speech; otherwise the network is late, so conceal without advancing (the delay grows by
    // holding) and fall back to buffering if it stays dry.
    fn underrun(&mut self, out: &mut [f32]) -> Playout {
        if self.last_was_silence || self.run >= MAX_CONCEALED {
            self.next = None;
            return Playout::Silence;
        }
        if self.run == 0 {
            self.stats.underruns += 1;
            self.target_ms = (self.target_ms + UNDERRUN_STEP_MS).min(MAX_DELAY_MS);
        }
        self.conceal(out)
    }

    fn conceal(&mut self, out: &mut [f32]) -> Playout {
        self.run += 1;
        self.stats.concealed += 1;
        if self.run > MAX_CONCEALED {
            return Playout::Silence;
        }
        if self.decoder.decode_float(&[], &mut out[..FRAME_LEN], false).is_err() {
            return Playout::Silence;
        }
        if self.run > PLC_FULL {
            let steps = (MAX_CONCEALED - PLC_FULL) as f32;
            let k = (self.run - PLC_FULL) as f32;
            ramp(out, 1.0 - (k - 1.0) / steps, 1.0 - k / steps);
        }
        Playout::Plc
    }

    fn buffered_ms(&self) -> f32 {
        let Some((&last, _)) = self.packets.last_key_value() else { return 0.0 };
        let first = self.next.or_else(|| self.packets.first_key_value().map(|(&k, _)| k)).unwrap_or(last);
        (last.saturating_sub(first) + 1) as f32 * FRAME_MS
    }

    // The 16-bit sequence unwrapped against the highest one seen.
    fn extend(&self, sequence: u16) -> u64 {
        let Some(highest) = self.highest else { return 1 << 32 | sequence as u64 };
        let delta = sequence.wrapping_sub(highest as u16) as i16;
        highest.saturating_add_signed(delta as i64)
    }

    // NetEQ-style: how much later than the fastest packet each one arrives, relative to its
    // media time. The target is a high quantile of that spread.
    fn measure(&mut self, timestamp: u32, arrival: Duration) {
        let &mut (origin_arrival, origin_ts) = self.origin.get_or_insert((arrival, timestamp));
        let media_ms = timestamp.wrapping_sub(origin_ts) as i32 as f32 / (SAMPLE_RATE / 1000) as f32;
        let arrival_ms = arrival.saturating_sub(origin_arrival).as_secs_f32() * 1000.0;
        self.delays.push_back(arrival_ms - media_ms);
        if self.delays.len() > WINDOW {
            self.delays.pop_front();
        }
        let mut sorted: Vec<f32> = self.delays.iter().copied().collect();
        sorted.sort_unstable_by(f32::total_cmp);
        let spread = sorted[((sorted.len() - 1) as f32 * QUANTILE) as usize] - sorted[0];
        let want = (spread + FRAME_MS).clamp(MIN_DELAY_MS, MAX_DELAY_MS);
        self.target_ms = if want > self.target_ms { want } else { self.target_ms + (want - self.target_ms) * SHRINK_RATE };
    }
}

// Linear gain from `from` to `to` across interleaved stereo `samples`.
fn ramp(samples: &mut [f32], from: f32, to: f32) {
    let frames = (samples.len() / 2).max(1) as f32;
    for (i, frame) in samples.chunks_mut(2).enumerate() {
        let gain = from + (to - from) * i as f32 / frames;
        frame.iter_mut().for_each(|s| *s *= gain);
    }
}

#[cfg(test)]
mod tests {
    use opus::{Application, Encoder};

    use super::{super::fixtures, *};

    const TS_STEP: u32 = 960;

    struct Talker {
        encoder: Encoder,
        signal: Vec<f32>,
        frame: usize,
    }

    impl Talker {
        // Speech-like stereo, with in-band FEC on as a lossy link would ask of the sender.
        fn new(seconds: f32) -> Self {
            let mut encoder = Encoder::new(SAMPLE_RATE, Channels::Stereo, Application::Voip).unwrap();
            encoder.set_bitrate(opus::Bitrate::Bits(64_000)).unwrap();
            encoder.set_inband_fec(true).unwrap();
            encoder.set_packet_loss_perc(5).unwrap();
            let signal = fixtures::speech(seconds).iter().flat_map(|&s| [s, s]).collect();
            Self { encoder, signal, frame: 0 }
        }

        fn next(&mut self) -> Option<Vec<u8>> {
            let pcm = self.signal.get(self.frame * FRAME_LEN..(self.frame + 1) * FRAME_LEN)?;
            self.frame += 1;
            let mut buf = [0u8; 4000];
            let n = self.encoder.encode_float(pcm, &mut buf).unwrap();
            Some(buf[..n].to_vec())
        }
    }

    fn ms(ms: u64) -> Duration {
        Duration::from_millis(ms)
    }

    // Packets 0..count arriving on time, 20 ms apart.
    fn steady(jb: &mut JitterBuffer, talker: &mut Talker, count: u16) {
        for seq in 0..count {
            jb.insert(seq, seq as u32 * TS_STEP, talker.next().unwrap(), ms(seq as u64 * 20));
        }
    }

    fn pulls(jb: &mut JitterBuffer, n: usize) -> Vec<Playout> {
        let mut out = vec![0.0; FRAME_LEN];
        (0..n).map(|_| jb.pull(&mut out)).collect()
    }

    #[test]
    fn plays_in_sequence_order_whatever_the_arrival_order() {
        let mut jb = JitterBuffer::new().unwrap();
        let mut talker = Talker::new(1.0);
        let packets: Vec<Vec<u8>> = (0..6).map(|_| talker.next().unwrap()).collect();
        for (i, &seq) in [0u16, 2, 1, 4, 3, 5].iter().enumerate() {
            jb.insert(seq, seq as u32 * TS_STEP, packets[seq as usize].clone(), ms(i as u64 * 20));
        }
        let mut order = Vec::new();
        let mut out = vec![0.0; FRAME_LEN];
        for _ in 0..6 {
            assert_eq!(jb.pull(&mut out), Playout::Normal);
            order.push(jb.played.unwrap() - (1 << 32));
        }
        assert_eq!(order, [0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn sequence_wraps_without_reordering() {
        let mut jb = JitterBuffer::new().unwrap();
        let mut talker = Talker::new(1.0);
        for (i, seq) in [65534u16, 65535, 0, 1].into_iter().enumerate() {
            jb.insert(seq, i as u32 * TS_STEP, talker.next().unwrap(), ms(i as u64 * 20));
        }
        assert_eq!(pulls(&mut jb, 4), [Playout::Normal; 4]);
    }

    #[test]
    fn holds_back_the_target_delay_before_playing() {
        let mut jb = JitterBuffer::new().unwrap();
        let mut talker = Talker::new(1.0);
        jb.insert(0, 0, talker.next().unwrap(), ms(0));
        assert_eq!(pulls(&mut jb, 1), [Playout::Silence], "one 20 ms packet is under the 40 ms target");
        jb.insert(1, TS_STEP, talker.next().unwrap(), ms(20));
        assert_eq!(pulls(&mut jb, 2), [Playout::Normal; 2]);
    }

    #[test]
    fn target_follows_jitter_up_fast_and_down_slowly() {
        let mut jb = JitterBuffer::new().unwrap();
        let mut talker = Talker::new(10.0);
        steady(&mut jb, &mut talker, 50);
        assert_eq!(jb.stats().target_ms, MIN_DELAY_MS as u32, "a clean link keeps the minimum");

        // Every 4th packet 100 ms late.
        for seq in 50..150u16 {
            let late = if seq % 4 == 0 { 100 } else { 0 };
            jb.insert(seq, seq as u32 * TS_STEP, talker.next().unwrap(), ms(seq as u64 * 20 + late));
        }
        let jittery = jb.stats().target_ms;
        assert!((110..=130).contains(&jittery), "100 ms spread plus a frame, got {jittery}");

        // Constant delay once the jittery packets have left the 100-packet window.
        for seq in 150..450u16 {
            jb.insert(seq, seq as u32 * TS_STEP, talker.next().unwrap(), ms(seq as u64 * 20 + 100));
        }
        let calm = jb.stats().target_ms;
        assert!(calm < jittery && calm > 80, "shrinks, but slowly: {jittery} -> {calm}");

        let before = jb.stats().target_ms;
        jb.packets.clear();
        jb.next = Some(1000);
        jb.last_was_silence = false;
        let mut out = vec![0.0; FRAME_LEN];
        assert_eq!(jb.pull(&mut out), Playout::Plc);
        assert_eq!(jb.stats().underruns, 1);
        assert_eq!(jb.stats().target_ms, before + UNDERRUN_STEP_MS as u32, "an underrun raises it at once");
    }

    #[test]
    fn missing_frame_uses_fec_from_the_next_packet_else_plc() {
        let mut jb = JitterBuffer::new().unwrap();
        let mut talker = Talker::new(1.0);
        let packets: Vec<Vec<u8>> = (0..8).map(|_| talker.next().unwrap()).collect();
        for seq in [0u16, 1, 3, 6, 7] {
            jb.insert(seq, seq as u32 * TS_STEP, packets[seq as usize].clone(), ms(seq as u64 * 20));
        }
        // All already here: a target that covers them keeps the shrinking out of this test.
        jb.target_ms = 160.0;
        let played = pulls(&mut jb, 8);
        assert_eq!(
            played,
            [Playout::Normal, Playout::Normal, Playout::Fec, Playout::Normal, Playout::Plc, Playout::Fec, Playout::Normal, Playout::Normal],
            "2 rebuilt from 3, 4 has no successor so PLC, 5 rebuilt from 6"
        );
        assert_eq!((jb.stats().fec, jb.stats().concealed), (2, 1));
    }

    #[test]
    fn late_packet_before_its_slot_is_played_after_it_is_counted_late() {
        let mut jb = JitterBuffer::new().unwrap();
        let mut talker = Talker::new(1.0);
        let packets: Vec<Vec<u8>> = (0..6).map(|_| talker.next().unwrap()).collect();
        for seq in [0u16, 1, 3, 4, 5] {
            jb.insert(seq, seq as u32 * TS_STEP, packets[seq as usize].clone(), ms(seq as u64 * 20));
        }
        pulls(&mut jb, 2);
        jb.insert(2, 2 * TS_STEP, packets[2].clone(), ms(90));
        assert_eq!(pulls(&mut jb, 1), [Playout::Normal], "arrived before its slot");
        jb.insert(2, 2 * TS_STEP, packets[2].clone(), ms(200));
        assert_eq!(jb.stats().late, 1);
    }

    #[test]
    fn dry_buffer_conceals_then_fades_to_silence_and_resumes_on_arrival() {
        let mut jb = JitterBuffer::new().unwrap();
        let mut talker = Talker::new(2.0);
        steady(&mut jb, &mut talker, 3);
        let played = pulls(&mut jb, 3 + MAX_CONCEALED as usize + 2);
        assert_eq!(played[..3], [Playout::Normal; 3]);
        assert_eq!(played[3..3 + MAX_CONCEALED as usize], [Playout::Plc; MAX_CONCEALED as usize]);
        assert_eq!(played[3 + MAX_CONCEALED as usize..], [Playout::Silence; 2]);
        assert_eq!(jb.stats().underruns, 1);

        for seq in 3..10u16 {
            jb.insert(seq, seq as u32 * TS_STEP, talker.next().unwrap(), ms(300 + seq as u64 * 20));
        }
        // The 300 ms stall raised the target, so they wait for it before playing.
        assert!(pulls(&mut jb, 14).contains(&Playout::Normal), "the late packets still play");
    }

    #[test]
    fn talkspurt_end_is_not_an_underrun() {
        let mut jb = JitterBuffer::new().unwrap();
        let mut talker = Talker::new(1.0);
        steady(&mut jb, &mut talker, 3);
        for seq in 3..8u16 {
            jb.insert(seq, seq as u32 * TS_STEP, OPUS_SILENCE_PACKET.to_vec(), ms(seq as u64 * 20));
        }
        assert_eq!(pulls(&mut jb, 10)[8..], [Playout::Silence; 2]);
        assert_eq!((jb.stats().underruns, jb.stats().concealed), (0, 0));
    }

    #[test]
    fn fast_sender_clock_is_absorbed_by_dropping_frames() {
        let mut jb = JitterBuffer::new().unwrap();
        let mut talker = Talker::new(30.0);
        // The sender's 20 ms is our 19.6 ms (2% fast): 30 s of speech gains 600 ms of backlog.
        let arrivals = (0..1500u16).map(|seq| (seq, seq as u64 * 19_600, talker.next().unwrap())).collect();
        let max_buffered = simulate(&mut jb, arrivals, 31_000_000).iter().map(|f| f.buffered_ms).max().unwrap();
        assert!(jb.stats().dropped >= 25, "dropped {}", jb.stats().dropped);
        assert!(max_buffered <= (MIN_DELAY_MS + FORCE_DROP_MS + 2.0 * FRAME_MS) as u32, "bounded, got {max_buffered} ms");
    }

    struct Pulled {
        at_us: u64,
        playout: Playout,
        played: Option<u64>,
        buffered_ms: u32,
    }

    // Inserts each (sequence, arrival µs, opus) at its arrival and pulls a frame every 20 ms, as
    // the mixer does on the device clock.
    fn simulate(jb: &mut JitterBuffer, mut arrivals: Vec<(u16, u64, Vec<u8>)>, until_us: u64) -> Vec<Pulled> {
        arrivals.sort_by_key(|a| a.1);
        let mut arrivals = arrivals.into_iter().peekable();
        let mut out = vec![0.0; FRAME_LEN];
        (0..until_us / 20_000)
            .map(|tick| {
                let at_us = tick * 20_000;
                while let Some((seq, arrival, opus)) = arrivals.next_if(|a| a.1 <= at_us) {
                    jb.insert(seq, seq as u32 * TS_STEP, opus, Duration::from_micros(arrival));
                }
                let playout = jb.pull(&mut out);
                Pulled { at_us, playout, played: jb.played, buffered_ms: jb.stats().buffered_ms }
            })
            .collect()
    }

    // How the receive path worked before this buffer: decode on arrival, conceal gaps of up to 5
    // frames with PLC, drop anything older than the last packet, queue PCM (capped at 200 ms)
    // for the device to pop, silence when the queue is dry.
    struct ArrivalOrderPath {
        decoder: Decoder,
        last: Option<u16>,
        queue: VecDeque<f32>,
        concealed: u32,
        late: u32,
        latency_ms: Vec<f32>,
    }

    impl ArrivalOrderPath {
        fn arrive(&mut self, seq: u16, opus: &[u8], delay_ms: f32) {
            let gap = self.last.map_or(0, |last| seq.wrapping_sub(last).wrapping_sub(1));
            if self.last.is_some() && gap >= u16::MAX / 2 {
                self.late += 1;
                return;
            }
            self.last = Some(seq);
            let mut pcm = vec![0.0; FRAME_LEN];
            if gap <= 5 {
                for _ in 0..gap {
                    self.decoder.decode_float(&[], &mut pcm, false).unwrap();
                    self.queue.extend(&pcm);
                    self.concealed += 1;
                }
            }
            self.latency_ms.push(delay_ms + self.queue.len() as f32 / (2 * SAMPLE_RATE / 1000) as f32);
            self.decoder.decode_float(opus, &mut pcm, false).unwrap();
            self.queue.extend(&pcm);
            let excess = self.queue.len().saturating_sub(FRAME_LEN * 10);
            self.queue.drain(..excess);
        }
    }

    #[derive(Debug)]
    struct Outcome {
        // 20 ms frames between the first and last audio that had hard silence in them.
        dry: u32,
        concealed: u32,
        late: u32,
        mean_latency_ms: f32,
    }

    // A talker's 20 s of speech (then Discord's 5 closing silence frames) over a link with
    // uniform jitter, random loss and, optionally, a 250 ms stall every 3 s that releases its
    // packets in a burst.
    fn network(jitter_ms: f32, loss: f32, stalls: bool, seed: u32) -> Vec<(u16, u64, Vec<u8>)> {
        let mut talker = Talker::new(20.0);
        let mut noise = fixtures::Noise(seed);
        (0..1005u16)
            .filter_map(|seq| {
                let opus = talker.next().unwrap_or_else(|| OPUS_SILENCE_PACKET.to_vec());
                let lost = noise.next(0.5) + 0.5 < loss;
                let sent = seq as f32 * 20.0;
                let mut arrival = sent + 40.0 + jitter_ms + noise.next(jitter_ms);
                let in_stall = sent % 3000.0 - 1000.0;
                if stalls && (0.0..250.0).contains(&in_stall) {
                    arrival = arrival.max(sent - in_stall + 250.0 + 40.0);
                }
                (!lost).then_some((seq, (arrival * 1000.0) as u64, opus))
            })
            .collect()
    }

    fn arrival_order(arrivals: &[(u16, u64, Vec<u8>)]) -> Outcome {
        let mut path = ArrivalOrderPath {
            decoder: Decoder::new(SAMPLE_RATE, Channels::Stereo).unwrap(),
            last: None,
            queue: VecDeque::new(),
            concealed: 0,
            late: 0,
            latency_ms: Vec::new(),
        };
        let mut sorted: Vec<_> = arrivals.iter().collect();
        sorted.sort_by_key(|a| a.1);
        let last_arrival = sorted.last().unwrap().1;
        let mut sorted = sorted.into_iter().peekable();
        let (mut dry, mut started) = (0, false);
        for tick in 0..last_arrival / 20_000 {
            while let Some((seq, arrival, opus)) = sorted.next_if(|a| a.1 <= tick * 20_000) {
                path.arrive(*seq, opus, (*arrival as f32 / 1000.0) - *seq as f32 * 20.0);
            }
            started |= !path.queue.is_empty();
            if started && path.queue.len() < FRAME_LEN {
                dry += 1;
            }
            path.queue.drain(..FRAME_LEN.min(path.queue.len()));
        }
        let mean_latency_ms = path.latency_ms.iter().sum::<f32>() / path.latency_ms.len() as f32;
        Outcome { dry, concealed: path.concealed, late: path.late, mean_latency_ms }
    }

    fn buffered(arrivals: &[(u16, u64, Vec<u8>)]) -> Outcome {
        let mut jb = JitterBuffer::new().unwrap();
        let pulled = simulate(&mut jb, arrivals.to_vec(), 25_000_000);
        let first = pulled.iter().position(|p| p.playout != Playout::Silence).unwrap();
        let last = pulled.iter().rposition(|p| p.playout != Playout::Silence).unwrap();
        let dry = pulled[first..=last].iter().filter(|p| p.playout == Playout::Silence).count() as u32;
        let latencies: Vec<f32> = pulled
            .iter()
            .filter(|p| p.playout == Playout::Normal)
            .map(|p| p.at_us as f32 / 1000.0 - (p.played.unwrap() - (1 << 32)) as f32 * 20.0)
            .collect();
        let stats = jb.stats();
        Outcome { dry, concealed: stats.concealed + stats.fec, late: stats.late, mean_latency_ms: latencies.iter().sum::<f32>() / latencies.len() as f32 }
    }

    // `cargo test --release --example voice_join jitter -- --nocapture` prints the table.
    #[test]
    fn jitter_buffer_beats_arrival_order_playout() {
        let scenarios = [
            ("±30 ms jitter, 2% loss", network(30.0, 0.02, false, 1)),
            ("±50 ms jitter, 3% loss, 250 ms stalls", network(50.0, 0.03, true, 2)),
            ("±80 ms jitter, 5% loss", network(80.0, 0.05, false, 3)),
        ];
        println!("| link | path | dry frames | concealed | late dropped | mean latency |");
        println!("|---|---|---|---|---|---|");
        for (name, arrivals) in &scenarios {
            let (before, after) = (arrival_order(arrivals), buffered(arrivals));
            for (path, o) in [("before", &before), ("after", &after)] {
                println!("| {name} | {path} | {} | {} | {} | {:.0} ms |", o.dry, o.concealed, o.late, o.mean_latency_ms);
            }
            assert!(after.dry * 4 < before.dry.max(1), "{name}: {after:?} vs {before:?}");
            assert!(after.late < before.late.max(1), "{name}: {after:?} vs {before:?}");
            assert!(after.mean_latency_ms < 300.0, "{name}: {after:?}");
        }
    }
}
