// The server rail and the channel list.
use gpui_kit::{
    AnyElement, App, Context, Div, FontWeight, Hsla, Radians, SharedString, Window,
    component::{ActiveTheme, Icon, tooltip::Tooltip},
    div, hsla, img, linear_color_stop, linear_gradient, prelude::*, px, rgb,
};
use gpui_kit::component::menu::{ContextMenuExt, PopupMenuItem};
use gpui_motion_kit::core::{Motion, Target};

use crate::{
    Rustcord, Edge, Part, RailEntry, Status,
    discord::{Channel, ChannelKind, Folder, Guild, Status as Status_},
    icons::Glyph,
    layout::{RAIL_WIDTH, avatar_with_status},
    motion, palette,
    rail_drag::{Drop, RailDrag},
};

const ROW_HEIGHT: f32 = 32.;
const ROW_GAP: f32 = 2.;

enum Face {
    Dms,
    Icon(String),
    Letters(String, String),
    Folder { open: bool, tint: Hsla, previews: Vec<(String, Option<String>, String)> },
}

impl Rustcord {
    pub fn render_rail(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let border = cx.theme().sidebar_border;
        if !cx.has_active_drag() {
            self.rail_drop = None;
        }
        let dm_mentions: u32 = self.dms.iter().map(|d| self.mention_count(&d.id)).sum();
        let dms = self.rail_item("dms".into(), "Direct Messages".into(), self.guild_id.is_none(), (false, dm_mentions), Face::Dms, None, window, cx);
        let mut items: Vec<AnyElement> = Vec::new();
        for entry in self.rail_entries() {
            match entry {
                RailEntry::Guild(guild) => items.push(self.guild_item(guild, window, cx)),
                RailEntry::Folder(folder, guilds) => items.push(self.folder_item(folder, guilds, window, cx)),
            }
        }
        div()
            .id("rail")
            .w(px(RAIL_WIDTH))
            .h_full()
            .flex_none()
            .pt_2()
            .pb(px(self.panel_stack_height() + 16.))
            .flex()
            .flex_col()
            .items_center()
            .gap_2()
            .overflow_y_scroll()
            .child(dms)
            .child(div().w(px(32.)).h(px(1.)).flex_none().bg(border))
            .children(items)
    }

    fn guild_item(&mut self, guild: Guild, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let selected = self.guild_id.as_ref() == Some(&guild.id);
        let badge = self.guild_badge(&guild);
        let face = match &guild.icon {
            Some(url) => Face::Icon(url.clone()),
            None => Face::Letters(guild.id.clone(), initials(&guild.name)),
        };
        self.rail_item(guild.id.clone(), guild.name.clone().into(), selected, badge, face, Some(guild.id), window, cx)
    }

    // A Discord folder: closed, a tile of its first four servers; open, a tinted column holding them all.
    fn folder_item(&mut self, folder: Folder, guilds: Vec<Guild>, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let open = self.open_folders.contains(&folder.id);
        let tint: Hsla = folder.color.map(|c| rgb(c).into()).unwrap_or(palette::blurple());
        let (unread, mentions) = guilds.iter().map(|g| self.guild_badge(g)).fold((false, 0), |(u, m), (gu, gm)| (u || gu, m + gm));
        let has_selected = guilds.iter().any(|g| self.guild_id.as_ref() == Some(&g.id));
        let name: SharedString = folder.name.clone().unwrap_or_else(|| guilds.iter().map(|g| g.name.as_str()).take(3).collect::<Vec<_>>().join(", ")).into();
        let id = folder.id.clone();
        let header = self.rail_item(
            format!("folder-{}", folder.id),
            name,
            has_selected && !open,
            if open { (false, 0) } else { (unread, mentions) },
            Face::Folder { open, tint, previews: guilds.iter().take(4).map(|g| (g.id.clone(), g.icon.clone(), initials(&g.name))).collect() },
            None,
            window,
            cx,
        );
        let header = div().id(SharedString::from(format!("folder-hit-{id}"))).child(header).on_click(cx.listener(move |this, _, _, cx| {
            if !this.open_folders.remove(&id) {
                this.open_folders.insert(id.clone());
            }
            cx.notify();
        }));
        if !open {
            return header.into_any_element();
        }
        let children: Vec<AnyElement> = guilds.into_iter().map(|g| self.guild_item(g, window, cx)).collect();
        div()
            .w(px(RAIL_WIDTH))
            .flex()
            .flex_col()
            .items_center()
            .child(div().w(px(48.)).rounded(px(16.)).bg(tint.opacity(0.18)).pb_1().flex().flex_col().items_center().gap_2().child(header).children(children))
            .when(theme.is_dark(), |c| c)
            .into_any_element()
    }

    // Discord's rail: 40px squircles that hold their shape, a pill that grows for unread, hover and the
    // selected server, and a red count for mentions.
    #[allow(clippy::too_many_arguments)]
    fn rail_item(
        &mut self,
        key: String,
        name: SharedString,
        selected: bool,
        (unread, mentions): (bool, u32),
        face: Face,
        guild_id: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let hovered = self.hovered.as_deref() == Some(&key);
        let pill_height = if selected { 40. } else if hovered { 20. } else if unread { 8. } else { 0. };
        let pill = self.motions.frame_one(
            Part::Pill(key.clone()),
            Motion::new(Target::default().height(pill_height).opacity(if pill_height > 0. { 1. } else { 0. })).transition(motion::spring()),
            window,
            cx,
        );
        let theme = cx.theme().clone();
        let (side, radius) = (40., 12.);
        let drag_icon = match &face {
            Face::Icon(url) => Some(url.clone()),
            _ => None,
        };
        let drop_hint = self.rail_drop.as_ref().filter(|(target, _)| *target == key).map(|(_, drop)| *drop);
        let face = match face {
            Face::Icon(url) => img(url).size(px(side)).rounded(px(radius)).into_any_element(),
            Face::Letters(id, label) => identity_tile(&id, label, 14.).size(px(side)).rounded(px(radius)).into_any_element(),
            Face::Dms => div()
                .size(px(side))
                .rounded(px(radius))
                .flex()
                .items_center()
                .justify_center()
                .bg(if selected || hovered { palette::blurple() } else { theme.foreground.opacity(0.1) })
                // White mark on blurple, blurple mark on the neutral tile: both official, never recoloured.
                .child(img(if selected || hovered { crate::icons::CLYDE_WHITE } else { crate::icons::CLYDE }).w(px(24.)).h(px(18.)))
                .into_any_element(),
            Face::Folder { open: true, tint, .. } => div()
                .size(px(side))
                .rounded(px(radius))
                .flex()
                .items_center()
                .justify_center()
                .text_color(tint)
                .child(Icon::new(Glyph::Folder).size(px(22.)))
                .into_any_element(),
            Face::Folder { open: false, tint, previews } => div()
                .size(px(side))
                .rounded(px(radius))
                .bg(tint.opacity(0.3))
                .p(px(4.))
                .flex()
                .flex_wrap()
                .gap(px(2.))
                .children(previews.into_iter().map(|(id, icon, label)| match icon {
                    Some(url) => img(url).size(px(15.)).rounded(px(4.)).into_any_element(),
                    None => identity_tile(&id, label.chars().take(1).collect::<String>(), 7.).size(px(15.)).rounded(px(4.)).into_any_element(),
                }))
                .into_any_element(),
        };
        let badge = (mentions > 0).then(|| {
            div()
                .absolute()
                .right(px(10.))
                .bottom(px(-2.))
                .min_w(px(18.))
                .h(px(18.))
                .px_1()
                .rounded_full()
                .border_2()
                .border_color(theme.sidebar)
                .bg(theme.danger)
                .text_color(gpui_kit::white())
                .text_size(px(11.))
                .font_weight(FontWeight::BOLD)
                .flex()
                .items_center()
                .justify_center()
                .child(if mentions > 99 { "99+".to_string() } else { mentions.to_string() })
        });
        let hover_key = key.clone();
        let item = div()
            .id(SharedString::from(format!("rail-{key}")))
            .relative()
            .w(px(RAIL_WIDTH))
            .h(px(40.))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .child(
                div()
                    .absolute()
                    .left_0()
                    .w(px(4.))
                    .h(px(pill.target().height_px().unwrap_or(pill_height)))
                    .rounded_r(px(4.))
                    .bg(theme.foreground.opacity(pill.target().alpha())),
            )
            .child(face)
            .children(badge)
            .children(drop_hint.map(|drop| drop_marker(drop, side, radius, theme.success)))
            .tooltip({
                let name = name.clone();
                move |window, cx| Tooltip::new(name.clone()).build(window, cx)
            })
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| this.set_hovered(&hover_key, *hovered, cx)))
            .when_some(guild_id.clone(), |item, guild_id| item.on_click(cx.listener(move |this, _, window, cx| this.open_guild(Some(guild_id.clone()), window, cx))))
            .when(key == "dms", |item| item.on_click(cx.listener(|this, _, window, cx| this.open_guild(None, window, cx))));
        let item = match key.as_str() {
            "dms" => item,
            _ => self.rail_draggable(item, key.clone(), drag_icon, name, cx),
        };
        let Some(guild_id) = guild_id else { return item.into_any_element() };
        let this = cx.weak_entity();
        item.context_menu(move |menu, _, _| {
            let (read, copy, g1, g2) = (this.clone(), this.clone(), guild_id.clone(), guild_id.clone());
            menu.item(PopupMenuItem::new("Mark As Read").on_click(move |_, _, cx| {
                read.update(cx, |this, cx| this.mark_guild_read(&g1, cx)).ok();
            }))
            .separator()
            .item(PopupMenuItem::new("Copy Server ID").on_click(move |_, _, cx| {
                copy.update(cx, |this, cx| this.copy_text(g2.clone(), cx)).ok();
            }))
        })
        .into_any_element()
    }

    fn rail_draggable(&self, item: gpui_kit::Stateful<Div>, key: String, icon: Option<String>, label: SharedString, cx: &mut Context<Self>) -> gpui_kit::Stateful<Div> {
        let can_combine = !key.starts_with("folder-");
        let (move_key, drop_key) = (key.clone(), key.clone());
        item.on_drag(RailDrag { key, icon, label }, |drag, _, _, cx| cx.new(|_| drag.clone()))
            .on_drag_move::<RailDrag>(cx.listener(move |this, event: &gpui_kit::DragMoveEvent<RailDrag>, _, cx| {
                let inside = event.bounds.contains(&event.event.position);
                let dragged_folder = event.drag(cx).key.starts_with("folder-");
                let hint = inside.then(|| {
                    let fraction = (event.event.position.y - event.bounds.top()) / event.bounds.size.height;
                    (move_key.clone(), Drop::at(fraction, can_combine && !dragged_folder))
                });
                match (&hint, &this.rail_drop) {
                    (Some(_), _) if this.rail_drop != hint => this.rail_drop = hint,
                    (None, Some((target, _))) if *target == move_key => this.rail_drop = None,
                    _ => return,
                }
                cx.notify();
            }))
            .on_drop(cx.listener(move |this, drag: &RailDrag, _, cx| {
                let drop = this.rail_drop.as_ref().filter(|(target, _)| *target == drop_key).map_or(Drop::Before, |(_, drop)| *drop);
                this.drop_on_rail(&drag.key.clone(), &drop_key, drop, cx);
            }))
    }

    pub fn render_channels(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let title: SharedString = match self.guild() {
            Some(guild) => guild.name.clone().into(),
            None => "Direct Messages".into(),
        };
        let mut rows: Vec<AnyElement> = Vec::new();
        match (&self.status, self.guild().cloned()) {
            (Status::Connecting, _) => rows.push(muted_line("Connecting to Discord…", cx)),
            (_, Some(guild)) => {
                for (category, channels) in channel_sections(&guild) {
                    let rows_for = channels.iter().flat_map(|c| self.channel_rows(c, &guild, cx)).collect();
                    match category {
                        Some(category) => rows.push(self.section(category, rows_for, window, cx)),
                        None => rows.extend(rows_for),
                    }
                }
            }
            (_, None) if self.dms.is_empty() => rows.push(muted_line("No direct messages yet", cx)),
            (_, None) => {
                rows.push(self.friends_row(cx));
                rows.push(div().h(px(28.)).px_2().mt_1().flex().items_center().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child("Direct Messages").into_any_element());
                rows.extend(self.dms.clone().iter().map(|c| self.dm_row(c, cx)));
            }
        }
        let reconnecting = match &self.status {
            Status::Reconnecting(reason) => Some(reason.clone()),
            _ => None,
        };
        let banner = self.motions.frame_one(
            Part::Banner,
            Motion::new(Target::default())
                .initial(Target::default().y(10.).opacity(0.))
                .exit(Target::default().y(10.).opacity(0.))
                .present(reconnecting.is_some())
                .transition(motion::spring()),
            window,
            cx,
        );
        let banner = banner.render(|frame| {
            frame.apply(
                div()
                    .mx_2()
                    .mb_2()
                    .px_3()
                    .py_2()
                    .rounded(px(10.))
                    .bg(theme.warning.opacity(0.12))
                    .text_size(px(12.))
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().size(px(6.)).flex_none().rounded_full().bg(theme.warning))
                    .child(div().truncate().child(format!("Reconnecting… {}", reconnecting.clone().unwrap_or_default()))),
            )
        });
        div()
            .relative()
            .w(px(self.channels_width))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .border_l_1()
            .border_color(theme.sidebar_border)
            .bg(theme.sidebar)
            .text_color(theme.sidebar_foreground)
            .child(self.resize_handle(Edge::Channels, cx))
            .child(
                div()
                    .h(px(48.))
                    .flex_none()
                    .px_4()
                    .flex()
                    .items_center()
                    .gap_1()
                    .border_b_1()
                    .border_color(theme.sidebar_border)
                    .text_size(px(16.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(div().flex_1().truncate().child(title))
                    .when(self.guild_id.is_some(), |h| h.child(Icon::new(Glyph::ChevronDown).size(px(16.))))
                    .id("server-header")
                    .when(self.guild_id.is_some(), |h| {
                        h.cursor_pointer().hover(|s| s.bg(theme.foreground.opacity(0.04))).on_click(cx.listener(|this, _, _, cx| {
                            this.server_menu = !this.server_menu;
                            cx.notify();
                        }))
                    }),
            )
            .child(
                div()
                    .id("channels")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_2()
                    .pt_2()
                    .pb(px(self.panel_stack_height() + 24.))
                    .flex()
                    .flex_col()
                    .gap(px(ROW_GAP))
                    .children(rows),
            )
            .child(div().pb(px(self.panel_stack_height() + 16.)).child(banner))
    }

    // A category folds from its label: height and fade together on the fold curve, the chevron
    // springs round with a little overshoot.
    fn section(&mut self, category: Channel, rows: Vec<AnyElement>, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let folded = self.folded.contains(&category.id);
        let [fold, chevron] = self.motions.frame(
            [
                (Part::Fold(category.id.clone()), Motion::new(Target::default().opacity(if folded { 0. } else { 1. })).transition(motion::fold())),
                // The chevron's angle in degrees rides the x channel; it is read back, never applied as an offset.
                (Part::Chevron(category.id.clone()), Motion::new(Target::default().x(if folded { -90. } else { 0. })).transition(motion::spring())),
            ],
            window,
            cx,
        );
        let degrees = chevron.target().x_px();
        let muted = cx.theme().muted_foreground;
        let id = category.id.clone();
        let label = div()
            .id(SharedString::from(format!("section-{}", category.id)))
            .h(px(24.))
            .mt_4()
            .px_1()
            .flex()
            .items_center()
            .gap_1()
            .cursor_pointer()
            .text_size(px(14.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(muted)
            .hover(|s| s.text_color(cx.theme().foreground))
            .child(div().truncate().child(category.name))
            .child(Icon::new(Glyph::ChevronDown).size(px(12.)).rotate(Radians(degrees.to_radians())))
            .on_click(cx.listener(move |this, _, _, cx| {
                if !this.folded.remove(&id) {
                    this.folded.insert(id.clone());
                }
                cx.notify();
            }));
        let alpha = fold.target().alpha();
        // Folded categories keep their unread and selected channels visible, as Discord does.
        let body = div().flex().flex_col().gap(px(ROW_GAP)).opacity(alpha).when(!folded || alpha > 0.05, |b| b.children(rows));
        div().flex().flex_col().gap(px(ROW_GAP)).child(label).child(body).into_any_element()
    }

    fn channel_rows(&self, channel: &Channel, guild: &Guild, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = cx.theme().clone();
        let selected = self.channel.as_ref().is_some_and(|c| c.id == channel.id);
        let unread = self.unread(&channel.id);
        let mentions = self.mention_count(&channel.id);
        let glyph = match (channel.kind, channel.private) {
            (ChannelKind::Voice, _) => Glyph::VolumeHigh,
            (_, true) => Glyph::HashtagLock,
            _ => Glyph::Hashtag,
        };
        let in_this_call = self.call.as_ref().is_some_and(|c| c.channel.id == channel.id);
        let lead_color = if in_this_call { palette::status(Status_::Online) } else { theme.muted_foreground };
        let lead = div().size(px(20.)).flex_none().flex().items_center().justify_center().text_color(lead_color).child(Icon::new(glyph).size(px(20.)));
        let elapsed = self.call.as_ref().filter(|c| c.channel.id == channel.id && c.connected).map(|c| {
            let secs = c.joined_at.elapsed().as_secs();
            div().text_size(px(12.)).font_weight(FontWeight::MEDIUM).text_color(palette::status(Status_::Online)).child(format!("{}:{:02}", secs / 60, secs % 60))
        });
        let row = div()
            .id(SharedString::from(channel.id.clone()))
            .relative()
            .h(px(ROW_HEIGHT))
            .flex_none()
            .px_2()
            .flex()
            .items_center()
            .gap_2()
            .rounded(px(8.))
            .cursor_pointer()
            .font_weight(FontWeight::MEDIUM)
            // --channels-default for read channels, --interactive-text-active for unread and selected.
            .text_color(if selected || unread { crate::palette::strong(cx) } else { gpui_kit::rgb(0x7A7B83).into() })
            .child(lead)
            .child(div().flex_1().truncate().child(channel.name.clone()))
            .children(elapsed)
            .when(mentions > 0, |r| r.child(mention_badge(mentions, cx)))
            .when(unread && !selected, |r| r.child(div().absolute().left(px(-8.)).w(px(4.)).h(px(8.)).rounded_r(px(4.)).bg(theme.foreground)));
        let row = if selected { row.bg(theme.sidebar_accent) } else { row.hover(|s| s.bg(theme.sidebar_accent.opacity(0.6)).text_color(theme.foreground)) };
        let mut rows = Vec::new();
        if channel.kind == ChannelKind::Voice {
            let join = channel.clone();
            let in_call = self.call.as_ref().is_some_and(|c| c.channel.id == channel.id);
            rows.push(
                row.when(in_call, |r| r.text_color(theme.foreground))
                    .on_click(cx.listener(move |this, _, window, cx| this.show_call(join.clone(), window, cx)))
                    .into_any_element(),
            );
            let speaking = self.call.as_ref().filter(|_| in_call).map(|c| c.speaking.clone()).unwrap_or_default();
            let watching = self.call.as_ref().and_then(|c| c.watching.as_ref()).map(|w| w.streamer_id.clone());
            let connected = self.call.as_ref().is_some_and(|c| c.connected && in_call);
            for state in guild.voice_states.iter().filter(|v| v.channel_id.as_ref() == Some(&channel.id)) {
                let user = state.user.clone();
                let name = user.as_ref().map(|u| u.name.clone()).unwrap_or_else(|| self.user_name(&state.user_id));
                let avatar = user.map(|u| u.avatar).unwrap_or_else(|| crate::discord::avatar_url(&state.user_id, None));
                let talking = speaking.contains(&state.user_id);
                let streamer = state.user_id.clone();
                let is_watched = watching.as_ref() == Some(&state.user_id);
                rows.push(
                    div()
                        .id(SharedString::from(format!("voice-{}", state.user_id)))
                        .h(px(32.))
                        .pl(px(30.))
                        .pr_2()
                        .flex()
                        .items_center()
                        .gap_2()
                        .rounded(px(6.))
                        .text_color(if talking { theme.foreground } else { theme.muted_foreground })
                        .child(
                            // Discord's speaking ring: green around the avatar while their mic is open.
                            div().size(px(24.)).flex_none().rounded_full().border_2().border_color(if talking { palette::status(Status_::Online) } else { gpui_kit::transparent_black() }).child(img(avatar).size_full().rounded_full()),
                        )
                        .child(div().flex_1().truncate().child(name))
                        .when(state.camera, |r| r.child(Icon::new(Glyph::Video).size(px(16.))))
                        .when(state.muted, |r| r.child(Icon::new(Glyph::MicOff).size(px(16.))))
                        .when(state.deafened, |r| r.child(Icon::new(Glyph::HeadphonesOff).size(px(16.))))
                        .when(state.streaming, |r| {
                            r.child(div().h(px(16.)).px_1().rounded(px(4.)).bg(theme.danger).text_color(gpui_kit::white()).text_size(px(10.)).font_weight(FontWeight::BOLD).flex().items_center().child("LIVE"))
                        })
                        .on_mouse_down(gpui_kit::MouseButton::Right, {
                            let user_id = state.user_id.clone();
                            cx.listener(move |this, e: &gpui_kit::MouseDownEvent, _, cx| {
                                this.participant_menu = Some((user_id.clone(), e.position));
                                cx.notify();
                            })
                        })
                        .when(state.streaming && connected, |r| {
                            r.cursor_pointer()
                                .when(is_watched, |r| r.bg(theme.sidebar_accent))
                                .hover(|s| s.bg(theme.sidebar_accent.opacity(0.6)))
                                .tooltip(|window, cx| Tooltip::new("Watch Stream").build(window, cx))
                                .on_click(cx.listener(move |this, _, _, cx| this.watch_stream(streamer.clone(), cx)))
                        })
                        .into_any_element(),
                );
            }
            return rows;
        }
        let (open, hover_id) = (channel.clone(), channel.id.clone());
        rows.push(
            row.on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if *hovered {
                    this.prefetch(&hover_id, cx);
                }
            }))
            .on_click(cx.listener(move |this, _, window, cx| this.open_channel(open.clone(), window, cx)))
            .context_menu({
                let (this, channel_id, guild_id) = (cx.weak_entity(), channel.id.clone(), guild.id.clone());
                move |menu, _, _| {
                    let (read, link, copy) = (this.clone(), this.clone(), this.clone());
                    let (c1, c2, c3, g) = (channel_id.clone(), channel_id.clone(), channel_id.clone(), guild_id.clone());
                    menu.item(PopupMenuItem::new("Mark As Read").on_click(move |_, _, cx| {
                        read.update(cx, |this, cx| this.mark_channel_read(&c1, cx)).ok();
                    }))
                    .separator()
                    .item(PopupMenuItem::new("Copy Link").on_click(move |_, _, cx| {
                        link.update(cx, |this, cx| this.copy_text(format!("https://discord.com/channels/{g}/{c2}"), cx)).ok();
                    }))
                    .item(PopupMenuItem::new("Copy Channel ID").on_click(move |_, _, cx| {
                        copy.update(cx, |this, cx| this.copy_text(c3.clone(), cx)).ok();
                    }))
                }
            })
            .into_any_element(),
        );
        rows
    }

    fn dm_row(&self, channel: &Channel, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let selected = self.channel.as_ref().is_some_and(|c| c.id == channel.id);
        let unread = self.unread(&channel.id);
        let mentions = self.mention_count(&channel.id);
        let status = channel.recipient.as_ref().map(|u| self.presences.get(&u.id).map(|p| p.status).unwrap_or_default());
        let avatar = channel.recipient.as_ref().map(|u| u.avatar.clone()).unwrap_or_else(|| crate::discord::avatar_url(&channel.id, None));
        let surface = theme.sidebar;
        let (open, hover_id) = (channel.clone(), channel.id.clone());
        let row = div()
            .id(SharedString::from(channel.id.clone()))
            .h(px(42.))
            .flex_none()
            .px_2()
            .flex()
            .items_center()
            .gap_3()
            .rounded(px(6.))
            .cursor_pointer()
            .text_color(if selected || unread { theme.foreground } else { theme.muted_foreground })
            .child(avatar_with_status(&avatar, 32., status, surface))
            .child(div().flex_1().truncate().child(channel.name.clone()))
            .when(mentions > 0, |r| r.child(mention_badge(mentions, cx)))
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if *hovered {
                    this.prefetch(&hover_id, cx);
                }
            }))
            .on_click(cx.listener(move |this, _, window, cx| this.open_channel(open.clone(), window, cx)));
        if selected {
            return row.bg(theme.sidebar_accent).into_any_element();
        }
        row.hover(|s| s.bg(theme.sidebar_accent.opacity(0.6)).text_color(theme.foreground)).into_any_element()
    }
}

fn mention_badge(count: u32, cx: &App) -> AnyElement {
    div()
        .min_w(px(16.))
        .h(px(16.))
        .px_1()
        .rounded_full()
        .bg(cx.theme().danger)
        .text_color(gpui_kit::white())
        .text_size(px(11.))
        .font_weight(FontWeight::BOLD)
        .flex()
        .items_center()
        .justify_center()
        .child(count.to_string())
        .into_any_element()
}

pub(crate) fn channel_sections(guild: &Guild) -> Vec<(Option<Channel>, Vec<Channel>)> {
    let visible = |c: &&Channel| matches!(c.kind, ChannelKind::Text | ChannelKind::Voice);
    let uncategorised: Vec<Channel> = guild.channels.iter().filter(visible).filter(|c| c.parent_id.is_none()).cloned().collect();
    let mut sections = vec![(None, uncategorised)];
    for category in guild.channels.iter().filter(|c| c.kind == ChannelKind::Category) {
        let children = guild.channels.iter().filter(visible).filter(|c| c.parent_id.as_ref() == Some(&category.id)).cloned().collect();
        sections.push((Some(category.clone()), children));
    }
    sections.retain(|(_, channels)| !channels.is_empty());
    sections
}

pub fn initials(name: &str) -> String {
    name.split_whitespace().filter_map(|w| w.chars().next()).take(3).collect()
}

// A lit tile like an app icon: a mid-to-deep gradient of the id's hue with the label in white.
// Discord's drop cues: a bar in the gap to reorder, a ring around the server to combine.
fn drop_marker(drop: Drop, side: f32, radius: f32, color: Hsla) -> Div {
    let bar = || div().absolute().w(px(side)).h(px(4.)).rounded_full().bg(color);
    match drop {
        Drop::Before => bar().top(px(-6.)),
        Drop::After => bar().bottom(px(-6.)),
        Drop::Into => div().absolute().size(px(side + 6.)).rounded(px(radius + 3.)).border_2().border_color(color),
    }
}

pub fn identity_tile(id: &str, label: impl Into<SharedString>, text_size: f32) -> Div {
    let hue = identity_hue(id);
    div()
        .flex()
        .items_center()
        .justify_center()
        .bg(linear_gradient(180., linear_color_stop(hsla(hue, 0.55, 0.56, 1.), 0.), linear_color_stop(hsla(hue, 0.6, 0.4, 1.), 1.)))
        .text_color(gpui_kit::white())
        .text_size(px(text_size))
        .font_weight(FontWeight::SEMIBOLD)
        .child(label.into())
}

fn identity_hue(id: &str) -> f32 {
    let hash = id.bytes().fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
    // Knuth multiplicative mix so near-identical ids (sequential snowflakes) still land on distant hues.
    (hash.wrapping_mul(2_654_435_761) % 360) as f32 / 360.
}

fn muted_line(text: &'static str, cx: &App) -> AnyElement {
    div().px_2().py_2().text_color(cx.theme().muted_foreground).child(text).into_any_element()
}
