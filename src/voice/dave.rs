// DAVE (E2EE) opcodes 21–31 on the voice gateway, per daveprotocol.com. The MLS work is done by
// `davey`; this file only moves bytes between it and the socket.

use std::{
    collections::{HashMap, HashSet},
    num::NonZeroU16,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use davey::{DaveSession, ProposalsOperationType};
use futures::channel::mpsc;
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message as WsMessage;

const PREPARE_TRANSITION: u64 = 21;
const EXECUTE_TRANSITION: u64 = 22;
const TRANSITION_READY: u64 = 23;
const PREPARE_EPOCH: u64 = 24;
const EXTERNAL_SENDER: u8 = 25;
const KEY_PACKAGE: u8 = 26;
const PROPOSALS: u8 = 27;
const COMMIT_WELCOME: u8 = 28;
const ANNOUNCE_COMMIT: u8 = 29;
const WELCOME: u8 = 30;
const INVALID_COMMIT_WELCOME: u64 = 31;

pub struct Dave {
    session: Arc<Mutex<DaveSession>>,
    e2ee: Arc<AtomicBool>,
    user_id: u64,
    channel_id: u64,
    // Everyone the voice gateway says is in the call; MLS membership is checked against it.
    users: HashSet<u64>,
    pending: HashMap<u16, u16>,
    ws: mpsc::UnboundedSender<WsMessage>,
}

// Server → client binary frames: [seq u16][opcode u8][payload].
pub fn parse_binary(frame: &[u8]) -> Option<(u16, u8, &[u8])> {
    let (head, body) = frame.split_at_checked(3)?;
    Some((u16::from_be_bytes([head[0], head[1]]), head[2], body))
}

// Client → server binary frames: [opcode u8][payload].
fn binary(op: u8, payload: &[&[u8]]) -> WsMessage {
    let mut frame = vec![op];
    payload.iter().for_each(|part| frame.extend_from_slice(part));
    WsMessage::Binary(frame)
}

fn split_transition(body: &[u8]) -> Option<(u16, &[u8])> {
    let (id, rest) = body.split_at_checked(2)?;
    Some((u16::from_be_bytes([id[0], id[1]]), rest))
}

impl Dave {
    pub fn new(user_id: u64, channel_id: u64, ws: mpsc::UnboundedSender<WsMessage>) -> Result<Self, String> {
        let version = NonZeroU16::new(davey::DAVE_PROTOCOL_VERSION).ok_or("DAVE version 0")?;
        let session = DaveSession::new(version, user_id, channel_id, None).map_err(|e| e.to_string())?;
        Ok(Self {
            session: Arc::new(Mutex::new(session)),
            e2ee: Arc::new(AtomicBool::new(false)),
            user_id,
            channel_id,
            users: HashSet::from([user_id]),
            pending: HashMap::new(),
            ws,
        })
    }

    pub fn session(&self) -> Arc<Mutex<DaveSession>> {
        self.session.clone()
    }

    pub fn e2ee(&self) -> Arc<AtomicBool> {
        self.e2ee.clone()
    }

    pub fn set_protocol_version(&self, version: u16) {
        self.e2ee.store(version > 0, Ordering::Relaxed);
    }

    pub fn users_joined<'a>(&mut self, ids: impl Iterator<Item = &'a str>) {
        self.users.extend(ids.filter_map(|id| id.parse::<u64>().ok()));
    }

    pub fn user_left(&mut self, id: &str) {
        if let Ok(id) = id.parse() {
            self.users.remove(&id);
        }
    }

    pub fn handle_json(&mut self, op: u64, d: &Value) {
        let transition_id = d["transition_id"].as_u64().unwrap_or(0) as u16;
        let version = d["protocol_version"].as_u64().unwrap_or(0) as u16;
        match op {
            PREPARE_TRANSITION => {
                if self.with_session(|s| s.prepare_transition(transition_id, version)).is_some() {
                    self.ready_for(transition_id, version);
                }
            }
            EXECUTE_TRANSITION => {
                if self.with_session(|s| s.execute_transition(transition_id)) == Some(true)
                    && let Some(version) = self.pending.remove(&transition_id)
                {
                    self.set_protocol_version(version);
                }
            }
            PREPARE_EPOCH if d["epoch"].as_u64() == Some(1) => self.restart(version),
            _ => {}
        }
    }

    pub fn handle_binary(&mut self, op: u8, body: &[u8]) {
        match op {
            EXTERNAL_SENDER => {
                if self.with_session(|s| s.set_external_sender(body)).is_some() {
                    self.send_key_package();
                }
            }
            PROPOSALS => {
                let Some((&kind, proposals)) = body.split_first() else { return };
                let kind = if kind == 0 { ProposalsOperationType::APPEND } else { ProposalsOperationType::REVOKE };
                let users: Vec<u64> = self.users.iter().copied().collect();
                if let Some(Some(cw)) = self.with_session(|s| s.process_proposals(kind, proposals, Some(&users))) {
                    let welcome = cw.welcome.unwrap_or_default();
                    let _ = self.ws.unbounded_send(binary(COMMIT_WELCOME, &[&cw.commit, &welcome]));
                }
            }
            ANNOUNCE_COMMIT => {
                let Some((transition_id, commit)) = split_transition(body) else { return };
                match self.with_session(|s| s.process_commit(commit)) {
                    Some(true) => self.joined_epoch(transition_id),
                    Some(false) => {}
                    None => self.reject(transition_id),
                }
            }
            WELCOME => {
                let Some((transition_id, welcome)) = split_transition(body) else { return };
                let users: Vec<u64> = self.users.iter().copied().collect();
                match self.with_session(|s| s.process_welcome(welcome, Some(&users))) {
                    Some(()) => self.joined_epoch(transition_id),
                    None => self.reject(transition_id),
                }
            }
            _ => {}
        }
    }

    fn joined_epoch(&mut self, transition_id: u16) {
        let version = davey::DAVE_PROTOCOL_VERSION;
        if self.with_session(|s| s.prepare_transition(transition_id, version)).is_some() {
            self.ready_for(transition_id, version);
        }
    }

    // Transition 0 applies immediately; any other waits for EXECUTE_TRANSITION.
    fn ready_for(&mut self, transition_id: u16, version: u16) {
        if transition_id == 0 {
            self.set_protocol_version(version);
            return;
        }
        self.pending.insert(transition_id, version);
        self.send_json(TRANSITION_READY, json!({ "transition_id": transition_id }));
    }

    fn reject(&mut self, transition_id: u16) {
        self.send_json(INVALID_COMMIT_WELCOME, json!({ "transition_id": transition_id }));
        self.restart(davey::DAVE_PROTOCOL_VERSION);
    }

    fn restart(&mut self, version: u16) {
        let Some(version) = NonZeroU16::new(version) else { return };
        let (user, channel) = (self.user_id, self.channel_id);
        if self.with_session(|s| s.reinit(version, user, channel, None)).is_some() {
            self.send_key_package();
        }
    }

    fn send_key_package(&mut self) {
        if let Some(package) = self.with_session(|s| s.create_key_package()) {
            let _ = self.ws.unbounded_send(binary(KEY_PACKAGE, &[&package]));
        }
    }

    fn send_json(&self, op: u64, d: Value) {
        let _ = self.ws.unbounded_send(WsMessage::Text(json!({ "op": op, "d": d }).to_string()));
    }

    // tls_codec debug_asserts on malformed lengths, so a bad server frame panics in debug builds
    // instead of returning Err; contain it here rather than lose the whole voice task.
    fn with_session<T, E: std::fmt::Display>(&self, f: impl FnOnce(&mut DaveSession) -> Result<T, E>) -> Option<T> {
        let mut session = self.session.lock().ok()?;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&mut session)));
        result.ok()?.map_err(|e| eprintln!("voice: DAVE error: {e}")).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_framing() {
        assert_eq!(parse_binary(&[0x01, 0x02, 29, 0, 7, 0xaa]), Some((0x0102, 29, &[0, 7, 0xaa][..])));
        assert_eq!(parse_binary(&[0, 1]), None);
        assert_eq!(split_transition(&[0, 7, 0xaa]), Some((7, &[0xaa][..])));
        let WsMessage::Binary(frame) = binary(COMMIT_WELCOME, &[&[1, 2], &[3]]) else { panic!() };
        assert_eq!(frame, [28, 1, 2, 3]);
    }

    #[test]
    fn bad_external_sender_is_rejected_and_key_package_is_framed() {
        let (tx, mut rx) = mpsc::unbounded();
        let mut dave = Dave::new(1, 2, tx).unwrap();
        dave.handle_binary(EXTERNAL_SENDER, &[0x05]);
        assert!(rx.try_recv().is_err(), "a malformed external sender must not produce a key package");
        dave.send_key_package();
        let Ok(WsMessage::Binary(frame)) = rx.try_recv() else { panic!("key package not sent") };
        assert_eq!(frame[0], KEY_PACKAGE);
        assert!(frame.len() > 100);
    }
}
