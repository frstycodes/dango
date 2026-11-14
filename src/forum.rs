// Discord's forum and media channels: a list of posts (each a thread), newest activity first.
// Active posts arrive over the gateway once the channel is subscribed with threads on; older ones
// come from THREAD_SEARCH (archived, by last message time, 25 at a time); each card's preview is
// the post's first message from FORUM_POSTS, fetched ten at a time as Discord does.
use std::collections::HashMap;

use gpui_kit::{
    AnyElement, Context, FontWeight, Hsla, SharedString, Window,
    component::{ActiveTheme, Icon},
    div, img, prelude::*, px, rgb,
};
use reqwest::Method;
use serde_json::{Value, json};

use crate::{
    Rustcord,
    discord::{Channel, Message, parse_message, parse_thread},
    icons::Glyph,
    palette,
};

#[derive(Default)]
pub struct ForumState {
    archived: Vec<Channel>,
    previews: HashMap<String, Message>,
    requested: std::collections::HashSet<String>,
    has_more: bool,
    loading: bool,
    error: Option<SharedString>,
}

impl Rustcord {
    // The forum's posts: active ones we know of, then archived ones, newest activity first.
    fn forum_posts(&self, forum_id: &str) -> Vec<Channel> {
        let mut posts: Vec<Channel> = self.guilds.iter().flat_map(|g| g.threads.iter()).filter(|t| t.parent_id.as_deref() == Some(forum_id)).cloned().collect();
        if let Some(state) = self.forums.get(forum_id) {
            posts.extend(state.archived.iter().filter(|a| !posts.iter().any(|p| p.id == a.id)).cloned().collect::<Vec<_>>());
        }
        posts.sort_by_key(|p| std::cmp::Reverse(p.last_message_id.as_deref().and_then(|id| id.parse::<u64>().ok()).unwrap_or(0)));
        posts
    }

    pub(crate) fn load_forum(&mut self, forum_id: String, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        let state = self.forums.entry(forum_id.clone()).or_default();
        if state.loading {
            return;
        }
        state.loading = true;
        let offset = state.archived.len();
        let guild_id = self.guild_id.clone().unwrap_or_default();
        cx.spawn(async move |this, cx| {
            let path = format!("/channels/{forum_id}/threads/search?archived=true&sort_by=last_message_time&sort_order=desc&limit=25&offset={offset}");
            let result = client.call(Method::GET, path, None).await;
            this.update(cx, |this, cx| {
                let state = this.forums.entry(forum_id.clone()).or_default();
                state.loading = false;
                match result {
                    Ok(body) => {
                        state.archived.extend(body["threads"].as_array().into_iter().flatten().filter_map(|t| parse_thread(t, &guild_id)));
                        state.has_more = body["has_more"].as_bool().unwrap_or(false);
                        for m in body["first_messages"].as_array().into_iter().flatten().filter_map(parse_message) {
                            state.previews.insert(m.channel_id.clone(), m);
                        }
                        state.error = None;
                    }
                    Err(error) => state.error = Some(error.into()),
                }
                this.load_forum_previews(&forum_id, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    // First messages for posts that don't have one yet, ten per request.
    fn load_forum_previews(&mut self, forum_id: &str, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        let posts = self.forum_posts(forum_id);
        let state = self.forums.entry(forum_id.to_owned()).or_default();
        let wanted: Vec<String> = posts.iter().map(|p| p.id.clone()).filter(|id| !state.previews.contains_key(id) && !state.requested.contains(id)).collect();
        for batch in wanted.chunks(10) {
            state.requested.extend(batch.iter().cloned());
            let (client, forum, ids) = (client.clone(), forum_id.to_owned(), batch.to_vec());
            cx.spawn(async move |this, cx| {
                let Ok(body) = client.call(Method::POST, format!("/channels/{forum}/post-data"), Some(json!({ "thread_ids": ids }))).await else { return };
                this.update(cx, |this, cx| {
                    let state = this.forums.entry(forum.clone()).or_default();
                    for (id, post) in body["threads"].as_object().into_iter().flatten() {
                        if let Some(first) = parse_message(&post["first_message"]) {
                            state.previews.insert(id.clone(), first);
                        }
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
    }

    // Discord's New Post: a title, then the opening message.
    fn new_post(&mut self, forum_id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.prompt("New Post", "Post title", "", "Next", move |this, title, window, cx| {
            if title.trim().is_empty() {
                return;
            }
            let (forum, title) = (forum_id.clone(), title.trim().to_owned());
            this.prompt("New Post", format!("Opening message for \u{201c}{title}\u{201d}"), "", "Post", move |this, content, window, cx| {
                let Some(client) = this.client.clone() else { return };
                let (forum, name) = (forum.clone(), title.clone());
                let guild_id = this.guild_id.clone().unwrap_or_default();
                cx.spawn_in(window, async move |this, cx| {
                    let body: Value = json!({ "name": name, "message": { "content": content } });
                    let created = client.call(Method::POST, format!("/channels/{forum}/threads"), Some(body)).await;
                    this.update_in(cx, |this, window, cx| match created.ok().and_then(|t| parse_thread(&t, &guild_id)) {
                        Some(post) => {
                            this.on_event(crate::discord::Event::ChannelUpsert { guild_id: guild_id.clone(), channel: post.clone() }, cx);
                            this.open_channel(post, window, cx);
                        }
                        None => this.show_toast("Couldn't create the post", cx),
                    })
                    .ok();
                })
                .detach();
            }, window, cx);
        }, window, cx);
    }

    pub(crate) fn render_forum(&mut self, forum: Channel, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let posts = self.forum_posts(&forum.id);
        let state = self.forums.get(&forum.id);
        let header = div()
            .h(px(48.))
            .flex_none()
            .px_4()
            .flex()
            .items_center()
            .gap_2()
            .border_b_1()
            .border_color(theme.sidebar_border)
            .child(Icon::new(Glyph::Forum).text_color(theme.muted_foreground))
            .child(div().flex_none().font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(forum.name.clone()))
            .children(forum.topic.clone().map(|t| div().flex_1().min_w_0().truncate().text_size(px(14.)).text_color(theme.muted_foreground).child(t)))
            .when(forum.topic.is_none(), |h| h.child(div().flex_1()));
        let forum_id = forum.id.clone();
        let new_post = div()
            .id("forum-new-post")
            .h(px(36.))
            .px_4()
            .rounded(px(8.))
            .flex()
            .items_center()
            .gap_2()
            .cursor_pointer()
            .bg(palette::blurple())
            .text_color(gpui_kit::white())
            .font_weight(FontWeight::MEDIUM)
            .hover(|s| s.opacity(0.9))
            .child(Icon::new(Glyph::Plus).size(px(18.)))
            .child("New Post")
            .on_click(cx.listener(move |this, _, window, cx| this.new_post(forum_id.clone(), window, cx)));
        let cards: Vec<AnyElement> = posts.iter().map(|post| self.post_card(post, state.and_then(|s| s.previews.get(&post.id)), cx)).collect();
        let empty = posts.is_empty() && state.is_some_and(|s| !s.loading);
        let more = state.filter(|s| s.has_more && !s.loading).map(|_| {
            let id = forum.id.clone();
            div()
                .id("forum-more")
                .py_3()
                .flex()
                .justify_center()
                .cursor_pointer()
                .text_color(theme.link)
                .child("Load more posts")
                .on_click(cx.listener(move |this, _, _, cx| this.load_forum(id.clone(), cx)))
        });
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .child(header)
            .child(
                div()
                    .id("forum-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(
                        div()
                            .max_w(px(900.))
                            .mx_auto()
                            .px_4()
                            .py_4()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(div().flex().justify_end().pb_2().child(new_post))
                            .children(state.and_then(|s| s.error.clone()).map(|e| div().text_color(theme.danger).child(e)))
                            .when(empty, |d| d.child(div().py_8().flex().justify_center().text_color(theme.muted_foreground).child("No posts yet. Start one!")))
                            .children(cards)
                            .children(more),
                    ),
            )
            .into_any_element()
    }

    // A post card: title, the opening message (author and text), its image, and reply count.
    fn post_card(&self, post: &Channel, first: Option<&Message>, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let replies = post.thread.as_ref().map_or(0, |t| t.message_count);
        let thumb = first.and_then(|m| m.media.first()).map(|m| match &m.kind {
            crate::discord::MediaKind::Image => m.url.clone(),
            crate::discord::MediaKind::Video { poster } | crate::discord::MediaKind::Gif { poster } => poster.clone(),
        });
        let preview = first.map(|m| {
            div()
                .flex()
                .gap_1()
                .text_size(px(14.))
                .child(div().flex_none().font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(format!("{}:", m.author.name)))
                .child(div().min_w_0().line_clamp(2).text_color(theme.muted_foreground).child(m.content.clone()))
        });
        let open = post.clone();
        let (menu_post, guild_id) = (post.clone(), self.guild_id.clone().unwrap_or_default());
        div()
            .id(SharedString::from(format!("post-{}", post.id)))
            .p_4()
            .rounded(px(8.))
            .border_1()
            .border_color(Hsla::from(rgb(0x97979F)).opacity(0.2))
            .bg(if theme.is_dark() { rgb(0x0A0A0C).into() } else { theme.muted })
            .flex()
            .gap_4()
            .cursor_pointer()
            .hover(|s| s.bg(Hsla::from(rgb(0x97979F)).opacity(0.08)))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(div().truncate().text_size(px(16.)).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(post.name.clone()))
                    .children(preview)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .text_size(px(14.))
                            .text_color(theme.muted_foreground)
                            .child(Icon::new(Glyph::ChatRound).size(px(16.)))
                            .child(replies.to_string()),
                    ),
            )
            .children(thumb.map(|url| img(url).size(px(88.)).flex_none().rounded(px(8.)).object_fit(gpui_kit::ObjectFit::Cover)))
            .on_click(cx.listener(move |this, _, window, cx| this.open_channel(open.clone(), window, cx)))
            .on_mouse_down(gpui_kit::MouseButton::Right, cx.listener(move |this, e: &gpui_kit::MouseDownEvent, _, cx| this.open_thread_menu(menu_post.clone(), guild_id.clone(), e.position, cx)))
            .into_any_element()
    }
}
