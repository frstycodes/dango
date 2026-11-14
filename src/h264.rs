// Discord's GIFs and most video uploads are H.264 in MP4: the video track is demuxed with `mp4` and
// decoded with OpenH264, and pictures come out in the order they're shown, each with its time.
use std::{io::Cursor, ops::ControlFlow, time::Duration};

use image::{RgbaImage, imageops::FilterType};
use openh264::{
    OpenH264API,
    decoder::{DecodedYUV, Decoder, DecoderConfig, Flush},
    formats::YUVSource,
};

// iPhone and QuickTime files describe AAC with a version 1 or 2 sound entry: 16 or 36 bytes of
// extra fields after the standard 28, and often the `esds` inside a `wave` box. The `mp4` crate
// reads only the version 0 layout, so it takes the extra fields for a box and gives up ("mp4a box
// contains a box with a larger size than it"). This rewrites each such entry in place as version
// 0: the `esds` moves up to follow the standard fields and a `free` box fills the rest, so no box
// size or file offset changes. None when nothing needed rewriting.
pub fn modernize_audio_entries(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut entries = Vec::new();
    find_sound_entries(bytes, 0, bytes.len(), &mut entries);
    if entries.is_empty() {
        return None;
    }
    let mut out = bytes.to_vec();
    for (start, end) in entries {
        rewrite_sound_entry(&mut out[start..end]);
    }
    Some(out)
}

const CONTAINERS: [&[u8; 4]; 5] = [b"moov", b"trak", b"mdia", b"minf", b"stbl"];
// The standard audio sample entry: box header, 6 reserved, data reference index, 8 version and
// reserved bytes, channels, sample size, 4 reserved, sample rate.
const SOUND_ENTRY_BASE: usize = 8 + 28;

// Boxes in `bytes[from..to]` as (start, end, type), stopping at the first malformed header.
fn boxes(bytes: &[u8], from: usize, to: usize) -> Vec<(usize, usize, [u8; 4])> {
    let mut out = Vec::new();
    let mut at = from;
    while at + 8 <= to {
        let size = u32::from_be_bytes(bytes[at..at + 4].try_into().expect("4 bytes")) as usize;
        let kind: [u8; 4] = bytes[at + 4..at + 8].try_into().expect("4 bytes");
        let end = if size == 0 { to } else { at + size };
        if size != 0 && (size < 8 || end > to) {
            break;
        }
        out.push((at, end, kind));
        at = end;
    }
    out
}

fn find_sound_entries(bytes: &[u8], from: usize, to: usize, found: &mut Vec<(usize, usize)>) {
    for (start, end, kind) in boxes(bytes, from, to) {
        if CONTAINERS.contains(&&kind) {
            find_sound_entries(bytes, start + 8, end, found);
        } else if &kind == b"stsd" && start + 16 <= end {
            // Full box (4 bytes version and flags) plus an entry count before the entries.
            for (entry, entry_end, entry_kind) in boxes(bytes, start + 16, end) {
                let version = bytes.get(entry + 16..entry + 18).map(|v| u16::from_be_bytes([v[0], v[1]]));
                if &entry_kind == b"mp4a" && matches!(version, Some(1 | 2)) && entry_end - entry >= SOUND_ENTRY_BASE + 16 {
                    found.push((entry, entry_end));
                }
            }
        }
    }
}

fn rewrite_sound_entry(entry: &mut [u8]) {
    let extra = if entry[17] == 2 { 36 } else { 16 };
    let children = (SOUND_ENTRY_BASE + extra).min(entry.len());
    let esds = esds_in(entry, children).map(|(s, e)| entry[s..e].to_vec());
    entry[16..18].copy_from_slice(&[0, 0]);
    let mut tail = esds.unwrap_or_default();
    let room = entry.len() - SOUND_ENTRY_BASE;
    if tail.len() + 8 > room {
        tail.clear();
    }
    let free = room - tail.len();
    tail.extend_from_slice(&(free as u32).to_be_bytes());
    tail.extend_from_slice(b"free");
    tail.resize(room, 0);
    entry[SOUND_ENTRY_BASE..].copy_from_slice(&tail);
}

// The `esds` among an entry's children, directly or inside QuickTime's `wave`.
fn esds_in(entry: &[u8], from: usize) -> Option<(usize, usize)> {
    for (start, end, kind) in boxes(entry, from, entry.len()) {
        match &kind {
            b"esds" => return Some((start, end)),
            b"wave" => {
                if let Some(found) = boxes(entry, start + 8, end).into_iter().find(|(_, _, k)| k == b"esds") {
                    return Some((found.0, found.1));
                }
            }
            _ => {}
        }
    }
    None
}

pub struct Track<B> {
    reader: mp4::Mp4Reader<Cursor<B>>,
    id: u32,
    timescale: u64,
    header: Vec<u8>,
    samples: Vec<Sample>,
    end: u64,
}

// One coded picture, in decode order; `pts` is when it's shown, in track ticks from the first picture.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Sample {
    pts: u64,
    sync: bool,
}

impl<B: AsRef<[u8]>> Track<B> {
    pub fn open(bytes: B) -> Result<Self, String> {
        let err = |e: mp4::Error| e.to_string();
        let size = bytes.as_ref().len() as u64;
        let reader = mp4::Mp4Reader::read_header(Cursor::new(bytes), size).map_err(err)?;
        let track = reader.tracks().values().find(|t| matches!(t.media_type(), Ok(mp4::MediaType::H264))).ok_or("no H.264 track")?;
        let mut header = Vec::new();
        for set in [track.sequence_parameter_set().map_err(err)?, track.picture_parameter_set().map_err(err)?] {
            header.extend_from_slice(&[0, 0, 0, 1]);
            header.extend_from_slice(set);
        }
        let stbl = &track.trak.mdia.minf.stbl;
        let (samples, end) = sample_table(
            stbl.stts.entries.iter().map(|e| (e.sample_count, e.sample_delta)),
            stbl.ctts.iter().flat_map(|c| &c.entries).map(|e| (e.sample_count, e.sample_offset)),
            stbl.stss.as_ref().map(|s| s.entries.as_slice()),
        );
        let (id, timescale) = (track.track_id(), u64::from(track.timescale().max(1)));
        Ok(Self { reader, id, timescale, header, samples, end })
    }

    pub fn duration(&self) -> Duration {
        self.time(self.end)
    }

    fn time(&self, ticks: u64) -> Duration {
        Duration::from_micros(ticks * 1_000_000 / self.timescale)
    }

    // Decoding has to start at a keyframe: the last one shown at or before `at`.
    pub fn keyframe_before(&self, at: Duration) -> usize {
        self.samples.iter().enumerate().filter(|(_, s)| s.sync && self.time(s.pts) <= at).map(|(i, _)| i).last().unwrap_or(0)
    }

    // Decodes from sample `from` (a keyframe) to the end, handing `show` each picture with its time;
    // `show` breaks to stop early.
    pub fn decode(&mut self, from: usize, mut show: impl FnMut(Duration, &DecodedYUV) -> ControlFlow<()>) -> Result<(), String> {
        // The default flushes after every packet, forcing pictures out before the B-frames that reference
        // them arrive; the decoder then errors on every packet after. Without it, it reorders them itself.
        let config = DecoderConfig::new().flush_after_decode(Flush::NoFlush);
        let mut decoder = Decoder::with_api_config(OpenH264API::from_source(), config).map_err(|e| e.to_string())?;
        // Pictures leave the decoder in display order, so each one takes the earliest time still waiting.
        let mut waiting: Vec<u64> = Vec::new();
        for index in from..self.samples.len() {
            let Some(sample) = self.reader.read_sample(self.id, index as u32 + 1).map_err(|e| e.to_string())? else { continue };
            let mut unit = if index == from { self.header.clone() } else { Vec::new() };
            unit.extend(annex_b(&sample.bytes));
            let pts = self.samples[index].pts;
            waiting.push(pts);
            match decoder.decode(&unit) {
                Ok(Some(yuv)) => {
                    if let Some(at) = take_earliest(&mut waiting)
                        && show(self.time(at), &yuv).is_break()
                    {
                        return Ok(());
                    }
                }
                Ok(None) => {}
                Err(_) => waiting.retain(|p| *p != pts),
            }
        }
        for yuv in decoder.flush_remaining().unwrap_or_default() {
            if let Some(at) = take_earliest(&mut waiting)
                && show(self.time(at), &yuv).is_break()
            {
                break;
            }
        }
        Ok(())
    }
}

fn take_earliest(waiting: &mut Vec<u64>) -> Option<u64> {
    let (i, _) = waiting.iter().enumerate().min_by_key(|(_, t)| **t)?;
    Some(waiting.swap_remove(i))
}

// Builds each sample's display time and keyframe flag from the track's time-to-sample (stts),
// composition offset (ctts) and sync sample (stss) tables, plus where the clip ends. Times start at the
// first shown picture, as the edit list encoders write for B-frame offsets would have them.
fn sample_table(deltas: impl Iterator<Item = (u32, u32)>, offsets: impl Iterator<Item = (u32, i32)>, sync: Option<&[u32]>) -> (Vec<Sample>, u64) {
    let mut times: Vec<(i64, u32)> = Vec::new();
    let mut dts = 0i64;
    for (count, delta) in deltas {
        for _ in 0..count {
            times.push((dts, delta));
            dts += i64::from(delta);
        }
    }
    let mut index = 0;
    for (count, offset) in offsets {
        for _ in 0..count {
            if let Some((t, _)) = times.get_mut(index) {
                *t += i64::from(offset);
            }
            index += 1;
        }
    }
    let first = times.iter().map(|(t, _)| *t).min().unwrap_or(0);
    let end = times.iter().map(|(t, d)| t - first + i64::from(*d)).max().unwrap_or(0) as u64;
    let mut samples: Vec<Sample> = times.iter().map(|(t, _)| Sample { pts: (t - first) as u64, sync: sync.is_none() }).collect();
    for n in sync.into_iter().flatten() {
        if let Some(sample) = (*n as usize).checked_sub(1).and_then(|i| samples.get_mut(i)) {
            sample.sync = true;
        }
    }
    (samples, end)
}

// A picture as gpui draws it (BGRA), no wider than `max_width`.
pub fn bgra(yuv: &DecodedYUV, max_width: u32) -> Option<RgbaImage> {
    let (w, h) = yuv.dimensions();
    let mut pixels = vec![0u8; yuv.rgba8_len()];
    yuv.write_rgba8(&mut pixels);
    let mut picture = RgbaImage::from_raw(w as u32, h as u32, pixels)?;
    if picture.width() > max_width {
        let height = picture.height() * max_width / picture.width();
        picture = image::imageops::resize(&picture, max_width, height.max(1), FilterType::Triangle);
    }
    picture.pixels_mut().for_each(|p| p.0.swap(0, 2));
    Some(picture)
}

// MP4 stores NAL units behind 4-byte big-endian lengths; the decoder wants start codes.
fn annex_b(mut avcc: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(avcc.len() + 16);
    while avcc.len() >= 4 {
        let len = u32::from_be_bytes([avcc[0], avcc[1], avcc[2], avcc[3]]) as usize;
        let Some(unit) = avcc.get(4..4 + len) else { break };
        out.extend_from_slice(&[0, 0, 0, 1]);
        out.extend_from_slice(unit);
        avcc = &avcc[4 + len..];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown(bytes: &[u8], from: Duration) -> Vec<Duration> {
        let mut track = Track::open(bytes).expect("opens");
        let mut times = Vec::new();
        track
            .decode(track.keyframe_before(from), |at, _| {
                times.push(at);
                ControlFlow::Continue(())
            })
            .expect("decodes");
        times
    }

    // High profile with two B-frames between references: every picture decodes, in display order.
    #[test]
    fn b_frames_decode_in_display_order() {
        let times = shown(include_bytes!("../testdata/video-aac.mp4"), Duration::ZERO);
        assert_eq!(times.len(), 20);
        assert!(times.windows(2).all(|w| w[0] < w[1]), "{times:?}");
        assert_eq!(times[0], Duration::ZERO);
        assert_eq!(times[19], Duration::from_millis(1900));
        let gif = shown(include_bytes!("../testdata/gifv-bframes.mp4"), Duration::ZERO);
        assert_eq!(gif.len(), 15);
    }

    // The test clip has a keyframe every 10 frames (1s): seeking to 1.45s starts decoding at 1s.
    #[test]
    fn seeking_starts_at_the_keyframe_before() {
        let track = Track::open(include_bytes!("../testdata/video-aac.mp4").as_slice()).expect("opens");
        assert_eq!(track.duration(), Duration::from_secs(2));
        let times = shown(include_bytes!("../testdata/video-aac.mp4"), Duration::from_millis(1450));
        assert_eq!(times.first(), Some(&Duration::from_secs(1)));
        assert_eq!(times.len(), 10);
    }

    #[test]
    fn sample_table_applies_offsets_and_keyframes() {
        // I P B B as stored (decode order), shown I B B P: offsets push the P past both Bs.
        let (samples, end) = sample_table([(4, 10)].into_iter(), [(1, 10), (1, 30), (2, 0)].into_iter(), Some(&[1]));
        let pts: Vec<u64> = samples.iter().map(|s| s.pts).collect();
        assert_eq!(pts, [0, 30, 10, 20]);
        assert_eq!(end, 40);
        assert_eq!(samples.iter().map(|s| s.sync).collect::<Vec<_>>(), [true, false, false, false]);
        // No sync table means every sample is a keyframe.
        let (samples, _) = sample_table([(2, 5)].into_iter(), std::iter::empty(), None);
        assert!(samples.iter().all(|s| s.sync));
    }

    #[test]
    fn avcc_to_annex_b() {
        assert_eq!(annex_b(&[0, 0, 0, 2, 9, 9, 0, 0, 0, 1, 7]), [0, 0, 0, 1, 9, 9, 0, 0, 0, 1, 7]);
        assert_eq!(annex_b(&[0, 0, 0, 9, 1]), Vec::<u8>::new());
    }

    fn boxed(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = ((body.len() + 8) as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(body);
        out
    }

    #[test]
    fn quicktime_sound_entries_become_version_0() {
        let esds = boxed(b"esds", &[0, 0, 0, 0, 3, 25, 0, 1, 0]);
        let wave = boxed(b"wave", &[boxed(b"frma", b"mp4a"), esds.clone()].concat());
        // Version 1 entry: 6 reserved, data reference 1, version 1, 6 reserved, 2 channels, 16 bit,
        // 4 reserved, 44.1 kHz, then QuickTime's 16 extra bytes, then the `wave`.
        let mut fields = vec![0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0, 0, 0, 2, 0, 16, 0, 0, 0, 0, 0xAC, 0x44, 0, 0];
        fields.extend_from_slice(&[0xEE; 16]);
        fields.extend_from_slice(&wave);
        let mp4a = boxed(b"mp4a", &fields);
        let stsd = boxed(b"stsd", &[&[0, 0, 0, 0, 0, 0, 0, 1][..], &mp4a].concat());
        let moov = boxed(b"moov", &boxed(b"trak", &boxed(b"mdia", &boxed(b"minf", &boxed(b"stbl", &stsd)))));
        let file = [moov.clone(), boxed(b"mdat", &[1, 2, 3])].concat();

        let fixed = modernize_audio_entries(&file).expect("rewritten");
        assert_eq!(fixed.len(), file.len(), "no offsets move");
        let entry = fixed.windows(4).position(|w| w == b"mp4a").expect("entry") - 4;
        assert_eq!(&fixed[entry + 16..entry + 18], &[0, 0], "version 0");
        assert_eq!(&fixed[entry + SOUND_ENTRY_BASE..entry + SOUND_ENTRY_BASE + esds.len()], esds.as_slice(), "esds follows the standard fields");
        assert_eq!(&fixed[entry + SOUND_ENTRY_BASE + esds.len() + 4..entry + SOUND_ENTRY_BASE + esds.len() + 8], b"free");
        assert_eq!(&fixed[moov.len()..], &file[moov.len()..], "media data untouched");
        assert!(modernize_audio_entries(&fixed).is_none(), "version 0 is left alone");
    }

    #[test]
    fn quicktime_file_opens_after_the_rewrite() {
        // ffmpeg's .mov muxer writes AAC with a version 1 sound entry, as iPhones do.
        let file = include_bytes!("../testdata/quicktime-aac.mov");
        let strict = mp4::Mp4Reader::read_header(Cursor::new(file.as_slice()), file.len() as u64);
        assert!(strict.is_err(), "the crate rejects it as is");
        let fixed = modernize_audio_entries(file).expect("rewritten");
        let reader = mp4::Mp4Reader::read_header(Cursor::new(fixed.as_slice()), fixed.len() as u64).expect("opens");
        assert!(reader.tracks().values().any(|t| matches!(t.media_type(), Ok(mp4::MediaType::AAC))), "audio track kept");
        assert!(Track::open(fixed.as_slice()).is_ok(), "video track opens");
    }
}
