// Every permission Discord shows in a role's editor, in its order and groups, with its names.
// Bits and wording from discord-api-docs topics/permissions (read 2026-11-18).
// GAP: channel_settings.rs (PR #18) keeps its own channel-only subset; fold it into this table on rebase.

pub struct Permission {
    pub bit: u64,
    pub name: &'static str,
    pub detail: &'static str,
}

pub struct PermissionGroup {
    pub title: &'static str,
    pub items: &'static [Permission],
}

const fn p(shift: u32, name: &'static str, detail: &'static str) -> Permission {
    Permission { bit: 1 << shift, name, detail }
}

pub const ADMINISTRATOR: u64 = 1 << 3;
pub const MANAGE_GUILD: u64 = 1 << 5;
pub const VIEW_AUDIT_LOG: u64 = 1 << 7;
pub const VIEW_GUILD_INSIGHTS: u64 = 1 << 19;
pub const MANAGE_WEBHOOKS: u64 = 1 << 29;
pub const MANAGE_GUILD_EXPRESSIONS: u64 = 1 << 30;
pub const CREATE_GUILD_EXPRESSIONS: u64 = 1 << 43;

pub const GROUPS: &[PermissionGroup] = &[
    PermissionGroup {
        title: "General server permissions",
        items: &[
            p(10, "View channels", "Lets members see channels by default, except private ones."),
            p(4, "Manage channels", "Lets members create, edit and delete channels."),
            p(28, "Manage roles", "Lets members create roles and edit or delete roles below their highest role."),
            p(43, "Create expressions", "Lets members add emoji, stickers and sounds."),
            p(30, "Manage expressions", "Lets members edit or remove emoji, stickers and sounds."),
            p(7, "View audit log", "Lets members see who changed what in this server."),
            p(19, "View server insights", "Lets members see the server's analytics."),
            p(29, "Manage webhooks", "Lets members create, edit and delete webhooks."),
            p(5, "Manage server", "Lets members change the server's name, region, invites and settings."),
        ],
    },
    PermissionGroup {
        title: "Membership permissions",
        items: &[
            p(0, "Create invite", "Lets members invite new people."),
            p(26, "Change nickname", "Lets members change their own nickname."),
            p(27, "Manage nicknames", "Lets members change other members' nicknames."),
            p(1, "Kick, approve and reject members", "Lets members remove others from this server."),
            p(2, "Ban members", "Lets members permanently ban others and delete their message history."),
            p(40, "Timeout members", "Lets members stop others from chatting, reacting or speaking for a while."),
        ],
    },
    PermissionGroup {
        title: "Text channel permissions",
        items: &[
            p(11, "Send messages and create posts", "Lets members send messages in text channels and create forum posts."),
            p(38, "Send messages in threads and posts", "Lets members reply in threads and posts."),
            p(35, "Create public threads", "Lets members start threads everyone in the channel can see."),
            p(36, "Create private threads", "Lets members start invite-only threads."),
            p(14, "Embed links", "Shows embedded content for links members share."),
            p(15, "Attach files", "Lets members upload files and media."),
            p(6, "Add reactions", "Lets members add new reactions to messages."),
            p(18, "Use external emoji", "Lets members use emoji from other servers."),
            p(37, "Use external stickers", "Lets members use stickers from other servers."),
            p(17, "Mention @everyone, @here and all roles", "Lets members notify everyone, or any role, at once."),
            p(13, "Manage messages", "Lets members delete others' messages."),
            p(51, "Pin messages", "Lets members pin and unpin messages."),
            p(52, "Bypass slowmode", "Lets members post without waiting out slowmode."),
            p(34, "Manage threads and posts", "Lets members rename, lock, close and delete threads and posts."),
            p(16, "Read message history", "Lets members read messages sent before they arrived."),
            p(12, "Send text-to-speech messages", "Lets members send /tts messages."),
            p(46, "Send voice messages", "Lets members send voice messages."),
            p(49, "Create polls", "Lets members create polls."),
        ],
    },
    PermissionGroup {
        title: "Voice channel permissions",
        items: &[
            p(20, "Connect", "Lets members join voice channels."),
            p(21, "Speak", "Lets members talk in voice channels."),
            p(9, "Video", "Lets members share their camera, screen or a game."),
            p(42, "Use soundboard", "Lets members play sounds in voice channels."),
            p(45, "Use external sounds", "Lets members play sounds from other servers."),
            p(25, "Use voice activity", "Lets members talk without push to talk."),
            p(8, "Priority speaker", "Makes members easier to hear by lowering others' volume while they speak."),
            p(22, "Mute members", "Lets members mute others in voice channels."),
            p(23, "Deafen members", "Lets members deafen others in voice channels."),
            p(24, "Move members", "Lets members move or disconnect others between voice channels."),
            p(48, "Set voice channel status", "Lets members set a voice channel's status."),
        ],
    },
    PermissionGroup { title: "Apps permissions", items: &[p(31, "Use application commands", "Lets members use commands from apps."), p(39, "Use activities", "Lets members start activities."), p(50, "Use external apps", "Lets members' own apps post in this server.")] },
    PermissionGroup {
        title: "Stage channel permissions",
        items: &[p(32, "Request to speak", "Lets members ask to speak in stage channels.")],
    },
    PermissionGroup { title: "Events permissions", items: &[p(44, "Create events", "Lets members create events."), p(33, "Manage events", "Lets members edit and cancel any event.")] },
    PermissionGroup { title: "Advanced permissions", items: &[p(3, "Administrator", "Grants every permission and skips channel overrides. Dangerous.")] },
];

pub fn all() -> impl Iterator<Item = &'static Permission> {
    GROUPS.iter().flat_map(|g| g.items)
}

pub fn name(bit: u64) -> Option<&'static str> {
    all().find(|p| p.bit == bit).map(|p| p.name)
}

// Names of the bits in `bits`, in the editor's order.
pub fn names(bits: u64) -> Vec<&'static str> {
    all().filter(|p| bits & p.bit != 0).map(|p| p.name).collect()
}

// The permission search in a role's editor: name or description, any case. Empty groups drop out.
pub fn search(query: &str) -> Vec<(&'static str, Vec<&'static Permission>)> {
    let query = query.trim().to_lowercase();
    GROUPS
        .iter()
        .map(|g| (g.title, g.items.iter().filter(|p| query.is_empty() || p.name.to_lowercase().contains(&query) || p.detail.to_lowercase().contains(&query)).collect::<Vec<_>>()))
        .filter(|(_, items)| !items.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_bit_listed_once() {
        let mut seen = 0u64;
        for p in all() {
            assert_eq!(seen & p.bit, 0, "{} listed twice", p.name);
            seen |= p.bit;
        }
        assert_eq!(seen.count_ones(), all().count() as u32);
    }

    #[test]
    fn search_matches_names_and_descriptions() {
        let hits = search("ban");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].1[0].name, "Ban members");
        assert!(search("slowmode").iter().any(|(_, ps)| ps.iter().any(|p| p.name == "Bypass slowmode")));
        assert!(search("zzz").is_empty());
        assert_eq!(search("").len(), GROUPS.len());
    }
}
