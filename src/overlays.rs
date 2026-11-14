// Floating surfaces over the app: profile popout, status menu, emoji picker, quick switcher, and the
// actions the right-click menus run.
use std::{sync::Arc, time::Duration};

use gpui_kit::{
    Animation, AnimationExt, AnyElement, App, ClipboardItem, Context, Entity, FontWeight, HighlightStyle, Hsla, KeyDownEvent, MouseButton, Pixels, Point, RenderImage, ScrollHandle, SharedString, StyledText, Window,
    component::{
        ActiveTheme, Icon, Theme,
        input::{Input, InputEvent},
    },
    div, img, prelude::*, px, relative, rgb,
};

use crate::{
    Channel, ChannelKind, Rustcord, EmojiTarget, ProfileCard, SearchState,
    discord::{Activity, ActivityKind, Emoji, Message, Status, User},
    icons::Glyph,
    layout::{TITLE_HEIGHT, avatar_with_status},
    menu, motion, palette,
    voice::ShareSource,
};

const PROFILE_WIDTH: f32 = 320.;
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
                        .child(div().flex().items_center().gap_2().child(crate::ui::avatar(m.author.avatar.clone(), 24.)).child(div().font_weight(FontWeight::MEDIUM).child(m.author.name.clone())))
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
                    crate::ui::surface(cx)
                        .id("inbox")
                        .absolute()
                        .top(px(TITLE_HEIGHT + 4.))
                        .right(px(12.))
                        .w(px(480.))
                        .max_h(px(600.))
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
                    crate::ui::surface(cx)
                        .id("gif-picker")
                        .absolute()
                        .right(px(32.))
                        .bottom(px(96.))
                        .top(px(viewport_top.max(120.)))
                        .w(px(PICKER_SIZE.0))
                        .max_h(px(PICKER_SIZE.1 + 80.))
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
                    .child(div().size_full().rounded(px(8.)).overflow_hidden().bg(gpui_kit::black()).child(crate::ui::edge(source_still(picker.thumb(source), glyph(source), 32., &theme), crate::ui::all(8.))))
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
            crate::ui::segment(format!("res-{value}"), label, resolution == value, cx).on_click(cx.listener(move |this, _, _, cx| this.update_prefs(cx, |p| p.stream_resolution = value))).into_any_element()
        });
        let rates = [(15, "15"), (30, "30"), (60, "60")].map(|(value, label)| {
            crate::ui::segment(format!("fps-{value}"), label, fps == value, cx).on_click(cx.listener(move |this, _, _, cx| this.update_prefs(cx, |p| p.stream_fps = value))).into_any_element()
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
            .child(div().flex_1().min_w_0().text_size(px(14.)).child(if whole_screen { "Share all sound except Wave" } else { "Share this app's sound" }))
            .child(crate::ui::switch(audio, cx))
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
        let still = |source: ShareSource| crate::ui::edge(source_still(picker.thumb(source), glyph(source), 40., &theme), crate::ui::all(8.));
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
                    .when(choice.is_none(), |box_| box_.child(crate::ui::edge(source_still(None, Glyph::ShareScreen, 40., &theme), crate::ui::all(8.))))
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

    // Points at the Toggle Screen Share keybind while one works; otherwise says who can watch.
    fn share_hint(&self) -> String {
        let keybind = self.prefs.keybinds.iter().filter(|k| k.action == crate::hotkeys::Action::ToggleScreenShare).find_map(|k| k.keys.as_ref()).filter(|keys| !self.keybind_errors.contains(keys));
        if let Some(keys) = keybind {
            return format!("Press {} in any app to start or stop streaming.", crate::hotkeys::display(keys));
        }
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
                        .child(crate::ui::avatar(m.author.avatar.clone(), 32.))
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
                    crate::ui::surface(cx)
                        .id("pins")
                        .absolute()
                        .top(px(TITLE_HEIGHT + 52.))
                        .right(px(self.panel_width + 16.))
                        .w(px(420.))
                        .max_h(px(560.))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(div().px_4().py_3().flex().items_center().gap_2().border_b_1().border_color(theme.border).child(Icon::new(Glyph::Pin).size(px(20.))).child(div().font_weight(FontWeight::SEMIBOLD).child("Pinned Messages")))
                        .child(div().id("pins-list").flex_1().min_h_0().overflow_y_scroll().p_3().child(body)),
                )
                .into_any_element(),
        )
    }

    // The profile popout (overlays.css `.profile`): banner, an 80px avatar ringed in the card's colour,
    // name and username, what they're up to, about me, roles, and a field to message them.
    fn profile_overlay(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let card = self.profile_card.as_ref()?;
        let viewport = window.viewport_size();
        let user = card.user.clone();
        let profile = card.profile.clone().unwrap_or_default();
        let overlay = palette::surface(palette::Level::Overlay, cx);
        let presence = self.presences.get(&user.id);
        let status = presence.map(|p| p.status).or(self.me.as_ref().filter(|me| me.id == user.id).map(|_| self.my_status));
        let guild = card.guild_id.as_ref().and_then(|id| self.guilds.iter().find(|g| &g.id == id));
        let roles: Vec<(String, Hsla)> = guild
            .map(|g| {
                let mut roles: Vec<_> = g.roles.iter().filter(|r| profile.roles.contains(&r.id)).collect();
                roles.sort_by_key(|r| std::cmp::Reverse(r.position));
                roles.into_iter().map(|r| (r.name.clone(), if r.color == 0 { palette::status_in(Status::Offline, cx) } else { rgb(r.color).into() })).collect()
            })
            .unwrap_or_default();
        let is_me = self.me.as_ref().is_some_and(|me| me.id == user.id);
        let doing = presence.and_then(|p| p.activity.as_ref()).map(activity_line);
        let handle = [Some(profile.username.clone()).filter(|u| !u.is_empty()), profile.pronouns.clone()].into_iter().flatten().collect::<Vec<_>>().join(" · ");
        let height = 360. + if roles.is_empty() { 0. } else { 56. } + if profile.bio.is_some() { 64. } else { 0. };
        let x = f32::from(card.at.x).clamp(8., f32::from(viewport.width) - PROFILE_WIDTH - 8.);
        let y = f32::from(card.at.y).clamp(TITLE_HEIGHT + 8., (f32::from(viewport.height) - height - 8.).max(TITLE_HEIGHT + 8.));
        let banner = match (&profile.banner, profile.accent) {
            (Some(url), _) => crate::ui::media(url.clone(), crate::ui::top(crate::ui::RADIUS_LG)).w_full().h(px(64.)).object_fit(gpui_kit::ObjectFit::Cover).into_any_element(),
            (None, Some(accent)) => {
                let accent: Hsla = rgb(accent).into();
                let deep = Hsla { l: accent.l * 0.78, ..accent };
                crate::ui::edge(div().w_full().h(px(64.)).bg(gpui_kit::linear_gradient(135., gpui_kit::linear_color_stop(accent, 0.), gpui_kit::linear_color_stop(deep, 1.))), crate::ui::top(crate::ui::RADIUS_LG)).into_any_element()
            }
            // No banner colour: the neutral fill, as the design's worst case.
            (None, None) => crate::ui::edge(div().w_full().h(px(64.)).bg(palette::selected(cx)), crate::ui::top(crate::ui::RADIUS_LG)).into_any_element(),
        };
        let who = div()
            .px_4()
            .mt(px(-40.))
            .flex()
            .items_end()
            .justify_between()
            .gap_2()
            .child(div().p(px(5.)).rounded_full().bg(overlay).child(avatar_with_status(&user.avatar, 80., status, overlay)))
            .when(is_me, |who| {
                who.child(
                    crate::ui::small(crate::ui::button("profile-edit", crate::ui::Tone::Secondary, cx))
                        .child(Icon::new(Glyph::Pen).size(px(16.)))
                        .child("Edit profile")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.profile_card = None;
                            this.open_settings(crate::settings::Page::Account, window, cx);
                        })),
                )
            });
        let block = |title: &'static str| div().flex().flex_col().gap(px(6.)).child(div().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(palette::muted(cx)).child(title));
        let body = div()
            .pt_3()
            .px_4()
            .pb_4()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(div().flex().items_center().gap_2().min_w_0().child(div().truncate().text_size(px(20.)).line_height(px(24.)).font_weight(FontWeight::BOLD).text_color(palette::strong(cx)).child(user.name.clone())).children(
                        user.tag.clone().map(|tag| crate::ui::badge(palette::selected(cx), palette::text(cx)).child(tag.text)),
                    ))
                    .when(!handle.is_empty(), |n| n.child(div().truncate().text_color(palette::muted(cx)).child(handle))),
            )
            .children(doing.map(|line| div().py(px(8.)).px(px(10.)).rounded(px(crate::ui::RADIUS_MD)).bg(palette::selected(cx)).text_color(palette::text(cx)).child(line)))
            .children(profile.bio.clone().map(|bio| block("About me").child(div().text_color(palette::text(cx)).line_clamp(6).child(bio))))
            .when(!roles.is_empty(), |b| b.child(block("Roles").child(div().flex().flex_wrap().gap_1().children(roles.into_iter().map(|(name, color)| role_chip(name, color, cx))))))
            .when(!is_me, |b| {
                let dm_user = user.id.clone();
                b.child(
                    menu::field_frame(cx)
                        .id("profile-message")
                        .cursor_text()
                        .child(div().truncate().text_color(palette::faint(cx)).child(format!("Message @{}", user.name)))
                        .on_click(cx.listener(move |this, _, window, cx| this.message_user(dm_user.clone(), window, cx))),
                )
            });
        let card_el = div()
            .id("profile-card")
            .absolute()
            .left(px(x))
            .top(px(y))
            .w(px(PROFILE_WIDTH))
            .rounded(px(crate::ui::RADIUS_LG))
            .bg(overlay)
            .shadow(menu::pop_shadow(cx))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(banner)
            .child(who)
            .child(body);
        Some(backdrop("profile-backdrop", cx).child(menu::rise(card_el, format!("profile-{}", user.id), Duration::ZERO)).into_any_element())
    }

    // Your presence, as a menu of radio rows each led by its mark (auth.html's status menu).
    fn status_overlay(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.status_menu {
            return None;
        }
        let options = [(Status::Online, "Online"), (Status::Idle, "Idle"), (Status::Dnd, "Do not disturb"), (Status::Offline, "Invisible")];
        let rows: Vec<_> = options
            .into_iter()
            .map(|(status, label)| {
                let row = menu::item(label, |_, _, _| {}).radio(self.my_status == status).status(status);
                menu::menu_item_row(SharedString::from(format!("status-{label}")), &row, false, true, cx).on_click(cx.listener(move |this, _, _, cx| this.set_status(status, cx)))
            })
            .collect();
        let card = menu::menu_surface(menu::WIDTH, cx)
            .id("status-menu")
            .absolute()
            .left(px(16.))
            .bottom(px(self.panel_stack_height() + 16.))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .children(rows);
        Some(backdrop("status-backdrop", cx).child(menu::rise(card, "status-menu".into(), Duration::ZERO)).into_any_element())
    }

    // The emoji picker (messages.css `.picker`): search on top, categories down the left, the grid in
    // the middle and the hovered emoji named at the foot. Recent first, then the server's own, then Unicode.
    fn emoji_overlay(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        self.emoji_picker.as_ref()?;
        let sections = self.emoji_sections(cx);
        let viewport = window.viewport_size();
        let scroll = window.use_keyed_state("emoji-scroll-handle", cx, |_, _| ScrollHandle::new()).read(cx).clone();
        let hovered = window.use_keyed_state("emoji-hovered", cx, |_, _| None::<(Emoji, SharedString)>);
        let preview = hovered.read(cx).clone().or_else(|| sections.first().and_then(|s| Some((s.items.first()?.clone(), s.source.clone()))));
        let in_view = scroll.top_item();
        let rail = div()
            .id("emoji-rail")
            .w(px(48.))
            .flex_none()
            .px(px(6.))
            .pb(px(6.))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(2.))
            .overflow_y_scroll()
            .border_r_1()
            .border_color(palette::line(cx))
            .children(sections.iter().enumerate().map(|(i, section)| {
                let (selected, hover) = (palette::selected(cx), palette::hover(cx));
                let scroll = scroll.clone();
                div()
                    .id(("emoji-cat", i))
                    .size(px(36.))
                    .flex_none()
                    .rounded(px(crate::ui::RADIUS_SM))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .when(i == in_view, |d| d.bg(selected))
                    .when(i != in_view, |d| d.hover(move |s| s.bg(hover)))
                    .child(section.lead.render(cx))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        scroll.scroll_to_top_of_item(i);
                        cx.notify();
                    }))
            }));
        let cell = |emoji: Emoji, source: SharedString, key: String| {
            let face = match emoji.url() {
                Some(url) => img(url).size(px(28.)).into_any_element(),
                None => div().text_size(px(26.)).line_height(px(26.)).child(emoji.name.clone()).into_any_element(),
            };
            let (picked, shown) = (emoji.clone(), (emoji, source));
            let hovered = hovered.clone();
            div()
                .id(SharedString::from(key))
                .w(relative(1. / 9.))
                .h(px(40.))
                .rounded(px(crate::ui::RADIUS_SM))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .hover(|s| s.bg(palette::selected(cx)))
                .child(face)
                .on_hover(move |on: &bool, _, cx| {
                    if *on {
                        hovered.update(cx, |h, cx| {
                            *h = Some(shown.clone());
                            cx.notify();
                        });
                    }
                })
                .on_click(cx.listener(move |this, _, window, cx| this.pick_emoji(picked.clone(), window, cx)))
        };
        let grid = div()
            .id("emoji-scroll")
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_y_scroll()
            .track_scroll(&scroll)
            .px(px(8.))
            .pb(px(8.))
            .children(sections.into_iter().map(|section| {
                // GAP: the section label sticks to the top of the grid in the design; gpui has no sticky.
                div()
                    .flex()
                    .flex_col()
                    .mb(px(4.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .py(px(6.))
                            .px(px(4.))
                            .text_size(px(12.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(palette::muted(cx))
                            .child(section.lead.small(cx))
                            .child(section.name.clone()),
                    )
                    .child(div().flex().flex_wrap().children(section.items.into_iter().enumerate().map(|(i, e)| cell(e, section.source.clone(), format!("emoji-{}-{i}", section.name)))))
            }));
        let foot = div()
            .min_h(px(52.))
            .flex_none()
            .py(px(6.))
            .px(px(12.))
            .flex()
            .items_center()
            .gap(px(10.))
            .border_t_1()
            .border_color(palette::line(cx))
            .children(preview.map(|(emoji, source)| {
                let face = match emoji.url() {
                    Some(url) => img(url).size(px(28.)).into_any_element(),
                    None => div().text_size(px(28.)).line_height(px(28.)).child(emoji.name.clone()).into_any_element(),
                };
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .min_w_0()
                    .child(div().w(px(32.)).flex_none().flex().justify_center().child(face))
                    .child(
                        div()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(div().truncate().font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(crate::message_menu::shortcode(&emoji)))
                            .child(div().text_size(px(12.)).text_color(palette::muted(cx)).child(source)),
                    )
            }));
        // GAP: the design's skin-tone button sits beside search; Wave has no skin tones yet.
        let head = div().p(px(10.)).child(
            div()
                .h(px(36.))
                .px(px(12.))
                .rounded(px(crate::ui::RADIUS_MD))
                .bg(palette::surface(palette::Level::Page, cx))
                .flex()
                .items_center()
                .gap(px(8.))
                .text_size(px(14.))
                .text_color(palette::faint(cx))
                .child(Icon::new(Glyph::Search).size(px(16.)))
                .child(div().flex_1().min_w_0().child(Input::new(&self.emoji_search).appearance(false))),
        );
        let picker = div()
            .id("emoji-picker")
            .absolute()
            .right(px(32.))
            .top(px((f32::from(viewport.height) - PICKER_SIZE.1 - 90.).max(TITLE_HEIGHT + 8.)))
            .w(px(PICKER_SIZE.0))
            .h(px(PICKER_SIZE.1))
            .rounded(px(crate::ui::RADIUS_LG))
            .bg(palette::surface(palette::Level::Overlay, cx))
            .shadow(menu::pop_shadow(cx))
            .flex()
            .flex_col()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(head)
            .child(div().flex_1().min_h_0().flex().child(rail).child(grid))
            .child(foot);
        Some(backdrop("emoji-backdrop", cx).child(menu::rise(picker, "emoji-picker".into(), Duration::ZERO)).into_any_element())
    }

    // What the picker lists for the current search: recent (only before you search), the server's own, then Unicode by group.
    fn emoji_sections(&self, cx: &App) -> Vec<EmojiSection> {
        let query = self.emoji_search.read(cx).value().to_lowercase();
        let unicode: SharedString = "Unicode emoji".into();
        let mut sections = Vec::new();
        if query.is_empty() {
            let recent = self.prefs.frequent_emojis(18, &crate::message_menu::QUICK_PAD);
            sections.push(EmojiSection { lead: SectionLead::Emoji("🕘"), name: "Recently used".into(), source: unicode.clone(), items: recent });
        }
        if let Some(guild) = self.guild() {
            let custom: Vec<Emoji> = guild.emojis.iter().filter(|e| query.is_empty() || e.name.to_lowercase().contains(&query)).cloned().collect();
            if !custom.is_empty() {
                let name: SharedString = guild.name.clone().into();
                sections.push(EmojiSection { lead: SectionLead::Server { icon: guild.icon.clone(), name: guild.name.clone() }, name: name.clone(), source: name, items: custom });
            }
        }
        for group in emojis::Group::iter() {
            let items: Vec<Emoji> = group
                .emojis()
                .filter(|e| query.is_empty() || e.name().contains(&query) || e.shortcodes().any(|s| s.contains(&query)))
                .map(|e| Emoji::unicode(e.as_str()))
                .collect();
            if !items.is_empty() {
                let (name, face) = group_name(group);
                sections.push(EmojiSection { lead: SectionLead::Emoji(face), name: name.into(), source: unicode.clone(), items });
            }
        }
        sections
    }

    // The quick switcher (navigation.css `.switcher`): a focused field, results grouped by kind with
    // the match in bold, arrows to pick, Enter to go.
    fn switcher_overlay(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let selected = self.switcher?;
        let query = self.switcher_input.read(cx).value().to_string();
        let results = self.switcher_results(cx);
        let mut list: Vec<AnyElement> = Vec::new();
        let mut last_group = None;
        for (i, (guild_id, channel, place)) in results.into_iter().enumerate() {
            let group = if channel.recipient.is_some() || channel.kind == ChannelKind::Dm { "People" } else { "Channels" };
            if last_group != Some(group) {
                list.push(div().pt(px(10.)).px(px(10.)).pb(px(4.)).text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(palette::faint(cx)).child(group).into_any_element());
                last_group = Some(group);
            }
            list.push(self.switcher_row(i == selected, guild_id, channel, place, &query, cx).into_any_element());
        }
        let is_empty = list.is_empty();
        let field = div()
            .h(px(52.))
            .px(px(14.))
            .flex_none()
            .rounded(px(crate::ui::RADIUS_MD))
            .bg(palette::surface(palette::Level::Page, cx))
            .border_1()
            .border_color(palette::accent())
            .shadow(vec![gpui_kit::BoxShadow { color: palette::accent_soft(cx), offset: gpui_kit::point(px(0.), px(0.)), blur_radius: px(0.), spread_radius: px(3.), inset: false }])
            .flex()
            .items_center()
            .gap(px(10.))
            .text_color(palette::faint(cx))
            .text_size(px(20.))
            .child(Icon::new(Glyph::Search).size(px(20.)))
            .child(div().flex_1().min_w_0().text_color(palette::strong(cx)).child(Input::new(&self.switcher_input).appearance(false)));
        let hint = |keys: Vec<AnyElement>, label: &'static str| div().flex().items_center().gap_1().children(keys).child(label);
        let key = |glyph: Glyph| crate::ui::kbd("", cx).child(Icon::new(glyph).size(px(12.))).into_any_element();
        let foot = div()
            .pt(px(10.))
            .px(px(10.))
            .pb(px(4.))
            .flex()
            .flex_wrap()
            .items_center()
            .gap_x_4()
            .gap_y_2()
            .border_t_1()
            .border_color(palette::line(cx))
            .text_size(px(12.))
            .text_color(palette::muted(cx))
            .when(!is_empty, |f| f.child(hint(vec![key(Glyph::ChevronUp), key(Glyph::ChevronDown)], "Move")).child(hint(vec![crate::ui::kbd("Enter", cx).into_any_element()], "Open")))
            .child(hint(vec![crate::ui::kbd("Esc", cx).into_any_element()], "Close"));
        let body: AnyElement = if is_empty {
            crate::ui::empty_state("switcher-empty", crate::mascot::Mood::Waiting, format!("Nothing called “{query}”"), "Check the spelling, or search messages in a channel instead.", cx).into_any_element()
        } else {
            div().id("switcher-list").flex().flex_col().gap(px(2.)).max_h(px(380.)).overflow_y_scroll().children(list).into_any_element()
        };
        let card = div()
            .id("switcher")
            .w(px(SWITCHER_WIDTH))
            .p(px(10.))
            .rounded(px(crate::ui::RADIUS_XL))
            .bg(palette::surface(palette::Level::Overlay, cx))
            .shadow(menu::pop_shadow(cx))
            .flex()
            .flex_col()
            .gap_2()
            .capture_key_down(cx.listener(Self::on_switcher_key))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(field)
            .child(body)
            .child(foot);
        Some(
            backdrop("switcher-backdrop", cx)
                .bg(menu::scrim(cx))
                .child(div().absolute().top(px(120.)).left_0().right_0().flex().justify_center().child(menu::rise(card, "switcher".into(), Duration::ZERO)))
                .into_any_element(),
        )
    }

    fn switcher_row(&self, active: bool, guild_id: Option<String>, channel: Channel, place: SharedString, query: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let lead = match (&channel.recipient, channel.kind) {
            (Some(user), _) => {
                let status = self.presences.get(&user.id).map(|p| p.status);
                avatar_with_status(&user.avatar, 20., status, palette::surface(palette::Level::Overlay, cx))
            }
            (None, kind) => {
                let glyph = match kind {
                    ChannelKind::Dm => Glyph::User,
                    ChannelKind::Voice => Glyph::VolumeHigh,
                    _ if channel.private => Glyph::HashtagLock,
                    _ => Glyph::Hashtag,
                };
                div().text_color(palette::faint(cx)).child(Icon::new(glyph).size(px(18.))).into_any_element()
            }
        };
        let name: SharedString = channel.name.clone().into();
        let strong = palette::strong(cx);
        let mark = name.to_lowercase().find(&query.to_lowercase()).filter(|_| !query.is_empty()).map(|at| at..at + query.len()).filter(|r| name.is_char_boundary(r.start) && name.is_char_boundary(r.end));
        let label = StyledText::new(name.clone()).with_highlights(mark.map(|r| (r, HighlightStyle { color: Some(strong), font_weight: Some(FontWeight::BOLD), ..Default::default() })));
        let hover = palette::hover(cx);
        div()
            .id(SharedString::from(format!("switch-{}", channel.id)))
            .h(px(36.))
            .px(px(10.))
            .flex_none()
            .rounded(px(crate::ui::RADIUS_SM))
            .flex()
            .items_center()
            .gap(px(10.))
            .cursor_pointer()
            .text_color(if active { strong } else { palette::text(cx) })
            .when(active, |r| r.bg(palette::selected(cx)))
            .when(!active, |r| r.hover(move |s| s.bg(hover)))
            .child(lead)
            .child(div().min_w_0().truncate().child(label))
            .child(div().ml_auto().pl_3().flex_none().text_size(px(12.)).text_color(palette::faint(cx)).child(place))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.switcher = None;
                this.guild_id = guild_id.clone();
                this.open_channel(channel.clone(), window, cx);
            }))
    }
}

struct EmojiSection {
    lead: SectionLead,
    name: SharedString,
    // What the foot says an emoji from here is.
    source: SharedString,
    items: Vec<Emoji>,
}

// GAP: the design marks Unicode groups with Phosphor icons (clock, leaf, hamburger…) that aren't in
// `icons.rs` yet; each group shows its own first emoji instead.
enum SectionLead {
    Emoji(&'static str),
    Server { icon: Option<String>, name: String },
}

impl SectionLead {
    fn render(&self, cx: &App) -> AnyElement {
        match self {
            SectionLead::Emoji(face) => div().text_size(px(20.)).line_height(px(20.)).child(*face).into_any_element(),
            SectionLead::Server { icon, name } => menu::server_icon(icon.as_deref(), name, 24., crate::ui::RADIUS_XS, cx),
        }
    }

    fn small(&self, cx: &App) -> AnyElement {
        match self {
            SectionLead::Emoji(face) => div().text_size(px(14.)).line_height(px(14.)).child(*face).into_any_element(),
            SectionLead::Server { icon, name } => menu::server_icon(icon.as_deref(), name, 16., 5., cx),
        }
    }
}

// What someone's up to, as the popout's status line says it.
fn activity_line(activity: &Activity) -> String {
    let text = &activity.text;
    match activity.kind {
        ActivityKind::Custom => text.clone(),
        ActivityKind::Playing => format!("Playing {text}"),
        ActivityKind::Streaming => format!("Streaming {text}"),
        ActivityKind::Listening => format!("Listening to {text}"),
        ActivityKind::Watching => format!("Watching {text}"),
        ActivityKind::Competing => format!("Competing in {text}"),
    }
}

// A role as a chip with its colour dot (primitives.css `.role`).
// MOVE(ui): role_chip
fn role_chip(name: String, color: Hsla, cx: &App) -> gpui_kit::Div {
    div()
        .h(px(24.))
        .px_2()
        .flex()
        .items_center()
        .gap(px(6.))
        .rounded(px(crate::ui::RADIUS_XS))
        .bg(palette::selected(cx))
        .text_size(px(12.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(palette::text(cx))
        .child(div().size(px(10.)).rounded_full().bg(color))
        .child(name)
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

// A Unicode group's name, and the emoji that stands for it in the picker's rail.
fn group_name(group: emojis::Group) -> (&'static str, &'static str) {
    use emojis::Group::*;
    match group {
        SmileysAndEmotion => ("Smileys & Emotion", "😀"),
        PeopleAndBody => ("People", "👋"),
        AnimalsAndNature => ("Nature", "🌿"),
        FoodAndDrink => ("Food & Drink", "🍔"),
        TravelAndPlaces => ("Travel & Places", "✈️"),
        Activities => ("Activities", "⚽"),
        Objects => ("Objects", "💡"),
        Symbols => ("Symbols", "❤️"),
        Flags => ("Flags", "🏁"),
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

// The selected tile's accent outline: springs out from inside the thumbnail's edge to 3px past it,
// overshooting a touch. Still when the selection wasn't the user's doing (the picker's preselection).
fn selection_ring(key: &str, animate: bool) -> AnyElement {
    let ring = div().absolute().border_2().border_color(palette::accent());
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
        .child(crate::ui::segmented(cx).children(segments))
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
