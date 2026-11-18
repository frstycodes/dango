// Discord's search syntax: `from:` `mentions:` `has:` `in:` `before:` `after:` `during:` `pinned:` `authorType:`
// become the search endpoint's filters; whatever is left is the text to find. A filter picked from the
// popover sits in the field as an atomic token whose text is that same syntax with an id (`from:42`), so
// typed and picked filters parse alike.
use chrono::{NaiveDate, TimeZone, Utc};
use gpui_kit::component::input::{InlineToken, InputContent};
use serde::{Deserialize, Serialize};

// Discord's epoch (2015-01-01) in milliseconds; a message id is (ms since it) << 22.
const DISCORD_EPOCH: i64 = 1_420_070_400_000;
// What `has:` takes, as the popover words them. `forward` is the API's `snapshot`.
pub const HAS: [&str; 9] = ["link", "embed", "file", "image", "video", "sound", "sticker", "poll", "forward"];
pub const AUTHOR_TYPES: [&str; 3] = ["user", "bot", "webhook"];
const HISTORY_CAP: usize = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    From,
    In,
    Has,
    Mentions,
    Before,
    After,
    During,
    Pinned,
    AuthorType,
}

impl Key {
    pub const MAIN: [Key; 4] = [Key::From, Key::In, Key::Has, Key::Mentions];
    pub const MORE: [Key; 5] = [Key::Before, Key::After, Key::During, Key::Pinned, Key::AuthorType];

    pub fn word(self) -> &'static str {
        match self {
            Key::From => "from",
            Key::In => "in",
            Key::Has => "has",
            Key::Mentions => "mentions",
            Key::Before => "before",
            Key::After => "after",
            Key::During => "during",
            Key::Pinned => "pinned",
            Key::AuthorType => "authorType",
        }
    }

    pub fn from_word(word: &str) -> Option<Key> {
        Key::MAIN.into_iter().chain(Key::MORE).find(|k| k.word().eq_ignore_ascii_case(word))
    }

    pub fn title(self) -> &'static str {
        match self {
            Key::From => "From a specific user",
            Key::In => "Sent in a specific channel",
            Key::Has => "Includes a specific type of data",
            Key::Mentions => "Mentions a specific user",
            Key::Before => "Sent before a date",
            Key::After => "Sent after a date",
            Key::During => "Sent on a date",
            Key::Pinned => "Pinned or not",
            Key::AuthorType => "Sent by a user, bot or webhook",
        }
    }

    // What the filter takes, shown after its word.
    pub fn takes(self) -> &'static str {
        match self {
            Key::From | Key::Mentions => "user",
            Key::In => "channel",
            Key::Has => "link, embed or file",
            Key::Before | Key::After | Key::During => "date",
            Key::Pinned => "true or false",
            Key::AuthorType => "user, bot or webhook",
        }
    }

    pub fn is_date(self) -> bool {
        matches!(self, Key::Before | Key::After | Key::During)
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct Query {
    pub content: String,
    // (parameter, value) pairs, in Discord's names, ready for the URL. A name may repeat: Discord takes arrays.
    pub params: Vec<(String, String)>,
    // Filters we couldn't resolve, e.g. an unknown user, to tell the reader about.
    pub unresolved: Vec<String>,
}

// `user` and `channel` resolve a name (without @ or #) or an id to an id.
pub fn parse(input: &str, user: impl Fn(&str) -> Option<String>, channel: impl Fn(&str) -> Option<String>) -> Query {
    let mut query = Query::default();
    let mut words: Vec<&str> = Vec::new();
    for token in input.split_whitespace() {
        let Some((key, value)) = token.split_once(':').filter(|(_, v)| !v.is_empty()) else {
            words.push(token);
            continue;
        };
        let Some(key) = Key::from_word(key) else {
            words.push(token);
            continue;
        };
        let value = value.trim_start_matches(['@', '#']);
        let param = match key {
            Key::From => user(value).map(|id| ("author_id", id)),
            Key::Mentions => user(value).map(|id| ("mentions", id)),
            Key::In => channel(value).map(|id| ("channel_id", id)),
            Key::Has => HAS.contains(&value).then(|| ("has", if value == "forward" { "snapshot".to_owned() } else { value.to_owned() })),
            Key::Pinned => (value == "true" || value == "false").then(|| ("pinned", value.to_owned())),
            Key::AuthorType => AUTHOR_TYPES.contains(&value).then(|| ("author_type", value.to_owned())),
            Key::Before => day(value).map(|d| ("max_id", snowflake(d))),
            Key::After => day(value).map(|d| ("min_id", snowflake(d + chrono::Duration::days(1)))),
            Key::During => {
                if let Some(d) = day(value) {
                    query.params.push(("min_id".into(), snowflake(d)));
                    query.params.push(("max_id".into(), snowflake(d + chrono::Duration::days(1))));
                    continue;
                }
                None
            }
        };
        match param {
            Some((name, value)) => query.params.push((name.into(), value)),
            None => query.unresolved.push(token.to_owned()),
        }
    }
    query.content = words.join(" ");
    query
}

// The filter being typed at the end of `text`: its key, the value so far, and the byte where its word
// starts. Nothing while the caret sits right after a token (`token_end`), whose own text is `key:id`.
pub fn typing(text: &str, token_end: usize) -> Option<(Key, &str, usize)> {
    let start = last_word_start(text);
    if start < token_end {
        return None;
    }
    let (key, value) = text[start..].split_once(':')?;
    Some((Key::from_word(key)?, value.trim_start_matches(['@', '#']), start))
}

// The filters whose word starts with the half-typed last word, for "fr" → `from:`.
pub fn keys_matching(text: &str, token_end: usize) -> Vec<Key> {
    let start = last_word_start(text);
    let word = &text[start..];
    if start < token_end || word.is_empty() || word.contains(':') {
        return Vec::new();
    }
    Key::MAIN.into_iter().chain(Key::MORE).filter(|k| k.word().to_ascii_lowercase().starts_with(&word.to_ascii_lowercase())).collect()
}

pub fn last_word_start(text: &str) -> usize {
    text.char_indices().rev().find(|(_, c)| c.is_whitespace()).map_or(0, |(i, c)| i + c.len_utf8())
}

pub fn day_text(day: NaiveDate) -> String {
    day.format("%Y-%m-%d").to_string()
}

pub fn day_label(day: NaiveDate) -> String {
    day.format("%b %-d, %Y").to_string()
}

fn day(text: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(text, "%Y-%m-%d").ok()
}

// The first possible message id on a UTC day.
fn snowflake(day: NaiveDate) -> String {
    let ms = Utc.from_utc_datetime(&day.and_hms_opt(0, 0, 0).expect("midnight")).timestamp_millis();
    (((ms - DISCORD_EPOCH).max(0) as u64) << 22).to_string()
}

// A past search as the field held it: its text and each token's byte range and label.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Saved {
    pub text: String,
    pub tokens: Vec<SavedToken>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedToken {
    pub start: usize,
    pub end: usize,
    pub label: String,
}

impl Saved {
    pub fn from_content(content: &InputContent) -> Self {
        let tokens = content.tokens().iter().map(|s| SavedToken { start: s.range().start, end: s.range().end, label: s.token().label().to_string() }).collect();
        Self { text: content.text().trim_end().to_owned(), tokens }
    }

    // Tokens that no longer fit the text (a hand-edited prefs file) are dropped, keeping their text.
    pub fn to_content(&self) -> InputContent {
        self.tokens.iter().fold(InputContent::new(self.text.clone()), |content, t| {
            let Some(text) = self.text.get(t.start..t.end) else { return content };
            let token = InlineToken::new(text.to_owned(), text.to_owned()).with_label(t.label.clone());
            content.clone().with_token(t.start..t.end, token).unwrap_or(content)
        })
    }

    // The search as words and token labels, in order: `from:` Rosa `in:` #clips dodge.
    pub fn parts(&self) -> Vec<(Option<&str>, &str)> {
        let mut parts = Vec::new();
        let mut at = 0;
        for t in &self.tokens {
            let Some(text) = self.text.get(t.start..t.end) else { continue };
            push_words(&mut parts, self.text.get(at..t.start).unwrap_or(""));
            parts.push((text.split_once(':').map(|(k, _)| k), t.label.as_str()));
            at = t.end;
        }
        push_words(&mut parts, self.text.get(at..).unwrap_or(""));
        parts
    }
}

fn push_words<'a>(parts: &mut Vec<(Option<&'a str>, &'a str)>, text: &'a str) {
    let text = text.trim();
    if !text.is_empty() {
        parts.push((None, text));
    }
}

// Newest first, each search once, at most `HISTORY_CAP`.
pub fn remember(history: &mut Vec<Saved>, search: Saved) {
    history.retain(|s| s.text != search.text);
    history.insert(0, search);
    history.truncate(HISTORY_CAP);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(n: &str) -> Option<String> {
        (n == "kiwi" || n == "42").then(|| "42".to_owned())
    }

    fn channel(n: &str) -> Option<String> {
        (n == "general" || n == "7").then(|| "7".to_owned())
    }

    #[test]
    fn filters_become_parameters() {
        let q = parse("rust from:@kiwi in:#general has:image pinned:true gpui", user, channel);
        assert_eq!(q.content, "rust gpui");
        assert_eq!(q.params, [("author_id".into(), "42".into()), ("channel_id".into(), "7".into()), ("has".into(), "image".into()), ("pinned".into(), "true".into())]);
        let q = parse("from:nobody has:spaceship", user, channel);
        assert_eq!(q.unresolved, ["from:nobody", "has:spaceship"]);
        assert_eq!(q.content, "");
    }

    #[test]
    fn tokens_parse_like_typed_filters() {
        let q = parse("from:42 mentions:42 in:7 has:forward authorType:bot dodge", user, channel);
        assert_eq!(q.content, "dodge");
        let names: Vec<&str> = q.params.iter().map(|(n, v)| if n == "has" { v.as_str() } else { n.as_str() }).collect();
        assert_eq!(names, ["author_id", "mentions", "channel_id", "snapshot", "author_type"]);
        assert_eq!(parse("authortype:robot", user, channel).unresolved, ["authortype:robot"]);
    }

    #[test]
    fn dates_bound_message_ids() {
        // 2015-01-02 is one day after Discord's epoch.
        let day = (86_400_000u64 << 22).to_string();
        let two = (2 * 86_400_000u64 << 22).to_string();
        assert_eq!(parse("before:2015-01-02", user, channel).params, [("max_id".into(), day.clone())]);
        // After a day means from the start of the next one.
        assert_eq!(parse("after:2015-01-02", user, channel).params, [("min_id".into(), two.clone())]);
        assert_eq!(parse("during:2015-01-02", user, channel).params, [("min_id".into(), day), ("max_id".into(), two)]);
        assert_eq!(parse("before:yesterday", user, channel).unresolved, ["before:yesterday"]);
    }

    #[test]
    fn typing_finds_the_open_filter() {
        assert_eq!(typing("dodge from:ro", 0), Some((Key::From, "ro", 6)));
        assert_eq!(typing("in:#gen", 0), Some((Key::In, "gen", 0)));
        assert_eq!(typing("dodge ", 0), None);
        // Right after a token, its own `from:42` isn't a filter being typed.
        assert_eq!(typing("from:42", 7), None);
        assert_eq!(keys_matching("dodge fr", 0), [Key::From]);
        assert_eq!(keys_matching("dodge ", 0), []);
    }

    #[test]
    fn history_is_newest_first_once_and_capped() {
        let saved = |text: &str| Saved { text: text.into(), tokens: Vec::new() };
        let mut history = Vec::new();
        for i in 0..12 {
            remember(&mut history, saved(&format!("q{i}")));
        }
        assert_eq!(history.len(), HISTORY_CAP);
        assert_eq!(history[0].text, "q11");
        remember(&mut history, saved("q5"));
        assert_eq!(history.len(), HISTORY_CAP);
        assert_eq!((history[0].text.as_str(), history.iter().filter(|s| s.text == "q5").count()), ("q5", 1));
    }

    #[test]
    fn saved_searches_keep_their_tokens() {
        let token = InlineToken::new("from:42", "from:42").with_label("Rosa");
        let content = InputContent::new("from:42 dodge ").with_token(0..7, token).expect("token fits");
        let saved = Saved::from_content(&content);
        assert_eq!(saved.text, "from:42 dodge");
        assert_eq!(saved.parts(), [(Some("from"), "Rosa"), (None, "dodge")]);
        assert_eq!(saved.to_content().tokens().len(), 1);
        // A range that no longer fits the text drops the token, not the search.
        let stale = Saved { text: "dodge".into(), tokens: vec![SavedToken { start: 0, end: 40, label: "Rosa".into() }] };
        assert_eq!(stale.to_content().tokens().len(), 0);
    }
}
