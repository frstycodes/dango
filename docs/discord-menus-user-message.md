# Discord context menus: user + message (code-derived spec)

Source: discord.com web app webpack modules, read 2026-11-14. The module ids are minified and change between builds. Labels come from the app's own intl module (en-US). Each group listed below is one `MenuGroup`. The menu draws a separator between groups that have items and hides empty groups.

Conventions
- **me** = current user, **T** = target user. A **non-user bot** is `user.isNonUserBot()` (webhook / system-style author). When T is one, almost every menu shows only "Copy User ID".
- Kinds: `item`, `checkbox`, `submenu`, `slider` (control item), `danger` (red, `color:"danger"`), `radio`.
- Flags (component props): `media` = showMediaItems, `chat` = showChatItems (default on), `modal` = showModalItems (default on), `call` = showChannelCallItems, `stage` = showStageChannelItems. `popout` = the menu was opened in a popped-out window (`context === POPOUT`). `overlay` = the in-game overlay.
- "Guild-manage(P)" means `PermissionStore.canManageUser(P, T, guild)`. It requires permission P and T's highest role below my highest role (or me = owner).
- "Dev mode" means Settings > Advanced > Developer Mode is on.

## Which user menu opens where

| Surface | Module | Props |
|---|---|---|
| Server chat username / avatar / member list (guild channel) | 107632 `GuildChannelUserContextMenu` | defaults (`chat`, `modal` on; `media` off) |
| Server voice channel user row (sidebar) | 107632 | `media:true`, `stage: channel.isGuildStageVoice()` |
| RTC panel / call tile in a guild | 107632 | `media:true` |
| DM username (1:1 DM) | 385913 `DMUserContextMenu` | `targetIsUser:true` (`showMute` true from chat, false from some openers) |
| Group DM member | 183245 `GroupDMUserContextMenu` | defaults |
| No channel (Friends list rows, generic) | 668569 `UserGenericContextMenu` | `media` only from the RTC panel |
| User not resolvable | 446828 (not loaded; contents unknown) | |

The router (841549 `wQ`) picks the menu in this order: group DM → 183245, DM → 385913, guild channel → 107632, otherwise → 668569. The Friends list (module 414711) opens 668569 with only `user`. That file was not read in full, so this mapping is believed, not verified.

`showChannelCallItems:true` is not set by any loaded caller. The "call tile" group (Pop Out, Show Non-Video Participants, Show My Own Camera) is therefore probably passed by video-call tile code in a chunk that was not loaded. Unknown.

---

## 1. Server user menu — navId `user-context` (107632)

If T is a non-user bot, only the last group (Copy User ID) shows. Otherwise:

| # | Group | Items (in order, each may self-hide) |
|---|---|---|
| G1 | Stage | Invite to Speak / Speak on Stage · (`media` && !`stage`) Ask to Stream · (`stage`) Move to Audience / Leave the Stage |
| G2 | Profile & social | (`modal`) Profile · (`modal`) Try This Name Style · (`chat`) Mention · Message · Start a Call · Ring · (`modal`) Add/Edit Note · (`modal`) Add/Change Friend Nickname · Watch Stream |
| G3 | Volume | (`media`) User Volume slider |
| G4 | Activities | (`modal`) activity items (Join / Ask to Join / Invite to Join …) |
| G5 | Media / social actions | (`media`) Mute, Mute Soundboard, Disable Video (or self Mute/Deafen) · (`media`) View Verification Code · (`media`) Focus This Person · (`call`) Change Video Background · (`modal`) Change Nickname / Edit Per-server Profile · Apps ▸ · (`modal`) Invite to Server ▸ · Add/Remove Friend · (`modal`) Ignore · (`modal`) Block |
| G6 | Server | Roles ▸ · Move to ▸ |
| G7 | (`call` only) | Pop Out User/Video/Stream · Show Non-Video Participants · Show My Own Camera |
| G8 | Moderation | Open in Mod View · Verify/Unverify Member · Server Mute · Server Deafen · Disconnect · Timeout/Remove Timeout · Kick · Ban |
| G9 | Dev | Copy User ID |

### Item details

| Label (variants) | id | Kind | Shows when | Action |
|---|---|---|---|---|
| Invite to Speak / Speak on Stage | `invite-speaker` | item | I am in a stage channel, T has a voice state there, and T is not a speaker. Self: "Speak on Stage" if I'm a stage moderator or was invited to speak (awaiting ack). Hidden for self in one age-gated case. Other: "Invite to Speak" only if I'm a stage moderator | self: become speaker (age-verification modal if required); other: send invite-to-speak (sets suppress=false request) |
| Ask to Stream / Ask to Stream {app} | `request-to-stream` | item (subtext + disabled during cooldown) | `media`, not a stage. T ≠ me, T is playing a game (or the experiment is on), I have SEND_MESSAGES in the channel, T has STREAM in the channel, the channel is not a stage, and T is not already streaming | Sends an activity invite of type STREAM_REQUEST to the channel, mentioning T |
| Move to Audience / Leave the Stage | `audience` | item | `stage`, T is a speaker. Self → "Leave the Stage". Other → requires MUTE_MEMBERS on the channel | Move T (or me) back to the audience |
| Profile | `user-profile` | item | always (`modal`) | Opens the user profile modal |
| Try This Name Style | `try-display-name-style` | item (Nitro wheel icon) | T has a display-name style and an experiment places the item at the top level (Nitro try-on promo) | Opens the name-style try-on editor |
| Mention | `mention` | item | `chat`. The composer channel exists and (it's a DM/group DM, or I have SEND_MESSAGES) | Inserts `@username` (raw `<@id>`) into the composer |
| Message | `message-user` | item | T ≠ me and not `popout` | Opens/creates a DM with T and navigates to it |
| Start a Call | `call` | item, **disabled if T is blocked** | T ≠ me, not `popout`, I'm not already in the voice call of my DM with T, T not a bot, T not provisional | Opens the DM with `joinCall`. A confirmation modal shows first unless the user disabled it |
| Ring to Voice / Ring to Call; Stop Ringing | `ring` / `stop-ringing` | item | Channel = **my current voice channel**. T is a friend, T ≠ me, not a bot/system/provisional user. The channel type is callable, the call has ≥1 participant, and T is not a participant. "Ring to Voice" in a guild, "Ring to Call" in a DM. If T is a ringing participant → "Stop Ringing" | Ring / stop ringing T |
| Add Note / Edit Note (Loading Note) | `note` | item, subtext "Only visible to you" | T ≠ me, not streamer-mode hide-personal-info, not popout/overlay | Opens the profile modal scrolled to the Note field |
| Add Friend Nickname / Change Friend Nickname | `add-friend-nickname` / `edit-friend-nickname` | item | T is a friend | Opens the friend-nickname modal |
| Watch Stream | `watch-stream` | item | T has a STREAMING-type presence activity with a URL (Twitch/YouTube, not Go Live) | `window.open(url)` |
| User Volume | `user-volume` | slider | `media`, T ≠ me. Range 0–200 in the desktop app, 0–100 on web | `setLocalVolume(T, v)` (local only) |
| Activity items | `invite-to-join`, `invite-to-listen`, `invite-to-join-embedded`, `join`, `ask-to-join`, `spotify-play-*`, `spotify-sync-*`, `embedded-activity-join-*` | item (subtext = activity name; Join may be disabled/loading: "Joining", "Joined", "You can't join yourself") | Only if the channel allows SEND_MESSAGES (or is a DM, or a guild voice channel). **My** joinable game → "Invite to Join". My Spotify → "Invite to Listen Along" (hidden for self target). **T's** joinable game → "Join" if joinable, else "Ask to Join". T's Spotify → Play + "Listen Along". T's embedded activity → "Join Activity" | Sends an activity invite (JOIN / LISTEN / JOIN_REQUEST), or joins |
| Mute (other) | `mute` | checkbox | `media`, T ≠ me | Local mute toggle (`toggleLocalMute`) |
| Mute (self) / Deafen (self) | `mute` / `deafen` | checkbox | `media`, T = me | `toggleSelfMute` / `toggleSelfDeaf` |
| Mute Soundboard | `soundboard-sound-mute` | checkbox | `media`, T ≠ me | Local soundboard mute for T |
| Disable Video | `disable-video` | checkbox; subtext "Unstable Internet Connection" when auto-disabled | `media`, T ≠ me, the media engine supports DISABLE_VIDEO | Toggle local video for T. If auto-disabled, a confirm dialog "Turn on video anyway" shows first |
| View Verification Code | `secure-frames-user-verification` | item | `media`, the channel has E2EE (secure frames) on, T ≠ me | Opens the E2EE verification modal |
| Focus This Person / Unfocus This Person | `focus-video` | item | `media`, T is in the channel, the channel has video, and the call layout is not MINIMUM/NORMAL | Select / deselect T as the focused participant |
| Change Video Background / Preview Camera | `change-video-background` | item | `call`, T = me, video supported, a camera exists, (video off or the experiment is on) | Opens the camera preview / background modal |
| Edit Per-server Profile (Edit Hub Profile in hubs) / Change Nickname | `change-nickname` | item | Guild exists, not popout/overlay, T not a guest/lurker. Self: always → "Edit Per-server Profile". Other: needs Guild-manage(MANAGE_NICKNAMES) → "Change Nickname" | Self: opens Settings › Profiles for this guild. Other: change-nickname modal (PATCH member nick) |
| Apps | `apps` (with `command-search`, `menu-commands-*`) | submenu | Always built. Lists USER-type app commands (search box, "Frequently Used Commands", empty state "No Commands Available") | Runs the selected command with T as the target |
| Invite to Server | `invite-to-server` | submenu of guilds (a sent guild becomes disabled "Invite Sent") | `modal`, T ≠ me, T not a bot, T not blocked/ignored, and ≥1 other guild where I can invite | Creates an invite (max_uses 1, unique; vanity if no CREATE_INSTANT_INVITE) and sends it to T's DM |
| Add Friend / Accept Friend Request / Upgrade to Friend; Remove Friend | `add-friend` / `remove-friend` | item / item | T not a bot, T ≠ me. If friends → "Remove Friend" with confirm "Remove '{name}'". Else "Accept Friend Request" (pending incoming), "Upgrade to Friend" (game friends), or "Add Friend". Disabled if T is blocked; after sending: "Friend Request Sent" (disabled) | addRelationship / removeFriend |
| Ignore / Unignore | `ignore` | item | `modal`, T ≠ me, T not blocked | Ignore opens a confirm modal; Unignore runs directly |
| Block / Unblock | `block` | **danger** (Block), default (Unblock) | `modal`, T ≠ me | Block opens a modal; Unblock runs directly and shows a toast |
| Roles (label: "Roles"/"Role"/"No Roles" by count) | `roles` | submenu | Not overlay, T is a guild member, and ≥1 visible row. **With MANAGE_ROLES:** each role is a checkbox (checked = T has it). A role at or above my highest is disabled; such a role is hidden if T doesn't have it. Some roles are excluded (helper `Oy`; probably @everyone/managed — unverified). **Without:** plain items for T's roles only | Toggle → PATCH member roles (add/remove one role) |
| Move to | `voice-move` | submenu of voice channels (child id = channel id) | T is in a voice channel in this guild and not a guest/lurker. Self: channels I can CONNECT to that aren't full. Other: needs MOVE_MEMBERS on T's current channel; lists channels where I have MOVE_MEMBERS and (I or T can CONNECT) and that aren't full. Excludes the current channel; hidden if empty | Self: selectVoiceChannel. Other: PATCH member `channel_id` |
| Pop Out Stream / Pop Out Video / Pop Out User | `call-tile-popout` | item | `call`, T is a call participant (not an activity tile) and not already popped out | Pops the tile out into a window |
| Show Non-Video Participants | `no-video-hide` | checkbox | `call` | Toggle hiding non-video participants |
| Show My Own Camera | `self-video-hide` | checkbox | `call`, T is me (or no target), video supported | Toggle self video (confirm alert unless dismissed) |
| Open in Mod View | `mod-view` | item | In a guild where I have any of ADMINISTRATOR, MANAGE_GUILD, BAN_MEMBERS, KICK_MEMBERS, MODERATE_MEMBERS, MANAGE_ROLES, MANAGE_NICKNAMES | Opens the Mod View side panel for T |
| Verify Member / Unverify Member | `verify` | item | T ≠ me, T has joinedAt, the guild has the GUILD_ONBOARDING_EVER_ENABLED feature, and (Guild-manage(MODERATE+KICK+BAN) or Guild-manage(MANAGE_GUILD) or Guild-manage(MANAGE_ROLES)) | Toggles the BYPASSES_VERIFICATION member flag |
| Server Mute | `voice-mute` | **danger** checkbox | T is in a voice channel and I can VIEW_CHANNEL it, T not a guest/lurker, MUTE_MEMBERS on this channel, and (not a stage or T not suppressed) | `setServerMute(guild, T, !muted)` |
| Server Deafen | `voice-deafen` | **danger** checkbox | Same, with DEAFEN_MEMBERS | `setServerDeaf` |
| Disconnect | `voice-disconnect` | **danger** | Same voice precondition, T ≠ me, MOVE_MEMBERS | Set T's voice channel to null |
| Timeout {user} / Remove Timeout From {user} | `timeout` / `removetimeout` | **danger** | T ≠ me, T not a guest/lurker, T not a non-user bot, T not the owner, T doesn't have ADMINISTRATOR, Guild-manage(MODERATE_MEMBERS). "Remove…" if T is currently timed out | Opens the timeout modal. **Ctrl/Cmd-click = instant 60 s timeout** with a toast |
| Kick {user} | `kick` | **danger** | T ≠ me, Guild-manage(KICK_MEMBERS), T not a non-user bot, T not provisional | Kick modal |
| Ban {user} | `ban` | **danger** | T ≠ me, Guild-manage(BAN_MEMBERS), T not a non-user bot, **T not a bot**, T not provisional | Ban modal |
| Copy User ID | `devmode-copy-id-{userId}` | item (ID icon) | Dev mode on, not overlay, clipboard available | Copy the ID |

### Voice channel participant (sidebar voice row → 107632 with `media:true`)
Compared with the chat menu, these appear: **User Volume** slider (G3), **Mute / Mute Soundboard / Disable Video** checkboxes (or self **Mute/Deafen**), **View Verification Code** (E2EE only), **Focus This Person** (full call view only), and **Ask to Stream** (non-stage). In stages, Move to Audience / Invite to Speak appear instead of Ask to Stream. **Server Mute / Server Deafen / Disconnect** (G8) and **Move to ▸** (G6) do *not* depend on `media`. They show in any context once T is in voice and I have the permission. `call` stays off for the sidebar row, so the G7 pop-out / video toggles do not appear.

---

## 2. DM user menu — navId `user-context` (385913, `DMUserContextMenu`)

There are two variants. When the current guild view is "Favorites" (`DZ()`), a minimal menu L renders. Otherwise the full menu V renders.

**V (normal)**. `bot` = T is a non-user bot. `sys` = the DM is the official system DM.

| # | Items |
|---|---|
| 1 | Mark As Read / Mark Unread (`mark-channel-read`/`mark-channel-unread`; Mark Unread is behind an experiment; Mark As Read is disabled if there's nothing unread). Hidden for `bot` unless `sys` |
| 2 | Pin / Unpin (`pin-dm`, private channels only) · (T ≠ me) Add to Favorites ▸ (`favorite-channel`; submenu of favorite categories when any exist; not overlay, only if eligible and not already favorited) |
| 3 | (!bot) Invite to Speak (stage only; normally empty here) |
| 4 | Open in New Tab (`open-in-new-tab`, only when tabs are enabled) · if !bot: (`modal`) Profile, (`modal`) Try This Name Style, Start a Call, (not a managed DM) Ring, (`modal`) Note, (`modal`) Friend Nickname, Watch Stream · (`sys`) Profile · (T ≠ me) **Close DM** (`close-dm`, 1:1 only → closePrivateChannel) |
| — | Groups 5–9 only if !bot: |
| 5 | (`media`) User Volume |
| 6 | (`modal`) activity items |
| 7 | Apps ▸ · (`media`) Change Video Background · (`media`) Mute / Mute Soundboard / Disable Video · (`modal`) View Verification Code · (`media`) Focus This Person · (`modal`) Invite to Server ▸ · Add/Remove Friend · Add Game Friend · Remove Game Friend (danger) · (`modal`) Ignore · (`modal`) Block |
| 8 | (`showMute`) **Mute @{name}** ▸ (`mute-channel`, submenu: For 15 Minutes / 1 Hour / 3 Hours / 8 Hours / 24 Hours / Until I turn it back on) or **Unmute @{name}** (`unmute-channel`, subtext = time remaining) |
| 9 | (`call`) Pop Out · Show Non-Video Participants · Show My Own Camera |
| — | (`sys`) Mute group again |
| 10 | (T ≠ me) Remove from Favorites (`favorite-channel`, **danger**, only if favorited) |
| 11 | Staff-only experimental items (`mark-as-message-request`, safety-warning tools); visible only to Discord staff, ignore |
| 12 | Copy User ID · (if `targetIsUser` is false) Copy Channel ID (`devmode-copy-id-{channelId}`) |

The "Message" and "Mention" items are absent in DMs. Add Game Friend / Remove Game Friend (`add-game-friend` / `remove-game-friend`) show only if T ≠ me, T not a bot, T has game relationships, and T is **not** already a friend. With one game the item has the game as its subtext; with several it is a submenu of games.

**L (Favorites view)**:
- G1: Mark read/unread (same bot rule as V).
- G2: (T ≠ me) **Move to… ▸** (`move-to`, with First in List / Last in List / First in Category / Last in Category / Category ▸), which reorders the DM inside Favorites.
- G3: (!bot) Friend Nickname.
- G4: (!bot) Start a Call, Ring (not managed), Watch Stream; (`sys`) Profile.
- G5: (`showMute`) Mute (shown for !bot, or for `sys`).
- G6: (T ≠ me) Remove from Favorites.
- G7: Staff tools.
- G8: Copy User ID, Copy Channel ID.

## 3. Group DM member menu — navId `user-context` (183245)

If T is a non-user bot, only Copy User ID shows. Otherwise:

| # | Items |
|---|---|
| 1 | Invite to Speak (stage only) |
| 2 | (`modal`) Profile · (`modal`) Try This Name Style · (`chat`) Mention · Message · Start a Call · (not managed) Ring · (`modal`) Note · (`modal`) Friend Nickname · Watch Stream |
| 3 | (`media`) User Volume |
| 4 | Only if T is a recipient of the group: (not managed) **Remove From Group** (`remove`, **danger**; I own the group, T isn't the owner → `removeRecipient`) · **Make Group Owner** (`make-dm-owner`, **danger**; I'm the owner and T ≠ me → confirm modal "Transfer Group Ownership" → `setDMOwner`) |
| 5 | (`modal`) activity items |
| 6 | (`call`) Pop Out · Show Non-Video Participants · Show My Own Camera |
| 7 | (`media`) Mute/Mute Soundboard/Disable Video · (`media`) View Verification Code · (`media`) Focus This Person · (`media`) Change Video Background · Apps ▸ · (`modal`) Invite to Server ▸ · Add/Remove Friend · Add Game Friend · Remove Game Friend · (`modal`) Ignore · (`modal`) Block |
| 8 | Copy User ID |

## 4. Generic / Friends-list menu — navId `user-context` (668569)

If T is a non-user bot, only Copy User ID shows. Otherwise:

| # | Items |
|---|---|
| 1 | Invite to Speak (stage only) |
| 2 | Profile · Message · Start a Call · Note · Friend Nickname · Watch Stream |
| 3 | (`media`) User Volume |
| 4 | activity items |
| 5 | (`media`) Mute/Mute Soundboard/Disable Video · Invite to Server ▸ · Add/Remove Friend · Add Game Friend · Remove Game Friend · Ignore · Block |
| 6 | Copy User ID |

There is no Mention, Ring, Roles, Nickname or moderation here. Item rules match §1.

---

## 5. Message context menu — navId `message` (761304)

The menu reads from the click target: selected text, the `<a href>`/`<img src>` under the cursor, and `data-type`/`data-id`/`data-name` (emoji/sticker). The "message-actions" overflow menu (§6) uses the same builder (`nP`) with `textSelection:""` and no target, so the selection, link, image and emoji items never show there.

| # | Group | Items |
|---|---|---|
| G1 | Selection | Copy (`copy`, shortcut ⌘C/Ctrl+C) — **desktop app only**, when text is selected |
| G2 | Selection | Search with Google (`search-google`) — desktop app only, text selected |
| G3 | Reactions | Quick-react row (`quickreact-*`, up to 4 frequent emoji, rendered as icons; only when no text is selected) · Add Reaction ▸ (`add-reaction`: 12 frequent emoji + separator + View More `other-reactions`) · View Reactions (`reactions`) |
| G4 | Primary | Edit Message · Reply · Forward · Create Thread |
| G5 | Secondary | Copy Text · Translate ▸ / Revert Translation · Pin/Unpin Message · Mark/Unmark as Official · Bookmark Message/Remove Bookmark + Remind Me ▸ · Apps ▸ · Resend message · Mark Unread · Configure · Copy Message Link · View Conversation (internal) · Speak Message / Stop Speaking Message · End Poll Now · View Interaction Info ▸ |
| G6 | Destructive | Remove Reactions ▸ · Remove All Reactions · Delete Message · Report Message / Report to Discord · (staff) Test Message Report · Report to mod |
| G7 | Item-specific | Favorite/Unfavorite Emoji/Sticker · Filter (N Words) ▸ (AutoMod) · Copy Image · Save Image · Mark As Not Sensitive · Save File (voice message) · Stats for Nerds |
| G8 | Link | Copy Link / Copy Image Link / Copy Media Link · Copy Email Address / Copy Phone Number · Send Text Message · Open Link / Open Image Link / Open Media Link |
| G9 | Dev | Message Details ▸ (emoji/sticker target) **or** Copy Message ID |

Order: G1–G6, then G7, G8, G9. A region-specific media-takedown regulation (`isMediaTakedownRegulation(TIDA)`) changes the order to G9, G7, G8, and drops G8 when Message Details is shown.

| Label | id | Kind | Shows when | Action |
|---|---|---|---|---|
| Copy | `copy` | item | Desktop app, selection non-empty | Native copy |
| Search with Google | `search-google` | item | Desktop app, selection non-empty | Opens google.com/search?q= |
| quick react / Add Reaction | `quickreact-{id}`, `add-reaction` | emoji row / submenu | Show-reactions setting on; I can react (guild: can chat + ADD_REACTIONS; or private non-system DM); channel active or unarchivable; not a media thread (Add Reaction); quick row also needs no selection and not MEDIA_MENTION. Shift-click keeps the menu open | PUT reaction / open the emoji picker |
| View Reactions | `reactions` | item | The message has reactions (polls: only if allowed) | Reactions modal |
| Edit Message | `edit` | item | Author = me, state SENT, user-type message, editable, not a voice message, not a poll, not a referral offer, not MEDIA_MENTION; channel active/unarchivable; not a locked thread without MANAGE_THREADS | Start inline edit |
| Reply | `reply` | item | (DM: not a system DM) or (SEND_MESSAGES + READ_MESSAGE_HISTORY); replyable type; SENT; not ephemeral; I'm not timed out; not an archived thread unless I can unarchive; replies not disabled by the surrounding context | Start reply |
| Forward | `forward` | item | Not failed; forwardable type; no poll/activity/call/theme; none of the flags CROSSPOSTED, IS_CROSSPOST, HAS_SNAPSHOT, HAS_THREAD, voice message, components-v2, URGENT, SUPPRESS_*, GUILD_OFFICIAL…; READ_MESSAGE_HISTORY (or private); guild lacks FORWARDING_DISABLED; channel not gated; not a mod-report channel | Opens the forward modal |
| Create Thread | `thread` | item | Not overlay; the channel type supports threads; CREATE_PUBLIC_THREADS + READ_MESSAGE_HISTORY (forum: SEND_MESSAGES); the message has no thread; user-type message | Create a thread from the message |
| Copy Text | `copy-text` | item | Clipboard available; the message has text (or components-v2 text) | Copy content |
| Translate ▸ / Translating… / Revert Translation | `translate` (children `translate-{locale}`) / `revert-translation` | submenu / item | Content non-empty and translation is enabled (feature gate) | POST AI_TRANSLATE; dispatches a local MESSAGE_UPDATE |
| Pin Message / Unpin Message | `pin` / `unpin` | item | Not failed; not a system DM; user-type message; (PIN_MESSAGES + READ_MESSAGE_HISTORY) or private; channel active; not guild voice/stage/media thread | Confirm dialog; **Shift-click = no confirm** |
| Mark as Official / Unmark as Official | `guild-official-set`/`-unset` | item | VERIFIED guild + experiment, MANAGE_OFFICIAL_MESSAGES, user-type message, text-like channel | PATCH message guild_official |
| Bookmark Message / Remove Bookmark | `bookmark` | item | Hidden when the message is a reminder with a due date. The whole entry is hidden if nothing is saved, it's not a DM, and I lack READ_MESSAGE_HISTORY | Save / unsave for later |
| Create Reminder / Edit Reminder ▸ | `reminder` | submenu: "in 30m", "in 1h", "in 4h", "Today/Tomorrow at 9:00am", "Monday at 9:00am", "Custom…"; when a reminder exists, also "Mark Complete" | same gate | Saved-message with dueAt |
| Apps | `apps` | submenu | MESSAGE-type commands | Run the command on the message |
| Resend message | `resend` | item | state SEND_FAILED | Re-send |
| Mark Unread | `mark-unread` | item | Channel active/unarchivable | Ack the channel before this message |
| Configure | `configure` | item | Guild USER_JOIN (welcome) message + MANAGE_GUILD | Opens welcome settings |
| Copy Message Link | `copy-link` | item | Clipboard available | Copies `https://discord.com/channels/{g}/{c}/{m}` |
| Speak Message / Stop Speaking Message | `tts` | item | Content non-empty | Local TTS |
| End Poll Now | `end-poll-early` | item | Poll not expired and author = me | endPollEarly |
| View Interaction Info ▸ | `view-interaction-info` (children `integration-owner`, `interaction-user`) | submenu | The message is from an app interaction | Shows the installer guild/user and the invoking user |
| Remove Reactions ▸ | `remove-emoji-reactions` (children per emoji) | **danger** submenu | MANAGE_MESSAGES, channel active, message has reactions | Confirm dialog, then delete all of that emoji (Shift = no confirm) |
| Remove All Reactions | `remove-reactions` | **danger** | Same | Confirm, then delete all reactions (Shift = no confirm) |
| Delete Message | `delete` | **danger** | Not SENDING; (MANAGE_MESSAGES or own deletable); AutoMod action messages need MANAGE_MESSAGES; type deletable; channel active; mod-report channel exceptions | Confirm dialog; **Shift = no confirm**; a failed message deletes locally |
| Report Message / Report to Discord | `report` | **danger** | `canReport`; reportable type; author ≠ me and not a system user. Label "Report to Discord" (Clyde icon) under an experiment | Opens the report flow |
| Report to mod (labels from a separate string table, not resolved) | `report-to-mod` | **danger**, disabled once reported, "new" badge | Guild has the mod-report feature configured; author ≠ me | Opens report-to-mods |
| Favorite/Unfavorite Emoji, Favorite/Unfavorite Sticker | `favorite`/`unfavorite` | item | Right-clicked on a custom emoji or sticker (`data-type`) | Toggle the favorite |
| Filter (N Words) ▸ | `guild-automod-add-selection` | submenu of keyword rules (radio) + "Add first rule"/"Add another rule" | Text selected, I can manage AutoMod in the guild, ≤1000 words | Add the selection to a keyword rule |
| Copy Image / Save Image | `copy-image`/`save-image` | item | Desktop app; the target is an image; not hidden media | Native copy/save |
| Mark As Not Sensitive | `report-image-false-positive` | item | Target media was flagged by the explicit-content filter | Opens the false-positive modal |
| Save File | `save-voice-message-audio` | item | Voice message with an attachment | Download |
| Stats for Nerds | `video-stats-for-nerds` | checkbox | Target media is video | Toggle the overlay |
| Copy … Link / Open … Link / Copy Email/Phone / Send Text Message | `copy-native-link`, `open-native-link`, `copy-native-contact`, `native-send-sms` | item | Desktop app; there is an href or img src; valid URL; custom-emoji image URLs need dev mode | Copy / open with a link-safety confirm |
| Copy Message ID | `devmode-copy-id-{msgId}` | item | Dev mode + clipboard. **Shift-click copies `{channelId}-{messageId}`** | Copy |
| Message Details ▸ | `message-details` (Copy Message ID, Copy Emoji/Sticker ID, Copy/Open Emoji/Sticker Image Link) | submenu | Dev mode + clipboard + regulation flag + target is an emoji/sticker | Copy / open |

## 6. Message hover bar + "More" (navId `message-actions`, built in 812266)

Hover-bar buttons, left → right. "Expanded" = **Shift held** while hovering, not in keyboard mode, and no picker or More popout open.

1. Only when expanded: Copy Message ID (dev mode + clipboard) · Copy Link (clipboard) · Configure (welcome msg + MANAGE_GUILD) · Mark Unread (channel active) · Bookmark Message / Remove Bookmark (hidden when the message is a reminder) · Pin/Unpin Message (same rule as Pin) · Create Thread (only when Forward is also available) · Reply (only on my own editable messages)
2. If I can react (show-reactions setting on and reaction creation not disabled): when **not** expanded, 3 quick-react emoji buttons (top frequent usable emoji, padded with 💯 😆 💖; tooltip "Click to react"/"Click to remove") + divider. Then **Add Reaction** (opens the emoji picker).
3. Reply (when not my own editable message) · Edit (my own message, editable, not a system DM, channel active, not a locked thread) · Forward · Create Thread (when Forward is unavailable) · View Thread (cannot create, and the message has a thread I can view) · Mark/Unmark as Official · Publish / Published (disabled) (announcement channel in a NEWS guild, SEND_MESSAGES, my message or MANAGE_MESSAGES, publishable)
4. Delete (**danger**, only when expanded and I can delete) — otherwise **More** (…) opens the `message-actions` menu (= §5 without the selection, link, image and emoji groups; `canReport` passed through).

Failed (SEND_FAILED) message: the bar shows only **Retry** (when retryable) and **Delete**.

Keyboard shortcuts on a focused message, from the same module: Backspace = delete · Ctrl/⌘+C = copy text · E = edit · P = pin (Shift skips the confirm) · + = add reaction · R = reply · F = forward · S = speak · T = create/open thread · Alt+Enter = mark unread · Esc = cancel edit.

## Unknowns
- Contents of 446828 (menu for an unresolved user) and of the video-call-tile caller that sets `showChannelCallItems`: not loaded.
- The role-exclusion helper `Oy` in Roles: probably @everyone and managed roles. Not verified.
- English labels for "Report to mod" (separate string table `307805`), and the Apps submenu enable rules (module 700210, partly read).
- The Friends list → 668569 mapping (module 414711) is believed from call shape, not confirmed by a display name.
