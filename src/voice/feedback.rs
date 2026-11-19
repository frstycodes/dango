// RTCP feedback for one voice-gateway connection, from webrtc-rs's interceptors: NACKs for the video
// we receive (`Generator`) and resends of the video we send when a viewer NACKs it (`Responder`),
// the jobs Discord's client leaves to libwebrtc. RTCP in and out is parsed and built by the `rtcp`
// crate; only the sealing (rtpsize AEAD) and the socket are ours.
//
// The interceptors are async (tokio) and our media threads are plain threads, so each call runs to
// completion in place (`run`): the awaits inside only take uncontended locks and call our own readers
// and writers, which return at once.

use std::{
    future::Future,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU16, Ordering},
    },
};

use bytes::{BufMut, Bytes, BytesMut};
use interceptor::{
    Attributes, Interceptor, RTCPReader, RTCPWriter, RTPReader, RTPWriter,
    nack::{generator::Generator, responder::Responder},
    registry::Registry,
    stream_info::{RTCPFeedback, StreamInfo},
};
use rtcp::{packet::Packet as RtcpPacket, transport_feedbacks::transport_layer_nack::TransportLayerNack};
use rtp::{header::Header, packet::Packet};
use webrtc_util::Marshal;

use super::{media::Link, rtp as wire};

// NACKs go out every 100 ms (the generator's default) for whatever is still missing in the last 512
// packets, about half a second of a 1080p stream; older gaps are given up on.
const NACK_LOG2_SIZE_MINUS_6: u8 = 3;
// Sent packets kept for resending, about a second of a 1080p stream.
const RESEND_LOG2_SIZE: u8 = 10;

type Rtcp = Box<dyn RtcpPacket + Send + Sync>;

pub struct Feedback {
    chain: Arc<dyn Interceptor + Send + Sync>,
    rtcp_in: Arc<dyn RTCPReader + Send + Sync>,
    rtcp_slot: Arc<Slot<Bytes>>,
}

impl Feedback {
    pub fn new(link: &Arc<Link>) -> Result<Arc<Self>, String> {
        let mut registry = Registry::new();
        registry.add(Box::new(Generator::builder().with_log2_size_minus_6(NACK_LOG2_SIZE_MINUS_6)));
        registry.add(Box::new(Responder::builder().with_log2_size(RESEND_LOG2_SIZE)));
        let chain = registry.build("").map_err(|e| e.to_string())?;
        let rtcp_slot = Arc::new(Slot::default());
        let rtcp_in = run(async {
            chain.bind_rtcp_writer(Arc::new(RtcpOut(link.clone()))).await;
            chain.bind_rtcp_reader(rtcp_slot.clone()).await
        });
        Ok(Arc::new(Self { chain, rtcp_in, rtcp_slot }))
    }

    // Decrypted RTCP from the server, parsed; the responder resends whatever a NACK in it names.
    pub fn rtcp_in(&self, rtcp: Bytes) -> Vec<Rtcp> {
        self.rtcp_slot.put(rtcp);
        run(self.rtcp_in.read(&mut [], &Attributes::new())).map(|(packets, _)| packets).unwrap_or_default()
    }

    // A remote picture whose losses get NACKed.
    pub fn receive_video(self: &Arc<Self>, ssrc: u32) -> RemoteVideo {
        let slot = Arc::new(Slot::default());
        let reader = run(self.chain.bind_remote_stream(&nack_stream(ssrc), slot.clone()));
        RemoteVideo { feedback: self.clone(), ssrc, slot, reader }
    }

    // Our picture, sent through the responder so it can resend packets viewers NACK.
    pub fn send_video(&self, link: &Arc<Link>) -> Arc<dyn RTPWriter + Send + Sync> {
        let out = Arc::new(VideoOut { link: link.clone(), last_sequence: Mutex::new(None), rtx_sequence: AtomicU16::new(rand::random()) });
        run(self.chain.bind_local_stream(&nack_stream(link.video_ssrc), out))
    }
}

impl Drop for Feedback {
    // Ends the generator's ticker; async, and dropped from async code, so it can't run in place.
    fn drop(&mut self) {
        let chain = self.chain.clone();
        crate::discord::runtime().spawn(async move { chain.close().await });
    }
}

pub struct RemoteVideo {
    feedback: Arc<Feedback>,
    ssrc: u32,
    slot: Arc<Slot<Packet>>,
    reader: Arc<dyn RTPReader + Send + Sync>,
}

impl RemoteVideo {
    // Logs the packet's arrival for the NACK generator and hands it back.
    pub fn arrived(&self, packet: Packet) -> Option<Packet> {
        self.slot.put(packet);
        run(self.reader.read(&mut [], &Attributes::new())).ok().map(|(packet, _)| packet)
    }
}

impl Drop for RemoteVideo {
    fn drop(&mut self) {
        run(self.feedback.chain.unbind_remote_stream(&nack_stream(self.ssrc)));
    }
}

fn nack_stream(ssrc: u32) -> StreamInfo {
    StreamInfo { ssrc, rtcp_feedback: vec![RTCPFeedback { typ: "nack".into(), parameter: String::new() }], ..Default::default() }
}

// Some interceptor calls tokio::spawn, so the shared runtime is entered first.
pub(super) fn run<F: Future>(future: F) -> F::Output {
    let _runtime = crate::discord::runtime().enter();
    futures::executor::block_on(future)
}

// The innermost reader: hands the interceptors the one packet just put in.
struct Slot<T>(Mutex<Option<T>>);

impl<T> Default for Slot<T> {
    fn default() -> Self {
        Self(Mutex::new(None))
    }
}

impl<T> Slot<T> {
    fn put(&self, value: T) {
        if let Ok(mut slot) = self.0.lock() {
            *slot = Some(value);
        }
    }

    fn take(&self) -> Result<T, interceptor::Error> {
        self.0.lock().ok().and_then(|mut slot| slot.take()).ok_or_else(|| interceptor::Error::Other("nothing to read".into()))
    }
}

#[async_trait::async_trait]
impl RTPReader for Slot<Packet> {
    async fn read(&self, _: &mut [u8], _: &Attributes) -> Result<(Packet, Attributes), interceptor::Error> {
        Ok((self.take()?, Attributes::new()))
    }
}

#[async_trait::async_trait]
impl RTCPReader for Slot<Bytes> {
    async fn read(&self, _: &mut [u8], _: &Attributes) -> Result<(Vec<Rtcp>, Attributes), interceptor::Error> {
        let mut rtcp = self.take()?;
        Ok((rtcp::packet::unmarshal(&mut rtcp)?, Attributes::new()))
    }
}

// The generator's NACKs, sealed and sent. It signs them with a random SSRC; Discord's client signs
// its feedback with its own video SSRC, so this does too.
struct RtcpOut(Arc<Link>);

#[async_trait::async_trait]
impl RTCPWriter for RtcpOut {
    async fn write(&self, packets: &[Rtcp], _: &Attributes) -> Result<usize, interceptor::Error> {
        for packet in packets {
            match packet.as_any().downcast_ref::<TransportLayerNack>() {
                Some(nack) => send_rtcp(&self.0, &TransportLayerNack { sender_ssrc: self.0.feedback_ssrc(), ..nack.clone() }),
                None => send_rtcp(&self.0, packet.as_ref()),
            }
        }
        Ok(packets.len())
    }
}

// Seals one RTCP packet with the 8-byte header as AAD and sends it.
pub fn send_rtcp(link: &Link, packet: &(impl RtcpPacket + ?Sized)) {
    let Ok(raw) = packet.marshal() else { return };
    let (header, body) = raw.split_at(wire::RTCP_HEADER_LEN);
    if let Some(sealed) = link.cipher.seal(header, body, link.next_nonce()) {
        let _ = link.socket.send(&sealed);
    }
}

// Our video packets onto the socket. The responder hands resends to the same writer; they go out as
// RTX on the retransmission SSRC, as Discord's client sends them (it requires an RTX SSRC on every
// video send stream).
struct VideoOut {
    link: Arc<Link>,
    last_sequence: Mutex<Option<u16>>,
    rtx_sequence: AtomicU16,
}

#[async_trait::async_trait]
impl RTPWriter for VideoOut {
    async fn write(&self, packet: &Packet, _: &Attributes) -> Result<usize, interceptor::Error> {
        let sequence = packet.header.sequence_number;
        let resend = {
            let mut last = self.last_sequence.lock().map_err(|e| interceptor::Error::Other(e.to_string()))?;
            let resend = last.is_some_and(|last| (sequence.wrapping_sub(last) as i16) <= 0);
            if !resend {
                *last = Some(sequence);
            }
            resend
        };
        let rtx = (resend && self.link.rtx_ssrc != 0).then(|| rtx(packet, self.link.rtx_ssrc, self.rtx_sequence.fetch_add(1, Ordering::Relaxed)));
        send_rtp(&self.link, rtx.as_ref().unwrap_or(packet));
        Ok(packet.payload.len())
    }
}

// RFC 4588: a retransmission on the RTX SSRC carries the original sequence number ahead of the
// original payload.
pub fn rtx(packet: &Packet, rtx_ssrc: u32, rtx_sequence: u16) -> Packet {
    let mut payload = BytesMut::with_capacity(2 + packet.payload.len());
    payload.put_u16(packet.header.sequence_number);
    payload.put_slice(&packet.payload);
    let header = Header { payload_type: wire::H264_RTX_PAYLOAD_TYPE, ssrc: rtx_ssrc, sequence_number: rtx_sequence, ..packet.header.clone() };
    Packet { header, payload: payload.freeze() }
}

// The reverse, for retransmissions we receive: None for padding-only probes.
pub fn unwrap_rtx(packet: &Packet, media_ssrc: u32) -> Option<Packet> {
    let (original, payload) = packet.payload.split_first_chunk::<2>()?;
    if payload.is_empty() {
        return None;
    }
    let header = Header { payload_type: wire::H264_PAYLOAD_TYPE, ssrc: media_ssrc, sequence_number: u16::from_be_bytes(*original), ..packet.header.clone() };
    Some(Packet { header, payload: Bytes::copy_from_slice(payload) })
}

fn send_rtp(link: &Link, packet: &Packet) {
    let Ok(header) = packet.header.marshal() else { return };
    if let Some(sealed) = link.cipher.seal(&header, &packet.payload, link.next_nonce()) {
        let _ = link.socket.send(&sealed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rtx_round_trip_keeps_the_original() {
        let header = Header { version: 2, marker: true, payload_type: wire::H264_PAYLOAD_TYPE, sequence_number: 4242, timestamp: 9000, ssrc: 7, ..Default::default() };
        let packet = Packet { header, payload: Bytes::from_static(&[0x65, 1, 2, 3]) };
        let resent = rtx(&packet, 8, 1);
        assert_eq!((resent.header.ssrc, resent.header.sequence_number, resent.header.payload_type), (8, 1, wire::H264_RTX_PAYLOAD_TYPE));
        assert_eq!(&resent.payload[..2], &4242u16.to_be_bytes());
        assert_eq!(unwrap_rtx(&resent, 7), Some(packet));
        let probe = Packet { payload: Bytes::from_static(&[0, 1]), ..resent };
        assert_eq!(unwrap_rtx(&probe, 7), None, "padding-only probes carry no packet");
    }
}
