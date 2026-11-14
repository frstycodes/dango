// Floating surfaces over the app: profile popout, status menu, emoji picker, quick switcher, and the
// actions the right-click menus run.
use gpui_kit::{
    AnyElement, ClipboardItem, Context, Entity, FontWeight, Hsla, KeyDownEvent, MouseButton, Pixels, Point, SharedString, Window,
    component::{
        ActiveTheme, Icon,
        input::{Input, InputEvent},
    },
    div, img, prelude::*, px, rgb,
};

use crate::{
    Channel, ChannelKind, Rustcord, EmojiTarget, ProfileCard, SearchState,
    discord::{Emoji, Message, Status, User},
    icons::Glyph,
    layout::{TITLE_HEIGHT, avatar_with_status, status_mark},
    palette,
};

const CARD_WIDTH: f32 = 300.;
const PICKER_SIZE: (f32, f32) = (420., 440.);
const SWITCHER_WIDTH: f32 = 560.;

impl Rustcord {
    pub(crate) fn on_emoji_search(&mut self, _: &Entity<SearchState>, _: &InputEvent, _: &mut Window, cx: &mut Context<Self>) {
        cx.notify();
    }

    pub(crate) fn on_switcher_input(&mut self, _: &Entity<SearchState>, event: &InputEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            InputEvent::PressEnter { .. } => self.accept_switcher(window, cx),
            InputEvent::Change => {
                self.switcher = Some(0);
                cx.notify();
            }
            _ => {}
        }
    }

    pub(crate) fn copy_text(&self, text: impl Into<String>, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(text.into()));
    }

    pub(crate) fn open_profile(&mut self, user: User, at: Point<Pixels>, cx: &mut Context<Self>) {
        let guild_id = self.guild_id.clone();
        self.profile_card = Some(ProfileCard { user: user.clone(), guild_id: guild_id.clone(), at, profile: None });
        cx.notify();
        let Some(client) = self.client.clone() else { return };
        cx.spawn(async move |this, cx| {
            let Ok(profile) = client.profile(&user.id, guild_id.as_deref()).await else { return };
            this.update(cx, |this, cx| {
                if let Some(card) = this.profile_card.as_mut().filter(|c| c.user.id == user.id) {
                    card.profile = Some(profile);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn message_user(&mut self, user_id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.profile_card = None;
        if let Some(dm) = self.dms.iter().find(|d| d.recipient.as_ref().is_some_and(|r| r.id == user_id)).cloned() {
            self.guild_id = None;
            return self.open_channel(dm, window, cx);
        }
        let Some(client) = self.client.clone() else { return };
        cx.spawn_in(window, async move |this, cx| {
            let Ok(dm) = client.open_dm(&user_id).await else { return };
            this.update_in(cx, |this, window, cx| {
                if !this.dms.iter().any(|d| d.id == dm.id) {
                    this.dms.insert(0, dm.clone());
                }
                this.guild_id = None;
                this.open_channel(dm, window, cx);
            })
            .ok();
        })
        .detach();
    }

    /// Puts "@name " in the composer, sent as a real mention.
    pub(crate) fn mention_user(&mut self, user: &User, window: &mut Window, cx: &mut Context<Self>) {
        let shown = format!("@{}", user.name);
        self.mention_wire.insert(shown.clone(), format!("<@{}>", user.id));
        let text = self.composer.read(cx).value().to_string();
        let joined = if text.is_empty() || text.ends_with(' ') { format!("{text}{shown} ") } else { format!("{text} {shown} ") };
        self.composer.update(cx, |state, cx| {
            state.set_value(joined, window, cx);
            state.focus(window, cx);
        });
        self.has_draft = true;
        self.profile_card = None;
        cx.notify();
    }

    pub(crate) fn set_status(&mut self, status: Status, cx: &mut Context<Self>) {
        self.my_status = status;
        self.status_menu = false;
        if let Some(client) = self.client.clone() {
            crate::discord::runtime().spawn(async move { client.set_status(status).await });
        }
        cx.notify();
    }

    pub(crate) fn mark_channel_read(&mut self, channel_id: &str, cx: &mut Context<Self>) {
        let Some(&last) = self.last_message.get(channel_id) else { return };
        let read = self.read_states.entry(channel_id.to_owned()).or_default();
        read.last_read = last;
        read.mentions = 0;
        if let Some(client) = self.client.clone() {
            let id = channel_id.to_owned();
            crate::discord::runtime().spawn(async move { client.ack(&id, &last.to_string()).await });
        }
        cx.notify();
    }

    pub(crate) fn mark_guild_read(&mut self, guild_id: &str, cx: &mut Context<Self>) {
        let Some(guild) = self.guilds.iter().find(|g| g.id == guild_id) else { return };
        let unread: Vec<(String, u64)> = guild
            .channels
            .iter()
            .filter(|c| self.unread(&c.id) || self.mention_count(&c.id) > 0)
            .filter_map(|c| Some((c.id.clone(), *self.last_message.get(&c.id)?)))
            .collect();
        for (channel, last) in &unread {
            let read = self.read_states.entry(channel.clone()).or_default();
            read.last_read = *last;
            read.mentions = 0;
        }
        if let Some(client) = self.client.clone().filter(|_| !unread.is_empty()) {
            crate::discord::runtime().spawn(async move { client.ack_bulk(unread).await });
        }
        cx.notify();
    }

    /// Moves the read line to just before this message, as Discord's "Mark Unread".
    pub(crate) fn mark_unread(&mut self, message: &Message, cx: &mut Context<Self>) {
        let Ok(id) = message.id.parse::<u64>() else { return };
        self.read_states.entry(message.channel_id.clone()).or_default().last_read = id - 1;
        self.unread_marker = Some(id - 1);
        cx.notify();
    }

    pub(crate) fn message_link(&self, message: &Message) -> String {
        let place = message.guild_id.clone().or_else(|| self.guild_id.clone()).unwrap_or_else(|| "@me".into());
        format!("https://discord.com/channels/{place}/{}/{}", message.channel_id, message.id)
    }

    pub(crate) fn open_emoji_picker(&mut self, target: EmojiTarget, window: &mut Window, cx: &mut Context<Self>) {
        self.emoji_picker = Some(target);
        self.emoji_search.update(cx, |state, cx| {
            state.set_value("", window, cx);
            state.focus(window, cx);
        });
        cx.notify();
    }

    fn pick_emoji(&mut self, emoji: Emoji, window: &mut Window, cx: &mut Context<Self>) {
        let Some(target) = self.emoji_picker.take() else { return };
        match target {
            EmojiTarget::Reaction(message) => self.toggle_reaction(&message, emoji, cx),
            EmojiTarget::Composer => {
                let shown = match &emoji.id {
                    Some(id) => {
                        let shown = format!(":{}:", emoji.name);
                        let wire = format!("<{}:{}:{id}>", if emoji.animated { "a" } else { "" }, emoji.name);
                        self.mention_wire.insert(shown.clone(), wire);
                        shown
                    }
                    None => emoji.name.clone(),
                };
                let text = self.composer.read(cx).value().to_string();
                self.composer.update(cx, |state, cx| {
                    state.set_value(format!("{text}{shown}"), window, cx);
                    state.focus(window, cx);
                });
                self.has_draft = true;
            }
        }
        cx.notify();
    }

    pub(crate) fn open_switcher(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.switcher = Some(0);
        self.switcher_input.update(cx, |state, cx| {
            state.set_value("", window, cx);
            state.focus(window, cx);
        });
        cx.notify();
    }

    // Every text channel and DM, best name matches first; empty query lists DMs then the open server.
    fn switcher_results(&self, cx: &Context<Self>) -> Vec<(Option<String>, Channel, SharedString)> {
        let query = self.switcher_input.read(cx).value().to_lowercase();
        let mut results: Vec<(Option<String>, Channel, SharedString)> = Vec::new();
        for dm in &self.dms {
            results.push((None, dm.clone(), "Direct Messages".into()));
        }
        for guild in &self.guilds {
            for channel in guild.channels.iter().filter(|c| c.kind == ChannelKind::Text) {
                results.push((Some(guild.id.clone()), channel.clone(), guild.name.clone().into()));
            }
        }
        if query.is_empty() {
            results.retain(|(guild, _, _)| guild.is_none() || guild == &self.guild_id);
        } else {
            results.retain(|(_, c, _)| c.name.to_lowercase().contains(&query));
            results.sort_by_key(|(_, c, _)| !c.name.to_lowercase().starts_with(&query));
        }
        results.truncate(12);
        results
    }

    fn accept_switcher(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(selected) = self.switcher else { return };
        let Some((guild_id, channel, _)) = self.switcher_results(cx).into_iter().nth(selected) else { return };
        self.switcher = None;
        self.guild_id = guild_id;
        self.open_channel(channel, window, cx);
        self.composer.update(cx, |state, cx| state.focus(window, cx));
    }

    pub(crate) fn on_switcher_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(selected) = self.switcher else { return };
        let count = self.switcher_results(cx).len().max(1);
        match event.keystroke.key.as_str() {
            "down" => self.switcher = Some((selected + 1) % count),
            "up" => self.switcher = Some((selected + count - 1) % count),
            "escape" => self.switcher = None,
            "enter" => return self.accept_switcher(window, cx),
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    /// Closes whichever floating surface is open; true if one was.
    pub(crate) fn dismiss_overlay(&mut self, cx: &mut Context<Self>) -> bool {
        let open = self.profile_card.take().is_some()
            | std::mem::take(&mut self.status_menu)
            | self.emoji_picker.take().is_some()
            | self.switcher.take().is_some()
            | self.share_picker.take().is_some()
            | self.pins.take().is_some()
            | std::mem::take(&mut self.server_menu)
            | self.inbox.take().is_some()
            | self.gif_picker.take().is_some()
            | self.dismiss_call_menus();
        if open {
            cx.notify();
        }
        open
    }

    pub(crate) fn render_overlays(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut out = Vec::new();
        out.extend(self.profile_overlay(window, cx));
        out.extend(self.status_overlay(cx));
        out.extend(self.emoji_overlay(window, cx));
        out.extend(self.switcher_overlay(cx));
        out.extend(self.share_overlay(cx));
        out.extend(self.pins_overlay(cx));
        out.extend(self.server_menu_overlay(cx));
        out.extend(self.inbox_overlay(cx));
        out.extend(self.gif_overlay(cx));
        out
    }

    // The title bar's Inbox: your recent mentions across servers, each jumping to its message.
    fn inbox_overlay(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let inbox = self.inbox.as_ref()?;
        let theme = cx.theme().clone();
        let body: AnyElement = match inbox {
            None => div().p_4().text_color(theme.muted_foreground).child("Loading mentions…").into_any_element(),
            Some(Err(error)) => div().p_4().text_color(theme.danger).child(error.clone()).into_any_element(),
            Some(Ok(list)) if list.is_empty() => div().p_6().text_color(theme.muted_foreground).child("You're all caught up!").into_any_element(),
            Some(Ok(list)) => div()
                .flex()
                .flex_col()
                .gap_2()
                .children(list.iter().map(|m| {
                    let place = m.guild_id.as_ref().and_then(|g| self.guilds.iter().find(|x| &x.id == g)).and_then(|g| g.channels.iter().find(|c| c.id == m.channel_id).map(|c| format!("#{} · {}", c.name, g.name))).unwrap_or_default();
                    let (guild_id, channel_id, message_id) = (m.guild_id.clone(), m.channel_id.clone(), m.id.clone());
                    div()
                        .id(SharedString::from(format!("inbox-{}", m.id)))
                        .p_3()
                        .rounded(px(8.))
                        .bg(theme.background)
                        .border_1()
                        .border_color(theme.border)
                        .cursor_pointer()
                        .hover(|s| s.border_color(theme.input))
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(div().text_size(px(12.)).text_color(theme.muted_foreground).child(place))
                        .child(div().flex().items_center().gap_2().child(img(m.author.avatar.clone()).size(px(24.)).rounded_full()).child(div().font_weight(FontWeight::MEDIUM).child(m.author.name.clone())))
                        .child(div().text_size(px(14.)).line_clamp(3).child(m.content.clone()))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.inbox = None;
                            let channel = guild_id.as_ref().and_then(|g| this.guilds.iter().find(|x| &x.id == g)).and_then(|g| g.channels.iter().find(|c| c.id == channel_id)).cloned();
                            if let Some(channel) = channel {
                                this.guild_id = guild_id.clone();
                                this.open_channel(channel, window, cx);
                            }
                            this.jump_to(message_id.clone(), cx);
                        }))
                }))
                .into_any_element(),
        };
        Some(
            backdrop("inbox-backdrop", cx)
                .child(
                    div()
                        .id("inbox")
                        .absolute()
                        .top(px(TITLE_HEIGHT + 4.))
                        .right(px(12.))
                        .w(px(480.))
                        .max_h(px(600.))
                        .rounded(px(10.))
                        .bg(theme.popover)
                        .border_1()
                        .border_color(theme.input)
                        .shadow_lg()
                        .flex()
                        .flex_col()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(div().px_4().py_3().flex().items_center().gap_2().border_b_1().border_color(theme.border).child(Icon::new(Glyph::Inbox).size(px(20.))).child(div().font_weight(FontWeight::SEMIBOLD).child("Inbox")).child(div().text_color(theme.muted_foreground).child("· Mentions")))
                        .child(div().id("inbox-list").flex_1().min_h_0().overflow_y_scroll().p_3().child(body)),
                )
                .into_any_element(),
        )
    }

    // Discord's GIF picker: search on top, trending when empty, two columns of moving previews.
    fn gif_overlay(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let results = self.gif_picker.as_ref()?;
        let theme = cx.theme().clone();
        let column_width = (PICKER_SIZE.0 - 28.) / 2.;
        let body: AnyElement = match results {
            None => div().p_4().text_color(theme.muted_foreground).child("Loading GIFs…").into_any_element(),
            Some(Err(error)) => div().p_4().text_color(theme.danger).child(error.clone()).into_any_element(),
            Some(Ok(gifs)) => div()
                .flex()
                .flex_wrap()
                .gap_1()
                .children(gifs.iter().enumerate().map(|(i, g)| {
                    let url = g.url.clone();
                    let height = column_width * g.height / g.width.max(1.);
                    div()
                        .id(SharedString::from(format!("gif-{i}")))
                        .w(px(column_width))
                        .h(px(height))
                        .rounded(px(6.))
                        .overflow_hidden()
                        .bg(theme.background)
                        .cursor_pointer()
                        .hover(|s| s.opacity(0.85))
                        .child(img(g.preview.clone()).size_full().object_fit(gpui_kit::ObjectFit::Cover).rounded(px(6.)))
                        .on_click(cx.listener(move |this, _, window, cx| this.send_gif(url.clone(), window, cx)))
                }))
                .into_any_element(),
        };
        let viewport_top = TITLE_HEIGHT + 8.;
        Some(
            backdrop("gif-backdrop", cx)
                .child(
                    div()
                        .id("gif-picker")
                        .absolute()
                        .right(px(32.))
                        .bottom(px(96.))
                        .top(px(viewport_top.max(120.)))
                        .w(px(PICKER_SIZE.0))
                        .max_h(px(PICKER_SIZE.1 + 80.))
                        .rounded(px(10.))
                        .bg(theme.popover)
                        .border_1()
                        .border_color(theme.input)
                        .shadow_lg()
                        .flex()
                        .flex_col()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(div().p_2().child(div().h(px(34.)).px_2().rounded(px(6.)).bg(theme.background).flex().items_center().child(Input::new(&self.gif_search).appearance(false))))
                        .child(div().id("gif-scroll").flex_1().min_h_0().overflow_y_scroll().px_2().pb_2().child(body)),
                )
                .into_any_element(),
        )
    }

    // Discord's Go Live picker: the whole screen, or one app's windows, each with only its own sound.
    // Discord's Go Live window: pick a screen, app or window, set the quality, then Go Live.
    fn share_overlay(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let listing = self.share_picker.as_ref()?;
        let theme = cx.theme().clone();
        let choice = self.share_choice;
        let tile = |id: String, title: String, subtitle: String, source: crate::voice::ShareSource| {
            let selected = choice == Some(source);
            div()
                .id(SharedString::from(id))
                .h(px(56.))
                .px_3()
                .rounded(px(8.))
                .flex()
                .items_center()
                .gap_3()
                .cursor_pointer()
                .border_2()
                .border_color(if selected { palette::blurple() } else { gpui_kit::transparent_black() })
                .bg(theme.background)
                .hover(|s| s.bg(theme.foreground.opacity(0.08)))
                .child(Icon::new(if matches!(source, crate::voice::ShareSource::Screen) { Glyph::Screen } else { Glyph::ShareScreen }).size(px(22.)).text_color(theme.muted_foreground))
                .child(div().flex_1().min_w_0().flex().flex_col().child(div().truncate().font_weight(FontWeight::MEDIUM).child(title)).child(div().truncate().text_size(px(12.)).text_color(theme.muted_foreground).child(subtitle)))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.share_choice = Some(source);
                    cx.notify();
                }))
        };
        let heading = |text: &'static str| div().pt_2().text_size(px(12.)).font_weight(FontWeight::BOLD).text_color(theme.muted_foreground).child(text).into_any_element();
        let mut rows: Vec<AnyElement> = vec![heading("SCREENS"), tile("share-screen".into(), "Entire Screen".into(), "Shares all system audio except Rustcord".into(), crate::voice::ShareSource::Screen).into_any_element()];
        match listing {
            None => rows.push(div().p_3().text_color(theme.muted_foreground).child("Finding windows…").into_any_element()),
            Some(Err(error)) => rows.push(div().p_3().text_color(theme.danger).child(error.clone()).into_any_element()),
            Some(Ok(windows)) => {
                let mut apps: Vec<(String, i32)> = Vec::new();
                for w in windows {
                    if !apps.iter().any(|(_, pid)| *pid == w.pid) {
                        apps.push((w.app_name.clone(), w.pid));
                    }
                }
                rows.push(heading("APPLICATIONS"));
                rows.extend(apps.into_iter().map(|(name, pid)| tile(format!("share-app-{pid}"), name, "Shares only this app's audio".into(), crate::voice::ShareSource::App(pid)).into_any_element()));
                rows.push(heading("WINDOWS"));
                rows.extend(windows.iter().map(|w| tile(format!("share-window-{}", w.id), w.title.clone(), w.app_name.clone(), crate::voice::ShareSource::Window(w.id)).into_any_element()));
            }
        }
        let (resolution, fps, audio) = (self.prefs.stream_resolution, self.prefs.stream_fps, self.prefs.stream_audio);
        let segment = |id: String, label: String, on: bool| {
            div()
                .id(SharedString::from(id))
                .h(px(32.))
                .px_3()
                .rounded(px(8.))
                .flex()
                .items_center()
                .cursor_pointer()
                .text_size(px(14.))
                .font_weight(FontWeight::MEDIUM)
                .border_1()
                .border_color(if on { palette::blurple() } else { theme.input })
                .bg(if on { palette::blurple().opacity(0.16) } else { gpui_kit::transparent_black() })
                .hover(|s| s.bg(theme.foreground.opacity(0.08)))
                .child(label)
        };
        let resolutions = [(480, "480p"), (720, "720p"), (1080, "1080p"), (0, "Source")].map(|(value, label)| {
            segment(format!("res-{value}"), label.into(), resolution == value).on_click(cx.listener(move |this, _, _, cx| this.update_prefs(cx, |p| p.stream_resolution = value))).into_any_element()
        });
        let rates = [15, 30, 60].map(|value| {
            segment(format!("fps-{value}"), format!("{value} FPS"), fps == value).on_click(cx.listener(move |this, _, _, cx| this.update_prefs(cx, |p| p.stream_fps = value))).into_any_element()
        });
        let quality = div()
            .pt_3()
            .border_t_1()
            .border_color(theme.border)
            .flex()
            .flex_col()
            .gap_2()
            .child(div().text_size(px(12.)).font_weight(FontWeight::BOLD).text_color(theme.muted_foreground).child("STREAM QUALITY"))
            .child(div().flex().items_center().gap_2().child(div().w(px(90.)).text_size(px(14.)).child("Resolution")).children(resolutions))
            .child(div().flex().items_center().gap_2().child(div().w(px(90.)).text_size(px(14.)).child("Frame Rate")).children(rates))
            .child(
                div()
                    .id("stream-audio")
                    .flex()
                    .items_center()
                    .gap_2()
                    .cursor_pointer()
                    .text_size(px(14.))
                    .child(
                        div()
                            .size(px(20.))
                            .rounded(px(4.))
                            .border_1()
                            .border_color(if audio { palette::blurple() } else { theme.muted_foreground })
                            .bg(if audio { palette::blurple() } else { gpui_kit::transparent_black() })
                            .text_color(gpui_kit::white())
                            .flex()
                            .items_center()
                            .justify_center()
                            .children(audio.then(|| Icon::new(Glyph::Checkmark).size(px(14.)))),
                    )
                    .child("Stream with sound")
                    .on_click(cx.listener(|this, _, _, cx| this.update_prefs(cx, |p| p.stream_audio = !p.stream_audio))),
            );
        let go = div()
            .id("go-live")
            .h(px(38.))
            .px_4()
            .rounded(px(8.))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .bg(palette::blurple())
            .text_color(gpui_kit::white())
            .font_weight(FontWeight::MEDIUM)
            .hover(|s| s.opacity(0.9))
            .when(choice.is_none(), |b| b.opacity(0.5))
            .child("Go Live")
            .on_click(cx.listener(|this, _, _, cx| {
                if let Some(source) = this.share_choice.take() {
                    this.start_share(source, cx);
                }
            }));
        Some(
            backdrop("share-backdrop", cx)
                .bg(gpui_kit::black().opacity(0.6))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .id("share-picker")
                        .w(px(520.))
                        .max_h(px(640.))
                        .p_4()
                        .rounded(px(12.))
                        .bg(theme.popover)
                        .border_1()
                        .border_color(theme.input)
                        .shadow_lg()
                        .flex()
                        .flex_col()
                        .gap_3()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(div().text_size(px(20.)).font_weight(FontWeight::BOLD).child("Screen Share"))
                        .child(div().id("share-list").flex_1().min_h_0().overflow_y_scroll().flex().flex_col().gap_1().children(rows))
                        .child(quality)
                        .child(div().flex().justify_end().child(go)),
                )
                .into_any_element(),
        )
    }

    // The pins popover under the header's pin button: newest first, each with a Jump link.
    fn pins_overlay(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let pins = self.pins.as_ref()?;
        let theme = cx.theme().clone();
        let body: AnyElement = match pins {
            None => div().p_4().text_color(theme.muted_foreground).child("Loading pins…").into_any_element(),
            Some(Err(error)) => div().p_4().text_color(theme.danger).child(error.clone()).into_any_element(),
            Some(Ok(list)) if list.is_empty() => div().p_6().flex().flex_col().items_center().gap_2().text_color(theme.muted_foreground).child("This channel doesn't have any pinned messages… yet.").into_any_element(),
            Some(Ok(list)) => div()
                .flex()
                .flex_col()
                .gap_2()
                .children(list.iter().map(|m| {
                    let id = m.id.clone();
                    div()
                        .id(SharedString::from(format!("pin-{}", m.id)))
                        .p_3()
                        .rounded(px(8.))
                        .bg(theme.background)
                        .border_1()
                        .border_color(theme.border)
                        .flex()
                        .gap_3()
                        .child(img(m.author.avatar.clone()).size(px(32.)).rounded_full().flex_none())
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child(div().flex().items_center().gap_2().child(div().font_weight(FontWeight::MEDIUM).child(m.author.name.clone())).child(div().flex_1()).child(
                                    div()
                                        .id(SharedString::from(format!("jump-{}", m.id)))
                                        .text_size(px(12.))
                                        .text_color(theme.link)
                                        .cursor_pointer()
                                        .child("Jump")
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.pins = None;
                                            this.jump_to(id.clone(), cx);
                                        })),
                                ))
                                .child(div().text_size(px(14.)).line_clamp(4).child(m.content.clone())),
                        )
                }))
                .into_any_element(),
        };
        Some(
            backdrop("pins-backdrop", cx)
                .child(
                    div()
                        .id("pins")
                        .absolute()
                        .top(px(TITLE_HEIGHT + 52.))
                        .right(px(self.panel_width + 16.))
                        .w(px(420.))
                        .max_h(px(560.))
                        .rounded(px(10.))
                        .bg(theme.popover)
                        .border_1()
                        .border_color(theme.input)
                        .shadow_lg()
                        .flex()
                        .flex_col()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(div().px_4().py_3().flex().items_center().gap_2().border_b_1().border_color(theme.border).child(Icon::new(Glyph::Pin).size(px(20.))).child(div().font_weight(FontWeight::SEMIBOLD).child("Pinned Messages")))
                        .child(div().id("pins-list").flex_1().min_h_0().overflow_y_scroll().p_3().child(body)),
                )
                .into_any_element(),
        )
    }

    // The server name's dropdown, trimmed to what Rustcord can do.
    fn server_menu_overlay(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.server_menu {
            return None;
        }
        let guild_id = self.guild_id.clone()?;
        let theme = cx.theme().clone();
        let item = |id: &'static str, label: &'static str| {
            div()
                .id(id)
                .h(px(32.))
                .px_2()
                .rounded(px(4.))
                .flex()
                .items_center()
                .cursor_pointer()
                .text_size(px(14.))
                .font_weight(FontWeight::MEDIUM)
                .hover(|s| s.bg(palette::blurple()).text_color(gpui_kit::white()))
                .child(label)
        };
        let (read, copy) = (guild_id.clone(), guild_id);
        Some(
            backdrop("server-menu-backdrop", cx)
                .child(
                    div()
                        .id("server-menu")
                        .absolute()
                        .top(px(TITLE_HEIGHT + 52.))
                        .left(px(crate::layout::RAIL_WIDTH + 10.))
                        .w(px(self.channels_width - 20.))
                        .p_2()
                        .rounded(px(8.))
                        .bg(theme.popover)
                        .border_1()
                        .border_color(theme.input)
                        .shadow_lg()
                        .flex()
                        .flex_col()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(item("server-mark-read", "Mark As Read").on_click(cx.listener(move |this, _, _, cx| {
                            this.server_menu = false;
                            this.mark_guild_read(&read, cx);
                        })))
                        .child(div().my_1().h(px(1.)).bg(theme.border))
                        .child(item("server-copy-id", "Copy Server ID").on_click(cx.listener(move |this, _, _, cx| {
                            this.server_menu = false;
                            this.copy_text(copy.clone(), cx);
                            cx.notify();
                        }))),
                )
                .into_any_element(),
        )
    }

    // Discord's profile popout: banner, avatar with status, names, pronouns, about me, roles, and a
    // button to message them.
    fn profile_overlay(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let card = self.profile_card.as_ref()?;
        let theme = cx.theme().clone();
        let viewport = window.viewport_size();
        let height = 420.;
        let x = f32::from(card.at.x).clamp(8., f32::from(viewport.width) - CARD_WIDTH - 8.);
        let y = f32::from(card.at.y).clamp(TITLE_HEIGHT + 8., (f32::from(viewport.height) - height - 8.).max(TITLE_HEIGHT + 8.));
        let user = card.user.clone();
        let profile = card.profile.clone().unwrap_or_default();
        let accent: Hsla = profile.accent.map(|c| rgb(c).into()).unwrap_or(palette::blurple());
        let surface = theme.popover;
        let status = self.presences.get(&user.id).map(|p| p.status).or(self.me.as_ref().filter(|me| me.id == user.id).map(|_| self.my_status));
        let guild = card.guild_id.as_ref().and_then(|id| self.guilds.iter().find(|g| &g.id == id));
        let roles: Vec<(String, Hsla)> = guild
            .map(|g| {
                let mut roles: Vec<_> = g.roles.iter().filter(|r| profile.roles.contains(&r.id)).collect();
                roles.sort_by_key(|r| std::cmp::Reverse(r.position));
                roles.into_iter().map(|r| (r.name.clone(), if r.color == 0 { theme.muted_foreground } else { rgb(r.color).into() })).collect()
            })
            .unwrap_or_default();
        let name_color = self.role_color(card.guild_id.as_ref(), &user.id, Some(&profile.roles)).unwrap_or(theme.foreground);
        let is_me = self.me.as_ref().is_some_and(|me| me.id == user.id);
        let (dm_user, mention) = (user.id.clone(), user.clone());
        let banner = match &profile.banner {
            Some(url) => img(url.clone()).w_full().h(px(106.)).object_fit(gpui_kit::ObjectFit::Cover).into_any_element(),
            None => div().w_full().h(px(106.)).bg(accent).into_any_element(),
        };
        let section = |title: &'static str| div().text_size(px(12.)).font_weight(FontWeight::BOLD).text_color(theme.foreground).child(title);
        let body = div()
            .px_4()
            .pb_4()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(div().text_size(px(20.)).font_weight(FontWeight::BOLD).text_color(name_color).child(user.name.clone()))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .text_size(px(14.))
                            .text_color(theme.foreground.opacity(0.85))
                            .child(profile.username.clone())
                            .children(profile.pronouns.clone().map(|p| div().text_color(theme.muted_foreground).child(format!("• {p}"))))
                            .children(user.tag.clone().map(|tag| div().h(px(18.)).px_1().ml_1().rounded(px(4.)).bg(theme.foreground.opacity(0.08)).text_size(px(11.)).font_weight(FontWeight::BOLD).flex().items_center().child(tag.text))),
                    ),
            )
            .children(profile.bio.clone().map(|bio| div().flex().flex_col().gap_1().child(section("About Me")).child(div().text_size(px(14.)).line_clamp(6).child(bio))))
            .when(!roles.is_empty(), |b| {
                b.child(
                    div().flex().flex_col().gap_1().child(section("Roles")).child(div().flex().flex_wrap().gap_1().children(roles.into_iter().map(|(name, color)| {
                        div()
                            .h(px(22.))
                            .px_2()
                            .flex()
                            .items_center()
                            .gap_1()
                            .rounded(px(4.))
                            .bg(theme.foreground.opacity(0.06))
                            .text_size(px(12.))
                            .font_weight(FontWeight::MEDIUM)
                            .child(div().size(px(10.)).rounded_full().bg(color))
                            .child(name)
                    }))),
                )
            })
            .when(!is_me, |b| {
                b.child(
                    div()
                        .id("profile-message")
                        .h(px(36.))
                        .px_3()
                        .rounded(px(8.))
                        .flex()
                        .items_center()
                        .cursor_pointer()
                        .border_1()
                        .border_color(theme.input)
                        .bg(theme.background)
                        .text_color(theme.muted_foreground)
                        .hover(|s| s.border_color(theme.foreground.opacity(0.3)))
                        .child(format!("Message @{}", user.name))
                        .on_click(cx.listener(move |this, _, window, cx| this.message_user(dm_user.clone(), window, cx))),
                )
                .child(
                    div()
                        .id("profile-mention")
                        .text_size(px(13.))
                        .text_color(theme.link)
                        .cursor_pointer()
                        .child("Mention")
                        .on_click(cx.listener(move |this, _, window, cx| this.mention_user(&mention, window, cx))),
                )
            });
        let card_el = div()
            .id("profile-card")
            .absolute()
            .left(px(x))
            .top(px(y))
            .w(px(CARD_WIDTH))
            .rounded(px(12.))
            .overflow_hidden()
            .bg(surface)
            .border_1()
            .border_color(theme.input)
            .shadow_lg()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(banner)
            .child(div().px_4().mt(px(-44.)).child(div().p(px(5.)).rounded_full().bg(surface).w(px(90.)).child(avatar_with_status(&user.avatar, 80., status, surface))))
            .child(body);
        Some(backdrop("profile-backdrop", cx).child(card_el).into_any_element())
    }

    fn status_overlay(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.status_menu {
            return None;
        }
        let theme = cx.theme().clone();
        let options = [
            (Status::Online, "Online", None),
            (Status::Idle, "Idle", None),
            (Status::Dnd, "Do Not Disturb", Some("You will not receive desktop notifications")),
            (Status::Offline, "Invisible", Some("You will appear offline")),
        ];
        let rows = options.into_iter().map(|(status, label, hint)| {
            let selected = self.my_status == status;
            div()
                .id(SharedString::from(format!("status-{label}")))
                .px_2()
                .py_1p5()
                .rounded(px(6.))
                .flex()
                .gap_2()
                .cursor_pointer()
                .when(selected, |r| r.bg(theme.foreground.opacity(0.06)))
                .hover(|s| s.bg(palette::blurple()).text_color(gpui_kit::white()))
                .child(div().pt(px(4.)).child(status_mark(status, 10., theme.popover)))
                .child(div().flex_1().min_w_0().flex().flex_col().child(div().font_weight(FontWeight::MEDIUM).child(label)).children(hint.map(|h| div().text_size(px(12.)).opacity(0.75).child(h))))
                .on_click(cx.listener(move |this, _, _, cx| this.set_status(status, cx)))
        });
        let rows: Vec<_> = rows.collect();
        Some(
            backdrop("status-backdrop", cx)
                .child(
                    div()
                        .id("status-menu")
                        .absolute()
                        .left(px(16.))
                        .bottom(px(self.panel_stack_height() + 16.))
                        .w(px(300.))
                        .p_2()
                        .rounded(px(10.))
                        .bg(theme.popover)
                        .border_1()
                        .border_color(theme.input)
                        .shadow_lg()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .children(rows),
                )
                .into_any_element(),
        )
    }

    // Discord's emoji picker: search on top, the server's own emoji first, then Unicode by category.
    fn emoji_overlay(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        self.emoji_picker.as_ref()?;
        let theme = cx.theme().clone();
        let query = self.emoji_search.read(cx).value().to_lowercase();
        let custom: Vec<Emoji> = self.guild().map(|g| g.emojis.iter().filter(|e| query.is_empty() || e.name.to_lowercase().contains(&query)).cloned().collect()).unwrap_or_default();
        let mut sections: Vec<(SharedString, Vec<Emoji>)> = Vec::new();
        if !custom.is_empty() {
            sections.push((self.guild().map(|g| g.name.clone()).unwrap_or_default().into(), custom));
        }
        for group in emojis::Group::iter() {
            let items: Vec<Emoji> = group
                .emojis()
                .filter(|e| query.is_empty() || e.name().contains(&query) || e.shortcodes().any(|s| s.contains(&query)))
                .map(|e| Emoji::unicode(e.as_str()))
                .collect();
            if !items.is_empty() {
                sections.push((group_name(group).into(), items));
            }
        }
        let viewport = window.viewport_size();
        let cell = |emoji: Emoji, key: String| {
            let face = match emoji.url() {
                Some(url) => img(url).size(px(32.)).into_any_element(),
                None => div().text_size(px(28.)).child(emoji.name.clone()).into_any_element(),
            };
            div()
                .id(SharedString::from(key))
                .size(px(44.))
                .rounded(px(6.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .hover(|s| s.bg(theme.foreground.opacity(0.08)))
                .child(face)
                .on_click(cx.listener(move |this, _, window, cx| this.pick_emoji(emoji.clone(), window, cx)))
        };
        let body = div().flex().flex_col().gap_2().children(sections.into_iter().map(|(name, items)| {
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(div().px_1().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child(name.clone()))
                .child(div().flex().flex_wrap().children(items.into_iter().enumerate().map(|(i, e)| cell(e, format!("emoji-{name}-{i}")))))
        }));
        Some(
            backdrop("emoji-backdrop", cx)
                .child(
                    div()
                        .id("emoji-picker")
                        .absolute()
                        .right(px(32.))
                        .top(px((f32::from(viewport.height) - PICKER_SIZE.1 - 90.).max(TITLE_HEIGHT + 8.)))
                        .w(px(PICKER_SIZE.0))
                        .h(px(PICKER_SIZE.1))
                        .rounded(px(10.))
                        .bg(theme.popover)
                        .border_1()
                        .border_color(theme.input)
                        .shadow_lg()
                        .flex()
                        .flex_col()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(div().p_2().child(div().h(px(34.)).px_2().rounded(px(6.)).bg(theme.background).flex().items_center().child(Input::new(&self.emoji_search).appearance(false))))
                        .child(div().id("emoji-scroll").flex_1().min_h_0().overflow_y_scroll().px_2().pb_2().child(body)),
                )
                .into_any_element(),
        )
    }

    // Ctrl/Cmd+K: jump to any channel or DM by name, arrows to pick, Enter to go.
    fn switcher_overlay(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let selected = self.switcher?;
        let theme = cx.theme().clone();
        let results = self.switcher_results(cx);
        let rows = results.into_iter().enumerate().map(|(i, (guild_id, channel, place))| {
            let active = i == selected;
            let lead = match (&channel.recipient, channel.kind) {
                (Some(user), _) => img(user.avatar.clone()).size(px(20.)).rounded_full().into_any_element(),
                (None, ChannelKind::Dm) => Icon::new(Glyph::User).size(px(18.)).into_any_element(),
                _ => Icon::new(Glyph::Hashtag).size(px(18.)).into_any_element(),
            };
            div()
                .id(SharedString::from(format!("switch-{}", channel.id)))
                .h(px(40.))
                .px_3()
                .rounded(px(6.))
                .flex()
                .items_center()
                .gap_2()
                .cursor_pointer()
                .text_color(if active { theme.foreground } else { theme.foreground.opacity(0.8) })
                .when(active, |r| r.bg(theme.foreground.opacity(0.08)))
                .hover(|s| s.bg(theme.foreground.opacity(0.06)))
                .child(lead)
                .child(div().flex_1().truncate().font_weight(FontWeight::MEDIUM).child(channel.name.clone()))
                .child(div().text_size(px(12.)).text_color(theme.muted_foreground).child(place))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.switcher = None;
                    this.guild_id = guild_id.clone();
                    this.open_channel(channel.clone(), window, cx);
                }))
        });
        let rows: Vec<_> = rows.collect();
        Some(
            backdrop("switcher-backdrop", cx)
                .bg(gpui_kit::black().opacity(0.6))
                .child(
                    div()
                        .id("switcher")
                        .absolute()
                        .top(px(120.))
                        .left_0()
                        .right_0()
                        .flex()
                        .justify_center()
                        .child(
                            div()
                                .w(px(SWITCHER_WIDTH))
                                .p_4()
                                .rounded(px(12.))
                                .bg(theme.popover)
                                .border_1()
                                .border_color(theme.input)
                                .shadow_lg()
                                .flex()
                                .flex_col()
                                .gap_3()
                                .capture_key_down(cx.listener(Self::on_switcher_key))
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .child(div().h(px(48.)).px_3().rounded(px(8.)).bg(theme.background).flex().items_center().text_size(px(18.)).child(Input::new(&self.switcher_input).appearance(false)))
                                .child(div().flex().flex_col().gap(px(2.)).children(rows))
                                .child(div().text_size(px(12.)).text_color(theme.muted_foreground).child("Use ↑↓ to navigate, Enter to go, Esc to close")),
                        ),
                )
                .into_any_element(),
        )
    }
}

// A full-window catcher: clicking outside the floating surface closes it.
fn backdrop(id: &'static str, cx: &mut Context<Rustcord>) -> gpui_kit::Stateful<gpui_kit::Div> {
    div().id(id).absolute().inset_0().occlude().on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _, _, cx| {
            this.dismiss_overlay(cx);
        }),
    )
}

fn group_name(group: emojis::Group) -> &'static str {
    use emojis::Group::*;
    match group {
        SmileysAndEmotion => "Smileys & Emotion",
        PeopleAndBody => "People",
        AnimalsAndNature => "Nature",
        FoodAndDrink => "Food & Drink",
        TravelAndPlaces => "Travel & Places",
        Activities => "Activities",
        Objects => "Objects",
        Symbols => "Symbols",
        Flags => "Flags",
    }
}
