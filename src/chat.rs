// The middle column: channel header, transcript and composer, ported from design/messages.html and
// design/navigation.html (channel header).
//
// GAP: hover fades (row fill, toolbar, 150ms) are instant: gpui's `.hover` styles don't transition.
use std::{collections::HashMap, f32::consts::TAU, rc::Rc, time::Duration};

use chrono::{DateTime, Local, NaiveDate};
use gpui_kit::{
    Animation, AnimationExt, AnyElement, App, BoxShadow, Context, Corners, Div, Focusable, FontWeight, HighlightStyle, Hsla, KeyDownEvent, Pixels, SharedString, Stateful, StyledText, WeakEntity, Window,
    WindowControlArea,
    component::{
        ActiveTheme, Icon, Sizable, Size,
        input::{Input, Textarea},
        text::TextView,
        tooltip::Tooltip,
    },
    div, img, list, point, prelude::*, px,
};
use gpui_motion_kit::core::{Motion, Target};

use crate::{
    EmojiTarget, Feed, RightPanel, Rustcord,
    discord::{Channel, ChannelKind, Embed, Emoji, MediaKind, Message, SendState, User},
    icons::Glyph,
    mascot::Mood,
    motion,
    palette::{self, Level},
    ui::{self, RADIUS_LG, RADIUS_MD, RADIUS_SM, RADIUS_XS, Squircle},
};

// Messages from one author closer together than this share a header, like Discord.
const GROUP_GAP_SECS: i64 = 7 * 60;
// A lone image fits inside this; galleries and cards fill the media column.
const MAX_IMAGE: (f32, f32) = (400., 320.);
const COLUMN: f32 = 432.;
const GALLERY_GAP: f32 = 4.;
const AVATAR: f32 = 40.;
// The avatar column plus the gap to the content column: where replies and continuations line up.
const CONTENT_INSET: f32 = AVATAR + 16.;

impl Rustcord {
    pub fn render_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let main = div().relative().flex_1().min_w_0().h_full().flex().flex_col().bg(palette::surface(Level::Page, cx)).border_l_1().border_color(palette::line(cx));
        let Some(channel) = self.channel.clone() else {
            return main.child(empty_state("idle", Mood::Idle, "No channel open", "Pick a server on the left, then a channel to start reading.", cx));
        };
        let header = self.channel_header(&channel, window, cx);
        let body = match self.feeds.get(&channel.id) {
            Some(Feed { loaded: true, messages, .. }) if messages.is_empty() => empty_state("empty", Mood::Happy, "No messages yet", "Be the first to say something.", cx),
            Some(Feed { loaded: true, .. }) => self.render_messages(cx).into_any_element(),
            Some(Feed { error: Some(error), .. }) => empty_state("failed", Mood::Idle, "Couldn't load messages", error.clone(), cx),
            _ => skeleton(cx),
        };
        let (body, unread_bar) = match self.stream_view(window, cx) {
            Some(stream) => (stream, None),
            None => (body, self.unread_bar(&channel.id, cx)),
        };
        let chat = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .on_drop(cx.listener(|this, paths: &gpui_kit::ExternalPaths, _, cx| this.add_files(paths.paths().to_vec(), cx)))
            .child(div().relative().flex_1().min_h_0().child(body).children(unread_bar))
            .child(self.render_composer(window, cx));
        main.child(header).child(div().flex_1().min_h_0().flex().child(chat).children(self.render_panel(window, cx)))
    }

    // Where you are as breadcrumbs, the topic, then the channel's tools (design: navigation.html's chanbar).
    fn channel_header(&self, channel: &Channel, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let (strong, faint, selected) = (palette::strong(cx), palette::faint(cx), palette::selected(cx));
        let parent = self.thread_parent(channel).map(|parent| {
            crumb("thread-parent", cx)
                .child(Icon::new(Glyph::Hashtag).size(px(14.)))
                .child(div().min_w_0().truncate().child(parent.name.clone()))
                .on_click(cx.listener(move |this, _, window, cx| this.open_channel(parent.clone(), window, cx)))
        });
        let glyph = match channel.kind {
            ChannelKind::Dm => Glyph::User,
            ChannelKind::Thread => Glyph::Thread,
            _ => Glyph::Hashtag,
        };
        let here = div()
            .h(px(32.))
            .px(px(6.))
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(6.))
            .text_size(px(16.))
            .font_weight(FontWeight::BOLD)
            .text_color(strong)
            .child(Icon::new(glyph).size(px(20.)).text_color(faint))
            .child(div().min_w_0().truncate().child(channel.name.clone()));
        // Thread names are often whole sentences: the crumbs cap at 60% and truncate rather than push the tools off.
        let crumbs = div()
            .flex_none()
            .max_w(gpui_kit::relative(0.6))
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(2.))
            .when_some(parent, |c, parent| c.child(parent).child(Icon::new(Glyph::ChevronRight).size(px(14.)).text_color(faint)))
            .child(here);
        let topic = channel.topic.clone().map(|topic| div().flex_1().min_w_0().pl(px(12.)).border_l_1().border_color(palette::line_strong(cx)).truncate().text_size(px(14.)).text_color(faint).child(topic));
        let threads = (channel.kind == ChannelKind::Text && self.guild_id.is_some()).then(|| {
            let id = channel.id.clone();
            ui::icon_button("threads-button", Glyph::Thread, "Threads", cx)
                .occlude()
                .on_mouse_down(gpui_kit::MouseButton::Left, cx.listener(move |this, e: &gpui_kit::MouseDownEvent, _, cx| this.open_threads_popout(id.clone(), e.position, cx)))
        });
        let pins = ui::icon_button("pins-button", Glyph::Pin, "Pinned Messages", cx)
            .occlude()
            .when(self.pins.is_some(), |b| b.bg(selected).text_color(strong))
            .on_click(cx.listener(|this, _, _, cx| this.open_pins(cx)));
        let members_on = matches!(self.right_panel, RightPanel::Members);
        let members = self.guild_id.is_some().then(|| {
            ui::icon_button("members-toggle", Glyph::Users, if members_on { "Hide Member List" } else { "Show Member List" }, cx)
                .occlude()
                .when(members_on, |b| b.bg(selected).text_color(strong))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.right_panel = if matches!(this.right_panel, RightPanel::Members) { RightPanel::Hidden } else { RightPanel::Members };
                    cx.notify();
                }))
        });
        let search = self.guild_id.is_some().then(|| self.channel_search(window, cx));
        div()
            .h(px(48.))
            .flex_none()
            .pl(px(12.))
            .pr(px(8.))
            .flex()
            .items_center()
            .gap(px(8.))
            .bg(palette::surface(Level::Page, cx))
            .border_b_1()
            .border_color(palette::line(cx))
            // Drag area: every control inside it occludes it, or Windows' caption takes its clicks.
            .window_control_area(WindowControlArea::Drag)
            .child(crumbs)
            .children(topic)
            .child(div().ml_auto().flex_none().flex().items_center().gap(px(2.)).children(threads).child(pins).children(members).children(search))
    }

    fn channel_search(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let focused = self.search_input.read(cx).focus_handle(cx).is_focused(window);
        div()
            .w(px(200.))
            .h(px(32.))
            .ml(px(8.))
            .px(px(10.))
            .flex_none()
            .occlude()
            .flex()
            .items_center()
            .gap(px(6.))
            .squircle(RADIUS_SM)
            .bg(palette::surface(Level::Rail, cx))
            .border_1()
            .border_color(if focused { palette::accent() } else { gpui_kit::transparent_black() })
            .text_size(px(14.))
            .text_color(palette::faint(cx))
            .child(div().flex_1().min_w_0().child(Input::new(&self.search_input).appearance(false).px_0().text_size(px(14.))))
            .child(Icon::new(Glyph::Search).size(px(16.)))
    }

    // The bar over the feed while unread messages wait: jump to the first, or mark the channel read.
    fn unread_bar(&self, channel_id: &str, cx: &mut Context<Self>) -> Option<AnyElement> {
        let read = self.unread_marker?;
        let feed = self.feeds.get(channel_id)?;
        let fresh: Vec<&Message> = feed.messages.iter().filter(|m| crate::order(&m.id) > read).collect();
        let first = fresh.first()?;
        let label = format!("{} new {} since {}", fresh.len(), if fresh.len() == 1 { "message" } else { "messages" }, clock(&first.timestamp));
        let tint = mix(palette::surface(Level::Page, cx), palette::accent(), 0.16);
        let press = palette::accent().opacity(0.14);
        let button = move |id: &'static str| div().id(id).h(px(28.)).px(px(8.)).flex().items_center().gap(px(6.)).squircle(RADIUS_XS).cursor_pointer().hover(move |s| s.bg(press));
        let (first_id, channel_id) = (first.id.clone(), channel_id.to_owned());
        Some(
            div()
                .absolute()
                .top_0()
                .left(px(8.))
                .right(px(8.))
                .min_h(px(36.))
                .pl(px(12.))
                .pr(px(4.))
                .flex()
                .items_center()
                .gap(px(8.))
                .squircle(RADIUS_SM)
                .bg(tint)
                .text_size(px(14.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette::accent_ink(cx))
                .child(
                    button("unread-jump")
                        .flex_1()
                        .min_w_0()
                        .ml(px(-8.))
                        .child(Icon::new(Glyph::ArrowUp).size(px(16.)))
                        .child(div().min_w_0().truncate().child(label))
                        .on_click(cx.listener(move |this, _, _, cx| this.jump_to(first_id.clone(), cx))),
                )
                .child(button("unread-read").child("Mark as read").child(Icon::new(Glyph::Checkmark).size(px(16.))).on_click(cx.listener(move |this, _, _, cx| {
                    this.mark_channel_read(&channel_id, cx);
                    this.unread_marker = None;
                    cx.notify();
                })))
                .into_any_element(),
        )
    }

    // A Go Live we're watching takes the transcript's place, fitted to the column, like Discord's stream view.
    fn stream_view(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let watching = self.call.as_ref()?.focused()?;
        let name = self.user_name(&watching.streamer_id);
        let stage: AnyElement = match &watching.frame {
            Some(frame) => {
                let viewport = window.viewport_size();
                let (max_w, max_h) = (f32::from(viewport.width) - self.channels_width - 400., f32::from(viewport.height) - 260.);
                let (w, h) = (frame.size(0).width.0 as f32, frame.size(0).height.0 as f32);
                let scale = (max_w / w).min(max_h / h).clamp(0.1, 2.);
                img(gpui_kit::ImageSource::Render(frame.clone())).w(px(w * scale)).h(px(h * scale)).squircle(RADIUS_XS).into_any_element()
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
                        .child(div().h(px(18.)).px_1().squircle(RADIUS_XS).bg(theme.danger).text_size(px(11.)).font_weight(FontWeight::BOLD).flex().items_center().child("LIVE"))
                        .child(div().flex_1().truncate().font_weight(FontWeight::SEMIBOLD).child(format!("{name}'s screen")))
                        .child(
                            div()
                                .id("stop-watching")
                                .h(px(30.))
                                .px_3()
                                .squircle(RADIUS_SM)
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

    pub(crate) fn render_messages(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(channel) = &self.channel else { return div().into_any_element() };
        let Some(feed) = self.feeds.get(&channel.id) else { return div().into_any_element() };
        // A pointer copy: the feed is copy-on-write, so this frame shares it rather than cloning every message.
        let messages = feed.messages.clone();
        let ctx = Rc::new(RowContext {
            me: self.me.clone(),
            names: self
                .guild_id
                .as_ref()
                .and_then(|g| self.members.get(g))
                .into_iter()
                .flat_map(|m| m.values())
                .chain(messages.iter().flat_map(|m| std::iter::once(&m.author).chain(&m.mentions)))
                .chain(self.me.iter())
                .map(|u| (u.id.clone(), u.name.clone()))
                .collect(),
            colors: messages.iter().filter_map(|m| Some((m.author.id.clone(), self.role_color(m.guild_id.as_ref().or(self.guild_id.as_ref()), &m.author.id, m.roles.as_ref(), cx.theme().is_dark())?))).collect(),
            mentions_me: messages.iter().filter(|m| self.mentions_me(m)).map(|m| m.id.clone()).collect(),
            channel_names: self.guild().map(|g| g.channels.iter().map(|c| (c.id.clone(), c.name.clone())).collect()).unwrap_or_default(),
            role_names: self.guild().map(|g| g.roles.iter().map(|r| (r.id.clone(), r.name.clone())).collect()).unwrap_or_default(),
            flash: self.flash.clone(),
            editing: self.editing.as_ref().map(|m| m.id.clone()),
            copied: self.copied.clone(),
            revealed: self.revealed.clone(),
            unread_marker: self.unread_marker,
            this: cx.weak_entity(),
            has_more: feed.has_more,
            quick_reactions: self.prefs.frequent_emojis(3, &["💯", "😆", "💖"]),
            shift: self.shift_held,
            manage_messages: self.guild().is_some_and(|g| self.me.as_ref().is_some_and(|me| g.can(&me.id, crate::discord::perm::MANAGE_MESSAGES))),
            threads: self.guild_id.is_some() && channel.kind == ChannelKind::Text,
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
            // One accent line above the first message after your read position.
            let first_unread = ctx.unread_marker.is_some_and(|read| crate::order(&message.id) > read && previous.is_none_or(|p| crate::order(&p.id) <= read));
            let row = match new_day || first_unread {
                false => row,
                true => div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .when(new_day, |d| d.child(day_divider(&message.timestamp, cx)))
                    .when(first_unread, |d| d.child(unread_line()))
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
        .pt(px(12.))
        .pb(px(4.))
        .into_any_element()
    }

    // One surface at the bottom of the chat: bars it gains (reply, attachments) stack inside it above
    // the text, one hairline apart, and popovers rise from its top edge.
    pub(crate) fn render_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.composer.read(cx).focus_handle(cx).is_focused(window);
        let (strong, muted) = (palette::strong(cx), palette::muted(cx));
        let row = div()
            .min_h(px(48.))
            .py(px(7.))
            .px(px(6.))
            .flex()
            .items_end()
            .gap(px(4.))
            .child(ui::icon_button("attach", Glyph::PlusCircle, "Upload a File", cx).on_click(cx.listener(|this, _, window, cx| this.upload(window, cx))))
            // XSmall gives the editor 0/4px padding of its own; with these, the design's 4/6.
            .child(div().flex_1().min_w_0().child(Textarea::new(&self.composer).appearance(false).with_size(Size::XSmall).py(px(4.)).px(px(2.)).text_size(px(16.)).line_height(px(24.)).text_color(strong)))
            .child(ui::icon_button("composer-gif", Glyph::Gif, "Open GIF picker", cx).on_click(cx.listener(|this, _, window, cx| this.open_gifs(window, cx))))
            .child(ui::icon_button("composer-emoji", Glyph::Emoji, "Select Emoji", cx).on_click(cx.listener(|this, _, window, cx| this.open_emoji_picker(EmojiTarget::Composer, window, cx))));
        let card = div()
            .relative()
            .mx(px(16.))
            .flex()
            .flex_col()
            .squircle(RADIUS_LG)
            .bg(palette::surface(Level::Raised, cx))
            .border_1()
            .border_color(if focused { palette::line_strong(cx) } else { palette::line(cx) })
            .children(self.autocomplete_popup(cx))
            .children(self.composer_bar(cx))
            .children(self.attachment_tray(cx))
            .child(row);
        let typers = self.typers();
        let names: Vec<&str> = typers.iter().map(|t| t.name.as_str()).collect();
        // A reserved slot above the composer, so the box never jumps when someone starts typing.
        let typing = div()
            .h(px(24.))
            .px(px(24.))
            .flex()
            .items_center()
            .gap(px(8.))
            .min_w_0()
            .text_size(px(12.))
            .text_color(muted)
            .when(!names.is_empty(), |slot| slot.child(ui::bars("typing", palette::accent_ink(cx), 12.)).child(div().min_w_0().truncate().child(emphasised(&typing_parts(&names), strong))));
        let note = self.send_error.clone().map(|error| div().mx(px(16.)).mt(px(6.)).text_size(px(12.)).text_color(palette::danger_ink(cx)).child(error));
        div()
            .relative()
            .flex_none()
            .flex()
            .flex_col()
            .capture_key_down(cx.listener(Self::on_composer_key))
            .children(self.jump_to_present(cx))
            .child(typing)
            .child(card)
            .children(note)
            .child(div().h(px(12.)))
    }

    // A small capsule 12px above the composer, out of the flow, so it never opens a gap.
    fn jump_to_present(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        if !self.scrolled_up {
            return None;
        }
        let overlay = palette::surface(Level::Overlay, cx);
        let hover = mix(overlay, palette::strong(cx), 0.06);
        let capsule = div()
            .id("jump-to-present")
            .h(px(32.))
            .px(px(14.))
            .flex()
            .items_center()
            .gap(px(8.))
            .rounded_full()
            .bg(overlay)
            .shadow(shadow_pop(cx))
            .cursor_pointer()
            .active(|s| s.scale(0.97))
            .whitespace_nowrap()
            .text_size(px(14.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(palette::strong(cx))
            .hover(move |s| s.bg(hover))
            .child(Icon::new(Glyph::ArrowDown).size(px(16.)))
            .child("Jump to present")
            .on_click(cx.listener(|this, _, _, cx| {
                this.list.scroll_to_end();
                this.scrolled_up = false;
                cx.notify();
            }));
        Some(div().absolute().left_0().right_0().top(px(-44.)).flex().justify_center().child(capsule).with_animation(
            "jump-to-present-in",
            Animation::new(ui::DUR_POP).with_easing(gpui_kit::ease_out_quint()),
            |d, t| d.opacity(t).mt(px((1. - t) * 8.)),
        ))
    }

    // The reply or edit bar at the top of the composer.
    fn composer_bar(&self, cx: &mut Context<Self>) -> Option<Div> {
        let strong = palette::strong(cx);
        let label = match (&self.replying, &self.editing) {
            (Some(reply), _) => emphasised(&[("Replying to ".into(), false), (reply.author.name.clone(), true)], strong),
            (_, Some(_)) => emphasised(&[("Editing message".into(), true)], strong),
            _ => return None,
        };
        let bar = div()
            .min_h(px(40.))
            .py(px(4.))
            .pl(px(14.))
            .pr(px(6.))
            .flex()
            .items_center()
            .gap(px(6.))
            .min_w_0()
            .border_b_1()
            .border_color(palette::line(cx))
            .text_size(px(14.))
            .text_color(palette::muted(cx))
            .child(div().flex_1().min_w_0().truncate().child(label));
        let bar = match self.replying.is_some() {
            true => bar.child(self.ping_toggle(cx)),
            false => bar.child(key_hint("Esc", "Cancel", cx)).child(key_hint("Enter", "Save", cx)),
        };
        Some(bar.child(icon_button_sm("cancel-compose", Glyph::X, "Cancel", cx).on_click(cx.listener(|this, _, window, cx| this.cancel_compose(window, cx)))))
    }

    // Whether the reply notifies its author; it remembers your last choice.
    fn ping_toggle(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let on = self.reply_ping;
        let hover = palette::hover(cx);
        div()
            .id("reply-ping")
            .h(px(28.))
            .px(px(8.))
            .flex()
            .items_center()
            .gap(px(4.))
            .squircle(RADIUS_XS)
            .cursor_pointer()
            .text_size(px(12.))
            .font_weight(FontWeight::BOLD)
            .text_color(if on { palette::accent_ink(cx) } else { palette::faint(cx) })
            .hover(move |s| s.bg(hover))
            .child(Icon::new(Glyph::AtBold).size(px(16.)))
            .child(if on { "On" } else { "Off" })
            .tooltip(|window, cx| Tooltip::new("Mention the author of this message").build(window, cx))
            .on_click(cx.listener(|this, _, _, cx| {
                this.reply_ping = !this.reply_ping;
                cx.notify();
            }))
    }

    // Files waiting to send: borderless wells inside the composer, never cards in a card.
    fn attachment_tray(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        if self.pending_files.is_empty() {
            return None;
        }
        let (selected, raised, strong, faint, muted) = (palette::selected(cx), palette::surface(Level::Raised, cx), palette::strong(cx), palette::faint(cx), palette::muted(cx));
        let mut chips = Vec::with_capacity(self.pending_files.len());
        for (i, path) in self.pending_files.iter().enumerate() {
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let size = std::fs::metadata(path).map(|m| format_size(m.len())).unwrap_or_default();
            let is_image = matches!(path.extension().and_then(|e| e.to_str()).map(str::to_lowercase).as_deref(), Some("png" | "jpg" | "jpeg" | "gif" | "webp"));
            let preview = match is_image {
                true => ui::media(path.clone(), ui::all(RADIUS_XS)).size(px(40.)).flex_none().object_fit(gpui_kit::ObjectFit::Cover).into_any_element(),
                false => div().size(px(40.)).flex_none().squircle(RADIUS_XS).bg(raised).text_color(muted).flex().items_center().justify_center().child(Icon::new(Glyph::File).size(px(24.))).into_any_element(),
            };
            chips.push(
                div()
                    .w(px(216.))
                    .flex_none()
                    .py(px(6.))
                    .pl(px(6.))
                    .pr(px(4.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .squircle(RADIUS_MD)
                    .bg(selected)
                    .child(preview)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(div().truncate().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).text_color(strong).child(name))
                            .child(div().truncate().text_size(px(12.)).text_color(faint).child(size)),
                    )
                    .child(icon_button_sm(SharedString::from(format!("remove-file-{i}")), Glyph::X, "Remove Attachment", cx).on_click(cx.listener(move |this, _, _, cx| this.remove_file(i, cx)))),
            );
        }
        Some(div().id("composer-tray").flex().gap(px(8.)).p(px(10.)).overflow_x_scroll().border_b_1().border_color(palette::line(cx)).children(chips))
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

    // Suggestions above the composer: its width, 8px above it, rising from it.
    fn autocomplete_popup(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let auto = self.autocomplete.as_ref()?;
        let (hover, selected, strong, muted, text) = (palette::hover(cx), palette::selected(cx), palette::strong(cx), palette::muted(cx), palette::text(cx));
        let heading = if auto.token.starts_with('#') { "Text Channels" } else { "Members" };
        let rows = auto.items.iter().enumerate().map(|(i, item)| {
            let lead = match &item.avatar {
                Some(url) => ui::avatar(url.clone(), 28.).into_any_element(),
                None => div().size(px(28.)).flex_none().rounded_full().bg(selected).text_color(muted).flex().items_center().justify_center().child(Icon::new(Glyph::Hashtag).size(px(16.))).into_any_element(),
            };
            div()
                .id(SharedString::from(format!("suggest-{i}")))
                .min_h(px(40.))
                .py(px(6.))
                .px(px(10.))
                .flex()
                .items_center()
                .gap(px(10.))
                .squircle(RADIUS_SM)
                .cursor_pointer()
                .text_color(text)
                .when(i == auto.selected, |r| r.bg(selected))
                .when(i != auto.selected, |r| r.hover(move |s| s.bg(hover)))
                .child(lead)
                .child(div().flex_1().min_w_0().truncate().font_weight(FontWeight::SEMIBOLD).text_color(strong).child(item.label.clone()))
                .on_click(cx.listener(move |this, _, window, cx| {
                    if let Some(auto) = &mut this.autocomplete {
                        auto.selected = i;
                    }
                    this.accept_suggestion(window, cx);
                }))
        });
        // msg-pop: it grows from 95% at its bottom centre as it fades in.
        Some(
            div()
                .absolute()
                .left(px(-1.))
                .right(px(-1.))
                .bottom(gpui_kit::relative(1.))
                .mb(px(9.))
                .p(px(6.))
                .flex()
                .flex_col()
                .gap(px(2.))
                .max_h(px(340.))
                .squircle(RADIUS_LG)
                .bg(palette::surface(Level::Overlay, cx))
                .shadow(shadow_pop(cx))
                .text_size(px(14.))
                .child(div().pt(px(6.)).px(px(10.)).pb(px(4.)).text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(muted).child(heading))
                .children(rows)
                .transform_origin(point(gpui_kit::relative(0.5), gpui_kit::relative(1.)))
                .with_animation("autocomplete", Animation::new(ui::DUR_POP).with_easing(gpui_kit::ease_out_quint()), |el, t| el.opacity(t).scale(0.95 + 0.05 * t)),
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
    editing: Option<String>,
    copied: Option<String>,
    revealed: Rc<std::cell::RefCell<std::collections::HashSet<String>>>,
    unread_marker: Option<u64>,
    this: WeakEntity<Rustcord>,
    has_more: bool,
    // The toolbar's three quick reactions: most used, padded with 💯 😆 💖.
    quick_reactions: Vec<Emoji>,
    shift: bool,
    manage_messages: bool,
    // Messages here can start threads.
    threads: bool,
}

// An avatar column and a content column; a continuation shows its time in the gutter on hover, and the
// toolbar floats at the row's top right.
fn message_row(message: &Message, starts_group: bool, ctx: &Rc<RowContext>, cx: &mut App) -> AnyElement {
    let media = media_grid(message, ctx, cx);
    let group = SharedString::from(format!("msg-{}", message.id));
    let mine = ctx.me.as_ref().is_some_and(|me| me.id == message.author.id);
    let mention = ctx.mentions_me.contains(&message.id);
    // Held at the hover fill while it's being edited, or flashed after a jump to it.
    let held = ctx.flash.as_ref() == Some(&message.id) || ctx.editing.as_ref() == Some(&message.id);
    let (hover, soft) = (palette::hover(cx), palette::accent_soft(cx));
    let mention_hover = Hsla { a: soft.a * 0.94 + 0.06, ..soft };
    let ink = match message.state {
        SendState::Sent => palette::text(cx),
        SendState::Sending => palette::faint(cx),
        SendState::Failed => palette::muted(cx),
    };
    let pending = message.state != SendState::Sent;
    let body = div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .text_size(px(16.))
        .text_color(ink)
        .children(content(message, ctx, cx))
        .children(media.map(|m| div().when(pending, |d| d.opacity(0.6)).child(m)))
        .children(message.embeds.iter().enumerate().map(|(i, e)| embed_card(e, format!("embed-{}-{i}", message.id), ctx, cx)))
        .children(message.thread.as_ref().map(|t| thread_card(t, ctx, cx)))
        .children(files(message, cx))
        .children(reactions(message, ctx, cx))
        .when(message.state == SendState::Failed, |b| b.child(send_failure(message, ctx, cx)));
    let row = div()
        .id(group.clone())
        .group(group.clone())
        .relative()
        .w_full()
        .px(px(16.))
        .py(px(2.))
        .squircle(RADIUS_SM)
        .flex()
        .flex_col()
        .line_height(gpui_kit::relative(1.45))
        .when(mention, |r| r.bg(soft).hover(move |s| s.bg(mention_hover)))
        .when(!mention, |r| r.when(held, |r| r.bg(hover)).hover(move |s| s.bg(hover)))
        .child(actions(message, mine, ctx, &group, cx));
    if !starts_group {
        let gutter = div()
            .w(px(AVATAR))
            .flex_none()
            .flex()
            .justify_end()
            .line_height(px(23.))
            .text_size(px(12.))
            .text_color(palette::faint(cx))
            .whitespace_nowrap()
            .map(|g| match message.state {
                SendState::Sending => g.child(Icon::new(Glyph::Clock).size(px(14.)).mt(px(4.))),
                _ => g.invisible().group_hover(group, |s| s.visible()).child(gutter_clock(&message.timestamp)),
            });
        return inset(with_message_menu(row.child(div().flex().gap(px(16.)).child(gutter).child(body)), message, ctx));
    }
    let row = row
        .mt(px(12.))
        .pt(px(4.))
        .children(message.reply.as_ref().map(|reply| reply_preview(reply, message.reply_to.clone(), ctx, cx)))
        .child(
            div()
                .flex()
                .gap(px(16.))
                .child(profile_trigger(format!("avatar-{}", message.id), &message.author, ctx).flex_none().mt(px(2.)).child(crate::layout::decorated_avatar(
                    &message.author.avatar,
                    message.author.decoration.as_deref(),
                    AVATAR,
                    None,
                    palette::surface(Level::Page, cx),
                )))
                .child(div().flex_1().min_w_0().flex().flex_col().child(message_head(message, ctx, cx)).child(body)),
        );
    inset(with_message_menu(row, message, ctx))
}

// Rows sit 8px in from the feed's edges, so their hover fill reads as a rounded card.
fn inset(row: AnyElement) -> AnyElement {
    div().w_full().px(px(8.)).child(row).into_any_element()
}

// Name in its top role's colour, tags, the time, and a pin when pinned.
fn message_head(message: &Message, ctx: &Rc<RowContext>, cx: &App) -> impl IntoElement + use<> {
    let name_color = ctx.colors.get(&message.author.id).copied().unwrap_or(palette::strong(cx));
    div()
        .flex()
        .items_baseline()
        .gap(px(8.))
        .min_w_0()
        .child(
            profile_trigger(format!("name-{}", message.id), &message.author, ctx)
                .min_w_0()
                .truncate()
                .text_size(px(16.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(name_color)
                .hover(|s| s.underline())
                .child(message.author.name.clone()),
        )
        .children(message.author.tag.clone().map(|tag| ui::badge(palette::selected(cx), palette::strong(cx)).h(px(18.)).px(px(6.)).gap(px(3.)).children(tag.badge.map(|b| img(b).size(px(12.)))).child(tag.text)))
        .when(message.author.bot, |h| h.child(ui::badge(palette::accent_soft(cx), palette::accent_ink(cx)).h(px(18.)).px(px(6.)).gap(px(3.)).child(Icon::new(Glyph::Checkmark).size(px(11.))).child("APP")))
        .child(div().flex_none().whitespace_nowrap().text_size(px(12.)).text_color(palette::faint(cx)).child(format_stamp(&message.timestamp)))
        .when(message.pinned, |h| h.child(div().flex_none().self_center().text_color(palette::muted(cx)).child(Icon::new(Glyph::Pin).size(px(14.)))))
}

// Clicking an avatar or name opens their profile popout at the pointer, as in Discord.
fn profile_trigger(id: String, user: &User, ctx: &Rc<RowContext>) -> Stateful<Div> {
    let (this, user) = (ctx.this.clone(), user.clone());
    let (menu_this, menu_user) = (this.clone(), user.clone());
    div()
        .id(SharedString::from(id))
        .cursor_pointer()
        .on_click(move |event, _, cx| {
            let at = event.position();
            this.update(cx, |this, cx| this.open_profile(user.clone(), at, cx)).ok();
        })
        // The author's menu, not the message's: Discord stops the click here.
        .on_mouse_down(gpui_kit::MouseButton::Right, move |event, _, cx| {
            cx.stop_propagation();
            let at = event.position;
            menu_this.update(cx, |this, cx| this.open_author_menu(menu_user.clone(), at, cx)).ok();
        })
}

// Right-click opens the same list as the toolbar's More.
fn with_message_menu(row: Stateful<Div>, message: &Message, ctx: &Rc<RowContext>) -> AnyElement {
    let (this, message) = (ctx.this.clone(), message.clone());
    row.on_mouse_down(gpui_kit::MouseButton::Right, move |event, _, cx| {
        let at = event.position;
        this.update(cx, |this, cx| this.open_message_menu(message.clone(), at, cx)).ok();
    })
    .into_any_element()
}

// The reference above a reply, in the content column; clicking it jumps to the original and flashes it.
fn reply_preview(reply: &Message, target: Option<String>, ctx: &Rc<RowContext>, cx: &App) -> AnyElement {
    let (faint, muted, text) = (palette::faint(cx), palette::muted(cx), palette::text(cx));
    let snippet = match reply.content.is_empty() {
        true => div().flex_none().flex().items_center().gap(px(4.)).text_color(muted).child(Icon::new(attachment_glyph(reply)).size(px(14.))).child(attachment_kind(reply)),
        false => div().min_w_0().truncate().child(plain(&readable_with(&reply.content, ctx, false))),
    };
    let this = ctx.this.clone();
    div()
        .id(SharedString::from(format!("reply-{}", reply.id)))
        .pl(px(CONTENT_INSET))
        .mb(px(2.))
        .min_w_0()
        .flex()
        .items_center()
        .gap(px(6.))
        .cursor_pointer()
        .text_size(px(14.))
        .text_color(faint)
        .hover(move |s| s.text_color(text))
        .child(Icon::new(Glyph::ReplyRight).size(px(14.)))
        .child(ui::avatar(reply.author.avatar.clone(), 16.))
        .child(div().flex_none().font_weight(FontWeight::SEMIBOLD).text_color(muted).child(reply.author.name.clone()))
        .child(snippet)
        .on_click(move |_, _, cx| {
            let Some(id) = target.clone() else { return };
            this.update(cx, |this, cx| this.jump_to(id, cx)).ok();
        })
        .into_any_element()
}

// What a reply without text quotes instead: the kind of attachment it carries.
fn attachment_kind(message: &Message) -> &'static str {
    match message.media.first().map(|m| &m.kind) {
        Some(MediaKind::Image) => "Image",
        Some(MediaKind::Video { .. }) => "Video",
        Some(MediaKind::Gif { .. }) => "GIF",
        None if !message.files.is_empty() => "File",
        None => "Attachment",
    }
}

fn attachment_glyph(message: &Message) -> Glyph {
    match message.media.is_empty() && !message.files.is_empty() {
        true => Glyph::File,
        false => Glyph::Image,
    }
}

// Discord drops the text of a message that is nothing but the link its GIF or image embed shows.
fn only_media_link(message: &Message) -> bool {
    let text = message.content.trim();
    let is_link = (text.starts_with("https://") || text.starts_with("http://")) && !text.contains(char::is_whitespace);
    is_link && !message.media.is_empty() && message.embeds.is_empty()
}

fn content(message: &Message, ctx: &Rc<RowContext>, cx: &App) -> Option<AnyElement> {
    if message.content.is_empty() || only_media_link(message) {
        return None;
    }
    // A message of only custom emoji renders them big, as Discord's "jumbo" emoji.
    let emoji = custom_emoji(&message.content);
    if !emoji.is_empty() && strip_emoji(&message.content).trim().is_empty() && emoji.len() <= 27 {
        return Some(div().flex().flex_wrap().gap_1().children(emoji.into_iter().filter_map(|e| e.url()).enumerate().map(|(i, url)| img(url).id(("jumbo", i)).size(px(48.)))).into_any_element());
    }
    // Spoilers stay covered until the message is clicked; "-# " lines are Discord's small grey subtext.
    let revealed = ctx.revealed.borrow().contains(&message.id);
    let has_spoiler = message.content.contains("||");
    let (body, subtext) = split_subtext(&message.content);
    let text = hide_spoilers(&readable(&body, ctx), revealed);
    let view = markdown(SharedString::from(format!("md-{}", message.id)), text, ctx);
    let (reveal, id) = (ctx.revealed.clone(), message.id.clone());
    let refresh = ctx.this.clone();
    let faint = palette::faint(cx);
    Some(
        div()
            .id(SharedString::from(format!("content-{}", message.id)))
            .w_full()
            .min_w_0()
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
                    .items_baseline()
                    // Capped at the row's width so long lines wrap instead of running off the side.
                    .when(!body.trim().is_empty(), |c| c.child(div().min_w_0().max_w_full().child(view)))
                    .when(message.edited, |c| c.child(div().ml(px(4.)).whitespace_nowrap().text_size(px(12.)).text_color(faint).child("(edited)"))),
            )
            .children(subtext.into_iter().map(|line| div().text_size(px(12.)).text_color(faint).child(line)))
            .into_any_element(),
    )
}

// Discord markdown with mentions, channel links and code copy buttons; messages and embeds share it.
fn markdown(id: SharedString, text: String, ctx: &RowContext) -> TextView {
    let this = ctx.this.clone();
    let (copy_target, copied) = (ctx.this.clone(), ctx.copied.clone());
    TextView::markdown(id, text)
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
        })
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

// Not sent: the text stays, with why and the two ways out inline.
fn send_failure(message: &Message, ctx: &Rc<RowContext>, cx: &App) -> AnyElement {
    let (retry_this, retry) = (ctx.this.clone(), message.clone());
    let (delete_this, delete) = (ctx.this.clone(), message.clone());
    let action = |id: String, label: &'static str| ui::button(SharedString::from(id), ui::Tone::Ghost, cx).small().h(px(28.)).child(label);
    div()
        .mt(px(4.))
        .flex()
        .flex_wrap()
        .items_center()
        .gap_x(px(8.))
        .gap_y(px(4.))
        .text_size(px(14.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(palette::danger_ink(cx))
        .child(Icon::new(Glyph::WarningCircle).size(px(16.)))
        .child("Not sent.")
        .child(action(format!("retry-{}", message.id), "Resend message").on_click(move |_, _, cx| {
            retry_this.update(cx, |this, cx| this.retry(retry.clone(), cx)).ok();
        }))
        .child(action(format!("discard-{}", message.id), "Delete message").on_click(move |_, _, cx| {
            delete_this.update(cx, |this, cx| this.delete(delete.clone(), cx)).ok();
        }))
        .into_any_element()
}

// The card under the message that started a thread: its name and how many messages it holds.
fn thread_card(thread: &Channel, ctx: &Rc<RowContext>, cx: &App) -> AnyElement {
    let meta = thread.thread.as_ref();
    let count = meta.map_or(0, |t| t.message_count);
    let archived = meta.is_some_and(|t| t.archived);
    let (this, open) = (ctx.this.clone(), thread.clone());
    let edge = palette::line_strong(cx);
    div()
        .id(SharedString::from(format!("thread-card-{}", thread.id)))
        .active(|s| s.scale(0.99))
        .mt(px(8.))
        .w_full()
        .max_w(px(COLUMN))
        .py(px(10.))
        .px(px(12.))
        .flex()
        .items_center()
        .gap(px(8.))
        .min_w_0()
        .squircle(RADIUS_LG)
        .bg(palette::surface(Level::Sidebar, cx))
        .border_1()
        .border_color(palette::line(cx))
        .hover(move |s| s.border_color(edge))
        .cursor_pointer()
        .text_size(px(14.))
        .child(div().min_w_0().truncate().font_weight(FontWeight::SEMIBOLD).text_color(if archived { palette::muted(cx) } else { palette::strong(cx) }).child(thread.name.clone()))
        .child(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap(px(2.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette::muted(cx))
                .child(format!("{count} {}", if count == 1 { "message" } else { "messages" }))
                .child(Icon::new(Glyph::ChevronRight).size(px(14.))),
        )
        .on_click(move |_, window, cx| {
            this.update(cx, |this, cx| this.open_channel(open.clone(), window, cx)).ok();
        })
        .into_any_element()
}

// gpui only advances an animated image's frames when the img has an id to keep its frame state under,
// so every img that may hold a GIF (or animated WebP) gets one.
pub(crate) fn animated_image(source: impl Into<gpui_kit::ImageSource>) -> Stateful<gpui_kit::Img> {
    img(source).id("animated")
}

// Media at its own shape: one alone fits 400×320; several tile the 432px column (2 side by side,
// 3 one tall and two stacked, 4 two by two, 5 or more two over three with the last counting the rest).
fn media_grid(message: &Message, ctx: &Rc<RowContext>, cx: &mut App) -> Option<AnyElement> {
    let first = message.media.first()?;
    if message.media.len() == 1 {
        let scale = (MAX_IMAGE.0 / first.width).min(MAX_IMAGE.1 / first.height).min(1.);
        let (w, h) = (first.width * scale, first.height * scale);
        return Some(div().mt(px(8.)).w(px(w)).h(px(h)).child(media_tile(message, 0, (w, h), ui::all(RADIUS_LG), ctx, cx)).into_any_element());
    }
    let (height, tiles) = gallery_layout(message.media.len());
    let rest = message.media.len() - tiles.len();
    let mut grid = div().mt(px(8.)).relative().flex_none().w(px(COLUMN)).h(px(height));
    for (i, tile) in tiles.iter().enumerate() {
        let corners = gallery_corners(*tile, height);
        let counts_rest = rest > 0 && i + 1 == tiles.len();
        grid = grid.child(
            div()
                .absolute()
                .left(px(tile.x))
                .top(px(tile.y))
                .w(px(tile.w))
                .h(px(tile.h))
                .child(media_tile(message, i, (tile.w, tile.h), corners, ctx, cx))
                .when(counts_rest, |t| {
                    t.child(
                        ui::edge(div(), corners)
                            .absolute()
                            .inset_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(on_media(0.55))
                            .text_color(gpui_kit::white())
                            .text_size(px(20.))
                            .font_weight(FontWeight::BOLD)
                            .child(format!("+{rest}")),
                    )
                }),
        );
    }
    Some(grid.into_any_element())
}

// One picture, video or GIF, rounded to `corners`; a click opens the viewer at it.
fn media_tile(message: &Message, i: usize, (w, h): (f32, f32), corners: Corners<Pixels>, ctx: &Rc<RowContext>, cx: &mut App) -> AnyElement {
    let item = &message.media[i];
    // A video plays where it is; clicks on it belong to its player.
    if let MediaKind::Video { poster } = &item.kind {
        return div().size_full().child(crate::video::tile(format!("{}-{i}", message.id), &item.url, poster, cx)).into_any_element();
    }
    // Fetched at the size it's drawn rather than full resolution; the viewer still opens the original.
    let sized = |url: &str| crate::discord::sized_image(url, w, h, (item.width, item.height));
    let picture = match &item.kind {
        MediaKind::Gif { poster } => div()
            .relative()
            .size_full()
            .child(crate::gif::rounded_tile(format!("{}-{i}-{}", message.id, item.url), &item.url, &sized(poster), corners, cx))
            .child(media_tag("GIF").top(px(8.)).left(px(8.)))
            .into_any_element(),
        _ => ui::edge(animated_image(sized(&item.url)), corners).size_full().object_fit(gpui_kit::ObjectFit::Cover).into_any_element(),
    };
    let (this, url) = (ctx.this.clone(), item.url.clone());
    ui::edge(div(), corners)
        .id(SharedString::from(format!("media-{}-{i}", message.id)))
        .size_full()
        .bg(palette::selected(cx))
        .cursor_pointer()
        .hover(|s| s.opacity(0.92))
        .child(picture)
        .on_click(move |_, window, cx| {
            this.update(cx, |this, cx| this.open_viewer(&url, window, cx)).ok();
        })
        .into_any_element()
}

// A label over media (GIF, a duration): fixed dark values, since it sits on pictures in both themes.
fn media_tag(text: &'static str) -> Div {
    div().absolute().h(px(20.)).px(px(6.)).flex().items_center().gap(px(4.)).squircle(RADIUS_XS).bg(on_media(0.72)).text_color(gpui_kit::white()).text_size(px(11.)).font_weight(FontWeight::BOLD).child(text)
}

// The design's media scrim, oklch(0.14 0.01 350), at `alpha`.
fn on_media(alpha: f32) -> Hsla {
    Hsla::from(gpui_kit::rgb(0x120E10)).opacity(alpha)
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Tile {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

// The gallery's height and its tiles for `count` items in the media column; past five, five tiles show.
fn gallery_layout(count: usize) -> (f32, Vec<Tile>) {
    let tile = |x, y, w, h| Tile { x, y, w, h };
    let half = (COLUMN - GALLERY_GAP) / 2.;
    match count {
        0 | 1 => (0., Vec::new()),
        2 => {
            let h = COLUMN / 2.;
            (h, vec![tile(0., 0., half, h), tile(half + GALLERY_GAP, 0., half, h)])
        }
        3 => {
            let h = COLUMN * 2. / 3.;
            let wide = (COLUMN - GALLERY_GAP) * 2. / 3.;
            let (narrow, row) = (COLUMN - GALLERY_GAP - wide, (h - GALLERY_GAP) / 2.);
            let right = wide + GALLERY_GAP;
            (h, vec![tile(0., 0., wide, h), tile(right, 0., narrow, row), tile(right, row + GALLERY_GAP, narrow, row)])
        }
        4 => {
            let h = COLUMN * 3. / 4.;
            let row = (h - GALLERY_GAP) / 2.;
            (h, [(0., 0.), (1., 0.), (0., 1.), (1., 1.)].map(|(c, r)| tile(c * (half + GALLERY_GAP), r * (row + GALLERY_GAP), half, row)).to_vec())
        }
        _ => {
            let h = COLUMN * 2. / 3.;
            let top = (h - GALLERY_GAP) * 3. / 5.;
            let (bottom, third) = (h - GALLERY_GAP - top, (COLUMN - 2. * GALLERY_GAP) / 3.);
            let mut tiles = vec![tile(0., 0., half, top), tile(half + GALLERY_GAP, 0., half, top)];
            tiles.extend((0..3).map(|i| tile(i as f32 * (third + GALLERY_GAP), top + GALLERY_GAP, third, bottom)));
            (h, tiles)
        }
    }
}

// The gallery's outer corners take the card radius; inner ones a hairline 2px.
fn gallery_corners(tile: Tile, height: f32) -> Corners<Pixels> {
    let at = |a: f32, b: f32| (a - b).abs() < 0.5;
    let (left, top, right, bottom) = (at(tile.x, 0.), at(tile.y, 0.), at(tile.x + tile.w, COLUMN), at(tile.y + tile.h, height));
    let radius = |outer: bool| px(if outer { RADIUS_LG } else { 2. });
    Corners { top_left: radius(top && left), top_right: radius(top && right), bottom_left: radius(bottom && left), bottom_right: radius(bottom && right) }
}

// A link preview: it leads with the site, never a coloured bar, one surface step from the feed. Link
// previews keep a thumbnail at the right; large images run the card's width.
fn embed_card(embed: &Embed, key: String, ctx: &RowContext, cx: &App) -> AnyElement {
    let (strong, muted, faint) = (palette::strong(cx), palette::muted(cx), palette::faint(cx));
    let link_ink = palette::accent_ink(cx);
    let large = embed.thumbnail.as_ref().filter(|t| t.width >= 400.);
    let small = embed.thumbnail.as_ref().filter(|t| t.width < 400.);
    let md = |part: &str, text: &str| markdown(SharedString::from(format!("{key}-{part}")), readable(text, ctx), ctx);
    let site = embed.provider.clone().map(|provider| {
        let mark: Hsla = embed.color.filter(|c| *c != 0).map(|c| gpui_kit::rgb(c).into()).unwrap_or(palette::info_ink(cx));
        div().flex().items_center().gap(px(6.)).min_w_0().text_size(px(12.)).font_weight(FontWeight::MEDIUM).text_color(faint).child(div().size(px(14.)).flex_none().squircle(4.).bg(mark)).child(div().min_w_0().truncate().child(provider))
    });
    let author = embed.author.clone().map(|name| {
        let url = embed.author_url.clone();
        div()
            .id(SharedString::from(format!("{key}-author")))
            .flex()
            .items_center()
            .gap(px(6.))
            .text_size(px(14.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(strong)
            .children(embed.author_icon.clone().map(|icon| ui::avatar(icon, 20.)))
            .child(div().when(url.is_some(), |d| d.cursor_pointer().hover(|s| s.underline())).child(name))
            .when_some(url, |d, url| d.on_click(move |_, _, cx| cx.open_url(&url)))
    });
    let link = embed.url.clone();
    let title = embed.title.clone().map(|title| {
        div()
            .id(SharedString::from(format!("{key}-title")))
            .text_size(px(14.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(if link.is_some() { link_ink } else { strong })
            .when_some(link.clone(), |t, url| t.cursor_pointer().hover(|s| s.underline()).on_click(move |_, _, cx| cx.open_url(&url)))
            .child(title)
    });
    let fields = (!embed.fields.is_empty()).then(|| {
        div().flex().flex_wrap().gap(px(8.)).children(embed.fields.iter().enumerate().map(|(i, (name, value, inline))| {
            div()
                .when(*inline, |f| f.min_w(px(150.)))
                .when(!inline, |f| f.w_full())
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(div().font_weight(FontWeight::SEMIBOLD).text_color(strong).child(name.clone()))
                .child(div().text_color(muted).child(md(&format!("field-{i}"), value)))
        }))
    });
    let footer_text: Vec<String> = embed.footer.clone().into_iter().chain(embed.timestamp.as_deref().map(format_stamp)).collect();
    let footer = (!footer_text.is_empty()).then(|| {
        div().flex().items_center().gap(px(6.)).text_size(px(12.)).text_color(faint).children(embed.footer_icon.clone().map(|icon| ui::avatar(icon, 20.))).child(footer_text.join(" · "))
    });
    let image = large.map(|m| {
        // The card's inner width: the column less its padding and border.
        let w = m.width.min(COLUMN - 30.);
        let h = w * m.height / m.width;
        img(crate::discord::sized_image(&m.url, w, h, (m.width, m.height))).w(px(w)).h(px(h)).mt(px(4.)).squircle(RADIUS_SM)
    });
    let text = div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(6.))
        .text_size(px(14.))
        .children(site)
        .children(author)
        .children(title)
        .children(embed.description.as_deref().map(|d| div().text_color(muted).child(md("description", d))))
        .children(fields)
        .children(image)
        .children(footer);
    div()
        .mt(px(8.))
        .w_full()
        .max_w(px(COLUMN))
        .py(px(12.))
        .px(px(14.))
        .flex()
        .items_start()
        .gap(px(14.))
        .squircle(RADIUS_LG)
        .bg(palette::surface(Level::Sidebar, cx))
        .border_1()
        .border_color(palette::line(cx))
        .child(text)
        .children(small.map(|m| img(m.url.clone()).size(px(80.)).flex_none().squircle(RADIUS_SM).object_fit(gpui_kit::ObjectFit::Cover)))
        .into_any_element()
}

// A file: its icon, name, size and one action.
fn files(message: &Message, cx: &App) -> Vec<AnyElement> {
    let (strong, faint, muted) = (palette::strong(cx), palette::faint(cx), palette::muted(cx));
    message
        .files
        .iter()
        .map(|file| {
            let (open, download) = (file.url.clone(), file.url.clone());
            div()
                .mt(px(8.))
                .w_full()
                .max_w(px(COLUMN))
                .p(px(12.))
                .flex()
                .items_center()
                .gap(px(12.))
                .squircle(RADIUS_LG)
                .bg(palette::surface(Level::Sidebar, cx))
                .border_1()
                .border_color(palette::line(cx))
                .when(message.state != SendState::Sent, |d| d.opacity(0.6))
                .child(div().w(px(40.)).flex_none().flex().justify_center().text_color(muted).child(Icon::new(Glyph::File).size(px(36.))))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .id(SharedString::from(format!("file-{}-{}", message.id, file.name)))
                                .truncate()
                                .cursor_pointer()
                                .text_size(px(14.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(strong)
                                .hover(|s| s.underline())
                                .child(file.name.clone())
                                .on_click(move |_, _, cx| cx.open_url(&open)),
                        )
                        .child(div().text_size(px(12.)).text_color(faint).child(format_size(file.size))),
                )
                .child(ui::icon_button(SharedString::from(format!("download-{}-{}", message.id, file.name)), Glyph::Download, "Download", cx).on_click(move |_, _, cx| cx.open_url(&download)))
                .into_any_element()
        })
        .collect()
}

// Sits in the corner of every code block; it turns to a check, in the online ink, for a moment after.
// GAP: the design gives code blocks a header line (language, then Copy with its label); TextView owns
// that layout and only offers this corner slot, so the button stays icon-only.
fn copy_button(code: String, copied: bool, this: WeakEntity<Rustcord>, cx: &App) -> AnyElement {
    let (hover, strong) = (palette::hover(cx), palette::strong(cx));
    div()
        .id(SharedString::from(format!("copy-{}", code.len())))
        .h(px(28.))
        .w(px(28.))
        .justify_center()
        .flex()
        .items_center()
        .squircle(RADIUS_XS)
        .cursor_pointer()
        .text_color(if copied { palette::positive_ink(cx) } else { palette::muted(cx) })
        .when(copied, |b| b.hover(move |s| s.bg(hover)))
        .when(!copied, |b| b.hover(move |s| s.bg(hover).text_color(strong)))
        .child(Icon::new(if copied { Glyph::Checkmark } else { Glyph::Copy }).size(px(16.)))
        .tooltip(move |window, cx| Tooltip::new(if copied { "Copied" } else { "Copy" }).build(window, cx))
        .on_click(move |_, _, cx| {
            this.update(cx, |this, cx| this.copy_code(code.clone(), cx)).ok();
        })
        .into_any_element()
}

// One click adds or removes yours; yours carry the accent, the rest stay neutral. Add stays last.
fn reactions(message: &Message, ctx: &Rc<RowContext>, cx: &App) -> Option<AnyElement> {
    if message.reactions.is_empty() {
        return None;
    }
    let pills = message.reactions.iter().map(|r| {
        let (this, msg, emoji) = (ctx.this.clone(), message.clone(), r.emoji.clone());
        reaction_pill(SharedString::from(format!("react-{}-{}", message.id, r.emoji.name)), r.me, cx)
            .px(px(9.))
            .child(match r.emoji.url() {
                Some(url) => img(url).size(px(18.)).into_any_element(),
                None => div().text_size(px(16.)).line_height(px(16.)).child(r.emoji.name.clone()).into_any_element(),
            })
            .child(r.count.to_string())
            .on_click(move |_, _, cx| {
                this.update(cx, |this, cx| this.toggle_reaction(&msg, emoji.clone(), cx)).ok();
            })
    });
    let (this, msg) = (ctx.this.clone(), message.clone());
    let add = reaction_pill(SharedString::from(format!("react-add-{}", message.id)), false, cx)
        .px(px(7.))
        .child(Icon::new(Glyph::EmojiAdd).size(px(18.)))
        .tooltip(|window, cx| Tooltip::new("Add Reaction").build(window, cx))
        .on_click(move |_, window, cx| {
            this.update(cx, |this, cx| this.open_emoji_picker(EmojiTarget::Reaction(Box::new(msg.clone())), window, cx)).ok();
        });
    Some(div().mt(px(6.)).flex().flex_wrap().gap(px(6.)).children(pills).child(add).into_any_element())
}

fn reaction_pill(id: SharedString, mine: bool, cx: &App) -> Stateful<Div> {
    let edge = palette::line_strong(cx);
    div()
        .id(id)
        .active(|s| s.scale(0.92))
        .h(px(28.))
        .flex()
        .items_center()
        .gap(px(6.))
        .squircle(RADIUS_SM)
        .border_1()
        .cursor_pointer()
        .text_size(px(14.))
        .font_weight(FontWeight::SEMIBOLD)
        .when(mine, |p| p.bg(palette::accent_soft(cx)).border_color(palette::accent().opacity(0.55)).text_color(palette::accent_ink(cx)))
        .when(!mine, |p| p.bg(palette::selected(cx)).border_color(gpui_kit::transparent_black()).text_color(palette::muted(cx)).hover(move |s| s.border_color(edge)))
}

// The hover toolbar: three quick reactions, then react, reply, thread and more. With Shift held, edit
// and delete take the quick reactions' place behind a Shift keycap; the rest stay put.
fn actions(message: &Message, mine: bool, ctx: &Rc<RowContext>, group: &SharedString, cx: &App) -> AnyElement {
    let (hover, line) = (palette::hover(cx), palette::line(cx));
    let sep = || div().w(px(1.)).h(px(20.)).mx(px(2.)).bg(line);
    let sent = message.state == SendState::Sent;
    let shifted = ctx.shift && (mine || ctx.manage_messages);
    let mut bar = div()
        .absolute()
        .top(px(-18.))
        .right(px(12.))
        .p(px(2.))
        .flex()
        .items_center()
        .gap(px(2.))
        .squircle(RADIUS_MD)
        .bg(palette::surface(Level::Overlay, cx))
        .shadow(shadow_pop(cx))
        .invisible()
        .group_hover(group.clone(), |s| s.visible());
    if shifted || message.state == SendState::Failed {
        if shifted {
            bar = bar.child(ui::kbd("Shift", cx).ml(px(6.)).mr(px(4.)));
        }
        if mine && sent {
            let (this, msg) = (ctx.this.clone(), message.clone());
            bar = bar.child(ui::icon_button(SharedString::from(format!("edit-{}", message.id)), Glyph::Pen, "Edit", cx).on_click(move |_, window, cx| {
                this.update(cx, |this, cx| this.start_edit(msg.clone(), window, cx)).ok();
            }));
        }
        let (this, msg) = (ctx.this.clone(), message.clone());
        let (danger, danger_ink) = (palette::danger(), palette::danger_ink(cx));
        bar = bar.child(
            ui::icon_button(SharedString::from(format!("delete-{}", message.id)), Glyph::Trash, "Delete", cx)
                .text_color(danger_ink)
                .hover(move |s| s.bg(danger).text_color(gpui_kit::white()))
                .on_click(move |_, _, cx| {
                    this.update(cx, |this, cx| this.delete(msg.clone(), cx)).ok();
                }),
        );
        if sent {
            bar = bar.child(sep());
        }
    } else if sent {
        for quick in ctx.quick_reactions.iter().cloned() {
            let (this, msg, emoji) = (ctx.this.clone(), message.clone(), quick.clone());
            let face: AnyElement = match quick.url() {
                Some(url) => img(url).size(px(20.)).into_any_element(),
                None => div().text_size(px(18.)).line_height(px(18.)).child(quick.name.clone()).into_any_element(),
            };
            bar = bar.child(
                div()
                    .id(SharedString::from(format!("quick-{}-{}", message.id, quick.name)))
                    .size(px(32.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .squircle(RADIUS_SM)
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover))
                    .active(|s| s.scale(0.92))
                    .child(face)
                    .tooltip(|window, cx| Tooltip::new("Click to react").build(window, cx))
                    .on_click(move |_, _, cx| {
                        this.update(cx, |this, cx| this.toggle_reaction(&msg, emoji.clone(), cx)).ok();
                    }),
            );
        }
        bar = bar.child(sep());
    }
    if sent {
        let (this, msg) = (ctx.this.clone(), message.clone());
        bar = bar.child(ui::icon_button(SharedString::from(format!("react-{}", message.id)), Glyph::Emoji, "Add Reaction", cx).on_click(move |_, window, cx| {
            this.update(cx, |this, cx| this.open_emoji_picker(EmojiTarget::Reaction(Box::new(msg.clone())), window, cx)).ok();
        }));
        let (this, msg) = (ctx.this.clone(), message.clone());
        bar = bar.child(ui::icon_button(SharedString::from(format!("reply-{}", message.id)), Glyph::Reply, "Reply", cx).on_click(move |_, window, cx| {
            this.update(cx, |this, cx| this.start_reply(msg.clone(), window, cx)).ok();
        }));
        if ctx.threads && message.thread.is_none() {
            let (this, msg) = (ctx.this.clone(), message.clone());
            bar = bar.child(ui::icon_button(SharedString::from(format!("thread-{}", message.id)), Glyph::ThreadNew, "Create Thread", cx).on_click(move |_, window, cx| {
                this.update(cx, |this, cx| this.create_thread(msg.clone(), window, cx)).ok();
            }));
        }
    }
    let (this, msg) = (ctx.this.clone(), message.clone());
    bar = bar.child(ui::icon_button(SharedString::from(format!("more-{}", message.id)), Glyph::MoreVertical, "More", cx).on_click(move |event, _, cx| {
        let at = event.position();
        this.update(cx, |this, cx| this.open_message_menu(msg.clone(), at, cx)).ok();
    }));
    // Deferred so it paints above the next message's images and embeds, which come later in the list.
    gpui_kit::deferred(bar).with_priority(1).into_any_element()
}

/// Discord's wire syntax made readable: mentions become links the transcript styles and routes,
/// custom emoji become inline images at Discord's 1.375em (animated ones play).
fn readable(content: &str, ctx: &RowContext) -> String {
    readable_with(content, ctx, true)
}

// `inline_emoji` false keeps custom emoji as `:name:`, for one-line plain text such as reply quotes.
fn readable_with(content: &str, ctx: &RowContext, inline_emoji: bool) -> String {
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
            match (inline_emoji, emoji_image(token)) {
                (true, Some(html)) => Some(html),
                _ => name.split(':').next().map(|n| format!(":{n}:")),
            }
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

// `a:name:id` or `:name:id` as an inline image the markdown view lays out in the text flow. Its
// images carry element ids, so animated emoji play. 22px is Discord's 1.375em at 16px text.
fn emoji_image(token: &str) -> Option<String> {
    let animated = token.starts_with("a:");
    let mut parts = token.trim_start_matches("a:").trim_start_matches(':').split(':');
    let (name, id) = (parts.next()?, parts.next()?);
    if parts.next().is_some() || name.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let url = Emoji { id: Some(id.to_owned()), name: name.to_owned(), animated }.url()?;
    Some(format!(r#"<img src="{url}" alt=":{name}:" width="22" height="22" />"#))
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

// Who is typing, names emphasised: "Mira is typing…", "Mira and Theo are typing…", then "Several people".
fn typing_parts(names: &[&str]) -> Vec<(String, bool)> {
    let name = |n: &str| (n.to_owned(), true);
    let word = |w: &str| (w.to_owned(), false);
    match names {
        [] => Vec::new(),
        [one] => vec![name(one), word(" is typing…")],
        [a, b] => vec![name(a), word(" and "), name(b), word(" are typing…")],
        [a, b, c] => vec![name(a), word(", "), name(b), word(" and "), name(c), word(" are typing…")],
        _ => vec![word("Several people are typing…")],
    }
}

// One line of text with some runs in strong semibold, as the design's <b>.
fn emphasised(parts: &[(String, bool)], strong: Hsla) -> StyledText {
    let mut text = String::new();
    let mut runs = Vec::new();
    for (part, bold) in parts {
        let start = text.len();
        text.push_str(part);
        if *bold {
            runs.push((start..text.len(), HighlightStyle { color: Some(strong), font_weight: Some(FontWeight::SEMIBOLD), ..Default::default() }));
        }
    }
    StyledText::new(text).with_highlights(runs)
}

// A keycap and what it does, for the edit bar.
fn key_hint(key: &'static str, label: &'static str, cx: &App) -> Div {
    div().flex_none().flex().items_center().gap(px(6.)).text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(palette::muted(cx)).child(ui::kbd(key, cx)).child(label)
}

// Placeholders shaped like the real transcript, sweeping in step.
fn skeleton(cx: &App) -> AnyElement {
    let bar = palette::selected(cx);
    let row = |w: f32| {
        div()
            .flex()
            .gap(px(16.))
            .child(div().size(px(AVATAR)).flex_none().rounded_full().bg(bar))
            .child(div().flex_1().flex().flex_col().gap_2().pt_1().child(div().w(px(120.)).h(px(12.)).rounded_full().bg(bar)).child(div().w(px(w)).h(px(12.)).rounded_full().bg(bar)))
    };
    div()
        .size_full()
        .px(px(24.))
        .pb_4()
        .flex()
        .flex_col()
        .justify_end()
        .gap_6()
        .children([360., 220., 420., 280.].map(row))
        .with_animation("skeleton", Animation::new(Duration::from_millis(1100)).repeat(), |el, t| el.opacity(0.6 + 0.4 * (t * TAU).cos().abs()))
        .into_any_element()
}

// One accent line where unread starts, ending in a New badge.
fn unread_line() -> AnyElement {
    div()
        .w_full()
        .px(px(16.))
        .my(px(8.))
        .flex()
        .items_center()
        .gap(px(8.))
        .child(div().flex_1().h(px(1.)).bg(palette::accent()))
        .child(ui::badge(palette::accent(), gpui_kit::white()).h(px(18.)).px(px(6.)).child("New"))
        .into_any_element()
}

// Between dates, inside the feed's gutters, never edge to edge.
fn day_divider(timestamp: &str, cx: &App) -> AnyElement {
    let line = || div().flex_1().h(px(1.)).bg(palette::line(cx));
    div()
        .w_full()
        .px(px(16.))
        .mt(px(16.))
        .mb(px(8.))
        .flex()
        .items_center()
        .gap(px(10.))
        .whitespace_nowrap()
        .text_size(px(12.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(palette::faint(cx))
        .child(line())
        .child(day(timestamp).map(|d| d.format("%A, %B %-d, %Y").to_string()).unwrap_or_default())
        .child(line())
        .into_any_element()
}

fn empty_state(id: &'static str, mood: Mood, title: &'static str, detail: impl Into<SharedString>, cx: &App) -> AnyElement {
    div().size_full().flex().items_center().justify_center().child(ui::empty_state(id, mood, title, detail, cx)).into_any_element()
}

fn continues(previous: &Message, message: &Message) -> bool {
    let (Ok(a), Ok(b)) = (DateTime::parse_from_rfc3339(&previous.timestamp), DateTime::parse_from_rfc3339(&message.timestamp)) else {
        return false;
    };
    previous.author.id == message.author.id && (b - a).num_seconds() < GROUP_GAP_SECS
}

fn local(timestamp: &str) -> Option<DateTime<Local>> {
    Some(DateTime::parse_from_rfc3339(timestamp).ok()?.with_timezone(&Local))
}

fn day(timestamp: &str) -> Option<NaiveDate> {
    Some(local(timestamp)?.date_naive())
}

// "Today at 9:41 PM", "Yesterday at 11:58 PM", the weekday within the week, then the date.
fn format_stamp(timestamp: &str) -> String {
    let Some(time) = local(timestamp) else { return String::new() };
    match (Local::now().date_naive() - time.date_naive()).num_days() {
        0 => time.format("Today at %-I:%M %p").to_string(),
        1 => time.format("Yesterday at %-I:%M %p").to_string(),
        2..=6 => time.format("%A at %-I:%M %p").to_string(),
        _ => time.format("%B %-d, %Y at %-I:%M %p").to_string(),
    }
}

// "9:41 PM".
fn clock(timestamp: &str) -> String {
    local(timestamp).map(|t| t.format("%-I:%M %p").to_string()).unwrap_or_default()
}

// The continuation gutter's "9:41": the half of the day is the header's to say.
fn gutter_clock(timestamp: &str) -> String {
    local(timestamp).map(|t| t.format("%-I:%M").to_string()).unwrap_or_default()
}

fn format_size(bytes: u64) -> String {
    match bytes {
        b if b >= 1 << 20 => format!("{:.1} MB", b as f64 / (1 << 20) as f64),
        b if b >= 1 << 10 => format!("{:.0} KB", b as f64 / (1 << 10) as f64),
        b => format!("{b} bytes"),
    }
}

// A breadcrumb back up the hierarchy: muted until hovered.
fn crumb(id: &'static str, cx: &App) -> Stateful<Div> {
    let (hover, strong) = (palette::hover(cx), palette::strong(cx));
    div()
        .id(id)
        .occlude()
        .h(px(32.))
        .px(px(6.))
        .min_w_0()
        .flex()
        .items_center()
        .gap(px(6.))
        .squircle(RADIUS_SM)
        .cursor_pointer()
        .whitespace_nowrap()
        .font_weight(FontWeight::MEDIUM)
        .text_color(palette::muted(cx))
        .hover(move |s| s.bg(hover).text_color(strong))
}

// MOVE(ui): icon_button_sm, the design's .iconbtn--sm: 24px, 16px glyph, the small radius.
fn icon_button_sm(id: impl Into<gpui_kit::ElementId>, glyph: Glyph, tooltip: &'static str, cx: &App) -> Stateful<Div> {
    let (hover, press, strong) = (palette::hover(cx), palette::press(cx), palette::strong(cx));
    div()
        .id(id)
        .size(px(24.))
        .flex_none()
        .squircle(RADIUS_XS)
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .text_color(palette::muted(cx))
        .hover(move |s| s.bg(hover).text_color(strong))
        .active(move |s| s.bg(press))
        .child(Icon::new(glyph).size(px(16.)))
        .tooltip(move |window, cx| Tooltip::new(tooltip).build(window, cx))
}

// MOVE(ui): shadow_pop, the design's --shadow-pop: a soft drop and a 1px line_strong ring.
fn shadow_pop(cx: &App) -> Vec<BoxShadow> {
    let drop = match cx.theme().is_dark() {
        true => gpui_kit::black().opacity(0.45),
        // oklch(0.3 0.03 40 / 0.16): a warm ink, not black, on paper.
        false => Hsla::from(gpui_kit::rgb(0x3E2B24)).opacity(0.16),
    };
    vec![
        BoxShadow { color: drop, offset: point(px(0.), px(12.)), blur_radius: px(32.), spread_radius: px(0.), inset: false },
        BoxShadow { color: palette::line_strong(cx), offset: point(px(0.), px(0.)), blur_radius: px(0.), spread_radius: px(1.), inset: false },
    ]
}

// MOVE(ui): mix (ui.rs keeps a private one; the forum shares this), CSS color-mix: `t` of `to` over `from`, in RGB.
pub(crate) fn mix(from: Hsla, to: Hsla, t: f32) -> Hsla {
    let (a, b) = (gpui_kit::Rgba::from(from), gpui_kit::Rgba::from(to));
    let lerp = |x: f32, y: f32| x + (y - x) * t;
    gpui_kit::Rgba { r: lerp(a.r, b.r), g: lerp(a.g, b.g), b: lerp(a.b, b.b), a: lerp(a.a, b.a) }.into()
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_media_links_are_hidden() {
        let gif = crate::discord::Media { url: "https://t/x.mp4".into(), width: 1., height: 1., kind: crate::discord::MediaKind::Image };
        let msg = |content: &str, media: bool| Message { content: content.into(), media: if media { vec![gif.clone()] } else { vec![] }, ..Default::default() };
        assert!(only_media_link(&msg("https://klipy.com/gifs/x", true)));
        assert!(!only_media_link(&msg("look https://klipy.com/gifs/x", true)));
        assert!(!only_media_link(&msg("https://example.com", false)));
    }

    #[test]
    fn jumbo_emoji_detection() {
        assert_eq!(custom_emoji("<:blob:1> <a:dance:2>").len(), 2);
        let dance = emoji_image("a:dance:2").unwrap();
        assert!(dance.contains("/emojis/2.gif") && dance.contains(r#"alt=":dance:""#) && dance.contains(r#"width="22""#));
        assert!(emoji_image(":blob:1").unwrap().contains("/emojis/1.webp"));
        assert_eq!(emoji_image(":bad:x1"), None);
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

    #[test]
    fn galleries_fill_the_column_with_four_round_corners() {
        for count in 2..=7 {
            let (height, tiles) = gallery_layout(count);
            assert_eq!(tiles.len(), count.min(5), "{count} items");
            for t in &tiles {
                assert!(t.x >= 0. && t.y >= 0. && t.x + t.w <= COLUMN + 0.01 && t.y + t.h <= height + 0.01, "{count}: {t:?} spills");
            }
            // Tiles and their gaps cover the column: nothing overlaps, nothing is left empty but the gaps.
            let area: f32 = tiles.iter().map(|t| t.w * t.h).sum();
            assert!(area < COLUMN * height && area > (COLUMN - 2. * GALLERY_GAP) * (height - GALLERY_GAP) * 0.95, "{count}: area {area}");
            let round = tiles.iter().map(|t| gallery_corners(*t, height)).flat_map(|c| [c.top_left, c.top_right, c.bottom_left, c.bottom_right]).filter(|r| *r == px(RADIUS_LG)).count();
            assert_eq!(round, 4, "{count} items");
        }
    }

    #[test]
    fn typing_reads_like_the_design() {
        let text = |names: &[&str]| typing_parts(names).into_iter().map(|(t, _)| t).collect::<String>();
        assert_eq!(text(&["Mira"]), "Mira is typing…");
        assert_eq!(text(&["Mira", "Theo"]), "Mira and Theo are typing…");
        assert_eq!(text(&["Mira", "Theo", "Ana"]), "Mira, Theo and Ana are typing…");
        assert_eq!(text(&["a", "b", "c", "d"]), "Several people are typing…");
        assert!(typing_parts(&["Mira"])[0].1);
    }

    #[test]
    fn stamps_read_like_the_design() {
        let now = Local::now();
        assert!(format_stamp(&now.to_rfc3339()).starts_with("Today at "));
        assert!(format_stamp(&(now - chrono::TimeDelta::days(1)).to_rfc3339()).starts_with("Yesterday at "));
        let old = format_stamp(&(now - chrono::TimeDelta::days(30)).to_rfc3339());
        assert!(old.contains(", 20") && old.contains(" at "), "{old}");
        assert!(!gutter_clock(&now.to_rfc3339()).contains('M'));
    }
}
