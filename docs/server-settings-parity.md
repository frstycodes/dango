# Server settings ↔ Discord parity

Pages and fields read from Discord web's server settings on 2026-11-18 (an owned server, settings layer only, nothing changed). Endpoints from discord-api-docs (`developers/resources/*`, read the same day); rows marked *user client* use routes Discord's own app calls that the docs don't list (not verified live).

Status: **Built** = in the app's server settings window (`src/server_settings/`), against the endpoint shown; **Link-out** = the page opens Discord, because a user account can't do it through the public API; **Not built** = left out, with the reason. `WAFFLE_DEMO=server-settings…` scenes show each page with made-up data.

Gating (`src/server_settings/access.rs`): a page shows when the member holds any of its permissions; owner and Administrator see everything. Roles at or above your highest role, and permissions you don't hold, are locked as in Discord.

| Discord page | Field / action | Endpoint | Needs | Waffle |
|---|---|---|---|---|
| **Server profile** | Name, icon, banner (BANNER), description | `PATCH /guilds/{id}` `name icon banner description` | Manage Server | Built |
| | Traits (up to 5), games, private profile | *user client* profile fields | Manage Server | Not built: undocumented shape |
| **Server tag** | Name, badge, colour | *user client* | Manage Server, boost | Not built: undocumented, boost-locked |
| **Engagement** | System messages channel | `PATCH /guilds/{id}` `system_channel_id` | Manage Server | Built |
| | Welcome message, sticker replies, boost message, setup tips | `system_channel_flags` (suppress bits 0–3) | Manage Server | Built |
| | Activity feed toggle | *user client* | Manage Server | Not built |
| | Default notifications | `default_message_notifications` | Manage Server | Built |
| | Inactive channel, timeout (1, 5, 15, 30, 60 min) | `afk_channel_id afk_timeout` | Manage Server | Built |
| | Server widget on/off, invite channel | `GET/PATCH /guilds/{id}/widget` | Manage Server | Built |
| **Boost perks** | Boost progress bar | `premium_progress_bar_enabled` | Manage Server | Built |
| | Invite background (INVITE_SPLASH, lvl 1) | `splash` | Manage Server | Built |
| | Custom invite link (VANITY_URL, lvl 3): shown and copied; changing it stays on Discord | guild `vanity_url_code` | Manage Server | Built |
| | Boost level, count | guild `premium_tier premium_subscription_count` | anyone | Built |
| **Emoji** | List, upload, delete (rename not built) | `GET/POST /guilds/{id}/emojis`, `PATCH/DELETE …/{emoji}` | Create or Manage Expressions | Built |
| **Stickers** | List, upload (multipart), delete (edit not built) | `GET/POST /guilds/{id}/stickers`, `PATCH/DELETE …/{sticker}` | Create or Manage Expressions | Built |
| **Soundboard** | List, upload, delete (edit not built) | `GET/POST /guilds/{id}/soundboard-sounds`, `PATCH/DELETE …/{sound}` | Create or Manage Expressions | Built |
| **Members** | Search by name or id | `GET /guilds/{id}/members/search` | Kick, Ban, Manage Roles, Timeout or Manage Server | Built |
| | Kick, ban (delete history ≤ 7 days), timeout ≤ 28 days | `DELETE …/members/{u}`, `PUT …/bans/{u}`, `PATCH …/members/{u}` | Kick / Ban / Timeout | Built |
| | Prune (count, then prune) | `GET/POST /guilds/{id}/prune` | Kick | Built |
| | Sort, page through all members | *user client* `members-search` | | Not built |
| **Roles** | List, member counts, search | `GET /guilds/{id}/roles`, `…/roles/member-counts` | Manage Roles | Built |
| | Create | `POST /guilds/{id}/roles` | Manage Roles | Built |
| | Reorder (up/down, with Undo; drag not built) | `PATCH /guilds/{id}/roles` `[{id, position}]`, only moved roles | Manage Roles, below your top role | Built |
| | Display: name, colour, gradient (ENHANCED_ROLE_COLORS), shown separately, mentionable (holographic and role icon not built) | `PATCH …/roles/{r}` `name colors icon unicode_emoji hoist mentionable` | Manage Roles | Built |
| | Permissions with search, clear | `permissions` (decimal string) | only bits you hold | Built |
| | Manage members: list, remove with Undo (adding from the role not built) | *user client* `…/roles/{r}/member-ids`; `PUT/DELETE …/members/{u}/roles/{r}` | Manage Roles | Built |
| | View server as role | client-side only | Manage Roles | Not built |
| | Links (linked roles) | application role connections | | Link-out |
| | Delete | `DELETE …/roles/{r}` | Manage Roles, not managed | Built |
| **Invites** | List, revoke | `GET /guilds/{id}/invites`, `DELETE /invites/{code}` | Manage Server | Built |
| | Pause invites (≤ 24 h) | `PUT /guilds/{id}/incident-actions` | Manage Server | Built |
| **Access** | Rules screening, age-restricted server | *user client* member verification | Manage Server | Not built |
| **Integrations** | Bots and apps, remove | `GET/DELETE /guilds/{id}/integrations` | Manage Server | Built |
| | Webhooks: list, create, copy URL, delete (rename, move and avatar not built) | `GET /guilds/{id}/webhooks`, `POST /channels/{c}/webhooks`, `PATCH/DELETE /webhooks/{w}` | Manage Webhooks | Built |
| | Channels followed | channel follows | | Not built |
| | App directory | | | Link-out |
| **Safety setup** | Verification level, media filter | `verification_level explicit_content_filter` | Manage Server | Built |
| | Safety notifications channel | `safety_alerts_channel_id` | Manage Server | Not built (field easy to add) |
| | Require 2FA for moderation | `POST /guilds/{id}/mfa` (owner) | Owner | Not built |
| **Audit log** | Entries as sentences, expandable changes | `GET /guilds/{id}/audit-logs` | View Audit Log | Built |
| | Filter by user, by action | `user_id`, `action_type` | | Built |
| | Load more | `before` = oldest id shown | | Built |
| **Bans** | List, search, unban | `GET /guilds/{id}/bans` (search filters the loaded list), `DELETE …/bans/{u}` with Undo | Ban Members | Built |
| **AutoMod** | Mention spam, suspected spam, flagged words (presets), custom words; on/off, delete | `GET/POST /guilds/{id}/auto-moderation/rules`, `PATCH/DELETE …/{rule}` | Manage Server | Built |
| | Actions beyond Block (alert channel, timeout, block interactions), exempt roles and channels, member-profile rules | rule `actions exempt_roles exempt_channels` | | Not built: rules are set up with Block; edit the rest on Discord |
| **Enable Community** | Turn on with rules and updates channels | `PATCH /guilds/{id}` `features += COMMUNITY` | Manage Server | Built |
| **Community** | Onboarding: status, question and channel counts; editing on Discord | `GET /guilds/{id}/onboarding` | Manage Server | Built, Link-out |
| | Welcome screen | `GET/PATCH /guilds/{id}/welcome-screen` | Manage Server | Not built |
| | Server insights | none for user accounts | View Insights | Link-out |
| | Discovery | *user client* | Manage Server | Link-out |
| **Server template** | Generate, sync, delete | `GET/POST /guilds/{id}/templates`, `PUT/DELETE …/{code}` | Manage Server | Built |
| **Delete server** | Type the server name to confirm | *user client* `POST /guilds/{id}/delete` | Owner | Built |

