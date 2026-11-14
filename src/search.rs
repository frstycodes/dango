// Discord's search syntax: `from:` `mentions:` `has:` `in:` `before:` `after:` `during:` `pinned:`
// become the search endpoint's filters; whatever is left is the text to find.
use chrono::{NaiveDate, TimeZone, Utc};

// Discord's epoch (2015-01-01) in milliseconds; a message id is (ms since it) << 22.
const DISCORD_EPOCH: i64 = 1_420_070_400_000;
pub const HAS: [&str; 8] = ["link", "embed", "file", "image", "video", "sound", "sticker", "poll"];

#[derive(Debug, Default, PartialEq)]
pub struct Query {
    pub content: String,
    // (parameter, value) pairs, in Discord's names, ready for the URL.
    pub params: Vec<(String, String)>,
    // Filters we couldn't resolve, e.g. an unknown user, to tell the reader about.
    pub unresolved: Vec<String>,
}

// `user` and `channel` resolve a name (without @ or #) to an id.
pub fn parse(input: &str, user: impl Fn(&str) -> Option<String>, channel: impl Fn(&str) -> Option<String>) -> Query {
    let mut query = Query::default();
    let mut words: Vec<&str> = Vec::new();
    for token in input.split_whitespace() {
        let Some((key, value)) = token.split_once(':').filter(|(_, v)| !v.is_empty()) else {
            words.push(token);
            continue;
        };
        let value = value.trim_start_matches(['@', '#']);
        let param = match key.to_ascii_lowercase().as_str() {
            "from" => user(value).map(|id| ("author_id", id)),
            "mentions" => user(value).map(|id| ("mentions", id)),
            "in" => channel(value).map(|id| ("channel_id", id)),
            "has" if HAS.contains(&value) => Some(("has", value.to_owned())),
            "pinned" if value == "true" || value == "false" => Some(("pinned", value.to_owned())),
            "before" => day(value).map(|d| ("max_id", snowflake(d))),
            "after" => day(value).map(|d| ("min_id", snowflake(d + chrono::Duration::days(1)))),
            "during" => {
                if let Some(d) = day(value) {
                    query.params.push(("min_id".into(), snowflake(d)));
                    query.params.push(("max_id".into(), snowflake(d + chrono::Duration::days(1))));
                    continue;
                }
                None
            }
            _ => {
                words.push(token);
                continue;
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

fn day(text: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(text, "%Y-%m-%d").ok()
}

// The first possible message id on a UTC day.
fn snowflake(day: NaiveDate) -> String {
    let ms = Utc.from_utc_datetime(&day.and_hms_opt(0, 0, 0).expect("midnight")).timestamp_millis();
    (((ms - DISCORD_EPOCH).max(0) as u64) << 22).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_become_parameters() {
        let user = |n: &str| (n == "kiwi").then(|| "42".to_owned());
        let channel = |n: &str| (n == "general").then(|| "7".to_owned());
        let q = parse("rust from:@kiwi in:#general has:image pinned:true gpui", user, channel);
        assert_eq!(q.content, "rust gpui");
        assert_eq!(q.params, [("author_id".into(), "42".into()), ("channel_id".into(), "7".into()), ("has".into(), "image".into()), ("pinned".into(), "true".into())]);
        let q = parse("from:nobody has:spaceship", user, channel);
        assert_eq!(q.unresolved, ["from:nobody"]);
        assert_eq!(q.content, "has:spaceship");
        // 2015-01-02 is one day after Discord's epoch.
        let q = parse("before:2015-01-02", user, channel);
        assert_eq!(q.params, [("max_id".into(), (86_400_000u64 << 22).to_string())]);
        assert_eq!(parse("during:2015-01-02", user, channel).params.len(), 2);
    }
}
