# Better than Discord: what users complain about, and what Dango does instead

Every Discord UX complaint we could find, in one place, each paired with Dango's answer. For our own reference. Gathered
2026-11-19.

[design/better.html](../design/better.html) holds 153 earlier "Better than Discord" ideas, written from memory. This file is
the evidence side. Where an idea there already answers a complaint here, the answer says **(in better.html › section)**.

## How to read this

- **Evidence.** Most complaints come from Discord's own feedback forum (support.discord.com/hc/…/community/posts), which
  was read through search-result snippets: WebFetch gets HTTP 403, and the forum's vote API needs a sign-in. So vote
  counts are missing, and complaints are ranked by how many separate sources report them. Press articles and the
  Vencord plugin list were fetched and read. Anything marked *(snippet)* was seen only as a search result.
- **Reach.** Dango is a client. It can change anything drawn on screen, but nothing Discord's servers decide (limits,
  spam filtering, moderation, account policy). Each answer is tagged:
  - **client**: Dango can fix it fully.
  - **mitigate**: the cause is on the server, but Dango can soften it.
  - **n/a**: out of our reach. Kept so we don't promise it.
- **Ⓜ** marks a gap that a popular client mod (Vencord, BetterDiscord) exists to fill. A mod people install is strong
  evidence of demand.

---

## 1. Change without consent

The loudest theme. The 2023 mobile redesign, the 2025 desktop refresh and the 2026 settings redesign each set off waves
of "stop changing the UI" threads.

1. **Redesigns are forced, with no way back.** The 2023 mobile layout was switched on from the server side. The 2025
   desktop refresh's "Enable desktop refresh" toggle only appeared after users already had it, and people installed
   Vencord to get the old look back. There was a change.org petition, and Nitro cancellations were reported.
   *Sources: Android Authority; techissuestoday; change.org; forum "Forced Discord Mobile UI" (snippets).*
   → **Dango (client):** a layout change never ships without its old version behind a toggle for at least one release.
   Release notes name every moved control.
2. **Muscle memory breaks.** One user with disabilities says each change "feels like learning a new app". Settings get
   harder to find after each reshuffle. *Forum "Stop changing the design just for the sake of change", "My Personal
   Issues With The New UI 2025" (snippets).*
   → **Dango (client):** settings search matches individual rows, not just page names **(in better.html › Settings)**.
   A moved setting keeps its old name as a search alias.
3. **Customisation gets removed.** The refresh dropped options and broke themes. *(snippet)*
   → **Dango (client):** never remove a setting in a redesign. Density, message spacing and theme stay user-controlled.
4. **The 2025 refresh hurt readability.** Message separators were removed, server icons got smaller and squarer, and
   even compact mode was too wide. *Forum "New desktop UI changes", "New UI is TERRIBLE 2025" (snippets).*
   → **Dango (client):** keep three densities (Compact, Default, Spacious), an optional group separator, and the 40px
   server tiles.
5. **No user testing.** A UX engineer on the forum says the redesign breaks natural interaction patterns. *(snippet)*
   → **Dango:** run `/user-panel` on a big layout change before shipping it.

## 2. Clutter and upsells

6. **The Gift button sits in the composer and gets clicked by accident.** The Dark Patterns Tip Line logged it in 2021,
   and Discord hasn't moved it. Users say the message bar has "barely room for two medium-sized words".
   *Source: deceptive.design (fetched); darkpatternstipline.org.*
   → **Dango (client):** no Gift button. The composer's right side holds only GIF, Sticker and Emoji, and Apps once
   built.
7. **The Shop tab has a red "NEW" badge that never goes away and can't be hidden.** *(snippet)*
   → **Dango (client):** no Shop, Quests, Orbs or Nitro tiles anywhere. A red badge only ever means someone wants you.
8. **Orbs and Quests are ads.** *Source: PC Gamer, Aftermath.*
   → **Dango (client):** don't build them (also in parity-gaps §6).
9. **Too many features.** PC Gamer's "stop bombarding me with pointless new features" names Super Reactions.
   → **Dango (client):** render super reactions as normal reactions with a small burst mark, no animation. Add a toggle
   for burst effects.
10. **Nitro walls feel like upsell pressure.** Free uploads were cut from 25 MB to 10 MB in 2024, then raised back to
    20 MB in 2026, and 1080p60 streaming needs Nitro. *Sources: TechRadar, Dexerto.*
    → **Dango (mitigate):** the composer shows the limit before you pick a file. An oversized file offers "Send as link"
    or "Compress" instead of an upsell **(in better.html › Media and drag)**.
11. **The "AKA" row shows server nicknames on your global profile.** *Source: PiunikaWeb.*
    → **Dango (client):** setting to hide AKA. By default, show it only to people who share that server.
12. **Joining voice sets a custom status ("Chilling tbh") on its own.** *Forum (snippet).*
    → **Dango (client):** never set a status the user didn't type.
13. **Activity cards clutter voice channels**, and there's no way to hide them. *(snippet)*
    → **Dango (client):** a toggle that collapses activity cards in the channel list.

## 3. Performance

14. **The Windows client uses far more RAM than intended**, often over 1 GB and sometimes over 4 GB. Discord's
    workaround was an automatic restart once it passes 4 GB. *Source: alternativeto.net citing Discord (fetched).*
    → **Dango (client):** native Rust and gpui, our main structural advantage. Set a memory budget, aim for under
    300 MB idle with 50 servers, and track it in CI. Never restart as a fix.
15. **Memory stays high after leaving voice.** The Windows client still holds 1.2 to 1.4 GB after a stream ends.
    *Source: Windows Latest.*
    → **Dango (client):** tear down decoders, capture and jitter buffers when a call ends. Test that memory returns to
    its pre-call level.
16. **Game Mode ignores RAM**, and the Electron 42 upgrade traded memory for CPU. *Sources: PC Gamer; Discord's
    Sept 2026 changelog.*
    → **Dango:** none needed. Show memory in the debug pane.

## 4. Notifications and unreads

17. **The red badge counts muted servers and things that would never notify you.** Open for about five years.
    *Forum "Unread Message Badge aka the red dot" (snippet).*
    → **Dango (client):** the badge counts only what would actually notify you under your current settings. A muted
    server contributes only direct mentions.
18. **The tray and taskbar badge can't tell mentions from unreads.** Requested since 2017. *(snippet)*
    → **Dango (client):** a number means mentions, a dot means unread, and you choose which ones show.
19. **@everyone from a muted server still inflates counts.** *(snippet)*
    → **Dango (client):** "Suppress @everyone" also removes it from counts.
20. **You can't hide unread markers without muting.** *(snippet)*
    → **Dango (client):** a per-server "Hide unread dots" option, separate from mute.
21. **The Inbox mixes servers, shows stale items, and drops @here and thread mentions.** *Forum "The New Inbox",
    "Hard to find new messages", "Disappearing @here", "Thread mention not showing in Inbox" (snippets).*
    → **Dango (client):** Inbox items can be grouped by server, a thread mention is a mention, and anything older than
    the read marker drops out. Mark read leaves a one-line record with Undo **(in better.html › Panels and views)**.
22. **Opening a channel doesn't jump to your oldest unread mention.** *(snippet)*
    → **Dango (client):** a "Jump to mention (2)" pill steps through unread mentions oldest first.
23. **No single place shows everything unread.** *(opinion piece)*
    → **Dango (client):** the Inbox Unreads tab covers every unread channel, newest first, with per-channel Mark read.
24. **Folders have no "mark as read" and no shared settings.** *(snippet)*
    → **Dango (client):** a folder menu with Mark folder read, Mute folder and Notification defaults.
25. **You can't silence call rings from one DM without muting its messages.** *Forum (snippets).*
    → **Dango (client):** a separate "Ring me" switch per DM and group.
26. **Repeated DM pings while you're away** Ⓜ OnePingPerDM.
    → **Dango (client):** one sound per DM until you read it.
27. **Notification sounds share the app volume** Ⓜ NotificationVolume.
    → **Dango (client):** a separate volume slider for cue sounds.
28. **Every new server starts with All Messages notifications** Ⓜ NewGuildSettings.
    → **Dango (client):** a setting for the defaults new servers get, such as "Mentions only, suppress @everyone".
29. **Friend-online notifications can't be controlled.** Discord partly fixed this in September 2026.
    → **Dango (client):** match Discord's frequency setting, and allow it per friend.

## 5. Search, jumping and history

30. **Search results change with trivial edits, and word matching is loose**: "final" returns "finally" but not
    "final boss". *Forum "Search is broken fundamentally" (snippet).*
    → **Dango (mitigate):** the server does the search. Quoted phrases filter the results down to exact matches on the
    client. Say plainly when results may be partial.
31. **The role, @everyone and @here filters are unreliable.** *(snippet)*
    → **Dango (mitigate):** check the results on the client and hide the ones that don't actually match.
32. **The mobile redesign lost the from: filter and moved in:.** *(snippet)*
    → **Dango (client):** every filter stays in the autocomplete, even rare ones **(built: search filters)**.
33. **Jump-to-message lands several screens away.** *r/discordapp bug megathread, April 2024 (snippet).*
    → **Dango (client):** load around the target, centre it, flash it, and keep it pinned while images above it load.
34. **Jumping to a result clears your search.** One user scrolled through about 19,000 posts. *(snippet)*
    → **Dango (client):** results stay in the side panel while you jump. Back returns to them with the scroll
    position kept.
35. **DM search results do nothing when clicked.** *(snippet)*
    → **Dango (client):** test it, because this is a regression magnet.
36. **Knowledge gets lost: chat buries answers and isn't indexed.** *Sources: curtismchale.ca, "Discord or the Death of
    Lore", Neowin quoting Discord's SVP calling it a "black hole".*
    → **Dango (client):** saved messages and bookmarks, built locally if Discord has no endpoint, plus "Copy as Markdown"
    for a thread.

## 6. Messaging

37. **Reply pings are on by default and people forget to turn them off.** *Source: PiunikaWeb, Jan 2023; forum
    "Reply Ping Default OFF" (snippet).* Ⓜ NoReplyMention.
    → **Dango (client):** a setting for the default, plus memory per channel. *(Dango already has the @ ON/OFF toggle.)*
38. **2,000 characters with no counter, and longer text turns into a .txt file.** *Forum "Message Max Length"
    (snippet).*
    → **Dango (client):** a counter appears after 1,800 characters. Over the limit, offer "Split into 2 messages" or
    "Send as file" **(in better.html › Flows)**.
39. **"Embed links" is one blunt switch.** It also hides bot embeds, and the link stays clickable. *(snippets)*
    → **Dango (client):** separate switches for Link previews, Bot embeds and Inline media, plus per-message
    suppression.
40. **Discord markdown renders inside link-preview text.** *(snippet)*
    → **Dango (client):** preview text is rendered as plain text.
41. **Voice messages can't be read without playing them**, which shuts out deaf users. *Forum "Auto Transcribe Voice
    Messages" (snippet).*
    → **Dango (client):** on-device transcription (Apple Speech or Whisper), shown under the waveform. Never uploaded.
42. **Polls lacked their own permission**, plus a 10-answer cap, no anonymous voting and no has:poll search.
    *(snippets)*
    → **Dango (mitigate):** the caps are Discord's. Show the answer count from the start **(in better.html › Media and
    drag)**. Polls are already in has:poll.
43. **The pin limit is too low.** It was 50 and became 250 in August 2025. *(snippets; Discord changelog)*
    → **Dango (mitigate):** name the limit before you reach it, and unpin with Undo **(in better.html › Flows:
    messaging)**.
44. **People miss being able to see deleted and edited messages** Ⓜ MessageLogger.
    → **Dango: won't build.** It breaks the sender's expectations. Show "(edited)" with Discord's edit time only.
45. **No translation** Ⓜ Translate.
    → **Dango (client):** Translate in the message menu, using on-device translation where the OS has it.
46. **Links carry trackers** Ⓜ ClearURLs.
    → **Dango (client):** strip known tracking parameters on paste, and show that it happened.
47. **Reply and edit are slow without the mouse** Ⓜ QuickReply, MessageClickActions.
    → **Dango (client):** Discord's keys (Up to edit; Tab-select, then R or E), plus double-click to edit your own
    message.
48. **Sticker and emoji upkeep is tedious** Ⓜ ExpressionManager.
    → **Dango (client):** in Server Settings, rename emoji in place and clone emoji between servers you manage.
49. **Tracking titles on YouTube embeds** Ⓜ Dearrow.
    → **Dango:** skip. Low value.

## 7. Threads and forums

50. **The Threads button only shows when a channel has active threads**, so archived threads can't be reached.
    *(snippets)*
    → **Dango (client):** the Threads button is always in the header, opening Active and Archived tabs.
51. **Archived threads can't be searched.** *(snippet)*
    → **Dango (client):** a search box at the top of the thread browser. Discord's API supports archived-thread search.
52. **On PC, the Archive Thread button was replaced by Close only.** *(snippet)*
    → **Dango (client):** both actions, with words that say what each does.
53. **Thread lists clutter the channel sidebar.** *(snippet)*
    → **Dango (client):** cap nested threads at 3 per channel, followed by "+N more". Threads open docked in the side
    panel **(in better.html › Panels and views)**.
54. **Forums can't be found from outside Discord, and the 20-tag cap hurts.** *Sources: Inverse, Astrogation, DEV.*
    → **Dango (n/a for the cap):** sorting, filtering and tag search inside the client are enough.

## 8. DMs, spam and safety

55. **Spam and scam DMs**, fake Nitro giveaways, and fake "Trust & Safety" DMs sent from hacked friends' accounts.
    *Forum (snippets); Kaspersky; communityone.io.*
    → **Dango (mitigate):** warn on first contact from a stranger, flag lookalike links to discord.com, gifts and
    Nitro, and add a one-click "Report spam and block".
56. **Real DMs land in the Spam folder, and the folder can't be switched off.** *(snippets; Discord help article)*
    → **Dango (mitigate):** Message Requests and Spam are tabs with counts, and each item has "Not spam".
57. **Friend requests from strangers.** *(snippets)*
    → **Dango (mitigate):** expose Discord's three request sources clearly, and show mutual servers on the request
    card.
58. **Block is incomplete.** Blocked users stay visible in voice (camera included), in the member list and in shared
    servers. *Forum "The useless, frustrating and utterly dangerous block feature" and others (snippets).*
    → **Dango (client):** a blocked user is muted for you in voice, their camera tile is hidden, their messages
    collapse to "Blocked message", and you're told once **(in better.html › Flows: people and servers)**.
59. **QR login scams hand over account tokens.** Discord shortened QR validity to 2 minutes. *Source: PortSwigger.*
    → **Dango (client):** Dango shows the QR, so the risk is on the phone that scans it. If we ever build scanning,
    the confirm screen must name the device and place.

## 9. Server admin

60. **Permission logic is counterintuitive.** An allow on any role beats a deny on a higher role. Open since 2020.
    *(snippets)* Ⓜ PermissionsViewer.
    → **Dango (client):** an effective-permissions view that names the rule that decides **(in better.html › Server
    settings)**.
61. **Manage Roles is too coarse.** It lets you edit channel permissions, and Muted roles keep video and slash
    commands. *(snippet)*
    → **Dango (n/a for the model):** warn when a role labelled "muted" still has Speak, Video or Use Application
    Commands.
62. **Moderators can't ban or assign roles because of role order**, and the UI doesn't say so. *Source: PeakBot guide.*
    → **Dango (client):** a disabled action names the role-order problem and links to the Roles page.
63. **AutoMod keyword false positives**: `ass*` matches "assistant". *Source: CommunityOne guide.*
    → **Dango (client):** an AutoMod test box **(in better.html › Server settings)**.
64. **The audit log has gaps.** Channel moves and effective-permission changes aren't logged. *(snippet)*
    → **Dango (n/a):** we can only show what's logged. Group bursts of entries **(in better.html)**.
65. **Enabling Community is opaque, and the welcome screen was replaced by onboarding.** *Sources: Discord help;
    CommunityOne.*
    → **Dango (client):** **(in better.html › Server settings)** "Community can be enabled any time".
66. **Slash command clutter**: bot commands show where they can't be used, and bot-only channels didn't stop them.
    *(snippets)*
    → **Dango (client):** the picker lists only commands usable in this channel, grouped by app, most-used first.

## 10. Voice and video

67. **Stage audio is low bitrate (about 40 kbps).** *(snippet)*
    → **Dango (n/a):** the server sets it. Show the bitrate in connection details.
68. **Screen share has no bitrate control** and is capped near 2,500 kbps. *Forum (snippet).* Ⓜ
    WebScreenShareFixes.
    → **Dango (client):** our own engine. Offer a Quality, Balanced or Smooth choice that sets bitrate, and show the
    live bitrate.
69. **Nitro 1080p60 streams still look blurry, and stream audio gets choppy.** *(snippets)*
    → **Dango (client):** show the actual stream resolution, fps and bitrate on the tile, so blur is explained.
70. **Linux screen share had no audio, and no native Wayland support until 2025.** Push-to-talk on Wayland is still
    weak. *Sources: GamingOnLinux.*
    → **Dango (client):** a Linux build with PipeWire capture and the xdg-desktop-portal GlobalShortcuts portal for
    push-to-talk.
71. **Bluetooth headsets drop to the low-quality call profile when their mic opens.** *Discord known-issue article.*
    → **Dango (client):** when the output is Bluetooth, suggest the built-in mic and explain why.
72. **Krisp distorts laughter and loud speech.** *(snippets)*
    → **Dango (client):** expose DeepFilterNet's attenuation limit as a strength slider (today it's only the
    `DFN_ATTEN_DB` environment variable), and add a one-click test recording.
73. **Voice stuck on "RTC Connecting" or "No Route".** *Source: PiunikaWeb, Feb 2026.*
    → **Dango (client):** every connection stage is already named. On stall, add "Try another region" and "Copy
    diagnostics".
74. **Push-to-talk clips the start of speech or leaks the key click, and mute delay doesn't apply to Push to Mute.**
    *(snippets)*
    → **Dango (client):** separate press and release delays, for both Push to Talk and Push to Mute.
75. **Keybinds regress without notice.** *Reddit (snippet).*
    → **Dango (client):** a hook self-test in Keybinds settings that shows live key state.
76. **Soundboard is too loud, with no global off switch.** *(snippets)*
    → **Dango (client):** a global soundboard volume, an "Off" setting, and per-user soundboard mute (already built).
77. **Stage moderation lacks an idle timer, fine-grained permissions and audit entries.** *(snippet)*
    → **Dango (mitigate):** handle hand-raise requests in one popover **(in better.html › Servers and invites)**.
78. **DM calls kick idle users after 3 minutes** Ⓜ DisableCallIdle.
    → **Dango (mitigate):** whether the server enforces this is not verified. If it's client-side, don't kick.
79. **No call timer** Ⓜ CallTimer.
    → **Dango (client):** elapsed time is already on voice rows. Add it to the call bar.
80. **Volume caps at 200%** Ⓜ VolumeBooster.
    → **Dango (client):** per-user and per-stream volume up to 400%, with a limiter.
81. **Picture-in-picture for video** Ⓜ PictureInPicture.
    → **Dango:** built (`pip.rs`).

## 11. Accessibility

82. **Screen readers struggle with the new layout**: too many headings, and JAWS and NVDA navigation breaks.
    *(snippet; AFB review)*
    → **Dango (client):** not verified whether gpui exposes an accessibility tree. Find out first. If it doesn't, this
    is our biggest gap, not an advantage.
83. **Unlabelled buttons** (emoji, attach, mentions, notifications). *Source: change.org petition.*
    → **Dango (client):** check every icon button has a tooltip label (not all are verified), and expose the labels to
    accessibility once that tree exists.
84. **Status colours are hard to tell apart, and Colorblind Mode was reportedly removed.** *(snippet)* Ⓜ ColorSighted
    goes the other way.
    → **Dango (client):** status shapes on by default, with an option to remove them.
85. **Icons are too small for low-vision users.** *(snippet)*
    → **Dango (client):** a UI zoom setting, plus chat font scaling that also scales icons.

## 12. Accounts, privacy and trust

86. **Government ID photos leaked from a support vendor**: about 70,000 users in October 2025, including people who
    appealed age checks. *Sources: The Register (fetched), 80.lv.*
    → **Dango (n/a):** we never handle IDs. Age verification links out to Discord.
87. **The age-verification rollout (UK, then global) drew a backlash.** Discord's CTO said they "missed the mark".
    *Source: TechCrunch, Feb 2026.*
    → **Dango (n/a):** link out, and say plainly what Discord will ask for.
88. **Ban appeals get bot replies, and tickets are auto-closed.** *Forum; change.org petition (snippets).*
    → **Dango (n/a):** when an account is restricted, show Discord's reason as given, with the appeal link.
89. **A phone number is required to unlock an account**, and some users see it as needless data collection.
    *(snippets)*
    → **Dango (n/a):** explain why Discord asks, and link out.
90. **No telemetry opt-out** Ⓜ NoTrack.
    → **Dango (client):** Dango sends no analytics. Say so in Settings › Privacy, and keep it true.
91. **You can't see your sessions or get alerts about new logins** Ⓜ BetterSessions.
    → **Dango (client):** a native Devices page with names, places and times, plus a toast on a new session
    **(in better.html › Profile and privacy)**.

## 13. Updates

92. **Forced updates ignore settings and break things.** A broken Linux voice release had to be rolled back.
    *(snippets)*
    → **Dango (client):** updates download in the background, wait until a call ends **(in better.html)**, and a bad
    release can be rolled back from Settings › Updates.

## 14. Platform gaps

93. **The web client lacks context menus, keybinds and rich presence** Ⓜ WebContextMenus, WebKeybinds,
    WebRichPresence.
    → **Dango:** not applicable, Dango is desktop-only.
94. **Linux is a second-class platform.** *(GamingOnLinux and others)*
    → **Dango (client):** ship Linux (parity-gaps §5). It's the easiest group of users to win.

## 15. Joining servers

95. **Full-screen onboarding blocks people who already know why they came.** One user calls joining "a slow, tedious,
    and painful process", and a vendor blog says channel lists feel redundant to new members. *Forum "Onboarding has bad
    UX" (snippet); peakbot.pro (weak).*
    → **Dango (client):** onboarding opens as a skippable panel. Skipping lands you in the default channels with a
    "Finish joining" row, and each answer shows the channels and role it adds **(in better.html › Servers and
    invites, Flows: people and servers)**.

---

## What client mods add, and Dango's call

The most popular Vencord and BetterDiscord plugins, as a demand signal. *(Vencord plugin list fetched; ranking is
judgement.)*

| Plugin | Gap it fills | Dango |
|---|---|---|
| FakeNitro | Nitro emoji, stickers and stream quality without paying | Won't build. It breaks Discord's terms and other users' expectations. |
| WebScreenShareFixes | Stream bitrate cap | Build: bitrate presets (#68) |
| MessageLogger | Deleted and edited history | Won't build (#44) |
| SpotifyControls | Spotify in the client | Maybe: media controls on the Listening card |
| NoTrack | Telemetry | Default: Dango sends none (#90) |
| ClearURLs | Tracking parameters | Build (#46) |
| VolumeBooster | Volume over 200% | Build (#80) |
| PictureInPicture | PiP | Built |
| CallTimer | Call length | Build (#79) |
| PermissionsViewer | Who can do what | Build (#60) |
| Translate | Translation | Build (#45) |
| QuickReply, MessageClickActions | Keyboard and mouse speed | Build (#47) |
| CustomRPC | Custom rich presence | Skip |
| ExpressionManager | Emoji upkeep | Build (#48) |
| OnePingPerDM | Ping spam | Build (#26) |
| DisableCallIdle | Idle call kick | Investigate (#78) |
| NewGuildSettings | Defaults for new servers | Build (#28) |
| NotificationVolume | Separate cue volume | Build (#27) |
| VoiceMessages | Recording voice messages on desktop | Build (parity-gaps §1) |
| Dearrow | YouTube clickbait | Skip |
| BetterSessions | Sessions and new-login alerts | Build (#91) |
| NoReplyMention | Reply ping off by default | Build (#37) |
| ColorSighted | Remove status shapes | Option (#84) |
| ShowHiddenChannels | See hidden channels | Won't build. Discord patched it, and it leaks. |

## Sources

Press and other fetched pages: alternativeto.net (Discord RAM, Dec 2025); The Register (ID leak, Oct 2025); TechCrunch
(age verification delay, Feb 2026; AI moderation bans, Jul 2026); PC Gamer ("stop bombarding me with pointless
features"; Game Mode; Orbs); Windows Latest (Electron RAM, Sept 2026); deceptive.design (gift button); PiunikaWeb
(reply pings, AKA, Feb 2026 voice issues); GamingOnLinux (Wayland, Jan 2025); Fortune and Windows Central (usernames,
2023); Android Authority (2023 mobile layout); TechRadar and Dexerto (upload limits); Inverse (forums); curtismchale.ca;
engineered.at "Death of Lore"; Neowin (Discord as a "black hole"); vencord.dev/plugins.

Discord feedback-forum threads are cited by title in each entry. Their URLs are under
`support.discord.com/hc/*/community/posts/` and were seen as search snippets only.
