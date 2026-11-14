// Floating surfaces over the app: profile popout, status menu, emoji picker, quick switcher, and the
// actions the right-click menus run.
use std::{sync::Arc, time::Duration};

use gpui_kit::{
    Animation, AnimationExt, AnyElement, App, ClipboardItem, Context, Entity, FontWeight, Hsla, KeyDownEvent, MouseButton, Pixels, Point, RenderImage, SharedString, Window,
    component::{
        ActiveTheme, Icon, Theme,
        input::{Input, InputEvent},
    },
    div, img, prelude::*, px, relative, rgb,
};

use crate::{
    Channel, ChannelKind, Rustcord, EmojiTarget, ProfileCard, SearchState,
    discord::{Emoji, Message, Status, User},
    icons::Glyph,
    layout::{TITLE_HEIGHT, avatar_with_status, status_mark},
    motion, palette,
    voice::ShareSource,
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
            for channel in guild.channels.iter().filter(|c| c.kind == ChannelKind::Text && guild.can_view(self.my_id(), c)) {
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
            | self.close_share_picker(cx)
            | self.pins.take().is_some()
            | self.inbox.take().is_some()
            | self.gif_picker.take().is_some()
            | self.dismiss_call_menus()
            | self.close_menu();
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

    // Discord's Go Live picker: what to share on the left, tabs over a grid of live thumbnails; on the
    // right a preview of the pick, the stream's quality and sound, and Go Live.
    fn share_overlay(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let picker = self.share_picker.as_ref()?;
        let theme = cx.theme().clone();
        // gpui has no transform on a div, so the open's scale is the dialog's own size; the grid and
        // panel are sized relative to it and scale along.
        let dialog = div()
            .id("share-picker")
            .max_h(relative(0.92))
            .rounded(px(12.))
            .bg(theme.popover)
            .border_1()
            .border_color(theme.input)
            .shadow_lg()
            .flex()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(self.share_sources(picker, cx))
            .child(self.share_panel(picker, cx))
            .with_animation("share-open", Animation::new(Duration::from_millis(200)), |dialog, t| {
                let scale = 0.96 + 0.04 * motion::back_out(t);
                dialog.w(px(SHARE_DIALOG.0 * scale)).h(px(SHARE_DIALOG.1 * scale)).opacity(ease_out((t / 0.6).min(1.)))
            });
        Some(
            backdrop("share-backdrop", cx)
                .flex()
                .items_center()
                .justify_center()
                .child(dialog)
                .with_animation("share-dim", Animation::new(Duration::from_millis(150)), |backdrop, t| backdrop.bg(gpui_kit::black().opacity(0.6 * t)))
                .into_any_element(),
        )
    }

    fn share_sources(&self, picker: &SharePicker, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let tab = picker.tab;
        let tabs = [(ShareTab::Applications, "Applications"), (ShareTab::Screen, "Entire Screen"), (ShareTab::Devices, "Devices")].map(|(id, text)| {
            let on = tab == id;
            div()
                .id(SharedString::from(format!("share-tab-{text}")))
                .h(px(32.))
                .px_3()
                .rounded(px(8.))
                .flex()
                .items_center()
                .cursor_pointer()
                .font_weight(FontWeight::MEDIUM)
                .text_color(if on { palette::strong(cx) } else { theme.muted_foreground })
                .when(on, |t| t.bg(theme.foreground.opacity(0.08)))
                .hover(|s| s.text_color(palette::strong(cx)))
                .child(text)
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(picker) = &mut this.share_picker {
                        picker.tab = id;
                    }
                    cx.notify();
                }))
        });
        let note = |text: SharedString, color| div().py_8().flex().justify_center().text_color(color).child(text).into_any_element();
        let body: AnyElement = match tab {
            ShareTab::Applications => match &picker.windows {
                None => note("Finding windows…".into(), theme.muted_foreground),
                Some(Err(error)) => note(error.clone(), theme.danger),
                Some(Ok(windows)) if windows.is_empty() => note("No windows to share".into(), theme.muted_foreground),
                Some(Ok(windows)) => grid(windows.iter().map(|w| self.share_tile(picker, ShareSource::Window(w.id), w.title.clone(), cx)).collect()),
            },
            ShareTab::Screen => match &picker.screens {
                None => note("Finding screens…".into(), theme.muted_foreground),
                Some(Err(error)) => note(error.clone(), theme.danger),
                Some(Ok(screens)) => grid(screens.iter().map(|s| self.share_tile(picker, ShareSource::Screen(s.id), s.name.clone(), cx)).collect()),
            },
            // GAP: capture cards aren't supported by the voice engine yet.
            ShareTab::Devices => note("No capture devices found".into(), theme.muted_foreground),
        };
        div()
            .flex_1()
            .min_w_0()
            .p_4()
            .flex()
            .flex_col()
            .gap_3()
            .child(div().text_size(px(20.)).font_weight(FontWeight::BOLD).text_color(palette::strong(cx)).child("Screen Share"))
            .child(div().flex().gap_1().children(tabs))
            // Bled 4px past the column so the selection ring outside a tile isn't clipped by the scroller.
            .child(div().id("share-list").flex_1().min_h_0().mx(px(-4.)).overflow_y_scroll().child(div().p(px(4.)).child(body)))
    }

    fn share_tile(&self, picker: &SharePicker, source: ShareSource, name: String, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let selected = self.share_choice == Some(source);
        let key = source_key(source);
        let label = name.clone();
        div()
            .id(SharedString::from(format!("share-{key}")))
            .min_w_0()
            .flex()
            .flex_col()
            .gap_2()
            .cursor_pointer()
            .child(
                div()
                    .relative()
                    .w_full()
                    .aspect_ratio(16. / 9.)
                    .child(div().size_full().rounded(px(8.)).overflow_hidden().bg(gpui_kit::black()).child(source_still(picker.thumb(source), glyph(source), 32., &theme)))
                    .children(selected.then(|| selection_ring(&key, picker.picked))),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_size(px(14.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(if selected { palette::strong(cx) } else { theme.foreground })
                    .child(source_icon(picker.icon(source), glyph(source), &theme))
                    .child(div().min_w_0().truncate().child(name)),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.pick_share(source, label.clone(), cx)))
            .into_any_element()
    }

    // The darker side: the pick, large; quality as joined segments; the sound switch; Go Live at the foot.
    fn share_panel(&self, picker: &SharePicker, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let (resolution, fps, audio) = (self.prefs.stream_resolution, self.prefs.stream_fps, self.prefs.stream_audio);
        let resolutions = [(720, "720p"), (1080, "1080p"), (1440, "1440p"), (0, "Source")].map(|(value, label)| {
            segment(format!("res-{value}"), label, resolution == value, cx).on_click(cx.listener(move |this, _, _, cx| this.update_prefs(cx, |p| p.stream_resolution = value))).into_any_element()
        });
        let rates = [(15, "15"), (30, "30"), (60, "60")].map(|(value, label)| {
            segment(format!("fps-{value}"), label, fps == value, cx).on_click(cx.listener(move |this, _, _, cx| this.update_prefs(cx, |p| p.stream_fps = value))).into_any_element()
        });
        let whole_screen = match self.share_choice {
            Some(source) => matches!(source, ShareSource::Screen(_)),
            None => picker.tab == ShareTab::Screen,
        };
        let sound = div()
            .id("stream-audio")
            .flex()
            .items_center()
            .gap_3()
            .cursor_pointer()
            .child(div().flex_1().min_w_0().text_size(px(14.)).child(if whole_screen { "Share all sound except Rustcord" } else { "Share this app's sound" }))
            .child(crate::settings::switch(audio, &theme))
            .on_click(cx.listener(|this, _, _, cx| this.update_prefs(cx, |p| p.stream_audio = !p.stream_audio)));
        let ready = self.share_choice.is_some();
        let go = div()
            .id("go-live")
            .h(px(40.))
            .rounded(px(8.))
            .flex()
            .items_center()
            .justify_center()
            .bg(theme.primary)
            .text_color(theme.primary_foreground)
            .font_weight(FontWeight::SEMIBOLD)
            .when(ready, |b| b.cursor_pointer().hover(|s| s.bg(theme.primary_hover)).active(|s| s.bg(theme.primary_active)))
            .when(!ready, |b| b.opacity(0.5))
            .child("Go Live")
            .on_click(cx.listener(|this, _, _, cx| this.go_live_from_picker(cx)));
        div()
            .w(relative(SHARE_PANEL))
            .flex_none()
            .p_4()
            .rounded_r(px(11.))
            .bg(gpui_kit::black().opacity(if theme.is_dark() { 0.32 } else { 0.05 }))
            .flex()
            .flex_col()
            .gap_4()
            .child(self.share_preview(picker, cx))
            .child(quality("Resolution", resolutions, cx))
            .child(quality("Frame rate", rates, cx))
            .child(sound)
            .child(div().flex_1())
            .child(div().text_size(px(14.)).text_color(theme.muted_foreground).child(self.share_hint()))
            .child(go)
    }

    // The pick, large, with its icon and name. A new pick fades in over the last one.
    fn share_preview(&self, picker: &SharePicker, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let choice = self.share_choice;
        let still = |source: ShareSource| source_still(picker.thumb(source), glyph(source), 40., &theme);
        let current = choice.map(|source| {
            if !picker.picked {
                return still(source).into_any_element();
            }
            still(source).with_animation(SharedString::from(format!("preview-{}", source_key(source))), Animation::new(Duration::from_millis(160)), |layer, t| layer.opacity(ease_out(t))).into_any_element()
        });
        let name: SharedString = match (choice, &self.share_choice_label) {
            (Some(_), Some(label)) => label.clone().into(),
            _ => "Choose what to share".into(),
        };
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .relative()
                    .w_full()
                    .aspect_ratio(16. / 9.)
                    .rounded(px(8.))
                    .overflow_hidden()
                    .bg(gpui_kit::black())
                    .when(choice.is_none(), |box_| box_.child(source_still(None, Glyph::ShareScreen, 40., &theme)))
                    .children(picker.previous.filter(|_| picker.picked).map(still))
                    .children(current),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .min_w_0()
                    .text_size(px(14.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(if choice.is_some() { palette::strong(cx) } else { theme.muted_foreground })
                    .children(choice.map(|source| source_icon(picker.icon(source), glyph(source), &theme)))
                    .child(div().min_w_0().truncate().child(name)),
            )
    }

    fn share_hint(&self) -> String {
        match &self.call {
            Some(call) => format!("Everyone in {} can watch.", call.channel.name),
            None => "Everyone in the voice channel can watch.".into(),
        }
    }

    fn pick_share(&mut self, source: ShareSource, label: String, cx: &mut Context<Self>) {
        if self.share_choice == Some(source) {
            return;
        }
        let Some(picker) = &mut self.share_picker else { return };
        picker.previous = self.share_choice;
        picker.picked = true;
        self.share_choice = Some(source);
        self.share_choice_label = Some(label);
        cx.notify();
    }

    fn go_live_from_picker(&mut self, cx: &mut Context<Self>) {
        let Some(source) = self.share_choice.take() else { return };
        let label = self.share_choice_label.take().unwrap_or_default();
        let windows = self.share_picker.as_ref().and_then(|p| p.windows.as_ref()?.as_ref().ok().cloned()).unwrap_or_default();
        self.share_and_remember(source, label, &windows, cx);
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

fn grid(tiles: Vec<AnyElement>) -> AnyElement {
    div().grid().grid_cols(2).gap_4().children(tiles).into_any_element()
}

fn source_key(source: ShareSource) -> String {
    match source {
        ShareSource::Screen(id) => format!("screen-{id}"),
        ShareSource::App(pid) => format!("app-{pid}"),
        ShareSource::Window(id) => format!("window-{id}"),
    }
}

fn glyph(source: ShareSource) -> Glyph {
    match source {
        ShareSource::Screen(_) => Glyph::Screen,
        ShareSource::App(_) | ShareSource::Window(_) => Glyph::ShareScreen,
    }
}

// A source's live still filling its box, or its glyph until the still arrives.
fn source_still(thumb: Option<Arc<RenderImage>>, glyph: Glyph, glyph_size: f32, theme: &Theme) -> gpui_kit::Div {
    div().absolute().inset_0().flex().items_center().justify_center().bg(gpui_kit::black()).child(match thumb {
        Some(image) => img(image).size_full().object_fit(gpui_kit::ObjectFit::Contain).into_any_element(),
        None => Icon::new(glyph).size(px(glyph_size)).text_color(theme.muted_foreground).into_any_element(),
    })
}

fn source_icon(icon: Option<Arc<RenderImage>>, glyph: Glyph, theme: &Theme) -> AnyElement {
    match icon {
        Some(image) => img(image).size(px(20.)).flex_none().into_any_element(),
        None => Icon::new(glyph).size(px(20.)).flex_none().text_color(theme.muted_foreground).into_any_element(),
    }
}

// The selected tile's blurple outline: springs out from inside the thumbnail's edge to 3px past it,
// overshooting a touch. Still when the selection wasn't the user's doing (the picker's preselection).
fn selection_ring(key: &str, animate: bool) -> AnyElement {
    let ring = div().absolute().border_2().border_color(palette::blurple());
    if !animate {
        return place_ring(ring, 1., 1.).into_any_element();
    }
    ring.with_animation(SharedString::from(format!("ring-{key}")), Animation::new(Duration::from_millis(220)), |ring, t| place_ring(ring, motion::back_out(t), (t / 0.4).min(1.)))
        .into_any_element()
}

fn place_ring(ring: gpui_kit::Div, grow: f32, alpha: f32) -> gpui_kit::Div {
    let offset = 2. - 5. * grow;
    ring.top(px(offset)).left(px(offset)).right(px(offset)).bottom(px(offset)).rounded(px(8. - offset)).opacity(alpha)
}

// One labelled row of joined segments; the chosen one reads as chosen, not as a primary button.
fn quality(title: &'static str, segments: impl IntoIterator<Item = AnyElement>, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(div().text_size(px(14.)).font_weight(FontWeight::MEDIUM).text_color(theme.muted_foreground).child(title))
        .child(div().p(px(2.)).rounded(px(8.)).bg(theme.foreground.opacity(0.06)).flex().children(segments))
}

fn segment(id: String, label: &'static str, on: bool, cx: &App) -> gpui_kit::Stateful<gpui_kit::Div> {
    let theme = cx.theme();
    let (hover, strong) = (theme.foreground, palette::strong(cx));
    div()
        .id(SharedString::from(id))
        .flex_1()
        .min_w_0()
        .h(px(30.))
        .rounded(px(6.))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .text_size(px(14.))
        .font_weight(FontWeight::MEDIUM)
        .border_1()
        .border_color(if on { theme.foreground.opacity(0.2) } else { gpui_kit::transparent_black() })
        .when(on, |s| s.bg(theme.foreground.opacity(0.12)).text_color(strong))
        .when(!on, |s| s.text_color(theme.muted_foreground).hover(move |s| s.text_color(hover)))
        .child(label)
}

// Ease-out cubic, for fades that should land softly beside a spring.
fn ease_out(t: f32) -> f32 {
    1. - (1. - t).powi(3)
}

const SHARE_DIALOG: (f32, f32) = (720., 560.);
// The side panel's share of the dialog's width (~250px of 720), so it scales with the open.
const SHARE_PANEL: f32 = 250. / 720.;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ShareTab {
    Applications,
    Screen,
    Devices,
}

pub struct SharePicker {
    tab: ShareTab,
    windows: Option<Result<Vec<crate::voice::ShareableWindow>, SharedString>>,
    screens: Option<Result<Vec<crate::voice::ShareableScreen>, SharedString>>,
    thumbs: Vec<(ShareSource, Arc<RenderImage>)>,
    icons: Vec<(i32, Arc<RenderImage>)>,
    // The pick before the current one, which the preview fades from.
    previous: Option<ShareSource>,
    // Whether the user has picked; the preselection lands without motion.
    picked: bool,
}

impl SharePicker {
    fn thumb(&self, source: ShareSource) -> Option<Arc<RenderImage>> {
        self.thumbs.iter().find(|(s, _)| *s == source).map(|(_, image)| image.clone())
    }

    fn icon(&self, source: ShareSource) -> Option<Arc<RenderImage>> {
        let pid = match source {
            ShareSource::Screen(_) => return None,
            ShareSource::App(pid) => pid,
            ShareSource::Window(id) => self.windows.as_ref()?.as_ref().ok()?.iter().find(|w| w.id == id)?.pid,
        };
        self.icons.iter().find(|(p, _)| *p == pid).map(|(_, image)| image.clone())
    }
}

impl Rustcord {
    // Opens the picker, lists what can be shared, preselects the last source (or Screen 1), then fills
    // in icons and thumbnails off the UI thread, the preselected source's first.
    pub(crate) fn open_share_picker(&mut self, cx: &mut Context<Self>) {
        self.empty_share_picker(cx);
        let listing = cx.background_spawn(async { (crate::voice::shareable_windows(), crate::voice::screens()) });
        cx.spawn(async move |this, cx| {
            let (windows, screens) = listing.await;
            let mut pids: Vec<i32> = windows.iter().flatten().map(|w| w.pid).collect();
            pids.sort_unstable();
            pids.dedup();
            let mut sources: Vec<ShareSource> = screens.iter().flatten().map(|s| ShareSource::Screen(s.id)).chain(windows.iter().flatten().map(|w| ShareSource::Window(w.id))).collect();
            let open = this.update(cx, |this, cx| this.list_shareables(windows, screens, cx)).ok().flatten();
            let Some(first) = open else { return };
            sources.sort_by_key(|s| Some(*s) != first);
            let icons = cx.background_spawn(async move { pids.into_iter().filter_map(|pid| Some((pid, crate::voice::app_icon(pid, 40)?))).collect::<Vec<_>>() }).await;
            let icons: Vec<_> = icons.iter().filter_map(|(pid, frame)| Some((*pid, crate::render_image(frame)?))).collect();
            let kept = this.update(cx, |this, cx| {
                let Some(picker) = &mut this.share_picker else { return false };
                picker.icons.extend(icons.iter().cloned());
                cx.notify();
                true
            });
            if !kept.unwrap_or(false) {
                cx.update(|cx| icons.into_iter().for_each(|(_, image)| cx.drop_image(image, None)));
                return;
            }
            for source in sources {
                let Ok(frame) = cx.background_spawn(async move { crate::voice::thumbnail(source, 400) }).await else { continue };
                let Some(image) = crate::render_image(&frame) else { continue };
                let kept = this.update(cx, |this, cx| {
                    let Some(picker) = &mut this.share_picker else { return false };
                    picker.thumbs.push((source, image.clone()));
                    cx.notify();
                    true
                });
                if !kept.unwrap_or(false) {
                    cx.update(|cx| cx.drop_image(image, None));
                    return;
                }
            }
        })
        .detach();
    }

    pub(crate) fn empty_share_picker(&mut self, cx: &mut Context<Self>) {
        self.close_share_picker(cx);
        self.share_picker = Some(SharePicker { tab: ShareTab::Applications, windows: None, screens: None, thumbs: Vec::new(), icons: Vec::new(), previous: None, picked: false });
        self.share_choice = None;
        self.share_choice_label = None;
        cx.notify();
    }

    // Fills the open picker's lists and preselects; the selection, or None once the picker has closed.
    pub(crate) fn list_shareables(
        &mut self,
        windows: Result<Vec<crate::voice::ShareableWindow>, String>,
        screens: Result<Vec<crate::voice::ShareableScreen>, String>,
        cx: &mut Context<Self>,
    ) -> Option<Option<ShareSource>> {
        let preselected = crate::go_live::preselect(self.prefs.last_share.as_ref(), windows.as_deref().unwrap_or_default(), screens.as_deref().unwrap_or_default());
        let picker = self.share_picker.as_mut()?;
        picker.windows = Some(windows.map_err(Into::into));
        picker.screens = Some(screens.map_err(Into::into));
        if self.share_choice.is_none()
            && let Some((source, label)) = preselected
        {
            picker.tab = if matches!(source, ShareSource::Screen(_)) { ShareTab::Screen } else { ShareTab::Applications };
            self.share_choice = Some(source);
            self.share_choice_label = Some(label);
        }
        cx.notify();
        Some(self.share_choice)
    }

    // Closes the picker and frees its thumbnails; true when it was open.
    pub(crate) fn close_share_picker(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(picker) = self.share_picker.take() else { return false };
        for image in picker.thumbs.into_iter().map(|(_, i)| i).chain(picker.icons.into_iter().map(|(_, i)| i)) {
            cx.drop_image(image, None);
        }
        true
    }
}
