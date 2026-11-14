# Discord channel and server menus (code-derived spec)

Source: Discord web client webpack modules, read 2026-11-14. Module ids are given as `#NNNNNN` so each item can be re-checked. Lazily loaded menus were read by fetching their chunk text and extracting the module source as text; nothing was executed. English labels were resolved with the client's own intl module.

## Conventions

- **Group**: a `MenuGroup`. Consecutive non-empty groups are drawn with a separator between them. An empty group draws nothing.
- **Kind**: `item` (plain), `check` (checkbox, has `checked`), `radio` (radio, has `group` + `checked`), `sub` (item with `children` = submenu; the parent is clickable too when it has an `action`), `control` (slider or meter row), `danger` (`color:"danger"`, red), `brand` (`color:"brand"`, accent colored).
- A **hook** returns one item or `null`. A menu lists hooks per group. Hooks are defined once in §1, and the menus in §2–§5 refer to them by name.
- **Favorites mode** means the user is viewing the Favorites pseudo-server (guild id `"373"` or the favorites constant, `#93055.DZ`). Most channel menus have a separate, shorter layout in that mode.
- **Developer Mode** means the user setting `developerMode` is on. Every "Copy … ID" item needs Developer Mode and clipboard support, and is hidden in the overlay (`#50268`).
- `__OVERLAY__` means the in-game overlay build. Many items are hidden there. A native client can ignore it.
- Mute durations (`#995102.g`), used by every Mute submenu: `For 15 Minutes` (900), `For 1 Hour` (3600), `For 3 Hours` (10800), `For 8 Hours` (28800), `For 24 Hours` (86400), `Until I turn it back on` (-1). Each child's item id is the number as a string. Clicking a child sends `muted:true, mute_config:{selected_time_window:N, end_time: now+N s, or null when N = -1}`.
- Notification enums: `message_notifications` ALL_MESSAGES=0, ONLY_MENTIONS=1, NO_MESSAGES=2, NULL(inherit)=3.
- Endpoints (the route constants were read from the client; the link from each action to its route is inferred, not traced): user guild settings `PATCH /users/@me/guilds/{guild_id}/settings`, with `channel_overrides` for channel-level settings and `@me` as the guild id for DMs. Thread settings use `/channels/{thread_id}/thread-members/@me/settings`. Channel delete uses `/channels/{id}`. Leave server uses `/users/@me/guilds/{id}`. Acks use `/channels/{id}/messages/ack` and `/read-states/ack-bulk`.

---

## 1. Shared item hooks

| Hook | Label (sublabel) | Item id | Kind | Shows when | Action |
|---|---|---|---|---|---|
| **MarkRead** `#511904` | `Mark As Read` | `mark-channel-read` | item, disabled when nothing is unread | Always rendered. Disabled unless the channel has unreads or mentions. For a forum or media channel it means the unread post count is above 0. For a forum post it means the post is unread. For a category it means any child channel, or any joined and unmuted active thread under it, is unread. | Ack the channel (`CHANNEL_CONTEXT_MENU`, manual ack) |
| | `Mark Unread` | `mark-channel-unread` | item | Replaces Mark As Read when the experiment `2026-08-mark-channel-unread` is on, the channel is NOT unread, it can be unread and has a last message, and it is not a category | Mark the channel unread at its last message id |
| **PinChannel** `#704543` | `Pin Channel to Top` / `Unpin Channel` | `pin-channel` | item | Not a thread and not a category. The label is chosen by whether the channel is already pinned (user-guild-settings "favorite" flag). | Toggle channel favorite flag (`FAVORITED`=2048 in channel override flags) |
| **ChannelListOptIn** `#969128` | `Add to Channel List` / `Remove from Channel List` | `opt-into-channel` (category: `opt-into-category`) | item | The server has opt-in ("Show All Channels" off) AND (server has COMMUNITY or user is staff), and the channel is not a thread. If the parent category is opted in, it shows instead: | Toggle the channel's opt-in flag |
| | `Unfollow Channel Category` | `opt-out-category` | item | (same, parent category opted in) | Opt the parent category out |
| **MuteChannel** `#995102` | Unmuted: `Mute Channel` · category `Mute Category` · group DM `Mute Conversation` · DM `Mute @{name}` (or `Mute Conversation` when the name is off) | `mute-channel` | sub | Always shown (when not muted) | Clicking the parent mutes indefinitely (`muted:true`). Children are the 6 durations. Muting a category also collapses it. |
| | Muted: `Unmute Channel` / `Unmute Category` / `Unmute Conversation` / `Unmute @{name}` (sublabel `Muted until {M/D h:mm}` when it has an end time) | `unmute-channel` | item | Channel is muted | `muted:false` |
| **NotificationSettings** `#288104` | `Notification Settings` (sublabel: the current effective value) | `channel-notifications` | sub | Not shown for GUILD_VOICE. Shown for every other channel type. | Opens the radio list below |
| ↳ children (text-like) | `Use Category Default` (when there is a parent, else `Use Server Default`; its sublabel is the inherited value) · `All Messages` · [`@mentions + Unreads`]* · `Only @mentions` · `Nothing` | `{setting}-{allUnreads}` e.g. `3-false`, `1-true` | radio, group `channel-notifications` | *`@mentions + Unreads` appears only when the experiment `ChannelNotificationItems` is on and the type is TEXT, CATEGORY, FORUM, ANNOUNCEMENT or APP | Sets `message_notifications`. With the experiment on, it also sets the flags UNREADS_ALL_MESSAGES(1024) or UNREADS_ONLY_MENTIONS(512), and clears them for the default option. |
| ↳ children (stage) | `Use Category/Server Default` · `All Live Stages` (=ONLY_MENTIONS) · `Nothing` | same | radio | GUILD_STAGE_VOICE | same |
| ↳ forum extra (first) | `New Posts Created` + separator | `new-forum-threads-created` | check | The channel is a forum or media channel | Toggle new-forum-thread notifications |
| **FavoriteAdd** `#438653.z` | `Add to Favorites` (star icon) | `favorite-channel` | item, or sub when favorites categories exist | Not a category, the server is joined (not lurking), the channel is not already a favorite | Add to Favorites. Submenu: group 1 `Favorites` (root) / group 2 one item per favorites category, labelled with the category name |
| **FavoriteRemove** `#438653.C` | `Remove from Favorites` | `favorite-channel` | danger | The channel is a favorite | Remove. For a category, a confirm modal opens first. |
| **CopyLink** `#886393` | `Copy Link` | `channel-copy-link` | item | Always (when a channel exists) | Copy the channel URL. For a forum post it also tracks a share. |
| **CopyTopic** (inline in `#834881`) | `Copy Channel Topic` | `copy-channel-topic` | item | Only in the channel-header menu with `includeTopic`, and the clipboard is supported | Copy `channel.topic` |
| **EditChannel** `#317910` | `Edit Channel` / `Edit Category` | `edit-channel` | item | The user can VIEW (access) the channel AND has any of MANAGE_CHANNELS, MANAGE_ROLES, MANAGE_WEBHOOKS, or can unlink the channel | Open channel settings |
| **Invite** `#531335` | `Invite to Channel` / `Invite to Voice` (GUILD_VOICE) / `Invite to Server` (server header or server context) | `invite-people` | brand (accent colour; person-plus icon in the server header) | A guild is present (always shown). | If the user can create invites (CREATE_INSTANT_INVITE on the channel or server, OR a vanity URL, OR a stage invite code), opens the Invite modal. Otherwise opens the "Invite permission required" info modal. |
| **Duplicate** `#533957` | `Duplicate Channel` | `clone-channel` | item | MANAGE_CHANNELS on the channel | Create Channel modal prefilled (`cloneChannelId`) |
| **CreateText / CreateVoice** `#477190` | `Create Text Channel` / `Create Voice Channel` | `create-text-channel` / `create-voice-channel` | item | MANAGE_CHANNELS on the channel. Returns null for any other type, so a category's own "create" hook renders nothing. | Create Channel modal (type, category = parent) |
| **Delete** `#307623` | `Delete Channel` / `Delete Category` / `Delete Thread` / forum post: `Delete Post` (or `Delete Message` for an author without manage rights whose post has replies) | `delete-channel` | danger | The user can view the channel and one of these holds: MANAGE_CHANNELS (MANAGE_THREADS for threads); forum post owner with 0 messages; forum post owner with replies (then it deletes only the starter message). Moderator-report channels are only deletable when the first message is gone. | Confirm modal, then `DELETE /channels/{id}` (or delete the starter message) |
| **CopyChannelId** `#50268` | `Copy Channel ID` / `Copy Category ID` / `Copy Thread ID` / `Copy Server ID` / `Copy User ID` | `devmode-copy-id-{id}` | item (ID icon) | Developer Mode on, the clipboard is supported, not the overlay | Copy the id (shift-click copies the alternate id if one is given) |
| **MoveTo** `#906964` | `Move to...` | `move-to` | sub | Not the overlay. In normal mode it is hidden for threads. The channel is not blocked. At least one destination is enabled OR a first/last placement exists. Destinations require MANAGE_CHANNELS (in Favorites mode, always allowed). | Submenu: group 1 `Category` (sub `move-to-category`: group with `No Category`-type root entry, then group with one item per category, labelled with the category name, disabled for the current parent or without permission); group 2 `First in Category`/`Last in Category` (`First in List`/`Last in List` for categories or uncategorized) ids `move-to-first`/`move-to-last`, disabled when already there |
| **GoToServer** `#6351` | `Go to Server` | `go-to-original-guild` | item | Favorites mode (or a related flag `#394953.lI`) and not the overlay | Navigate to the channel in its real server |
| **SetNickname** `#250737` | `Change Nickname` / `Rename Category` | `set-channel-nickname` | item | Favorites mode only | Modal to set a local nickname for the channel or category |
| **OpenInTab** `#687340` | `Open in New Tab` | `open-in-new-tab` | item | The tabs feature is enabled | Opens the channel in a new app tab |
| **ActivityItems** `#468389` | `Invite to Join` (sub: game/app name), `Invite to Listen Along`, `Join`/`Joining`/`Joined`, `Ask to Join`, Spotify play/`Listen Along`, `Join Activity`/`You can't join yourself` | `invite-to-join`, `invite-to-join-embedded`, `invite-to-listen`, `join`, `ask-to-join`, `spotify-play-*`, `spotify-sync-*`, `embedded-activity-join-*` | items (a separator follows the self-invite items) | Channel is private, OR the user has SEND_MESSAGES, OR it is GUILD_VOICE. Each item depends on the current user's rich-presence / embedded activity. | Sends an activity invite, or joins |
| **ReportApp** `#586486` | `Report App` (flag icon) | `report-app` | danger | GUILD_APP channel with an application id | Report flow for the app |
| **Conjure** `#130332` | `Make a Copy` / `Edit App` / `Settings` / `Show background overlay` (check) | `conjure-remix` / `conjure-edit` / `conjure-settings` / `conjure-show-overlay` | items + check | Only for the "app channel" (Conjure) feature (`#246338.w$`). Each item needs its own sub-condition (remixable, editable app, has settings, frame overlay). | App-builder actions |

---

## 2. Channel menus (channel list, right-click)

How dispatch works (`#616740`, and the quick switcher `#910839`): GUILD_TEXT, ANNOUNCEMENT, FORUM, MEDIA and APP open the **Text** menu. VOICE and STAGE open the **Voice** menu. Threads open the **Thread** menu. CATEGORY opens the **Category** menu. DIRECTORY and STORE open their own small menus. Moderator-report channels open the **Mod report** menu. GROUP_DM opens the **Group DM** menu. DM opens the **DM** menu.

### 2.1 Text channel `#823255`, navId `channel-context`, aria "Channel Actions"

Normal mode, groups in order:
1. MarkRead
2. FavoriteAdd
3. Conjure: Make a Copy, Edit App, Settings, Show background overlay
4. Invite · ActivityItems · PinChannel · ChannelListOptIn · CopyLink
5. `Open Chat` (`open-chat`, item): GUILD_APP only. It opens the app channel's chat.
6. MuteChannel · NotificationSettings
7. `Re-hide Spoiler Channel` (`clear-spoiler-agree`, item): the channel is a spoiler channel and the user has already agreed to view it. It clears the agreement.
8. MoveTo
9. EditChannel · Duplicate · CreateText · Delete (all admin; each is hidden without permission)
10. OpenInTab
11. ReportApp
12. FavoriteRemove
13. CopyChannelId

Favorites mode, groups:
1. MarkRead
2. Conjure items
3. GoToServer · MoveTo · SetNickname
4. [thread only: `Join Thread`/`Follow Post`] · [thread only: archive item, see §2.3] · CopyLink
5. `Open Chat` (APP)
6. MuteChannel (threads use the thread mute, see §2.3) · NotificationSettings
7. OpenInTab
8. ReportApp
9. [thread only: `Leave Thread`] · FavoriteRemove
10. CopyChannelId

### 2.2 Voice / Stage channel `#428631`, navId `channel-context`

Normal mode, groups in order:
1. `End Event` (`End Event`, danger) when the channel has an active scheduled event the user can manage that is not EXTERNAL. It confirms with "Are you sure you want to end the event?", buttons Cancel / "Yes, end the event". **Otherwise** `End Stage` (`end-stage`, danger) when it is a stage with a live instance and the user has stage-moderator permissions (`#233993.QY`, exact set unknown). It confirms with "Do you want to end the Stage?" and then ends the stage instance.
2. MarkRead
3. FavoriteAdd
4. Invite · ActivityItems · PinChannel · ChannelListOptIn · OpenInTab · CopyLink
5. "voice-actions":
   - `Join Without Entrance Sound` (`join-muted-custom-join-sound`): the channel is vocal, the user has CONNECT, the user has a custom join sound for this server, and the user is not already in this channel. It suppresses the join sound, then joins.
   - `Open Chat` (`open-chat`): the user has CONNECT and the channel is vocal. It opens the voice text chat.
   - `Clear Channel Status` (`clear-status`): GUILD_VOICE, the user has MANAGE_CHANNELS, the user cannot set a status themselves (`#345640.b` false), and a status exists. It sets the status to an empty string. Otherwise `Set Channel Status` (`set-status`) when the user can set a status. It opens the Set Status modal.
   - `Hide Names` (`hide-voice-names`, check): checked when the channel's voice-user list is collapsed. It toggles a local collapse. Hidden in the overlay.
   - Console transfer: `Transfer Voice to This Device` (`handoff`) when another device of the user is in this channel. Otherwise, one item per linked console, `Join on Xbox` / `Join on PlayStation` (`Transfer to …` when already connected), id `transfer-{type}-{id}`. Hidden for stage. Disabled when the user lacks CONNECT or the channel is full.
   - `Report Stage` (`report-stage`, danger, flag icon): a stage that is live.
6. MuteChannel · NotificationSettings (NotificationSettings is hidden for GUILD_VOICE, see §1)
7. MoveTo
8. EditChannel · Duplicate · CreateVoice · Delete
9. FavoriteRemove
10. CopyChannelId

Favorites mode: 1 End Event/End Stage · 2 MarkRead · 3 GoToServer, MoveTo, SetNickname · 4 ActivityItems, CopyLink · 5 Join Without Entrance Sound, Open Chat, Clear/Set Status, Report Stage (no Hide Names) · 6 Mute, Notifications · 7 FavoriteRemove · 8 CopyChannelId.

### 2.3 Thread / forum post `#949488`, navId `thread-context`, aria "Thread Actions"

The labels switch to the "Post" wording when `isForumPost()`.

Normal mode, groups:
1. MarkRead
2. FavoriteAdd
3. "thread-actions":
   - OpenInTab
   - `Join Thread` / `Follow Post` (`join-thread`): the user has not joined and is not lurking the server. Calls `joinThread`.
   - `Start Call` / `Join Call` (`join-thread-voice`) plus `Open Chat` (`open-chat`): voice-in-threads only (a vocal thread with the experiment on, CONNECT, and the user not already connected). "Join Call" is used when someone is already in the call. Open Chat shows only when there are voice states and the chat is closed.
   - `Close Thread` / `Close Post` (`archive-thread`) or `Open Thread` / `Open Post` (`unarchive-thread`): the user can manage the thread (MANAGE_THREADS on the parent, or is the owner and the thread is not locked). For archived threads it uses the send-messages-in-threads check instead. Not a moderator-report forum post.
   - `Lock Thread` / `Lock Post` (`lock-thread`) or `Unlock Thread` / `Unlock post` (`unlock-thread`): MANAGE_THREADS, not a moderator-report channel.
   - `Edit Thread` / `Edit Post` (`edit-thread`): the user can manage the thread, can send in it, the thread is not locked (unless the user has MANAGE_THREADS), not a moderator-report channel. Opens thread settings.
   - `Edit Tags` (`edit-tags`, sub): forum post, the user can manage it, and the parent has tags. Children are checkboxes, id = tag id, label = tag name with its emoji. Unchecked tags are disabled once 5 are selected.
   - CopyLink
4. "notifications":
   - Thread mute `#57907`: when not joined, `Mute Thread` (`mute-disabled`, disabled). When joined and unmuted, `Mute Thread` / `Mute Post` (`mute-channel`, sub with the 6 durations). When muted, `Unmute Thread` / `Unmute Post` (`unmute-channel`, sublabel "Muted until …").
   - `Notification Settings` (`thread-notifications`, sub): when joined, radios in group `thread-notifications`: `All Messages`(flag 2), `Only @mentions`(4), `Nothing`(8). Item id = the flag. When not joined, `notifications-disabled` (disabled).
5. "admin-actions":
   - `Anyone can invite` (`toggle-thread-invitable`, check): PRIVATE_THREAD and MANAGE_THREADS.
   - `Pin Post` / `Unpin Post` (`pin-thread` / `unpin-thread`): parent is a forum or media channel and MANAGE_THREADS. If another post is already pinned, a confirm ("Pin this post?") replaces it.
   - `Resolve flag` (`resolve-report`): moderator-report post, not archived, the first message is loaded.
   - `Summarize Thread` (`summarize-thread`, sub `For Everyone` / `For Me`): the summaries feature is on (`#147087.b`). While running it shows `Summarizing Thread...`, disabled.
   - Delete (`Delete Thread` / `Delete Post`)
6. `Leave Thread` / `Unfollow Post` (`leave-thread`, plain, not red): joined and not lurking. · FavoriteRemove
7. CopyChannelId (`Copy Thread ID`)

Favorites mode: 1 MarkRead · 2 FavoriteAdd · 3 Join Thread, Start/Join Call, Close/Open Thread, CopyLink · 4 thread mute, thread notifications · 5 Summarize Thread · 6 Leave Thread, FavoriteRemove · 7 Copy Thread ID.

### 2.4 Category `#914907`, navId `channel-context`

Normal mode, groups:
1. MarkRead (key `mark-as-read`)
2. ChannelListOptIn (category form) · `Collapse Category` (`collapse-category`, check, checked when collapsed; toggles collapse) · `Collapse All Categories` (`collapse-all-categories`, item; hidden when the server has no categories or all are already collapsed)
3. MuteChannel (`Mute Category`) · NotificationSettings
4. MoveTo
5. EditChannel (`Edit Category`) · CreateText/Voice (renders nothing for a category) · Delete (`Delete Category`). SetNickname is also in this group, but it only renders in Favorites mode.
6. CopyChannelId (`Copy Category ID`)

Favorites mode (a favorites category): 1 MarkRead · 2 `Add to Category` (`add-channel-to-category`, plus icon; opens the add-to-favorites picker with this parent) · `Rename Category` · 3 MoveTo · 4 `Remove Category` (`delete-channel`, danger, sublabel "Channels will be uncategorized"; confirm modal, then removes the favorites category) · 5 `Copy Category ID`.

### 2.5 Small channel menus
- **Directory channel** `#994058` (`channel-context`): EditChannel · CopyChannelId.
- **Store channel** `#313140` (`channel-context`, legacy): EditChannel · Invite · Delete · CopyChannelId.
- **Moderator-report channel** `#907647` (navId `report-to-mod-channel-context`): MarkRead · FavoriteAdd · CopyLink · MuteChannel + NotificationSettings · EditChannel · FavoriteRemove · CopyChannelId.

### 2.6 Channel header menus (not the list)
- **Channel title menu** `#834881` (`channel-context`). Opened by right-clicking the channel name in the header, in the voice chat panel, or in the app-channel sidebar. Groups:
  1. PinChannel · ChannelListOptIn
  2. MuteChannel
  3. FavoriteAdd
  4. CopyTopic (when `includeTopic`; the chat header passes it) else NotificationSettings · EditChannel
  5. Invite · CopyLink · Duplicate · CreateText · CreateVoice · Delete
  6. FavoriteRemove
  7. CopyChannelId
- **Header bell popout** `#187360` → `#461678` (navId `channel-context`, aria "Channel Actions"). The bell icon shows BellSlash when the channel is muted or its resolved notifications are not ALL_MESSAGES. Tooltip key `h850Ss`, English not resolved. Shift-click toggles mute without opening the popout. Popout groups:
  1. MuteChannel
  2. (Mute Server, only if `includeGuildMute`; the header does not pass it)
  3. The NotificationSettings radios **inline**, not in a submenu, including the forum `New Posts Created` checkbox.

### 2.7 Group DM `#4027`, navId `gdm-context`

Not shown for managed (app-owned) group DMs. Normal mode groups:
1. MarkRead
2. OpenInTab · `Pin` / `Unpin` (`pin-dm`, pin icon; toggles DM pin, private channels only) · FavoriteAdd
3. `Invites` (`instant-invites`; owner only; opens the group invites modal) · `Edit Group` (`edit-gdm`)
4. ActivityItems
5. MuteChannel (`Mute Conversation`)
6. FavoriteRemove · `Leave Group` (`leave-channel`, danger). It confirms with `Leave '{name}'` and "won't be able to rejoin unless re-invited". Managed groups use the wording `Leave {name} Group`. Then it closes the private channel.
7. CopyChannelId

Favorites mode: 1 MarkRead · 2 MoveTo, SetNickname · 3 Invites, Edit Group · 4 ActivityItems · 5 Mute · 6 FavoriteRemove · 7 CopyChannelId.

### 2.8 DM `#385913` (navId `user-context`, aria "User Settings Actions")

This menu is the user context menu with the DM channel attached. The user-specific items belong to the user-menu spec (`docs/discord-menus-user-message.md`). Channel-related items and group order:
1. MarkRead (hidden for non-user bots, except the system DM)
2. `Pin`/`Unpin` (`pin-dm`) · FavoriteAdd (not for self)
3. user profile item (user spec)
4. OpenInTab · user items (profile, mention, call, note, …) · `Close DM` (`close-dm`, not for self; one-to-one DMs only; closes the private channel)
5–7. user items (non-bots)
8. MuteChannel (`Mute @{name}`) when `showMute`
9. (optional call items)
10. FavoriteRemove (not for self)
11. staff/experimental message-request items (`#565289`, staff only)
12. `Copy User ID` · `Copy Channel ID` (unless `targetIsUser`)

From the DM list the menu is opened with `showModalItems:false`, which hides the modal-opening user items.

---

## 3. Server menus

### 3.1 Server icon in the rail `#871638`, navId `guild-context`, aria "Server Actions"

Regular server, groups:
1. `Mark As Read` (`mark-guild-read`; disabled unless the server has unread channels, ignoring muted ones; bulk-acks the server)
2. Invite (`Invite to Server`, brand)
3. Server mute `#507238` · Notification Settings · `Hide Muted Channels` · `Show All Channels`:
   - Unmuted: `Mute Server` (`mute-guild`, sub; the parent mutes indefinitely; the 6 durations follow). Muted: `Unmute Server` (`unmute-guild`, sublabel "Muted until …").
   - `Notification Settings` (`guild-notifications`, sub, sublabel = current level; clicking the parent opens the server Notification Settings modal; hidden in the overlay). Submenu groups:
     - (a) radios `guild-notifications`: `All Messages`, [`@mentions + Unreads`, only with experiment `GuildNotificationItems`], `Only @mentions`, `Nothing`. Ids `{setting}-{allUnreads}`.
     - (b) checks: `Suppress @everyone and @here` (`suppress-everyone`), `Suppress All Role @mentions` (`suppress-roles`), `Suppress Highlights` (`suppress-highlights`; checked when `notify_highlights` = DISABLED), `Mute New Events` (`mute-events`).
     - (c) check `Mobile Push Notifications` (`mobile-push`).
   - `Hide Muted Channels` (`hide-muted-channels`, check; toggles the user guild setting `hide_muted_channels`)
   - `Show All Channels` (`opt-in`, check; checked when opt-in is OFF; server is COMMUNITY or user is staff)
4. "settings":
   - `Server Settings` (`guild-settings`, sub). Shown when the user has any of MANAGE_GUILD, MANAGE_ROLES, BAN_MEMBERS, MANAGE_NICKNAMES, CREATE/MANAGE_GUILD_EXPRESSIONS, MANAGE_WEBHOOKS, VIEW_AUDIT_LOG. Hidden when `hideSettings` (quick switcher). The parent opens settings. Children are the server-settings sidebar sections the user may see, minus headers, dividers and Delete. The COMMUNITY section is labelled `Community Settings`. APP_DIRECTORY is `App Directory` (opens discovery, with a launch icon). Others use the section label. The full list is in a separate module and was not enumerated (unknown).
   - `Privacy Settings` (`privacy`; not in the overlay; opens the privacy modal)
   - `Edit Per-server Profile` (`change-nickname`; shown when the user may change their own nickname or manage nicknames, and is not a guest or lurker; `Edit Hub Profile` in hubs)
5. `Create Channel` (`create-channel`) + `Create Category` (`create-category`) when MANAGE_CHANNELS. In the Favorites pseudo-server only `Create Category` shows. · `Create Event` (`create-event`) when the user has ADMINISTRATOR or CREATE_EVENTS, or can create events in some voice channel.
6. `Move to...` (`move-to`, sub). Shown when the server is in the rail tree and folders exist or there are 2+ siblings. Group 1: `Folder` sub (`move-to-folder`: `No Folder` (`no-folder`, disabled when not in a folder) / one item per folder). Group 2: `First in Folder|List` (`move-to-first`), `Last in Folder|List` (`move-to-last`).
7. `Security Actions` / `Edit Security Actions` (`server-lockdown`, danger): COMMUNITY server with incident actions available for the user. Opens the security-actions modal. · `Leave Server` (`leave-guild`, danger): not the owner. Opens the leave confirm.
8. `[STAFF] Test Guild Report` (staff + dev flag only; ignore)
9. Copy server info `#810263`. Developer Mode and clipboard required. With experiment `#207560.lF` it is `Copy Server Info` (`copy-server-info`, sub): `Copy Server ID` (`copy-server-id`), `Copy Server Icon Image Link` (when the server has an icon), `Copy Server Banner Image Link` (when it has a banner). Otherwise a plain `Copy Server ID`.

Hub server (`HUB` feature): group 1 Invite · `Hub Privacy Settings` (`privacy`) · `Edit Hub Profile`. Group 2 `Leave Hub` (`leave-guild`, danger; not owner). Group 3 Copy Server ID.

### 3.2 Rail folder `#865815`, navId `guild-context`
1. `Mark Folder As Read` (`mark-folder-read`; disabled unless the folder is unread; bulk-acks all servers in it)
2. `Move to...` (`move-to`, sub: `First in List` / `Last in List`, disabled at the ends; only when the rail has 2+ roots)
3. `Folder Settings` (`folder-settings`; opens a modal for name and colour) · `Close All Folders` (`folder-collapse`; only when any folder is expanded)

### 3.3 Pending join-request server in the rail `#97187`, navId `join-request-guild-context`
- `Reapply` (`reapply`): application REJECTED. Resets the request and opens verification.
- `Discard Application` (status STARTED) or `Withdraw Application` (otherwise), id `withdraw`, danger. Confirm "Are you sure?" with Yes / Never Mind. Removes the join request.
- group: Copy Server ID

### 3.4 Server name dropdown `#808144`, navId `guild-header-popout` (variant fixed; every item has a leading icon)

- **Lurker or guest**: `Leave Server` (`leave-guild`, plain; leaves without confirm) · group: copy server info.
- **Favorites**: navId `favorites-header-popout`. Group `Thread Settings` (`favorites-thread-settings`) · group `Hide Favorites` (`hide-favorites`, danger) · group `Reset Favorites` (`reset-favorites`, danger, Developer Mode only).
- **Hub**: `Server Settings` (when the user can access settings) · Invite · `Privacy Settings` · `Edit Hub Profile` · group `Leave Hub` (danger, not owner).
- **Regular server** (hidden entirely for one special server id `gNP`). Groups:
  1. `Server Boost` (`premium-subscribe`; always) · `Use Server Theme` (`use-server-theme`, check; only when the server has a theme; toggles guild vs personal theme) · `Server Tag` (`guild-tag`; server has the GUILD_TAGS feature and a tag; opens the tag modal)
  2. Invite (`Invite to Server`, brand) · `Invite to Linked {noun}` (`invite-to-linked-game-organization`; linked-game-org feature; disabled with a sublabel when unavailable) · `Server Settings` (`settings`; when the user can access guild settings) · `Server Insights` (`insights`; VIEW_GUILD_ANALYTICS + COMMUNITY) · `Create Channel` + `Create Category` (MANAGE_CHANNELS) · `Create Event` (`create-event`, calendar icon) · `Active Threads` (`active-threads`; MANAGE_THREADS and the server has active threads) · `App Directory` (`application-directory`) · `Linked Roles` (`guild-connection-roles`; server has roles with connection requirements) · `Friends in Server` (`friends`; experiment `2026-01-friends-in-server`)
  3. `Show All Channels` (check, see 3.1) · `Notification Settings` (`notifications`; opens the modal, no submenu here) · `Privacy Settings` (`privacy`)
  4. `Edit Per-server Profile` (`change-nickname`, pencil) · `Hide Muted Channels` (check)
  5. `Security Actions` (danger, see 3.1) · `Report Raid` (`report-raid`, danger; COMMUNITY, the user has BAN, KICK, MODERATE_MEMBERS or MANAGE_GUILD, and there is no active raid incident) · `Leave Server` (`leave`, danger; not owner; confirm modal)
  6. copy server info (see 3.1 group 9)

### 3.5 Channel-list empty area `#455557`, navId `guild-context`
1. `Hide Muted Channels` (check)
2. `Create Channel` · `Create Category` (MANAGE_CHANNELS) · Invite (`Invite to Server`, using the default invitable channel)

Favorites header "+" `#329671` (navId `favorites-channel-list-context`): `Add to Favorites` (`add-to-favorites`; opens the picker or the limit upsell) · group `Create Category` (`create-favorites-category`).

### 3.6 `guild-entry-context` `#283354`

This menu is NOT the server-rail menu. It is the Student Hub directory entry menu. `Edit Settings` (`update-entry`; can edit, not hidden) · `Remove from Hub` (`remove-from-hub`, danger; confirm) · `Report Server` (`report-server-listing`, danger; not an entry admin) · group Copy Server ID.

---

## 4. Voice control chevrons (user panel `#166783`)

### 4.1 `audio-device-context` `#96020` (variant fixed, aria "Audio Device Actions")

Group layout: [Input devices sub, Input Profile sub, Output devices sub] / [Input Volume, Input Level meter, Output Volume] / [Push to Talk, Deafen, Spatial Audio, Voice Settings]. Each piece is behind a flag.

- **Mic chevron** flags: input devices, input profiles (only when any are enabled), input meter, input volume, PTT checkbox, settings.
  1. `Input Device` (`audioinput-devices`, sub, sublabel = current device; radios `audioinput-{deviceId}` in group `audioinput-devices`, label = device name, sublabel = sub-name, disabled devices greyed) · `Input Profile` (`input-profiles`, sub, sublabel = active; radios `Voice Isolation` / `Studio` / `Custom`, ids `VOICE_ISOLATION` / `STUDIO` / `CUSTOM`, group `input-profiles`; unset counts as Custom)
  2. `Input Volume` (`input`, slider control) · `Input Level` (`input-device-meter`, meter; only while in a voice channel)
  3. `Push to Talk` (`input-mode`, check; shown only when a PTT keybind exists; disabled when the profile is Studio) · `Voice Settings` (`voice-settings`, gear; opens Settings → Voice)
- **Headphone chevron** flags: output devices, output volume, spatial audio, settings.
  1. `Output Device` (`audiooutput-devices`, sub, radios `audiooutput-{deviceId}`)
  2. `Output Volume` (`output`, slider; max 200 on desktop, 100 on web)
  3. `Spatial Audio` (`spatial-audio`, check; hidden when the state is HIDDEN; disabled with sublabel "Spatial audio needs stereo headphones" or "Spatial audio failed to start") · `Voice Settings`
- **Right-click on the mic or headphone button** (`#385318`, same navId, `minimal`). Mic: Input Device sub, Input Profile sub, group [Input Volume, separator, `Push to Talk` check (desktop app only), Voice Settings]. Headphones: Output Device sub, group [Output Volume, separator, Voice Settings]. The non-minimal variant can also show an `Input Mode` group with radios `Voice Activity` / `Push to Talk` (`Push to Talk (Limited)` on web). Deafen check (`deafen`) shows when `renderDeafen`.
- The overlay passes all flags plus a `Deafen` check (`deafen`).

### 4.2 `video-device-context` `#28499` (aria "Camera")
1. Camera devices: radios `videoinput-{deviceId}`, group `videoinput-devices`. In minimal mode they sit in a `Camera` sub (`videoinput-devices`). Otherwise they form a labelled group "Camera".
2. `Preview Camera` (video off) / `Change Video Background` (video on), id `change-video-background`, eye or blur icon. Shows when video is supported, the user has a camera, and video is off (or a background-capable flag is set). Opens the camera preview modal. · `Video Settings` (`video-settings`, gear; minimal only; opens Settings → Camera).

### 4.3 `manage-streams` `#848362` (aria "Stop Streaming" when the user is streaming, else "Share Your Screen")
1. `Stop Watching — {username}` (`{ownerId}`, screen-x icon), one item per watched stream that is not the user's own. The group is omitted when the only stream is the selected participant.
2. Stream items `#600125` (unless `hideSelfOptions`):
   - Not streaming: `Share Your Screen` (`share-your-screen`).
   - Streaming, order: `Stream Quality` (`stream-settings`, sub, desktop app only; groups "Frame Rate" radios `15 FPS`/`30 FPS`/`60 FPS` ids `stream-settings-fps-{n}`, and "Resolution" radios `480p`/`720p`/`1080p`/`1440p`/`Source` ids `stream-settings-resolution-{n}`, Source = 0; options above the plan's limit open the upsell) · `Report Problem` (only with `showReportOption`) · `Share Audio` (`stream-settings-audio-enable`, check; soundshare supported for the source) · `Change Windows` / `Add Window` / `Add Application` (`change-windows`; app capture supported, not at max quality, not using the system picker) · `Stop Streaming` (`stop-streaming`, danger) · pass-controller item (`pass-controller`, game controller; label from feature).
3. `Report Problem` (`report-stream-problem`, danger, error icon): the user's own stream exists. Opens the stream report modal.

---

## 5. Not covered / unknown
- The labels of the Server Settings submenu children (generated by `#541223.generateSections`): not enumerated.
- The channel-header bell tooltip string `h850Ss`: not resolved.
- The `End Stage` permission constant `#233993.QY`: exact permission set not resolved.
- DM-menu user items: deliberately left to the user/message menu spec.
- Endpoints in Conventions: the routes were read; the mapping from action creator to route is inferred.
