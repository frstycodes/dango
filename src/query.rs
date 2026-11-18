// What we fetch over REST, cached the way Discord's stores behave: the last value shows at once (from
// memory, else from disk, so it survives a restart), a background fetch refreshes it when it's stale or
// known to have changed, and the fresh value replaces it in place. Once something has been seen, it never
// shows a loading state again.
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Serialize, de::DeserializeOwned};

// Entries held in memory; older ones fall back to their file on disk.
const IN_MEMORY: usize = 256;

pub struct Query<V> {
    dir: Option<PathBuf>,
    fresh_for: Duration,
    memory: HashMap<String, Entry<V>>,
    // Known to have changed (a gateway update): kept on screen, but fetched again on next use.
    changed: HashSet<String>,
    in_flight: HashSet<String>,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct Entry<V> {
    fetched_ms: u64,
    value: V,
}

impl<V: Clone + Serialize + DeserializeOwned + Send + 'static> Query<V> {
    // `name` is its folder under the app's cache directory.
    pub fn new(name: &str, fresh_for: Duration) -> Self {
        Self::at(crate::data_dir().map(|d| d.join("cache").join(name)), fresh_for)
    }

    fn at(dir: Option<PathBuf>, fresh_for: Duration) -> Self {
        Self { dir, fresh_for, memory: HashMap::new(), changed: HashSet::new(), in_flight: HashSet::new() }
    }

    // The last value we have for `key`, however old.
    pub fn cached(&mut self, key: &str) -> Option<V> {
        if let Some(entry) = self.memory.get(key) {
            return Some(entry.value.clone());
        }
        // GAP: a synchronous read of one small JSON file; move it off the UI thread if values grow.
        let entry: Entry<V> = serde_json::from_str(&std::fs::read_to_string(self.file(key)?).ok()?).ok()?;
        let value = entry.value.clone();
        self.remember(key, entry);
        Some(value)
    }

    // Demo fixtures: held in memory as fresh, never written to disk.
    pub fn seed(&mut self, key: &str, value: V) {
        self.remember(key, Entry { fetched_ms: now_ms(), value });
    }

    // What `cached` has already brought into memory, for callers that can't borrow mutably.
    pub fn peek(&self, key: &str) -> Option<V> {
        self.memory.get(key).map(|e| e.value.clone())
    }

    // Whether to fetch `key` now: never fetched, stale, or changed, and no fetch already running. A yes
    // counts as the fetch starting; finish it with `store` or `failed`.
    pub fn should_fetch(&mut self, key: &str) -> bool {
        if self.in_flight.contains(key) {
            return false;
        }
        let fresh = !self.changed.contains(key) && self.memory.get(key).is_some_and(|e| age(e.fetched_ms) < self.fresh_for);
        if fresh {
            return false;
        }
        self.in_flight.insert(key.to_owned());
        true
    }

    pub fn store(&mut self, key: &str, value: V) {
        self.in_flight.remove(key);
        self.changed.remove(key);
        let entry = Entry { fetched_ms: now_ms(), value };
        if let Some(path) = self.file(key)
            && let Ok(text) = serde_json::to_string(&entry)
        {
            std::thread::spawn(move || {
                if let Some(dir) = path.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                let _ = std::fs::write(path, text);
            });
        }
        self.remember(key, entry);
    }

    // The fetch failed; whatever was cached stays, and the next use tries again.
    pub fn failed(&mut self, key: &str) {
        self.in_flight.remove(key);
    }

    // Marks every key `matches` picks as changed, keeping their values until the refetch lands.
    pub fn changed(&mut self, matches: impl Fn(&str) -> bool) {
        let keys: Vec<String> = self.memory.keys().filter(|k| matches(k)).cloned().collect();
        self.changed.extend(keys);
    }

    fn remember(&mut self, key: &str, entry: Entry<V>) {
        if self.memory.len() >= IN_MEMORY
            && !self.memory.contains_key(key)
            && let Some(oldest) = self.memory.iter().min_by_key(|(_, e)| e.fetched_ms).map(|(k, _)| k.clone())
        {
            self.memory.remove(&oldest);
        }
        self.memory.insert(key.to_owned(), entry);
    }

    // Keys are ids joined by `-`; anything else never names a file.
    fn file(&self, key: &str) -> Option<PathBuf> {
        key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_').then_some(())?;
        Some(self.dir.as_ref()?.join(format!("{key}.json")))
    }
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

fn age(fetched_ms: u64) -> Duration {
    Duration::from_millis(now_ms().saturating_sub(fetched_ms))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wave-query-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn fetches_once_then_serves_the_cache() {
        let mut q: Query<String> = Query::at(None, Duration::from_secs(300));
        assert_eq!(q.cached("1"), None);
        assert!(q.should_fetch("1"));
        assert!(!q.should_fetch("1"), "a running fetch isn't started twice");
        q.store("1", "a".into());
        assert_eq!(q.cached("1").as_deref(), Some("a"));
        assert!(!q.should_fetch("1"), "fresh values aren't refetched");
    }

    #[test]
    fn stale_and_changed_values_show_while_they_refetch() {
        let mut q: Query<String> = Query::at(None, Duration::ZERO);
        q.store("1", "old".into());
        assert!(q.should_fetch("1"), "stale");
        q.failed("1");
        assert_eq!(q.cached("1").as_deref(), Some("old"), "a failed fetch keeps the old value");

        let mut q: Query<String> = Query::at(None, Duration::from_secs(300));
        q.store("1-2", "old".into());
        q.changed(|k| k.starts_with("1-"));
        assert_eq!(q.cached("1-2").as_deref(), Some("old"));
        assert!(q.should_fetch("1-2"), "changed");
    }

    #[test]
    fn values_outlive_the_session_on_disk() {
        let dir = temp("disk");
        let mut q: Query<String> = Query::at(Some(dir.clone()), Duration::from_secs(300));
        q.store("7-8", "kept".into());
        // The write is in the background.
        for _ in 0..100 {
            if dir.join("7-8.json").exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let mut again: Query<String> = Query::at(Some(dir.clone()), Duration::from_secs(300));
        assert_eq!(again.cached("7-8").as_deref(), Some("kept"));
        assert!(again.file("../x").is_none());
        let _ = std::fs::remove_dir_all(dir);
    }
}
