// The side panel (design/css/panels.css .panel): one right-hand frame for the member list and search
// results, with a fixed head and a scrolling body.
use gpui_kit::{
    AnyElement, App, Context, Div, FontWeight, SharedString, Window,
    component::{ActiveTheme, Icon},
    div, prelude::*, px,
};

use crate::{
    Rustcord, Edge, RightPanel,
    discord::{ListItem, Member, Message, Status},
    icons::Glyph,
    layout::{decorated_avatar, nameplate},
    palette,
    sidebar::{activity_glyph, mix},
    ui::{self, Squircle},
};

impl Rustcord {
    pub fn render_panel(&mut self, _: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        self.guild_id.as_ref()?;
        let (title, body): (SharedString, AnyElement) = match &self.right_panel {
            RightPanel::Hidden => return None,
            RightPanel::Members => ("Members".into(), self.member_list(cx)),
            RightPanel::Search { query, results } => (format!("Results for “{query}”").into(), self.search_results(results.clone(), cx)),
        };
        let searching = matches!(self.right_panel, RightPanel::Search { .. });
        let close = ui::icon_button("close-panel", Glyph::X, if searching { "Back to members" } else { "Close" }, cx).on_click(cx.listener(move |this, _, _, cx| {
            this.right_panel = if searching { RightPanel::Members } else { RightPanel::Hidden };
            cx.notify();
        }));
        Some(
            div()
                .relative()
                .w(px(self.panel_width))
                .h_full()
                .flex_none()
                .flex()
                .flex_col()
                .bg(palette::surface(palette::Level::Sidebar, cx))
                .text_color(palette::text(cx))
                .text_size(px(14.))
                .child(self.resize_handle(Edge::Panel, cx))
                .child(
                    div()
                        .h(px(48.))
                        .flex_none()
                        .pl_4()
                        .pr_2()
                        .flex()
                        .items_center()
                        .gap_2()
                        .border_b_1()
                        .border_color(palette::line(cx))
                        .text_color(palette::strong(cx))
                        .child(div().flex_1().min_w_0().truncate().text_size(px(16.)).font_weight(FontWeight::BOLD).child(title))
                        .child(close),
                )
                .child(div().id("right-panel").flex_1().min_h_0().overflow_y_scroll().p_2().flex().flex_col().gap(px(2.)).child(body))
                .into_any_element(),
        )
    }

    // Who is here, grouped by top role, then online and offline (navigation.css .members).
    fn member_list(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(guild) = self.guild() else { return div().into_any_element() };
        let Some(list) = self.member_lists.get(&guild.id).filter(|l| !l.is_empty()) else { return members_loading(cx).into_any_element() };
        let mut first = true;
        let rows = list.iter().filter_map(|item| match item {
            ListItem::Group { id, count } => {
                let count = self.member_groups.get(&guild.id).and_then(|g| g.get(id)).copied().unwrap_or(*count);
                let name = match id.as_str() {
                    "online" => "Online".to_string(),
                    "offline" => "Offline".to_string(),
                    "" => return None,
                    role => guild.roles.iter().find(|r| r.id == role).map(|r| r.name.clone()).unwrap_or_else(|| "Members".into()),
                };
                let spaced = !std::mem::take(&mut first);
                Some(group_label(format!("{name} · {count}"), cx).when(spaced, |l| l.mt_3()).into_any_element())
            }
            ListItem::Member(member) => Some(self.member_row(member, cx)),
        });
        div().flex().flex_col().gap(px(2.)).children(rows.collect::<Vec<_>>()).into_any_element()
    }

    // A member (.mem): avatar with status, the name in their top role's colour, one line of activity.
    // Offline dims the avatar and drops the name to faint; the open profile's member keeps the fill.
    fn member_row(&self, member: &Member, cx: &mut Context<Self>) -> AnyElement {
        let dark = cx.theme().is_dark();
        let offline = member.presence.status == Status::Offline;
        let (strong, faint, hover) = (palette::strong(cx), palette::faint(cx), palette::hover(cx));
        let name_ink = match offline {
            true => faint,
            false => self.role_color(self.guild_id.as_ref(), &member.user.id, Some(&member.roles), dark).unwrap_or(strong),
        };
        let surface = palette::surface(palette::Level::Sidebar, cx);
        let plate = member.user.nameplate.as_ref().map(|plate| nameplate(plate, surface, true, dark));
        let owner = self.guild().is_some_and(|g| g.owner_id.as_ref() == Some(&member.user.id));
        let selected = self.profile_card.as_ref().is_some_and(|p| p.user.id == member.user.id);
        let crown = mix(palette::status_in(Status::Idle, cx), strong, 0.4);
        div()
            .id(SharedString::from(format!("member-{}", member.user.id)))
            .relative()
            .h(px(44.))
            .flex_none()
            .px_2()
            .flex()
            .items_center()
            .gap(px(10.))
            .squircle(ui::RADIUS_SM)
            .cursor_pointer()
            .when(selected, |r| r.bg(palette::selected(cx)))
            .when(!selected, |r| r.hover(move |s| s.bg(hover)))
            .children(plate)
            .child(div().when(offline, |a| a.opacity(0.5)).child(decorated_avatar(&member.user.avatar, member.user.decoration.as_deref(), 32., Some(member.presence.status), surface)))
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
                            .gap(px(6.))
                            .child(div().min_w_0().truncate().font_weight(FontWeight::MEDIUM).text_color(name_ink).child(member.user.name.clone()))
                            .when(owner, |r| r.child(Icon::new(Glyph::Crown).size(px(14.)).flex_none().text_color(crown)))
                            .when(member.user.bot, |r| r.child(ui::tag("App", cx))),
                    )
                    .children(member.presence.activity.as_ref().filter(|_| !offline).map(|a| activity_line(a, cx))),
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

    fn search_results(&self, results: Option<Result<Vec<Message>, SharedString>>, cx: &mut Context<Self>) -> AnyElement {
        let muted = palette::muted(cx);
        match results {
            None => wait_line("search-wait", "Searching…", cx).into_any_element(),
            Some(Err(error)) => div().px_2().text_color(palette::danger_ink(cx)).child(error).into_any_element(),
            Some(Ok(hits)) if hits.is_empty() => div().px_2().py_2().text_color(muted).child("No results").into_any_element(),
            Some(Ok(hits)) => {
                let (hover, page, faint, strong) = (palette::hover(cx), palette::surface(palette::Level::Page, cx), palette::faint(cx), palette::strong(cx));
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .children(hits.into_iter().map(|hit| {
                        let channel = self.guild().and_then(|g| g.channels.iter().find(|c| c.id == hit.channel_id).cloned());
                        let (target, message_id) = (channel.clone(), hit.id.clone());
                        div()
                            .id(SharedString::from(format!("hit-{}", hit.id)))
                            .p_2()
                            .squircle(ui::RADIUS_MD)
                            .bg(page)
                            .cursor_pointer()
                            .hover(move |s| s.bg(hover))
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(div().text_size(px(12.)).text_color(faint).child(format!("#{}", channel.map(|c| c.name).unwrap_or_default())))
                            .child(div().flex().items_center().gap_2().child(ui::avatar(hit.author.avatar.clone(), 20.)).child(div().font_weight(FontWeight::SEMIBOLD).text_color(strong).child(hit.author.name.clone())))
                            .child(div().line_clamp(3).child(hit.content.clone()))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                let Some(channel) = target.clone() else { return };
                                if this.channel.as_ref().is_none_or(|c| c.id != channel.id) {
                                    this.open_channel(channel, window, cx);
                                }
                                this.jump_to(message_id.clone(), cx);
                            }))
                    }))
                    .into_any_element()
            }
        }
    }
}

// A group heading (.cat): "Online · 4".
fn group_label(text: impl Into<SharedString>, cx: &App) -> Div {
    div().h(px(28.)).flex_none().px(px(6.)).flex().items_center().whitespace_nowrap().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(palette::faint(cx)).child(text.into())
}

// A wait names what it waits on, beside the mini wave (.pwait).
fn wait_line(id: &'static str, text: &'static str, cx: &App) -> Div {
    div().min_h(px(32.)).px_2().flex().items_center().gap_2().text_color(palette::muted(cx)).font_weight(FontWeight::MEDIUM).child(ui::bars(id, palette::muted(cx), 12.)).child(text)
}

// The list before Discord sends it: the static label, no false count, rows shaped like the real ones.
fn members_loading(cx: &App) -> Div {
    let fill = palette::selected(cx);
    let bar = move |w: f32, h: f32| div().w(gpui_kit::relative(w)).h(px(h)).squircle(ui::RADIUS_XS).bg(fill);
    // GAP: the design sweeps one shimmer band across all skeletons; these hold still.
    let row = move |name: f32, line: Option<f32>| {
        div().h(px(44.)).px_2().flex().items_center().gap(px(10.)).child(div().size(px(32.)).flex_none().rounded_full().bg(fill)).child(div().flex_1().flex().flex_col().gap(px(6.)).child(bar(name, 12.)).children(line.map(|w| bar(w, 8.))))
    };
    div()
        .flex()
        .flex_col()
        .gap(px(2.))
        .child(wait_line("members-wait", "Loading members…", cx))
        .child(group_label("Online", cx))
        .child(row(0.7, Some(0.45)))
        .child(row(0.52, None))
        .child(row(0.8, Some(0.6)))
        .child(row(0.4, None))
}

// The line under a member: the activity's filled glyph (live in accent ink), then its text and "+N".
fn activity_line(activity: &crate::discord::Activity, cx: &App) -> Div {
    let live = activity.kind == crate::discord::ActivityKind::Streaming;
    let text = if activity.more > 0 { format!("{} +{}", activity.text, activity.more) } else { activity.text.clone() };
    div()
        .flex()
        .items_center()
        .gap_1()
        .min_w_0()
        .text_size(px(12.))
        .text_color(palette::faint(cx))
        .children(activity_glyph(activity.kind).map(|icon| icon.size(px(13.)).flex_none().when(live, |i| i.text_color(palette::accent_ink(cx)))))
        .child(div().truncate().child(text))
}
