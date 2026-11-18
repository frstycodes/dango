// The server rail and the channel list (design/css/navigation.css: .rail, .srv, .folder, .side, .ch).
use std::time::Duration;

use gpui_kit::{
    Animation, AnimationExt, AnyElement, App, Context, Div, FontWeight, Hsla, Radians, SharedString, Stateful, Window,
    base::{Transition as Tween, transition},
    component::{ActiveTheme, Icon, tooltip::Tooltip},
    div, img, point, prelude::*, px, relative, rgb,
};
use gpui_motion_kit::core::{Bezier, Easing, Motion, Target, Transition};

use crate::{
    Rustcord, Edge, Part, RailEntry, Status,
    discord::{ActivityKind, Channel, ChannelKind, Folder, Guild, Status as Status_},
    drag::{RowDrag, hole},
    icons::Glyph,
    layout::{RAIL_WIDTH, avatar_with_status},
    motion, palette,
    rail_drag::{Drop, RailDrag},
    ui::{self, Squircle},
};

// Rail geometry: 44px tiles 8px apart, centred in the 68px rail, so the state pill sits on its edge.
pub(crate) const TILE: f32 = 44.;
const RAIL_GAP: f32 = 8.;
pub(crate) const TILE_X: f32 = (RAIL_WIDTH - TILE) / 2.;
const MINI: f32 = 16.;
const MINI_GAP: f32 = 4.;
const MINI_INSET: f32 = (TILE - 2. * MINI - MINI_GAP) / 2.;
// A folder opening: badges shrink away, the servers travel from their mini tiles to their slots, then
// their badges pop back. Closing runs the same steps in reverse.
const BADGE_OUT: Duration = Duration::from_millis(100);
const BADGE_IN: Duration = Duration::from_millis(200);
const TRIP: Duration = Duration::from_millis(260);

const ROW_HEIGHT: f32 = 32.;
const ROW_GAP: f32 = 2.;

#[derive(Clone)]
pub(crate) enum Face {
    Home,
    Icon(String),
    Letters(String),
    Folder { tint: Hsla, minis: Vec<Mini> },
}

// A server as a folder shows it: its icon, or its first letter.
#[derive(Clone)]
pub(crate) struct Mini {
    icon: Option<String>,
    letters: String,
}

// One tile's state, as the rail draws it.
struct RailTile {
    key: String,
    name: SharedString,
    selected: bool,
    unread: bool,
    mentions: u32,
    muted: bool,
    in_voice: bool,
    face: Face,
    guild_id: Option<String>,
    // Badges shrink with a folder's trip (1 shown, 0 gone).
    badge_scale: f32,
    // An open folder's head trades its minis for the folder glyph (0 closed, 1 open).
    open: f32,
}

impl Rustcord {
    pub fn render_rail(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !cx.has_active_drag() {
            self.rail_drop = None;
            self.channel_drop = None;
        }
        let dm_mentions: u32 = self.dms.iter().map(|d| self.mention_count(&d.id)).sum();
        let home = RailTile {
            key: "dms".into(),
            name: "Direct messages".into(),
            selected: self.guild_id.is_none(),
            unread: false,
            mentions: dm_mentions,
            muted: false,
            in_voice: false,
            face: Face::Home,
            guild_id: None,
            badge_scale: 1.,
            open: 0.,
        };
        let home = self.rail_item(home, window, cx);
        let mut items: Vec<AnyElement> = Vec::new();
        for entry in self.rail_entries() {
            match entry {
                RailEntry::Guild(guild) => items.push(self.guild_item(guild, 1., window, cx)),
                RailEntry::Folder(folder, guilds) => items.push(self.folder_item(folder, guilds, window, cx)),
            }
        }
        div()
            .id("rail")
            .w(px(RAIL_WIDTH))
            .h_full()
            .flex_none()
            .py(px(12.))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(RAIL_GAP))
            .overflow_y_scroll()
            .text_size(px(14.))
            .child(home)
            .children(items)
            .child(rail_action("rail-discover", Nav::Compass, "Discover servers", cx).on_click(|_, _, cx| cx.open_url("https://discord.com/servers")))
            .child(div().flex_none().h(self.user_panel_clearance()))
    }

    // Who's in the server's voice channels and who's live there, by avatar, for its hover card.
    fn server_preview(&self, guild_id: &str, name: SharedString) -> ServerPreview {
        let Some(guild) = self.guilds.iter().find(|g| g.id == guild_id) else { return ServerPreview { name, voice: Vec::new(), live: Vec::new() } };
        let members = self.members.get(guild_id);
        let avatar = |state: &crate::discord::VoiceState| {
            state.user.as_ref().or_else(|| members.and_then(|m| m.get(&state.user_id))).map(|u| u.avatar.clone())
        };
        let present = guild.voice_states.iter().filter(|v| v.channel_id.is_some());
        ServerPreview {
            name,
            voice: present.clone().filter_map(avatar).collect(),
            live: present.filter(|v| v.streaming).filter_map(avatar).collect(),
        }
    }

    fn guild_tile(&self, guild: &Guild, badge_scale: f32) -> RailTile {
        let (unread, mentions) = self.guild_badge(guild);
        RailTile {
            key: guild.id.clone(),
            name: guild.name.clone().into(),
            selected: self.guild_id.as_ref() == Some(&guild.id),
            unread,
            mentions,
            muted: self.guild_settings.get(&guild.id).is_some_and(|s| s.is_muted()),
            in_voice: self.call.as_ref().is_some_and(|c| c.guild_id.as_ref() == Some(&guild.id)),
            face: server_face(guild),
            guild_id: Some(guild.id.clone()),
            badge_scale,
            open: 0.,
        }
    }

    fn guild_item(&mut self, guild: Guild, badge_scale: f32, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let tile = self.guild_tile(&guild, badge_scale);
        self.rail_item(tile, window, cx)
    }

    // A folder: closed, a tile of its first four servers; open, a tinted column holding them all.
    fn folder_item(&mut self, folder: Folder, guilds: Vec<Guild>, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let open = self.open_folders.contains(&folder.id);
        let key = format!("folder-{}", folder.id);
        let motion = FolderMotion::frame(&key, open, window, cx);
        let tint: Hsla = folder.color.map(|c| rgb(c).into()).unwrap_or(palette::faint(cx));
        let (unread, mentions) = guilds.iter().map(|g| self.guild_badge(g)).fold((false, 0), |(u, m), (gu, gm)| (u || gu, m + gm));
        let has_selected = guilds.iter().any(|g| self.guild_id.as_ref() == Some(&g.id));
        let name: SharedString = folder.name.clone().unwrap_or_else(|| guilds.iter().map(|g| g.name.as_str()).take(3).collect::<Vec<_>>().join(", ")).into();
        let minis: Vec<Mini> = guilds.iter().map(mini).collect();
        let head = RailTile {
            key: key.clone(),
            name,
            selected: has_selected && !open,
            unread: unread && !open,
            mentions: if open { 0 } else { mentions },
            muted: false,
            in_voice: false,
            face: Face::Folder { tint, minis: minis.iter().take(4).cloned().collect() },
            guild_id: None,
            badge_scale: motion.head_badge,
            open: motion.trip,
        };
        let head = self.rail_item(head, window, cx);
        let (toggle_id, menu_id) = (folder.id.clone(), folder.id.clone());
        let head = div()
            .id(SharedString::from(format!("folder-hit-{}", folder.id)))
            .absolute()
            .top_0()
            .left_0()
            .child(head)
            .on_mouse_down(gpui_kit::MouseButton::Right, cx.listener(move |this, e: &gpui_kit::MouseDownEvent, _, cx| this.open_folder_menu(menu_id.clone(), e.position, cx)))
            .on_click(cx.listener(move |this, _, _, cx| {
                if !this.open_folders.remove(&toggle_id) {
                    this.open_folders.insert(toggle_id.clone());
                }
                cx.notify();
            }));
        let count = guilds.len();
        let landed = motion.trip >= 1.;
        let servers: Vec<AnyElement> = match (motion.trip > 0., landed) {
            (false, _) => Vec::new(),
            (true, true) => guilds.into_iter().enumerate().map(|(i, g)| div().absolute().left_0().top(px(slot_y(i))).child(self.guild_item(g, motion.child_badge, window, cx)).into_any_element()).collect(),
            (true, false) => minis.iter().enumerate().map(|(i, m)| flying_tile(m, i, motion.trip, cx)).collect(),
        };
        div()
            .relative()
            .w(px(RAIL_WIDTH))
            .h(px(TILE + motion.trip * count as f32 * (TILE + RAIL_GAP)))
            .flex_none()
            .child(div().absolute().top_0().bottom_0().left(px(TILE_X)).w(px(TILE)).squircle(ui::RADIUS_LG).bg(tint.opacity(0.18 * motion.trip)))
            .children(servers)
            .child(head)
            .into_any_element()
    }

    // A rail tile: the face, the state pill on the rail's edge (dot unread, longer on hover, full when
    // selected), and a count for mentions.
    fn rail_item(&mut self, tile: RailTile, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let RailTile { key, name, selected, unread, mentions, muted, in_voice, face, guild_id, badge_scale, open } = tile;
        let hovered = self.hovered.as_deref() == Some(&key);
        let pill_height = if selected { 36. } else if hovered { 20. } else if unread { 8. } else { 0. };
        let pill = self.motions.frame_one(Part::Pill(key.clone()), Motion::new(Target::default().height(pill_height)).transition(hover_ease()), window, cx);
        let pill_height = pill.target().height_px().unwrap_or(pill_height).max(0.);
        let rail = palette::surface(palette::Level::Rail, cx);
        let look = crate::drag::Look::Tile(face.clone());
        let drop_hint = self.rail_drop.as_ref().filter(|(target, _)| *target == key).map(|(_, drop)| *drop);
        let item_id = SharedString::from(format!("rail-{key}"));
        let carried = self.drag_motion.row(&item_id);
        let face = match carried {
            RowDrag::Rest => render_face(face, TILE, ui::RADIUS_LG, selected, open, cx),
            RowDrag::Hole => hole(div().size(px(TILE)).squircle(ui::RADIUS_LG), cx).into_any_element(),
            RowDrag::Hidden => div().size(px(TILE)).into_any_element(),
        };
        let hover_key = key.clone();
        let preview = guild_id.as_deref().map(|id| self.server_preview(id, name.clone()));
        let item = ui::pressable(div().id(item_id.clone()), 0.97)
            .children(self.drag_motion.spot(&item_id))
            .relative()
            .w(px(RAIL_WIDTH))
            .h(px(TILE))
            .flex_none()
            .cursor_pointer()
            .child(div().absolute().left_0().top(px((TILE - pill_height) / 2.)).w(px(4.)).h(px(pill_height)).rounded_r(px(4.)).bg(palette::strong(cx)))
            // GAP: muted tiles skip the design's saturate(0.35): gpui has no element filters.
            .child(div().absolute().left(px(TILE_X)).top_0().when(muted, |f| f.opacity(0.55)).child(face))
            .when(mentions > 0 && badge_scale > 0.01, |item| item.child(rail_count(mentions, badge_scale, rail)))
            .when(in_voice && mentions == 0, |item| item.child(voice_mark(rail, cx)))
            .children(drop_hint.map(drop_marker))
            .tooltip({
                let name = name.clone();
                move |window, cx| match preview.clone().filter(ServerPreview::has_activity) {
                    Some(preview) => cx.new(|_| preview).into(),
                    None => Tooltip::new(name.clone()).build(window, cx),
                }
            })
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| this.set_hovered(&hover_key, *hovered, cx)))
            .when_some(guild_id.clone(), |item, guild_id| item.on_click(cx.listener(move |this, _, window, cx| this.open_guild(Some(guild_id.clone()), window, cx))))
            .when(key == "dms", |item| item.on_click(cx.listener(|this, _, window, cx| this.open_guild(None, window, cx))));
        let item = match key.as_str() {
            "dms" => item,
            _ => self.rail_draggable(item, RailDrag { key: key.clone() }, look, cx),
        };
        let Some(guild_id) = guild_id else { return item.into_any_element() };
        item.on_mouse_down(gpui_kit::MouseButton::Right, cx.listener(move |this, e: &gpui_kit::MouseDownEvent, _, cx| this.open_server_menu(guild_id.clone(), e.position, false, cx)))
            .into_any_element()
    }

    // Carried, a tile swells to 104% and hangs at -4° (.srv.dragging), tilting further with speed.
    fn rail_draggable(&self, item: ui::Pressable, drag: RailDrag, look: crate::drag::Look, cx: &mut Context<Self>) -> ui::Pressable {
        let key = drag.key.clone();
        let can_combine = !key.starts_with("folder-");
        let (move_key, drop_key) = (key.clone(), key.clone());
        let pick_up = crate::drag::pick_up(cx.weak_entity(), format!("rail-{key}").into(), look, point(px(RAIL_WIDTH), px(TILE)), 1.04, -4.);
        item.on_drag(drag, pick_up)
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

    // .preview, the hover card on someone live: the stream's thumbnail under LIVE, with a way in
    // over it on hover, then what they share and how many watch. It grows toward its row from 96%
    // with a fade, passing 100% before it settles.
    fn stream_card(&self, streamer: Streamer, channel: &Channel, stream_key: &str, connected: bool, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Only once the bytes are in, so the fade below shows the picture rather than an empty box.
        let preview = self.stream_preview(streamer.id).filter(|url| matches!(window.use_asset::<gpui_kit::ImgResourceLoader>(&gpui_kit::Resource::Uri(url.clone().into()), cx), Some(Ok(_))));
        let app = self.presences.get(streamer.id).and_then(|p| p.activity.as_ref()).filter(|a| matches!(a.kind, ActivityKind::Playing | ActivityKind::Streaming)).map(|a| a.text.as_str());
        let viewer_ids: Vec<&String> = self.stream_viewers.get(stream_key).into_iter().flatten().filter(|id| *id != streamer.id).collect();
        let viewer_names: Vec<String> = viewer_ids.iter().map(|id| self.user_name(id)).collect();
        let watching = (!viewer_ids.is_empty()).then(|| format!("{} watching", viewer_ids.len()));
        let live = stream_meta(None, self.stream_live_since.get(stream_key).map(|at| at.elapsed()));
        let detail: Vec<String> = app.map(|_| streamer.name.to_owned()).into_iter().chain(watching).chain(live).collect();
        let who: Option<SharedString> = viewer_line(&viewer_names).map(Into::into);
        let hover_key = stream_hover_key(streamer.id);
        let (watch_id, watch_channel) = (streamer.id.to_owned(), channel.clone());
        let overlay = palette::surface(palette::Level::Overlay, cx);
        let thumb_group = SharedString::from(format!("stream-thumb-{}", streamer.id));
        let thumb = div()
            .id(SharedString::from(format!("stream-card-thumb-{}", streamer.id)))
            .group(thumb_group.clone())
            .relative()
            .w_full()
            .aspect_ratio(16. / 9.)
            .cursor_pointer()
            .child(crate::call::stream_fill(ui::RADIUS_MD).absolute().inset_0())
            .children(preview.map(|url| {
                ui::media(url, ui::all(ui::RADIUS_MD))
                    .absolute()
                    .size_full()
                    .object_fit(gpui_kit::ObjectFit::Cover)
                    .with_animation(SharedString::from(format!("stream-preview-{}", streamer.id)), Animation::new(ui::DUR_POP), |el, t| el.opacity(t))
            }))
            .child(ui::live_badge().absolute().top(px(8.)).left(px(8.)))
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .squircle(ui::RADIUS_MD)
                    .bg(gpui_kit::black().opacity(0.45))
                    .flex()
                    .items_center()
                    .justify_center()
                    .opacity(0.)
                    .group_hover(thumb_group, |s| s.opacity(1.))
                    .child(crate::call::watch_button(if connected { "Watch stream" } else { "Join and watch" })),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.stream_card_hover = None;
                match connected {
                    true => this.watch_stream(watch_id.clone(), cx),
                    // Not in that call yet: join it, then watch once connected.
                    false => this.watch_on_connect = Some(watch_id.clone()),
                }
                this.show_call(watch_channel.clone(), window, cx);
            }));
        let cap = div()
            .id(SharedString::from(format!("stream-card-cap-{}", streamer.id)))
            .px(px(6.))
            .pt(px(2.))
            .pb(px(4.))
            .flex()
            .items_center()
            .gap(px(8.))
            .min_w_0()
            .child(ui::avatar(streamer.avatar.to_owned(), 24.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(div().truncate().font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(app.unwrap_or(streamer.name).to_owned()))
                    .child(div().truncate().text_size(px(12.)).text_color(palette::faint(cx)).child(detail.join(" · "))),
            )
            .when_some(who, |d, who| d.tooltip(move |window, cx| Tooltip::new(who.clone()).build(window, cx)));
        div()
            .id(SharedString::from(format!("stream-card-{}", streamer.id)))
            .w(px(STREAM_CARD_WIDTH))
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                this.stream_card_hover = hovered.then(|| hover_key.clone());
                cx.notify();
            }))
            .p(px(6.))
            .squircle(ui::RADIUS_LG)
            .bg(overlay)
            .shadow(ui::shadow_pop(cx))
            .flex()
            .flex_col()
            .gap(px(6.))
            .text_size(px(14.))
            .child(thumb)
            .child(cap)
            .transform_origin(point(relative(0.), relative(0.)))
            .with_animation(SharedString::from(format!("stream-card-open-{}", streamer.id)), Animation::new(Duration::from_millis(180)).with_easing(motion::back_out), |card, t| {
                card.scale(0.96 + 0.04 * t).opacity((t / 0.6).clamp(0., 1.))
            })
    }

    pub fn render_channels(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut rows: Vec<AnyElement> = Vec::new();
        match (&self.status, self.guild().cloned()) {
            (Status::Connecting, _) => rows.push(muted_line("Connecting to Discord…", cx)),
            (_, Some(guild)) => {
                let me = self.my_id().to_owned();
                for (i, (category, channels)) in channel_sections(&guild, &me).into_iter().enumerate() {
                    let channel_rows = channels.iter().flat_map(|c| self.channel_rows(c, &guild, window, cx)).collect();
                    match category {
                        Some(category) => rows.push(self.section(category, channel_rows, i > 0, window, cx)),
                        None => rows.extend(channel_rows.into_iter().map(|row: ChannelRow| row.element)),
                    }
                }
            }
            (_, None) => {
                rows.push(self.friends_row(cx));
                let start = cat_add("dm-start", None, "Start a conversation", cx).on_click(cx.listener(|this, _, window, cx| this.open_switcher(window, cx)));
                rows.push(div().mt(px(16.)).child(category_head("dm-head".into(), div().id("dm-head").child("Direct messages"), true, cx).child(start)).into_any_element());
                match self.dms.is_empty() {
                    true => rows.push(muted_line("No direct messages yet", cx)),
                    false => rows.extend(self.dms.clone().iter().map(|c| self.dm_row(c, cx))),
                }
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
        let warning = cx.theme().warning;
        let banner = banner.render(|frame| {
            frame.apply(
                div()
                    .mx_2()
                    .mb_2()
                    .px_3()
                    .py_2()
                    .squircle(ui::RADIUS_MD)
                    .bg(warning.opacity(0.12))
                    .text_size(px(12.))
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().size(px(6.)).flex_none().rounded_full().bg(warning))
                    .child(div().truncate().child(format!("Reconnecting… {}", reconnecting.clone().unwrap_or_default()))),
            )
        });
        let name_in_strip = crate::layout::brand_bar_height(window) > 0.;
        let header = match self.guild() {
            Some(_) if name_in_strip => None,
            Some(guild) => Some(server_name_button(guild.name.clone().into(), false, cx).into_any_element()),
            None => Some(dm_find(cx).on_click(cx.listener(|this, _, window, cx| this.open_switcher(window, cx))).into_any_element()),
        };
        div()
            .relative()
            .w(px(self.channels_width))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .bg(palette::surface(palette::Level::Sidebar, cx))
            .text_color(palette::muted(cx))
            .text_size(px(14.))
            .child(self.resize_handle(Edge::Channels, cx))
            .children(header.map(|header| {
                div()
                    .h(px(crate::layout::HEADER_HEIGHT))
                    .flex_none()
                    .flex()
                    .items_center()
                    .map(|h| if self.guild_id.is_some() { h.p(px(6.)) } else { h.px_2() })
                    .window_control_area(gpui_kit::WindowControlArea::Drag)
                    .child(header)
            }))
            .child(div().id("channels").flex_1().min_h_0().overflow_y_scroll().p_2().flex().flex_col().gap(px(ROW_GAP)).children(rows))
            .child(banner)
            .child(div().flex_none().h(self.user_panel_clearance()))
    }


    // A category folds from its label: height and fade together on the fold curve, the chevron turns.
    // Folded, its unread and selected channels stay in view.
    fn section(&mut self, category: Channel, rows: Vec<ChannelRow>, spaced: bool, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let folded = self.folded.contains(&category.id);
        let [fold, chevron] = self.motions.frame(
            [
                (Part::Fold(category.id.clone()), Motion::new(Target::default().opacity(if folded { 0. } else { 1. })).transition(motion::fold())),
                // The chevron's angle in degrees rides the x channel; it is read back, never applied as an offset.
                (Part::Chevron(category.id.clone()), Motion::new(Target::default().x(if folded { -90. } else { 0. })).transition(hover_ease())),
            ],
            window,
            cx,
        );
        let degrees = chevron.target().x_px();
        let target_ink = palette::accent_ink(cx);
        let id = category.id.clone();
        let label = div()
            .id(SharedString::from(format!("section-{}", category.id)))
            .child(Nav::Caret.icon().size(px(11.)).rotate(Radians(degrees.to_radians())))
            .child(div().truncate().child(category.name.clone()))
            .on_click(cx.listener(move |this, _, _, cx| {
                if !this.folded.remove(&id) {
                    this.folded.insert(id.clone());
                }
                cx.notify();
            }))
            .drag_over::<crate::drag::ChannelDrag>(move |s, _, _, _| s.text_color(target_ink))
            .on_drop({
                let category = category.clone();
                cx.listener(move |this, drag: &crate::drag::ChannelDrag, _, cx| this.drop_channel(drag, &category, crate::drag::Place::Into, cx))
            })
            .on_mouse_down(gpui_kit::MouseButton::Right, {
                let category = category.clone();
                cx.listener(move |this, e: &gpui_kit::MouseDownEvent, _, cx| {
                    let Some(guild_id) = this.guild_id.clone() else { return };
                    this.open_channel_menu(category.clone(), guild_id, e.position, cx);
                })
            });
        let can_add = self.guild_id.as_ref().is_some_and(|g| self.can_arrange_channels(g));
        let group: SharedString = format!("cathead-{}", category.id).into();
        let head = category_head(group.clone(), label, false, cx).when(can_add, |head| {
            let category = category.clone();
            head.child(cat_add(SharedString::from(format!("section-add-{}", category.id)), Some(group), "Create channel", cx).on_mouse_down(
                gpui_kit::MouseButton::Left,
                cx.listener(move |this, e: &gpui_kit::MouseDownEvent, _, cx| {
                    let Some(guild_id) = this.guild_id.clone() else { return };
                    cx.stop_propagation();
                    this.open_channel_menu(category.clone(), guild_id, e.position, cx);
                }),
            ))
        });
        let alpha = fold.target().alpha();
        let gone = folded && alpha <= 0.05;
        let rows = rows.into_iter().filter(|row| !gone || row.keep).map(|row| match row.keep {
            true => row.element,
            false => div().opacity(alpha).child(row.element).into_any_element(),
        });
        div().when(spaced, |g| g.mt(px(16.))).flex().flex_col().gap(px(ROW_GAP)).child(head).children(rows).into_any_element()
    }

    fn channel_rows(&self, channel: &Channel, guild: &Guild, window: &mut Window, cx: &mut Context<Self>) -> Vec<ChannelRow> {
        let selected = self.channel.as_ref().is_some_and(|c| c.id == channel.id);
        let settings = self.guild_settings.get(&guild.id);
        let muted = settings.is_some_and(|s| s.channel_muted(&channel.id) || channel.parent_id.as_ref().is_some_and(|p| s.channel_muted(p)));
        // Discord's "Hide Muted Channels" keeps the open channel visible.
        if muted && !selected && settings.is_some_and(|s| s.hide_muted) {
            return Vec::new();
        }
        let unread = self.unread(&channel.id) && !muted;
        let mentions = self.mention_count(&channel.id);
        let in_call = self.call.as_ref().is_some_and(|c| c.channel.id == channel.id);
        let state = RowState { selected, unread, muted, strong: in_call };
        let elapsed = self.call.as_ref().filter(|c| c.channel.id == channel.id && c.connected).and_then(|_| self.voice_elapsed(&channel.id)).map(|secs| {
            div().flex_none().text_size(px(12.)).font_weight(FontWeight::NORMAL).text_color(palette::positive_ink(cx)).child(format!("{}:{:02}", secs / 60, secs % 60))
        });
        let group: SharedString = format!("ch-{}", channel.id).into();
        let hovered = self.hovered_row.as_deref() == Some(channel.id.as_str());
        let meta = match (muted, channel.private) {
            (true, _) => Some(Nav::BellSlash.icon()),
            (false, true) => Some(Icon::new(Glyph::HashtagLock)),
            _ => None,
        };
        let faint = palette::faint(cx);
        let carried = self.drag_motion.row(&channel.id);
        let welcomes = channel.kind == ChannelKind::Voice && self.welcomes_voice_drop(channel, guild);
        let row = channel_row(SharedString::from(channel.id.clone()), group.clone(), state, cx).children(self.drag_motion.spot(&channel.id));
        let row = match carried {
            RowDrag::Hole => hole(row.bg(gpui_kit::transparent_black()), cx),
            _ => row
                .when(carried == RowDrag::Hidden, |r| r.invisible())
                .when(welcomes, |r| r.bg(palette::accent_soft(cx)).text_color(palette::strong(cx)))
                .child(channel_glyph(channel).size(px(18.)).flex_none().text_color(match (welcomes, selected) {
                    (true, _) => palette::accent_ink(cx),
                    (false, true) => palette::muted(cx),
                    (false, false) => faint,
                }))
                .child(div().flex_1().min_w_0().truncate().child(channel.name.clone()))
                .children(elapsed)
                .children(meta.filter(|_| !selected && !hovered).map(|icon| div().flex_none().flex().text_color(faint).child(icon.size(px(15.)))))
                .when(selected || hovered, |r| r.child(self.row_actions(channel, guild, cx)))
                .when(mentions > 0, |r| r.child(ui::count_badge(mentions))),
        };
        let is_voice = channel.kind == ChannelKind::Voice;
        let row = self.track_row_hover(self.channel_draggable(row, channel, guild, cx), &channel.id, !is_voice, cx);
        let keep = selected || unread;
        if !is_voice {
            let open = channel.clone();
            let row = row
                .on_click(cx.listener(move |this, _, window, cx| this.open_channel(open.clone(), window, cx)))
                .on_mouse_down(gpui_kit::MouseButton::Right, {
                    let (channel, guild_id) = (channel.clone(), guild.id.clone());
                    cx.listener(move |this, e: &gpui_kit::MouseDownEvent, _, cx| this.open_channel_menu(channel.clone(), guild_id.clone(), e.position, cx))
                });
            let mut rows = vec![ChannelRow { keep, element: row.into_any_element() }];
            rows.extend(self.thread_rows(channel, guild, cx));
            return rows;
        }
        let join = channel.clone();
        let row = self
            .voice_drop_zone(row, channel.id.clone().into(), channel, cx)
            .on_click(cx.listener(move |this, _, window, cx| this.show_call(join.clone(), window, cx)))
            .on_mouse_down(gpui_kit::MouseButton::Right, {
                let (channel, guild_id) = (channel.clone(), guild.id.clone());
                cx.listener(move |this, e: &gpui_kit::MouseDownEvent, _, cx| this.open_channel_menu(channel.clone(), guild_id.clone(), e.position, cx))
            });
        let mut rows = vec![ChannelRow { keep: keep || in_call, element: row.into_any_element() }];
        // A carried channel takes its people along in the ghost.
        if carried == RowDrag::Rest {
            rows.extend(self.voice_rows(channel, guild, in_call, window, cx).into_iter().map(|element| ChannelRow { keep: in_call, element }));
        }
        rows
    }

    // A voice channel takes people dropped anywhere on it, its row or the people under it. Each part
    // claims the target while the pointer is on it and lets go only of its own claim, so moving
    // between parts of one channel never drops it.
    fn voice_drop_zone(&self, zone: Stateful<Div>, part: SharedString, channel: &Channel, cx: &mut Context<Self>) -> Stateful<Div> {
        let (target, name, drop_target) = (channel.id.clone(), channel.name.clone(), channel.clone());
        zone.on_drag_move::<crate::drag::VoiceDrag>(cx.listener(move |this, e: &gpui_kit::DragMoveEvent<crate::drag::VoiceDrag>, _, cx| {
            let claim = Some((target.clone(), part.clone()));
            let inside = e.bounds.contains(&e.event.position);
            match (inside, this.drag_motion.voice_target == claim) {
                (true, false) => {
                    let joining = this.me.as_ref().is_some_and(|me| me.id == e.drag(cx).user_id);
                    let home = this.guilds.iter().flat_map(|g| &g.voice_states).any(|v| v.user_id == e.drag(cx).user_id && v.channel_id.as_ref() == Some(&target));
                    this.drag_motion.voice_target = claim;
                    this.drag_motion.set_tag((!home).then(|| format!("{} {name}", if joining { "Join" } else { "Move to" }).into()));
                }
                (false, true) => {
                    this.drag_motion.voice_target = None;
                    this.drag_motion.set_tag(None);
                }
                _ => return,
            }
            cx.notify();
        }))
        .on_drop(cx.listener(move |this, drag: &crate::drag::VoiceDrag, _, cx| this.drop_voice_user(drag, drop_target.clone(), cx)))
    }

    // Held over this voice channel by someone from elsewhere: it lights up and opens a slot.
    fn welcomes_voice_drop(&self, channel: &Channel, guild: &Guild) -> bool {
        let targeted = self.drag_motion.voice_target.as_ref().is_some_and(|(id, _)| *id == channel.id);
        targeted && !guild.voice_states.iter().any(|v| v.channel_id.as_ref() == Some(&channel.id) && self.drag_motion.row(&format!("voice-{}", v.user_id)) == RowDrag::Hole)
    }

    // Who a carried voice channel takes along, as its ghost shows them.
    fn voice_people(&self, channel: &Channel, guild: &Guild) -> Vec<(String, SharedString)> {
        guild
            .voice_states
            .iter()
            .filter(|v| v.channel_id.as_ref() == Some(&channel.id))
            .map(|v| {
                let avatar = v.user.as_ref().map(|u| u.avatar.clone()).unwrap_or_else(|| crate::discord::avatar_url(&v.user_id, None));
                let name = v.user.as_ref().map(|u| u.name.clone()).unwrap_or_else(|| self.user_name(&v.user_id));
                (avatar, name.into())
            })
            .collect()
    }

    // Rows show their hover-only controls from this, not from `group_hover`: gpui resolves group hover
    // only while painting, so a `display` toggled by it paints children that were never prepainted.
    fn track_row_hover(&self, row: Stateful<Div>, id: &str, prefetch: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        let id = id.to_owned();
        row.on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
            match *hovered {
                true if prefetch => this.prefetch(&id, cx),
                true => {}
                false if this.hovered_row.as_deref() != Some(id.as_str()) => return,
                false => {}
            }
            this.hovered_row = hovered.then(|| id.clone());
            cx.notify();
        }))
    }

    // Invite and options, shown in the row's meta slot while it's hovered or selected.
    fn row_actions(&self, channel: &Channel, guild: &Guild, cx: &mut Context<Self>) -> Div {
        let (gid, cid) = (guild.id.clone(), channel.id.clone());
        let (menu_channel, menu_guild) = (channel.clone(), guild.id.clone());
        let invite = row_action(SharedString::from(format!("invite-{}", channel.id)), Icon::new(Glyph::GroupPlus), "Invite people", cx)
            .on_click(cx.listener(move |this, _, window, cx| this.show_invite(gid.clone(), cid.clone(), window, cx)));
        let options = row_action(SharedString::from(format!("options-{}", channel.id)), Icon::new(Glyph::Settings), "Channel options", cx).on_mouse_down(
            gpui_kit::MouseButton::Left,
            cx.listener(move |this, e: &gpui_kit::MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.open_channel_menu(menu_channel.clone(), menu_guild.clone(), e.position, cx);
            }),
        );
        div()
            .flex_none()
            .flex()
            .gap(px(2.))
            .child(invite)
            .child(options)
    }

    // Who is in a voice channel: 30px rows under it, the mini wave for whoever speaks, then one mark.
    fn voice_rows(&self, channel: &Channel, guild: &Guild, in_call: bool, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let speaking = self.call.as_ref().filter(|_| in_call).map(|c| c.speaking.clone()).unwrap_or_default();
        let watching: Vec<String> = self.call.as_ref().map(|c| c.watching.iter().map(|w| w.streamer_id.clone()).collect()).unwrap_or_default();
        let connected = self.call.as_ref().is_some_and(|c| c.connected && in_call);
        let can_drag_people = self.can_move_people(&guild.id);
        let mut present: Vec<&crate::discord::VoiceState> = guild.voice_states.iter().filter(|v| v.channel_id.as_ref() == Some(&channel.id)).collect();
        // Whoever is streaming leads the list; the rest keep their order (a stable sort).
        present.sort_by_key(|v| !v.streaming);
        let avatar_of = |v: &crate::discord::VoiceState| v.user.as_ref().map(|u| u.avatar.clone()).unwrap_or_else(|| crate::discord::avatar_url(&v.user_id, None));
        if self.hidden_voice_names.contains(&channel.id) && !present.is_empty() {
            let sidebar = palette::surface(palette::Level::Sidebar, cx);
            return vec![div().h(px(30.)).pl(px(32.)).flex().items_center().child(ui::avatar_stack(present.iter().map(|v| avatar_of(v)), 22., sidebar)).into_any_element()];
        }
        let (hover, selected, text) = (palette::hover(cx), palette::selected(cx), palette::text(cx));
        let mut rows = Vec::new();
        let me = self.me.as_ref().map(|m| m.id.clone());
        let elsewhere = |user_id: &str| me.as_deref() == Some(user_id) && !in_call;
        for state in present {
            let name = state.user.as_ref().map(|u| u.name.clone()).unwrap_or_else(|| self.user_name(&state.user_id));
            let avatar = avatar_of(state);
            let talking = speaking.contains(&state.user_id);
            let streamer = state.user_id.clone();
            let is_watched = watching.contains(&state.user_id);
            let hover_key = stream_hover_key(&state.user_id);
            // Open while the row or the card itself is hovered, so the card's button can be reached.
            let card_open = self.hovered.as_deref() == Some(hover_key.as_str()) || self.stream_card_hover.as_deref() == Some(hover_key.as_str());
            let stream_card = (state.streaming && card_open).then(|| {
                let stream_key = format!("guild:{}:{}:{}", guild.id, channel.id, state.user_id);
                self.stream_card(Streamer { id: &state.user_id, name: &name, avatar: &avatar }, channel, &stream_key, connected, window, cx).into_any_element()
            });
            let stream_hover = cx.listener(move |this, hovered: &bool, _, cx| this.set_hovered(&hover_key, *hovered, cx));
            let end = voice_mark_for(state, cx);
            let key = SharedString::from(format!("voice-{}", state.user_id));
            let carried = self.drag_motion.row(&key);
            let row = div().id(key.clone()).h(px(30.)).flex_none().squircle(ui::RADIUS_SM).children(self.drag_motion.spot(&key));
            let row = self.voice_drop_zone(row, key.clone(), channel, cx);
            if carried == RowDrag::Hole {
                rows.push(hole(row, cx).into_any_element());
                continue;
            }
            let row = match can_drag_people || self.me.as_ref().is_some_and(|m| m.id == state.user_id) {
                true => {
                    let drag = crate::drag::VoiceDrag { user_id: state.user_id.clone(), guild_id: guild.id.clone(), name: name.clone().into(), avatar: avatar.clone() };
                    let look = crate::drag::Look::Person { avatar: drag.avatar.clone(), name: drag.name.clone() };
                    row.on_drag(drag, crate::drag::pick_up(cx.weak_entity(), key, look, point(px(self.channels_width - 16.), px(30.)), 1.05, 0.))
                }
                false => row,
            };
            rows.push(
                row.when(carried == RowDrag::Hidden, |r| r.invisible())
                    .pl(px(32.))
                    .pr_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .font_weight(FontWeight::NORMAL)
                    .text_color(if talking { palette::strong(cx) } else { palette::muted(cx) })
                    .hover(move |s| s.bg(hover).text_color(text))
                    // You, in this call from another client (not this app): avatar and name faded, as Discord does.
                    .child(speaking_avatar(&avatar, talking).when(elsewhere(&state.user_id), |a| a.opacity(0.45)))
                    .child(div().flex_1().min_w_0().truncate().when(elsewhere(&state.user_id), |n| n.opacity(0.45)).child(name.clone()))
                    .children(end)
                    .on_mouse_down(gpui_kit::MouseButton::Right, {
                        let user_id = state.user_id.clone();
                        cx.listener(move |this, e: &gpui_kit::MouseDownEvent, _, cx| this.voice_user_menu(&user_id, e.position, cx))
                    })
                    .when(state.streaming, |r| r.on_hover(stream_hover).children(stream_card.map(|card| div().absolute().top_0().right(px(4.)).child(gpui_kit::deferred(gpui_kit::anchored().snap_to_window_with_margin(px(8.)).child(card))))))
                    .when(state.streaming && connected, |r| {
                        r.cursor_pointer()
                            .when(is_watched, |r| r.bg(selected))
                            .tooltip(|window, cx| Tooltip::new("Watch stream").build(window, cx))
                            .on_click(cx.listener(move |this, _, _, cx| this.watch_stream(streamer.clone(), cx)))
                    })
                    .into_any_element(),
            );
        }
        // .vd-slot: where the person held over this channel will sit.
        if self.welcomes_voice_drop(channel, guild) {
            rows.push(div().h(px(30.)).ml(px(32.)).flex_none().squircle(ui::RADIUS_SM).border(px(1.5)).border_color(palette::accent()).bg(palette::accent_soft(cx)).into_any_element());
        }
        rows
    }

    // With Manage Channels, a channel can be dragged above or below another, or onto a category.
    fn channel_draggable(&self, row: Stateful<Div>, channel: &Channel, guild: &Guild, cx: &mut Context<Self>) -> Stateful<Div> {
        if !self.can_arrange_channels(&guild.id) {
            return row;
        }
        let hint = self.channel_drop.as_ref().filter(|(id, _)| *id == channel.id).map(|(_, place)| *place);
        let marker = hint.map(|place| {
            let bar = div().absolute().left_0().right_0().h(px(2.)).rounded_full().bg(palette::accent());
            match place {
                crate::drag::Place::After => bar.bottom(px(-2.)),
                _ => bar.top(px(-2.)),
            }
        });
        let drag = crate::drag::ChannelDrag { channel_id: channel.id.clone(), guild_id: guild.id.clone() };
        let people = match channel.kind {
            ChannelKind::Voice => self.voice_people(channel, guild),
            _ => Vec::new(),
        };
        // The ghost is the row and its people, 30px each under it 1px apart.
        let size = point(px(self.channels_width - 16.), px(ROW_HEIGHT + people.len() as f32 * 31.));
        let look = crate::drag::Look::Channel { channel: Box::new(channel.clone()), people };
        let (move_id, drop_target) = (channel.id.clone(), channel.clone());
        row.children(marker)
            .on_drag(drag, crate::drag::pick_up(cx.weak_entity(), channel.id.clone().into(), look, size, 1.05, 0.))
            .on_drag_move::<crate::drag::ChannelDrag>(cx.listener(move |this, e: &gpui_kit::DragMoveEvent<crate::drag::ChannelDrag>, _, cx| {
                let inside = e.bounds.contains(&e.event.position);
                let hint = inside.then(|| (move_id.clone(), crate::drag::place_at((e.event.position.y - e.bounds.top()) / e.bounds.size.height)));
                let tag = inside.then(|| this.category_move(&e.drag(cx).channel_id, &move_id)).flatten();
                match (&hint, &this.channel_drop) {
                    (Some(_), _) if this.channel_drop != hint => this.channel_drop = hint,
                    (None, Some((id, _))) if *id == move_id => this.channel_drop = None,
                    _ => return,
                }
                this.drag_motion.set_tag(tag);
                cx.notify();
            }))
            .on_drop(cx.listener(move |this, drag: &crate::drag::ChannelDrag, _, cx| {
                let place = this.channel_drop.take().map_or(crate::drag::Place::Before, |(_, p)| p);
                this.drop_channel(drag, &drop_target, place, cx);
            }))
    }

    // "Move to Archive": the tag a channel carries over a row in another category.
    fn category_move(&self, moved: &str, over: &str) -> Option<SharedString> {
        let channels = &self.guild()?.channels;
        let parent_of = |id: &str| channels.iter().find(|c| c.id == id).and_then(|c| c.parent_id.clone());
        let to = parent_of(over)?;
        if parent_of(moved).as_ref() == Some(&to) {
            return None;
        }
        let name = &channels.iter().find(|c| c.id == to)?.name;
        Some(format!("Move to {name}").into())
    }

    // The threads you've joined (and the one you have open), hung under their channel on a hairline
    // tree: each draws its branch from the row above, and all but the last carry the trunk on.
    fn thread_rows(&self, channel: &Channel, guild: &Guild, cx: &mut Context<Self>) -> Vec<ChannelRow> {
        let open = self.channel.as_ref().map(|c| c.id.clone());
        let threads: Vec<&Channel> = guild
            .threads
            .iter()
            .filter(|t| t.parent_id.as_ref() == Some(&channel.id))
            .filter(|t| open.as_ref() == Some(&t.id) || t.thread.as_ref().is_some_and(|m| m.joined && !m.archived))
            .collect();
        let count = threads.len();
        let line = ui::mix(palette::faint(cx), palette::surface(palette::Level::Sidebar, cx), 0.5);
        threads
            .into_iter()
            .enumerate()
            .map(|(i, thread)| {
                let selected = open.as_ref() == Some(&thread.id);
                let unread = self.unread(&thread.id);
                let target = thread.clone();
                let (menu_thread, guild_id) = (thread.clone(), guild.id.clone());
                // The first branch starts just below its parent, never touching the parent's fill.
                let gap = if i == 0 { -4. } else { ROW_GAP };
                let branch = div().absolute().left(px(16.)).top(px(-gap)).w(px(12.)).h(px(THREAD_HEIGHT / 2. + gap)).border_l(px(1.5)).border_b(px(1.5)).rounded_bl(px(8.)).border_color(line);
                let trunk = (i + 1 < count).then(|| div().absolute().left(px(16.)).top(px(THREAD_HEIGHT / 2. - 8.)).bottom_0().w(px(1.5)).bg(line));
                let state = RowState { selected, unread, muted: false, strong: false };
                let row = channel_row(SharedString::from(format!("thread-{}", thread.id)), format!("thread-{}", thread.id).into(), state, cx)
                    .h(px(THREAD_HEIGHT))
                    .pl(px(34.))
                    .child(branch)
                    .children(trunk)
                    .child(div().flex_1().min_w_0().truncate().child(thread.name.clone()))
                    .on_click(cx.listener(move |this, _, window, cx| this.open_channel(target.clone(), window, cx)))
                    .on_mouse_down(gpui_kit::MouseButton::Right, cx.listener(move |this, e: &gpui_kit::MouseDownEvent, _, cx| this.open_thread_menu(menu_thread.clone(), guild_id.clone(), e.position, cx)));
                ChannelRow { keep: selected || unread, element: row.into_any_element() }
            })
            .collect()
    }

    // A conversation: the person, then one line on what they're doing.
    fn dm_row(&self, channel: &Channel, cx: &mut Context<Self>) -> AnyElement {
        let selected = self.channel.as_ref().is_some_and(|c| c.id == channel.id);
        let unread = self.unread(&channel.id);
        let mentions = self.mention_count(&channel.id);
        let presence = channel.recipient.as_ref().map(|u| self.presences.get(&u.id).cloned().unwrap_or_default());
        let status = presence.as_ref().map(|p| p.status);
        let offline = status == Some(Status_::Offline);
        let avatar = channel.recipient.as_ref().map(|u| u.avatar.clone()).unwrap_or_else(|| crate::discord::avatar_url(&channel.id, None));
        let typing = self.typing.get(&channel.id).is_some_and(|t| t.iter().any(|t| t.until > std::time::Instant::now()));
        let faint = palette::faint(cx);
        let line: Option<Div> = match (typing, presence) {
            (true, _) => Some(div().child(ui::bars(SharedString::from(format!("dm-typing-{}", channel.id)), faint, 10.)).child("Typing…")),
            (false, Some(p)) => match (p.activity, p.status) {
                (Some(a), _) => Some(div().children(activity_glyph(a.kind).map(|i| i.size(px(13.)).flex_none())).child(div().truncate().child(a.text))),
                (None, Status_::Offline) => None,
                (None, s) => Some(div().child(s.label())),
            },
            (false, None) => None,
        };
        let state = RowState { selected, unread, muted: false, strong: false };
        let (open, close_id) = (channel.clone(), channel.id.clone());
        let group: SharedString = format!("dm-{}", channel.id).into();
        let hovered = self.hovered_row.as_deref() == Some(channel.id.as_str());
        // Hovered, the close button takes the count's place.
        let close = row_action(SharedString::from(format!("dm-close-{}", channel.id)), Nav::Close.icon(), "Close", cx)
            .flex_none()
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.close_dm(&close_id, cx);
            }));
        let row = channel_row(SharedString::from(channel.id.clone()), group.clone(), state, cx);
        self.track_row_hover(row, &channel.id, true, cx)
            .h(px(44.))
            .gap(px(10.))
            .font_weight(FontWeight::NORMAL)
            .child(div().when(offline, |a| a.opacity(0.5)).child(avatar_with_status(&avatar, 32., status, palette::surface(palette::Level::Sidebar, cx))))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(div().truncate().font_weight(if unread { FontWeight::MEDIUM } else { FontWeight::NORMAL }).child(channel.name.clone()))
                    .children(line.map(|l| l.flex().items_center().gap_1().min_w_0().text_size(px(12.)).text_color(faint))),
            )
            .when(mentions > 0 && !hovered, |r| r.child(ui::count_badge(mentions)))
            .when(hovered, |r| r.child(close))
            .on_click(cx.listener(move |this, _, window, cx| this.open_channel(open.clone(), window, cx)))
            .on_mouse_down(gpui_kit::MouseButton::Right, {
                let channel = channel.clone();
                cx.listener(move |this, e: &gpui_kit::MouseDownEvent, _, cx| this.open_dm_menu(channel.clone(), e.position, cx))
            })
            .into_any_element()
    }
}

// CHANNEL LIST

const THREAD_HEIGHT: f32 = 28.;

// A row and whether it stays in view while its category is folded.
struct ChannelRow {
    keep: bool,
    element: AnyElement,
}

#[derive(Clone, Copy)]
pub(crate) struct RowState {
    pub selected: bool,
    pub unread: bool,
    pub muted: bool,
    // Full ink without the unread pill (the voice channel you're in).
    pub strong: bool,
}

// A sidebar row (.ch): muted ink at rest; selected is a fill, unread is bold with the pill on the
// list's edge, muted is faint. `group` names the row for its hover-revealed actions.
pub(crate) fn channel_row(id: SharedString, group: SharedString, state: RowState, cx: &App) -> Stateful<Div> {
    let RowState { selected, unread, muted, strong } = state;
    let (hover, text) = (palette::hover(cx), palette::text(cx));
    let ink = match (selected || unread || strong, muted) {
        (true, _) => palette::strong(cx),
        (false, true) => palette::faint(cx),
        (false, false) => palette::muted(cx),
    };
    div()
        .id(id)
        .group(group)
        .relative()
        .h(px(ROW_HEIGHT))
        .w_full()
        .flex_none()
        .px_2()
        .flex()
        .items_center()
        .gap_2()
        .squircle(ui::RADIUS_SM)
        .cursor_pointer()
        .font_weight(if unread { FontWeight::MEDIUM } else { FontWeight::NORMAL })
        .text_color(ink)
        .when(selected, |r| r.bg(palette::selected(cx)))
        .when(!selected, |r| r.hover(move |s| s.bg(hover).text_color(text)))
        .when(unread && !selected, |r| r.child(div().absolute().left(px(-8.)).w(px(4.)).h(px(8.)).rounded_r(px(4.)).bg(palette::strong(cx))))
}

// A category's header row (.cathead): the label, a button that folds the category unless `static_`,
// and room on its right for the add button. `group` names the row so the add can show on hover.
fn category_head(group: SharedString, label: Stateful<Div>, static_: bool, cx: &App) -> Div {
    let (hover, strong) = (palette::hover(cx), palette::strong(cx));
    div().group(group).relative().flex().items_center().child(
        label
            .flex_1()
            .min_w_0()
            .h(px(28.))
            .px(px(6.))
            .squircle(ui::RADIUS_SM)
            .flex()
            .items_center()
            .gap_1()
            .whitespace_nowrap()
            .text_size(px(12.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(palette::faint(cx))
            .when(!static_, |l| l.cursor_pointer().hover(move |s| s.bg(hover).text_color(strong))),
    )
}

// The + at a category's end: creates a channel there (or, on Direct messages, starts a conversation).
// With `reveal_on`, it shows only while that header group is hovered.
fn cat_add(id: impl Into<gpui_kit::ElementId>, reveal_on: Option<SharedString>, tooltip: &'static str, cx: &App) -> Stateful<Div> {
    let strong = palette::strong(cx);
    div()
        .id(id)
        .absolute()
        .right(px(2.))
        .size(px(24.))
        .squircle(ui::RADIUS_XS)
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .text_color(palette::muted(cx))
        .hover(move |s| s.text_color(strong))
        .when_some(reveal_on, |add, group| add.opacity(0.).group_hover(group, |s| s.opacity(1.)))
        .child(Nav::Plus.icon().size(px(14.)))
        .tooltip(move |window, cx| Tooltip::new(tooltip).build(window, cx))
}

// A 24px action inside a row (.ch .acts button).
fn row_action(id: SharedString, icon: Icon, tooltip: &'static str, cx: &App) -> Stateful<Div> {
    let strong = palette::strong(cx);
    div()
        .id(id)
        .size(px(24.))
        .squircle(ui::RADIUS_XS)
        .flex()
        .items_center()
        .justify_center()
        .text_color(palette::muted(cx))
        .hover(move |s| s.text_color(strong))
        .child(icon.size(px(16.)))
        .tooltip(move |window, cx| Tooltip::new(tooltip).build(window, cx))
}

// The type glyph a channel leads with (Discord's raw types: 5 announcement, 13 stage, 16 media).
pub(crate) fn channel_glyph(channel: &Channel) -> Icon {
    match (channel.kind, channel.raw_type) {
        (_, 5) => Nav::Megaphone.icon(),
        (_, 13) => Nav::Stage.icon(),
        (_, 16) => Nav::Media.icon(),
        (ChannelKind::Voice, _) => Icon::new(Glyph::VolumeHigh),
        (ChannelKind::Forum, _) => Icon::new(Glyph::Forum),
        _ => Icon::new(Glyph::Hashtag),
    }
}

// A voice occupant's avatar; while they speak, an accent ring sits 1px clear of it, as Discord's does.
fn speaking_avatar(avatar: &str, talking: bool) -> Div {
    div()
        .relative()
        .flex_none()
        .size(px(22.))
        .child(ui::avatar(avatar.to_owned(), 22.))
        .when(talking, |a| a.child(div().absolute().top(px(-3.)).left(px(-3.)).size(px(28.)).rounded_full().border(px(2.)).border_color(palette::accent())))
}

// The one mark a voice occupant gets, most important first: server-muted, live, camera, muted or
// deafened. Speaking shows as a ring on the avatar instead.
fn voice_mark_for(state: &crate::discord::VoiceState, cx: &App) -> Option<AnyElement> {
    let mark = |icon: Icon, color: Hsla| div().flex_none().flex().text_color(color).child(icon.size(px(15.))).into_any_element();
    let faint = palette::faint(cx);
    if state.server_muted || state.server_deafened {
        return Some(mark(Nav::MicOffFill.icon(), palette::danger_ink(cx)));
    }
    if state.streaming {
        return Some(ui::live_badge().h(px(18.)).px(px(6.)).text_size(px(11.)).into_any_element());
    }
    if state.camera {
        return Some(mark(Nav::CameraFill.icon(), faint));
    }
    if state.deafened {
        return Some(mark(Nav::DeafenedFill.icon(), faint));
    }
    if state.muted {
        return Some(mark(Nav::MicOffFill.icon(), faint));
    }
    None
}

// The DM list's header: a field-shaped button that opens the quick switcher.
fn dm_find(cx: &App) -> Stateful<Div> {
    let strong = palette::strong(cx);
    div()
        .id("dm-find")
        .occlude()
        .w_full()
        .h(px(32.))
        .px(px(10.))
        .squircle(ui::RADIUS_SM)
        .flex()
        .items_center()
        .gap_2()
        .cursor_pointer()
        .bg(palette::surface(palette::Level::Rail, cx))
        .text_color(palette::muted(cx))
        .text_size(px(14.))
        .hover(move |s| s.text_color(strong))
        .child(Icon::new(Glyph::Search).size(px(16.)))
        .child(div().truncate().child("Find or start a conversation"))
}

pub(crate) fn channel_sections(guild: &Guild, me: &str) -> Vec<(Option<Channel>, Vec<Channel>)> {
    let visible = |c: &&Channel| matches!(c.kind, ChannelKind::Text | ChannelKind::Voice | ChannelKind::Forum) && guild.can_view(me, c);
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

// RAIL

// A folder's motion this frame. Each step's delay lets the one before it finish: badges leave, the
// servers travel, badges return.
struct FolderMotion {
    trip: f32,
    head_badge: f32,
    child_badge: f32,
}

impl FolderMotion {
    fn frame(key: &str, open: bool, window: &mut Window, cx: &mut App) -> Self {
        let id = SharedString::from(key.to_owned());
        let (target, back) = if open { (1., 0.) } else { (0., 1.) };
        let trip = transition((id.clone(), "trip"), target, Tween::new(TRIP).delay(BADGE_OUT).ease(ease_out()), window, cx);
        let head_delay = if open { Duration::ZERO } else { BADGE_OUT + TRIP };
        let head_badge = transition((id.clone(), "head-badge"), back, badge_tween(open, head_delay), window, cx);
        let child_delay = if open { BADGE_OUT + TRIP } else { Duration::ZERO };
        let child_badge = transition((id, "child-badge"), target, badge_tween(!open, child_delay), window, cx);
        Self { trip, head_badge, child_badge }
    }
}

// Badges shrink quickly on the way out and pop back with a little overshoot.
fn badge_tween(leaving: bool, delay: Duration) -> Tween {
    match leaving {
        true => Tween::new(BADGE_OUT).delay(delay).ease(ease_out()),
        false => Tween::new(BADGE_IN).delay(delay).ease(motion::back_out),
    }
}

// --ease-out: cubic-bezier(0.23, 1, 0.32, 1).
fn ease_out() -> impl Fn(f32) -> f32 + 'static {
    gpui_kit::base::animation::cubic_bezier(0.23, 1., 0.32, 1.)
}

// Hover and state changes: --dur-hover on --ease-out.
fn hover_ease() -> Transition {
    Transition::Tween(ui::DUR_HOVER, Easing::Bezier(Bezier::new(0.23, 1., 0.32, 1.).expect("monotonic bezier")))
}

// Where an open folder's `i`th server sits, from the folder's top.
fn slot_y(i: usize) -> f32 {
    (i + 1) as f32 * (TILE + RAIL_GAP)
}

// A server on its way between its mini tile and its slot. Past the fourth, servers have no mini and
// grow out of the last one.
fn flying_tile(mini: &Mini, i: usize, t: f32, cx: &App) -> AnyElement {
    let slot = i.min(3);
    let from = (TILE_X + MINI_INSET + (slot % 2) as f32 * (MINI + MINI_GAP), MINI_INSET + (slot / 2) as f32 * (MINI + MINI_GAP), if i < 4 { MINI } else { 0. });
    let to = (TILE_X, slot_y(i), TILE);
    let lerp = |a: f32, b: f32| a + (b - a) * t;
    let size = lerp(from.2, to.2);
    let face = match &mini.icon {
        Some(url) => img(url.clone()).size(px(size)).squircle(lerp(ui::RADIUS_XS, ui::RADIUS_LG)).into_any_element(),
        None => letters_tile(&mini.letters, size, lerp(ui::RADIUS_XS, ui::RADIUS_LG), cx).into_any_element(),
    };
    div().absolute().left(px(lerp(from.0, to.0))).top(px(lerp(from.1, to.1))).child(face).into_any_element()
}

pub(crate) fn server_face(guild: &Guild) -> Face {
    match &guild.icon {
        Some(url) => Face::Icon(url.clone()),
        None => Face::Letters(initials(&guild.name)),
    }
}

fn mini(guild: &Guild) -> Mini {
    Mini { icon: guild.icon.clone(), letters: initials(&guild.name) }
}

// A tile's face at `size`. `open` trades a folder's minis for the folder glyph.
pub(crate) fn render_face(face: Face, size: f32, radius: f32, selected: bool, open: f32, cx: &App) -> AnyElement {
    let tile = || div().size(px(size)).flex_none().squircle(radius).flex().items_center().justify_center();
    match face {
        Face::Icon(url) => img(url).size(px(size)).flex_none().squircle(radius).into_any_element(),
        Face::Letters(label) => letters_tile(&label, size, radius, cx).into_any_element(),
        Face::Home => tile()
            .bg(if selected { palette::accent() } else { palette::surface(palette::Level::Raised, cx) })
            .child(img(if selected { crate::icons::MARK_WHITE } else { crate::icons::MARK }).size(px(size / 2.)))
            .into_any_element(),
        Face::Folder { tint, minis } => tile()
            .relative()
            .bg(tint.opacity(0.24 * (1. - open)))
            .child(div().absolute().inset_0().flex().items_center().justify_center().opacity(open).text_color(tint).child(Icon::new(Glyph::Folder).size(px(22.))))
            .when(open <= 0., |t| t.child(minis_grid(minis, cx)))
            .into_any_element(),
    }
}

fn minis_grid(minis: Vec<Mini>, cx: &App) -> Div {
    let sidebar = palette::surface(palette::Level::Raised, cx);
    let cells = (0..4).map(|i| match minis.get(i) {
        Some(Mini { icon: Some(url), .. }) => img(url.clone()).size(px(MINI)).squircle(ui::RADIUS_XS).into_any_element(),
        Some(Mini { letters, .. }) => letters_tile(letters, MINI, ui::RADIUS_XS, cx).into_any_element(),
        None => div().size(px(MINI)).squircle(ui::RADIUS_XS).bg(sidebar).into_any_element(),
    });
    div().absolute().left(px(MINI_INSET)).top(px(MINI_INSET)).w(px(2. * MINI + MINI_GAP)).flex().flex_wrap().gap(px(MINI_GAP)).children(cells)
}

// A server with no icon: its initials on the sidebar tone. Minis keep only the first letter.
fn letters_tile(label: &str, size: f32, radius: f32, cx: &App) -> Div {
    let label: String = if size < 24. { label.chars().take(1).collect() } else { label.to_owned() };
    div()
        .size(px(size))
        .flex_none()
        .squircle(radius)
        .bg(palette::surface(palette::Level::Raised, cx))
        .text_color(palette::strong(cx))
        // Minis keep a readable letter rather than scaling the tile's 14px down to nothing.
        .text_size(px((14. * size / TILE).max(8.)))
        .font_weight(FontWeight::SEMIBOLD)
        .flex()
        .items_center()
        .justify_center()
        .child(label)
}

// The hairline between home, the servers and the actions.
// Add and Discover: a quiet tile that lifts to the raised tone on hover.
// A server's hover card, as Discord's: its name, then who's talking in voice and who's live.
#[derive(Clone)]
struct ServerPreview {
    name: SharedString,
    voice: Vec<String>,
    live: Vec<String>,
}

impl ServerPreview {
    fn has_activity(&self) -> bool {
        !self.voice.is_empty() || !self.live.is_empty()
    }
}

impl Render for ServerPreview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let surface = palette::surface(palette::Level::Overlay, cx);
        let muted = palette::muted(cx);
        // Six faces at most per row; the card stays a glance, not a roster.
        let row = |glyph: Glyph, avatars: &[String]| {
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(Icon::new(glyph).size(px(20.)).text_color(muted))
                .child(ui::avatar_stack(avatars.iter().take(6).cloned(), 28., surface))
        };
        ui::surface(cx)
            .max_w(px(280.))
            .p(px(12.))
            .gap(px(10.))
            .child(div().truncate().text_size(px(16.)).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(self.name.clone()))
            .when(!self.voice.is_empty(), |c| c.child(row(Glyph::VolumeHigh, &self.voice)))
            .when(!self.live.is_empty(), |c| c.child(row(Glyph::ShareScreen, &self.live)))
    }
}

fn rail_action(id: &'static str, glyph: Nav, tooltip: &'static str, cx: &App) -> Stateful<Div> {
    let (raised, strong) = (palette::surface(palette::Level::Raised, cx), palette::strong(cx));
    div()
        .id(id)
        .size(px(TILE))
        .flex_none()
        .squircle(ui::RADIUS_LG)
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .bg(palette::surface(palette::Level::Raised, cx))
        .text_color(palette::muted(cx))
        .hover(move |s| s.bg(raised).text_color(strong))
        .child(glyph.icon().size(px(20.)))
        .tooltip(move |window, cx| Tooltip::new(tooltip).build(window, cx))
}

// The mention count on a tile's corner, ringed in the rail so it reads apart from the art.
fn rail_count(count: u32, scale: f32, rail: Hsla) -> Div {
    let ring = 3.;
    div()
        .absolute()
        .right(px(RAIL_WIDTH - TILE_X - TILE - 5. - ring))
        .bottom(px(-4. - ring))
        .p(px(ring))
        .rounded_full()
        .bg(rail)
        .scale(scale.max(0.))
        .child(ui::count_badge(count).min_w(px(18.)).h(px(18.)).px(px(5.)).text_size(px(12.)))
}

// Connected to voice here: the still wave in a rail-coloured capsule on the corner.
fn voice_mark(rail: Hsla, cx: &App) -> Div {
    div()
        .absolute()
        .right(px(RAIL_WIDTH - TILE_X - TILE - 7.))
        .bottom(px(-6.))
        .h(px(22.))
        .px(px(7.))
        .rounded_full()
        .bg(rail)
        .flex()
        .items_center()
        .child(ui::bars_still(palette::positive_ink(cx), 10.))
}

// Drop cues: a bar in the gap to reorder, a ring around the tile to combine.
fn drop_marker(drop: Drop) -> Div {
    let accent = palette::accent();
    let bar = || div().absolute().left(px((RAIL_WIDTH - 36.) / 2.)).w(px(36.)).h(px(4.)).rounded_full().bg(accent);
    match drop {
        Drop::Before => bar().top(px(-(RAIL_GAP + 4.) / 2.)),
        Drop::After => bar().bottom(px(-(RAIL_GAP + 4.) / 2.)),
        Drop::Into => div().absolute().left(px(TILE_X - 4.)).top(px(-4.)).size(px(TILE + 8.)).squircle(ui::RADIUS_LG + 4.).border_2().border_color(accent),
    }
}

// ICONS

// MOVE(icons): glyphs the navigation chrome needs that `icons::Glyph` lacks yet (window controls and
// voice marks included); promote each into `Glyph` and drop this.
#[derive(Clone, Copy)]
pub(crate) enum Nav {
    Megaphone,
    Media,
    Stage,
    BellSlash,
    Compass,
    Plus,
    Caret,
    MoreVertical,
    Close,
    MicOffFill,
    DeafenedFill,
    CameraFill,
    LiveFill,
    GameFill,
    MusicFill,
    TvFill,
    Mic,
    Headphones,
    HeadphonesOff,
    MicOff,
    Settings,
    Minimize,
    Maximize,
    Restore,
}

impl Nav {
    pub(crate) fn icon(self) -> Icon {
        Icon::default().data(self.svg())
    }

    fn svg(self) -> &'static [u8] {
        match self {
            Nav::Megaphone => include_bytes!("../assets/icons/reicon/bullhorn.svg"),
            Nav::Media => include_bytes!("../assets/icons/reicon/gallery.svg"),
            Nav::Stage => include_bytes!("../assets/icons/reicon/podcast.svg"),
            Nav::BellSlash => include_bytes!("../assets/icons/reicon/bell-off2.svg"),
            Nav::Compass => include_bytes!("../assets/icons/reicon/compass.svg"),
            Nav::Plus => include_bytes!("../assets/icons/reicon/add.svg"),
            Nav::Caret => include_bytes!("../assets/icons/reicon/chevron-down.svg"),
            Nav::MoreVertical => include_bytes!("../assets/icons/reicon/more-v.svg"),
            Nav::Close => include_bytes!("../assets/icons/reicon/xmark.svg"),
            Nav::MicOffFill => include_bytes!("../assets/icons/reicon/muted.svg"),
            Nav::DeafenedFill => include_bytes!("../assets/icons/reicon/deafened.svg"),
            Nav::CameraFill => include_bytes!("../assets/icons/reicon/video.svg"),
            Nav::LiveFill => include_bytes!("../assets/icons/reicon/signal-stream.svg"),
            Nav::GameFill => include_bytes!("../assets/icons/reicon/gamepad.svg"),
            Nav::MusicFill => include_bytes!("../assets/icons/reicon/music-notes.svg"),
            Nav::TvFill => include_bytes!("../assets/icons/reicon/tv.svg"),
            Nav::Mic => include_bytes!("../assets/icons/reicon/microphone2.svg"),
            Nav::Headphones => include_bytes!("../assets/icons/reicon/headphone.svg"),
            Nav::HeadphonesOff => include_bytes!("../assets/icons/reicon/deafened.svg"),
            Nav::MicOff => include_bytes!("../assets/icons/reicon/muted.svg"),
            Nav::Settings => include_bytes!("../assets/icons/reicon/gear2.svg"),
            Nav::Minimize => include_bytes!("../assets/icons/reicon/minus.svg"),
            Nav::Maximize => include_bytes!("../assets/icons/reicon/win-max.svg"),
            Nav::Restore => include_bytes!("../assets/icons/reicon/win-restore.svg"),
        }
    }
}

// An activity's kind as its filled glyph: what someone is playing, hearing, streaming or watching.
pub(crate) fn activity_glyph(kind: ActivityKind) -> Option<Icon> {
    match kind {
        ActivityKind::Playing | ActivityKind::Competing => Some(Nav::GameFill.icon()),
        ActivityKind::Listening => Some(Nav::MusicFill.icon()),
        ActivityKind::Streaming => Some(Nav::LiveFill.icon()),
        ActivityKind::Watching => Some(Nav::TvFill.icon()),
        ActivityKind::Custom => None,
    }
}

fn muted_line(text: &'static str, cx: &App) -> AnyElement {
    div().px_2().py_2().text_color(cx.theme().muted_foreground).child(text).into_any_element()
}

const STREAM_CARD_WIDTH: f32 = 260.;

struct Streamer<'a> {
    id: &'a str,
    name: &'a str,
    avatar: &'a str,
}

// The row and its card share one hover key, so moving from one onto the other keeps the card open.
fn stream_hover_key(user_id: &str) -> String {
    format!("stream-{user_id}")
}

// "Theo watching", "Theo and Ivy watching", "Theo, Ivy and Sam watching", "Theo, Ivy and 2 others watching".
fn viewer_line(names: &[String]) -> Option<String> {
    let who = match names {
        [] => return None,
        [one] => one.clone(),
        [a, b] => format!("{a} and {b}"),
        [a, b, c] => format!("{a}, {b} and {c}"),
        [a, b, rest @ ..] => format!("{a}, {b} and {} others", rest.len()),
    };
    Some(format!("{who} watching"))
}

// The shared app and how long it's been live, "Blender · 24 min"; either half may be missing.
fn stream_meta(app: Option<&str>, live: Option<Duration>) -> Option<String> {
    let parts: Vec<String> = app.map(str::to_owned).into_iter().chain(live.map(live_for)).collect();
    (!parts.is_empty()).then(|| parts.join(" · "))
}

fn live_for(live: Duration) -> String {
    let minutes = live.as_secs() / 60;
    match (minutes / 60, minutes % 60) {
        (0, 0) => "Just started".into(),
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} hr"),
        (h, m) => format!("{h} hr {m} min"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewer_line_names_two_then_counts() {
        let names = |n: &[&str]| n.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(viewer_line(&[]), None);
        assert_eq!(viewer_line(&names(&["Theo"])).as_deref(), Some("Theo watching"));
        assert_eq!(viewer_line(&names(&["Theo", "Ivy"])).as_deref(), Some("Theo and Ivy watching"));
        assert_eq!(viewer_line(&names(&["Theo", "Ivy", "Sam"])).as_deref(), Some("Theo, Ivy and Sam watching"));
        assert_eq!(viewer_line(&names(&["Theo", "Ivy", "Sam", "Ada"])).as_deref(), Some("Theo, Ivy and 2 others watching"));
    }

    #[test]
    fn live_for_drops_zero_units() {
        assert_eq!(live_for(Duration::from_secs(59)), "Just started");
        assert_eq!(live_for(Duration::from_secs(24 * 60 + 59)), "24 min");
        assert_eq!(live_for(Duration::from_secs(2 * 3600)), "2 hr");
        assert_eq!(live_for(Duration::from_secs(3600 + 5 * 60)), "1 hr 5 min");
    }

    #[test]
    fn stream_meta_joins_what_it_has() {
        assert_eq!(stream_meta(Some("Blender"), Some(Duration::from_secs(24 * 60))).as_deref(), Some("Blender · 24 min"));
        assert_eq!(stream_meta(None, Some(Duration::from_secs(24 * 60))).as_deref(), Some("24 min"));
        assert_eq!(stream_meta(Some("Blender"), None).as_deref(), Some("Blender"));
        assert_eq!(stream_meta(None, None), None);
    }
}

// The open server's name as one button that drops the server menu: the channel list's header (16px), or,
// `compact`, the macOS strip beside the traffic lights (14px).
pub(crate) fn server_name_button(name: SharedString, compact: bool, cx: &mut Context<Rustcord>) -> Stateful<Div> {
    let hover = palette::hover(cx);
    let size = if compact { 14. } else { 16. };
    div()
        .id("server-header")
        .occlude()
        .min_w_0()
        .when(compact, |b| b.h(px(28.)))
        .when(!compact, |b| b.flex_1().h_full())
        .px_2()
        .squircle(ui::RADIUS_SM)
        .flex()
        .items_center()
        .gap(px(if compact { 4. } else { 8. }))
        .cursor_pointer()
        .text_size(px(size))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(palette::strong(cx))
        .hover(move |s| s.bg(hover))
        .child(div().when(!compact, |d| d.flex_1()).min_w_0().truncate().child(name))
        .child(Nav::Caret.icon().size(px(size)).text_color(palette::muted(cx)))
        .on_mouse_down(gpui_kit::MouseButton::Left, cx.listener(move |this, _: &gpui_kit::MouseDownEvent, window, cx| {
            let Some(guild_id) = this.guild_id.clone() else { return };
            // Drops from the button's bottom edge: in the strip under the name, else under the list's header.
            let at = match compact {
                true => gpui_kit::point(px(crate::layout::TRAFFIC_LIGHTS), px(crate::layout::BRAND_BAR_HEIGHT - 4.)),
                false => gpui_kit::point(px(RAIL_WIDTH + 6.), px(crate::layout::brand_bar_height(window) + crate::layout::HEADER_HEIGHT)),
            };
            this.open_server_menu(guild_id, at, true, cx);
        }))
}
