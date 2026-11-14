// The Direct Messages home: Discord's Friends page, the Friends row atop the DM list, the Inbox and the
// GIF picker actions, and reopening the last channel on launch.
use std::time::Duration;

use gpui_kit::{
    AnyElement, Context, Entity, FontWeight, SharedString, Window, WindowControlArea,
    component::{
        ActiveTheme, Icon,
        input::InputEvent,
        tooltip::Tooltip,
    },
    div, prelude::*, px,
};

use crate::{
    Rustcord, SearchState,
    discord::{Relation, Status},
    icons::Glyph,
    layout::avatar_with_status,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FriendsTab {
    Online,
    All,
    Pending,
}

fn last_channel_file() -> Option<std::path::PathBuf> {
    Some(crate::data_dir()?.join("last-channel"))
}

/// "guild_id channel_id" ("@me" for DMs), so a restart lands where you left off, as Discord does.
pub fn load_last_channel() -> Option<(Option<String>, String)> {
    let text = std::fs::read_to_string(last_channel_file()?).ok()?;
    let (guild, channel) = text.trim().split_once(' ')?;
    Some(((guild != "@me").then(|| guild.to_owned()), channel.to_owned()))
}

pub fn save_last_channel(guild_id: Option<&str>, channel_id: &str) {
    let Some(path) = last_channel_file() else { return };
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

    /// The Friends row at the top of the DM list.
    pub(crate) fn friends_row(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let selected = self.guild_id.is_none() && self.channel.is_none();
        let pending = self.relationships.iter().filter(|r| r.kind == Relation::Incoming).count();
        div()
            .id("friends-row")
            .h(px(42.))
            .px_2()
            .flex()
            .items_center()
            .gap_3()
            .rounded(px(8.))
            .cursor_pointer()
            .font_weight(FontWeight::MEDIUM)
            .text_color(if selected { crate::palette::strong(cx) } else { theme.muted_foreground })
            .when(selected, |r| r.bg(theme.sidebar_accent))
            .hover(|s| s.bg(theme.sidebar_accent.opacity(0.6)).text_color(theme.foreground))
            .child(Icon::new(Glyph::Friends).size(px(24.)))
            .child(div().flex_1().child("Friends"))
            .when(pending > 0, |r| {
                r.child(div().min_w(px(16.)).h(px(16.)).px_1().rounded_full().bg(theme.danger).text_color(gpui_kit::white()).text_size(px(11.)).font_weight(FontWeight::BOLD).flex().items_center().justify_center().child(pending.to_string()))
            })
            .on_click(cx.listener(|this, _, _, cx| this.open_home(cx)))
            .into_any_element()
    }

    /// Discord's Friends page: a header with Online / All / Pending, then the people in that tab.
    pub(crate) fn render_home(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let tab = self.friends_tab;
        let status_of = |id: &str| self.presences.get(id).map(|p| p.status).unwrap_or_default();
        let people: Vec<_> = self
            .relationships
            .iter()
            .filter(|r| match tab {
                FriendsTab::Online => r.kind == Relation::Friend && status_of(&r.user.id) != Status::Offline,
                FriendsTab::All => r.kind == Relation::Friend,
                FriendsTab::Pending => matches!(r.kind, Relation::Incoming | Relation::Outgoing),
            })
            .cloned()
            .collect();
        let heading = match tab {
            FriendsTab::Online => "Online",
            FriendsTab::All => "All Friends",
            FriendsTab::Pending => "Pending",
        };
        let tab_button = |id: &'static str, label: &'static str, this_tab: FriendsTab| {
            let active = tab == this_tab;
            div()
                .id(id)
                .h(px(28.))
                .px_2()
                .rounded(px(6.))
                .flex()
                .items_center()
                .cursor_pointer()
                .font_weight(FontWeight::MEDIUM)
                .text_color(if active { crate::palette::strong(cx) } else { theme.muted_foreground })
                .when(active, |b| b.bg(theme.sidebar_accent))
                .hover(|s| s.bg(theme.sidebar_accent.opacity(0.6)).text_color(theme.foreground))
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.friends_tab = this_tab;
                    cx.notify();
                }))
        };
        let header = div()
            .h(px(48.))
            .flex_none()
            .px_4()
            .flex()
            .items_center()
            .gap_4()
            .border_b_1()
            .border_color(theme.border)
            .window_control_area(WindowControlArea::Drag)
            .child(div().flex().items_center().gap_2().font_weight(FontWeight::MEDIUM).text_color(crate::palette::strong(cx)).child(Icon::new(Glyph::Friends).size(px(24.)).text_color(theme.muted_foreground)).child("Friends"))
            .child(div().size(px(4.)).rounded_full().bg(theme.muted_foreground.opacity(0.5)))
            .child(tab_button("tab-online", "Online", FriendsTab::Online))
            .child(tab_button("tab-all", "All", FriendsTab::All))
            .child(tab_button("tab-pending", "Pending", FriendsTab::Pending));
        let rows = people.into_iter().map(|r| {
            let presence = self.presences.get(&r.user.id).cloned().unwrap_or_default();
            let subtitle = match r.kind {
                Relation::Incoming => "Incoming Friend Request".to_string(),
                Relation::Outgoing => "Outgoing Friend Request".to_string(),
                _ => presence.activity.as_ref().map(|a| a.text.clone()).unwrap_or_else(|| if presence.status == Status::Offline { "Offline".into() } else { presence.status.label().into() }),
            };
            let user_id = r.user.id.clone();
            div()
                .id(SharedString::from(format!("friend-{}", r.user.id)))
                .h(px(62.))
                .mx_4()
                .px_2()
                .flex()
                .items_center()
                .gap_3()
                .border_t_1()
                .border_color(theme.border)
                .rounded(px(8.))
                .cursor_pointer()
                .hover(|s| s.bg(theme.list_hover))
                .child(avatar_with_status(&r.user.avatar, 32., Some(presence.status), theme.background))
                .child(div().flex_1().min_w_0().flex().flex_col().child(div().truncate().font_weight(FontWeight::SEMIBOLD).text_color(crate::palette::strong(cx)).child(r.user.name.clone())).child(div().truncate().text_size(px(14.)).text_color(theme.muted_foreground).child(subtitle)))
                .child(
                    div()
                        .size(px(36.))
                        .rounded_full()
                        .bg(theme.muted)
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(theme.muted_foreground)
                        .child(Icon::new(Glyph::ChatRound).size(px(20.))),
                )
                .tooltip(|window, cx| Tooltip::new("Message").build(window, cx))
                .on_click(cx.listener(move |this, _, window, cx| this.message_user(user_id.clone(), window, cx)))
        });
        let rows: Vec<_> = rows.collect();
        let count = rows.len();
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .bg(theme.background)
            .border_l_1()
            .border_color(theme.sidebar_border)
            .child(header)
            .child(
                div()
                    .id("friends-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .pt_4()
                    .child(div().px_6().pb_2().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child(format!("{heading} — {count}")))
                    .children(rows),
            )
            .into_any_element()
    }
}
