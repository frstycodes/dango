// `:` emoji suggestions in the composer: `:` plus two characters lists up to ten matches, your most used
// first. Separators don't count, so `:thisis` finds `:this_is_the_way:` (Discord's substring match doesn't).
use crate::{Waffle, discord::Emoji};

const MAX: usize = 10;
// Characters after the `:` before suggestions start, as Discord.
const MIN_QUERY: usize = 2;

/// One emoji offered for a query: its name as typed (`:name:` without colons) and, for a server's own,
/// that server.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct EmojiMatch {
    pub emoji: Emoji,
    pub name: String,
    pub server: Option<String>,
}

/// The query in `token` (`:smi` → `smi`) when it should list emoji: long enough, and not already a
/// finished `:name:`.
pub(crate) fn emoji_query(token: &str) -> Option<&str> {
    let query = token.strip_prefix(':')?;
    (query.chars().count() >= MIN_QUERY && !query.contains(':') && !query.contains(char::is_whitespace)).then_some(query)
}

fn squash(s: &str) -> String {
    s.chars().filter(|c| !matches!(c, '_' | '-' | ' ')).flat_map(char::to_lowercase).collect()
}

/// The best `MAX` of `candidates` for `query`: those whose name holds it, separators ignored, ranked by how
/// often you've used them, then names that start with it, then shorter names.
pub(crate) fn emoji_matches(query: &str, candidates: impl IntoIterator<Item = EmojiMatch>, uses: impl Fn(&Emoji) -> u32) -> Vec<EmojiMatch> {
    let query = squash(query);
    let mut found: Vec<(u32, bool, usize, EmojiMatch)> = candidates
        .into_iter()
        .filter_map(|m| {
            let name = squash(&m.name);
            let at = name.find(&query)?;
            Some((uses(&m.emoji), at == 0, name.len(), m))
        })
        .collect();
    found.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)).then(a.2.cmp(&b.2)));
    let mut seen = std::collections::HashSet::new();
    found.into_iter().map(|(.., m)| m).filter(|m| seen.insert((m.emoji.id.clone(), m.emoji.name.clone()))).take(MAX).collect()
}

impl Waffle {
    // This server's emoji (every server's with Nitro, as Discord allows), then Unicode by each shortcode.
    pub(crate) fn emoji_suggestions(&self, query: &str) -> Vec<EmojiMatch> {
        let here = self.guild_id.as_deref();
        let servers = self.guilds.iter().filter(|g| Some(g.id.as_str()) == here || self.my_account.premium).flat_map(|g| {
            g.emojis.iter().map(move |e| EmojiMatch { emoji: e.clone(), name: e.name.clone(), server: Some(g.name.clone()) })
        });
        let unicode = emojis::iter().flat_map(|e| e.shortcodes().map(move |code| EmojiMatch { emoji: Emoji::unicode(e.as_str()), name: code.to_owned(), server: None }));
        let uses = &self.prefs.emoji_uses;
        emoji_matches(query, servers.chain(unicode), |e| uses.iter().find(|(u, _)| u == e).map_or(0, |(_, n)| *n))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(name: &str, glyph: &str) -> EmojiMatch {
        EmojiMatch { emoji: Emoji::unicode(glyph), name: name.into(), server: None }
    }

    #[test]
    fn query_needs_two_characters_and_an_open_code() {
        assert_eq!(emoji_query(":sm"), Some("sm"));
        assert_eq!(emoji_query(":s"), None);
        assert_eq!(emoji_query(":smile:"), None);
        assert_eq!(emoji_query("smile"), None);
    }

    #[test]
    fn matches_ignore_separators_and_rank_use_then_prefix_then_length() {
        let all = [named("this_is_the_way", "a"), named("smile", "b"), named("smiley_cat", "c"), named("grinning_smile", "d"), named("smirk", "e")];
        let names = |q: &str, used: &str| emoji_matches(q, all.clone(), |e| u32::from(e.name == used)).into_iter().map(|m| m.name).collect::<Vec<_>>();
        assert_eq!(names("thisis", ""), ["this_is_the_way"]);
        assert_eq!(names("smil", ""), ["smile", "smiley_cat", "grinning_smile"]);
        assert_eq!(names("smil", "d"), ["grinning_smile", "smile", "smiley_cat"]);
    }

    #[test]
    fn one_row_per_emoji_however_many_shortcodes_match() {
        let all = [named("grin", "x"), named("grinning", "x")];
        assert_eq!(emoji_matches("grin", all, |_| 0).len(), 1);
    }
}
