// Worst-case fixture data for checking each screen without a Discord account (debug builds only):
// CHORD_DEMO=chat | gallery | profile | profile-plain | emoji | switcher | status | accounts | unread | home | folder | stream-card | login | scanned | failed | empty.
use gpui_kit::{Context, Window};

use crate::{
    Rustcord, Feed, Status,
    discord::{
        Activity, ActivityKind, Channel, ChannelKind, Embed, Emoji, Event, Folder, Guild, GuildTag, ListItem, ListOp, Media, MediaKind, Member, Message, Presence, Reaction, ReadState, Ready, Relation, Relationship, Role,
        Status as Presence_, User, VoiceState, avatar_url,
    },
    login::Login,
};

pub fn load(chord: &mut Rustcord, scene: &str, window: &mut Window, cx: &mut Context<Rustcord>) {
    let me = me_user();
    match scene {
        "login" => return chord.status = Status::SignedOut(Login::qr("https://discord.com/ra/demo-fingerprint-0123456789abcdef")),
        "scanned" => return chord.status = Status::SignedOut(Login::Scanned(me)),
        "failed" => return chord.status = Status::SignedOut(Login::Failed("Invalid ticket".into())),
        _ => {}
    }
    chord.on_event(Event::Ready(Box::new(ready(me))), cx);
    chord.on_event(Event::MemberList { guild_id: "1".into(), groups: vec![("r1".into(), 2), ("online".into(), 3), ("offline".into(), 1)], ops: vec![ListOp::Sync { start: 0, items: members() }] }, cx);
    chord.guild_id = Some("1".into());
    if scene == "empty" {
        return;
    }
    let messages = messages();
    chord.feeds.insert("general".into(), Feed { messages: std::sync::Arc::new(messages), loaded: true, ..Default::default() });
    let general = chord.guild().and_then(|g| g.channels.iter().find(|c| c.id == "general").cloned());
    if let Some(channel) = general {
        chord.open_channel(channel, window, cx);
    }
    chord.on_event(Event::Typing { channel_id: "general".into(), user_id: "8388608".into(), name: Some("Zoë".into()) }, cx);
    match scene {
        "gallery" => chord.open_viewer(&avatar_url("3", None), window, cx),
        "profile" => demo_profile(chord, cx),
        // No theme, banner, badges or games: the plain surface, and the Add role row in view.
        "profile-plain" => {
            demo_profile(chord, cx);
            if let Some(profile) = chord.profile_card.as_mut().and_then(|c| c.profile.as_mut()) {
                profile.theme = None;
                profile.banner = None;
                profile.badges.clear();
                profile.games.clear();
            }
        }
        "emoji" => chord.open_emoji_picker(crate::EmojiTarget::Composer, window, cx),
        "switcher" => chord.open_switcher(window, cx),
        "status" => chord.status_menu = true,
        "accounts" => {
            chord.prefs.accounts = vec![account(&me_user(), Presence_::Online), account(&user("8388608", "Bartholomew Featherstonehaugh-Wright"), Presence_::Dnd), account(&user("16777216", "theo.alt"), Presence_::Idle)];
            chord.status_menu = true;
        }
        "settings" => chord.open_settings(crate::settings::Page::Account, window, cx),
        "voice-settings" => chord.open_settings(crate::settings::Page::VoiceVideo, window, cx),
        "keybinds" => chord.open_settings(crate::settings::Page::Keybinds, window, cx),
        "keybinds-select" => {
            chord.open_settings(crate::settings::Page::Keybinds, window, cx);
            if let Some(view) = chord.settings.as_mut() {
                view.open_first_select();
            }
        }
        "appearance" => chord.open_settings(crate::settings::Page::Appearance, window, cx),
        "unread" => chord.unread_marker = Some(103),
        "hover" => chord.hovered_message = Some("105".into()),
        "home" => {
            chord.restore = None;
            chord.open_home(cx);
        }
        "folder" => {
            chord.open_folders.insert("9".into());
        }
        "stream-card" => {
            let key = "guild:1:lounge:12582912".to_owned();
            let blender = Activity { kind: ActivityKind::Playing, text: "Blender".into(), more: 0 };
            chord.presences.insert("12582912".into(), Presence { status: Presence_::Online, activity: Some(blender), rich: None });
            let since = std::time::Instant::now().checked_sub(std::time::Duration::from_secs(24 * 60)).unwrap_or_else(std::time::Instant::now);
            chord.stream_live_since.insert(key.clone(), since);
            chord.on_event(Event::StreamViewers { key, viewers: vec!["8388608".into(), "16777216".into(), "20971520".into(), "4194304".into()] }, cx);
            chord.stream_card_hover = Some("stream-12582912".into());
        }
        "user-menu" => {
            demo_call(chord);
            chord.call_view = false;
            chord.voice_user_menu("8388608", gpui_kit::point(gpui_kit::px(300.), gpui_kit::px(160.)), cx);
        }
        "server-menu" => chord.open_server_menu("1".into(), gpui_kit::point(gpui_kit::px(60.), gpui_kit::px(130.)), false, cx),
        // Move server is entry 14 (the header and separators count); Folder is its first child.
        "server-menu-nested" => {
            chord.open_server_menu("1".into(), gpui_kit::point(gpui_kit::px(60.), gpui_kit::px(130.)), false, cx);
            chord.open_submenus(vec![14, 0]);
        }
        "channel-menu" => {
            let general = chord.guild().and_then(|g| g.channels.iter().find(|c| c.id == "general").cloned());
            if let Some(channel) = general {
                chord.open_channel_menu(channel, "1".into(), gpui_kit::point(gpui_kit::px(200.), gpui_kit::px(170.)), cx);
            }
        }
        "message-menu" => {
            let message = chord.feeds.get("general").and_then(|f| f.messages.iter().find(|m| m.author.id == "8388608").cloned());
            if let Some(message) = message {
                chord.open_message_menu(message, gpui_kit::point(gpui_kit::px(700.), gpui_kit::px(260.)), cx);
            }
        }
        // Fixture sources rather than the real listing, which would ask for Screen Recording access.
        // share-window remembers a closed Figma window, so a sibling is preselected.
        "share" | "share-window" => {
            demo_call(chord);
            if scene == "share-window" {
                chord.prefs.last_share = Some(crate::go_live::LastShare::Window { id: 99, pid: 501, app_name: "Figma".into() });
            }
            let window = |id, pid, app: &str, title: &str| crate::voice::ShareableWindow { id, title: title.into(), app_name: app.into(), pid };
            let windows = vec![
                window(1, 500, "Safari", "Wave: a Discord client in Rust with gpui, the full and very long window title of a pull request"),
                window(2, 501, "Figma", "Go Live picker"),
                window(3, 502, "Terminal", "zsh"),
                window(4, 503, "Notes", "N"),
            ];
            let screen = |id, name: &str| crate::voice::ShareableScreen { id, name: name.into(), width: 1512, height: 982 };
            chord.empty_share_picker(cx);
            chord.list_shareables(Ok(windows), Ok(vec![screen(1, "Screen 1"), screen(2, "Screen 2")]), cx);
        }
        "call" | "call-info" | "call-devices" => {
            demo_call(chord);
            match scene {
                "call-info" => chord.connection_info = true,
                "call-devices" => chord.open_device_menu(crate::call::DeviceKind::Input, gpui_kit::point(gpui_kit::px(260.), gpui_kit::px(760.)), cx),
                _ => {}
            }
        }
        _ => {}
    }
}

// Zoë's popout with every section filled: theme, decoration, tag, badges, mutuals, an emoji bio, games and a joinable party.
fn demo_profile(chord: &mut Rustcord, cx: &mut Context<Rustcord>) {
    let started = chrono::Utc::now().timestamp_millis() - 7 * 60_000 - 2_000;
    let rich = crate::discord::RichPresence {
        kind: ActivityKind::Playing,
        name: "PUBG: BATTLEGROUNDS".into(),
        details: Some("Normal, Erangel, 58/99".into()),
        state: Some("4 Squad".into()),
        image: Some("brand/icons/blueprint.png".into()),
        party: Some((4, 4)),
        started: Some(started),
        joinable: true,
    };
    let playing = Activity { kind: ActivityKind::Playing, text: rich.name.clone(), more: 0 };
    chord.presences.insert("8388608".into(), Presence { status: Presence_::Dnd, activity: Some(playing), rich: Some(rich) });
    let mut zoe = user("8388608", "Zoë");
    zoe.tag = Some(GuildTag { text: "wony".into(), badge: Some("brand/icons/sticker.png".into()) });
    zoe.decoration = Some("demo/decoration.png".into());
    chord.open_profile(zoe, gpui_kit::point(gpui_kit::px(520.), gpui_kit::px(120.)), cx);
    let badge = |name: &str, description: &str| crate::discord::Badge { description: description.into(), icon: format!("brand/icons/{name}.png") };
    let profile = crate::discord::Profile {
        username: "zoe.wony".into(),
        bio: Some("LUCKY VICKY <a:aWonUwu:1328465377218269257>\n**gpui** fan, _probably_ in voice".into()),
        pronouns: Some("she/her".into()),
        banner: Some("demo/banner.png".into()),
        roles: vec!["r2".into()],
        theme: Some((0x9B1FA8, 0xE9A6C9)),
        badges: ["brand", "glass", "pixel", "paper", "graphite", "sticker", "peek", "sleepy", "bone"].iter().map(|n| badge(n, "Subscriber since 14 Nov 2024")).collect(),
        mutual_friends: vec![user("12582912", "abyss reverie"), user("16777216", "ani khet"), user("20971520", "ferris")],
        mutual_friend_count: 5,
        mutual_server_count: 1,
        games: ["peek", "riso", "pixel", "brand", "glass"].iter().map(|n| format!("brand/icons/{n}.png")).collect(),
        ..Default::default()
    };
    if let Some(card) = chord.profile_card.as_mut() {
        card.profile = Some(profile);
    }
}

// A connected call in team-lounge with made-up stats; the session itself never reaches a server.
fn demo_call(chord: &mut Rustcord) {
    let Some(lounge) = chord.guild().and_then(|g| g.channels.iter().find(|c| c.id == "lounge").cloned()) else { return };
    let (session, _) = crate::voice::join("4194304".into(), Some("1".into()), "lounge".into(), crate::voice::GatewayHandle::new().0, Default::default());
    let stats = crate::voice::Stats {
        ping_ms: 42,
        avg_ping_ms: 47,
        last_pings: vec![44, 51, 47, 40, 62, 45, 43, 48, 120, 52, 46, 44, 41, 47, 50, 42],
        outbound_loss_pct: 0.,
        inbound_loss_pct: 0.4,
        endpoint: "c-bom06-1a2b3c4d.discord.media".into(),
        server_ip: "162.159.0.1".into(),
        encryption: "aead_aes256_gcm_rtpsize".into(),
        dave: true,
    };
    chord.call = Some(crate::Call {
        session,
        guild_id: Some("1".into()),
        channel: lounge,
        connected: true,
        stage: crate::voice::Stage::Connected,
        stats: Some(stats),
        level: (-30., true),
        speaking: ["8388608".to_owned()].into_iter().collect(),
        last_spoke: Default::default(),
        cameras: Default::default(),
        camera_on: false,
        watching: Vec::new(),
        focus: None,
        stream_layout: crate::call::StreamLayout::Focus,
        ended: None,
        sharing: None,
        share_source: None, live_since: None, self_view: None, share_label: None,
        joined: std::time::Instant::now().checked_sub(std::time::Duration::from_secs(5047)).unwrap_or_else(std::time::Instant::now),
        _events: gpui_kit::Task::ready(()),
    });
    chord.call_view = true;
    chord.restore = None;
}

fn demo_thread() -> Channel {
    let mut thread = channel("t1", "gpui list virtualisation", ChannelKind::Thread, Some("general"));
    thread.thread = Some(crate::discord::ThreadMeta { joined: true, message_count: 12, ..Default::default() });
    thread
}

fn first_thread_chip(message: &mut Message) {
    message.thread = Some(demo_thread());
}

fn user(id: &str, name: &str) -> User {
    User { id: id.into(), name: name.into(), avatar: avatar_url(id, None), ..Default::default() }
}

fn channel(id: &str, name: &str, kind: ChannelKind, parent: Option<&str>) -> Channel {
    Channel {
        id: id.into(),
        name: name.into(),
        kind,
        parent_id: parent.map(Into::into),
        position: 0,
        topic: None,
        last_message_id: None,
        private: false,
        recipient: None,
        thread: None,
        raw_type: 0,
        overwrites: Vec::new(),
    }
}

fn ready(me: User) -> Ready {
    let mut general = channel("general", "👋-general", ChannelKind::Text, Some("c0"));
    general.topic = Some("Este es un buen lugar para hablar de Rust, gpui y todo lo demás".into());
    let mut staff = channel("staff", "🔒-staff-notes", ChannelKind::Text, Some("c0"));
    staff.private = true;
    staff.last_message_id = Some("900".into());
    let mut media = channel("media", "📷-media", ChannelKind::Text, Some("c0"));
    media.last_message_id = Some("900".into());
    let roles = vec![
        Role { id: "r1".into(), name: "Council".into(), color: 0xE91E63, position: 3, permissions: 0, managed: false },
        Role { id: "r2".into(), name: "Rustacean".into(), color: 0xE67E22, position: 2, permissions: 0, managed: false },
        Role { id: "1".into(), name: "@everyone".into(), color: 0, position: 0, permissions: 0, managed: false },
    ];
    let guild = |id: &str, name: &str, channels: Vec<Channel>, voice_states: Vec<VoiceState>| Guild {
        id: id.into(),
        name: name.into(),
        icon: None,
        channels,
        roles: roles.clone(),
        voice_states,
        threads: Vec::new(),
        default_notifications: 1,
        owner_id: Some("4194304".into()),
        my_roles: Vec::new(),
        emojis: Vec::new(),
    };
    let lounge_states = vec![
        VoiceState { guild_id: Some("1".into()), channel_id: Some("lounge".into()), user_id: "12582912".into(), user: Some(user("12582912", "abyss reverie")), muted: false, deafened: false, server_muted: false, server_deafened: false, streaming: true, camera: false },
        VoiceState { guild_id: Some("1".into()), channel_id: Some("lounge".into()), user_id: "16777216".into(), user: Some(user("16777216", "ani khet")), muted: true, deafened: false, server_muted: false, server_deafened: false, streaming: false, camera: false },
        VoiceState { guild_id: Some("1".into()), channel_id: Some("lounge".into()), user_id: "8388608".into(), user: Some(user("8388608", "Zoë")), muted: false, deafened: false, server_muted: false, server_deafened: false, streaming: false, camera: false },
        VoiceState { guild_id: Some("1".into()), channel_id: Some("lounge".into()), user_id: "20971520".into(), user: Some(user("20971520", "ferris")), muted: true, deafened: true, server_muted: false, server_deafened: false, streaming: false, camera: false },
        VoiceState { guild_id: Some("1".into()), channel_id: Some("lounge".into()), user_id: "4194304".into(), user: Some(user("4194304", "frsty")), muted: false, deafened: false, server_muted: false, server_deafened: false, streaming: false, camera: false },
    ];
    let channels = vec![
        channel("c0", "General", ChannelKind::Category, None),
        general,
        media,
        staff,
        channel("long", "a-channel-name-long-enough-to-truncate-in-the-sidebar", ChannelKind::Text, Some("c0")),
        channel("c1", "Voice", ChannelKind::Category, None),
        channel("lounge", "team-lounge", ChannelKind::Voice, Some("c1")),
        channel("hub", "beginners-hub", ChannelKind::Voice, Some("c1")),
    ];
    let mut dm = channel("dm", "Zoë", ChannelKind::Dm, None);
    dm.recipient = Some(user("8388608", "Zoë"));
    Ready {
        developer_mode: true,
        guild_settings: Vec::new(),
        ignored: Vec::new(),
        me,
        status: Presence_::Dnd,
        custom_status: None,
        session_id: String::new(),
        guilds: vec![
            {
                let mut g = guild("1", "We are Sandesh Pandey", channels, lounge_states);
                g.threads = vec![demo_thread()];
                g
            },
            guild("2", "gpui", vec![channel("g2", "general", ChannelKind::Text, None)], vec![]),
            guild("3", "Zed Industries", vec![], vec![]),
            guild("4", "Bevy", vec![], vec![]),
            guild("5", "Tokio", vec![], vec![]),
        ],
        folders: vec![
            Folder { id: String::new(), name: None, color: None, guild_ids: vec!["1".into()] },
            Folder { id: "9".into(), name: Some("Rust".into()), color: Some(0xCE422B), guild_ids: vec!["2".into(), "3".into(), "4".into(), "5".into()] },
        ],
        dms: vec![dm],
        read_states: vec![
            ("media".into(), ReadState { last_read: 100, mentions: 0 }),
            ("staff".into(), ReadState { last_read: 100, mentions: 2 }),
        ],
        presences: vec![("8388608".into(), Presence { status: Presence_::Online, activity: None, rich: None })],
        relationships: vec![
            Relationship { user: user("8388608", "Zoë"), kind: Relation::Friend },
            Relationship { user: user("12582912", "abyss reverie"), kind: Relation::Friend },
            Relationship { user: user("16777216", "ani khet"), kind: Relation::Incoming },
        ],
    }
}

fn members() -> Vec<ListItem> {
    let member = |id: &str, name: &str, roles: &[&str], status, activity: Option<(ActivityKind, &str)>, bot: bool| {
        let mut user = user(id, name);
        user.bot = bot;
        // A nameplate, with bundled art standing in for Discord's (the demo stays offline).
        if id == "4194304" {
            user.nameplate = Some(crate::discord::Nameplate { art: "brand/icons/glass.png".into(), tint: (0x2D6A4F, 0x95D5B2) });
        }
        ListItem::Member(Member { user, roles: roles.iter().map(|r| r.to_string()).collect(), presence: Presence { status, activity: activity.map(|(kind, text)| Activity { kind, text: text.into(), more: 0 }), rich: None } })
    };
    vec![
        ListItem::Group { id: "r1".into(), count: 2 },
        member("4194304", "frsty", &["r1"], Presence_::Dnd, None, false),
        member("20971520", "Jockie Music", &["r1"], Presence_::Online, Some((ActivityKind::Custom, "m!help")), true),
        ListItem::Group { id: "online".into(), count: 3 },
        member("12582912", "abyss reverie", &["r2"], Presence_::Online, Some((ActivityKind::Playing, "Apex Legends")), false),
        member("16777216", "ani khet", &[], Presence_::Idle, Some((ActivityKind::Listening, "Hozier")), false),
        member("8388608", "Zoë", &["r2"], Presence_::Online, None, false),
        ListItem::Group { id: "offline".into(), count: 1 },
        member("25165824", "a member whose display name runs long", &[], Presence_::Offline, None, false),
    ]
}

fn messages() -> Vec<Message> {
    let message = |id: &str, author: User, roles: &[&str], ts: &str, content: &str| Message {
        id: id.into(),
        channel_id: "general".into(),
        guild_id: Some("1".into()),
        author,
        roles: Some(roles.iter().map(|r| r.to_string()).collect()),
        content: content.into(),
        timestamp: ts.into(),
        ..Default::default()
    };
    let me = user("4194304", "frsty");
    let mut abyss = user("12582912", "abyss reverie");
    abyss.tag = Some(GuildTag { text: "FNC".into(), badge: None });
    let zoe = user("8388608", "Zoë");
    let mut first = message("101", abyss.clone(), &["r2"], "2026-11-13T17:18:00Z", "Morning! Anyone tried the new **gpui** list? It virtualises rows, so long channels stay cheap.");
    first.reactions = vec![Reaction { emoji: Emoji::unicode("🔥"), count: 3, me: true }, Reaction { emoji: Emoji::unicode("👀"), count: 1, me: false }];
    let mut image = message("102", zoe.clone(), &["r2"], "2026-11-13T17:20:00Z", "look at this");
    image.media = vec![
        Media { url: avatar_url("3", None), width: 256., height: 256., kind: MediaKind::Image },
        Media { url: "https://example.com/clip.mp4".into(), width: 256., height: 256., kind: MediaKind::Video { poster: avatar_url("4", None) } },
    ];
    image.embeds = vec![Embed {
        url: Some("https://www.rust-lang.org".into()),
        color: Some(0xCE422B),
        provider: Some("rust-lang.org".into()),
        title: Some("Rust Programming Language".into()),
        description: Some("A language empowering everyone to build reliable and efficient software.".into()),
        thumbnail: Some(Media { url: avatar_url("1", None), width: 80., height: 80., kind: MediaKind::Image }),
        ..Default::default()
    }, Embed {
        color: Some(0x43B581),
        author: Some("_frsty".into()),
        author_icon: Some(avatar_url("4194304", None)),
        description: Some("**<@8388608> joined voice channel <#lounge>**".into()),
        footer: Some("ID: 8388608".into()),
        timestamp: Some("2026-11-14T08:12:00Z".into()),
        ..Default::default()
    }];
    first_thread_chip(&mut image);
    let mut reply = message("103", me.clone(), &["r1"], "2026-11-14T08:15:00Z", "discord on rust <@8388608>, check <#media>");
    reply.reply_to = Some("101".into());
    reply.reply = Some(Box::new(first.clone()));
    reply.mentions = vec![zoe.clone()];
    reply.edited = true;
    let mut mention = message("104", zoe.clone(), &["r2"], "2026-11-14T08:17:00Z", "<@4194304> it already feels faster than the real one");
    mention.mentions = vec![me.clone()];
    vec![
        first,
        image,
        reply,
        mention,
        message("105", abyss, &["r2"], "2026-11-14T08:18:00Z", "```rust\nfn main() {\n    println!(\"hi\");\n}\n```"),
    ]
}

fn me_user() -> User {
    user("4194304", "frsty")
}

fn account(user: &User, status: Presence_) -> crate::accounts::Account {
    crate::accounts::Account { id: user.id.clone(), name: user.name.clone(), avatar: user.avatar.clone(), status }
}
