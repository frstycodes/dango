use aes_gcm::{Aes256Gcm, aead::{Aead, KeyInit, Payload}};
use chacha20poly1305::XChaCha20Poly1305;

pub const HEADER_LEN: usize = 12;
pub const OPUS_PAYLOAD_TYPE: u8 = 120;
pub const H264_PAYLOAD_TYPE: u8 = 101;
pub const H264_RTX_PAYLOAD_TYPE: u8 = 102;
const NONCE_SUFFIX_LEN: usize = 4;
const DISCOVERY_LEN: usize = 74;

// Preference order; Discord requires clients to support xchacha, aes-gcm is preferred when offered.
pub const MODES: [&str; 2] = ["aead_aes256_gcm_rtpsize", "aead_xchacha20_poly1305_rtpsize"];

pub fn discovery_request(ssrc: u32) -> [u8; DISCOVERY_LEN] {
    let mut packet = [0u8; DISCOVERY_LEN];
    packet[0..2].copy_from_slice(&1u16.to_be_bytes());
    packet[2..4].copy_from_slice(&70u16.to_be_bytes());
    packet[4..8].copy_from_slice(&ssrc.to_be_bytes());
    packet
}

pub fn discovery_response(packet: &[u8]) -> Option<(String, u16)> {
    if packet.len() < DISCOVERY_LEN || packet[0..2] != 2u16.to_be_bytes() {
        return None;
    }
    let address = &packet[8..72];
    let end = address.iter().position(|&b| b == 0).unwrap_or(address.len());
    let ip = std::str::from_utf8(&address[..end]).ok()?.to_owned();
    Some((ip, u16::from_be_bytes([packet[72], packet[73]])))
}

pub fn header(payload_type: u8, marker: bool, sequence: u16, timestamp: u32, ssrc: u32) -> [u8; HEADER_LEN] {
    let mut h = [0u8; HEADER_LEN];
    h[0] = 0x80;
    h[1] = payload_type | if marker { 0x80 } else { 0 };
    h[2..4].copy_from_slice(&sequence.to_be_bytes());
    h[4..8].copy_from_slice(&timestamp.to_be_bytes());
    h[8..12].copy_from_slice(&ssrc.to_be_bytes());
    h
}

pub const RTCP_HEADER_LEN: usize = 8;

// RTCP picture loss indication (RFC 4585 §6.3.1): asks the sender of `media_ssrc` for a keyframe.
// Sealed like RTP under rtpsize, with the 8-byte RTCP header as AAD — Not verified for Discord.
pub fn pli(sender_ssrc: u32, media_ssrc: u32) -> [u8; 12] {
    let mut p = [0u8; 12];
    p[0] = 0x81;
    p[1] = 206;
    p[2..4].copy_from_slice(&2u16.to_be_bytes());
    p[4..8].copy_from_slice(&sender_ssrc.to_be_bytes());
    p[8..12].copy_from_slice(&media_ssrc.to_be_bytes());
    p
}

// The loss (percent) a sender or receiver report gives for `ssrc`: RTCP's fraction-lost byte.
pub fn reported_loss(rtcp: &[u8], ssrc: u32) -> Option<f32> {
    compound(rtcp).find_map(|packet| {
        let blocks_at = match packet.get(1)? {
            200 => 28,
            201 => 8,
            _ => return None,
        };
        let count = (packet[0] & 0x1f) as usize;
        (0..count).find_map(|i| {
            let block = packet.get(blocks_at + i * 24..blocks_at + i * 24 + 24)?;
            (u32::from_be_bytes(block[..4].try_into().ok()?) == ssrc).then(|| block[4] as f32 / 256.0 * 100.0)
        })
    })
}

// The packets inside a compound RTCP packet.
fn compound(rtcp: &[u8]) -> impl Iterator<Item = &[u8]> {
    let mut rest = rtcp;
    std::iter::from_fn(move || {
        let [_, _, hi, lo, ..] = *rest else { return None };
        let len = ((u16::from_be_bytes([hi, lo]) as usize) + 1) * 4;
        let (packet, next) = rest.split_at_checked(len)?;
        rest = next;
        Some(packet)
    })
}

// Any picture loss indication in a (possibly compound) RTCP packet.
pub fn is_pli(rtcp: &[u8]) -> bool {
    compound(rtcp).any(|p| p[1] == 206 && p[0] & 0x1f == 1)
}

pub fn is_rtcp(packet: &[u8]) -> bool {
    // RFC 5761: RTCP packet types occupy 192–223 in the byte where RTP keeps marker + payload type.
    packet.len() > 1 && (192..=223).contains(&packet[1])
}

pub enum Cipher {
    Aes(Box<Aes256Gcm>),
    XChaCha(Box<XChaCha20Poly1305>),
}

pub struct Received {
    pub ssrc: u32,
    pub payload_type: u8,
    pub marker: bool,
    pub sequence: u16,
    pub timestamp: u32,
    pub payload: Vec<u8>,
}

impl Cipher {
    pub fn new(mode: &str, key: &[u8]) -> Option<Self> {
        match mode {
            "aead_aes256_gcm_rtpsize" => Aes256Gcm::new_from_slice(key).ok().map(|c| Self::Aes(Box::new(c))),
            "aead_xchacha20_poly1305_rtpsize" => XChaCha20Poly1305::new_from_slice(key).ok().map(|c| Self::XChaCha(Box::new(c))),
            _ => None,
        }
    }

    // rtpsize: the unencrypted header is the AAD, the 32-bit counter sits at the front of a
    // zero-padded nonce and is appended to the packet in the clear.
    pub fn seal(&self, header: &[u8], payload: &[u8], counter: u32) -> Option<Vec<u8>> {
        let sealed = self.apply(header, payload, counter, true)?;
        let mut packet = Vec::with_capacity(header.len() + sealed.len() + NONCE_SUFFIX_LEN);
        packet.extend_from_slice(header);
        packet.extend_from_slice(&sealed);
        packet.extend_from_slice(&counter.to_be_bytes());
        Some(packet)
    }

    // The RTP extension header (4 bytes) is part of the AAD; its body is encrypted and stripped here.
    pub fn open(&self, packet: &[u8]) -> Option<Received> {
        if packet.len() < HEADER_LEN + NONCE_SUFFIX_LEN || packet[0] >> 6 != 2 {
            return None;
        }
        let csrc_count = (packet[0] & 0x0f) as usize;
        let has_extension = packet[0] & 0x10 != 0;
        let mut aad_len = HEADER_LEN + csrc_count * 4;
        if has_extension {
            aad_len += 4;
        }
        let body_end = packet.len() - NONCE_SUFFIX_LEN;
        if aad_len > body_end {
            return None;
        }
        let counter = u32::from_be_bytes(packet[body_end..].try_into().ok()?);
        let mut plain = self.apply(&packet[..aad_len], &packet[aad_len..body_end], counter, false)?;
        // Padding sits inside the encrypted payload; its last byte is the padding length.
        if packet[0] & 0x20 != 0 {
            let padding = *plain.last()? as usize;
            plain.truncate(plain.len().checked_sub(padding)?);
        }
        if has_extension {
            let words = u16::from_be_bytes([packet[aad_len - 2], packet[aad_len - 1]]) as usize;
            if words * 4 > plain.len() {
                return None;
            }
            plain.drain(..words * 4);
        }
        Some(Received {
            ssrc: u32::from_be_bytes(packet[8..12].try_into().ok()?),
            payload_type: packet[1] & 0x7f,
            marker: packet[1] & 0x80 != 0,
            sequence: u16::from_be_bytes([packet[2], packet[3]]),
            timestamp: u32::from_be_bytes(packet[4..8].try_into().ok()?),
            payload: plain,
        })
    }

    // RTCP under rtpsize: the 8-byte RTCP header is the AAD. Returns header + decrypted body.
    pub fn open_rtcp(&self, packet: &[u8]) -> Option<Vec<u8>> {
        let body_end = packet.len().checked_sub(NONCE_SUFFIX_LEN)?;
        if body_end < RTCP_HEADER_LEN {
            return None;
        }
        let counter = u32::from_be_bytes(packet[body_end..].try_into().ok()?);
        let mut rtcp = packet[..RTCP_HEADER_LEN].to_vec();
        rtcp.extend(self.apply(&packet[..RTCP_HEADER_LEN], &packet[RTCP_HEADER_LEN..body_end], counter, false)?);
        Some(rtcp)
    }

    fn apply(&self, aad: &[u8], msg: &[u8], counter: u32, encrypt: bool) -> Option<Vec<u8>> {
        let payload = Payload { msg, aad };
        match self {
            Self::Aes(c) => {
                let mut nonce = [0u8; 12];
                nonce[..4].copy_from_slice(&counter.to_be_bytes());
                if encrypt { c.encrypt(&nonce.into(), payload) } else { c.decrypt(&nonce.into(), payload) }.ok()
            }
            Self::XChaCha(c) => {
                let mut nonce = [0u8; 24];
                nonce[..4].copy_from_slice(&counter.to_be_bytes());
                if encrypt { c.encrypt(&nonce.into(), payload) } else { c.decrypt(&nonce.into(), payload) }.ok()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_round_trip() {
        let req = discovery_request(0xdead_beef);
        assert_eq!(&req[..8], &[0, 1, 0, 70, 0xde, 0xad, 0xbe, 0xef]);

        let mut res = req;
        res[1] = 2;
        res[8..8 + 11].copy_from_slice(b"203.0.113.9");
        res[72..74].copy_from_slice(&50_004u16.to_be_bytes());
        assert_eq!(discovery_response(&res), Some(("203.0.113.9".into(), 50_004)));
        assert_eq!(discovery_response(&req), None);
    }

    #[test]
    fn rtp_header_layout() {
        let h = header(OPUS_PAYLOAD_TYPE, false, 0x0102, 0x0304_0506, 0x0708_090a);
        assert_eq!(header(H264_PAYLOAD_TYPE, true, 0, 0, 0)[1], 0x80 | 101);
        assert_eq!(h, [0x80, 0x78, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
        assert!(!is_rtcp(&h));
        assert!(is_rtcp(&[0x81, 201, 0, 0]));
    }

    #[test]
    fn pli_layout_seal_and_detect() {
        let p = pli(0x0102_0304, 0x0a0b_0c0d);
        assert_eq!(p, [0x81, 206, 0, 2, 1, 2, 3, 4, 0x0a, 0x0b, 0x0c, 0x0d]);
        assert!(is_rtcp(&p));

        let cipher = Cipher::new(MODES[0], &[3u8; 32]).unwrap();
        let (header, body) = p.split_at(RTCP_HEADER_LEN);
        let opened = cipher.open_rtcp(&cipher.seal(header, body, 7).unwrap()).unwrap();
        assert_eq!(opened, p);

        let receiver_report = [0x80, 201, 0, 1, 0, 0, 0, 9];
        let mut rr = vec![0x81, 201, 0, 7, 0, 0, 0, 9];
        rr.extend_from_slice(&42u32.to_be_bytes());
        rr.push(64);
        rr.extend_from_slice(&[0; 19]);
        assert_eq!(reported_loss(&rr, 42), Some(25.0), "fraction lost 64/256");
        assert_eq!(reported_loss(&rr, 7), None);
        assert!(is_pli(&[&receiver_report[..], &p[..]].concat()), "PLI found inside a compound packet");
        assert!(!is_pli(&receiver_report));
    }

    #[test]
    fn seal_open_round_trip_both_modes() {
        for mode in MODES {
            let cipher = Cipher::new(mode, &[7u8; 32]).unwrap();
            let packet = cipher.seal(&header(OPUS_PAYLOAD_TYPE, false, 9, 960, 42), b"opus", 5).unwrap();
            let got = cipher.open(&packet).unwrap();
            assert_eq!((got.ssrc, got.payload_type, got.marker, got.sequence), (42, OPUS_PAYLOAD_TYPE, false, 9));
            assert_eq!(got.payload, b"opus");

            let mut tampered = packet.clone();
            tampered[3] ^= 1;
            assert!(cipher.open(&tampered).is_none(), "{mode}: header is authenticated");
        }
    }

    #[test]
    fn open_strips_padding() {
        let cipher = Cipher::new(MODES[0], &[2u8; 32]).unwrap();
        let mut aad = header(OPUS_PAYLOAD_TYPE, false, 1, 2, 3).to_vec();
        aad[0] |= 0x20;
        let packet = cipher.seal(&aad, &[b'h', b'i', 0, 0, 3], 0).unwrap();
        assert_eq!(cipher.open(&packet).unwrap().payload, b"hi");
    }

    #[test]
    fn open_strips_encrypted_extension_body() {
        let cipher = Cipher::new(MODES[0], &[1u8; 32]).unwrap();
        let mut aad = header(OPUS_PAYLOAD_TYPE, false, 1, 2, 3).to_vec();
        aad[0] |= 0x10;
        aad.extend_from_slice(&[0xbe, 0xde, 0, 1]);
        let packet = cipher.seal(&aad, &[0xaa, 0xbb, 0xcc, 0xdd, b'h', b'i'], 0).unwrap();
        assert_eq!(cipher.open(&packet).unwrap().payload, b"hi");
    }
}
