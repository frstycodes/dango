// The side panel (design/css/panels.css .panel): one right-hand frame for the member list and search
// results, with a fixed head and a scrolling body.
use std::collections::HashMap;

use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::{
    AnyElement, App, Context, Div, FontWeight, Radians, SharedString, Window,
    component::{ActiveTheme, Icon},
    div, img, prelude::*, px,
};
use gpui_motion_kit::core::{Motion, Target};

use crate::{
    Part, Waffle, Edge, RightPanel,
    discord::{ActivityKind, FeedEntry, ListItem, Member, Status},
    icons::Glyph,
    layout::{decorated_avatar, nameplate},
    motion, palette,
    sidebar::{Nav, activity_glyph, category_head, hover_ease},
    ui::{self, Squircle},
};

// The section shows this many cards; its chevron opens the rest.
const ACTIVITY_SHOWN: usize = 3;

impl Waffle {
    // Whether the side panel is on screen: a server's (unless hidden), or search results anywhere.
    pub(crate) fn panel_shown(&self) -> bool {
        match self.right_panel {
            RightPanel::Search(_) => true,
            RightPanel::Hidden => false,
            _ => self.guild_id.is_some(),
        }
    }

    // The side panel's width while it shows. A DM has no member list; its side panel is only ever search results.
    pub fn panel_column_width(&self) -> Option<f32> {
        match &self.right_panel {
            _ if !self.panel_shown() => None,
            RightPanel::Hidden => None,
            RightPanel::Members | RightPanel::Activity => Some(self.panel_width),
            RightPanel::Search(_) => Some(crate::search_results::PANEL_WIDTH),
        }
    }

    pub fn render_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let width = self.panel_column_width()?;
        // Its header sits on the window's top line beside the channel header's: "Members", or the search's
        // query with a way back to the members.
        let (head, body, foot): (Div, AnyElement, Option<AnyElement>) = match &self.right_panel {
            RightPanel::Hidden => return None,
            RightPanel::Members => (self.panel_head("Members".into(), false, cx), self.member_list(window, cx), None),
            RightPanel::Activity => (self.panel_head("Activity".into(), true, cx), self.activity_cards(usize::MAX, cx), None),
            RightPanel::Search(results) => {
                let head = self.panel_frame(cx).child(self.search_head(results, self.back_button(cx), cx));
                (head, self.search_body(results, cx), self.search_pager(results, cx))
            }
        };
        let line = palette::line(cx);
        Some(
            div()
                .relative()
                .w(px(width))
                .h_full()
                .flex_none()
                .flex()
                .flex_col()
                // The sidebar's tone: the step from the feed's tone is the only edge it needs.
                .bg(palette::surface(palette::Level::Sidebar, cx))
                .text_color(palette::text(cx))
                .text_size(px(14.))
                // The list runs up under the header, which blurs and fades it (ui::scroll_edge), as the feed does.
                .child(div().id("right-panel").flex_1().min_h_0().overflow_y_scrollbar().pt(px(crate::layout::HEADER_HEIGHT + 8.)).px_2().pb_2().flex().flex_col().gap(px(2.)).child(body))
                .child(ui::scroll_edge(palette::surface(palette::Level::Sidebar, cx), 0.).child(head))
                .child(self.resize_handle(Edge::Panel, cx))
                .children(foot.map(|pager| div().flex_none().py(px(8.)).border_t_1().border_color(line).child(pager)))
                .into_any_element(),
        )
    }

    // The column's header row, clear of Windows' caption buttons at the window's top right; `back` adds the
    // way back to the members.
    fn panel_head(&self, title: SharedString, back: bool, cx: &mut Context<Self>) -> Div {
        let back = back.then(|| self.back_button(cx));
        self.panel_frame(cx).child(div().flex_1().min_w_0().truncate().text_size(px(16.)).font_weight(FontWeight::SEMIBOLD).child(title)).children(back)
    }

    fn panel_frame(&self, cx: &App) -> Div {
        div()
            .h(px(crate::layout::HEADER_HEIGHT))
            .pl_4()
            .pr(px(8. + crate::window_controls::WIDTH))
            .flex()
            .items_center()
            .gap_2()
            .window_control_area(gpui_kit::WindowControlArea::Drag)
            .text_color(palette::strong(cx))
    }

    fn back_button(&self, cx: &mut Context<Self>) -> ui::Control {
        ui::icon_button("close-panel", Glyph::X, "Back to members", cx).occlude().on_click(cx.listener(|this, _, _, cx| {
            this.right_panel = RightPanel::Members;
            cx.notify();
        }))
    }

    // Who is here, grouped by top role, then online and offline (navigation.css .members).
    fn member_list(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let activity = self.activity_section(window, cx);
        let Some(guild) = self.guild() else { return div().into_any_element() };
        let Some(list) = self.member_lists.get(&guild.id).filter(|l| !l.is_empty()) else { return members_loading(cx).into_any_element() };
        let mut first = activity.is_none();
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
        div().flex().flex_col().gap(px(2.)).children(activity).children(rows.collect::<Vec<_>>()).into_any_element()
    }

    pub(crate) fn load_activity_feed(&mut self, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        cx.spawn(async move |this, cx| {
            // No feed (an error, or none for this account) leaves the section out.
            let Ok(feed) = client.activity_feed().await else { return };
            this.update(cx, |this, cx| {
                this.activity_feed = feed;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    // The feed's entries by members of the open server, each with that member, in Discord's order.
    fn guild_activity(&self) -> Vec<(&FeedEntry, &Member)> {
        let Some(list) = self.guild_id.as_ref().and_then(|g| self.member_lists.get(g)) else { return Vec::new() };
        let members: HashMap<&str, &Member> = list
            .iter()
            .filter_map(|item| match item {
                ListItem::Member(m) => Some((m.user.id.as_str(), m)),
                ListItem::Group { .. } => None,
            })
            .collect();
        self.activity_feed.iter().filter_map(|e| Some((e, *members.get(e.author_id.as_str())?))).collect()
    }

    // "Activity · N" over the member groups: folds from its label like a channel category, and its
    // chevron opens every card in the panel.
    fn activity_section(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<Div> {
        let count = self.guild_activity().len();
        if count == 0 {
            return None;
        }
        let key = format!("activity:{}", self.guild_id.clone().unwrap_or_default());
        let folded = self.folded.contains(&key);
        let [fold, chevron] = self.motions.frame(
            [
                (Part::Fold(key.clone()), Motion::new(Target::default().opacity(if folded { 0. } else { 1. })).transition(motion::fold())),
                // The chevron's angle in degrees rides the x channel; it is read back, never applied as an offset.
                (Part::Chevron(key.clone()), Motion::new(Target::default().x(if folded { -90. } else { 0. })).transition(hover_ease())),
            ],
            window,
            cx,
        );
        let label = div()
            .id("activity-section")
            .child(Nav::Caret.icon().size(px(11.)).rotate(Radians(chevron.target().x_px().to_radians())))
            .child(format!("Activity · {count}"))
            .on_click(cx.listener(move |this, _, _, cx| {
                if !this.folded.remove(&key) {
                    this.folded.insert(key.clone());
                }
                cx.notify();
            }));
        let all = ui::icon_button("activity-all", Glyph::ChevronRight, "Show all activity", cx).on_click(cx.listener(|this, _, _, cx| {
            this.right_panel = RightPanel::Activity;
            cx.notify();
        }));
        let alpha = fold.target().alpha();
        let cards = (!folded || alpha > 0.05).then(|| div().opacity(alpha).child(self.activity_cards(ACTIVITY_SHOWN, cx)));
        Some(div().flex().flex_col().gap(px(2.)).child(category_head("activity-head".into(), label, false, cx).child(all)).children(cards))
    }

    fn activity_cards(&self, limit: usize, cx: &mut Context<Self>) -> AnyElement {
        let now = chrono::Utc::now().timestamp_millis();
        let cards: Vec<AnyElement> = self.guild_activity().into_iter().take(limit).enumerate().map(|(i, (entry, member))| self.feed_card(i, entry, member, now, cx)).collect();
        div().flex().flex_col().gap(px(8.)).children(cards).into_any_element()
    }

    // One card: who (avatar, name in their role's colour), what, and when, with its art on the right.
    fn feed_card(&self, i: usize, entry: &FeedEntry, member: &Member, now: i64, cx: &mut Context<Self>) -> AnyElement {
        let dark = cx.theme().is_dark();
        let (strong, muted, hover) = (palette::strong(cx), palette::muted(cx), palette::hover(cx));
        let name_ink = self.role_color(self.guild_id.as_ref(), &member.user.id, Some(&member.roles), dark).unwrap_or(strong);
        let when = if entry.live { "Now".to_owned() } else { ago(now - entry.at) };
        let meta = div()
            .flex()
            .items_center()
            .gap(px(4.))
            .min_w_0()
            .text_size(px(12.))
            .font_weight(FontWeight::NORMAL)
            .text_color(muted)
            .children(activity_glyph(entry.kind).map(|icon| icon.size(px(14.)).flex_none().when(entry.live, |i| i.text_color(palette::status_in(Status::Online, cx)))))
            .child(div().flex_none().child(when))
            .when_some(entry.streak_days, |m, days| m.child(div().w(px(6.)).flex_none()).child(Icon::default().data(BOLT).size(px(14.)).flex_none()).child(div().truncate().child(format!("{days}× streak"))));
        let image = entry.image.clone().or_else(|| self.app_icon(entry.app_id.as_deref()?, cx));
        let art = match image {
            Some(url) => img(url).size(px(56.)).flex_none().object_fit(gpui_kit::ObjectFit::Cover).squircle(ui::RADIUS_MD).into_any_element(),
            None => div()
                .size(px(56.))
                .flex_none()
                .squircle(ui::RADIUS_MD)
                .bg(palette::selected(cx))
                .flex()
                .items_center()
                .justify_center()
                .text_color(muted)
                .children(activity_glyph(entry.kind).or_else(|| activity_glyph(ActivityKind::Playing)).map(|icon| icon.size(px(24.))))
                .into_any_element(),
        };
        div()
            .id(SharedString::from(format!("activity-{i}")))
            .flex()
            .items_center()
            .gap(px(12.))
            .p(px(10.))
            .squircle(ui::RADIUS_LG)
            .bg(palette::surface(palette::Level::Page, cx))
            .border_1()
            .border_color(palette::line(cx))
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(div().flex().items_center().gap(px(6.)).min_w_0().child(ui::avatar(member.user.avatar.clone(), 16.).flex_none()).child(div().truncate().text_color(name_ink).child(member.user.name.clone())))
                    .child(div().truncate().font_weight(FontWeight::MEDIUM).text_color(strong).child(entry.title.clone()))
                    .child(meta),
            )
            .child(art)
            .on_click({
                let user = member.user.clone();
                cx.listener(move |this, event: &gpui_kit::ClickEvent, _, cx| this.open_profile(user.clone(), event.position() - gpui_kit::point(px(320.), px(0.)), cx))
            })
            .into_any_element()
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
        let plate = member.user.nameplate.as_ref().map(|plate| nameplate(plate, surface, surface, [ui::RADIUS_SM; 4], true, dark));
        let owner = self.guild().is_some_and(|g| g.owner_id.as_ref() == Some(&member.user.id));
        let selected = self.profile_card.as_ref().is_some_and(|p| p.user.id == member.user.id);
        let crown = ui::mix(palette::status_in(Status::Idle, cx), strong, 0.4);
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
                            .child(ui::truncating(format!("member-name-{}", member.user.id), member.user.name.clone()).font_weight(FontWeight::NORMAL).text_color(name_ink))
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
}

// A group heading (.cat): "Online · 4".
fn group_label(text: impl Into<SharedString>, cx: &App) -> Div {
    div().h(px(28.)).flex_none().px(px(6.)).flex().items_center().whitespace_nowrap().text_size(px(12.)).font_weight(FontWeight::MEDIUM).text_color(palette::faint(cx)).child(text.into())
}

// A wait names what it waits on, beside the mini wave (.pwait).
fn wait_line(id: &'static str, text: &'static str, cx: &App) -> Div {
    div().min_h(px(32.)).px_2().flex().items_center().gap_2().text_color(palette::muted(cx)).font_weight(FontWeight::NORMAL).child(ui::bars(id, palette::muted(cx), 12.)).child(text)
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
        .child(ui::truncating(format!("activity-{text}"), text))
}

const BOLT: &[u8] = include_bytes!("../assets/icons/reicon/bolt.svg");

// How long ago, in the feed's steps: minutes, hours, days, then weeks.
pub(crate) fn ago(ms: i64) -> String {
    let minutes = (ms / 60_000).max(0);
    match minutes {
        0 => "Just now".to_owned(),
        1..60 => format!("{minutes}m ago"),
        60..1440 => format!("{}h ago", minutes / 60),
        1440..10080 => format!("{}d ago", minutes / 1440),
        _ => format!("{}w ago", minutes / 10080),
    }
}

#[cfg(test)]
mod tests {
    use super::ago;

    #[test]
    fn ago_steps_up_by_unit() {
        assert_eq!(ago(-5), "Just now");
        assert_eq!(ago(59_000), "Just now");
        assert_eq!(ago(5 * 60_000), "5m ago");
        assert_eq!(ago(3 * 3_600_000), "3h ago");
        assert_eq!(ago(26 * 3_600_000), "1d ago");
        assert_eq!(ago(15 * 86_400_000), "2w ago");
    }
}
