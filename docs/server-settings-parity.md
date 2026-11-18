# Server settings ↔ Discord parity

Pages and fields read from Discord web's server settings on 2026-11-18 (an owned server, settings layer only, nothing changed). Endpoints from discord-api-docs (`developers/resources/*`, read the same day); rows marked *user client* use routes Discord's own app calls that the docs don't list (not verified live).

Status: **API** = call built and unit-tested in `src/server_settings/api.rs`; **Mock** = drawn in `design/server-settings.html`; **UI** = built in the app; **Link-out** = a user account can't do it through the API, so the page opens Discord. Every row below is API-only until the window lands on the shared settings shell (PR #18).

Gating (`src/server_settings/access.rs`): a page shows when the member holds any of its permissions; owner and Administrator see everything. Roles at or above your highest role, and permissions you don't hold, are locked as in Discord.

| Discord page | Field / action | Endpoint | Needs | Dango |
|---|---|---|---|---|
| **Server profile** | Name, icon, banner (BANNER), description | `PATCH /guilds/{id}` `name icon banner description` | Manage Server | API, Mock |
| | Traits (up to 5), games, private profile | *user client* profile fields | Manage Server | Not built: undocumented shape |
| **Server tag** | Name, badge, colour | *user client* | Manage Server, boost | Not built: undocumented, boost-locked |
| **Engagement** | System messages channel | `PATCH /guilds/{id}` `system_channel_id` | Manage Server | API, Mock |
| | Welcome message, sticker replies, boost message, setup tips | `system_channel_flags` (suppress bits 0–3) | Manage Server | API, Mock |
| | Activity feed toggle | *user client* | Manage Server | Not built |
| | Default notifications | `default_message_notifications` | Manage Server | API, Mock |
| | Inactive channel, timeout (1, 5, 15, 30, 60 min) | `afk_channel_id afk_timeout` | Manage Server | API, Mock |
| | Server widget on/off, invite channel | `GET/PATCH /guilds/{id}/widget` | Manage Server | API |
| **Boost perks** | Boost progress bar | `premium_progress_bar_enabled` | Manage Server | API |
| | Invite background (INVITE_SPLASH, lvl 1) | `splash` | Manage Server | API |
| | Custom invite link (VANITY_URL, lvl 3) | `GET /guilds/{id}/vanity-url`; *user client* `PATCH` | Manage Server | API |
| | Boost level, count | guild `premium_tier premium_subscription_count` | anyone | API (read) |
| **Emoji** | List, upload, rename, delete | `GET/POST /guilds/{id}/emojis`, `PATCH/DELETE …/{emoji}` | Create or Manage Expressions | API, Mock |
| **Stickers** | List, upload (multipart), edit, delete | `GET/POST /guilds/{id}/stickers`, `PATCH/DELETE …/{sticker}` | Create or Manage Expressions | API, Mock |
| **Soundboard** | List, upload, edit name/volume/emoji, delete | `GET/POST /guilds/{id}/soundboard-sounds`, `PATCH/DELETE …/{sound}` | Create or Manage Expressions | API, Mock |
| **Members** | Search by name or id | `GET /guilds/{id}/members/search` | Kick, Ban, Manage Roles, Timeout or Manage Server | API, Mock |
| | Kick, ban (delete history ≤ 7 days), timeout ≤ 28 days | `DELETE …/members/{u}`, `PUT …/bans/{u}`, `PATCH …/members/{u}` | Kick / Ban / Timeout | API, Mock |
| | Prune (count, then prune) | `GET/POST /guilds/{id}/prune` | Kick | API |
| | Sort, page through all members | *user client* `members-search` | | Not built |
| **Roles** | List, member counts, search | `GET /guilds/{id}/roles`, `…/roles/member-counts` | Manage Roles | API, Mock |
| | Create | `POST /guilds/{id}/roles` | Manage Roles | API, Mock |
| | Reorder (drag) | `PATCH /guilds/{id}/roles` `[{id, position}]`, only moved roles | Manage Roles, below your top role | API, Mock |
| | Display: name, colour, gradient / holographic (ENHANCED_ROLE_COLORS), icon (ROLE_ICONS), shown separately, mentionable | `PATCH …/roles/{r}` `name colors icon unicode_emoji hoist mentionable` | Manage Roles | API, Mock |
| | Permissions with search, clear | `permissions` (decimal string) | only bits you hold | API, Mock |
| | Manage members: list, add, remove | *user client* `…/roles/{r}/member-ids`; `PUT/DELETE …/members/{u}/roles/{r}` | Manage Roles | API, Mock |
| | View server as role | client-side only | Manage Roles | Not built |
| | Links (linked roles) | application role connections | | Link-out |
| | Delete | `DELETE …/roles/{r}` | Manage Roles, not managed | API, Mock |
| **Invites** | List, revoke | `GET /guilds/{id}/invites`, `DELETE /invites/{code}` | Manage Server | API, Mock |
| | Pause invites (≤ 24 h) | `PUT /guilds/{id}/incident-actions` | Manage Server | API, Mock |
| **Access** | Rules screening, age-restricted server | *user client* member verification | Manage Server | Not built |
| **Integrations** | Bots and apps, remove | `GET/DELETE /guilds/{id}/integrations` | Manage Server | API, Mock |
| | Webhooks: list, create, rename, move, avatar, delete | `GET /guilds/{id}/webhooks`, `POST /channels/{c}/webhooks`, `PATCH/DELETE /webhooks/{w}` | Manage Webhooks | API, Mock |
| | Channels followed | channel follows | | Not built |
| | App directory | | | Link-out |
| **Safety setup** | Verification level, media filter | `verification_level explicit_content_filter` | Manage Server | API, Mock |
| | Safety notifications channel | `safety_alerts_channel_id` | Manage Server | Not built (field easy to add) |
| | Require 2FA for moderation | `POST /guilds/{id}/mfa` (owner) | Owner | Not built |
| **Audit log** | Entries as sentences, expandable changes | `GET /guilds/{id}/audit-logs` | View Audit Log | API (`audit.rs`), Mock |
| | Filter by user, by action | `user_id`, `action_type` | | API |
| | Load more | `before` = oldest id shown | | API |
| **Bans** | List, search, unban | `GET /guilds/{id}/bans`, *user client* `…/bans/search`, `DELETE …/bans/{u}` | Ban Members | API, Mock |
| **AutoMod** | Mention spam, suspected spam, flagged words (presets), custom words, member profile | `GET/POST /guilds/{id}/auto-moderation/rules`, `PATCH/DELETE …/{rule}` | Manage Server | API, Mock |
| | Actions: block (custom message), alert channel, timeout, block interactions; exempt roles and channels | rule `actions exempt_roles exempt_channels` | | API, Mock |
| **Enable Community** | Turn on with rules and updates channels | `PATCH /guilds/{id}` `features += COMMUNITY` | Manage Server | API, Mock |
| **Community** | Onboarding | `GET/PUT /guilds/{id}/onboarding` | Manage Server | API, Mock |
| | Welcome screen | `GET/PATCH /guilds/{id}/welcome-screen` | Manage Server | API |
| | Server insights | none for user accounts | View Insights | Link-out |
| | Discovery | *user client* | Manage Server | Link-out |
| **Server template** | Generate, sync, delete | `GET/POST /guilds/{id}/templates`, `PUT/DELETE …/{code}` | Manage Server | API |
| **Delete server** | Type the server name to confirm | *user client* `POST /guilds/{id}/delete` | Owner | API, Mock |

