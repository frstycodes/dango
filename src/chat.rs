// The middle column: channel header, transcript and composer.
use std::{collections::HashMap, f32::consts::TAU, time::Duration};

use chrono::{DateTime, Local, NaiveDate};
use gpui_kit::{
    Animation, AnimationExt, AnyElement, App, Context, FontWeight, Hsla, KeyDownEvent, SharedString, WeakEntity, Window,
    WindowControlArea,
    component::{
        ActiveTheme, Icon,
        input::{Input, Textarea},
        text::TextView,
        tooltip::Tooltip,
    },
    div, img, list, prelude::*, px,
};
use gpui_motion_kit::core::{Motion, Target};

use std::rc::Rc;

use gpui_kit::component::menu::{ContextMenuExt, PopupMenuItem};

use crate::{
    Rustcord, EmojiTarget, Feed, RightPanel,
    discord::{ChannelKind, Embed, Emoji, MediaKind, Message, SendState, User},
    icons::Glyph,
    mascot::{Mood, clyde},
    motion,
};

// Messages from one author closer together than this share a header, like Discord.
const GROUP_GAP_SECS: i64 = 7 * 60;
const QUICK_REACTIONS: [&str; 3] = ["👍", "😂", "❤️"];
const MAX_IMAGE: (f32, f32) = (400., 320.);

impl Rustcord {
    pub fn render_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        // Discord's middle area: the channel header spans the chat and the member list, which sit side by side under it.
        let main = div().relative().flex_1().min_w_0().h_full().flex().flex_col().bg(theme.background).border_l_1().border_color(theme.sidebar_border);
        let Some(channel) = self.channel.clone() else {
            return main.child(empty_state("idle", Mood::Idle, "No channel open", "Pick a server on the left, then a channel to start reading.", cx));
        };
        let members_on = matches!(self.right_panel, RightPanel::Members);
        let header = div()
            .h(px(48.))
            .flex_none()
            .px_4()
            .flex()
            .items_center()
            .gap_2()
            .border_b_1()
            .border_color(theme.sidebar_border)
            .window_control_area(WindowControlArea::Drag)
            .child(Icon::new(if channel.kind == ChannelKind::Dm { Glyph::User } else { Glyph::Hashtag }).text_color(theme.muted_foreground))
            .child(div().flex_none().font_weight(FontWeight::MEDIUM).text_color(crate::palette::strong(cx)).child(channel.name.clone()))
            .children(channel.topic.clone().map(|topic| {
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().size(px(4.)).flex_none().rounded_full().bg(theme.muted_foreground.opacity(0.5)))
                    .child(div().truncate().text_size(px(13.)).text_color(theme.muted_foreground).child(topic))
            }))
            .when(channel.topic.is_none(), |h| h.child(div().flex_1()))
            .child(
                div()
                    .id("pins-button")
                    .size(px(32.))
                    .flex_none()
                    .rounded(px(8.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .text_color(if self.pins.is_some() { theme.foreground } else { theme.muted_foreground })
                    .hover(|s| s.text_color(theme.foreground))
                    .child(Icon::new(Glyph::Pin).size(px(20.)))
                    .tooltip(|window, cx| Tooltip::new("Pinned Messages").build(window, cx))
                    .on_click(cx.listener(|this, _, _, cx| this.open_pins(cx))),
            )
            .when(self.guild_id.is_some(), |h| {
                h.child(
                    div()
                        .id("members-toggle")
                        .size(px(32.))
                        .flex_none()
                        .rounded(px(8.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .text_color(if members_on { theme.foreground } else { theme.muted_foreground })
                        .hover(|s| s.text_color(theme.foreground))
                        .child(Icon::new(Glyph::Users).size(px(20.)))
                        .tooltip(move |window, cx| Tooltip::new(if members_on { "Hide Member List" } else { "Show Member List" }).build(window, cx))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.right_panel = if matches!(this.right_panel, RightPanel::Members) { RightPanel::Hidden } else { RightPanel::Members };
                            cx.notify();
                        })),
                )
                .child(
                    div()
                        .w(px(240.))
                        .flex_none()
                        .h(px(32.))
                        .pl_1()
                        .pr_2()
                        .flex()
                        .items_center()
                        .rounded(px(8.))
                        .bg(theme.muted)
                        .border_1()
                        .border_color(theme.input)
                        .text_size(px(14.))
                        .child(div().flex_1().min_w_0().child(Input::new(&self.search_input).appearance(false)))
                        .child(Icon::new(Glyph::Search).size(px(16.)).text_color(theme.muted_foreground)),
                )
            });
        let feed = self.feeds.get(&channel.id);
        let body = match feed {
            Some(Feed { loaded: true, messages, .. }) if messages.is_empty() => empty_state("empty", Mood::Happy, "No messages yet", "Be the first to say something.", cx),
            Some(Feed { loaded: true, .. }) => self.render_messages(cx).into_any_element(),
            Some(Feed { error: Some(error), .. }) => empty_state("failed", Mood::Idle, "Couldn't load messages", error.clone(), cx),
            _ => skeleton(cx),
        };
        let body = match self.stream_view(window, cx) {
            Some(stream) => stream,
            None => body,
        };
        let chat = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .on_drop(cx.listener(|this, paths: &gpui_kit::ExternalPaths, _, cx| this.add_files(paths.paths().to_vec(), cx))).child(div().relative().flex_1().min_h_0().child(body)).child(self.render_composer(window, cx));
        main.child(header).child(div().flex_1().min_h_0().flex().child(chat).children(self.render_panel(window, cx)))
    }

    // A Go Live we're watching takes the transcript's place, fitted to the column, like Discord's stream view.
    fn stream_view(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let watching = self.call.as_ref()?.watching.as_ref()?;
        let name = self.user_name(&watching.streamer_id);
        let stage: AnyElement = match &watching.frame {
            Some(frame) => {
                let viewport = window.viewport_size();
                let (max_w, max_h) = (f32::from(viewport.width) - self.channels_width - 400., f32::from(viewport.height) - 260.);
                let (w, h) = (frame.size(0).width.0 as f32, frame.size(0).height.0 as f32);
                let scale = (max_w / w).min(max_h / h).clamp(0.1, 2.);
                img(gpui_kit::ImageSource::Render(frame.clone())).w(px(w * scale)).h(px(h * scale)).rounded(px(8.)).into_any_element()
            }
            None => div().text_color(gpui_kit::white().opacity(0.7)).child(format!("Joining {name}'s stream…")).into_any_element(),
        };
        let theme = cx.theme().clone();
        Some(
            div()
                .size_full()
                .bg(gpui_kit::black())
                .flex()
                .flex_col()
                .child(
                    div()
                        .h(px(44.))
                        .px_4()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_color(gpui_kit::white())
                        .child(div().h(px(18.)).px_1().rounded(px(4.)).bg(theme.danger).text_size(px(11.)).font_weight(FontWeight::BOLD).flex().items_center().child("LIVE"))
                        .child(div().flex_1().truncate().font_weight(FontWeight::SEMIBOLD).child(format!("{name}'s screen")))
                        .child(
                            div()
                                .id("stop-watching")
                                .h(px(30.))
                                .px_3()
                                .rounded(px(8.))
                                .flex()
                                .items_center()
                                .cursor_pointer()
                                .bg(gpui_kit::white().opacity(0.12))
                                .hover(|s| s.bg(gpui_kit::white().opacity(0.2)))
                                .child("Stop Watching")
                                .on_click(cx.listener(|this, _, _, cx| this.stop_watching(cx))),
                        ),
                )
                .child(div().flex_1().min_h_0().flex().items_center().justify_center().child(stage))
                .into_any_element(),
        )
    }

    fn render_messages(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(channel) = &self.channel else { return div().into_any_element() };
        let Some(feed) = self.feeds.get(&channel.id) else { return div().into_any_element() };
        let messages: std::rc::Rc<Vec<Message>> = std::rc::Rc::new(feed.messages.clone());
        let ctx = std::rc::Rc::new(RowContext {
            me: self.me.clone(),
            names: messages.iter().flat_map(|m| std::iter::once(&m.author).chain(&m.mentions)).chain(self.me.iter()).map(|u| (u.id.clone(), u.name.clone())).collect(),
            colors: messages.iter().filter_map(|m| Some((m.author.id.clone(), self.role_color(m.guild_id.as_ref().or(self.guild_id.as_ref()), &m.author.id, m.roles.as_ref())?))).collect(),
            mentions_me: messages.iter().filter(|m| self.mentions_me(m)).map(|m| m.id.clone()).collect(),
            channel_names: self.guild().map(|g| g.channels.iter().map(|c| (c.id.clone(), c.name.clone())).collect()).unwrap_or_default(),
            role_names: self.guild().map(|g| g.roles.iter().map(|r| (r.id.clone(), r.name.clone())).collect()).unwrap_or_default(),
            flash: self.flash.clone(),
            copied: self.copied.clone(),
            revealed: self.revealed.clone(),
            unread_marker: self.unread_marker,
            this: cx.weak_entity(),
            has_more: feed.has_more,
        });
        let arrivals = self.arrivals.clone();
        list(self.list.clone(), move |ix, window, cx| {
            if ix == 0 && ctx.has_more {
                let this = ctx.this.clone();
                cx.defer(move |cx| {
                    this.update(cx, |this, cx| this.load_older(cx)).ok();
                });
            }
            let message = &messages[ix];
            let previous = ix.checked_sub(1).map(|i| &messages[i]);
            let new_day = day(&message.timestamp) != previous.and_then(|p| day(&p.timestamp));
            let starts_group = new_day || message.reply.is_some() || previous.is_none_or(|p| !continues(p, message));
            let row = message_row(message, starts_group, &ctx, cx);
            // Discord's red NEW line sits above the first message after your read position.
            let first_unread = ctx.unread_marker.is_some_and(|read| crate::order(&message.id) > read && previous.is_none_or(|p| crate::order(&p.id) <= read));
            let row = match (new_day, first_unread) {
                (false, false) => row,
                _ => div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .when(new_day && !first_unread, |d| d.child(day_divider(&message.timestamp, cx)))
                    .when(first_unread, |d| d.child(new_divider(new_day.then(|| message.timestamp.clone()), cx)))
                    .child(row)
                    .into_any_element(),
            };
            let mut arrivals = arrivals.borrow_mut();
            if !arrivals.fresh.contains(&message.id) {
                return row;
            }
            // A new message pops up from the composer with a little bounce.
            let frame = arrivals.motions.frame_one(
                message.id.clone(),
                Motion::new(Target::default()).initial(Target::default().y(8.).opacity(0.)).transition(motion::spring()),
                window,
                cx,
            );
            if !frame.is_animating() {
                arrivals.fresh.remove(&message.id);
                arrivals.motions.remove(&message.id);
            }
            frame.apply(div().w_full().child(row)).into_any_element()
        })
        .size_full()
        .pb_2()
        .into_any_element()
    }

    fn render_composer(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let attach = div()
            .id("attach")
            .size(px(36.))
            .flex_none()
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .text_color(theme.muted_foreground)
            .hover(|s| s.text_color(theme.foreground))
            .child(Icon::new(Glyph::Plus).size(px(22.)))
            .tooltip(|window, cx| Tooltip::new("Upload a File").build(window, cx))
            .on_click(cx.listener(|this, _, window, cx| this.upload(window, cx)));
        let ping = self.reply_ping;
        let is_reply = self.replying.is_some();
        let context_bar = match (&self.replying, &self.editing) {
            (Some(reply), _) => Some(format!("Replying to {}", reply.author.name)),
            (_, Some(_)) => Some("Editing message · Esc to cancel".into()),
            _ => None,
        }
        .map(|label| {
            div()
                .h(px(32.))
                .px_4()
                .flex()
                .items_center()
                .gap_2()
                .rounded_t(px(8.))
                .bg(theme.muted)
                .border_1()
                .border_b_0()
                .border_color(theme.input)
                .text_size(px(13.))
                .text_color(theme.muted_foreground)
                .child(div().flex_1().truncate().child(label))
                .when(is_reply, |bar| {
                    bar.child(
                        div()
                            .id("reply-ping")
                            .px_1()
                            .cursor_pointer()
                            .font_weight(FontWeight::BOLD)
                            .text_color(if ping { crate::palette::blurple() } else { theme.muted_foreground })
                            .child(if ping { "@ ON" } else { "@ OFF" })
                            .tooltip(|window, cx| Tooltip::new("Mention the author of this message").build(window, cx))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.reply_ping = !this.reply_ping;
                                cx.notify();
                            })),
                    )
                })
                .child(
                    div()
                        .id("cancel-compose")
                        .size(px(20.))
                        .rounded_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .hover(|s| s.text_color(theme.foreground))
                        .child(Icon::new(Glyph::X).size(px(14.)))
                        .on_click(cx.listener(|this, _, window, cx| this.cancel_compose(window, cx))),
                )
        });
        let has_bar = context_bar.is_some();
        let attachments = (!self.pending_files.is_empty()).then(|| {
            div().flex().gap_2().px_2().pt_2().pb_1().children(self.pending_files.iter().enumerate().map(|(i, path)| {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let is_image = matches!(path.extension().and_then(|e| e.to_str()).map(str::to_lowercase).as_deref(), Some("png" | "jpg" | "jpeg" | "gif" | "webp"));
                div()
                    .relative()
                    .w(px(176.))
                    .h(px(176.))
                    .p_2()
                    .rounded(px(8.))
                    .bg(theme.background)
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div().flex_1().min_h_0().rounded(px(4.)).overflow_hidden().flex().items_center().justify_center().child(if is_image {
                            img(path.clone()).size_full().object_fit(gpui_kit::ObjectFit::Contain).into_any_element()
                        } else {
                            Icon::new(Glyph::Copy).size(px(40.)).text_color(theme.muted_foreground).into_any_element()
                        }),
                    )
                    .child(div().truncate().text_size(px(12.)).child(name))
                    .child(
                        div()
                            .id(SharedString::from(format!("remove-file-{i}")))
                            .absolute()
                            .top(px(-6.))
                            .right(px(-6.))
                            .size(px(24.))
                            .rounded(px(6.))
                            .bg(theme.popover)
                            .border_1()
                            .border_color(theme.input)
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .text_color(theme.danger)
                            .child(Icon::new(Glyph::Trash).size(px(16.)))
                            .tooltip(|window, cx| Tooltip::new("Remove Attachment").build(window, cx))
                            .on_click(cx.listener(move |this, _, _, cx| this.remove_file(i, cx))),
                    )
            }))
        });
        let row = div()
            .min_h(px(56.))
            .pl_2()
            .pr_3()
            .py(px(8.))
            .flex()
            .items_center()
            .gap_2()
            .child(attach)
            .child(div().flex_1().min_w_0().child(Textarea::new(&self.composer).appearance(false)))
            .child(
                div()
                    .id("composer-gif")
                    .size(px(32.))
                    .flex_none()
                    .rounded(px(6.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .text_color(theme.muted_foreground)
                    .hover(|s| s.text_color(theme.foreground))
                    .child(Icon::new(Glyph::Gif).size(px(22.)))
                    .tooltip(|window, cx| Tooltip::new("Open GIF picker").build(window, cx))
                    .on_click(cx.listener(|this, _, window, cx| this.open_gifs(window, cx))),
            )
            .child(
                div()
                    .id("composer-emoji")
                    .size(px(32.))
                    .flex_none()
                    .rounded(px(6.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .text_color(theme.muted_foreground)
                    .hover(|s| s.text_color(theme.foreground))
                    .child(Icon::new(Glyph::Emoji).size(px(22.)))
                    .tooltip(|window, cx| Tooltip::new("Select Emoji").build(window, cx))
                    .on_click(cx.listener(|this, _, window, cx| this.open_emoji_picker(EmojiTarget::Composer, window, cx))),
            );
        let card = div()
            .flex()
            .flex_col()
            .rounded(px(8.))
            .when(has_bar, |c| c.rounded_t(px(0.)))
            .bg(theme.muted)
            .border_1()
            .border_color(theme.input)
            .children(attachments)
            .child(row);
        let typers = self.typers();
        // Discord's wording: names in bold, the rest muted.
        let typing_line = (!typers.is_empty()).then(|| {
            let bold = |name: &str| div().font_weight(FontWeight::BOLD).text_color(theme.foreground).child(name.to_owned());
            let names: Vec<&str> = typers.iter().map(|t| t.name.as_str()).collect();
            let parts: Vec<AnyElement> = match names.as_slice() {
                [one] => vec![bold(one).into_any_element(), div().child("is typing…").into_any_element()],
                [a, b] => vec![bold(a).into_any_element(), div().child("and").into_any_element(), bold(b).into_any_element(), div().child("are typing…").into_any_element()],
                [a, b, c] => vec![bold(&format!("{a}, {b}")).into_any_element(), div().child("and").into_any_element(), bold(c).into_any_element(), div().child("are typing…").into_any_element()],
                _ => vec![div().child("Several people are typing…").into_any_element()],
            };
            div().flex().items_center().gap_1().child(div().mr_1().child(typing_dots(cx))).children(parts)
        });
        // A reserved slot above the composer, so the box never jumps when someone starts typing.
        let status_row = div()
            .h(px(24.))
            .px_1()
            .flex()
            .items_center()
            .text_size(px(13.))
            .text_color(theme.muted_foreground)
            .children(match &self.send_error {
                Some(error) => Some(div().text_color(theme.danger).child(error.clone()).into_any_element()),
                None => typing_line.map(IntoElement::into_any_element),
            });
        div()
            .relative()
            .flex_none()
            .px_2()
            .flex()
            .flex_col()
            .capture_key_down(cx.listener(Self::on_composer_key))
            .children(self.autocomplete_popup(cx))
            .children(self.scrolled_up.then(|| {
                div()
                    .id("jump-to-present")
                    .h(px(32.))
                    .mb_1()
                    .px_3()
                    .rounded(px(8.))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .bg(theme.muted)
                    .border_1()
                    .border_color(theme.input)
                    .text_size(px(13.))
                    .child(div().flex_1().text_color(theme.muted_foreground).child("You're viewing older messages"))
                    .child(div().font_weight(FontWeight::SEMIBOLD).text_color(theme.foreground).child("Jump To Present"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.list.scroll_to_end();
                        this.scrolled_up = false;
                        cx.notify();
                    }))
            }))
            .child(status_row)
            .children(context_bar)
            .child(card)
            .child(div().h(px(8.)))
    }

    // Arrows and Tab drive the suggestion list; Esc cancels a reply or edit; Up in an empty box edits your last message.
    fn on_composer_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        let modifiers = event.keystroke.modifiers;
        if key == "v" && (modifiers.platform || modifiers.control) && self.paste_image(cx) {
            return cx.stop_propagation();
        }
        if self.autocomplete.is_some() {
            match key {
                "up" => self.move_suggestion(-1, cx),
                "down" => self.move_suggestion(1, cx),
                "tab" => self.accept_suggestion(window, cx),
                "escape" => self.autocomplete = None,
                _ => return,
            }
            return cx.stop_propagation();
        }
        match key {
            "escape" if self.replying.is_some() || self.editing.is_some() => self.cancel_compose(window, cx),
            "up" if !self.has_draft && self.edit_last(window, cx) => {}
            _ => return,
        }
        cx.stop_propagation();
    }

    fn autocomplete_popup(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let auto = self.autocomplete.as_ref()?;
        let theme = cx.theme().clone();
        let heading = if auto.token.starts_with('#') { "Text Channels" } else { "Members" };
        let rows = auto.items.iter().enumerate().map(|(i, item)| {
            let selected = i == auto.selected;
            div()
                .id(SharedString::from(format!("suggest-{i}")))
                .h(px(36.))
                .px_2()
                .flex()
                .items_center()
                .gap_2()
                .rounded(px(6.))
                .cursor_pointer()
                .when(selected, |r| r.bg(theme.foreground.opacity(0.08)))
                .hover(|r| r.bg(theme.foreground.opacity(0.06)))
                .child(match &item.avatar {
                    Some(url) => img(url.clone()).size(px(24.)).rounded_full().into_any_element(),
                    None => Icon::new(Glyph::Hashtag).text_color(theme.muted_foreground).into_any_element(),
                })
                .child(div().truncate().child(item.label.clone()))
                .on_click(cx.listener(move |this, _, window, cx| {
                    if let Some(auto) = &mut this.autocomplete {
                        auto.selected = i;
                    }
                    this.accept_suggestion(window, cx);
                }))
        });
        Some(
            div()
                .absolute()
                .left(px(16.))
                .right(px(16.))
                .bottom(px(70.))
                .p_2()
                .rounded(px(12.))
                .bg(theme.popover)
                .border_1()
                .border_color(theme.border)
                .shadow_lg()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(div().px_2().pb_1().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child(heading))
                .children(rows)
                .with_animation("autocomplete", Animation::new(Duration::from_millis(180)).with_easing(motion::back_out), |el, t| {
                    el.mb(px(6. * (1. - t))).opacity(t.clamp(0., 1.))
                }),
        )
    }
}

struct RowContext {
    me: Option<User>,
    names: HashMap<String, String>,
    colors: HashMap<String, Hsla>,
    mentions_me: std::collections::HashSet<String>,
    channel_names: HashMap<String, String>,
    role_names: HashMap<String, String>,
    flash: Option<String>,
    copied: Option<String>,
    revealed: Rc<std::cell::RefCell<std::collections::HashSet<String>>>,
    unread_marker: Option<u64>,
    this: WeakEntity<Rustcord>,
    has_more: bool,
}

fn message_row(message: &Message, starts_group: bool, ctx: &std::rc::Rc<RowContext>, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let group = SharedString::from(format!("msg-{}", message.id));
    let mine = ctx.me.as_ref().is_some_and(|me| me.id == message.author.id);
    let highlighted = ctx.mentions_me.contains(&message.id);
    let flashing = ctx.flash.as_ref() == Some(&message.id);
    let name_color = ctx.colors.get(&message.author.id).copied().unwrap_or(crate::palette::strong(cx));
    let body = div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(2.))
        .when(message.state != SendState::Sent, |b| b.opacity(0.55))
        .children(content(message, ctx, cx))
        .children(media_grid(message, ctx, cx))
        .children(message.embeds.iter().map(|e| embed_card(e, cx)))
        .children(files(message, cx))
        .children(reactions(message, ctx, cx))
        .when(message.state == SendState::Failed, |b| {
            let (this, retry) = (ctx.this.clone(), message.clone());
            b.child(
                div()
                    .id(SharedString::from(format!("retry-{}", message.id)))
                    .text_size(px(12.))
                    .text_color(theme.danger)
                    .cursor_pointer()
                    .child("Message failed to send. Click to retry.")
                    .on_click(move |_, _, cx| {
                        this.update(cx, |this, cx| this.retry(retry.clone(), cx)).ok();
                    }),
            )
        });
    let row = div()
        .id(group.clone())
        .group(group.clone())
        .relative()
        .w_full()
        .px_4()
        .flex()
        .flex_col()
        .when(highlighted, |r| r.bg(theme.warning.opacity(0.08)))
        .when(flashing, |r| r.bg(crate::palette::blurple().opacity(0.14)))
        .hover(|s| s.bg(theme.foreground.opacity(0.03)))
        .child(actions(message, mine, ctx, &group, cx));
    if !starts_group {
        let time = div()
            .w(px(40.))
            .flex_none()
            .pt(px(3.))
            .text_size(px(10.))
            .text_color(theme.muted_foreground)
            .invisible()
            .group_hover(group, |s| s.visible())
            .child(format_clock(&message.timestamp));
        return with_message_menu(row.py(px(1.)).child(div().flex().gap_4().child(time).child(body)), message, mine, ctx);
    }
    let header = div()
        .flex()
        .items_center()
        .gap_1()
        .child(profile_trigger(format!("name-{}", message.id), &message.author, ctx).font_weight(FontWeight::MEDIUM).text_color(name_color).hover(|s| s.underline()).child(message.author.name.clone()))
        .children(message.author.tag.clone().map(|tag| {
            div()
                .h(px(18.))
                .px_1()
                .flex()
                .items_center()
                .gap_0p5()
                .rounded(px(4.))
                .bg(theme.foreground.opacity(0.08))
                .text_size(px(11.))
                .font_weight(FontWeight::BOLD)
                .children(tag.badge.map(|b| img(b).size(px(12.))))
                .child(tag.text)
        }))
        .when(message.author.bot, |h| h.child(chip("APP", cx)))
        .child(div().ml_1().text_size(px(12.)).font_weight(FontWeight::MEDIUM).text_color(gpui_kit::rgb(0x81828A)).child(format_stamp(&message.timestamp)));
    let row = row
        .mt(px(17.))
        .pt(px(2.))
        .pb(px(2.))
        .children(message.reply.as_ref().map(|reply| reply_preview(reply, message.reply_to.clone(), ctx, cx)))
        .child(
            div()
                .flex()
                .gap_4()
                .child(profile_trigger(format!("avatar-{}", message.id), &message.author, ctx).flex_none().child(crate::layout::decorated_avatar(&message.author.avatar, message.author.decoration.as_deref(), 40., None, cx.theme().background)))
                .child(div().flex_1().min_w_0().flex().flex_col().gap(px(2.)).child(header).child(body)),
        );
    with_message_menu(row, message, mine, ctx)
}

// Clicking an avatar or name opens their profile popout at the pointer, as in Discord.
fn profile_trigger(id: String, user: &User, ctx: &std::rc::Rc<RowContext>) -> gpui_kit::Stateful<gpui_kit::Div> {
    let (this, user) = (ctx.this.clone(), user.clone());
    div().id(SharedString::from(id)).cursor_pointer().on_click(move |event, _, cx| {
        let at = event.position();
        this.update(cx, |this, cx| this.open_profile(user.clone(), at, cx)).ok();
    })
}

type MenuAction = Rc<dyn Fn(&mut Rustcord, &mut Window, &mut Context<Rustcord>)>;

// Discord's message menu, trimmed to what Rustcord can do.
fn with_message_menu(row: gpui_kit::Stateful<gpui_kit::Div>, message: &Message, mine: bool, ctx: &std::rc::Rc<RowContext>) -> AnyElement {
    if message.state != SendState::Sent {
        return row.into_any_element();
    }
    let (this, message) = (ctx.this.clone(), message.clone());
    row.context_menu(move |menu, _, _| {
        let item = |label: &'static str, run: MenuAction| {
            let this = this.clone();
            PopupMenuItem::new(label).on_click(move |_, window, cx| {
                let run = run.clone();
                this.update(cx, |chord, cx| run(chord, window, cx)).ok();
            })
        };
        let m = message.clone();
        let menu = menu
            .item(item("Add Reaction", Rc::new({ let m = m.clone(); move |c, w, cx| c.open_emoji_picker(EmojiTarget::Reaction(Box::new(m.clone())), w, cx) })))
            .item(item("Reply", Rc::new({ let m = m.clone(); move |c, w, cx| c.start_reply(m.clone(), w, cx) })));
        let menu = if mine { menu.item(item("Edit Message", Rc::new({ let m = m.clone(); move |c, w, cx| c.start_edit(m.clone(), w, cx) }))) } else { menu };
        let menu = menu
            .separator()
            .item(item("Copy Text", Rc::new({ let m = m.clone(); move |c, _, cx| c.copy_text(m.content.clone(), cx) })))
            .item(item("Mark Unread", Rc::new({ let m = m.clone(); move |c, _, cx| c.mark_unread(&m, cx) })))
            .item(item("Copy Message Link", Rc::new({ let m = m.clone(); move |c, _, cx| { let link = c.message_link(&m); c.copy_text(link, cx) } })));
        let menu = if mine { menu.separator().item(item("Delete Message", Rc::new({ let m = m.clone(); move |c, _, cx| c.delete(m.clone(), cx) }))) } else { menu };
        menu.separator().item(item("Copy Message ID", Rc::new({ let m = m.clone(); move |c, _, cx| c.copy_text(m.id.clone(), cx) })))
    })
    .into_any_element()
}

// The quoted line above a reply; clicking it jumps to the original, as in Discord.
fn reply_preview(reply: &Message, target: Option<String>, ctx: &std::rc::Rc<RowContext>, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let color = ctx.colors.get(&reply.author.id).copied().unwrap_or(theme.foreground);
    let snippet = if reply.content.is_empty() { "Click to see attachment".to_string() } else { plain(&readable(&reply.content, ctx)) };
    let this = ctx.this.clone();
    div()
        .id(SharedString::from(format!("reply-{}", reply.id)))
        .h(px(20.))
        .pl(px(20.))
        .mb(px(2.))
        .flex()
        .items_center()
        .gap_1()
        .cursor_pointer()
        .text_size(px(13.))
        .text_color(theme.muted_foreground)
        .child(div().w(px(32.)).h(px(10.)).mt(px(8.)).flex_none().border_l_2().border_t_2().border_color(theme.muted_foreground.opacity(0.4)).rounded_tl(px(6.)))
        .child(img(reply.author.avatar.clone()).size(px(16.)).rounded_full())
        .child(div().flex_none().font_weight(FontWeight::SEMIBOLD).text_color(color).child(format!("@{}", reply.author.name)))
        .child(div().truncate().hover(|s| s.text_color(theme.foreground)).child(snippet))
        .on_click(move |_, _, cx| {
            let Some(id) = target.clone() else { return };
            this.update(cx, |this, cx| this.jump_to(id, cx)).ok();
        })
        .into_any_element()
}

fn content(message: &Message, ctx: &std::rc::Rc<RowContext>, cx: &App) -> Option<AnyElement> {
    if message.content.is_empty() {
        return None;
    }
    // A message of only custom emoji renders them big, as Discord's "jumbo" emoji.
    let emoji = custom_emoji(&message.content);
    if !emoji.is_empty() && strip_emoji(&message.content).trim().is_empty() && emoji.len() <= 27 {
        return Some(div().flex().flex_wrap().gap_1().children(emoji.into_iter().filter_map(|e| e.url()).map(|url| img(url).size(px(48.)))).into_any_element());
    }
    // Spoilers stay covered until the message is clicked; "-# " lines are Discord's small grey subtext.
    let revealed = ctx.revealed.borrow().contains(&message.id);
    let has_spoiler = message.content.contains("||");
    let (body, subtext) = split_subtext(&message.content);
    let text = hide_spoilers(&readable(&body, ctx), revealed);
    let this = ctx.this.clone();
    let (copy_target, copied) = (ctx.this.clone(), ctx.copied.clone());
    let view = TextView::markdown(SharedString::from(format!("md-{}", message.id)), text)
        .selectable(true)
        .code_block_actions(move |block, _, cx| copy_button(block.code().to_string(), copied.as_deref() == Some(block.code().as_ref()), copy_target.clone(), cx))
        .on_link_click(move |url, _, window, cx| {
            let url = url.to_string();
            if let Some(channel_id) = url.strip_prefix("rustcord://channel/") {
                this.update(cx, |this, cx| {
                    let channel = this.guild().and_then(|g| g.channels.iter().find(|c| c.id == channel_id).cloned());
                    if let Some(channel) = channel {
                        this.open_channel(channel, window, cx);
                    }
                })
                .ok();
                return;
            }
            if !url.starts_with("rustcord://") {
                cx.open_url(&url);
            }
        });
    let (reveal, id) = (ctx.revealed.clone(), message.id.clone());
    let refresh = ctx.this.clone();
    Some(
        div()
            .id(SharedString::from(format!("content-{}", message.id)))
            .flex()
            .flex_col()
            .when(has_spoiler && !revealed, |c| {
                c.cursor_pointer().on_click(move |_, _, cx| {
                    reveal.borrow_mut().insert(id.clone());
                    refresh.update(cx, |_, cx| cx.notify()).ok();
                })
            })
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_end()
                    .gap_1()
                    .when(!body.trim().is_empty(), |c| c.child(view))
                    .when(message.edited, |c| c.child(div().text_size(px(10.)).text_color(cx.theme().muted_foreground).child("(edited)"))),
            )
            .children(subtext.into_iter().map(|line| div().text_size(px(12.)).text_color(cx.theme().muted_foreground).child(line)))
            .into_any_element(),
    )
}

fn split_subtext(content: &str) -> (String, Vec<String>) {
    let (sub, body): (Vec<&str>, Vec<&str>) = content.lines().partition(|l| l.starts_with("-# "));
    (body.join("\n"), sub.into_iter().map(|l| l.trim_start_matches("-# ").to_owned()).collect())
}

// `||secret||` → a solid bar of the same length until revealed (gpui's markdown can't paint a cover).
fn hide_spoilers(text: &str, revealed: bool) -> String {
    let parts: Vec<&str> = text.split("||").collect();
    // An odd number of `||` leaves the last one unmatched: it stays literal text.
    let paired = if parts.len().is_multiple_of(2) { parts.len() - 1 } else { parts.len() };
    let mut out = String::with_capacity(text.len());
    for (i, part) in parts.iter().enumerate() {
        match (i % 2 == 1 && i < paired, revealed) {
            (true, true) => out.push_str(part),
            (true, false) => {
                out.push('`');
                out.extend(std::iter::repeat_n('█', part.chars().count().clamp(1, 40)));
                out.push('`');
            }
            (false, _) if i >= paired => {
                out.push_str("||");
                out.push_str(part);
            }
            (false, _) => out.push_str(part),
        }
    }
    out
}

// Images and videos sized like Discord's: one fills up to 400×320, several tile smaller. A click opens
// the gallery at that item.
fn media_grid(message: &Message, ctx: &std::rc::Rc<RowContext>, cx: &App) -> Option<AnyElement> {
    if message.media.is_empty() {
        return None;
    }
    let max = if message.media.len() == 1 { MAX_IMAGE } else { (240., 240.) };
    let tiles = message.media.iter().enumerate().map(|(i, item)| {
        let scale = (max.0 / item.width).min(max.1 / item.height).min(1.);
        let (w, h) = (item.width * scale, item.height * scale);
        let (this, url) = (ctx.this.clone(), item.url.clone());
        let picture = match &item.kind {
            MediaKind::Image => img(item.url.clone()).size_full().rounded(px(8.)).into_any_element(),
            MediaKind::Video { poster } => div()
                .relative()
                .size_full()
                .child(img(poster.clone()).size_full().rounded(px(8.)))
                .child(
                    div().absolute().inset_0().flex().items_center().justify_center().child(
                        div().size(px(48.)).rounded_full().bg(gpui_kit::black().opacity(0.6)).flex().items_center().justify_center().text_color(gpui_kit::white()).child(Icon::new(Glyph::Play).size(px(24.))),
                    ),
                )
                .into_any_element(),
        };
        div()
            .id(SharedString::from(format!("media-{}-{i}", message.id)))
            .w(px(w))
            .h(px(h))
            .rounded(px(8.))
            .bg(cx.theme().muted)
            .cursor_pointer()
            .hover(|s| s.opacity(0.92))
            .child(picture)
            .on_click(move |_, window, cx| {
                this.update(cx, |this, cx| this.open_viewer(&url, window, cx)).ok();
            })
    });
    Some(div().mt_1().flex().flex_wrap().gap_1().max_w(px(MAX_IMAGE.0 + 8.)).children(tiles).into_any_element())
}

// Discord's rich embed: a card with the embed's colour down its left edge, provider, author, linked
// title, description, fields, and a thumbnail (large for video links, small beside text otherwise).
fn embed_card(embed: &Embed, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let bar: Hsla = embed.color.filter(|c| *c != 0).map(|c| gpui_kit::rgb(c).into()).unwrap_or(theme.input);
    let large = embed.thumbnail.as_ref().filter(|t| t.width >= 400.);
    let small = embed.thumbnail.as_ref().filter(|t| t.width < 400.);
    let link = embed.url.clone();
    let title = embed.title.clone().map(|title| {
        div()
            .id(SharedString::from(format!("embed-title-{}", title.len())))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(if link.is_some() { theme.link } else { theme.foreground })
            .when_some(link.clone(), |t, url| t.cursor_pointer().hover(|s| s.underline()).on_click(move |_, _, cx| cx.open_url(&url)))
            .child(title)
    });
    let text = div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap_1()
        .children(embed.provider.clone().map(|p| div().text_size(px(12.)).text_color(theme.muted_foreground).child(p)))
        .children(embed.author.clone().map(|a| div().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).child(a)))
        .children(title)
        .children(embed.description.clone().map(|d| div().text_size(px(14.)).text_color(theme.foreground.opacity(0.9)).line_clamp(6).child(d)))
        .when(!embed.fields.is_empty(), |t| {
            t.child(div().flex().flex_wrap().gap_x_4().gap_y_2().children(embed.fields.iter().map(|(name, value, inline)| {
                div()
                    .when(*inline, |f| f.min_w(px(140.)))
                    .when(!inline, |f| f.w_full())
                    .flex()
                    .flex_col()
                    .child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).child(name.clone()))
                    .child(div().text_size(px(13.)).text_color(theme.foreground.opacity(0.9)).child(value.clone()))
            })))
        })
        .children(large.map(|m| {
            let scale = (400. / m.width).min(225. / m.height).min(1.);
            img(m.url.clone()).w(px(m.width * scale)).h(px(m.height * scale)).mt_1().rounded(px(6.))
        }))
        .children(embed.footer.clone().map(|f| div().text_size(px(12.)).text_color(theme.muted_foreground).child(f)));
    div()
        .mt_1()
        .max_w(px(520.))
        .flex()
        .rounded(px(6.))
        .overflow_hidden()
        .bg(theme.muted)
        .child(div().w(px(4.)).flex_none().bg(bar))
        .child(div().flex_1().min_w_0().p_3().flex().gap_3().child(text).children(small.map(|m| img(m.url.clone()).size(px(80.)).rounded(px(6.)).flex_none())))
        .into_any_element()
}

fn files(message: &Message, cx: &App) -> Vec<AnyElement> {
    let theme = cx.theme();
    message
        .files
        .iter()
        .map(|file| {
            let url = file.url.clone();
            div()
                .id(SharedString::from(format!("file-{}-{}", message.id, file.name)))
                .mt_1()
                .w(px(400.))
                .p_3()
                .rounded(px(8.))
                .border_1()
                .border_color(theme.input)
                .bg(theme.muted)
                .flex()
                .flex_col()
                .cursor_pointer()
                .child(div().truncate().text_color(theme.link).child(file.name.clone()))
                .child(div().text_size(px(12.)).text_color(theme.muted_foreground).child(format_size(file.size)))
                .on_click(move |_, _, cx| cx.open_url(&url))
                .into_any_element()
        })
        .collect()
}

// Sits in the corner of every code block; the icon turns into a check for a moment after copying.
fn copy_button(code: String, copied: bool, this: WeakEntity<Rustcord>, cx: &App) -> AnyElement {
    let theme = cx.theme();
    div()
        .id(SharedString::from(format!("copy-{}", code.len())))
        .size(px(28.))
        .rounded(px(6.))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .text_color(if copied { theme.success } else { theme.muted_foreground })
        .hover(|s| s.bg(theme.foreground.opacity(0.08)).text_color(theme.foreground))
        .child(Icon::new(if copied { Glyph::Checkmark } else { Glyph::Copy }).size(px(16.)))
        .tooltip(move |window, cx| Tooltip::new(if copied { "Copied" } else { "Copy" }).build(window, cx))
        .on_click(move |_, _, cx| {
            this.update(cx, |this, cx| this.copy_code(code.clone(), cx)).ok();
        })
        .into_any_element()
}

fn reactions(message: &Message, ctx: &std::rc::Rc<RowContext>, cx: &App) -> Option<AnyElement> {
    if message.reactions.is_empty() {
        return None;
    }
    let theme = cx.theme();
    let pills = message.reactions.iter().map(|r| {
        let (this, msg, emoji) = (ctx.this.clone(), message.clone(), r.emoji.clone());
        div()
            .id(SharedString::from(format!("react-{}-{}", message.id, r.emoji.name)))
            .h(px(24.))
            .px_1p5()
            .flex()
            .items_center()
            .gap_1()
            .rounded(px(8.))
            .border_1()
            .cursor_pointer()
            .border_color(if r.me { crate::palette::blurple() } else { gpui_kit::transparent_black() })
            .bg(if r.me { crate::palette::blurple().opacity(0.15) } else { theme.foreground.opacity(0.06) })
            .hover(|s| s.border_color(theme.foreground.opacity(0.2)))
            .active(|s| s.opacity(0.7))
            .child(match r.emoji.url() {
                Some(url) => img(url).size(px(16.)).into_any_element(),
                None => div().text_size(px(14.)).child(r.emoji.name.clone()).into_any_element(),
            })
            .child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).child(r.count.to_string()))
            .on_click(move |_, _, cx| {
                this.update(cx, |this, cx| this.toggle_reaction(&msg, emoji.clone(), cx)).ok();
            })
    });
    Some(div().mt_1().flex().flex_wrap().gap_1().children(pills).into_any_element())
}

// The hover bar: quick reactions, reply, and for your own messages edit and delete.
fn actions(message: &Message, mine: bool, ctx: &std::rc::Rc<RowContext>, group: &SharedString, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let button = |id: String, label: &'static str| {
        let hover_ink = if label == "Delete" { theme.danger } else { theme.foreground };
        div()
            .id(SharedString::from(id))
            .size(px(32.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(6.))
            .cursor_pointer()
            .text_color(theme.muted_foreground)
            .hover(move |s| s.bg(theme.foreground.opacity(0.08)).text_color(hover_ink))
            .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(label).build(window, cx))
    };
    let mut bar = div()
        .absolute()
        .top(px(-14.))
        .right(px(16.))
        .p(px(2.))
        .flex()
        .rounded(px(8.))
        .bg(theme.popover)
        .border_1()
        .border_color(theme.border)
        .shadow_sm()
        .invisible()
        .group_hover(group.clone(), |s| s.visible());
    if message.state == SendState::Sent {
        let (this, msg) = (ctx.this.clone(), message.clone());
        bar = bar.child(button(format!("react-{}", message.id), "Add Reaction").child(Icon::new(Glyph::EmojiAdd).size(px(18.))).on_click(move |_, window, cx| {
            this.update(cx, |this, cx| this.open_emoji_picker(EmojiTarget::Reaction(Box::new(msg.clone())), window, cx)).ok();
        }));
        for quick in QUICK_REACTIONS {
            let (this, msg) = (ctx.this.clone(), message.clone());
            bar = bar.child(button(format!("quick-{}-{quick}", message.id), "Add Reaction").text_size(px(16.)).child(quick).on_click(move |_, _, cx| {
                this.update(cx, |this, cx| this.toggle_reaction(&msg, Emoji::unicode(quick), cx)).ok();
            }));
        }
        let (this, msg) = (ctx.this.clone(), message.clone());
        bar = bar.child(button(format!("reply-{}", message.id), "Reply").child(Icon::new(Glyph::Reply).size(px(18.))).on_click(move |_, window, cx| {
            this.update(cx, |this, cx| this.start_reply(msg.clone(), window, cx)).ok();
        }));
    }
    if mine && message.state == SendState::Sent {
        let (this, msg) = (ctx.this.clone(), message.clone());
        bar = bar.child(button(format!("edit-{}", message.id), "Edit").child(Icon::new(Glyph::Pen).size(px(18.))).on_click(move |_, window, cx| {
            this.update(cx, |this, cx| this.start_edit(msg.clone(), window, cx)).ok();
        }));
    }
    if mine {
        let (this, msg) = (ctx.this.clone(), message.clone());
        bar = bar.child(
            button(format!("delete-{}", message.id), "Delete").child(Icon::new(Glyph::Trash).size(px(18.))).on_click(move |_, _, cx| {
                this.update(cx, |this, cx| this.delete(msg.clone(), cx)).ok();
            }),
        );
    }
    bar.into_any_element()
}

fn chip(label: &'static str, cx: &App) -> AnyElement {
    let _ = cx;
    div()
        .h(px(16.))
        .px_1()
        .flex()
        .items_center()
        .rounded(px(4.))
        .bg(crate::palette::blurple())
        .text_color(gpui_kit::white())
        .text_size(px(10.))
        .font_weight(FontWeight::BOLD)
        .child(label)
        .into_any_element()
}

/// Discord's wire syntax made readable: mentions become links the transcript styles and routes,
/// custom emoji fall back to their `:name:`.
fn readable(content: &str, ctx: &RowContext) -> String {
    let mut out = String::with_capacity(content.len());
    let mut rest = content;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        let Some(end) = tail.find('>') else {
            out.push_str(tail);
            return out;
        };
        let token = &tail[1..end];
        let replacement = if let Some(id) = token.strip_prefix("@&") {
            ctx.role_names.get(id).map(|n| format!("**@{n}**"))
        } else if let Some(id) = token.strip_prefix("@!").or_else(|| token.strip_prefix('@')) {
            Some(format!("[@{}](rustcord://user/{id})", ctx.names.get(id).map_or("unknown-user", String::as_str)))
        } else if let Some(id) = token.strip_prefix('#') {
            ctx.channel_names.get(id).map(|n| format!("[#{n}](rustcord://channel/{id})"))
        } else if let Some(stamp) = token.strip_prefix("t:") {
            discord_timestamp(stamp)
        } else if let Some(name) = token.strip_prefix("a:").or_else(|| token.strip_prefix(':')) {
            name.split(':').next().map(|n| format!(":{n}:"))
        } else {
            None
        };
        match replacement {
            Some(r) => out.push_str(&r),
            None => out.push_str(&tail[..=end]),
        }
        rest = &tail[end + 1..];
    }
    out.push_str(rest);
    out
}

// One line of plain text for the reply quote: markdown markers and link targets dropped.
fn plain(markdown: &str) -> String {
    let mut out = String::with_capacity(markdown.len());
    let (mut skipping_target, mut after_bracket) = (false, false);
    for c in markdown.chars() {
        match c {
            '(' if after_bracket => skipping_target = true,
            ')' if skipping_target => skipping_target = false,
            _ if skipping_target => {}
            '*' | '_' | '`' | '~' | '[' | ']' => {}
            '\n' => out.push(' '),
            _ => out.push(c),
        }
        after_bracket = c == ']';
    }
    out
}

// `<t:1731590400:R>` and friends, rendered in the reader's time zone like Discord does.
fn discord_timestamp(stamp: &str) -> Option<String> {
    let (secs, style) = stamp.split_once(':').unwrap_or((stamp, "f"));
    let time = DateTime::from_timestamp(secs.parse().ok()?, 0)?.with_timezone(&Local);
    Some(format!("`{}`", match style {
        "t" => time.format("%H:%M").to_string(),
        "T" => time.format("%H:%M:%S").to_string(),
        "d" => time.format("%d/%m/%Y").to_string(),
        "D" => time.format("%-d %B %Y").to_string(),
        "F" => time.format("%A, %-d %B %Y %H:%M").to_string(),
        "R" => relative(Local::now().signed_duration_since(time)),
        _ => time.format("%-d %B %Y %H:%M").to_string(),
    }))
}

fn relative(delta: chrono::TimeDelta) -> String {
    let (secs, future) = (delta.num_seconds().abs(), delta.num_seconds() < 0);
    let (n, unit) = match secs {
        s if s < 60 => (s, "second"),
        s if s < 3600 => (s / 60, "minute"),
        s if s < 86_400 => (s / 3600, "hour"),
        s if s < 2_592_000 => (s / 86_400, "day"),
        s if s < 31_536_000 => (s / 2_592_000, "month"),
        s => (s / 31_536_000, "year"),
    };
    let unit = if n == 1 { unit.to_string() } else { format!("{unit}s") };
    if future { format!("in {n} {unit}") } else { format!("{n} {unit} ago") }
}

fn custom_emoji(content: &str) -> Vec<Emoji> {
    let mut out = Vec::new();
    let mut rest = content;
    while let Some(start) = rest.find('<') {
        let tail = &rest[start..];
        let Some(end) = tail.find('>') else { break };
        let token = &tail[1..end];
        let animated = token.starts_with("a:");
        let parts: Vec<&str> = token.trim_start_matches("a:").trim_start_matches(':').split(':').collect();
        if (token.starts_with(':') || animated) && parts.len() == 2 {
            out.push(Emoji { id: Some(parts[1].to_owned()), name: parts[0].to_owned(), animated });
        }
        rest = &tail[end + 1..];
    }
    out
}

fn strip_emoji(content: &str) -> String {
    let mut out = String::new();
    let mut rest = content;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        let Some(end) = tail.find('>') else { break };
        let token = &tail[1..end];
        if !(token.starts_with(':') || token.starts_with("a:")) {
            out.push_str(&tail[..=end]);
        }
        rest = &tail[end + 1..];
    }
    out.push_str(rest);
    out
}

// Three grey dots that brighten in turn, like Discord's.
fn typing_dots(cx: &App) -> AnyElement {
    let ink = cx.theme().muted_foreground;
    div()
        .flex()
        .gap(px(3.))
        .children((0..3).map(|i| {
            div().size(px(6.)).rounded_full().bg(ink).with_animation(
                SharedString::from(format!("typing-dot-{i}")),
                Animation::new(Duration::from_millis(1200)).repeat(),
                move |dot, t| {
                    let phase = ((t - i as f32 * 0.18) * TAU).sin().max(0.);
                    dot.opacity(0.35 + 0.65 * phase)
                },
            )
        }))
        .into_any_element()
}

// Placeholders shaped like the real transcript, sweeping in step.
fn skeleton(cx: &App) -> AnyElement {
    let bar = cx.theme().foreground.opacity(0.07);
    let row = |w: f32| {
        div()
            .flex()
            .gap_4()
            .child(div().size(px(40.)).flex_none().rounded_full().bg(bar))
            .child(div().flex_1().flex().flex_col().gap_2().pt_1().child(div().w(px(120.)).h(px(12.)).rounded(px(6.)).bg(bar)).child(div().w(px(w)).h(px(12.)).rounded(px(6.)).bg(bar)))
    };
    div()
        .size_full()
        .px_4()
        .pb_4()
        .flex()
        .flex_col()
        .justify_end()
        .gap_6()
        .children([360., 220., 420., 280.].map(row))
        .with_animation("skeleton", Animation::new(Duration::from_millis(1100)).repeat(), |el, t| el.opacity(0.6 + 0.4 * (t * TAU).cos().abs()))
        .into_any_element()
}

// The red line marking where unread messages begin, with the date when a new day starts there too.
fn new_divider(day_of: Option<String>, cx: &App) -> AnyElement {
    let red = cx.theme().danger;
    div()
        .w_full()
        .px_4()
        .mt_4()
        .flex()
        .items_center()
        .gap_2()
        .text_size(px(12.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(red)
        .child(div().flex_1().h(px(1.)).bg(red))
        .children(day_of.and_then(|t| day(&t)).map(|d| div().flex().items_center().gap_2().child(d.format("%-d %B %Y").to_string()).child(div().w(px(40.)).h(px(1.)).bg(red))))
        .child(div().h(px(16.)).px_1().rounded(px(4.)).bg(red).text_color(gpui_kit::white()).text_size(px(10.)).font_weight(FontWeight::BOLD).flex().items_center().child("NEW"))
        .into_any_element()
}

// Stays inside the transcript gutters, never edge to edge.
fn day_divider(timestamp: &str, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let line = || div().flex_1().h(px(1.)).bg(theme.border);
    div()
        .w_full()
        .px_4()
        .mt_4()
        .flex()
        .items_center()
        .gap_2()
        .text_size(px(12.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme.muted_foreground)
        .child(line())
        .child(day(timestamp).map(|d| d.format("%-d %B %Y").to_string()).unwrap_or_default())
        .child(line())
        .into_any_element()
}

pub fn empty_state(id: &'static str, mood: Mood, title: &'static str, detail: impl Into<SharedString>, cx: &App) -> AnyElement {
    let detail = detail.into();
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_3()
        .px_6()
        .child(clyde(id, mood, cx))
        .child(div().text_size(px(16.)).font_weight(FontWeight::SEMIBOLD).child(title))
        .when(!detail.is_empty(), |s| s.child(div().max_w(px(360.)).text_center().text_color(cx.theme().muted_foreground).child(detail)))
        .into_any_element()
}

fn continues(previous: &Message, message: &Message) -> bool {
    let (Ok(a), Ok(b)) = (DateTime::parse_from_rfc3339(&previous.timestamp), DateTime::parse_from_rfc3339(&message.timestamp)) else {
        return false;
    };
    previous.author.id == message.author.id && (b - a).num_seconds() < GROUP_GAP_SECS
}

fn day(timestamp: &str) -> Option<NaiveDate> {
    Some(DateTime::parse_from_rfc3339(timestamp).ok()?.with_timezone(&Local).date_naive())
}

// Discord's header stamp: "14:00" today, "Yesterday at 23:03", then the full date.
fn format_stamp(timestamp: &str) -> String {
    let Ok(time) = DateTime::parse_from_rfc3339(timestamp) else { return String::new() };
    let time = time.with_timezone(&Local);
    match (Local::now().date_naive() - time.date_naive()).num_days() {
        0 => time.format("%H:%M").to_string(),
        1 => time.format("Yesterday at %H:%M").to_string(),
        _ => time.format("%d/%m/%Y, %H:%M").to_string(),
    }
}

fn format_clock(timestamp: &str) -> String {
    DateTime::parse_from_rfc3339(timestamp).map(|t| t.with_timezone(&Local).format("%H:%M").to_string()).unwrap_or_default()
}

fn format_size(bytes: u64) -> String {
    match bytes {
        b if b >= 1 << 20 => format!("{:.1} MB", b as f64 / (1 << 20) as f64),
        b if b >= 1 << 10 => format!("{:.0} KB", b as f64 / (1 << 10) as f64),
        b => format!("{b} bytes"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jumbo_emoji_detection() {
        assert_eq!(custom_emoji("<:blob:1> <a:dance:2>").len(), 2);
        assert!(strip_emoji("<:blob:1> <a:dance:2>").trim().is_empty());
        assert_eq!(strip_emoji("hi <:blob:1> <@5>"), "hi  <@5>");
    }

    #[test]
    fn reply_quotes_are_plain() {
        assert_eq!(plain("the **new** [@Wren](rustcord://user/1) `list`\nok"), "the new @Wren list ok");
    }

    #[test]
    fn timestamps_read_like_discord() {
        assert_eq!(relative(chrono::TimeDelta::seconds(-7200)), "in 2 hours");
        assert_eq!(relative(chrono::TimeDelta::seconds(60)), "1 minute ago");
        assert!(discord_timestamp("0:d").is_some_and(|t| t.starts_with('`')));
        assert!(discord_timestamp("nonsense").is_none());
    }

    #[test]
    fn spoilers_cover_until_revealed() {
        assert_eq!(hide_spoilers("a ||bc|| d", false), "a `██` d");
        assert_eq!(hide_spoilers("a ||bc|| d", true), "a bc d");
        assert_eq!(hide_spoilers("a || b", false), "a || b");
        assert_eq!(split_subtext("hi\n-# small"), ("hi".into(), vec!["small".into()]));
    }

    #[test]
    fn sizes_read_like_discord() {
        assert_eq!(format_size(512), "512 bytes");
        assert_eq!(format_size(2048), "2 KB");
        assert_eq!(format_size(3 * 1024 * 1024), "3.0 MB");
    }
}
