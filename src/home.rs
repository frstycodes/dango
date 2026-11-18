// The Direct Messages home: Discord's Friends page, the Friends row atop the DM list, the Inbox and the
// GIF picker actions, and reopening the last channel on launch.
use std::time::Duration;

use gpui_kit::{
    AnyElement, App, Context, Div, Entity, FontWeight, Hsla, SharedString, Stateful, Window, WindowControlArea,
    component::{Icon, input::InputEvent, tooltip::Tooltip},
    div, prelude::*, px,
};
use reqwest::Method;
use serde_json::json;

use crate::{
    Rustcord, SearchState,
    discord::{Relation, Relationship, Status},
    icons::Glyph,
    layout::avatar_with_status,
    mascot::Mood,
    palette,
    sidebar::{Nav, RowState, channel_row},
    ui::{self, Squircle, Tone},
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FriendsTab {
    Online,
    All,
    Pending,
    Blocked,
}

// One file per account ("last-channel-<id>"); the shared file from before accounts is read as a fallback.
fn last_channel_file(user_id: Option<&str>) -> Option<std::path::PathBuf> {
    let name = match user_id {
        Some(id) if !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()) => format!("last-channel-{id}"),
        Some(_) => return None,
        None => "last-channel".into(),
    };
    Some(crate::data_dir()?.join(name))
}

/// "guild_id channel_id" ("@me" for DMs), so a restart lands where you left off, as Discord does.
pub fn load_last_channel(user_id: &str) -> Option<(Option<String>, String)> {
    let text = [Some(user_id), None].into_iter().find_map(|id| std::fs::read_to_string(last_channel_file(id)?).ok())?;
    let (guild, channel) = text.trim().split_once(' ')?;
    Some(((guild != "@me").then(|| guild.to_owned()), channel.to_owned()))
}

pub fn save_last_channel(user_id: &str, guild_id: Option<&str>, channel_id: &str) {
    let Some(path) = last_channel_file(Some(user_id)) else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, format!("{} {channel_id}", guild_id.unwrap_or("@me")));
}

impl Rustcord {
    /// Called on render: once the gateway has delivered servers and DMs, reopen the saved channel.
    pub(crate) fn restore_last_channel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.guilds.is_empty() && self.dms.is_empty() {
            return;
        }
        let Some((guild_id, channel_id)) = self.restore.take() else { return };
        let channel = match &guild_id {
            Some(id) => self.guilds.iter().find(|g| &g.id == id).and_then(|g| g.channels.iter().find(|c| c.id == channel_id)).cloned(),
            None => self.dms.iter().find(|d| d.id == channel_id).cloned(),
        };
        if let Some(channel) = channel {
            self.guild_id = guild_id;
            self.open_channel(channel, window, cx);
        }
    }

    pub(crate) fn open_home(&mut self, cx: &mut Context<Self>) {
        self.guild_id = None;
        self.channel = None;
        self.call_view = false;
        cx.notify();
    }

    pub(crate) fn open_inbox(&mut self, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        self.inbox = Some(None);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = client.mentions().await;
            this.update(cx, |this, cx| {
                if this.inbox.is_some() {
                    this.inbox = Some(Some(result.map_err(Into::into)));
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn open_gifs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.gif_picker = Some(None);
        self.gif_search.update(cx, |state, cx| {
            state.set_value("", window, cx);
            state.focus(window, cx);
        });
        self.search_gifs(String::new(), cx);
    }

    pub(crate) fn on_gif_search(&mut self, state: &Entity<SearchState>, event: &InputEvent, _: &mut Window, cx: &mut Context<Self>) {
        let InputEvent::Change = event else { return };
        let query = state.read(cx).value().to_string();
        // Waits for a pause in typing, then searches if the text is still the same.
        let input = state.clone();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(300)).await;
            this.update(cx, |this, cx| {
                if input.read(cx).value() == query.as_str() {
                    this.search_gifs(query, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn search_gifs(&mut self, query: String, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        cx.spawn(async move |this, cx| {
            let result = client.gifs(query.trim()).await;
            this.update(cx, |this, cx| {
                if this.gif_picker.is_some() {
                    this.gif_picker = Some(Some(result.map_err(Into::into)));
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Sends the GIF the way Discord does: its Tenor link, which Discord embeds.
    pub(crate) fn send_gif(&mut self, url: String, window: &mut Window, cx: &mut Context<Self>) {
        self.gif_picker = None;
        self.composer.update(cx, |state, cx| state.set_value(url, window, cx));
        self.submit(window, cx);
    }

    /// The Friends row at the top of the DM list (.ch.navrow).
    pub(crate) fn friends_row(&self, cx: &mut Context<Self>) -> AnyElement {
        let selected = self.guild_id.is_none() && self.channel.is_none();
        let pending = self.relationships.iter().filter(|r| r.kind == Relation::Incoming).count() as u32;
        let state = RowState { selected, unread: false, muted: false, strong: false };
        channel_row("friends-row".into(), "friends-row".into(), state, cx)
            .h(px(40.))
            .gap_3()
            .child(Icon::new(Glyph::Users).size(px(20.)).flex_none())
            .child(div().flex_1().min_w_0().truncate().child("Friends"))
            .when(pending > 0, |r| r.child(ui::count_badge(pending)))
            .on_click(cx.listener(|this, _, _, cx| this.open_home(cx)))
            .into_any_element()
    }

    /// The Friends page: a header with the filter tabs, then the people in that tab.
    pub(crate) fn render_home(&self, cx: &mut Context<Self>) -> AnyElement {
        let tab = self.friends_tab;
        let status_of = |id: &str| self.presences.get(id).map(|p| p.status).unwrap_or_default();
        let people: Vec<Relationship> = self
            .relationships
            .iter()
            .filter(|r| match tab {
                FriendsTab::Online => r.kind == Relation::Friend && status_of(&r.user.id) != Status::Offline,
                FriendsTab::All => r.kind == Relation::Friend,
                FriendsTab::Pending => matches!(r.kind, Relation::Incoming | Relation::Outgoing),
                FriendsTab::Blocked => r.kind == Relation::Blocked,
            })
            .cloned()
            .collect();
        let pending = self.relationships.iter().filter(|r| r.kind == Relation::Incoming).count() as u32;
        let tabs = [(FriendsTab::Online, "Online"), (FriendsTab::All, "All"), (FriendsTab::Pending, "Pending"), (FriendsTab::Blocked, "Blocked")];
        let header = div()
            .h(px(crate::layout::HEADER_HEIGHT))
            .pl_4()
            .pr_3()
            .flex()
            .items_center()
            .gap_3()
            .window_control_area(WindowControlArea::Drag)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .pr_3()
                    .border_r_1()
                    .border_color(palette::line_strong(cx))
                    .whitespace_nowrap()
                    .text_size(px(16.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(palette::strong(cx))
                    .child(Icon::new(Glyph::Users).size(px(20.)).text_color(palette::faint(cx)))
                    .child("Friends"),
            )
            .child(div().flex().min_w_0().overflow_hidden().gap_1().children(tabs.map(|(this_tab, label)| {
                let count = (this_tab == FriendsTab::Pending && pending > 0).then_some(pending);
                friends_tab(this_tab, label, tab == this_tab, count, cx).on_click(cx.listener(move |this, _, _, cx| {
                    this.friends_tab = this_tab;
                    cx.notify();
                }))
            })))
            .child(div().ml_auto().child(self.header_trail(true, cx)));
        let heading = match tab {
            FriendsTab::Online => "Online",
            FriendsTab::All => "All friends",
            FriendsTab::Pending => "Pending",
            FriendsTab::Blocked => "Blocked",
        };
        let body = match (people.is_empty(), tab) {
            (true, FriendsTab::Online) => ui::empty_state("friends-empty", Mood::Idle, "No one's around right now", "Your friends are offline. Say hi anyway, they'll see it when they're back.", cx)
                .child(ui::button("show-all-friends", Tone::Secondary, cx).mt_2().child("Show all friends").on_click(cx.listener(|this, _, _, cx| {
                    this.friends_tab = FriendsTab::All;
                    cx.notify();
                })))
                .into_any_element(),
            (true, _) => ui::empty_state("friends-empty", Mood::Idle, format!("No one under {heading}"), "Friends, requests and blocks show up here as they happen.", cx).into_any_element(),
            (false, _) => {
                let label = div().pt_2().px_2().pb_1().text_size(px(12.)).font_weight(FontWeight::MEDIUM).text_color(palette::faint(cx)).child(format!("{heading} · {}", people.len()));
                let rows: Vec<AnyElement> = people.iter().map(|r| self.friend_row(r, cx)).collect();
                div().flex().flex_col().gap(px(2.)).child(label).children(rows).into_any_element()
            }
        };
        // The list runs up under the header, which blurs and fades it (ui::scroll_edge).
        let page = palette::surface(palette::Level::Page, cx);
        div()
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .bg(page)
            .text_color(palette::text(cx))
            .text_size(px(14.))
            .child(div().id("friends-list").flex_1().min_h_0().overflow_y_scroll().pt(px(crate::layout::HEADER_HEIGHT + 16.)).px_4().pb_6().child(body))
            .child(ui::scroll_edge(page, 0.).child(header))
            .into_any_element()
    }

    // A person in the list (.friend): who they are, what they're up to, and the actions that fit.
    fn friend_row(&self, relation: &Relationship, cx: &mut Context<Self>) -> AnyElement {
        let user = &relation.user;
        let presence = self.presences.get(&user.id).cloned().unwrap_or_default();
        let subtitle: String = match relation.kind {
            Relation::Incoming => "Wants to be friends".into(),
            Relation::Outgoing => "You sent a request".into(),
            Relation::Blocked => "Blocked".into(),
            Relation::Friend => presence.activity.as_ref().map(|a| a.text.clone()).unwrap_or_else(|| if presence.status == Status::Offline { "Offline".into() } else { presence.status.label().into() }),
        };
        let page = palette::surface(palette::Level::Page, cx);
        let status = (relation.kind == Relation::Friend).then_some(presence.status);
        let id = user.id.clone();
        let acts: Vec<Stateful<Div>> = match relation.kind {
            Relation::Friend => vec![
                friend_action(("message", &id), Icon::new(Glyph::ChatRound), "Message", None, cx).on_click(cx.listener({
                    let id = id.clone();
                    move |this, _, window, cx| this.message_user(id.clone(), window, cx)
                })),
                friend_action(("more", &id), Nav::MoreVertical.icon(), "More", None, cx).on_mouse_down(gpui_kit::MouseButton::Left, {
                    let user = user.clone();
                    cx.listener(move |this, e: &gpui_kit::MouseDownEvent, _, cx| this.open_user_menu(user.clone(), None, crate::user_menu::Surface::Generic, e.position, cx))
                }),
            ],
            Relation::Incoming => vec![
                friend_action(("accept", &id), Icon::new(Glyph::Checkmark), "Accept", Some(palette::positive_ink(cx)), cx).on_click(cx.listener({
                    let user = user.clone();
                    move |this, _, _, cx| this.relation_optimistic(&user.id, Some((user.clone(), Relation::Friend)), Method::PUT, Some(json!({})), cx)
                })),
                friend_action(("ignore", &id), Nav::Close.icon(), "Ignore", Some(palette::danger_ink(cx)), cx).on_click(cx.listener({
                    let id = id.clone();
                    move |this, _, _, cx| this.relation_optimistic(&id, None, Method::DELETE, None, cx)
                })),
            ],
            Relation::Outgoing => vec![friend_action(("cancel", &id), Nav::Close.icon(), "Cancel request", Some(palette::danger_ink(cx)), cx).on_click(cx.listener({
                let id = id.clone();
                move |this, _, _, cx| this.relation_optimistic(&id, None, Method::DELETE, None, cx)
            }))],
            Relation::Blocked => vec![friend_action(("unblock", &id), Nav::Close.icon(), "Unblock", None, cx).on_click(cx.listener({
                let id = id.clone();
                move |this, _, _, cx| this.relation_optimistic(&id, None, Method::DELETE, None, cx)
            }))],
        };
        let hover = palette::hover(cx);
        div()
            .id(SharedString::from(format!("friend-{}", user.id)))
            .min_h(px(60.))
            .px_2()
            .flex()
            .items_center()
            .gap_3()
            .squircle(ui::RADIUS_MD)
            .hover(move |s| s.bg(hover))
            .child(avatar_with_status(&user.avatar, 32., status, page))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(div().truncate().font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(user.name.clone()))
                    .child(div().truncate().text_size(px(12.)).text_color(palette::muted(cx)).child(subtitle)),
            )
            .child(div().flex_none().flex().gap_2().children(acts))
            .on_mouse_down(gpui_kit::MouseButton::Right, {
                let user = user.clone();
                cx.listener(move |this, e: &gpui_kit::MouseDownEvent, _, cx| this.open_user_menu(user.clone(), None, crate::user_menu::Surface::Generic, e.position, cx))
            })
            .into_any_element()
    }
}

// A filter tab (.pills .tab): selected takes the fill, the rest hover to a lighter one.
fn friends_tab(tab: FriendsTab, label: &'static str, selected: bool, count: Option<u32>, cx: &App) -> Stateful<Div> {
    let (hover, strong) = (palette::hover(cx), palette::strong(cx));
    div()
        .id(("friends-tab", tab as usize))
        .occlude()
        .h(px(30.))
        .px_3()
        .squircle(ui::RADIUS_SM)
        .flex()
        .items_center()
        .gap(px(6.))
        .cursor_pointer()
        .whitespace_nowrap()
        .font_weight(FontWeight::MEDIUM)
        .text_color(if selected { strong } else { palette::muted(cx) })
        .when(selected, |t| t.bg(palette::selected(cx)))
        .when(!selected, |t| t.hover(move |s| s.bg(hover).text_color(strong)))
        .child(label)
        .children(count.map(ui::count_badge))
}

// A friend row's action (.friend .acts .iconbtn): a raised 36px tile with a hairline; `tone` is the
// ink it hovers to (accept green, ignore coral).
fn friend_action(id: (&'static str, &str), icon: Icon, tooltip: &'static str, tone: Option<Hsla>, cx: &App) -> Stateful<Div> {
    let ink = tone.unwrap_or(palette::strong(cx));
    div()
        .id(SharedString::from(format!("{}-{}", id.0, id.1)))
        .size(px(36.))
        .flex_none()
        .squircle(ui::RADIUS_MD)
        .border_1()
        .border_color(palette::line(cx))
        .bg(palette::surface(palette::Level::Raised, cx))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .text_color(palette::muted(cx))
        .hover(move |s| s.text_color(ink))
        .child(icon.size(px(20.)))
        .tooltip(move |window, cx| Tooltip::new(tooltip).build(window, cx))
}
