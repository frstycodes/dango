// Discord's message cache: the newest messages of each channel kept on disk, so reopening a
// channel after a restart shows its messages at once while the fresh copy loads.
use crate::discord::{Message, SendState};

// Matches one history page, so a cached feed looks exactly like a fetched one.
const KEEP: usize = 50;

fn file(channel_id: &str) -> Option<std::path::PathBuf> {
    // Channel ids are snowflakes; anything else never names a file.
    channel_id.bytes().all(|b| b.is_ascii_digit()).then_some(())?;
    Some(crate::data_dir()?.join("cache").join(format!("{channel_id}.json")))
}

pub fn load(channel_id: &str) -> Option<Vec<Message>> {
    let text = std::fs::read_to_string(file(channel_id)?).ok()?;
    serde_json::from_str(&text).ok()
}

// Writes in the background; only messages Discord has accepted are kept.
pub fn save(channel_id: &str, messages: &[Message]) {
    let Some(path) = file(channel_id) else { return };
    let sent: Vec<&Message> = messages.iter().filter(|m| m.state == SendState::Sent).collect();
    let newest = &sent[sent.len().saturating_sub(KEEP)..];
    let Ok(text) = serde_json::to_string(newest) else { return };
    std::thread::spawn(move || {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, text);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_snowflakes_name_files() {
        assert!(file("../etc/passwd").is_none());
        assert!(file("123456789").is_some_and(|p| p.ends_with("cache/123456789.json")));
    }

    #[test]
    fn messages_round_trip() {
        let message = Message { id: "1".into(), content: "hi **there**".into(), ..Default::default() };
        let text = serde_json::to_string(&[&message]).unwrap();
        let back: Vec<Message> = serde_json::from_str(&text).unwrap();
        assert_eq!(back[0].content, "hi **there**");
    }
}
