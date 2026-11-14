use std::time::Duration;

use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use futures::{SinkExt, StreamExt, channel::mpsc};
use rsa::{Oaep, RsaPrivateKey, pkcs8::EncodePublicKey};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio_tungstenite::tungstenite::{Message as WsMessage, client::IntoClientRequest, http::HeaderValue};

use crate::discord::{self, User};

// Discord's QR sign-in ("remote auth"). Undocumented; the op names below are what the official
// clients use as of v2 of the protocol. Not verified against a live server in this repo yet.
const REMOTE_AUTH: &str = "wss://remote-auth-gateway.discord.gg/?v=2";
const KEYRING_SERVICE: &str = "rustcord";
// Before the rename the app was "Chord"; its Keychain entry and dev-token file are read once and moved.
const OLD_KEYRING_SERVICE: &str = "chord";
const KEYRING_USER: &str = "discord-token";

pub enum AuthEvent {
    Qr(String),
    Scanned(User),
    SignedIn(String),
    Failed(String),
}

// Release builds keep the token in the Keychain. Debug builds use a file readable only by the user
// (0600): every rebuild changes the binary's signature, so the Keychain would ask for the password
// on each run. Testing convenience only: the token sits on disk in plain text.
#[cfg(not(debug_assertions))]
pub fn stored_token() -> Option<String> {
    if let Some(token) = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER).ok().and_then(|e| e.get_password().ok()) {
        return Some(token);
    }
    let token = keyring::Entry::new(OLD_KEYRING_SERVICE, KEYRING_USER).ok()?.get_password().ok()?;
    store_token(&token);
    Some(token)
}

#[cfg(not(debug_assertions))]
pub fn store_token(token: &str) {
    if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER) {
        let _ = entry.set_password(token);
    }
}

#[cfg(not(debug_assertions))]
pub fn forget_token() {
    if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER) {
        let _ = entry.delete_credential();
    }
}

#[cfg(debug_assertions)]
fn token_file() -> Option<std::path::PathBuf> {
    Some(crate::data_dir()?.join("dev-token"))
}

#[cfg(debug_assertions)]
pub fn stored_token() -> Option<String> {
    let from_file = token_file().and_then(|p| std::fs::read_to_string(p).ok()).map(|t| t.trim().to_owned()).filter(|t| !t.is_empty());
    if from_file.is_some() {
        return from_file;
    }
    let home = std::env::var_os("HOME")?;
    let old_file = std::path::Path::new(&home).join("Library/Application Support/Chord/dev-token");
    if let Some(token) = std::fs::read_to_string(&old_file).ok().map(|t| t.trim().to_owned()).filter(|t| !t.is_empty()) {
        store_token(&token);
        let _ = std::fs::remove_file(old_file);
        return Some(token);
    }
    // One-time move from the Keychain (the last password prompt), so a release sign-in carries over.
    let token = [KEYRING_SERVICE, OLD_KEYRING_SERVICE].into_iter().find_map(|service| keyring::Entry::new(service, KEYRING_USER).ok()?.get_password().ok())?;
    store_token(&token);
    Some(token)
}

#[cfg(debug_assertions)]
pub fn store_token(token: &str) {
    use std::{io::Write, os::unix::fs::OpenOptionsExt};
    let Some(path) = token_file() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let file = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&path);
    if let Ok(mut file) = file {
        let _ = file.write_all(token.as_bytes());
    }
}

#[cfg(debug_assertions)]
pub fn forget_token() {
    if let Some(path) = token_file() {
        let _ = std::fs::remove_file(path);
    }
}

// Keeps offering fresh QR codes (each expires after ~2 minutes, or when the phone cancels)
// until one is approved. Dropping the receiver stops it.
pub fn sign_in() -> mpsc::UnboundedReceiver<AuthEvent> {
    let (tx, rx) = mpsc::unbounded();
    discord::runtime().spawn(async move {
        loop {
            let event = match qr_session(&tx).await {
                Ok(token) => AuthEvent::SignedIn(token),
                Err(Outcome::Expired) if !tx.is_closed() => continue,
                Err(Outcome::Expired) => return,
                Err(Outcome::Failed(error)) => AuthEvent::Failed(error),
            };
            let _ = tx.unbounded_send(event);
            return;
        }
    });
    rx
}

enum Outcome {
    Expired,
    Failed(String),
}

async fn qr_session(tx: &mpsc::UnboundedSender<AuthEvent>) -> Result<String, Outcome> {
    let failed = |e: &dyn std::fmt::Display| Outcome::Failed(e.to_string());
    let key = tokio::task::spawn_blocking(|| RsaPrivateKey::new(&mut rand::thread_rng(), 2048))
        .await
        .map_err(|e| failed(&e))?
        .map_err(|e| failed(&e))?;
    let public_key = key.to_public_key().to_public_key_der().map_err(|e| failed(&e))?;

    let mut request = REMOTE_AUTH.into_client_request().map_err(|e| failed(&e))?;
    // The gateway refuses connections without Discord's own origin.
    request.headers_mut().insert("Origin", HeaderValue::from_static("https://discord.com"));
    let (ws, _) = tokio_tungstenite::connect_async(request).await.map_err(|e| failed(&e))?;
    let (mut sink, mut stream) = ws.split();
    let mut heartbeat = tokio::time::interval(Duration::from_secs(3600));
    heartbeat.tick().await;
    let send = |payload: Value| WsMessage::Text(payload.to_string());

    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                sink.send(send(json!({ "op": "heartbeat" }))).await.map_err(|e| failed(&e))?;
            }
            frame = stream.next() => {
                let text = match frame {
                    Some(Ok(WsMessage::Text(text))) => text,
                    Some(Ok(WsMessage::Close(_))) | None => return Err(Outcome::Expired),
                    Some(Ok(_)) => continue,
                    Some(Err(e)) => return Err(failed(&e)),
                };
                let msg: Value = serde_json::from_str(&text).map_err(|e| failed(&e))?;
                match msg["op"].as_str().unwrap_or_default() {
                    "hello" => {
                        let ms = msg["heartbeat_interval"].as_u64().unwrap_or(41_250);
                        heartbeat = tokio::time::interval(Duration::from_millis(ms));
                        heartbeat.tick().await;
                        let init = json!({ "op": "init", "encoded_public_key": STANDARD.encode(public_key.as_bytes()) });
                        sink.send(send(init)).await.map_err(|e| failed(&e))?;
                    }
                    "nonce_proof" => {
                        let nonce = decrypt(&key, &msg["encrypted_nonce"])?;
                        let proof = URL_SAFE_NO_PAD.encode(Sha256::digest(&nonce));
                        sink.send(send(json!({ "op": "nonce_proof", "proof": proof }))).await.map_err(|e| failed(&e))?;
                    }
                    "pending_remote_init" => {
                        let fingerprint = msg["fingerprint"].as_str().unwrap_or_default();
                        let _ = tx.unbounded_send(AuthEvent::Qr(format!("https://discord.com/ra/{fingerprint}")));
                    }
                    "pending_ticket" | "pending_finish" => {
                        let payload = String::from_utf8(decrypt(&key, &msg["encrypted_user_payload"])?).map_err(|e| failed(&e))?;
                        if let Some(user) = parse_user_payload(&payload) {
                            let _ = tx.unbounded_send(AuthEvent::Scanned(user));
                        }
                    }
                    "pending_login" => return exchange_ticket(&key, msg["ticket"].as_str().unwrap_or_default()).await,
                    // Protocol v1 sends the token directly.
                    "finish" => return String::from_utf8(decrypt(&key, &msg["encrypted_token"])?).map_err(|e| failed(&e)),
                    "cancel" => return Err(Outcome::Expired),
                    _ => {}
                }
            }
        }
    }
}

async fn exchange_ticket(key: &RsaPrivateKey, ticket: &str) -> Result<String, Outcome> {
    let req = reqwest::Client::new()
        .post(format!("{}/users/@me/remote-auth/login", discord::API))
        .json(&json!({ "ticket": ticket }));
    let body = discord::send(req).await.map_err(Outcome::Failed)?;
    let token = decrypt(key, &body["encrypted_token"])?;
    String::from_utf8(token).map_err(|e| Outcome::Failed(e.to_string()))
}

fn decrypt(key: &RsaPrivateKey, encoded: &Value) -> Result<Vec<u8>, Outcome> {
    let failed = |e: &dyn std::fmt::Display| Outcome::Failed(e.to_string());
    let bytes = STANDARD.decode(encoded.as_str().unwrap_or_default()).map_err(|e| failed(&e))?;
    key.decrypt(Oaep::new::<Sha256>(), &bytes).map_err(|e| failed(&e))
}

// "id:discriminator:avatar_hash:username"
fn parse_user_payload(payload: &str) -> Option<User> {
    let mut parts = payload.splitn(4, ':');
    let (id, _discriminator, avatar, name) = (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
    Some(User { id: id.to_owned(), name: name.to_owned(), avatar: discord::avatar_url(id, Some(avatar)), ..Default::default() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsa::RsaPublicKey;

    #[test]
    fn parses_scanned_user() {
        let user = parse_user_payload("9:0:abc:ferris").unwrap();
        assert_eq!(user.name, "ferris");
        assert!(user.avatar.ends_with("/avatars/9/abc.png?size=128"));
        assert!(parse_user_payload("9:0").is_none());
    }

    #[test]
    fn decrypts_oaep_sha256() {
        let key = RsaPrivateKey::new(&mut rand::thread_rng(), 1024).unwrap();
        let sealed = RsaPublicKey::from(&key).encrypt(&mut rand::thread_rng(), Oaep::new::<Sha256>(), b"nonce").unwrap();
        let plain = decrypt(&key, &Value::String(STANDARD.encode(sealed))).ok().unwrap();
        assert_eq!(plain, b"nonce");
    }
}
