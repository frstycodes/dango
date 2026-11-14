// H.264 over RTP (RFC 6184): receive (depacketize → OpenH264 decode → BGRA frames) and send
// (BGRA → OpenH264 encode → packetize).
// ponytail: H.264 only, and it is the only video codec we advertise, so Discord should not send
// VP8/VP9/AV1 to us. Add those decoders (libvpx, dav1d) if streams arrive in another codec.
// ponytail: software encode (OpenH264, also portable to Windows/Linux); VideoToolbox hardware
// encode on macOS if CPU use while streaming matters.

use openh264::{
    OpenH264API,
    decoder::Decoder,
    encoder::{BitRate, Encoder, EncoderConfig, FrameRate, IntraFramePeriod, UsageType},
    formats::{BgraSliceU8, YUVBuffer, YUVSource},
};

const START_CODE: [u8; 4] = [0, 0, 0, 1];
const STAP_A: u8 = 24;
const FU_A: u8 = 28;
const NAL_IDR: u8 = 5;

// One decoded picture. Pixels are BGRA, the order GPUI's RenderImage expects.
pub struct VideoFrame {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

// ponytail: any lost or reordered packet drops the whole frame and decoding waits for the next
// keyframe; add a reorder buffer and RTCP PLI keyframe requests if streams freeze on lossy links.
#[derive(Default)]
pub struct Depacketizer {
    unit: Vec<u8>,
    last_sequence: Option<u16>,
    broken: bool,
}

impl Depacketizer {
    // Returns a complete access unit when the packet carrying the RTP marker bit arrives.
    pub fn push(&mut self, sequence: u16, marker: bool, payload: &[u8]) -> Option<Vec<u8>> {
        if self.last_sequence.is_some_and(|last| sequence != last.wrapping_add(1)) {
            self.broken = true;
        }
        self.last_sequence = Some(sequence);
        if !self.unpack(payload) {
            self.broken = true;
        }
        if !marker {
            return None;
        }
        let unit = std::mem::take(&mut self.unit);
        let broken = std::mem::replace(&mut self.broken, false);
        (!broken && !unit.is_empty()).then_some(unit)
    }

    fn unpack(&mut self, payload: &[u8]) -> bool {
        let Some(&first) = payload.first() else { return false };
        match first & 0x1f {
            1..=23 => {
                self.unit.extend_from_slice(&START_CODE);
                self.unit.extend_from_slice(payload);
            }
            STAP_A => {
                let mut rest = &payload[1..];
                while let Some((size, tail)) = rest.split_first_chunk::<2>() {
                    let Some((nal, next)) = tail.split_at_checked(u16::from_be_bytes(*size) as usize) else { return false };
                    self.unit.extend_from_slice(&START_CODE);
                    self.unit.extend_from_slice(nal);
                    rest = next;
                }
            }
            FU_A => {
                let Some(&header) = payload.get(1) else { return false };
                if header & 0x80 != 0 {
                    self.unit.extend_from_slice(&START_CODE);
                    self.unit.push((first & 0xe0) | (header & 0x1f));
                }
                self.unit.extend_from_slice(&payload[2..]);
            }
            _ => return false,
        }
        true
    }
}

pub struct VideoDecoder {
    decoder: Decoder,
    awaiting_keyframe: bool,
}

impl VideoDecoder {
    pub fn new() -> Result<Self, String> {
        Ok(Self { decoder: Decoder::new().map_err(|e| e.to_string())?, awaiting_keyframe: true })
    }

    // A dropped frame breaks the reference chain, so frames are skipped until the next IDR.
    pub fn frame_lost(&mut self) {
        self.awaiting_keyframe = true;
    }

    pub fn awaiting_keyframe(&self) -> bool {
        self.awaiting_keyframe
    }

    pub fn decode(&mut self, unit: &[u8]) -> Option<VideoFrame> {
        if self.awaiting_keyframe && !has_idr(unit) {
            return None;
        }
        self.awaiting_keyframe = false;
        let yuv = match self.decoder.decode(unit) {
            Ok(yuv) => yuv?,
            Err(_) => {
                self.awaiting_keyframe = true;
                return None;
            }
        };
        let (width, height) = yuv.dimensions();
        let mut bgra = vec![0u8; yuv.rgba8_len()];
        yuv.write_rgba8(&mut bgra);
        bgra.as_chunks_mut::<4>().0.iter_mut().for_each(|px| px.swap(0, 2));
        Some(VideoFrame { width: width as u32, height: height as u32, bgra })
    }
}

pub struct VideoEncoder {
    encoder: Encoder,
}

impl VideoEncoder {
    // A keyframe every few seconds lets late viewers start even if keyframe requests are lost.
    pub fn new(fps: u32, bitrate: u32) -> Result<Self, String> {
        let config = EncoderConfig::new()
            .usage_type(UsageType::ScreenContentRealTime)
            .adaptive_quantization(false)
            .background_detection(false)
            .bitrate(BitRate::from_bps(bitrate))
            .max_frame_rate(FrameRate::from_hz(fps as f32))
            .intra_frame_period(IntraFramePeriod::from_num_frames(fps * 3));
        let encoder = Encoder::with_api_config(OpenH264API::from_source(), config).map_err(|e| e.to_string())?;
        Ok(Self { encoder })
    }

    pub fn force_keyframe(&mut self) {
        self.encoder.force_intra_frame();
    }

    // One Annex-B access unit, or None when the rate controller skipped the frame.
    pub fn encode(&mut self, frame: &VideoFrame) -> Option<Vec<u8>> {
        let size = (frame.width as usize, frame.height as usize);
        let yuv = YUVBuffer::from_rgb8_source(BgraSliceU8::new(&frame.bgra, size));
        let unit = self.encoder.encode(&yuv).ok()?.to_vec();
        (!unit.is_empty()).then_some(unit)
    }
}

// RTP payloads for one access unit: each NAL whole if it fits `mtu`, else as FU-A fragments.
pub fn packetize(unit: &[u8], mtu: usize) -> Vec<Vec<u8>> {
    let mut packets = Vec::new();
    for nal in nal_units(unit) {
        let Some((&header, body)) = nal.split_first() else { continue };
        if nal.len() <= mtu {
            packets.push(nal.to_vec());
            continue;
        }
        let chunks: Vec<&[u8]> = body.chunks(mtu - 2).collect();
        let last = chunks.len() - 1;
        for (i, chunk) in chunks.into_iter().enumerate() {
            let start = if i == 0 { 0x80 } else { 0 };
            let end = if i == last { 0x40 } else { 0 };
            let mut packet = vec![(header & 0xe0) | FU_A, start | end | (header & 0x1f)];
            packet.extend_from_slice(chunk);
            packets.push(packet);
        }
    }
    packets
}

fn has_idr(unit: &[u8]) -> bool {
    nal_units(unit).iter().any(|nal| nal.first().is_some_and(|h| h & 0x1f == NAL_IDR))
}

// NAL units of an Annex-B stream, without start codes. A zero before `00 00 01` belongs to a
// 4-byte start code, not the previous NAL — the same split DAVE uses, so bytes stay identical.
fn nal_units(unit: &[u8]) -> Vec<&[u8]> {
    let starts: Vec<usize> = unit.windows(3).enumerate().filter(|(_, w)| *w == [0, 0, 1]).map(|(i, _)| i).collect();
    starts
        .iter()
        .enumerate()
        .map(|(n, &at)| {
            let end = starts.get(n + 1).map_or(unit.len(), |&next| if next > 0 && unit[next - 1] == 0 { next - 1 } else { next });
            &unit[at + 3..end.max(at + 3)]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use openh264::{
        encoder::Encoder,
        formats::{RgbSliceU8, YUVBuffer},
    };

    use super::*;

    #[test]
    fn depacketizes_single_stap_and_fragmented_nals() {
        let mut d = Depacketizer::default();
        assert_eq!(d.push(1, false, &[0x67, 1, 2]), None);
        assert_eq!(d.push(2, false, &[0x18, 0, 2, 0x68, 9, 0, 1, 0x06]), None);
        assert_eq!(d.push(3, false, &[0x7c, 0x85, 0xaa]), None);
        let unit = d.push(4, true, &[0x7c, 0x45, 0xbb]).unwrap();
        assert_eq!(
            unit,
            [0, 0, 0, 1, 0x67, 1, 2, 0, 0, 0, 1, 0x68, 9, 0, 0, 0, 1, 0x06, 0, 0, 0, 1, 0x65, 0xaa, 0xbb]
        );
        assert!(has_idr(&unit));
        assert!(!has_idr(&[0, 0, 0, 1, 0x41, 0]));
    }

    #[test]
    fn sequence_gap_drops_the_frame() {
        let mut d = Depacketizer::default();
        d.push(10, false, &[0x7c, 0x85, 1]);
        assert_eq!(d.push(12, true, &[0x7c, 0x45, 2]), None);
        assert!(d.push(13, true, &[0x41, 3]).is_some());
    }

    #[test]
    #[ignore]
    fn bench_720p_encode() {
        let (w, h) = (1280u32, 720u32);
        let mut encoder = VideoEncoder::new(30, 2_500_000).unwrap();
        let mut total = std::time::Duration::ZERO;
        for i in 0..60u32 {
            // A moving gradient so every frame differs, like scrolling content.
            let bgra: Vec<u8> = (0..w * h).flat_map(|p| { let x = (p % w + i * 8) as u8; let y = (p / w) as u8; [x, y, x ^ y, 255] }).collect();
            let frame = VideoFrame { width: w, height: h, bgra };
            let t = std::time::Instant::now();
            encoder.encode(&frame);
            total += t.elapsed();
        }
        eprintln!("BENCH avg encode (incl. BGRA->YUV) per 720p frame: {:?}", total / 60);
    }

    #[test]
    fn encode_packetize_depacketize_decode_round_trip() {
        let (width, height) = (96, 64);
        let bgra: Vec<u8> = (0..width * height).flat_map(|i| [(i % 251) as u8, 40, 200, 255]).collect();
        let frame = VideoFrame { width, height, bgra };
        let mut encoder = VideoEncoder::new(30, 500_000).unwrap();
        let unit = encoder.encode(&frame).unwrap();

        let packets = packetize(&unit, 100);
        assert!(packets.iter().all(|p| p.len() <= 100));
        assert!(packets.iter().any(|p| p[0] & 0x1f == FU_A), "small MTU forces fragmentation");
        let mut depacketizer = Depacketizer::default();
        let last = packets.len() - 1;
        let rebuilt = packets.iter().enumerate().find_map(|(i, p)| depacketizer.push(i as u16, i == last, p)).unwrap();
        assert_eq!(rebuilt, unit, "Annex-B survives the RTP round trip byte for byte");

        let decoded = VideoDecoder::new().unwrap().decode(&rebuilt).unwrap();
        assert_eq!((decoded.width, decoded.height), (width, height));
    }

    #[test]
    fn decodes_openh264_keyframe_to_bgra() {
        let (w, h) = (64, 48);
        let red: Vec<u8> = [255u8, 0, 0].repeat(w * h);
        let yuv = YUVBuffer::from_rgb8_source(RgbSliceU8::new(&red, (w, h)));
        let mut encoder = Encoder::new().unwrap();
        let unit = encoder.encode(&yuv).unwrap().to_vec();

        let mut decoder = VideoDecoder::new().unwrap();
        assert!(decoder.decode(&[0, 0, 0, 1, 0x41, 0]).is_none(), "non-IDR before a keyframe is skipped");
        let frame = decoder.decode(&unit).unwrap();
        assert_eq!((frame.width, frame.height, frame.bgra.len()), (64, 48, 64 * 48 * 4));
        let px = &frame.bgra[..4];
        assert!(px[2] > 200 && px[0] < 60, "red lands in the R slot of BGRA: {px:?}");
    }
}
