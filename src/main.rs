mod auth;
mod cache;
mod captcha;
mod call;
mod channel_menu;
mod chat;
#[cfg(debug_assertions)]
mod demo;
mod discord;
mod drag;
mod icons;
mod layout;
mod login;
mod mascot;
mod material;
mod forum;
mod gif;
mod home;
mod overlays;
mod menu;
mod message_menu;
mod motion;
mod palette;
mod notify;
mod panel;
mod rail_drag;
mod sidebar;
mod search;
mod settings;
mod user_menu;
mod hotkeys;
mod sounds;
mod voice;
mod window_controls;

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    path::PathBuf,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};

use discord::{Channel, ChannelKind, Client, Emoji, Event, Folder, Gif, Guild, ListItem, ListOp, Media, Message, Presence, Profile, ReadState, Relationship, SendState, User};
use futures::StreamExt;
use gpui_kit::{
    AppContext, Bounds, Context, Entity, FocusHandle, Hsla, RenderImage, ListAlignment, ListOffset, ListState, SharedString, Task, TitlebarOptions, Window, WindowBounds,
    WindowOptions,
    component::input::{InputEvent, InputState as SearchState, TextareaState},
    px, rgb, size,
};
use gpui_motion_kit::Motions;
use icons::Assets;
use login::Login;

const TYPING_TTL: Duration = Duration::from_secs(10);
const TYPING_SEND_EVERY: Duration = Duration::from_secs(8);

fn main() {
    gpui_kit::application().with_assets(Assets).run(move |cx| {
        gpui_kit::init(cx);
        cx.text_system()
            .add_fonts(vec![
                std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/Figtree-Regular.ttf")),
                std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/Figtree-Medium.ttf")),
                std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/Figtree-SemiBold.ttf")),
                std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/Figtree-Bold.ttf")),
            ])
            .expect("bundled Figtree");
        cx.set_http_client(std::sync::Arc::new(discord::Images::default()));
        cx.set_reduce_motion(material::system_reduce_motion());
        material::set_app_icon();
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(1280.), px(820.)), cx))),
            window_min_size: Some(size(px(940.), px(520.))),
            titlebar: Some(TitlebarOptions { title: Some("Rustcord".into()), appears_transparent: true, ..Default::default() }),
            window_background: material::window_background(),
            ..Default::default()
        };
        let handle = gpui_kit::open_window(options, cx, |window, cx| {
            palette::sync(window, cx);
            cx.new(|cx| Rustcord::new(window, cx))
        })
        .expect("open window");
        if material::NATIVE_SIDEBAR {
            // gpui-component's root paints the theme background over the whole window, hiding the native
            // material; Root's own style is applied last, so it can clear that.
            handle
                .0
                .downcast::<gpui_kit::base::Root>()
                .expect("kit root")
                .update(cx, |root, _, _| {
                    use gpui_kit::{Refineable as _, prelude::*};
                    root.style().refine(&gpui_kit::StyleRefinement::default().bg(gpui_kit::transparent_black()));
                })
                .expect("clear root background");
        }
        cx.activate(true);
    });
}

enum Status {
    SignedOut(Login),
    Connecting,
    Online,
    Reconnecting(SharedString),
}

// One channel's messages, kept after you leave it so coming back is instant.
#[derive(Default)]
struct Feed {
    messages: Vec<Message>,
    loaded: bool,
    fetching: bool,
    has_more: bool,
    loading_older: bool,
    error: Option<SharedString>,
}

enum RightPanel {
    Members,
    Search { query: SharedString, results: Option<Result<Vec<Message>, SharedString>> },
    Hidden,
}

#[derive(Clone)]
struct Typer {
    user_id: String,
    name: String,
    until: Instant,
}

#[derive(Clone)]
struct Suggestion {
    label: String,
    insert: String,
    // The wire form substituted on send: `<@id>` or `<#id>`.
    wire: String,
    avatar: Option<String>,
}

struct Autocomplete {
    token: String,
    items: Vec<Suggestion>,
    selected: usize,
}

// Where Rustcord keeps its files: Application Support on macOS, %APPDATA% on Windows,
// $XDG_DATA_HOME (or ~/.local/share) elsewhere.
fn data_dir() -> Option<std::path::PathBuf> {
    let var = |name| std::env::var_os(name).map(std::path::PathBuf::from);
    let dir = if cfg!(target_os = "macos") {
        var("HOME")?.join("Library/Application Support/Rustcord")
    } else if cfg!(target_os = "windows") {
        var("APPDATA")?.join("Rustcord")
    } else {
        var("XDG_DATA_HOME").or_else(|| var("HOME").map(|h| h.join(".local/share")))?.join("rustcord")
    };
    Some(dir)
}

// The quietest mic level the meters show, in dBFS.
const SELF_VIEW_INTERVAL: Duration = Duration::from_millis(66);
const SILENCE_DB: f32 = -100.;

// A voice channel we're in. Dropping it leaves the channel.
struct Call {
    session: voice::VoiceSession,
    guild_id: Option<String>,
    channel: Channel,
    connected: bool,
    stage: voice::Stage,
    stats: Option<voice::Stats>,
    // Our processed mic level and whether the voice gate is open.
    level: (f32, bool),
    speaking: HashSet<String>,
    // When each person last stopped talking, for Discord's tile order.
    last_spoke: HashMap<String, Instant>,
    // Latest camera frame per user, ours included.
    cameras: HashMap<String, Arc<RenderImage>>,
    camera_on: bool,
    // Go Lives we're watching, in the order we started them.
    watching: Vec<Watching>,
    // The stream shown large in Focus layout; the first watched when unset.
    focus: Option<String>,
    stream_layout: crate::call::StreamLayout,
    // A watched stream that just ended, so the stage can say so instead of going blank.
    ended: Option<String>,
    // Our own Go Live; dropping it ends the stream.
    sharing: Option<voice::GoLive>,
    // What we're sharing, as the Go Live panel names it ("Screen 1", an app, a window).
    share_label: Option<String>,
    // The source behind `sharing`, kept so a quality change can restart the same stream.
    share_source: Option<voice::ShareSource>,
    live_since: Option<Instant>,
    // Our own Go Live from its local frames, for the panel thumbnail and the self-view.
    self_view: Option<(Arc<RenderImage>, Instant)>,
    _events: Task<()>,
}

impl Call {
    fn watched(&self, streamer_id: &str) -> Option<&Watching> {
        self.watching.iter().find(|w| w.streamer_id == streamer_id)
    }

    // The stream shown large: the focused one, else the first watched.
    fn focused(&self) -> Option<&Watching> {
        self.focus.as_deref().and_then(|id| self.watched(id)).or(self.watching.first())
    }

    fn unwatch(&mut self, streamer_id: &str) -> Option<Watching> {
        let at = self.watching.iter().position(|w| w.streamer_id == streamer_id)?;
        if self.focus.as_deref() == Some(streamer_id) {
            self.focus = None;
        }
        Some(self.watching.remove(at))
    }
}

// Someone's Go Live we're watching. Dropping it stops the stream.
pub(crate) struct Watching {
    pub(crate) streamer_id: String,
    pub(crate) frame: Option<Arc<RenderImage>>,
    // When recent frames arrived, for the header's health dot.
    pub(crate) arrivals: std::collections::VecDeque<Instant>,
    // None for our own Go Live, shown from its local frames: watching it through Discord would
    // end it on stop, since leaving a stream and ending one are the same gateway op.
    _watch: Option<voice::StreamWatch>,
}

// A profile popout opened from an avatar or name, at the click point.
struct ProfileCard {
    user: User,
    guild_id: Option<String>,
    at: gpui_kit::Point<gpui_kit::Pixels>,
    profile: Option<Profile>,
}

// Where a picked emoji goes.
#[derive(Clone)]
enum EmojiTarget {
    Composer,
    Reaction(Box<Message>),
}

// The gallery: every image and video in the open channel, paged with the arrow keys.
struct Viewer {
    items: Vec<Media>,
    index: usize,
}

#[derive(Clone, Copy, PartialEq)]
enum Edge {
    Channels,
    Panel,
}

struct Drag {
    edge: Edge,
    start_x: f32,
    start_width: f32,
}

#[derive(Clone, PartialEq, Eq, Hash)]
enum Part {
    Pill(String),
    Fold(String),
    Chevron(String),
    Banner,
}

type ComposerState = TextareaState;

pub struct Rustcord {
    status: Status,
    client: Option<Client>,
    _session: Option<Task<()>>,
    me: Option<User>,
    my_status: discord::Status,
    custom_status: Option<String>,
    guilds: Vec<Guild>,
    folders: Vec<Folder>,
    open_folders: HashSet<String>,
    rail_drop: Option<(String, rail_drag::Drop)>,
    // Where a dragged channel would land: the row's id and above/below it.
    channel_drop: Option<(String, drag::Place)>,
    // Every user seen as their server shows them, keyed by guild then user, for voice rows and tiles.
    members: HashMap<String, HashMap<String, User>>,
    dms: Vec<Channel>,
    // None shows direct messages.
    guild_id: Option<String>,
    channel: Option<Channel>,
    last_channel: HashMap<Option<String>, String>,
    feeds: HashMap<String, Feed>,
    list: ListState,
    read_states: HashMap<String, ReadState>,
    last_message: HashMap<String, u64>,
    presences: HashMap<String, Presence>,
    member_roles: HashMap<String, HashMap<String, Vec<String>>>,
    member_lists: HashMap<String, Vec<ListItem>>,
    member_groups: HashMap<String, HashMap<String, u32>>,
    requested_members: HashSet<(String, String)>,
    typing: HashMap<String, Vec<Typer>>,
    last_typing_sent: Option<Instant>,
    right_panel: RightPanel,
    composer: Entity<ComposerState>,
    search_input: Entity<SearchState>,
    has_draft: bool,
    send_error: Option<SharedString>,
    replying: Option<Message>,
    // Discord's "@ ON / OFF" on the reply bar: whether the reply pings its author.
    reply_ping: bool,
    // Files picked, pasted or dropped, waiting to go out with the next message.
    pending_files: Vec<PathBuf>,
    talk_key_down: bool,
    prefs: settings::Prefs,
    settings: Option<settings::SettingsView>,
    settings_search: Entity<SearchState>,
    hotkeys: Option<hotkeys::Hotkeys>,
    // Keybinds the system refused because another app holds them.
    keybind_errors: Vec<String>,
    // Our mic level in dBFS, from the call or the mic test.
    mic_level: f32,
    mic_test: Option<(voice::MicTest, Task<()>)>,
    // Each slider's last painted bounds, so a click or drag maps to a value.
    slider_bounds: Rc<RefCell<HashMap<SharedString, gpui_kit::Bounds<gpui_kit::Pixels>>>>,
    // The open voice channel's call view replaces the chat.
    call_view: bool,
    connection_info: bool,
    device_menu: Option<call::DeviceMenu>,
    menu: Option<menu::OpenMenu>,
    // Go Live thumbnails by streamer: the CDN url and when it was fetched.
    stream_previews: HashMap<String, (String, Instant)>,
    // Forum channels' archived posts and post previews, by forum id.
    forums: HashMap<String, forum::ForumState>,
    developer_mode: bool,
    // Notification settings per server, "@me" for DMs.
    guild_settings: HashMap<String, discord::GuildSettings>,
    ignored: HashSet<String>,
    // A DM call we started: ring them once the call connects.
    ring_on_connect: Option<String>,
    // Voice channels whose member list is collapsed (Discord's "Hide Names").
    hidden_voice_names: HashSet<String>,
    // Shift held: Discord's hover bar shows Delete instead of More.
    shift_held: bool,
    // The stream hover card under the pointer, which keeps it open off the row.
    stream_card_hover: Option<String>,
    // A stream to start watching as soon as the call we're joining connects.
    watch_on_connect: Option<String>,
    // The window is in front; notifications for the open channel are skipped then.
    window_active: bool,
    dialog: Option<menu::Dialog>,
    dialog_input: Entity<SearchState>,
    // A short notice at the bottom, e.g. why a moderation action failed.
    toast: Option<SharedString>,
    // The Go Live picker: None closed, Some(None) loading, Some(Some(..)) listed.
    share_picker: Option<overlays::SharePicker>,
    // The source picked in the Go Live window, waiting for "Go Live".
    share_choice: Option<voice::ShareSource>,
    share_choice_label: Option<String>,
    // Who watches each Go Live we've heard of, by stream key (STREAM_UPDATE).
    stream_viewers: HashMap<String, Vec<String>>,
    // When we first saw each Go Live by stream key; Discord sends no start time.
    stream_live_since: HashMap<String, Instant>,
    // When the pointer last moved over the call view; the bar and header fade out after a while.
    call_pointer_at: Instant,
    call_strip_hidden: bool,
    // Our stream's self-view: shown, and which corner of the stage it sits in.
    pip_shown: bool,
    pip_corner: crate::call::Corner,
    pip_drag: Option<crate::call::PipDrag>,
    // Our local volume for each streamer's Go Live (0..=2), kept across watches.
    stream_volumes: HashMap<String, f32>,
    // Where a released self-view glides from, and when it let go.
    pip_snap: Option<((f32, f32), Instant)>,
    // Bumped on each Go Live so an earlier stream's preview uploads stop.
    preview_generation: u64,
    // The call view filling the screen on its own, as Discord's full screen does.
    call_fullscreen: bool,
    // The voice channel's own chat, open beside the call.
    call_chat: bool,
    // Voice channel → when its current session began (Unix seconds), from CHANNEL_INFO.
    voice_start: HashMap<String, u64>,
    // Servers whose channel info we've asked for since connecting.
    channel_info_requested: HashSet<String>,
    // Each call participant's avatar colour for their tile; None while loading or when it failed.
    avatar_colors: HashMap<String, Option<Hsla>>,
    pins: Option<Option<Result<Vec<Message>, SharedString>>>,
    // Messages whose spoilers were clicked open.
    revealed: Rc<RefCell<HashSet<String>>>,
    relationships: Vec<Relationship>,
    friends_tab: home::FriendsTab,
    inbox: Option<Option<Result<Vec<Message>, SharedString>>>,
    // The GIF picker: None closed, Some(None) loading, Some(Some(..)) results.
    gif_picker: Option<Option<Result<Vec<Gif>, SharedString>>>,
    gif_search: Entity<SearchState>,
    // The channel open when Rustcord last quit, reopened once the gateway is ready.
    restore: Option<(Option<String>, String)>,
    editing: Option<Message>,
    mention_wire: HashMap<String, String>,
    autocomplete: Option<Autocomplete>,
    flash: Option<String>,
    muted: bool,
    deafened: bool,
    call: Option<Call>,
    voice_error: Option<SharedString>,
    viewer: Option<Viewer>,
    profile_card: Option<ProfileCard>,
    status_menu: bool,
    emoji_picker: Option<EmojiTarget>,
    emoji_search: Entity<SearchState>,
    switcher: Option<usize>,
    switcher_input: Entity<SearchState>,
    // The read position when the open channel was opened: messages after it sit under the NEW line.
    unread_marker: Option<u64>,
    // Scrolled well above the newest message: shows Discord's "Jump to Present" bar.
    scrolled_up: bool,
    copied: Option<String>,
    focus: FocusHandle,
    channels_width: f32,
    panel_width: f32,
    drag: Option<Drag>,
    back: Vec<(Option<String>, Channel)>,
    forward: Vec<(Option<String>, Channel)>,
    folded: HashSet<String>,
    hovered: Option<String>,
    motions: Motions<Part>,
    // Live messages pop in; history never animates. Shared with the list's row closure.
    arrivals: Rc<RefCell<Arrivals>>,
}

#[derive(Default)]
struct Arrivals {
    fresh: HashSet<String>,
    motions: Motions<String>,
}

impl Rustcord {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let composer = cx.new(|cx| {
            let mut state = ComposerState::new(window, cx).placeholder("Message").submit_on_enter(true);
            state.set_auto_grow(1, 10, cx);
            state
        });
        cx.subscribe_in(&composer, window, Self::on_composer).detach();
        let search_input = cx.new(|cx| SearchState::new(window, cx).placeholder("Search"));
        cx.subscribe_in(&search_input, window, Self::on_search_input).detach();
        cx.observe_window_appearance(window, |_, window, cx| palette::sync(window, cx)).detach();
        cx.observe_window_activation(window, |this: &mut Self, window, cx| {
            this.window_active = window.is_window_active();
            cx.notify();
        })
        .detach();
        // Typing filters the picker and switcher; the views read the inputs' text on render.
        let emoji_search = cx.new(|cx| SearchState::new(window, cx).placeholder("Find the perfect emoji"));
        cx.subscribe_in(&emoji_search, window, Self::on_emoji_search).detach();
        let switcher_input = cx.new(|cx| SearchState::new(window, cx).placeholder("Where would you like to go?"));
        let gif_search = cx.new(|cx| SearchState::new(window, cx).placeholder("Search Tenor"));
        cx.subscribe_in(&gif_search, window, Self::on_gif_search).detach();
        cx.subscribe_in(&switcher_input, window, Self::on_switcher_input).detach();
        let settings_search = cx.new(|cx| SearchState::new(window, cx).placeholder("Search"));
        let dialog_input = cx.new(|cx| SearchState::new(window, cx));
        cx.subscribe(&settings_search, |_, _, _: &InputEvent, cx| cx.notify()).detach();
        #[allow(unused_mut)]
        let mut chord = Self {
            status: Status::Connecting,
            client: None,
            _session: None,
            me: None,
            my_status: discord::Status::Online,
            custom_status: None,
            guilds: Vec::new(),
            folders: Vec::new(),
            open_folders: HashSet::new(),
            rail_drop: None,
            channel_drop: None,
            members: HashMap::new(),
            dms: Vec::new(),
            guild_id: None,
            channel: None,
            last_channel: HashMap::new(),
            feeds: HashMap::new(),
            list: ListState::new(0, ListAlignment::Bottom, px(600.)),
            read_states: HashMap::new(),
            last_message: HashMap::new(),
            presences: HashMap::new(),
            member_roles: HashMap::new(),
            member_lists: HashMap::new(),
            member_groups: HashMap::new(),
            requested_members: HashSet::new(),
            typing: HashMap::new(),
            last_typing_sent: None,
            right_panel: RightPanel::Members,
            composer,
            search_input,
            has_draft: false,
            send_error: None,
            replying: None,
            reply_ping: true,
            pending_files: Vec::new(),
            talk_key_down: false,
            prefs: settings::Prefs::load(),
            settings: None,
            settings_search,
            hotkeys: None,
            keybind_errors: Vec::new(),
            mic_level: SILENCE_DB,
            mic_test: None,
            slider_bounds: Rc::default(),
            call_view: false,
            connection_info: false,
            device_menu: None,
            menu: None,
            stream_previews: HashMap::new(),
            forums: HashMap::new(),
            developer_mode: false,
            guild_settings: HashMap::new(),
            ignored: HashSet::new(),
            ring_on_connect: None,
            hidden_voice_names: HashSet::new(),
            shift_held: false,
            stream_card_hover: None,
            watch_on_connect: None,
            window_active: true,
            dialog: None,
            dialog_input,
            toast: None,
            share_picker: None,
            share_choice: None,
            share_choice_label: None,
            stream_viewers: HashMap::new(),
            stream_live_since: HashMap::new(),
            call_pointer_at: Instant::now(),
            call_strip_hidden: false,
            preview_generation: 0,
            pip_shown: true,
            pip_corner: crate::call::Corner::TopRight,
            pip_drag: None,
            stream_volumes: HashMap::new(),
            pip_snap: None,
            call_fullscreen: false,
            call_chat: false,
            voice_start: HashMap::new(),
            channel_info_requested: HashSet::new(),
            avatar_colors: HashMap::new(),
            pins: None,
            revealed: Rc::default(),
            relationships: Vec::new(),
            friends_tab: home::FriendsTab::Online,
            inbox: None,
            gif_picker: None,
            gif_search,
            restore: home::load_last_channel(),
            editing: None,
            mention_wire: HashMap::new(),
            autocomplete: None,
            flash: None,
            muted: false,
            deafened: false,
            call: None,
            voice_error: None,
            viewer: None,
            profile_card: None,
            status_menu: false,
            emoji_picker: None,
            emoji_search,
            switcher: None,
            switcher_input,
            unread_marker: None,
            scrolled_up: false,
            copied: None,
            focus: cx.focus_handle(),
            channels_width: layout::CHANNELS_WIDTH,
            panel_width: layout::PANEL_WIDTH,
            drag: None,
            back: Vec::new(),
            forward: Vec::new(),
            folded: HashSet::new(),
            hovered: None,
            motions: Motions::new(),
            arrivals: Rc::default(),
        };
        let weak = cx.weak_entity();
        chord.list.set_scroll_handler(move |event, _, cx| {
            let away = event.visible_range.end + 4 < event.count;
            weak.update(cx, |this, cx| {
                if this.scrolled_up != away {
                    this.scrolled_up = away;
                    cx.notify();
                }
            })
            .ok();
        });
        chord.apply_prefs();
        #[cfg(debug_assertions)]
        if let Ok(scene) = std::env::var("CHORD_DEMO") {
            demo::load(&mut chord, &scene, window, cx);
            return chord;
        }
        chord.start_hotkeys(cx);
        match std::env::var("DISCORD_TOKEN").ok().filter(|t| !t.trim().is_empty()).or_else(auth::stored_token) {
            Some(token) => chord.connect(token, cx),
            None => chord.sign_in(cx),
        }
        chord
    }

    fn connect(&mut self, token: String, cx: &mut Context<Self>) {
        let (client, mut events) = discord::connect(token);
        self.client = Some(client);
        self.status = Status::Connecting;
        self._session = Some(cx.spawn(async move |this, cx| {
            while let Some(event) = events.next().await {
                if this.update(cx, |this, cx| this.on_event(event, cx)).is_err() {
                    break;
                }
            }
        }));
        cx.notify();
    }

    fn sign_in(&mut self, cx: &mut Context<Self>) {
        self.client = None;
        self.me = None;
        self.channel = None;
        self.guilds.clear();
        self.dms.clear();
        self.feeds.clear();
        self.settings = None;
        self.status = Status::SignedOut(Login::Preparing);
        let mut events = auth::sign_in();
        // Replacing the task drops the gateway's receiver, which stops the old session.
        self._session = Some(cx.spawn(async move |this, cx| {
            while let Some(event) = events.next().await {
                if this.update(cx, |this, cx| this.on_auth(event, cx)).is_err() {
                    break;
                }
            }
        }));
        cx.notify();
    }

    fn sign_out(&mut self, cx: &mut Context<Self>) {
        auth::forget_token();
        self.sign_in(cx);
    }

    fn on_auth(&mut self, event: auth::AuthEvent, cx: &mut Context<Self>) {
        let login = match event {
            auth::AuthEvent::SignedIn(token) => {
                auth::store_token(&token);
                return self.connect(token, cx);
            }
            auth::AuthEvent::Qr(url) => Login::qr(&url),
            auth::AuthEvent::Scanned(user) => Login::Scanned(user),
            auth::AuthEvent::Captcha(url) => {
                cx.open_url(&url);
                Login::Captcha(url.into())
            }
            auth::AuthEvent::Failed(error) => Login::Failed(error.into()),
        };
        self.status = Status::SignedOut(login);
        cx.notify();
    }

    pub fn on_event(&mut self, event: Event, cx: &mut Context<Self>) {
        match event {
            Event::Ready(ready) => {
                let ready = *ready;
                self.channel_info_requested.clear();
                self.me = Some(ready.me);
                self.my_status = ready.status;
                self.custom_status = ready.custom_status;
                for channel in ready.guilds.iter().flat_map(|g| &g.channels).chain(&ready.dms) {
                    if let Some(id) = channel.last_message_id.as_deref().and_then(|id| id.parse().ok()) {
                        self.last_message.insert(channel.id.clone(), id);
                    }
                }
                if cfg!(debug_assertions) && std::env::var_os("RUSTCORD_STREAM_DEBUG").is_some() {
                    for guild in &ready.guilds {
                        let (voice, live) = (guild.voice_states.len(), guild.voice_states.iter().filter(|v| v.streaming).count());
                        if voice > 0 {
                            eprintln!("ready guild {}: {voice} in voice, {live} streaming", guild.id);
                        }
                    }
                }
                // Streams already live when we connect: their start is unknown, so count from now.
                for guild in &ready.guilds {
                    for state in guild.voice_states.iter().filter(|v| v.streaming) {
                        if let Some(channel) = &state.channel_id {
                            self.stream_live_since.entry(format!("guild:{}:{channel}:{}", guild.id, state.user_id)).or_insert_with(Instant::now);
                        }
                    }
                }
                self.guilds = ready.guilds;
                self.folders = ready.folders;
                self.dms = ready.dms;
                self.dms.sort_by_key(|c| std::cmp::Reverse(self.last_message.get(&c.id).copied().unwrap_or(0)));
                self.read_states = ready.read_states.into_iter().collect();
                self.relationships = ready.relationships;
                self.developer_mode = ready.developer_mode;
                self.guild_settings = ready.guild_settings.into_iter().collect();
                self.ignored = ready.ignored.into_iter().collect();
                self.presences = ready.presences.into_iter().collect();
                self.status = Status::Online;
            }
            Event::Batch(events) => {
                for event in events {
                    self.on_event(event, cx);
                }
            }
            Event::ChannelUpsert { guild_id, channel } => {
                let Some(guild) = self.guilds.iter_mut().find(|g| g.id == guild_id) else { return };
                let list = if channel.kind == ChannelKind::Thread { &mut guild.threads } else { &mut guild.channels };
                match list.iter_mut().find(|c| c.id == channel.id) {
                    Some(existing) => {
                        // THREAD_UPDATE carries no membership: keep whether we'd joined.
                        let joined = existing.thread.as_ref().is_some_and(|t| t.joined);
                        *existing = channel;
                        if let Some(thread) = existing.thread.as_mut() {
                            thread.joined |= joined;
                        }
                    }
                    None => list.push(channel),
                }
                guild.channels.sort_by_key(|c| c.position);
            }
            Event::ChannelDelete { guild_id, id } => {
                if let Some(guild) = self.guilds.iter_mut().find(|g| g.id == guild_id) {
                    guild.channels.retain(|c| c.id != id);
                    guild.threads.retain(|c| c.id != id);
                }
                if self.channel.as_ref().is_some_and(|c| c.id == id) {
                    self.channel = None;
                }
            }
            Event::MemberUpdate { guild_id, user, roles } => {
                if self.me.as_ref().is_some_and(|me| me.id == user.id)
                    && let Some(guild) = self.guilds.iter_mut().find(|g| g.id == guild_id)
                {
                    guild.my_roles = roles.clone();
                }
                self.member_roles.entry(guild_id.clone()).or_default().insert(user.id.clone(), roles);
                self.members.entry(guild_id.clone()).or_default().insert(user.id.clone(), user);
                self.apply_members(&guild_id);
            }
            Event::RelationshipChange { user_id, relationship } => {
                // An update may arrive without the user object; keep the one we have.
                let known = self.relationships.iter().find(|r| r.user.id == user_id).map(|r| r.user.clone());
                self.relationships.retain(|r| r.user.id != user_id);
                if let Some(mut relationship) = relationship {
                    if relationship.user.name.is_empty()
                        && let Some(user) = known
                    {
                        relationship.user = user;
                    }
                    self.relationships.push(relationship);
                }
            }
            Event::ThreadJoined { id, joined } => {
                for thread in self.guilds.iter_mut().flat_map(|g| g.threads.iter_mut()).filter(|t| t.id == id) {
                    if let Some(meta) = thread.thread.as_mut() {
                        meta.joined = joined;
                    }
                }
            }
            Event::GuildSettings((key, settings)) => {
                self.guild_settings.insert(key, settings);
            }
            Event::GuildCreate(mut guild) => {
                // GUILD_CREATE carries no member entry for us: keep the roles we know (READY, or
                // GUILD_MEMBER_UPDATE), or take them from the member cache, so permissions hold.
                let me = self.me.as_ref().map(|m| m.id.clone()).unwrap_or_default();
                let known_roles = self.member_roles.get(&guild.id).and_then(|r| r.get(&me)).cloned();
                match self.guilds.iter_mut().find(|g| g.id == guild.id) {
                    Some(existing) => {
                        if guild.my_roles.is_empty() {
                            guild.my_roles = std::mem::take(&mut existing.my_roles);
                        }
                        for thread in existing.threads.drain(..) {
                            if !guild.threads.iter().any(|t| t.id == thread.id) {
                                guild.threads.push(thread);
                            }
                        }
                        *existing = guild;
                    }
                    None => {
                        if guild.my_roles.is_empty() {
                            guild.my_roles = known_roles.unwrap_or_default();
                        }
                        self.guilds.push(guild);
                    }
                }
            }
            Event::MessageCreate(message) => self.on_message(*message, cx),
            Event::MessageUpdate(patch) => {
                let (channel_id, id) = (patch["channel_id"].as_str().unwrap_or_default(), patch["id"].as_str().unwrap_or_default());
                if let Some(m) = self.feeds.get_mut(channel_id).and_then(|f| f.messages.iter_mut().find(|m| m.id == id)) {
                    discord::apply_update(m, &patch);
                }
            }
            Event::MessageDelete { channel_id, id } => self.remove_message(&channel_id, &id),
            Event::Reaction { channel_id, message_id, user_id, emoji, added } => {
                let mine = self.me.as_ref().is_some_and(|me| me.id == user_id);
                if let Some(m) = self.message_mut(&channel_id, &message_id) {
                    adjust_reaction(m, &emoji, added, mine, true);
                }
            }
            Event::Typing { channel_id, user_id, name } => self.on_typing(channel_id, user_id, name, cx),
            Event::Presence { user_id, presence } => {
                self.presences.insert(user_id, presence);
            }
            Event::MemberList { guild_id, groups, ops } => {
                if !groups.is_empty() {
                    self.member_groups.insert(guild_id.clone(), groups.into_iter().collect());
                }
                let list = self.member_lists.entry(guild_id.clone()).or_default();
                for op in ops {
                    apply_list_op(list, op);
                }
                let roles = self.member_roles.entry(guild_id.clone()).or_default();
                let known = self.members.entry(guild_id.clone()).or_default();
                for item in list.iter() {
                    if let ListItem::Member(m) = item {
                        known.insert(m.user.id.clone(), m.user.clone());
                        roles.insert(m.user.id.clone(), m.roles.clone());
                        self.presences.insert(m.user.id.clone(), m.presence.clone());
                    }
                }
                self.apply_members(&guild_id);
            }
            Event::MembersChunk { guild_id, members } => {
                let roles = self.member_roles.entry(guild_id.clone()).or_default();
                let known = self.members.entry(guild_id.clone()).or_default();
                for (user, member_roles) in members {
                    roles.insert(user.id.clone(), member_roles);
                    known.insert(user.id.clone(), user);
                }
                self.apply_members(&guild_id);
            }
            Event::Ack { channel_id, message_id } => {
                let read = self.read_states.entry(channel_id).or_default();
                read.last_read = read.last_read.max(message_id);
                read.mentions = 0;
            }
            Event::VoiceState(state) => {
                let ours = self.call.as_ref().map(|c| c.channel.id.clone());
                let me = self.me.as_ref().map(|m| m.id.clone());
                let Some(guild) = state.guild_id.as_ref().and_then(|id| self.guilds.iter_mut().find(|g| &g.id == id)) else { return };
                let was_here = guild.voice_states.iter().any(|v| v.user_id == state.user_id && v.channel_id == ours);
                let is_here = ours.is_some() && state.channel_id == ours;
                let was_live = guild.voice_states.iter().any(|v| v.user_id == state.user_id && v.channel_id == ours && v.streaming);
                let is_live = is_here && state.streaming;
                if Some(&state.user_id) != me.as_ref() && was_here != is_here {
                    sounds::play(if is_here { sounds::Cue::UserJoin } else { sounds::Cue::UserLeave });
                } else if Some(&state.user_id) != me.as_ref() && !was_live && is_live {
                    sounds::play(sounds::Cue::StreamStarted);
                } else if was_live && !is_live && self.call.as_ref().is_some_and(|c| c.watched(&state.user_id).is_some()) {
                    sounds::play(sounds::Cue::StreamEnded);
                }
                let live_key = state.channel_id.as_ref().filter(|_| state.streaming).map(|c| format!("guild:{}:{c}:{}", guild.id, state.user_id));
                let suffix = format!(":{}", state.user_id);
                self.stream_live_since.retain(|key, _| !key.ends_with(&suffix) || Some(key) == live_key.as_ref());
                if let Some(key) = live_key {
                    self.stream_live_since.entry(key).or_insert_with(Instant::now);
                }
                guild.voice_states.retain(|v| v.user_id != state.user_id);
                if state.channel_id.is_some() {
                    guild.voice_states.push(state);
                }
            }
            Event::VoiceServer(_) => {}
            Event::VoiceStartTimes { guild_id, times, all } => {
                if all
                    && let Some(guild) = self.guilds.iter().find(|g| g.id == guild_id)
                {
                    for channel in &guild.channels {
                        self.voice_start.remove(&channel.id);
                    }
                }
                for (channel, start) in times {
                    match start {
                        Some(start) => self.voice_start.insert(channel, start),
                        None => self.voice_start.remove(&channel),
                    };
                }
            }
            Event::StreamViewers { key, viewers } => {
                let before = self.stream_viewers.get(&key).map_or(0, Vec::len);
                if self.in_stream(&key)
                    && let Some(cue) = viewer_cue(before, viewers.len())
                {
                    sounds::play(cue);
                }
                self.stream_viewers.insert(key, viewers);
            }
            Event::Disconnected(reason) => self.status = Status::Reconnecting(reason.into()),
            Event::AuthFailed => {
                auth::forget_token();
                return self.sign_in(cx);
            }
        }
        self.resolve_voice_users();
        self.refresh_stream_previews(cx);
        self.refresh_avatar_colors(cx);
        self.update_badge();
        cx.notify();
    }

    // READY's voice states carry only user ids: fill them from members we know, and ask the
    // gateway once for the rest so every voice row and tile shows the real name and avatar.
    fn resolve_voice_users(&mut self) {
        let mut missing: Vec<(String, String)> = Vec::new();
        for guild in &mut self.guilds {
            let known = self.members.get(&guild.id);
            for state in guild.voice_states.iter_mut().filter(|v| v.user.is_none()) {
                match known.and_then(|k| k.get(&state.user_id)) {
                    Some(user) => state.user = Some(user.clone()),
                    None if self.requested_members.insert((guild.id.clone(), state.user_id.clone())) => missing.push((guild.id.clone(), state.user_id.clone())),
                    None => {}
                }
            }
        }
        let Some(client) = &self.client else { return };
        let mut by_guild: HashMap<String, Vec<String>> = HashMap::new();
        for (guild_id, user_id) in missing {
            by_guild.entry(guild_id).or_default().push(user_id);
        }
        for (guild_id, ids) in by_guild {
            for chunk in ids.chunks(100) {
                client.request_members(&guild_id, chunk.to_vec());
            }
        }
    }

    fn on_message(&mut self, message: Message, cx: &mut Context<Self>) {
        let channel_id = message.channel_id.clone();
        let mine = self.me.as_ref().is_some_and(|me| me.id == message.author.id);
        let is_open = self.channel.as_ref().is_some_and(|c| c.id == channel_id);
        if let Ok(id) = message.id.parse() {
            self.last_message.insert(channel_id.clone(), id);
        }
        if let (Some(guild_id), Some(roles)) = (&message.guild_id, &message.roles) {
            self.member_roles.entry(guild_id.clone()).or_default().insert(message.author.id.clone(), roles.clone());
        }
        if let Some(typers) = self.typing.get_mut(&channel_id) {
            typers.retain(|t| t.user_id != message.author.id);
        }
        let is_dm = self.dms.iter().any(|d| d.id == channel_id);
        if !mine && !is_open && (self.mentions_me(&message) || is_dm) {
            self.read_states.entry(channel_id.clone()).or_default().mentions += 1;
        }
        let kind = self.notify_kind(&message, is_open && self.window_active);
        if kind != notify::Notify::None {
            sounds::play(if is_dm { sounds::Cue::Message } else { sounds::Cue::Mention });
            if !self.window_active || !is_open {
                self.notify(&message);
            }
        }
        if let Some(pos) = self.dms.iter().position(|d| d.id == channel_id) {
            let dm = self.dms.remove(pos);
            self.dms.insert(0, dm);
        }
        let Some(feed) = self.feeds.get_mut(&channel_id) else { return };
        // Our own optimistic copy, sent with this nonce: swap in the real one in place.
        let nonce = message.nonce.clone();
        if let Some(pending) = feed.messages.iter_mut().find(|m| m.state != SendState::Sent && m.nonce.is_some() && m.nonce == nonce) {
            *pending = message;
            return;
        }
        if feed.messages.iter().any(|m| m.id == message.id) {
            return;
        }
        let at = feed.messages.len();
        feed.messages.push(message.clone());
        if is_open {
            self.arrivals.borrow_mut().fresh.insert(message.id);
            self.list.splice(at..at, 1);
            self.ack_open(cx);
        }
    }

    fn on_typing(&mut self, channel_id: String, user_id: String, name: Option<String>, cx: &mut Context<Self>) {
        if self.me.as_ref().is_some_and(|me| me.id == user_id) {
            return;
        }
        let name = name.unwrap_or_else(|| self.user_name(&user_id));
        let typers = self.typing.entry(channel_id).or_default();
        typers.retain(|t| t.user_id != user_id);
        typers.push(Typer { user_id, name, until: Instant::now() + TYPING_TTL });
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(TYPING_TTL).await;
            this.update(cx, |_, cx| cx.notify()).ok();
        })
        .detach();
    }

    fn open_guild(&mut self, guild_id: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        self.guild_id = guild_id.clone();
        self.refresh_stream_previews(cx);
        self.request_channel_info();
        let remembered = self.last_channel.get(&guild_id).cloned();
        // In sidebar order, and only channels we can open (Discord also lists hidden ones).
        let channels: Vec<Channel> = match &guild_id {
            Some(_) => self.guild().map(|g| sidebar::channel_sections(g, self.my_id()).into_iter().flat_map(|(_, c)| c).collect()).unwrap_or_default(),
            None => self.dms.clone(),
        };
        let target = remembered
            .and_then(|id| channels.iter().find(|c| c.id == id).cloned())
            .or_else(|| channels.into_iter().find(|c| matches!(c.kind, ChannelKind::Text | ChannelKind::Dm)));
        match target {
            Some(channel) => self.open_channel(channel, window, cx),
            None => {
                self.channel = None;
                cx.notify();
            }
        }
    }

    fn open_channel(&mut self, channel: Channel, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(current) = self.channel.clone().filter(|c| c.id != channel.id) {
            self.back.push((self.guild_id.clone(), current));
            self.forward.clear();
        }
        self.show_channel(channel, window, cx);
    }

    /// Back/forward in the title bar, like a browser's history of channels.
    fn navigate(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let (from, to) = if forward { (&mut self.forward, &mut self.back) } else { (&mut self.back, &mut self.forward) };
        let Some((guild_id, channel)) = from.pop() else { return };
        if let Some(current) = self.channel.clone() {
            to.push((self.guild_id.clone(), current));
        }
        self.guild_id = guild_id;
        self.show_channel(channel, window, cx);
    }

    fn show_channel(&mut self, channel: Channel, window: &mut Window, cx: &mut Context<Self>) {
        self.call_view = false;
        let placeholder = match channel.kind {
            ChannelKind::Dm => format!("Message @{}", channel.name),
            _ => format!("Message #{}", channel.name),
        };
        self.composer.update(cx, |state, cx| state.set_placeholder(placeholder, window, cx));
        let search = format!("Search {}", self.guild().map(|g| g.name.as_str()).unwrap_or("messages"));
        self.search_input.update(cx, |state, cx| state.set_placeholder(search, window, cx));
        self.last_channel.insert(self.guild_id.clone(), channel.id.clone());
        home::save_last_channel(self.guild_id.as_deref(), &channel.id);
        let id = channel.id.clone();
        if let (Some(client), Some(guild_id)) = (&self.client, &self.guild_id) {
            client.request_member_list(guild_id, &id);
        }
        // The channel we're leaving keeps what arrived while it was open.
        if let Some(previous) = self.channel.as_ref().filter(|c| c.id != id).and_then(|c| self.feeds.get(&c.id).map(|f| (c.id.clone(), f)))
            && previous.1.loaded
        {
            cache::save(&previous.0, &previous.1.messages);
        }
        if !self.feeds.contains_key(&id)
            && let Some(messages) = cache::load(&id)
        {
            self.feeds.insert(id.clone(), Feed { messages, loaded: true, has_more: true, ..Default::default() });
        }
        self.channel = Some(channel);
        self.replying = None;
        self.editing = None;
        self.send_error = None;
        self.autocomplete = None;
        // Cached channels show at once; every open also refreshes in the background.
        // Messages after this sit under Discord's red NEW line until the channel is left.
        self.unread_marker = self.unread(&id).then(|| self.read_states.get(&id).map(|r| r.last_read)).flatten();
        if self.channel.as_ref().is_some_and(|c| c.kind == ChannelKind::Forum) {
            if !self.forums.contains_key(&id) {
                self.load_forum(id, cx);
            }
            return cx.notify();
        }
        let count = self.feeds.get(&id).map_or(0, |f| f.messages.len());
        self.list.reset(count);
        self.fetch_latest(&id, cx);
        self.ack_open(cx);
        cx.notify();
    }

    /// Hovering a channel warms its cache so the click shows messages instantly.
    fn prefetch(&mut self, channel_id: &str, cx: &mut Context<Self>) {
        if self.feeds.get(channel_id).is_some_and(|f| f.loaded || f.fetching) {
            return;
        }
        self.fetch_latest(channel_id, cx);
    }

    fn fetch_latest(&mut self, channel_id: &str, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        let feed = self.feeds.entry(channel_id.to_owned()).or_default();
        if feed.fetching {
            return;
        }
        feed.fetching = true;
        let id = channel_id.to_owned();
        cx.spawn(async move |this, cx| {
            let result = client.messages(&id, None, None).await;
            this.update(cx, |this, cx| this.merge_latest(&id, result, cx)).ok();
        })
        .detach();
    }

    fn merge_latest(&mut self, channel_id: &str, result: Result<Vec<Message>, String>, cx: &mut Context<Self>) {
        let is_open = self.channel.as_ref().is_some_and(|c| c.id == channel_id);
        let feed = self.feeds.entry(channel_id.to_owned()).or_default();
        feed.fetching = false;
        let fetched = match result {
            Ok(fetched) => fetched,
            Err(error) => {
                feed.error = Some(error.into());
                return cx.notify();
            }
        };
        let newest = fetched.last().map(|m| order(&m.id)).unwrap_or(0);
        let kept: Vec<Message> = feed.messages.drain(..).filter(|m| m.state != SendState::Sent || order(&m.id) > newest).collect();
        let unchanged = kept.is_empty() && feed.loaded && feed.messages.len() == fetched.len();
        feed.has_more = fetched.len() == 50;
        feed.messages = fetched.into_iter().chain(kept).collect();
        feed.loaded = true;
        feed.error = None;
        cache::save(channel_id, &feed.messages);
        let count = feed.messages.len();
        let authors: Vec<String> = feed.messages.iter().map(|m| m.author.id.clone()).collect();
        if is_open && !unchanged {
            self.list.reset(count);
        }
        self.request_missing_roles(authors);
        cx.notify();
    }

    fn load_older(&mut self, cx: &mut Context<Self>) {
        let (Some(client), Some(channel)) = (self.client.clone(), self.channel.clone()) else { return };
        let Some(feed) = self.feeds.get_mut(&channel.id) else { return };
        if !feed.has_more || feed.loading_older || !feed.loaded {
            return;
        }
        let Some(oldest) = feed.messages.first().map(|m| m.id.clone()) else { return };
        feed.loading_older = true;
        cx.spawn(async move |this, cx| {
            let result = client.messages(&channel.id, Some(&oldest), None).await;
            this.update(cx, |this, cx| {
                let is_open = this.channel.as_ref().is_some_and(|c| c.id == channel.id);
                let Some(feed) = this.feeds.get_mut(&channel.id) else { return };
                feed.loading_older = false;
                let Ok(older) = result else { return };
                feed.has_more = older.len() == 50;
                let added = older.len();
                let authors: Vec<String> = older.iter().map(|m| m.author.id.clone()).collect();
                feed.messages.splice(0..0, older);
                if is_open && added > 0 {
                    // Keep the same message under the reader's eye while history grows above it.
                    let top = this.list.logical_scroll_top();
                    this.list.splice(0..0, added);
                    this.list.scroll_to(ListOffset { item_ix: top.item_ix + added, offset_in_item: top.offset_in_item });
                }
                this.request_missing_roles(authors);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    // History from REST carries no member roles; ask the gateway for the authors we can't colour yet.
    // History from REST names authors by their account; once their member entries arrive, show
    // them as the server does (nickname and server avatar).
    fn apply_members(&mut self, guild_id: &str) {
        let Some(known) = self.members.get(guild_id) else { return };
        for message in self.feeds.values_mut().flat_map(|f| f.messages.iter_mut()).filter(|m| m.guild_id.as_deref() == Some(guild_id)) {
            let author = &mut message.author;
            if let Some(member) = known.get(&author.id) {
                author.name.clone_from(&member.name);
                author.avatar.clone_from(&member.avatar);
            }
        }
    }

    fn request_missing_roles(&mut self, authors: Vec<String>) {
        if let Some(guild_id) = self.guild_id.clone() {
            self.apply_members(&guild_id);
        }
        let (Some(client), Some(guild_id)) = (&self.client, &self.guild_id) else { return };
        let known = self.member_roles.get(guild_id);
        let mut missing: Vec<String> = Vec::new();
        for id in authors {
            if known.is_some_and(|k| k.contains_key(&id)) || !self.requested_members.insert((guild_id.clone(), id.clone())) {
                continue;
            }
            missing.push(id);
        }
        for chunk in missing.chunks(100) {
            client.request_members(guild_id, chunk.to_vec());
        }
    }

    /// Scrolls to the replied-to message and flashes it; loads the history around it first if needed.
    fn jump_to(&mut self, message_id: String, cx: &mut Context<Self>) {
        let Some(channel) = self.channel.clone() else { return };
        let position = self.feeds.get(&channel.id).and_then(|f| f.messages.iter().position(|m| m.id == message_id));
        if let Some(ix) = position {
            self.list.scroll_to(ListOffset { item_ix: ix, offset_in_item: px(0.) });
            self.flash = Some(message_id.clone());
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(Duration::from_millis(1600)).await;
                this.update(cx, |this, cx| {
                    if this.flash.as_ref() == Some(&message_id) {
                        this.flash = None;
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
            return cx.notify();
        }
        let Some(client) = self.client.clone() else { return };
        cx.spawn(async move |this, cx| {
            let Ok(around) = client.messages(&channel.id, None, Some(&message_id)).await else { return };
            this.update(cx, |this, cx| {
                let feed = this.feeds.entry(channel.id.clone()).or_default();
                feed.messages = around;
                feed.has_more = true;
                this.list.reset(feed.messages.len());
                this.jump_to(message_id, cx);
            })
            .ok();
        })
        .detach();
    }

    fn ack_open(&mut self, cx: &mut Context<Self>) {
        let (Some(client), Some(channel)) = (self.client.clone(), self.channel.as_ref()) else { return };
        let Some(&last) = self.last_message.get(&channel.id) else { return };
        let read = self.read_states.entry(channel.id.clone()).or_default();
        if read.last_read >= last && read.mentions == 0 {
            return;
        }
        read.last_read = last;
        read.mentions = 0;
        let id = channel.id.clone();
        discord::runtime().spawn(async move { client.ack(&id, &last.to_string()).await });
        let _ = cx;
    }

    fn on_composer(&mut self, state: &Entity<ComposerState>, event: &InputEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            InputEvent::Change => {
                let text = state.read(cx).value();
                self.has_draft = !text.trim().is_empty();
                self.autocomplete = self.suggest(&text);
                self.send_typing();
                cx.notify();
            }
            InputEvent::PressEnter { shift: false, .. } if self.autocomplete.is_some() => self.accept_suggestion(window, cx),
            InputEvent::PressEnter { shift: false, .. } => self.submit(window, cx),
            _ => {}
        }
    }

    fn send_typing(&mut self) {
        let (Some(client), Some(channel)) = (self.client.clone(), self.channel.as_ref()) else { return };
        if !self.has_draft || self.last_typing_sent.is_some_and(|t| t.elapsed() < TYPING_SEND_EVERY) {
            return;
        }
        self.last_typing_sent = Some(Instant::now());
        let id = channel.id.clone();
        discord::runtime().spawn(async move { client.typing(&id).await });
    }

    fn on_search_input(&mut self, state: &Entity<SearchState>, event: &InputEvent, _: &mut Window, cx: &mut Context<Self>) {
        let InputEvent::PressEnter { .. } = event else { return };
        let query = state.read(cx).value().trim().to_owned();
        let Some(client) = self.client.clone() else { return };
        // A server searches all its channels; a DM searches itself.
        let scope = match (&self.guild_id, &self.channel) {
            (Some(guild), _) => format!("guilds/{guild}"),
            (None, Some(dm)) => format!("channels/{}", dm.id),
            (None, None) => return,
        };
        if query.is_empty() {
            return;
        }
        let users = self.known_users();
        let members = self.guild_id.as_ref().and_then(|g| self.members.get(g)).cloned().unwrap_or_default();
        let channels = self.guild().map(|g| g.channels.clone()).unwrap_or_default();
        let parsed = search::parse(
            &query,
            |name| {
                let name = name.to_lowercase();
                members.values().chain(users.iter()).find(|u| u.name.to_lowercase() == name || u.id == name).map(|u| u.id.clone())
            },
            |name| {
                let name = name.to_lowercase();
                channels.iter().find(|c| c.name.to_lowercase() == name || c.id == name).map(|c| c.id.clone())
            },
        );
        if !parsed.unresolved.is_empty() {
            self.show_toast(format!("Couldn't find {}", parsed.unresolved.join(", ")), cx);
        }
        self.right_panel = RightPanel::Search { query: query.clone().into(), results: None };
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = client.search(&scope, &parsed.content, &parsed.params).await;
            this.update(cx, |this, cx| {
                if let RightPanel::Search { results, .. } = &mut this.right_panel {
                    *results = Some(result.map_err(Into::into));
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Enter in the composer: saves the edit in progress, otherwise sends (optimistically).
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(client), Some(channel), Some(me)) = (self.client.clone(), self.channel.clone(), self.me.clone()) else { return };
        let typed = self.composer.read(cx).value().trim().to_owned();
        if typed.is_empty() && self.pending_files.is_empty() {
            return;
        }
        let content = self.to_wire(&typed);
        self.composer.update(cx, |state, cx| state.set_value("", window, cx));
        self.has_draft = false;
        self.send_error = None;
        self.last_typing_sent = None;
        if let Some(editing) = self.editing.take() {
            return self.edit(editing, content, cx);
        }
        if !self.pending_files.is_empty() {
            return self.send_files(content, cx);
        }
        let nonce = nonce();
        let reply = self.replying.take();
        let ping = self.reply_ping;
        let pending = Message {
            id: format!("pending-{nonce}"),
            channel_id: channel.id.clone(),
            guild_id: self.guild_id.clone(),
            author: me,
            content: content.clone(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            reply_to: reply.as_ref().map(|r| r.id.clone()),
            reply: reply.clone().map(Box::new),
            nonce: Some(nonce.clone()),
            state: SendState::Sending,
            ..Default::default()
        };
        self.push_local(pending, cx);
        cx.spawn(async move |this, cx| {
            let result = client.send_message(&channel.id, &content, &nonce, reply.as_ref().map(|r| r.id.as_str()), ping).await;
            this.update(cx, |this, cx| this.settle(&channel.id, &nonce, result, cx)).ok();
        })
        .detach();
    }

    fn push_local(&mut self, message: Message, cx: &mut Context<Self>) {
        let is_open = self.channel.as_ref().is_some_and(|c| c.id == message.channel_id);
        let feed = self.feeds.entry(message.channel_id.clone()).or_default();
        let at = feed.messages.len();
        self.arrivals.borrow_mut().fresh.insert(message.id.clone());
        feed.messages.push(message);
        if is_open {
            self.list.splice(at..at, 1);
            self.list.scroll_to_end();
        }
        cx.notify();
    }

    // The REST reply and the gateway echo race; whichever lands first replaces the pending copy.
    fn settle(&mut self, channel_id: &str, nonce: &str, result: Result<Message, String>, cx: &mut Context<Self>) {
        let pending = self.feeds.get_mut(channel_id).and_then(|f| f.messages.iter_mut().find(|m| m.nonce.as_deref() == Some(nonce) && m.state != SendState::Sent));
        let Some(pending) = pending else { return };
        match result {
            Ok(sent) => *pending = sent,
            Err(error) => {
                pending.state = SendState::Failed;
                self.send_error = Some(format!("Couldn't send: {error}").into());
            }
        }
        cx.notify();
    }

    pub(crate) fn retry(&mut self, message: Message, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        let Some(nonce) = message.nonce.clone() else { return };
        if let Some(m) = self.message_mut(&message.channel_id, &message.id) {
            m.state = SendState::Sending;
        }
        self.send_error = None;
        cx.spawn(async move |this, cx| {
            let result = client.send_message(&message.channel_id, &message.content, &nonce, message.reply_to.as_deref(), true).await;
            this.update(cx, |this, cx| this.settle(&message.channel_id, &nonce, result, cx)).ok();
        })
        .detach();
        cx.notify();
    }

    fn edit(&mut self, original: Message, content: String, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        if let Some(m) = self.message_mut(&original.channel_id, &original.id) {
            m.content = content.clone();
            m.edited = true;
        }
        cx.spawn(async move |this, cx| {
            if client.edit_message(&original.channel_id, &original.id, &content).await.is_ok() {
                return;
            }
            this.update(cx, |this, cx| {
                if let Some(m) = this.message_mut(&original.channel_id, &original.id) {
                    *m = original;
                }
                this.send_error = Some("Couldn't save your edit".into());
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    pub(crate) fn delete(&mut self, message: Message, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        self.remove_message(&message.channel_id, &message.id);
        cx.notify();
        if message.state != SendState::Sent {
            return;
        }
        cx.spawn(async move |this, cx| {
            if client.delete_message(&message.channel_id, &message.id).await.is_ok() {
                return;
            }
            this.update(cx, |this, cx| {
                this.send_error = Some("Couldn't delete that message".into());
                this.fetch_latest(&message.channel_id, cx);
            })
            .ok();
        })
        .detach();
    }

    fn toggle_reaction(&mut self, message: &Message, emoji: Emoji, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        let add = !message.reactions.iter().any(|r| r.emoji == emoji && r.me);
        if add {
            self.update_prefs(cx, |p| p.record_emoji(&emoji));
        }
        let (channel_id, message_id) = (message.channel_id.clone(), message.id.clone());
        if let Some(m) = self.message_mut(&channel_id, &message_id) {
            adjust_reaction(m, &emoji, add, true, false);
        }
        cx.spawn(async move |this, cx| {
            if client.react(&channel_id, &message_id, &emoji, add).await.is_ok() {
                return;
            }
            this.update(cx, |this, cx| {
                if let Some(m) = this.message_mut(&channel_id, &message_id) {
                    adjust_reaction(m, &emoji, !add, true, false);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// The + button: pick files to attach; they wait in the composer until sent, as in Discord.
    fn upload(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(gpui_kit::PathPromptOptions { files: true, directories: false, multiple: true, prompt: None });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else { return };
            this.update(cx, |this, cx| this.add_files(paths, cx)).ok();
        })
        .detach();
    }

    fn add_files(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        // Discord caps a message at 10 attachments.
        for path in paths.into_iter().filter(|p| p.is_file()) {
            if self.pending_files.len() < 10 && !self.pending_files.contains(&path) {
                self.pending_files.push(path);
            }
        }
        self.has_draft = true;
        cx.notify();
    }

    /// Cmd/Ctrl+V with an image on the clipboard attaches it (saved to a temp file first).
    fn paste_image(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(item) = cx.read_from_clipboard() else { return false };
        let Some(image) = item.entries().iter().find_map(|e| match e {
            gpui_kit::ClipboardEntry::Image(image) => Some(image.clone()),
            _ => None,
        }) else {
            return false;
        };
        let ext = match image.format {
            gpui_kit::ImageFormat::Jpeg => "jpg",
            gpui_kit::ImageFormat::Gif => "gif",
            gpui_kit::ImageFormat::Webp => "webp",
            _ => "png",
        };
        let path = std::env::temp_dir().join(format!("rustcord-paste-{}.{ext}", nonce()));
        if std::fs::write(&path, &image.bytes).is_err() {
            return false;
        }
        self.add_files(vec![path], cx);
        true
    }

    fn send_files(&mut self, content: String, cx: &mut Context<Self>) {
        let (Some(client), Some(channel), Some(me)) = (self.client.clone(), self.channel.clone(), self.me.clone()) else { return };
        let files = std::mem::take(&mut self.pending_files);
        let nonce = nonce();
        let label = match files.len() {
            1 => format!("Uploading {}…", files[0].file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()),
            n => format!("Uploading {n} files…"),
        };
        let pending = Message {
            id: format!("pending-{nonce}"),
            channel_id: channel.id.clone(),
            author: me,
            content: if content.is_empty() { label } else { content.clone() },
            timestamp: chrono::Utc::now().to_rfc3339(),
            nonce: Some(nonce.clone()),
            state: SendState::Sending,
            ..Default::default()
        };
        self.push_local(pending, cx);
        cx.spawn(async move |this, cx| {
            let result = client.upload(&channel.id, files, &content, &nonce).await;
            this.update(cx, |this, cx| this.settle(&channel.id, &nonce, result, cx)).ok();
        })
        .detach();
    }

    fn remove_file(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.pending_files.len() {
            self.pending_files.remove(index);
        }
        self.has_draft = !self.pending_files.is_empty() || !self.composer.read(cx).value().trim().is_empty();
        cx.notify();
    }

    fn open_pins(&mut self, cx: &mut Context<Self>) {
        let (Some(client), Some(channel)) = (self.client.clone(), self.channel.clone()) else { return };
        self.pins = Some(None);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = client.pins(&channel.id).await;
            this.update(cx, |this, cx| {
                if this.pins.is_some() {
                    this.pins = Some(Some(result.map_err(Into::into)));
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Alt+↑/↓: the previous or next text channel in the sidebar's order, as in Discord.
    fn step_channel(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let order: Vec<Channel> = match self.guild() {
            Some(guild) => sidebar::channel_sections(guild, self.my_id()).into_iter().flat_map(|(_, c)| c).filter(|c| c.kind == ChannelKind::Text).collect(),
            None => self.dms.clone(),
        };
        let current = self.channel.as_ref().and_then(|c| order.iter().position(|o| o.id == c.id)).unwrap_or(0) as isize;
        let next = (current + delta).clamp(0, order.len() as isize - 1);
        if let Some(channel) = order.get(next as usize).cloned() {
            self.open_channel(channel, window, cx);
        }
    }

    fn set_talk_key(&mut self, down: bool) {
        if self.talk_key_down == down {
            return;
        }
        self.talk_key_down = down;
        if let Some(call) = &self.call {
            call.session.set_talk_key(down);
            crate::sounds::play(if down { crate::sounds::Cue::PttStart } else { crate::sounds::Cue::PttStop });
        }
    }

    fn start_reply(&mut self, message: Message, window: &mut Window, cx: &mut Context<Self>) {
        self.editing = None;
        self.reply_ping = true;
        self.replying = Some(message);
        self.composer.update(cx, |state, cx| state.focus(window, cx));
        cx.notify();
    }

    fn start_edit(&mut self, message: Message, window: &mut Window, cx: &mut Context<Self>) {
        self.replying = None;
        let text = self.shown_text(&message.content);
        self.editing = Some(message);
        self.composer.update(cx, |state, cx| {
            state.set_value(text, window, cx);
            state.focus(window, cx);
        });
        self.has_draft = true;
        cx.notify();
    }

    fn cancel_compose(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.editing.take().is_some() {
            self.composer.update(cx, |state, cx| state.set_value("", window, cx));
            self.has_draft = false;
        }
        self.replying = None;
        self.autocomplete = None;
        cx.notify();
    }

    /// Up arrow in an empty composer edits your last message, like Discord.
    fn edit_last(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let (Some(me), Some(channel)) = (&self.me, &self.channel) else { return false };
        let last = self.feeds.get(&channel.id).and_then(|f| f.messages.iter().rev().find(|m| m.author.id == me.id && m.state == SendState::Sent)).cloned();
        let Some(last) = last else { return false };
        self.start_edit(last, window, cx);
        true
    }

    fn suggest(&self, text: &str) -> Option<Autocomplete> {
        let token = text.rsplit(char::is_whitespace).next().filter(|t| t.len() > 1 && (t.starts_with('@') || t.starts_with('#')))?;
        let query = token[1..].to_lowercase();
        let mut items: Vec<Suggestion> = Vec::new();
        if token.starts_with('#') {
            for c in self.guild().map(|g| g.channels.as_slice()).unwrap_or_default() {
                if c.kind == ChannelKind::Text && c.name.to_lowercase().contains(&query) {
                    items.push(Suggestion { label: c.name.clone(), insert: format!("#{}", c.name), wire: format!("<#{}>", c.id), avatar: None });
                }
            }
        } else {
            for user in self.known_users() {
                if user.name.to_lowercase().contains(&query) && !items.iter().any(|s| s.wire == format!("<@{}>", user.id)) {
                    items.push(Suggestion { label: user.name.clone(), insert: format!("@{}", user.name), wire: format!("<@{}>", user.id), avatar: Some(user.avatar) });
                }
            }
        }
        items.truncate(8);
        (!items.is_empty()).then(|| Autocomplete { token: token.to_owned(), items, selected: 0 })
    }

    fn accept_suggestion(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(auto) = self.autocomplete.take() else { return };
        let Some(pick) = auto.items.get(auto.selected).cloned() else { return };
        let text = self.composer.read(cx).value().to_string();
        let Some(head) = text.strip_suffix(auto.token.as_str()) else { return };
        let replaced = format!("{head}{} ", pick.insert);
        self.mention_wire.insert(pick.insert, pick.wire);
        self.composer.update(cx, |state, cx| state.set_value(replaced, window, cx));
        cx.notify();
    }

    fn move_suggestion(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(auto) = &mut self.autocomplete else { return };
        auto.selected = (auto.selected as isize + delta).rem_euclid(auto.items.len() as isize) as usize;
        cx.notify();
    }

    // "@Ferris" → "<@id>": longest first, so "@Ann Marie" wins over "@Ann".
    fn to_wire(&self, text: &str) -> String {
        let mut pairs: Vec<_> = self.mention_wire.iter().collect();
        pairs.sort_by_key(|(k, _)| std::cmp::Reverse(k.len()));
        pairs.into_iter().fold(text.to_owned(), |text, (shown, wire)| text.replace(shown.as_str(), wire))
    }

    fn shown_text(&self, text: &str) -> String {
        self.mention_wire.iter().fold(text.to_owned(), |text, (shown, wire)| text.replace(wire.as_str(), shown))
    }

    fn known_users(&self) -> Vec<User> {
        let mut users: Vec<User> = Vec::new();
        if let Some(list) = self.guild_id.as_ref().and_then(|g| self.member_lists.get(g)) {
            users.extend(list.iter().filter_map(|i| match i {
                ListItem::Member(m) => Some(m.user.clone()),
                ListItem::Group { .. } => None,
            }));
        }
        if let Some(feed) = self.channel.as_ref().and_then(|c| self.feeds.get(&c.id)) {
            users.extend(feed.messages.iter().rev().map(|m| m.author.clone()));
        }
        users.extend(self.dms.iter().filter_map(|d| d.recipient.clone()));
        users
    }

    // Asked once per server per connection, when it's opened, as Discord does.
    fn request_channel_info(&mut self) {
        let (Some(guild_id), Some(client)) = (self.guild_id.clone(), self.client.as_ref()) else { return };
        if self.channel_info_requested.insert(guild_id.clone()) {
            client.request_channel_info(&guild_id);
        }
    }

    // How long a voice channel's current session has run, from Discord's start time.
    pub(crate) fn voice_elapsed(&self, channel_id: &str) -> Option<u64> {
        let start = *self.voice_start.get(channel_id)?;
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
        Some(now.saturating_sub(start))
    }

    fn user_avatar(&self, user_id: &str) -> String {
        self.known_users().into_iter().find(|u| u.id == user_id).map(|u| u.avatar).unwrap_or_else(|| discord::avatar_url(user_id, None))
    }

    fn user_name(&self, user_id: &str) -> String {
        self.known_users().into_iter().find(|u| u.id == user_id).map(|u| u.name).unwrap_or_else(|| "Someone".into())
    }

    pub(crate) fn message_mut(&mut self, channel_id: &str, id: &str) -> Option<&mut Message> {
        self.feeds.get_mut(channel_id)?.messages.iter_mut().find(|m| m.id == id)
    }

    fn remove_message(&mut self, channel_id: &str, id: &str) {
        let is_open = self.channel.as_ref().is_some_and(|c| c.id == channel_id);
        let Some(feed) = self.feeds.get_mut(channel_id) else { return };
        let Some(ix) = feed.messages.iter().position(|m| m.id == id) else { return };
        feed.messages.remove(ix);
        if is_open {
            self.list.splice(ix..ix + 1, 0);
        }
    }

    fn guild(&self) -> Option<&Guild> {
        let id = self.guild_id.as_ref()?;
        self.guilds.iter().find(|g| &g.id == id)
    }

    fn mentions_me(&self, message: &Message) -> bool {
        let Some(me) = &self.me else { return false };
        message.mention_everyone || message.mentions.iter().any(|u| u.id == me.id) || self.mentions_role(message)
    }

    // A thread's parent channel, for the header breadcrumb and the sidebar.
    fn thread_parent(&self, channel: &Channel) -> Option<Channel> {
        if channel.kind != ChannelKind::Thread {
            return None;
        }
        let parent = channel.parent_id.as_ref()?;
        self.guilds.iter().flat_map(|g| g.channels.iter()).find(|c| &c.id == parent).cloned()
    }

    // Whether the message pings one of our roles in its server.
    fn mentions_role(&self, message: &Message) -> bool {
        let Some(me) = &self.me else { return false };
        let mine = message.guild_id.as_ref().and_then(|g| self.member_roles.get(g)).and_then(|r| r.get(&me.id));
        mine.is_some_and(|roles| message.mention_roles.iter().any(|r| roles.contains(r)))
    }

    /// The colour of the user's highest coloured role, as Discord paints their name.
    fn role_color(&self, guild_id: Option<&String>, user_id: &str, hint: Option<&Vec<String>>) -> Option<Hsla> {
        let guild = self.guilds.iter().find(|g| Some(&g.id) == guild_id)?;
        let roles = hint.or_else(|| self.member_roles.get(&guild.id)?.get(user_id))?;
        guild.roles.iter().filter(|r| r.color != 0 && roles.contains(&r.id)).max_by_key(|r| r.position).map(|r| rgb(r.color).into())
    }

    fn unread(&self, channel_id: &str) -> bool {
        let (Some(last), Some(read)) = (self.last_message.get(channel_id), self.read_states.get(channel_id)) else { return false };
        *last > read.last_read
    }

    /// Our user id, or "" before READY.
    pub(crate) fn my_id(&self) -> &str {
        self.me.as_ref().map_or("", |m| m.id.as_str())
    }

    fn mention_count(&self, channel_id: &str) -> u32 {
        self.read_states.get(channel_id).map_or(0, |r| r.mentions)
    }

    // A muted server, or muted channel, keeps its mention count but drops its unread pill, as in Discord.
    fn guild_badge(&self, guild: &Guild) -> (bool, u32) {
        let settings = self.guild_settings.get(&guild.id);
        let server_muted = settings.is_some_and(|s| s.is_muted());
        let me = self.my_id();
        let text = guild.channels.iter().filter(|c| c.kind == ChannelKind::Text && guild.can_view(me, c));
        text.fold((false, 0), |(unread, mentions), c| {
            let muted = server_muted || settings.is_some_and(|s| s.channel_muted(&c.id) || c.parent_id.as_ref().is_some_and(|p| s.channel_muted(p)));
            (unread || (self.unread(&c.id) && !muted), mentions + self.mention_count(&c.id))
        })
    }

    fn typers(&self) -> Vec<Typer> {
        let Some(channel) = &self.channel else { return Vec::new() };
        let now = Instant::now();
        self.typing.get(&channel.id).map(|t| t.iter().filter(|t| t.until > now).cloned().collect()).unwrap_or_default()
    }

    fn join_voice(&mut self, channel: Channel, cx: &mut Context<Self>) {
        let (Some(client), Some(me)) = (self.client.clone(), self.me.clone()) else { return };
        if self.call.as_ref().is_some_and(|c| c.channel.id == channel.id) {
            return;
        }
        self.leave_voice(cx);
        self.voice_error = None;
        let (session, mut events) = voice::join(me.id, self.guild_id.clone(), channel.id.clone(), client.voice.clone(), self.prefs.voice_settings());
        for (user_id, volume) in &self.prefs.user_volumes {
            session.set_user_volume(user_id, *volume);
        }
        for user_id in &self.prefs.local_mutes {
            session.set_user_muted(user_id, true);
        }
        if self.muted || self.deafened {
            session.set_muted(true);
        }
        if self.deafened {
            session.set_deafened(true);
        }
        let task = cx.spawn(async move |this, cx| {
            while let Some(event) = events.next().await {
                if this.update(cx, |this, cx| this.on_voice(event, cx)).is_err() {
                    break;
                }
            }
        });
        // Ticks the "Voice Connected" timer once a second while in the call.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let in_call = this.update(cx, |this, cx| {
                    cx.notify();
                    this.call.is_some()
                });
                if !matches!(in_call, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
        self.call = Some(Call { session, guild_id: self.guild_id.clone(), channel, connected: false, stage: voice::Stage::AwaitingEndpoint, stats: None, level: (SILENCE_DB, false), speaking: HashSet::new(), last_spoke: HashMap::new(), cameras: HashMap::new(), camera_on: false, watching: Vec::new(), focus: None, stream_layout: crate::call::StreamLayout::Focus, ended: None, sharing: None, share_label: None, share_source: None, live_since: None, self_view: None, _events: task });
        cx.notify();
    }

    fn leave_voice(&mut self, cx: &mut Context<Self>) {
        self.call_fullscreen = false;
        let Some(mut call) = self.call.take() else { return };
        sounds::play(sounds::Cue::Disconnect);
        for frame in call.watching.drain(..).filter_map(|w| w.frame) {
            cx.drop_image(frame, None);
        }
        for (_, frame) in call.cameras.drain() {
            cx.drop_image(frame, None);
        }
        cx.notify();
    }

    fn on_voice(&mut self, event: voice::VoiceEvent, cx: &mut Context<Self>) {
        // A stream picked from the hover card before we'd joined: watch it once connected.
        let mut start_watch: Option<String> = None;
        let Some(call) = &mut self.call else { return };
        match event {
            voice::VoiceEvent::Connected => {
                if self.ring_on_connect.as_ref() == Some(&call.channel.id)
                    && let Some(client) = self.client.clone()
                {
                    let channel = self.ring_on_connect.take().unwrap_or_default();
                    crate::discord::runtime().spawn(async move {
                        let _ = client.call(reqwest::Method::POST, format!("/channels/{channel}/call/ring"), Some(serde_json::json!({ "recipients": null }))).await;
                    });
                }
                start_watch = self.watch_on_connect.take();
                let Some(call) = &mut self.call else { return };
                call.connected = true;
                call.stage = voice::Stage::Connected;
                sounds::play(sounds::Cue::UserJoin);
            }
            voice::VoiceEvent::Stage(stage) => call.stage = stage,
            voice::VoiceEvent::Stats(stats) => call.stats = Some(stats),
            voice::VoiceEvent::InputLevel { db, gate_open } => {
                call.level = (db, gate_open);
                if self.mic_test.is_none() {
                    self.mic_level = db;
                }
            }
            voice::VoiceEvent::Speaking { user_id, speaking: true } => {
                call.speaking.insert(user_id);
            }
            voice::VoiceEvent::Speaking { user_id, speaking: false } => {
                call.speaking.remove(&user_id);
                call.last_spoke.insert(user_id, Instant::now());
            }
            voice::VoiceEvent::VideoState { user_id, streaming, camera } => {
                let guild_id = call.guild_id.clone();
                let state = self.guilds.iter_mut().filter(|g| Some(&g.id) == guild_id.as_ref()).flat_map(|g| g.voice_states.iter_mut()).find(|v| v.user_id == user_id);
                if let Some(state) = state {
                    state.streaming = streaming;
                    state.camera = camera;
                }
            }
            voice::VoiceEvent::StreamFrame { user_id, source: voice::VideoSource::GoLive, frame } => {
                let mine = self.me.as_ref().is_some_and(|m| m.id == user_id);
                let watching_self = call.watched(&user_id).is_some();
                // The self-view runs at 15 fps: a full-rate copy of every frame isn't worth it for a thumbnail.
                if mine && !watching_self {
                    if call.self_view.as_ref().is_some_and(|(_, at)| at.elapsed() < SELF_VIEW_INTERVAL) {
                        return;
                    }
                    let Some(image) = render_image(&frame) else { return };
                    if let Some((old, _)) = call.self_view.replace((image, Instant::now())) {
                        cx.drop_image(old, None);
                    }
                    return cx.notify();
                }
                let Some(watching) = call.watching.iter_mut().find(|w| w.streamer_id == user_id) else { return };
                let Some(image) = render_image(&frame) else { return };
                watching.arrivals.push_back(Instant::now());
                while watching.arrivals.len() > 120 {
                    watching.arrivals.pop_front();
                }
                if let Some(old) = watching.frame.replace(image) {
                    // Each frame is a new GPU texture; the old one must be freed or the atlas grows forever.
                    cx.drop_image(old, None);
                }
            }
            voice::VoiceEvent::StreamFrame { user_id, source: voice::VideoSource::Camera, frame } => {
                let Some(image) = render_image(&frame) else { return };
                if let Some(old) = call.cameras.insert(user_id, image) {
                    cx.drop_image(old, None);
                }
            }
            voice::VoiceEvent::StreamEnded { user_id, reason } => {
                if let Some(frame) = call.cameras.remove(&user_id) {
                    cx.drop_image(frame, None);
                }
                if let Some(watch) = call.unwatch(&user_id) {
                    sounds::play(sounds::Cue::StreamEnded);
                    if let Some(frame) = watch.frame {
                        cx.drop_image(frame, None);
                    }
                    // The stage says who stopped; the reason only matters when it wasn't them.
                    if call.watching.is_empty() {
                        call.ended = Some(user_id);
                    }
                    if reason != "Stream ended" && reason != "user_requested" {
                        eprintln!("voice: stream ended: {reason}");
                    }
                }
            }
            voice::VoiceEvent::Disconnected(reason) => {
                self.call = None;
                sounds::play(sounds::Cue::Disconnect);
                if reason != "Left" {
                    self.voice_error = Some(reason.into());
                }
            }
        }
        if let Some(streamer) = start_watch {
            self.watch_stream(streamer, cx);
        }
        cx.notify();
    }

    // Adds `streamer_id`'s Go Live to the streams we watch and focuses it; Discord lets you watch several.
    fn watch_stream(&mut self, streamer_id: String, cx: &mut Context<Self>) {
        let mine = self.me.as_ref().is_some_and(|m| m.id == streamer_id);
        let Some(call) = &mut self.call else { return };
        call.ended = None;
        call.focus = Some(streamer_id.clone());
        if call.watched(&streamer_id).is_some() {
            return cx.notify();
        }
        let watch = match mine {
            true if call.sharing.is_some() => None,
            true => return cx.notify(),
            false => match call.session.watch_stream(&streamer_id) {
                Ok(watch) => Some(watch),
                Err(error) => {
                    self.voice_error = Some(error.into());
                    return cx.notify();
                }
            },
        };
        call.watching.push(Watching { streamer_id, frame: None, arrivals: Default::default(), _watch: watch });
        cx.notify();
    }

    fn unwatch_stream(&mut self, streamer_id: &str, cx: &mut Context<Self>) {
        if let Some(frame) = self.call.as_mut().and_then(|c| c.unwatch(streamer_id)).and_then(|w| w.frame) {
            cx.drop_image(frame, None);
        }
        cx.notify();
    }

    // Whether `key` is our own Go Live or the one we're watching: the stream whose viewers Discord announces.
    fn in_stream(&self, key: &str) -> bool {
        let Some(call) = &self.call else { return false };
        let mine = call.sharing.is_some() && self.me.as_ref().is_some_and(|m| key.ends_with(&format!(":{}", m.id)));
        // Discord announces viewers only for the single stream you're in.
        let watched = matches!(call.watching.as_slice(), [only] if key.ends_with(&format!(":{}", only.streamer_id)));
        mine || watched
    }

    fn stop_watching(&mut self, cx: &mut Context<Self>) {
        let Some(call) = &mut self.call else { return };
        call.focus = None;
        for frame in call.watching.drain(..).filter_map(|w| w.frame) {
            cx.drop_image(frame, None);
        }
        cx.notify();
    }

    /// Stops sharing, or opens Discord's picker of what to share: the whole screen, an app or a window.
    fn toggle_screen_share(&mut self, cx: &mut Context<Self>) {
        let Some(call) = &mut self.call else { return };
        if call.sharing.take().is_some() {
            sounds::play(sounds::Cue::StreamEnded);
            call.share_source = None;
            call.live_since = None;
            if let Some((frame, _)) = call.self_view.take() {
                cx.drop_image(frame, None);
            }
            if let Some(me) = self.me.as_ref().map(|m| m.id.clone()) {
                self.unwatch_stream(&me, cx);
            }
            return cx.notify();
        }
        self.open_share_picker(cx);
    }

    fn start_share(&mut self, source: voice::ShareSource, cx: &mut Context<Self>) {
        self.close_share_picker(cx);
        let label = self.share_choice_label.take();
        if self.go_live(source, cx) {
            if let Some(call) = &mut self.call {
                call.share_label = label;
                call.live_since = Some(Instant::now());
            }
            self.pip_shown = true;
            sounds::play(sounds::Cue::StreamStarted);
        }
        cx.notify();
    }

    // Starts (or restarts) our Go Live of `source` with the current quality prefs.
    fn go_live(&mut self, source: voice::ShareSource, cx: &mut Context<Self>) -> bool {
        let Some(call) = &mut self.call else { return false };
        let options = voice::GoLiveOptions { resolution: self.prefs.stream_resolution, fps: self.prefs.stream_fps, audio: self.prefs.stream_audio };
        match call.session.go_live_with_options(source, options) {
            Ok(live) => {
                call.sharing = Some(live);
                call.share_source = Some(source);
                self.upload_stream_previews(source, cx);
                true
            }
            Err(error) => {
                self.voice_error = Some(error.into());
                false
            }
        }
    }

    // Quality and sound changes take effect by restarting the same stream; viewers rejoin it.
    pub(crate) fn restart_share(&mut self, cx: &mut Context<Self>) {
        let Some(call) = self.call.as_mut() else { return };
        let Some(source) = call.share_source.filter(|_| call.sharing.take().is_some()) else { return };
        self.go_live(source, cx);
        cx.notify();
    }

    // Mute and deafen apply to the call as well as the buttons; deafening also mutes, as in Discord.
    fn sync_voice_flags(&self) {
        let Some(call) = &self.call else { return };
        call.session.set_muted(self.muted || self.deafened);
        call.session.set_deafened(self.deafened);
    }

    fn open_viewer(&mut self, url: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(feed) = self.channel.as_ref().and_then(|c| self.feeds.get(&c.id)) else { return };
        let items: Vec<Media> = feed.messages.iter().flat_map(|m| m.media.iter().cloned()).collect();
        let Some(index) = items.iter().position(|m| m.url == url) else { return };
        self.viewer = Some(Viewer { items, index });
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn step_viewer(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(viewer) = &mut self.viewer else { return };
        let last = viewer.items.len() as isize - 1;
        viewer.index = (viewer.index as isize + delta).clamp(0, last) as usize;
        cx.notify();
    }

    fn copy_code(&mut self, code: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(code.clone()));
        self.copied = Some(code.clone());
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(1500)).await;
            this.update(cx, |this, cx| {
                if this.copied.as_ref() == Some(&code) {
                    this.copied = None;
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// The rail in Discord's order: folders and loose servers as the user arranged them, then any others.
    fn rail_entries(&self) -> Vec<RailEntry> {
        let mut placed: HashSet<&str> = HashSet::new();
        let mut entries = Vec::new();
        for folder in &self.folders {
            let guilds: Vec<Guild> = folder.guild_ids.iter().filter_map(|id| self.guilds.iter().find(|g| &g.id == id).cloned()).collect();
            placed.extend(folder.guild_ids.iter().map(String::as_str));
            match (folder.id.is_empty() || folder.id == "0", guilds.len()) {
                (_, 0) => {}
                (true, _) => entries.extend(guilds.into_iter().map(RailEntry::Guild)),
                (false, _) => entries.push(RailEntry::Folder(folder.clone(), guilds)),
            }
        }
        entries.extend(self.guilds.iter().filter(|g| !placed.contains(g.id.as_str())).cloned().map(RailEntry::Guild));
        entries
    }

    fn set_hovered(&mut self, key: &str, hovered: bool, cx: &mut Context<Self>) {
        if hovered {
            self.hovered = Some(key.to_owned());
        } else if self.hovered.as_deref() == Some(key) {
            self.hovered = None;
        }
        cx.notify();
    }
}

enum RailEntry {
    Guild(Guild),
    Folder(Folder, Vec<Guild>),
}

// gpui's render images are BGRA, which is what the decoder produces, so the bytes go in as they are.
fn render_image(frame: &voice::VideoFrame) -> Option<Arc<RenderImage>> {
    let buffer = image::RgbaImage::from_raw(frame.width, frame.height, frame.bgra.clone())?;
    Some(Arc::new(RenderImage::new([image::Frame::new(buffer)])))
}

/// Applies a reaction change. `from_gateway` echoes of our own toggles are already counted.
fn adjust_reaction(message: &mut Message, emoji: &Emoji, added: bool, mine: bool, from_gateway: bool) {
    if from_gateway && mine && message.reactions.iter().any(|r| &r.emoji == emoji && r.me == added) {
        return;
    }
    if from_gateway && mine && !added && !message.reactions.iter().any(|r| &r.emoji == emoji) {
        return;
    }
    match message.reactions.iter().position(|r| &r.emoji == emoji) {
        Some(ix) => {
            let r = &mut message.reactions[ix];
            r.count = if added { r.count + 1 } else { r.count.saturating_sub(1) };
            if mine {
                r.me = added;
            }
            if r.count == 0 {
                message.reactions.remove(ix);
            }
        }
        None if added => message.reactions.push(discord::Reaction { emoji: emoji.clone(), count: 1, me: mine }),
        None => {}
    }
}

fn apply_list_op(list: &mut Vec<ListItem>, op: ListOp) {
    match op {
        ListOp::Sync { start, items } => {
            if list.len() < start {
                list.resize(start, ListItem::Group { id: String::new(), count: 0 });
            }
            let end = (start + items.len()).min(list.len());
            list.splice(start..end, items);
        }
        ListOp::Insert { index, item } if index <= list.len() => list.insert(index, item),
        ListOp::Update { index, item } if index < list.len() => list[index] = item,
        ListOp::Delete { index } if index < list.len() => {
            list.remove(index);
        }
        _ => {}
    }
}

// Pending ids sort after every real snowflake.
fn order(id: &str) -> u64 {
    id.parse().unwrap_or(u64::MAX)
}

fn nonce() -> String {
    let millis = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
    // A snowflake-shaped number, as Discord's own clients send.
    ((millis.saturating_sub(1_420_070_400_000) << 22) | u64::from(std::process::id() % 4096)).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(id: &str) -> ListItem {
        ListItem::Member(discord::Member { user: User { id: id.into(), ..Default::default() }, roles: vec![], presence: Presence::default() })
    }

    fn ids(list: &[ListItem]) -> Vec<String> {
        list.iter()
            .map(|i| match i {
                ListItem::Member(m) => m.user.id.clone(),
                ListItem::Group { id, .. } => format!("g:{id}"),
            })
            .collect()
    }

    #[test]
    fn member_list_ops_keep_order() {
        let mut list = Vec::new();
        apply_list_op(&mut list, ListOp::Sync { start: 0, items: vec![ListItem::Group { id: "online".into(), count: 2 }, member("a"), member("b")] });
        apply_list_op(&mut list, ListOp::Insert { index: 2, item: member("c") });
        apply_list_op(&mut list, ListOp::Delete { index: 1 });
        apply_list_op(&mut list, ListOp::Update { index: 2, item: member("d") });
        assert_eq!(ids(&list), ["g:online", "c", "d"]);
        apply_list_op(&mut list, ListOp::Sync { start: 0, items: vec![member("x")] });
        assert_eq!(ids(&list), ["x", "c", "d"]);
    }

    #[test]
    fn reactions_count_once_per_toggle() {
        let mut m = Message::default();
        let thumbs = Emoji::unicode("👍");
        adjust_reaction(&mut m, &thumbs, true, true, false);
        // The gateway echo of our own optimistic add.
        adjust_reaction(&mut m, &thumbs, true, true, true);
        assert_eq!(m.reactions[0].count, 1);
        adjust_reaction(&mut m, &thumbs, true, false, true);
        assert_eq!(m.reactions[0].count, 2);
        adjust_reaction(&mut m, &thumbs, false, true, false);
        assert_eq!((m.reactions[0].count, m.reactions[0].me), (1, false));
        adjust_reaction(&mut m, &thumbs, false, true, true);
        assert_eq!(m.reactions[0].count, 1);
    }

    #[test]
    fn pending_sorts_after_real_ids() {
        assert!(order("pending-1") > order("1300000000000000000"));
    }

    #[test]
    fn nonce_looks_like_a_snowflake() {
        assert!(nonce().parse::<u64>().unwrap() > 1 << 40);
    }
}

// Discord's cue when the viewer count of the stream we're in changes (it stays quiet past 25).
fn viewer_cue(before: usize, after: usize) -> Option<sounds::Cue> {
    match () {
        _ if before > 25 => None,
        _ if after > before => Some(sounds::Cue::StreamUserJoined),
        _ if after < before => Some(sounds::Cue::StreamUserLeft),
        _ => None,
    }
}

#[cfg(test)]
mod viewer_tests {
    use super::*;

    #[test]
    fn viewer_cues() {
        assert_eq!(viewer_cue(0, 1), Some(sounds::Cue::StreamUserJoined));
        assert_eq!(viewer_cue(2, 1), Some(sounds::Cue::StreamUserLeft));
        assert_eq!(viewer_cue(1, 1), None);
        assert_eq!(viewer_cue(30, 31), None);
    }
}
