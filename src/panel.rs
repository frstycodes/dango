// The right-hand panel: the member list, or search results.
use gpui_kit::{
    AnyElement, Context, FontWeight, SharedString, Window,
    component::{ActiveTheme, Icon},
    div, img, prelude::*, px,
};


use crate::{
    Rustcord, Edge, RightPanel,
    discord::{ListItem, Member, Message, Status},
    icons::Glyph,
    layout::{decorated_avatar, nameplate},
};

impl Rustcord {
    pub fn render_panel(&mut self, _: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        self.guild_id.as_ref()?;
        let theme = cx.theme().clone();
        let body = match &self.right_panel {
            RightPanel::Hidden => return None,
            RightPanel::Members => self.member_list(cx),
            RightPanel::Search { query, results } => self.search_results(query.clone(), results.clone(), cx),
        };
        Some(
            div()
                .relative()
                .w(px(self.panel_width))
                .h_full()
                .flex_none()
                .border_l_1()
                .border_color(theme.sidebar_border)
                .bg(theme.sidebar)
                .child(self.resize_handle(Edge::Panel, cx))
                .child(div().id("right-panel").size_full().overflow_y_scroll().px_2().pb_4().child(body))
                .into_any_element(),
        )
    }

    fn member_list(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let Some(guild) = self.guild() else { return div().into_any_element() };
        let list = self.member_lists.get(&guild.id);
        let Some(list) = list.filter(|l| !l.is_empty()) else {
            return div().pt_4().px_2().text_color(theme.muted_foreground).child("Loading members…").into_any_element();
        };
        let rows = list.iter().map(|item| match item {
            ListItem::Group { id, count } => {
                let count = self.member_groups.get(&guild.id).and_then(|g| g.get(id)).copied().unwrap_or(*count);
                let name = match id.as_str() {
                    "online" => "Online".to_string(),
                    "offline" => "Offline".to_string(),
                    "" => return div().into_any_element(),
                    role => guild.roles.iter().find(|r| r.id == role).map(|r| r.name.clone()).unwrap_or_else(|| "Members".into()),
                };
                div()
                    .pt_5()
                    .pb_1()
                    .px_2()
                    .text_size(px(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.muted_foreground)
                    .child(format!("{name} — {count}"))
                    .into_any_element()
            }
            ListItem::Member(member) => self.member_row(member, cx),
        });
        div().flex().flex_col().gap(px(2.)).children(rows).into_any_element()
    }

    fn member_row(&self, member: &Member, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let offline = member.presence.status == Status::Offline;
        let color = self.role_color(self.guild_id.as_ref(), &member.user.id, Some(&member.roles)).unwrap_or(theme.foreground);
        let surface = theme.sidebar;
        let nameplate = member.user.nameplate.as_ref().map(|plate| nameplate(plate, surface, true, theme.is_dark()));
        let owner = self.guild().is_some_and(|g| g.owner_id.as_ref() == Some(&member.user.id));
        let crown = theme.warning;
        div()
            .id(SharedString::from(format!("member-{}", member.user.id)))
            .relative()
            .h(px(44.))
            .px_2()
            .flex()
            .items_center()
            .gap_3()
            .rounded(px(8.))
            .when(offline, |r| r.opacity(0.4))
            .cursor_pointer()
            .hover(|s| s.bg(theme.sidebar_accent.opacity(0.6)).opacity(1.))
            .children(nameplate)
            .child(decorated_avatar(&member.user.avatar, member.user.decoration.as_deref(), 32., (!offline).then_some(member.presence.status), surface))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(div().truncate().font_weight(FontWeight::MEDIUM).text_color(color).child(member.user.name.clone()))
                            .when(owner, |r| r.child(Icon::new(Glyph::Crown).size(px(14.)).flex_none().ml(px(3.)).text_color(crown)))
                            .when(member.user.bot, |r| {
                                r.child(
                                    div().h(px(15.)).px_1().flex().items_center().rounded(px(4.)).bg(crate::palette::blurple()).text_color(gpui_kit::white()).text_size(px(10.)).font_weight(FontWeight::BOLD).child("APP"),
                                )
                            }),
                    )
                    .children(member.presence.activity.clone().map(|a| activity_line(&a, theme.muted_foreground))),
            )
            .on_click({
                let user = member.user.clone();
                cx.listener(move |this, event: &gpui_kit::ClickEvent, _, cx| this.open_profile(user.clone(), event.position() - gpui_kit::point(px(320.), px(0.)), cx))
            })
            .on_mouse_down(gpui_kit::MouseButton::Right, {
                let (user, guild_id) = (member.user.clone(), self.guild_id.clone());
                cx.listener(move |this, e: &gpui_kit::MouseDownEvent, _, cx| this.open_user_menu(user.clone(), guild_id.clone(), crate::user_menu::Surface::ServerChat, e.position, cx))
            })
            .into_any_element()
    }

    fn search_results(&self, query: SharedString, results: Option<Result<Vec<Message>, SharedString>>, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let header = div()
            .pt_3()
            .pb_2()
            .px_2()
            .flex()
            .items_center()
            .gap_2()
            .child(div().flex_1().truncate().font_weight(FontWeight::SEMIBOLD).child(format!("Results for “{query}”")))
            .child(
                div()
                    .id("close-search")
                    .size(px(24.))
                    .rounded(px(6.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .text_color(theme.muted_foreground)
                    .hover(|s| s.text_color(theme.foreground))
                    .child(Icon::new(Glyph::X).size(px(16.)))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.right_panel = RightPanel::Members;
                        cx.notify();
                    })),
            );
        let body = match results {
            None => div().px_2().text_color(theme.muted_foreground).child("Searching…").into_any_element(),
            Some(Err(error)) => div().px_2().text_color(theme.danger).child(error).into_any_element(),
            Some(Ok(hits)) if hits.is_empty() => div().px_2().text_color(theme.muted_foreground).child("No results").into_any_element(),
            Some(Ok(hits)) => div()
                .flex()
                .flex_col()
                .gap_2()
                .children(hits.into_iter().map(|hit| {
                    let channel = self.guild().and_then(|g| g.channels.iter().find(|c| c.id == hit.channel_id).cloned());
                    let (target, message_id) = (channel.clone(), hit.id.clone());
                    div()
                        .id(SharedString::from(format!("hit-{}", hit.id)))
                        .p_2()
                        .rounded(px(8.))
                        .bg(theme.background)
                        .cursor_pointer()
                        .hover(|s| s.bg(theme.sidebar_accent))
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(div().text_size(px(12.)).text_color(theme.muted_foreground).child(format!("#{}", channel.map(|c| c.name).unwrap_or_default())))
                        .child(div().flex().items_center().gap_2().child(img(hit.author.avatar.clone()).size(px(20.)).rounded_full()).child(div().font_weight(FontWeight::SEMIBOLD).child(hit.author.name.clone())))
                        .child(div().text_size(px(13.)).line_clamp(3).child(hit.content.clone()))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            let Some(channel) = target.clone() else { return };
                            if this.channel.as_ref().is_none_or(|c| c.id != channel.id) {
                                this.open_channel(channel, window, cx);
                            }
                            this.jump_to(message_id.clone(), cx);
                        }))
                }))
                .into_any_element(),
        };
        div().flex().flex_col().child(header).child(body).into_any_element()
    }
}

// Discord's line under a member: a kind icon (controller, music, screen, TV), the name or status, "+N".
fn activity_line(activity: &crate::discord::Activity, color: gpui_kit::Hsla) -> AnyElement {
    use crate::discord::ActivityKind::*;
    let glyph = match activity.kind {
        Playing | Competing => Some(Glyph::Game),
        Listening => Some(Glyph::Music),
        Streaming => Some(Glyph::Stream),
        Watching => Some(Glyph::Tv),
        Custom => None,
    };
    let text = if activity.more > 0 { format!("{} +{}", activity.text, activity.more) } else { activity.text.clone() };
    div()
        .flex()
        .items_center()
        .gap_1()
        .min_w_0()
        .text_size(px(12.))
        .text_color(color)
        .children(glyph.map(|g| Icon::new(g).size(px(14.)).flex_none()))
        .child(div().truncate().child(text))
        .into_any_element()
}
