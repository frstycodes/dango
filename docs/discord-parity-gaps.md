# Discord features Dango doesn't have yet, and how Discord does them

A build spec. Each gap gives what Dango has today, how Discord's desktop client does it (entry point, steps, options,
limits, states, keys), and where we can do better. Complaints with evidence live in [better.md](better.md), cited as
**B#n**. Audited 2026-11-19.

## Source tags

Every Discord fact carries one:

| Tag | Source |
|---|---|
| **[hc]** | Discord help-center article, read in full through support.discord.com's public article API |
| **[api]** | docs.discord.com developer docs, fetched |
| **[cl]** | Discord changelog or patch notes, fetched |
| **[snip]** | search-result snippet only |
| **[mem]** | memory of the client, not checked. Verify in the client before copying |

Dango's status comes from reading `src/` and grepping for each feature's endpoint, gateway event and field. Nothing was
run.

Mobbin wasn't used. It returns HTTP 403 to the in-app browser, and Claude in Chrome isn't connected in this session.

Order: most-noticed first within each section.

---

## 1. Messaging

### 1.1 Stickers: missing

**Dango:** nothing. `sticker_items` isn't parsed, there's no picker, and nothing is sent.

**Discord:**
- **Entry:** the sticker button in the composer. Ctrl/Cmd+S opens it [snip]. The picker has GIF, Stickers and Emoji tabs
  [mem].
- **Picker layout:** search on top; a left rail of packs (Favourites, Recent, each server, then Discord's standard
  packs); a grid of 4 stickers per row. Hover shows the name, and clicking sends at once [mem]. Sending on click is
  confirmed [snip].
- **Formats:** PNG (1), APNG (2), Lottie (3), GIF (4) [api]. Lottie only in verified or partnered servers [api].
- **Size and limits:** 320×320, at most 512 KiB, animated at most 5 s. Name 2–30 characters, description 2–100 [api]. A
  message carries up to 3 stickers in the API [mem].
- **Server slots:** 5, then 15, 30 and 60 at boost levels 1–3 [hc].
- **Rendering:** 160px in chat with no bubble. Animated ones animate per the Accessibility setting "Sticker Animation
  Control": Always animate, Animate on interaction, or never [hc]. Stickers from servers you're not in show a
  "Join to use" hover card [mem].
- **Permissions:** Use External Stickers (bit 37) [api].

**Do better:** sticker search also matches the related emoji and tags; show a "Can't use here" reason inline instead of
greying out; Lottie through a lightweight player, or a static frame plus a GIF fallback (gpui has no Lottie).

### 1.2 `:` emoji autocomplete: missing

**Dango:** the composer suggests `@` and `#` only (`main.rs` ~2108).

**Discord:**
- **Trigger:** `:` plus 2 characters [snip].
- **List:** up to 10 matches above the composer, each showing emoji, `:name:` and server [mem].
- **Keys:** ↑ and ↓ move, Tab or Enter inserts, Esc closes [mem].
- **Matching:** substring only. `:thisis` doesn't match `:this_is_the_way:`, a known complaint [snip].
- **Unavailable emoji:** emoji you can't use (another server, no Nitro) show with a lock [mem].

**Do better:** separator-insensitive fuzzy match; rank recent and frequent first; typing `:)`-style emoticons offers
the conversion inline instead of silently converting.

### 1.3 Custom status: shown, can't be set

**Dango:** reads `user_settings.custom_status` from READY and shows it. `set_status` always sends `activities: []`.

**Discord:**
- **Entry:** the profile icon at the bottom left, then **Add Status** [hc].
- **Fields:** emoji, then text, then **Clear After**, then a **Status** dropdown, then **Save** [hc]. Text is up to 128
  characters [mem].
- **Clear After:** "Today" (the default is one day [hc]), 4 hours, 1 hour, 30 minutes, "Don't clear" [mem].
  A known bug: "Don't Clear" reverts to "Today" [snip].
- **Emoji:** custom emoji from servers need Nitro [hc].
- **Wire format:** user settings `custom_status` (text, emoji, expiry) plus a type-4 presence activity [mem].

**Do better:** one dialog for status and custom status **(design/better.html › Profile and privacy)**; remember the
last Clear After choice; add a custom date and time. Never auto-set a status (**B#12**).

### 1.4 Forwarding: missing both ways

**Dango:** `message_snapshots` isn't parsed and there's no Forward action. The message menu header says it's left out.

**Discord:**
- **Entry:** the **Forward** button on the hover bar, top right of the message [hc]. The F key on a highlighted
  message [cl].
- **Flow:** **Forward To** opens, you pick up to **5** destinations, add an optional note, then **Send** [hc].
- **Rendering:** the message shows "Forwarded" and no original author [hc], with a link to the original's channel and
  time [mem].
- **Edits:** a forwarded copy doesn't follow later edits [hc].
- **Restrictions:** messages from monetized channels, and from age-restricted channels into unrestricted ones, can't be
  forwarded. Forwards can't be published to announcement channels [hc].
- **Wire format:** `message_reference.type = 1` with `message_snapshots`, flag `HAS_SNAPSHOT` (1<<14) [api].

**Do better:** destinations you can't post in stay in the list with the reason **(design/better.html › Flows:
messaging)**. Show the original author when you can see the source channel. Show the 5-destination limit as a counter.

### 1.5 Polls: missing

**Dango:** nothing (no `poll` field and no `/polls` route). Only `has:poll` exists in search.

**Discord:**
- **Entry:** the **+** in the composer, then **Poll** [hc].
- **Fields:** question (up to 300 characters [api]); up to **10** answers [hc], each up to 55 characters [api] with an
  optional emoji; duration **1 hour, 4 hours, 8 hours, 24 hours, 3 days, 1 week** [hc]; a multiple-answers toggle;
  **Post** [hc].
- **Voting:** select answers, then **Vote**. **Remove Vote** to change [hc].
- **Results:** percentages and vote counts, plus a voter list per answer [mem].
- **Ending:** the creator can end it early (`POST …/polls/{id}/expire`) [api]. A "poll result" system message is posted
  at the end [mem].
- **Rules:** polls can't be edited [api]. AutoMod checks poll text [hc]. There's a role and channel permission [hc].

**Do better:** show the answer limit from the start **(design/better.html)**; show the time left as an absolute
"ends 9:41 PM"; voted state works with the keyboard.

### 1.6 Slash commands and app interactions: missing

**Dango:** nothing. No `/` picker, no `POST /interactions`, no components. This is the biggest gap in bot-heavy servers.

**Discord:**
- **Entry:** type `/` at the start of the composer [hc]. The **App Launcher** button to the right of the composer also
  launches apps and Activities [hc].
- **Picker:** a popover with apps listed in a left rail by icon and commands on the right as `/name` plus description.
  It filters as you type [mem].
- **Option entry:** each option becomes a chip. Required options come first, optional ones are listed after.
  Autocomplete fills values for STRING, INTEGER and NUMBER, but never together with `choices` [api]. Option types 1–11
  [api].
- **Limits:** name 1–32, description 1–100, 25 options, 25 choices [api].
- **Running a command:** Enter sends `POST /interactions` [mem]. While waiting, the reply shows "<app> is thinking…"
  (deferred response type 5) [mem].
- **Ephemeral replies:** flag 64. They show "Only you can see this • Dismiss message" [mem; flag api].
- **Components:**
  - Action Row: up to 5 buttons, or one select [api].
  - Button styles: Primary, Secondary, Success, Danger, Link, Premium [api].
  - Selects: up to 25 options, placeholder up to 150 characters [api].
  - Components V2 (flag 32768): Container, Section (1–3 text displays plus a button or thumbnail), Text Display,
    Thumbnail, Media Gallery (1–10 items), File, and Separator (small or large) [api].
  - A message holds up to 40 components [api].
- **Modals:** app-defined text inputs in a dialog [mem].
- **Permissions:** Use Application Commands [hc].

**Do better:** list only commands you can run in this channel, group them by app, and put most-used first (**B#66**);
show interaction failures inline with the app's name, instead of a red "This interaction failed" line.

### 1.7 System messages: only call records render

**Dango:** `type == 3` (call) only. Every other type is dropped or rendered raw.

**Discord:**
- **Member join (type 7):** one of about 40 random lines ("X just slid into the server.") plus a **Wave to say hi!**
  sticker button [mem]. The sticker reply is a server setting [hc, settings].
- **Boost (types 8–11):** "X just boosted the server!" and "…reached Level N" [mem].
- **Pin (type 6):** "X pinned a message to this channel. See all pinned messages." [mem]
- **Thread created (18), thread starter (21), channel follow added (12), AutoMod action (24, with an embed), poll
  result (46), guild discovery and incident messages** [api types; copy mem].

**Do better:** collapse runs of joins ("5 people joined") and leave the server setting alone.

### 1.8 Voice messages: missing

**Dango:** no playback and no recording (no `flags`, `waveform` or `duration_secs`).

**Discord:**
- **Desktop can only play them, not record** [hc]. Recording is mobile only: hold the mic, release to send, swipe up to
  lock, drag to the trash to cancel [hc].
- **Limits:** up to 20 minutes [hc]. Waveform is up to 256 bytes, base64. `duration_secs` is required, and the content
  type must be `audio/*` [api]. Flag `IS_VOICE_MESSAGE` (1<<13) [api].
- **Player:** play button, waveform scrubber, elapsed and total time, playback speed [mem].
- **Permissions:** the per-role **Send Voice Messages** toggle [hc].

**Do better:**
- **Recording on desktop.** This beats Discord (Vencord's VoiceMessages plugin exists to add it). Use our existing
  capture chain with noise suppression.
- **On-device transcripts** (**B#41**).

### 1.9 Audio attachments: no player

**Dango:** audio falls back to a file card.

**Discord:** an inline player with file name, size, play, a scrubber, time and volume [mem].

**Do better:** use the same player as voice messages, with a waveform computed locally.

### 1.10 Composer states: all missing

**Dango:** slowmode is only a field in channel settings. There's no gate for permissions, timeouts or the age gate.

**Discord:**
- **Slowmode:** after you send, the composer shows a countdown. "Slowmode is enabled, but you're immune." for members
  with Manage Channel or Manage Messages [snip]. A known bug leaves the countdown stuck [snip].
- **No send permission:** the composer is replaced by "You do not have permission to send messages in this channel." It
  was restyled as the "chat guard" in February 2026 [cl].
- **Timed out:** "You're timed out until <time>" and the composer is locked [mem].
- **Age-restricted channel:** a full-pane gate warning that the channel may contain adult content, with Continue or
  Nope. Regional age assurance can apply [snip].
- **DMs:** "You cannot send messages to this user" for blocked users or DMs closed by privacy settings [mem]. A
  blocked user's DM shows a banner [mem].
- **Rules screening:** the composer becomes "Complete a few more steps…", opening the rules, then **Submit** [hc].

**Do better:**
- Name the role that can post **(design/better.html › Sign in and states)**.
- Keep the draft during slowmode or a timeout, and send it when the countdown ends, if you choose.

### 1.11 Reactions: no "who reacted", no super reactions

**Dango:** add and remove are built. "All reactions" opens the picker.

**Discord:**
- **Who reacted:** right-click a reaction, or More, then **Reactions**. A dialog lists emoji on the left and users on the
  right [mem]. `GET …/reactions/{emoji}?type=0|1` [api].
- **Limits:** 20 different emoji per message [snip].
- **Super Reactions:** Nitro, with a burst animation. Fields `burst`, `burst_colors`, `count_details` [api]. A setting
  turns off the animation [hc].
- **Tap to React:** set the preset emoji in Settings › Chat [cl Jun 2026].

**Do better:** a hover tooltip lists the first 3 names ("Ana, Bo and 4 others") straight from the cached list. Burst
effects can be turned off (**B#9**).

### 1.12 Blocked and ignored users: not filtered

**Dango:** the ignore list loads but the transcript never reads it. Block exists.

**Discord:**
- **Blocked:** a message shows as "1 blocked message — Show message" [mem].
- **Ignore:** hides new messages, notifications, profile and activity without unfriending or telling them [snip].
  Undo it in Settings › Content & Social [snip].

**Do better:** go further than Discord, and mute blocked users in voice (**B#58**).

### 1.13 Per-channel drafts: missing

**Dango:** switching channels loses what you typed.

**Discord:** drafts are kept per channel, and since February 2026 they survive the app being force-closed [cl]. A
channel with a draft shows a pencil in the DM list [mem].

**Do better:** drafts are kept per thread too, and the switcher shows which channels have drafts.

### 1.14 Code highlighting: missing

**Dango:** code blocks have a Copy button. gpui-kit's highlighter hook isn't used.

**Discord:** highlight.js language tags (` ```rust `), monospace, and a copy button on hover [mem].

**Do better:** show the language name in the block header (already noted as a GAP in `chat.rs`), and wrap long lines
with a toggle.

### 1.15 Embeds: no suppress, no inline YouTube

**Dango:** cards and link previews are built. Video embeds render as static cards.

**Discord:**
- **Suppress:** flag `SUPPRESS_EMBEDS` (4) [api]. An ✕ on embeds on your own messages, and "Remove embeds" in the menu
  [mem]. Wrapping a link as `<url>` stops the embed [mem].
- **YouTube:** click the thumbnail to play inline in an iframe [mem].
- **Special cards:** Spotify (play preview), Twitter/X and Tumblr (with post tags, Aug 2025) [cl].

**Do better:** separate switches for Link previews, Bot embeds and Inline media (**B#39**), and render preview text
without markdown (**B#40**). For YouTube in gpui, play through our H.264 path when the stream is reachable; otherwise
show a thumbnail plus "Open".

### 1.16 Send options: missing

**Dango:** none of the three.

**Discord:**
- **@everyone and @here:** a confirm dialog when a mention would notify many people [snip]. Whether the role is allowed
  is set per role ("Mention @everyone, @here, and All Roles") [hc].
- **@silent:** prefix a message with `@silent` to send it without push or desktop notifications [snip].
- **/tts:** needs the Send Text-to-Speech Messages permission [hc]. A client setting blocks /tts playback [snip].

**Do better:** show the reach ("This will notify 1,204 people") **(design/better.html › Flows: messaging)**.

### 1.17 Mark unread and the Inbox: local only, Mentions only

**Dango:** Mark Unread only moves the local marker (no ack is sent). The Inbox has Mentions only.

**Discord:**
- **Inbox:** the icon top right, desktop only [hc]. Tabs **Mentions** and **Unreads** [hc].
- **Mentions filters:** **Include @everyone mentions**, **Include @role mentions**, **Include all servers** [hc].
  Remove an item with its ✕. Mentions clear after **7 days** [hc].
- **Unreads:** unmuted channels, each with a ✓ to mark it read, plus Mark all read. @here appears under Unreads [hc].
- **Mark Unread:** Alt+click a message, or the menu [snip]. It sends an ack with `manual: true` [mem].
- **Shortcut:** Ctrl/Cmd+Alt+@ toggles the mentions popout [hc].

**Do better:** **B#17–#23**. Filters are visible chips **(design/better.html)**, and there's a "Jump to mention (2)"
pill.

### 1.18 Alt text and spoiler on upload: missing

**Dango:** `SPOILER_` uploads are built. No alt text.

**Discord:**
- **Alt text:** on the upload preview, the pencil (**Modify Attachment**), then **Description (Alt Text)**, then
  **Save** [hc]. Since September 2026 alt text and spoiler can also be added after sending [cl]. Images with alt text
  can show an "ALT" badge [snip].
- **Spoiler:** the eye icon on the preview [snip].

**Do better:** one upload dialog edits every file **(design/better.html › Media and drag)**.

### 1.19 Message Requests and Spam: missing

**Dango:** nothing, and no privacy toggle.

**Discord:**
- **Tabs:** a Message Requests tab appears on the DM home only while a request is pending. **Spam** is a sub-tab
  [snip].
- **Rules:** the spam folder can't be disabled [snip]. Safety alerts don't appear in either folder [hc]. The DM spam
  filter is in Settings › Content & Social [snip].

**Do better:** **B#55–#56**: each item has a "Not spam" action, and requests show mutual servers.

### 1.20 Small 2026 additions

| Feature | Dango | Discord |
|---|---|---|
| Spellcheck | missing | Squiggles, with fixes in the right-click menu [mem] |
| `@time` | missing | Inserts a `<t:…>` timestamp, desktop only, Feb 2026 [cl] |
| Shift+click a GIF | missing | Inserts its URL without sending, Oct 2026 [cl] |
| Right-click a GIF | missing | Copies the link [cl] |
| Rendered `.md` uploads | missing | Markdown attachments render with tables, Oct 2026 [cl] |
| Email links | missing | `<user@x.com>` becomes a mailto link, Jun 2025 [cl] |

---

## 2. Navigation, servers and DMs

### 2.1 Back and Forward: missing

**Discord:**
- **Buttons:** top left of the window, working "just like your web browser" [cl Mar 2026].
- **Keys:** Alt+← goes back [hc]. Mouse buttons 4 and 5 [snip].

**Dango plan:** a history stack of channel, thread and scroll anchor; caret buttons in the title bar strip.

### 2.2 Pinned DMs and pinned channels: missing

**Discord:**
- **What:** pin DMs and group DMs to the top of the DM list, and channels to the top of a server's list [cl Jun 2026].
- **Since September 2026:** channel pinning works in any server [cl] and is personal [snip]. DM pins carry over between
  devices [snip].

**Do better:** drag to reorder pinned items, with a small "Pinned" divider.

### 2.3 Group DMs: render only

**Dango:** existing groups render. You can't create, add, remove, rename, set the icon or leave.

**Discord:**
- **Create:** Friends page, then the **New Group DM** icon top right. **Select Friends** has search and checkboxes,
  then **Create Group DM** [hc]. Or the **Add Friends to DM** icon in a 1:1 DM, which makes a new group and leaves the
  DM alone [hc].
- **Limit:** **10** people including you [hc].
- **Header icons:** call, video call, pins, **Add Member**, member list [hc].
- **Rename:** click the group name in the header, edit, **Save** [hc].
- **Icon:** click the name, then the pencil to upload, or **Remove Icon**, then **Save** [hc].
- **Remove a member:** right-click them in the member list, then **Remove From Group**. Creator only [hc].
- **Leave:** right-click the group in the DM list, **Leave Group**, then confirm. You can only come back if someone adds
  you [hc].
- **Ownership:** passes to someone else when the owner leaves [snip]; manual transfer is not confirmed.
- **2026:** group invite links, "Clean Up Inactive Group DMs", and the invite button hidden in full groups [cl Sept
  2026].

**Do better:** the 10-person limit shows as "7 of 10" in the picker; leaving has Undo for a few seconds; picking a new
owner is a step in Leave.

### 2.4 Friends: no Add Friend, no Active Now

**Dango:** Online, All, Pending and Blocked tabs; befriend from a profile; remove, block and ignore.

**Discord:**
- **Add Friend:** Discord logo, then the **Add Friend** tab. Type a username, then **Send Friend Request**. Check
  **Pending** [hc]. Cancel with the ✕ in Pending [hc].
- **Errors:** "Hm, didn't work. Double check that the username is correct." and "Friend Request Failed" when the
  person's requests are off [hc]. An unfamiliar-sender alert appears before you accept, and can't be turned off [hc].
- **Limits:** 1,000 friends; 1,500 incoming and 1,000 outgoing pending requests. The oldest incoming requests are dropped
  past the cap [hc].
- **Active Now:** a right column of cards for friends playing, listening or in voice, with Join [snip; layout mem].
- **2026:** a friend wave prompt for new friends (Jun), "Friends Since", and a "Remind Friends" modal (Sept) [cl].

**Do better:** each failure says what happened, without revealing a block; pasted usernames are cleaned up
**(design/better.html › Flows: people and servers)**. Active Now becomes a strip in narrow windows
**(design/better.html › Panels and views)**.

### 2.5 Create a server: missing

**Discord:**
- **Entry:** the **+** (Add a Server) at the bottom of the rail [hc]. Ctrl/Cmd+Shift+N [hc].
- **Flow:**
  1. **Create My Own**, or pick a template (Gaming, Friends, Study Group, School Club, Local Community, Artists &
     Creators) [hc for Create My Own; template names mem].
  2. "For me and my friends" or "For a club or community" [mem].
  3. Name and icon (at least 128×128) [hc], then **Create**.
  4. **Join a Server** with an invite at the bottom of the same dialog [hc].
- **Templates:** a template link previews the server, then you rename it, set an icon and **Create** [hc]. Forum,
  announcement and stage channels aren't copied [hc].
- **Limit:** 100 servers, or 200 with Nitro [mem].

**Do better:** templates list the channels they create; the 100-server limit shows before you press Create
**(design/better.html › Servers and invites)**.

### 2.6 Create channel: text, voice and category only

**Dango:** text, voice and category via `POST /guilds/{id}/channels`. No other types, and no private toggle.

**Discord:**
- **Entry:** the **+** on a category on hover, or right-click the channel list [hc].
- **Dialog:**
  - A type list with radio cards: Text, Voice, Forum, Announcement, Stage, Media. The Community-only types are hidden
    otherwise [hc for types; layout mem].
  - Channel name, auto-lowercased with dashes for text [mem].
  - A **Private Channel** toggle; on, **Next** opens a member and role picker [mem].
  - **Create Channel** [hc].
- **Private means:** View Channel is denied to @everyone and allowed to the picked roles and members [hc].
- **Requirements:** forum, announcement and stage need a Community server [hc].

**Do better:** pick private access inside Create channel **(design/better.html › Flows: people and servers)**.

### 2.7 Invite dialog: no friend list, fixed link settings

**Dango:** 7-day, unlimited-use links; "Invite to Server" sends a single-use link to a DM.

**Discord:**
- **Entry:** **Create Invite** on a channel's hover, or the server menu's **Invite People** [hc].
- **Dialog:** a search over friends with an **Invite** button per row (it becomes "Sent"), then the link with Copy,
  then "Your invite link expires in 7 days. **Edit invite link**." [hc for edit; layout mem]
- **Edit invite link:**
  - Expire after: 30 minutes, 1 hour, 6 hours, 12 hours, 1 day, 7 days, Never [mem; hc confirms the 30-minute and
    no-expiry ends].
  - Max number of uses: No limit, 1, 5, 10, 25, 50, 100 [hc "No limit"; values mem].
  - **Grant temporary membership**: members are kicked on disconnect unless given a role [hc].
  - **Generate a New Link** [hc].
- **Rules:** a deleted invite code can't be recreated [hc]. Invites can be paused in Server Settings [hc].
- **2026:** voice invites show who's already there [cl Jun 2026].

**Do better:** link settings open in place; say when invites are paused **(design/better.html › Servers and invites)**.

### 2.8 Threads: no split pane, no archived browser

**Dango:** create, join and leave; threads open in the main view with a breadcrumb.

**Discord:**
- **Create:** the **…** on a message, then **Create Thread**. Optional title, then send [hc].
- **Private threads:** from the Threads menu or the attachment menu, turn on **Private Thread**, then @mention people.
  Needs boost Level 2 and Create Private Threads [hc].
- **Opening:** threads open in a resizable pane to the right of the channel [mem].
- **Thread menu:** **Edit Thread**, **Close Thread**, **Open Thread**, **Delete Thread** [hc].
- **Edit Thread:** name; slowmode (5 s–6 h); **Hide After Inactivity**: 1 hour, 24 hours, **3 days** (default),
  7 days [hc].
- **Threads Discovery:** the button at the top of the channel opens the browser with Joined, Active and Archived
  [hc for the entry; tabs mem].
- **Reopening:** typing reopens a thread unless a moderator locked it [hc].

**Do better:**
- The Threads button is always visible, with archived-thread search (**B#50–#51**).
- Threads dock in the side panel **(design/better.html)**.
- Cap the sidebar at 3 threads per channel (**B#53**).

### 2.9 Forums and media channels: no tags, sort or layout

**Dango:** a list of posts, Load more, New Post, open as a thread.

**Discord:**
- **Requirements:** Community servers only [hc].
- **Post guidelines:** shown on New Post [hc].
- **Tags:** up to 20 per channel [api]. Can be required. Fix a post's tags with right-click › **Edit Tags** [hc].
  Filter by tapping tags, with several allowed and **All** listing them [hc].
- **Layout:** **List View** or **Gallery View** as the default [hc]. Sort by Recent Activity or Creation Date [mem].
- **Search:** matches post titles [hc].
- **Hide After Inactivity:** 1 hour, 24 hours, 3 days, 1 week. Archived posts move to **Older Posts** [hc].
- **Media channels:** gallery-only forums with thumbnail tiles [mem].

**Do better:** search also matches the first message; tag chips show counts.

### 2.10 Announcement channels: missing

**Dango:** type 5 is treated as plain text.

**Discord:**
- **Publish:** after posting, a **Publish** button appears and sends the message to all following servers [hc].
  Limit: **10 publishes an hour** [hc]. @everyone and @here aren't pushed [hc].
- **Edits and deletes:** edits propagate; deletes leave "[Original Message Deleted]" [hc].
- **Permissions:** Send Messages lets you publish your own; Manage Messages publishes any [hc].
- **Converting a channel:** Edit Channel › **Announcement Channel** toggle [hc].
- **Follow:** a Follow button opens a dialog to pick a server and channel you manage [hc entry; dialog mem].

**Do better:** show the publish limit as "3 of 10 this hour".

### 2.11 Scheduled events: missing

**Discord:**
- **Entry:** server dropdown › **Create Event** [hc].
- **Step 1, location:** **Voice Channel**, **Stage Channel**, or **Somewhere Else** (a text channel, link or place)
  [hc].
- **Step 2, info:** topic, start date and time, end time, description, cover image, and repeat (recurring) [mem].
- **Step 3:** review [snip].
- **Permissions:** **Create Events** for your own, **Manage Events** for all [hc]. Voice events host up to 99 people
  [hc].
- **Showing events:** an "N Events" row at the top of the channel list opens the list. Each event has
  **Interested** (bell) and Share [mem]. A notification fires at the start [mem].

**Do better:** show times in the viewer's zone and the creator's; "Add to calendar" downloads an .ics file.

### 2.12 Joining a server: rules, onboarding, Channels & Roles, Server Guide

**Dango:** none for members. Server settings only show a read-only onboarding summary.

**Discord:**
- **Rules Screening:** up to **16** rules. A bar in the composer asks you to agree, then **Submit** grants access [hc].
- **Onboarding:** pre-join questions with answer chips that assign roles and channels, full screen, then post-join
  questions [mem].
- **Server Guide:** Welcome Sign, **New Member To Do's** (3–5 tasks), and Resources (read-only channels shown as pages)
  [hc]. It replaces the Welcome Screen [hc].
- **Channels & Roles / Browse Channels:** at the top of the channel list, with per-channel follow checkboxes [mem].

**Do better:** **design/better.html › Servers and invites** covers onboarding answers showing what they add, a rules
prompt in the composer, and a channel browser with select-all. Leaving onboarding halfway keeps a "Finish joining"
row (**B#95**).

### 2.13 Server discovery: links out

**Discord:** the Discover button at the bottom of the rail opens in-app Servers, Apps and Quests, with search and
categories [mem].

**Dango plan:** skip. Leave it as a link-out (low value, heavy UI).

### 2.14 Notification settings: no dialog, no bell, no suppress toggles

**Dango:** menus for per-server and per-channel levels, and mute durations. `suppress_everyone` and `suppress_roles` are
read but never written.

**Discord:**
- **Entry:** server dropdown or right-click › **Notification Settings** [hc].
- **Dialog:**
  - **Mute Server** with a duration: 15 minutes, 1 hour, 3 hours, 8 hours, 24 hours, Until I turn it back on [mem for
    the list].
  - Server Notification Settings: All Messages, Only @mentions, Nothing.
  - **Suppress @everyone and @here**, **Suppress All Role @mentions**, **Suppress Highlights** [hc].
  - Mute new events, mobile push notifications [mem].
  - Channel overrides, as a table of channel and setting [mem].
- **Channel header bell:** the same levels plus mute for the channel [mem].

**Do better:** **B#17–#20, #28**. Mute rows show when they end ("Muted until 9:41 PM") **(design/better.html ›
Context menus)**.

### 2.15 Report: missing

**Discord:** message menu › **Report Message**. A sheet of reason categories with sub-reasons, then Submit, then an
offer to block [mem]. Since September 2026 there are report previews [cl].

**Do better:** a reason advances in one click, and "Also block" sits beside Send **(design/better.html)**.

### 2.16 Smaller navigation gaps

| Feature | Dango | Discord |
|---|---|---|
| Favorites server | missing | A star rail item with channels you favourite from any server [mem] |
| Show All Channels | missing | A server menu toggle that brings back opted-out channels [mem] |
| `@role` click | missing | A popout listing up to 100 members [cl Mar 2026] |
| Mod View | missing | Member › Mod View: account age, join date, roles, messages, timeout and kick shortcuts [mem]; keyboard fixes Sept 2026 [cl] |
| Members page safety | missing | Flags for new accounts and unusual activity [mem] |
| Discover and Add tiles | missing | Bottom of the rail [mem] |

---

## 3. Profiles and presence

### 3.1 Profile popout gaps

**Dango has:** banner, avatar and status, names, roles, bio, pronouns, badges, mutual servers, notes, activity card,
decoration.

**Discord adds:**
- Member Since, for Discord and for the server [mem].
- Friends Since [cl Sept 2026].
- Connections with verified checks [mem].
- Profile effects (animated overlays) [mem].
- Display-name styles: fonts and effects such as Gummy and Prism [cl Sept 2026].
- Stacked activity cards and recently played games [cl Sept 2025].
- Click the avatar for full size [cl Sept 2026].
- Game profile pages from a game name: screenshots, platforms, reviews, official server [cl Jun 2026].
- Game stats widgets [cl Mar 2026].

**Do better:** the AKA row is hidden by default from people who don't share a server (**B#11**).

### 3.2 Editing your profile

**Dango has:** display name, username, avatar and banner from a file, per-server nickname. Bio, pronouns, decoration,
effect and nameplate link out to Discord.

**Discord:**
- A **Main Profile** and **Per-server Profiles** tabs, with a live preview on the right [mem].
- Bio up to **300** characters [cl Sept 2026].
- Avatar history: the last 6 avatars [cl Jun 2025].
- Server Tag picker [cl Jun 2025].
- Nameplates per server [cl Nov 2025].

**Do better:** the preview is the real popout, and the unsaved bar names what changed **(design/better.html › Profile
and privacy)**.

### 3.3 Rich presence

**Dango:** parses other people's activities. Your own presence always sends `activities: []`.

**Discord:**
- Registered Games: detects running games from a process list, plus "Add it!" for any app [mem].
- Activity privacy toggle [mem].
- Spotify Listen Along: invite and join [mem].

**Dango plan:** process detection on macOS (NSWorkspace) and Windows. Listen Along is a skip.

---

## 4. Voice, video and calls

### 4.1 Stage channels: missing

**Discord:**
- **Requirements:** Community servers [hc].
- **Roles:** Stage moderators set the topic, open the stage, and add, mute or remove speakers. Speakers. Audience,
  muted on join [hc].
- **Opening:** writing a topic opens the stage, which is then pinned under the server name [hc].
- **Speaking:** audience members **raise hand**, and a moderator approves under "Show Requests" [hc, snip]. Speakers
  have **Move to Audience** [hc].
- **Sound:** no join or leave sounds [hc]. "Sorry, this Stage is full." [cl]

**Do better:** hand state is written out; moderators handle requests in one popover **(design/better.html ›
Servers and invites)**.

### 4.2 Soundboard playback: shows a toast

**Dango:** management in Server Settings and per-user soundboard mute are built. You can't play sounds.

**Discord:**
- **Entry:** the confetti/soundboard button in voice controls [hc]. Ctrl/Cmd+Shift+B [cl].
- **Panel:** sounds grouped by server. Hover shows **Play**, ★ favourites a sound, and the speaker previews it to you
  only. **Add Sound** [hc].
- **Slots:** 8 default, then 24, 36 and 48 by boost tier [hc]. Deafened users don't hear sounds [hc].
- **Permissions:** Use Soundboard. Other servers' sounds need Nitro [snip].
- **Volume:** set in Voice & Video [snip]. Entrance sounds are Nitro [mem].

**Do better:** a global soundboard volume and an Off switch (**B#76**). Show who played each sound.

### 4.3 Activities: shows a toast

**Discord:**
- **Entry:** the **App Launcher** to the right of the composer, or the rocket in voice controls. Pick an app, then
  **Launch** [hc].
- **Pop-out:** Activities open in a floating window [cl Sept 2025].
- **Joining:** "Join Activity" on a voice channel's hover [snip].

**Dango plan:** skip unless gpui can host a webview. Show who is in an Activity, with "Open in Discord".

### 4.4 Text chat in voice channels: missing

**Discord:**
- **Entry:** the **Open Chat** bubble beside the voice channel name, or top right in the call view. It opens a right
  panel [hc].
- **Previews:** up to 3 recent message previews float top right; "Show New Message Previews" turns them off [hc].
- **Badge:** red number for mentions, grey for unread [hc].
- **Permissions:** Send Messages and Read Message History [hc].
- **2026:** works alongside PiP in stages [cl Oct 2026].

**Dango plan:** the existing chat view, docked in the call's side panel.

### 4.5 Voice channel status: missing

**Discord:** "Set a channel status" under the voice channel name once you've joined. Text plus an emoji, with a
character-limit warning [hc, cl]. Permission: **Voice Channel Status** [hc].

### 4.6 Camera, streams and the rest

| Feature | Dango | Discord |
|---|---|---|
| Camera send | Code exists; marked not verified in `voice/mod.rs` | Camera button plus a device chevron; a sound plays on start [cl Sept 2025] |
| Camera preview in settings | GAP | Preview tile in Voice & Video [mem] |
| Background blur and replacement | missing | Blur free, custom images Nitro [mem] |
| Stream pop-out window | missing | Pop out a stream to its own window [mem] |
| Zoom and pan a stream | missing | Scroll wheel or trackpad [cl Feb & Mar 2026] |
| Go Live on Windows and Linux | macOS only per README | All desktop platforms |
| Clips | missing | Windows only, Nitro early access. **Enable Clips**; keybind Alt+C; length **30 s, 60 s, 2 min**; hardware encoding; saved locally [hc] |
| Game overlay | missing | Rebuilt in 2025: chat widget, voice, notifications [cl] |
| Streamer Mode | missing | **Enable Streamer Mode**; auto-on with OBS or XSplit; Hide Personal Information, Hide Instant Invite Links, Disable Sounds, Disable Notifications; a banner with **Disable** [hc] |
| Voice effects | missing | Nitro voice filters [mem] |

**Do better:** **B#67–#81**. Bitrate presets for streams, the actual resolution and fps on the tile, separate press and
release delays for PTT and PTM, volume up to 400%, and a call timer.

---

## 5. Account, settings and the app

### 5.1 Login: QR only

**Discord:**
- **Login page:** email or phone, password, "Forgot your password?", **Log In**, "Need an account? Register". A QR panel
  on the right [mem].
- **2FA:** Passkeys and Security Keys (recommended), Authenticator App (30-second TOTP), SMS (only after an
  authenticator app), and Backup Codes (one use each) [hc]. A passkey login uses the device prompt with no password
  [hc].
- **Captcha:** hCaptcha [mem]. Dango already handles it.

**Do better:** a six-box code that accepts paste and submits on the last digit **(design/better.html › Sign in and
states)**.

### 5.2 Account security: links out

**Dango:** email, password, phone, 2FA, devices, authorized apps and connections all open discord.com.

**Discord:** Settings › My Account (email, phone, password, 2FA with backup codes, Security Keys › **Add** [hc],
disable or delete), plus Devices (sessions with "Log out all known devices") [mem].

**Do better:** **B#91**. A native Devices page with new-login toasts. Account deletion transfers your servers inside
the flow **(design/better.html)**.

### 5.3 Appearance: themes only

**Dango:** Light, Midnight and Sync with OS, plus a font family.

**Discord:**
- Themes: Light, Ash, Dark, Onyx, Sync, and the original dark theme returned in September 2026 [cl]. Nitro gradient
  themes with up to 5 colours [cl Sept 2025].
- Message display: Cozy or Compact [mem].
- Chat font scaling, zoom level, space between message groups [mem].
- UI density: Compact, Default or Spacious [mem].

**Dango plan:** compact mode and font, zoom and spacing sliders are the gaps users notice (**B#3–#4, #85**).

### 5.4 Accessibility: none

**Discord [hc]:**
- Saturation slider.
- **Sticker Animation Control**: Always animate, Animate on interaction, never.
- Text-to-Speech rate with **Preview**.
- High Contrast Mode, and Sync contrast settings (Windows).
- **Role Colors**: Show role colors in names, Show role colors next to names, Don't show role colors.
- **Sync profile themes**.
- Reduced Motion, GIF autoplay and animated emoji, linked from this page.

**Dango:** none of these. Screen-reader support is unknown: whether gpui exposes an accessibility tree is not verified
(**B#82**).

### 5.5 Chat settings: one toggle

**Dango:** RenderEmbeds only.

**Discord:**
- Display images, videos and lolcats: when posted as links, and when uploaded.
- Image descriptions.
- Embeds and link previews.
- Emoji: reactions, emoticon conversion.
- Stickers: suggestions.
- Text box: preview emoji, mentions and markdown.
- Spoilers: on click, on servers I moderate, always.
- Tap to React. [all mem; Tap to React cl]

### 5.6 Keyboard shortcuts

**Dango has:** Cmd/Ctrl+K, Alt+↑/↓, Esc to mark read, Cmd/Ctrl+F, ↑ to edit your last message, message-focus
navigation with action keys, and Cmd/Ctrl+Q and R.

**Discord has but Dango doesn't [hc]:**

| Shortcut | Action |
|---|---|
| Ctrl/Cmd+, | Settings |
| Ctrl/Cmd+/ | Shortcuts dialog |
| Ctrl/Cmd+Shift+N | Create or join a server |
| Alt+← | Back |
| Ctrl/Cmd+Shift+F | Search all channels |
| Ctrl/Cmd+E | Emoji picker |
| Ctrl/Cmd+G | GIF picker |
| Ctrl/Cmd+↑/↓ | Browse history |
| Ctrl+Alt+←/→ (⌘⌥↑/↓) | Switch servers |
| Ctrl/Cmd+P | Pins |
| Ctrl/Cmd+Alt+@ | Mentions |
| Ctrl/Cmd+U | Member list |
| Ctrl/Cmd+Shift+U | Upload |
| Ctrl/Cmd+Shift+H | Help |
| Ctrl/Cmd+B, I, U | Bold, italic, underline |
| Ctrl/Cmd+Shift+M, D | Mute, deafen in-app |
| Ctrl/Cmd+Enter, Esc | Answer, decline a call [cl] |
| Ctrl+' | Call the DM you're viewing [cl] |
| Ctrl/Cmd+Shift+Alt+V | Jump to the call [cl] |
| Ctrl/Cmd+Shift+B | Soundboard [cl] |

**Do better:** shortcuts are shown as keycaps, and conflicts name the clash **(design/better.html › Settings)**.

### 5.7 Other app gaps

| Feature | Dango | Discord |
|---|---|---|
| Language | English only | About 30 locales [mem] |
| Open on startup, start minimized | missing | Windows Settings page [mem] |
| Minimize to tray | Windows only | Windows Settings page [mem] |
| Hardware acceleration | missing | Advanced [mem] |
| Linux build | missing | .deb, .tar.gz, Flatpak (community) [mem] |
| Windows taskbar badge and flash | missing (`notify.rs:108` TODO) | Yes [mem] |
| Notification settings (2026 redesign) | partial | Friend-online frequency per sender or receiver [cl Sept 2026]; reaction notifications [hc] |

---

## 6. Paid and rare (skip)

Nitro purchase, boosting, gifts and the gift inventory, Shop and wishlists, Quests and Orbs, server subscriptions,
linked roles, App Directory and adding bots, user-installed apps, Family Center, Trending Games, Wordle and `/share`,
and WoW guild chat. **B#6–#8**: Dango shows no upsell or shop UI at all, which is a feature.

## Sources

**Help center** (support.discord.com/hc/en-us/articles/…), each read in full through the public article API:

| Article | Topic |
|---|---|
| 218344397 | Add friends |
| 204849977 | Create a server |
| 360041033511 | Server Templates |
| 10543994968087 | Channel Permissions 101 |
| 19583625604887 | Voice Channels FAQ |
| 208866998 | Invites 101 |
| 4403205878423 | Threads FAQ |
| 6208479917079 | Forum Channels FAQ |
| 360032008192 | Announcement Channel FAQ |
| 4409494125719 | Scheduled Events |
| 1500000466882 | Rules Screening |
| 13497665141655 | Server Guide |
| 215253258 | Notifications 101 |
| 1500005513722 | Stage Channels FAQ |
| 12612888127767 | Soundboard |
| 21334461140375 | Using Apps |
| 4412085582359 | Text Chat in Voice |
| 4403089981975 | Custom Stickers |
| 22163184112407 | Polls |
| 24640649961367 | Message Forwarding |
| 13091096725527 | Voice Messages |
| 360035407531 | Custom Status |
| 360045027712 | Inbox |
| 31232432266647 | Commands, Shortcuts and Navigation |
| 1500010454681 | Accessibility |
| 218485407 | Streamer Mode |
| 16861982215703 | Clips |
| 219576828 | MFA |
| 211866427 | Uploads and alt text |
| 221421867 | Pins |
| 12102061808663 | Reactions |
| 223657667 | Group Chat and Calls |

**Developer docs:** docs.discord.com/developers/resources/{message, poll, sticker, channel},
interactions/application-commands, components/reference, topics/threads.

**Changelogs:** discord.com/blog: changelogs for 2025-03-25, 2025-06-30, 2025-09-25, 2025-11-06, 2026-03-24,
2026-06-25, 2026-09-25 and 2026-10-22; patch notes for 2026-02-04, 2026-08-04, 2026-09-08 and 2026-10-06.
