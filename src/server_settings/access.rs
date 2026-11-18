// Which server-settings pages and controls the signed-in member may use, as Discord decides it.
// Pages and their order from Discord web's server settings (read 2026-11-18). Which permission opens
// each page is from memory of Discord's client where the docs don't say: Not verified per page.
use crate::discord::perm;

use super::perms::{ADMINISTRATOR, CREATE_GUILD_EXPRESSIONS, MANAGE_GUILD, MANAGE_GUILD_EXPRESSIONS, MANAGE_WEBHOOKS, VIEW_AUDIT_LOG, VIEW_GUILD_INSIGHTS};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Page {
    Profile,
    Tag,
    Engagement,
    BoostPerks,
    Emoji,
    Stickers,
    Soundboard,
    Members,
    Roles,
    Invites,
    Access,
    Integrations,
    Safety,
    AuditLog,
    Bans,
    AutoMod,
    Community,
    Onboarding,
    Insights,
    Template,
    Delete,
}

pub const NAV: &[(Option<&str>, &[Page])] = &[
    (None, &[Page::Profile, Page::Tag, Page::Engagement, Page::BoostPerks]),
    (Some("Expression"), &[Page::Emoji, Page::Stickers, Page::Soundboard]),
    (Some("People"), &[Page::Members, Page::Roles, Page::Invites, Page::Access]),
    (Some("Apps"), &[Page::Integrations]),
    (Some("Moderation"), &[Page::Safety, Page::AuditLog, Page::Bans, Page::AutoMod]),
    (Some("Community"), &[Page::Community, Page::Onboarding, Page::Insights]),
    (None, &[Page::Template, Page::Delete]),
];

impl Page {
    pub fn label(self, community: bool) -> &'static str {
        match self {
            Page::Profile => "Server profile",
            Page::Tag => "Server tag",
            Page::Engagement => "Engagement",
            Page::BoostPerks => "Boost perks",
            Page::Emoji => "Emoji",
            Page::Stickers => "Stickers",
            Page::Soundboard => "Soundboard",
            Page::Members => "Members",
            Page::Roles => "Roles",
            Page::Invites => "Invites",
            Page::Access => "Access",
            Page::Integrations => "Integrations",
            Page::Safety => "Safety setup",
            Page::AuditLog => "Audit log",
            Page::Bans => "Bans",
            Page::AutoMod => "AutoMod",
            Page::Community if community => "Community overview",
            Page::Community => "Enable Community",
            Page::Onboarding => "Onboarding",
            Page::Insights => "Server insights",
            Page::Template => "Server template",
            Page::Delete => "Delete server",
        }
    }

    // Any one of these bits opens the page.
    fn needs(self) -> u64 {
        match self {
            Page::Emoji | Page::Stickers | Page::Soundboard => MANAGE_GUILD_EXPRESSIONS | CREATE_GUILD_EXPRESSIONS,
            Page::Members => MANAGE_GUILD | perm::KICK_MEMBERS | perm::BAN_MEMBERS | perm::MANAGE_ROLES | perm::MODERATE_MEMBERS,
            Page::Roles => perm::MANAGE_ROLES,
            Page::Integrations => MANAGE_GUILD | MANAGE_WEBHOOKS,
            Page::AuditLog => VIEW_AUDIT_LOG,
            Page::Bans => perm::BAN_MEMBERS,
            Page::Insights => VIEW_GUILD_INSIGHTS,
            Page::Delete => 0,
            _ => MANAGE_GUILD,
        }
    }
}

// What the signed-in member holds in one server.
#[derive(Clone, Copy, Debug)]
pub struct Me<'a> {
    pub id: &'a str,
    pub owner: bool,
    // Server-wide permission bits (Guild::permissions).
    pub permissions: u64,
    // Position of our highest role; roles at or above it are out of reach.
    pub top_position: i64,
    pub features: &'a [String],
}

impl Me<'_> {
    fn admin(&self) -> bool {
        self.owner || self.permissions & ADMINISTRATOR != 0
    }

    pub fn has(&self, bits: u64) -> bool {
        self.admin() || self.permissions & bits != 0
    }

    pub fn feature(&self, name: &str) -> bool {
        self.features.iter().any(|f| f == name)
    }

    pub fn can_open(&self, page: Page) -> bool {
        let community = self.feature("COMMUNITY");
        match page {
            Page::Delete => self.owner,
            Page::Onboarding | Page::Insights if !community => false,
            _ => self.has(page.needs()),
        }
    }

    // The nav as this member sees it: pages they can't open are left out, and so are empty sections.
    pub fn nav(&self) -> Vec<(Option<&'static str>, Vec<Page>)> {
        NAV.iter().map(|(title, pages)| (*title, pages.iter().copied().filter(|p| self.can_open(*p)).collect::<Vec<_>>())).filter(|(_, pages)| !pages.is_empty()).collect()
    }

    // Whether "Server settings" shows in the server menu at all.
    pub fn any_page(&self) -> bool {
        NAV.iter().flat_map(|(_, pages)| pages.iter()).any(|p| *p != Page::Delete && self.can_open(*p))
    }
}

// Why a role can't be edited, in Discord's words where it has them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoleLock {
    Open,
    // At or above our highest role.
    Above,
    // A bot's or integration's role: editable, but nobody can hand it out or delete it.
    Managed,
}

impl RoleLock {
    pub fn reason(self) -> Option<&'static str> {
        match self {
            RoleLock::Open => None,
            RoleLock::Above => Some("You can only edit roles below your highest role."),
            RoleLock::Managed => Some("This role is managed by an integration and can't be assigned or deleted."),
        }
    }
}

pub fn role_lock(me: &Me, position: i64, managed: bool) -> RoleLock {
    if !me.owner && position >= me.top_position {
        return RoleLock::Above;
    }
    if managed {
        return RoleLock::Managed;
    }
    RoleLock::Open
}

// A member can only hand out permissions they hold themselves; Administrator and the owner hold all.
pub fn can_toggle_permission(me: &Me, bit: u64) -> bool {
    me.admin() || me.permissions & bit == bit
}

// Roles drag only between the ones below our own highest role.
pub fn can_drag_role(me: &Me, position: i64, is_everyone: bool) -> bool {
    me.has(perm::MANAGE_ROLES) && !is_everyone && (me.owner || position < me.top_position)
}

// With only Create Expressions, a member edits and deletes just what they uploaded.
pub fn can_edit_expression(me: &Me, uploader_id: Option<&str>) -> bool {
    me.has(MANAGE_GUILD_EXPRESSIONS) || (me.has(CREATE_GUILD_EXPRESSIONS) && uploader_id == Some(me.id))
}

// Boost-locked controls: the server feature that unlocks each and the boost level that brings it.
pub const BANNER: (&str, u8) = ("BANNER", 2);
pub const INVITE_SPLASH: (&str, u8) = ("INVITE_SPLASH", 1);
pub const VANITY_URL: (&str, u8) = ("VANITY_URL", 3);
pub const ENHANCED_ROLE_COLORS: (&str, u8) = ("ENHANCED_ROLE_COLORS", 2);

#[cfg(test)]
mod tests {
    use super::*;

    fn me(permissions: u64, owner: bool) -> Me<'static> {
        Me { id: "me", owner, permissions, top_position: 5, features: &[] }
    }

    fn pages(me: &Me) -> Vec<Page> {
        me.nav().into_iter().flat_map(|(_, p)| p).collect()
    }

    #[test]
    fn moderator_sees_only_their_pages() {
        let m = me(VIEW_AUDIT_LOG | perm::BAN_MEMBERS, false);
        assert_eq!(pages(&m), vec![Page::Members, Page::AuditLog, Page::Bans]);
        assert_eq!(m.nav().iter().map(|(t, _)| *t).collect::<Vec<_>>(), vec![Some("People"), Some("Moderation")]);
        assert!(m.any_page());
        assert!(!me(perm::CREATE_INSTANT_INVITE, false).any_page());
    }

    #[test]
    fn owner_and_admin() {
        let owner = me(0, true);
        assert!(owner.can_open(Page::Delete));
        assert!(!owner.can_open(Page::Onboarding), "Onboarding needs Community");
        let admin = me(ADMINISTRATOR, false);
        assert!(admin.can_open(Page::Roles) && !admin.can_open(Page::Delete));
        let features = ["COMMUNITY".to_string()];
        let community = Me { features: &features, ..owner };
        assert!(community.can_open(Page::Insights));
        assert_eq!(Page::Community.label(true), "Community overview");
    }

    #[test]
    fn expressions_with_create_only() {
        let m = me(CREATE_GUILD_EXPRESSIONS, false);
        assert!(m.can_open(Page::Emoji));
        assert!(can_edit_expression(&m, Some("me")));
        assert!(!can_edit_expression(&m, Some("other")));
        assert!(can_edit_expression(&me(MANAGE_GUILD_EXPRESSIONS, false), Some("other")));
    }

    #[test]
    fn role_hierarchy() {
        let m = me(perm::MANAGE_ROLES | perm::KICK_MEMBERS, false);
        assert_eq!(role_lock(&m, 5, false), RoleLock::Above);
        assert_eq!(role_lock(&m, 4, true), RoleLock::Managed);
        assert_eq!(role_lock(&m, 4, false), RoleLock::Open);
        assert_eq!(role_lock(&me(0, true), 99, false), RoleLock::Open);
        assert!(can_toggle_permission(&m, perm::KICK_MEMBERS));
        assert!(!can_toggle_permission(&m, perm::BAN_MEMBERS));
        assert!(can_toggle_permission(&me(ADMINISTRATOR, false), perm::BAN_MEMBERS));
        assert!(can_drag_role(&m, 4, false));
        assert!(!can_drag_role(&m, 5, false));
        assert!(!can_drag_role(&m, 0, true));
    }
}
