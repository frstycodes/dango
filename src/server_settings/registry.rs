// Server settings in the shared settings registry, so the window's search reaches every page.
// Each row draws itself (Control::Custom); `id` is the settings_row id the jump lights.
use super::access::Page;
use crate::settings::registry::{Control, Scope, Setting};

impl Scope for Page {
    fn label(self) -> &'static str {
        Page::label(self, true)
    }
}

const fn s(id: &'static str, page: Page, group: &'static str, title: &'static str, keywords: &'static [&'static str]) -> Setting<Page> {
    Setting { id, page, group, title, caption: None, keywords, control: Control::Custom, row: None }
}

pub static SERVER_SETTINGS: &[Setting<Page>] = &[
    s("ss-icon", Page::Profile, "Icon and banner", "Server icon", &["picture", "logo", "avatar", "image"]),
    s("ss-banner", Page::Profile, "Icon and banner", "Banner", &["header", "cover", "image"]),
    s("ss-name", Page::Profile, "Name", "Server name", &["rename", "title"]),
    s("ss-description", Page::Profile, "Name", "Description", &["about", "bio"]),
    s("server-tag", Page::Tag, "Server tag", "Server tag", &["badge", "clan", "guild tag"]),
    s("ss-system-channel", Page::Engagement, "System messages", "System messages channel", &["welcome", "join", "boost message"]),
    s("ss-flag-join", Page::Engagement, "System messages", "Welcome people when they join", &["greeting", "join message"]),
    s("ss-flag-sticker", Page::Engagement, "System messages", "Suggest a sticker reply to welcomes", &["wave"]),
    s("ss-flag-boost", Page::Engagement, "System messages", "Thank people who boost", &["boost message"]),
    s("ss-flag-tips", Page::Engagement, "System messages", "Send setup tips", &["reminder", "tips"]),
    s("ss-notify-0", Page::Engagement, "Default notifications", "Default notifications", &["mentions", "all messages", "notification"]),
    s("ss-afk-channel", Page::Engagement, "Inactive voice", "Inactive channel", &["afk", "idle"]),
    s("ss-afk-timeout", Page::Engagement, "Inactive voice", "Move after", &["afk timeout", "inactive timeout", "idle"]),
    s("ss-widget", Page::Engagement, "Server widget", "Server widget", &["embed", "website", "html"]),
    s("ss-boost-level", Page::BoostPerks, "Progress", "Boost level", &["nitro", "premium", "tier"]),
    s("ss-progress-bar", Page::BoostPerks, "Progress", "Boost progress bar", &["nitro", "premium"]),
    s("ss-splash", Page::BoostPerks, "Invite background", "Invite background", &["splash", "invite image"]),
    s("ss-vanity", Page::BoostPerks, "Custom invite link", "Custom invite link", &["vanity", "url", "discord.gg"]),
    s("ss-emoji", Page::Emoji, "Emoji", "Upload emoji", &["emote", "custom emoji", "expression"]),
    s("ss-stickers", Page::Stickers, "Stickers", "Upload sticker", &["expression"]),
    s("ss-sounds", Page::Soundboard, "Sounds", "Upload sound", &["soundboard", "audio", "expression"]),
    s("ss-member-search", Page::Members, "Find members", "Find members", &["kick", "ban", "timeout", "mute", "user"]),
    s("ss-prune", Page::Members, "Prune", "Prune inactive members", &["kick inactive", "cleanup"]),
    s("ss-roles", Page::Roles, "Roles", "Roles", &["permissions", "rank", "colour", "color", "create role"]),
    s("ss-invites", Page::Invites, "Invite links", "Invite links", &["revoke", "link"]),
    s("ss-pause-invites", Page::Invites, "Pause", "Pause invites", &["raid", "disable invites", "lockdown"]),
    s("access", Page::Access, "Access", "Rules screening", &["rules", "verification", "age restricted", "membership"]),
    s("ss-integrations", Page::Integrations, "Bots and apps", "Bots and apps", &["bot", "integration", "app"]),
    s("ss-webhooks", Page::Integrations, "Webhooks", "Webhooks", &["webhook", "hook", "url"]),
    s("ss-verify-0", Page::Safety, "Who can chat", "Verification level", &["raid", "phone", "email", "security"]),
    s("ss-filter-0", Page::Safety, "Sensitive images", "Explicit image filter", &["nsfw", "media", "content filter"]),
    s("ss-audit", Page::AuditLog, "Audit log", "Audit log", &["history", "log", "who changed", "actions"]),
    s("ss-bans", Page::Bans, "Bans", "Bans", &["unban", "banned", "ban list"]),
    s("ss-automod", Page::AutoMod, "Rules", "AutoMod", &["filter", "spam", "words", "mention spam", "moderation", "keyword"]),
    s("ss-community", Page::Community, "Community", "Community", &["discovery", "rules channel", "public"]),
    s("onboarding", Page::Onboarding, "Onboarding", "Onboarding", &["welcome screen", "questions", "server guide"]),
    s("insights", Page::Insights, "Insights", "Server insights", &["analytics", "stats"]),
    s("ss-template", Page::Template, "Template", "Server template", &["copy server", "clone"]),
    s("ss-delete-name", Page::Delete, "Delete server", "Delete server", &["remove server", "destroy"]),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::registry::search;

    #[test]
    fn ids_unique_and_search_finds_server_words() {
        let mut ids: Vec<&str> = SERVER_SETTINGS.iter().map(|s| s.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), SERVER_SETTINGS.len());
        let first = |q: &str| search(SERVER_SETTINGS, q).first().map(|s| s.id);
        assert_eq!(first("afk"), Some("ss-afk-channel"));
        assert_eq!(first("unban"), Some("ss-bans"));
        assert_eq!(first("vanity"), Some("ss-vanity"));
        assert_eq!(first("audit log"), Some("ss-audit"));
    }
}
