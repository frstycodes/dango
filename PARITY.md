# Waffle ↔ Discord parity

Reference: the user's Discord desktop client (Midnight theme), captured 2026-11-14.
Order: features first (most used at the top), then polish. `[x]` done, `[~]` partial, `[ ]` open.

Missing features, with how Discord's UX works for each: [docs/discord-parity-gaps.md](docs/discord-parity-gaps.md).
User complaints about Discord and how Waffle answers them: [docs/better.md](docs/better.md).

## Features

### Everyday (build first)
- [x] Right-click context menus: message (reply, react, edit, delete, copy text, copy link, pin, mark unread), channel (mark as read, mute, copy link), server (mark as read, mute, leave), user (profile, mention, message, copy ID)
- [x] User profile popout on avatar/name click: banner, avatar + status, display name, username, tag, roles, "Message @x" box
- [x] Emoji picker: composer button and "add reaction" with search, categories, recents and server emoji
- [x] Status menu from the user panel: custom status, presence (Idle, Do not disturb and Invisible for 15 minutes to Forever from a hover submenu), accounts; "Set your status" with a live profile preview, emoji, text, Clear after and presence (timed statuses fall back to Online only while Waffle is open)
- [x] Quick switcher (Ctrl/Cmd+K)
- [x] "NEW" unread divider in the transcript, plus a "You're viewing older messages / Jump to present" bar
- [x] Markdown parity (timestamps, spoilers, subtext): spoilers `||x||`, block quotes `>`, headers `#`, subtext `-#`, timestamps `<t:…:R>`, masked links, lists
- [x] Reply composer toggle "@ ON / OFF" (ping the author) and a jump arrow on the reply bar
- [x] Paste or drag-and-drop images and files into the composer, with attachment previews before sending
- [x] Voice channel row: green speaker and elapsed timer while people are in it; your own row highlighted
- [x] Voice: input and output device menus (the chevrons by mic and headphones), per-user volume and local mute (right-click a participant), rebindable push-to-talk
- [x] Popups and menus block the UI behind them
- [x] Drag servers in the rail: reorder, drop on a server to make a folder, drop into or out of folders (saved to Discord: not verified that the legacy settings endpoint still accepts folders)
- [x] Voice panel: Discord's connection stages (Awaiting Endpoint, Authenticating, RTC Connecting, ICE Checking, DTLS Connecting, Voice Connected, No Route, RTC Disconnected), signal icon by ping, noise suppression toggle, camera and screen share buttons
- [x] Connection details popout: ping graph, average and last ping, packet loss both ways, hostname, encryption mode, end-to-end encryption (DAVE)
- [x] Call view: participant tiles with speaking ring, mute and deafen marks, cameras, LIVE and Watch Stream, the stream large with a strip below; call bar with mic, camera (device chevrons), share screen, hang up
- [x] Settings: Discord's new layout (Account, Notifications, Voice & Video with Voice/Camera/Sounds/Advanced, Keybinds, Appearance, Log Out), saved to settings.json
- [x] Keybinds: global shortcuts for push to talk, push to mute, toggle mute and deafen, voice activity mode, disconnect; record, edit, delete
- [x] Go Live window: choose screen, app or window, then resolution, frame rate and sound
- [ ] Voice: cameras (send and receive), DM and group calls
- [ ] In-app video playback in the gallery (native AVPlayer view) instead of the system player

### Voice extras
- [x] Discord's sound cues: mute, unmute, deafen, undeafen, join, leave, disconnect, stream start/end, DM and mention (macOS)
- [~] Icon toggle animation: a quick pop on mute/deafen; Discord's own Lottie animations are not ported (gpui has no Lottie player)
- [x] Go Live picker: choose an app or window to share (engine: `voice::shareable_windows`, `ShareSource`)
- [x] Keybinds listen through a keyboard hook (event tap on macOS, low-level hook on Windows): any keys held together, e.g. = + Backspace, and a bare key such as ` still types in other apps
- [ ] Sound cues on Windows/Linux (rodio)

### Regular
- [x] GIF picker (Discord's Tenor search)
- [~] Stickers: sent from a Stickers tab beside GIFs and Emoji (recent, your servers, Discord's packs, search by name and tags) and drawn at 160px, APNG animated; Lottie stickers (most of Discord's own packs) show a placeholder because there's no Lottie player
- [x] Pins popover from the header
- [x] Inbox: Unreads (unmuted channels with their newest cached messages, Mark read, Mark all read) and Mentions with filter chips (@everyone, roles, this server only); Discord's "For you" notifications aren't built
- [x] Threads: thread chips under messages, joined threads nested under their channel, a Threads popout in the header, Create Thread, Discord's thread menu (join, close, lock, mute, notifications, delete, leave)
- [ ] Threads open in the main view with a breadcrumb; Discord opens them in a split pane beside the channel
- [ ] Threads popout lists active threads only; Discord also searches older and archived ones
- [x] Forum and media channels: post list (active from the gateway, older via THREAD_SEARCH), previews via post-data, New Post, open a post as a thread (not yet checked against a live forum)
- [ ] Forum tags, sort and layout options, and the grid view of media channels
- [x] Friends page (Online, All, Pending) as the Direct Messages home
- [x] System notifications for DMs and mentions (Discord's rules: DND, muted servers and channels, notification levels, suppressed @everyone and roles), plus a Dock badge (macOS; Windows taskbar badge open)
- [x] Server dropdown (header chevron): mark as read, notification settings, leave server
- [x] Notification bell in the channel header: mute and notification level, the same entries as the channel's right-click menu; a slashed bell while muted
- [x] Channel settings (Edit channel / Edit category): Overview (name, topic, slowmode, age-restricted, hide threads after; voice bitrate, user limit, video quality, region), Permissions (private switch, category sync, role and member overwrites), Invites (revoke), Webhooks (copy URL, delete), Delete; pages gated on our permissions in the channel; searchable from its nav (settings registry)
- [ ] Channel settings: add a member overwrite, create webhooks, forum tags and default reaction, stage and announcement specifics
- [x] Mark channel read with Esc; Alt+↑/↓ to move between channels
- [x] Message search filters: from:, mentions:, has:, in:, before:, after:, during:, pinned: (and search inside DMs)
- [ ] Search autocomplete popout (Discord suggests filters and names as you type)
- [x] Persist the message cache to disk so a cold start shows the last messages instantly (newest 50 per channel)

### Rare (skip unless asked)
- Server boosts, events, discovery, creating servers or channels, invites, Nitro, shop, app directory.

## Polish (flagged differences, fix after features)

### Measured from Discord web (2026-11-14)
Title bar 32px; rail 72px, icons 40px; channel rows 32px in 34px slots, 4×8 padding, radius 8, 16/24 weight 500;
categories 14/18 weight 500; headers 48px + hairline, padding 8/8/8/16; messages 16/22, names 16 weight 500,
timestamps 12 weight 500 #81828A; group top margin 17px, padding 2/24/2/72, avatar 40; composer 56px, radius 8,
8px below, form padding 0 8; user panel 58px, radius 8, inset 8, border #97979F @16%.
Nameplates: palette tint gradient 10%→40% across the card; art contain-fit right, 40% in the user panel; in the
member list full strength, masked to 30% under the name and full 50px past it.
- [x] Applied: title bar, rows, categories, type sizes and weights, composer, user panel, nameplates
- [x] Nameplate palette colours: Discord's exact table, dark and light
- [ ] Avatar decorations: static frame only; Discord animates them on hover

### Colour
Tokens: `docs/discord-tokens.json` (Discord web, Midnight, read 2026-11-14).
- [x] Status, danger, link, hover/selected, composer and text colours set from Discord's tokens
- [x] Midnight surfaces, hairlines and composer sampled from Discord
- [x] Red reads washed out: LIVE badge, mention badges, Delete and Log Out use gpui-component's red instead of Discord's (#F23F43 status, #DA373C for buttons, LIVE #ED4245)
- [ ] Unread pill and unread channel text are full white in Discord; check ours against #F2F3F5
- [x] Selected channel row fill: --interactive-background-selected #97979F @24%
- [x] Muted text is --text-subtle #96979E (timestamps use it; Discord's timestamps are --text-muted #81828A)
- [ ] Role colours in light mode: Discord darkens them for contrast; ours use the raw value

### Typography
- [x] Message text is 16px in Discord; ours is 15px
- [x] Author names are 16px medium (500) in Discord; ours are semibold
- [x] Channel names in the sidebar are 16px medium; ours are 15px regular
- [x] Category labels are 12px semibold; matches
- [x] Member names are 16px; ours are 15px

### Layout and size
- [ ] Friends page: Discord adds an "Add Friend" button, a search field and an "Active Now" column
- [ ] DM list: Discord has "Find or start a conversation" on top and a "+" beside "Direct Messages"
- [ ] Go Live picker: Discord shows live thumbnails of each screen and window; ours is a list
- [ ] Spoilers: Discord covers them with a grey box that fades on click; ours shows a bar of █
- [ ] GIF picker: Discord opens on categories (Trending, Favourites, moods); ours opens on trending results
- [ ] Title bar right side: Inbox, Help and Updates icons are missing
- [x] Channel header right side: Threads, Notification bell and Pins icons
- [ ] Server header: "Invite to Server" icon is missing; Events and Server Boosts rows (rare, skip)
- [ ] Category rows: "+ Create Channel" on hover is missing (rare, skip)
- [ ] Channel row hover: "Invite" and "Edit Channel" icons on the right (rare, skip)
- [ ] Rail bottom: "Add a Server" and "Discover" buttons (rare, skip)
- [~] Composer right side: GIF, Sticker and Emoji buttons open one tabbed picker and show which is open; Gift and Apps are missing
- [x] User panel: device chevrons beside mic and headphones
- [ ] Member row: Discord shows the activity icon (a game controller) before the text
- [ ] Member list group header "Council — 3": spacing check
- [ ] Message hover bar: Discord shows four recent reactions, then React, Reply, Forward and More; ours shows three quick reactions, Reply, Edit and Delete
- [ ] Reply connector line: Discord's curve starts from the avatar centre
- [ ] Image attachments: Discord shows a size badge for files and a hover download button
- [x] Voice channel participant row is 32px with a 24px avatar in Discord
- [ ] Voice panel: Activities and Soundboard buttons are missing (not built)
- [x] Server nicknames and server avatars (messages, history, member list, voice, typing)
- [x] Voice participants resolve to real names and avatars (members requested from the gateway)
- [x] Owner crown in the member list (--text-feedback-warning, 14px)
- [x] Stream previews: thumbnails on call tiles and a hover card on LIVE voice rows (GET /streams/{key}/preview, refreshed each minute)
- [x] A message that is only a GIF or image link shows just the embed
- [x] Drag people between voice channels (Move Members) and drag channels to reorder or into a category (Manage Channels)
- [x] Optimistic actions: settings, pins, reactions, roles, voice moderation, relationships, ignore, nicknames, channel and thread edits, deletes, leaving a server; rolled back with a toast if Discord refuses
- [x] Right-click menus rebuilt from Discord's own menu code: user (server, voice, DM, friends), message, channel, voice channel, category, server icon, folder, server dropdown
- [ ] Menu items for features not built yet: Forward, Translate, Bookmarks/Reminders, Apps, Report, Favorites, Activities, Mod View, Server Settings, Create Event
- [ ] Edit Channel and Folder Settings: Discord opens full settings screens; ours rename only
- [x] GIFs play inline: Tenor/Giphy MP4s (OpenH264, every B-frame decoded) and animated .gif/.webp attachments
- [x] Video attachments play inline (H.264 + AAC MP4s): play/pause, scrubber, volume and mute, fullscreen, one at a time; other codecs (HEVC, VP9) open in the browser
- [x] Embeds: markdown descriptions and fields, author icon, footer icon and timestamp, Discord's card styling
- [ ] Mention pills: Discord draws mentions as tinted pills; ours are coloured links (gpui-component markdown can't give links a background)
- [x] Call view: Discord's tile order, overlay name pills, stream tiles with dimmed preview, 112px strip when watching
- [x] Muted channels dim, Hide Muted Channels, Hide Names; muted servers keep mention badges only
- [ ] Call view tiles: Discord tints each tile with the user's banner colour; ours are neutral
- [ ] Settings search only filters page names; Discord searches every setting

- [ ] Reaction picker opens above the composer; Discord anchors it beside the message
- [ ] Profile popout lacks the badges row, member-since dates and the "Roles" add button

### Motion
- [ ] Rail pill: Discord grows it at about 150ms ease-out; ours springs over ~220ms
- [ ] Hover fills: Discord changes instantly; ours do too (check)
- [ ] Typing dots: Discord's pulse in a wave at roughly 1.4s; ours run at 1.2s
