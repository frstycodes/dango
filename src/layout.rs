// Wave's frame (design/navigation.html): a title bar over the rail, channel list, chat and side panel,
// with the user area at the foot of the channel list.
use std::time::Duration;

use gpui_kit::{
    Animation, AnimationExt, AnyElement, App, Context, Div, FontWeight, Hsla, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent,
    ObjectFit, Render, SharedString, Stateful, Window, WindowControlArea,
    component::{ActiveTheme, Icon, tooltip::Tooltip},
    div, img, linear_color_stop, linear_gradient, prelude::*, px,
};

use crate::{
    Rustcord, Drag, Edge, Status,
    discord::{self, MediaKind},
    icons::Glyph,
    login, palette,
    sidebar::{Nav, render_face, server_face},
    ui::{self, Squircle},
};

pub const TITLE_HEIGHT: f32 = 36.;
pub const RAIL_WIDTH: f32 = 68.;
pub const CHANNELS_WIDTH: f32 = 240.;
pub const PANEL_WIDTH: f32 = 264.;
// The user area: 8px over and 10px under its 32px row.
pub const PANEL_HEIGHT: f32 = 50.;
// The room macOS's traffic lights take at the title bar's start (12px lights 8px apart, from x 12).
pub const TRAFFIC_LIGHTS: f32 = 72.;
const CHANNELS_RANGE: (f32, f32) = (200., 440.);
const PANEL_RANGE: (f32, f32) = (200., 380.);

impl Render for Rustcord {
    // Everything draws through the app's image cache, which frees images that are no longer drawn.
    // Out of sight (minimized, in the tray or fully covered) and no video playing, nothing is drawn at
    // all: incoming events keep updating the app's data, but nothing is laid out, decoded or touched
    // until the window can be seen again, so memory given back in the background stays given back.
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if crate::presence::out_of_sight() && !crate::video::is_playing(cx) {
            return div().into_any_element();
        }
        let images = crate::images::cache(cx);
        images.update(cx, |images, cx| images.next_frame(cx));
        gpui_kit::image_cache(images).size_full().child(self.render_root(window, cx)).into_any_element()
    }
}

impl Rustcord {
    fn render_root(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        // Before the rows draw, so a row just let go of draws as landing rather than as a hole.
        self.settle_carry(cx);
        let theme = cx.theme().clone();
        let root = div()
            .id("chord")
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .bg(theme.title_bar)
            .text_color(theme.foreground)
            .text_size(px(16.))
            .on_key_down(cx.listener(Self::on_root_key))
            .on_modifiers_changed(cx.listener(|this, e: &gpui_kit::ModifiersChangedEvent, _, cx| {
                if this.shift_held != e.modifiers.shift {
                    this.shift_held = e.modifiers.shift;
                    cx.notify();
                }
            }))
            .on_mouse_move(cx.listener(Self::on_drag_move))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, cx| {
                if this.drag.take().is_some() {
                    cx.notify();
                }
            }));
        self.restore_last_channel(window, cx);
        if let Status::SignedOut(login) = &self.status {
            let login = login.clone();
            let back = self.prefs.account.as_ref().and_then(|id| self.prefs.accounts.iter().find(|a| &a.id == id)).cloned();
            return root
                .bg(theme.background)
                .child(login::render(&login, back, window, cx))
                .children(crate::window_controls::render(window, cx))
                .into_any_element();
        }
        if let Some(video) = crate::video::fullscreen(cx) {
            return root.child(video).into_any_element();
        }
        if self.call_fullscreen && self.call_view
            && let Some(call) = self.render_call_with_chat(window, cx)
        {
            return root
                .child(call)
                .children(self.render_overlays(window, cx))
                .children(self.render_call_overlays(window, cx))
                .children(self.render_toast(cx))
                .children(self.render_menu(window, cx))
                .children(self.render_dialog(cx))
                .into_any_element();
        }
        let body = div()
            .relative()
            .flex_1()
            .min_h_0()
            .flex()
            .child(self.render_nav(window, cx))
            .child(match self.call_view.then(|| self.render_call_with_chat(window, cx)).flatten() {
                Some(call) => div().flex_1().min_w_0().h_full().child(call).into_any_element(),
                None if self.guild_id.is_none() && self.channel.is_none() => self.render_home(cx),
                None if self.channel.as_ref().is_some_and(|c| c.kind == discord::ChannelKind::Forum) => {
                    let forum = self.channel.clone().expect("checked above");
                    self.render_forum(forum, cx)
                }
                None => self.render_chat(window, cx).into_any_element(),
            });
        root.child(self.title_bar(cx))
            .child(body)
            .children(self.render_overlays(window, cx))
            .children(self.render_call_overlays(window, cx))
            .children(self.render_toast(cx))
            .children(self.render_menu(window, cx))
            .children(self.render_dialog(cx))
            .children(self.viewer_overlay(window, cx))
            .children(self.render_settings(window, cx))
            .children(crate::window_controls::render(window, cx))
            .children(self.render_landing(window, cx))
            .into_any_element()
    }
}

impl Rustcord {
    fn on_root_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        if self.record_keystroke(keystroke, cx) {
            return cx.stop_propagation();
        }
        if keystroke.key == "escape" && self.call_fullscreen {
            return self.set_call_fullscreen(false, window, cx);
        }
        if keystroke.key == "escape" && self.settings.is_some() {
            self.close_settings(cx);
            return cx.stop_propagation();
        }
        if keystroke.key == "k" && (keystroke.modifiers.platform || keystroke.modifiers.control) {
            self.open_switcher(window, cx);
            return cx.stop_propagation();
        }
        if keystroke.key == "escape" && self.viewer.is_none() && self.dismiss_overlay(cx) {
            return cx.stop_propagation();
        }
        if keystroke.modifiers.alt && (keystroke.key == "up" || keystroke.key == "down") {
            self.step_channel(if keystroke.key == "up" { -1 } else { 1 }, window, cx);
            return cx.stop_propagation();
        }
        // Esc with nothing else to close marks the channel read and returns to the newest message, as Discord does.
        if keystroke.key == "escape" && self.viewer.is_none() {
            if let Some(id) = self.channel.as_ref().map(|c| c.id.clone()) {
                self.mark_channel_read(&id, cx);
                self.unread_marker = None;
                self.list.scroll_to_end();
            }
            return;
        }
        if self.viewer.is_none() {
            return;
        }
        match event.keystroke.key.as_str() {
            "left" => self.step_viewer(-1, cx),
            "right" => self.step_viewer(1, cx),
            "escape" => {
                self.viewer = None;
                cx.notify();
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    fn on_drag_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(drag) = &self.drag else { return };
        let dx = f32::from(event.position.x) - drag.start_x;
        match drag.edge {
            Edge::Channels => self.channels_width = (drag.start_width + dx).clamp(CHANNELS_RANGE.0, CHANNELS_RANGE.1),
            // The member list grows leftward.
            Edge::Panel => self.panel_width = (drag.start_width - dx).clamp(PANEL_RANGE.0, PANEL_RANGE.1),
        }
        cx.notify();
    }

    /// An 8px grab strip over a column's edge drawing a 2px line: accent at 60% on hover, full while
    /// dragging (.panel-grip). Dragging it resizes the column.
    pub(crate) fn resize_handle(&self, edge: Edge, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.drag.as_ref().is_some_and(|d| d.edge == edge);
        let accent = palette::accent();
        div()
            .id(match edge {
                Edge::Channels => "resize-channels",
                Edge::Panel => "resize-panel",
            })
            .absolute()
            .top_0()
            .bottom_0()
            .w(px(8.))
            .when(edge == Edge::Channels, |h| h.right(px(-4.)))
            .when(edge == Edge::Panel, |h| h.left(px(-4.)))
            .group("resize-grip")
            .cursor_col_resize()
            .child(div().ml(px(3.)).w(px(2.)).h_full().rounded(px(2.)).when(active, |l| l.bg(accent)).when(!active, |l| l.group_hover("resize-grip", |l| l.bg(accent.opacity(0.6)))))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    let start_width = if edge == Edge::Channels { this.channels_width } else { this.panel_width };
                    this.drag = Some(Drag { edge, start_x: f32::from(event.position.x), start_width });
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
    }

    // The window's own chrome (.titlebar): history arrows lead, the current place sits centred, inbox and
    // help close the bar. The whole bar drags the window.
    fn title_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (mark, title): (AnyElement, SharedString) = match self.guild() {
            Some(guild) => (render_face(server_face(guild), 20., ui::RADIUS_XS, false, 0., cx), guild.name.clone().into()),
            None if self.channel.is_none() => (Icon::new(Glyph::Users).size(px(18.)).text_color(palette::faint(cx)).into_any_element(), "Friends".into()),
            None => (Icon::new(Glyph::ChatRound).size(px(18.)).text_color(palette::faint(cx)).into_any_element(), "Direct messages".into()),
        };
        let mentions = self.read_states.values().any(|r| r.mentions > 0);
        let back = title_button("back", Glyph::ArrowLeft, !self.back.is_empty(), "Back", cx).on_click(cx.listener(|this, _, window, cx| this.navigate(false, window, cx)));
        let forward = title_button("forward", Glyph::ArrowRight, !self.forward.is_empty(), "Forward", cx).on_click(cx.listener(|this, _, window, cx| this.navigate(true, window, cx)));
        let inbox_open = self.inbox.is_some();
        let (selected, accent) = (palette::selected(cx), palette::accent());
        let inbox = title_button("inbox", Glyph::Inbox, true, "Inbox", cx)
            .when(inbox_open, |b| b.bg(selected).text_color(palette::strong(cx)))
            .when(mentions, |b| b.child(div().absolute().top(px(4.)).right(px(4.)).size(px(7.)).rounded_full().bg(accent)))
            .on_click(cx.listener(|this, _, _, cx| this.open_inbox(cx)));
        div()
            .relative()
            .h(px(TITLE_HEIGHT))
            .flex_none()
            .px_2()
            .flex()
            .items_center()
            .gap_1()
            .text_size(px(14.))
            .window_control_area(WindowControlArea::Drag)
            // macOS draws its traffic lights in the first 72px; Windows leads with the app's name instead.
            .map(|bar| match cfg!(target_os = "macos") {
                true => bar.child(div().w(px(TRAFFIC_LIGHTS - 8.)).flex_none()),
                false => bar.child(brand(cx)),
            })
            .child(back)
            .child(forward)
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top_0()
                    .bottom_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .max_w(gpui_kit::relative(0.4))
                            .flex()
                            .items_center()
                            .gap_2()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(palette::strong(cx))
                            .child(mark)
                            .child(div().min_w_0().truncate().child(title)),
                    ),
            )
            .child(div().ml_auto().mr(px(crate::window_controls::WIDTH)).flex().items_center().gap_1().child(inbox).child(
                title_button("help", Glyph::Help, true, "Help", cx).on_click(|_, _, cx| cx.open_url("https://support.discord.com")),
            ))
    }

    // The user area (.me), at the foot of the channel list: who you are, how you look to others, and
    // mute, deafen and settings. Mute and deafen turn coral when off; right-click either for devices.
    // In a call, the voice panel sits on top of it inside the same card.
    pub(crate) fn user_panel(&mut self, me: discord::User, cx: &mut Context<Self>) -> AnyElement {
        let rail = palette::surface(palette::Level::Rail, cx);
        let sharing = self.call.as_ref().is_some_and(|c| c.sharing.is_some());
        let server_muted = self.call.as_ref().and_then(|c| self.guilds.iter().find(|g| Some(&g.id) == c.guild_id.as_ref())).and_then(|g| g.voice_states.iter().find(|v| v.user_id == me.id)).is_some_and(|v| v.server_muted);
        let (muted, deafened) = (self.muted || self.deafened || server_muted, self.deafened);
        let (faint, danger) = (palette::faint(cx), palette::danger_ink(cx));
        let status_line: AnyElement = match (server_muted, sharing) {
            (true, _) => div().truncate().text_color(danger).child("Server muted by a moderator").into_any_element(),
            (false, true) => div().flex().items_center().gap_1().child(Icon::new(Glyph::Screen).size(px(13.)).text_color(palette::positive_ink(cx))).child(div().truncate().child("Sharing their screen")).into_any_element(),
            (false, false) => div().truncate().child(self.custom_status.clone().unwrap_or_else(|| self.my_status.label().into())).into_any_element(),
        };
        let mic = me_button("mic", if muted { Nav::MicOff } else { Nav::Mic }, muted, if server_muted { "A moderator muted you here. Only they can unmute you." } else if muted { "Unmute" } else { "Mute" }, cx)
            .when(!server_muted, |b| b.on_click(cx.listener(|this, _, _, cx| this.toggle_mute(cx))))
            .on_mouse_down(MouseButton::Right, cx.listener(|this, e: &MouseDownEvent, _, cx| this.open_device_menu(crate::call::DeviceKind::Input, e.position, cx)));
        let headphones = me_button("deafen", if deafened { Nav::HeadphonesOff } else { Nav::Headphones }, deafened, if deafened { "Undeafen" } else { "Deafen" }, cx)
            .on_click(cx.listener(|this, _, _, cx| this.toggle_deafen(cx)))
            .on_mouse_down(MouseButton::Right, cx.listener(|this, e: &MouseDownEvent, _, cx| this.open_device_menu(crate::call::DeviceKind::Output, e.position, cx)));
        let settings = me_button("settings", Nav::Settings, false, "User settings", cx).on_click(cx.listener(|this, _, window, cx| this.open_settings(crate::settings::Page::Account, window, cx)));
        // The card's bottom corners, and its top ones too when no voice panel sits above; outside them, the sidebar.
        let in_call = self.voice_panel_height() > 0.;
        let corners = [0., if in_call { 0. } else { ui::RADIUS_MD }, ui::RADIUS_MD, 0.];
        let behind = palette::surface(palette::Level::Sidebar, cx);
        let nameplate = me.nameplate.as_ref().map(|plate| nameplate(plate, rail, behind, corners, false, cx.theme().is_dark()));
        let has_plate = nameplate.is_some();
        let hover = palette::hover(cx);
        let who = div()
            .id("me")
            .flex_1()
            .min_w_0()
            .p_1()
            .my(px(-4.))
            .ml(px(-4.))
            .squircle(ui::RADIUS_SM)
            .flex()
            .items_center()
            .gap_2()
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .when(self.status_menu, |w| w.bg(hover))
            .on_click(cx.listener(|this, _, _, cx| {
                this.status_menu = !this.status_menu;
                cx.notify();
            }))
            .child(decorated_avatar(&me.avatar, me.decoration.as_deref(), 32., Some(self.my_status), rail))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(div().truncate().font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(me.name.clone()))
                    .child(div().text_size(px(12.)).text_color(faint).child(status_line)),
            );
        let voice = self.voice_panel(cx);
        div()
            .absolute()
            .left_2()
            .right_2()
            .bottom_2()
            .squircle(ui::RADIUS_MD)
            .border_1()
            .border_color(palette::line(cx))
            .overflow_hidden()
            .bg(rail)
            .flex()
            .flex_col()
            .text_size(px(14.))
            .children(voice)
            .child(
                div()
                    .relative()
                    .h(px(PANEL_HEIGHT))
                    .flex_none()
                    .children(nameplate)
                    .child(
                        div()
                            .relative()
                            .size_full()
                            .pt_2()
                            .px_2()
                            .pb(px(10.))
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(who)
                            // Over nameplate art the controls sit on frosted glass, so they keep their contrast.
                            .child(div().flex().items_center().gap_1().when(has_plate, |d| d.squircle(ui::RADIUS_SM).bg(rail.opacity(0.72)).backdrop_blur(px(12.))).child(mic).child(headphones).child(settings)),
                    ),
            )
            .into_any_element()
    }

    // The rail and the channel list, with the user area floating across the foot of both.
    fn render_nav(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let user_panel = self.me.clone().map(|me| self.user_panel(me, cx));
        div()
            .relative()
            .h_full()
            .flex_none()
            .flex()
            .child(self.render_rail(window, cx))
            .child(self.render_channels(window, cx))
            .children(user_panel)
    }

    /// Room the columns leave at their foot so their last rows scroll clear of the user area.
    pub(crate) fn user_panel_clearance(&self) -> gpui_kit::Pixels {
        match self.me.is_some() {
            true => px(self.panel_stack_height() + 16.),
            false => px(0.),
        }
    }

    /// The user area's height with the voice panel above it, for overlays that open just over it.
    pub fn panel_stack_height(&self) -> f32 {
        PANEL_HEIGHT + self.voice_panel_height()
    }

    // A gallery over the whole window: every image and video in the channel, with arrows and ←/→ to page.
    fn viewer_overlay(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let viewer = self.viewer.as_ref()?;
        let item = viewer.items.get(viewer.index)?.clone();
        let (index, count) = (viewer.index, viewer.items.len());
        let viewport = window.viewport_size();
        let (max_w, max_h) = (f32::from(viewport.width) - 200., f32::from(viewport.height) - 160.);
        let scale = (max_w / item.width).min(max_h / item.height).min(2.);
        let (w, h) = (item.width * scale, item.height * scale);
        let url = item.url.clone();
        let stage = match &item.kind {
            MediaKind::Image => crate::chat::animated_image(item.url.clone()).w(px(w)).h(px(h)).squircle(ui::RADIUS_MD).into_any_element(),
            MediaKind::Gif { poster } => div().w(px(w)).h(px(h)).squircle(ui::RADIUS_MD).overflow_hidden().child(crate::gif::tile(format!("viewer-{}", item.url), &item.url, poster, cx)).into_any_element(),
            // ponytail: video plays in the system player; an in-app AVPlayer view is the upgrade.
            MediaKind::Video { poster } => {
                let open = item.url.clone();
                div()
                    .id("viewer-video")
                    .relative()
                    .w(px(w))
                    .h(px(h))
                    .cursor_pointer()
                    .child(img(poster.clone()).size_full().squircle(ui::RADIUS_MD))
                    .child(
                        div().absolute().inset_0().flex().items_center().justify_center().child(
                            div()
                                .size(px(72.))
                                .rounded_full()
                                .bg(gpui_kit::black().opacity(0.6))
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_color(gpui_kit::white())
                                .child(Icon::new(Glyph::Play).size(px(36.))),
                        ),
                    )
                    .on_click(move |_, _, cx| cx.open_url(&open))
                    .into_any_element()
            }
        };
        let stage = div()
            .id("viewer-stage")
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(stage)
            .with_animation(SharedString::from(format!("viewer-{index}")), Animation::new(Duration::from_millis(140)), |el, t| el.opacity(t));
        let nav = |id: &'static str, glyph: Glyph, enabled: bool, delta: isize| {
            div()
                .id(id)
                .size(px(48.))
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(gpui_kit::white().opacity(if enabled { 0.12 } else { 0.04 }))
                .text_color(gpui_kit::white().opacity(if enabled { 1. } else { 0.3 }))
                .when(enabled, |b| b.cursor_pointer().hover(|s| s.bg(gpui_kit::white().opacity(0.2))))
                .child(Icon::new(glyph).size(px(26.)))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _, _, cx| this.step_viewer(delta, cx)))
        };
        let top = div()
            .absolute()
            .top(px(TITLE_HEIGHT + 8.))
            .right(px(16.))
            .flex()
            .gap_2()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .id("viewer-open")
                    .h(px(32.))
                    .px_3()
                    .squircle(ui::RADIUS_SM)
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .cursor_pointer()
                    .text_color(gpui_kit::white())
                    .bg(gpui_kit::white().opacity(0.12))
                    .hover(|s| s.bg(gpui_kit::white().opacity(0.2)))
                    .child(Icon::new(Glyph::Open).size(px(16.)))
                    .child("Open in Browser")
                    .on_click(move |_, _, cx| cx.open_url(&url)),
            )
            .child(
                div()
                    .id("viewer-close")
                    .size(px(32.))
                    .squircle(ui::RADIUS_SM)
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .text_color(gpui_kit::white())
                    .bg(gpui_kit::white().opacity(0.12))
                    .hover(|s| s.bg(gpui_kit::white().opacity(0.2)))
                    .child(Icon::new(Glyph::X).size(px(18.)))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.viewer = None;
                        cx.notify();
                    })),
            );
        Some(
            div()
                .id("viewer")
                .absolute()
                .occlude()
                .inset_0()
                .bg(gpui_kit::black().opacity(0.88))
                .flex()
                .items_center()
                .justify_between()
                .px_6()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        this.viewer = None;
                        cx.notify();
                    }),
                )
                .child(nav("viewer-prev", Glyph::ChevronLeft, index > 0, -1))
                .child(stage)
                .child(nav("viewer-next", Glyph::ChevronRight, index + 1 < count, 1))
                .child(top)
                .child(div().absolute().bottom(px(20.)).left_0().right_0().flex().justify_center().text_color(gpui_kit::white().opacity(0.8)).child(format!("{} / {count}", index + 1)))
                .with_animation("viewer-open", Animation::new(Duration::from_millis(160)), |el, t| el.opacity(t))
                .into_any_element(),
        )
    }
}

/// A collectible nameplate behind a row, as Discord draws it: the palette colour as a left-to-right tint
/// (10% → 40%) across the whole card, and the art on top, contain-fit against the right edge. In the user
/// panel the art sits at 40%; in the member list it is full strength but held back to ~30% under the name,
/// rising to full ~50px past it (Discord masks it; gpui has no masks, so a surface wash does the same).
/// `corners` are the radii of the shape it fills (top-left, top-right, bottom-right, bottom-left), and
/// `behind` the colour just outside that shape.
pub fn nameplate(plate: &discord::Nameplate, surface: Hsla, behind: Hsla, corners: [f32; 4], member_row: bool, dark: bool) -> AnyElement {
    let tint: Hsla = gpui_kit::rgb(if dark { plate.tint.0 } else { plate.tint.1 }).into();
    let art = img(plate.art.clone()).size_full().object_fit(ObjectFit::Cover).opacity(if member_row { 1. } else { 0.4 });
    div()
        .absolute()
        .inset_0()
        .when(!member_row, |p| p.left(px(50.)))
        .overflow_hidden()
        .bg(linear_gradient(90., linear_color_stop(tint.opacity(0.1), 0.), linear_color_stop(tint.opacity(0.4), 1.)))
        .child(art)
        // A wash of the surface from the left: under the name in a member row, and in the user panel the
        // art's own left edge, which would otherwise stop in a hard line.
        .child(div().absolute().inset_0().bg(match member_row {
            true => linear_gradient(90., linear_color_stop(surface.opacity(0.7), 0.45), linear_color_stop(surface.opacity(0.), 0.75)),
            false => linear_gradient(90., linear_color_stop(surface, 0.), linear_color_stop(surface.opacity(0.), 0.5)),
        }))
        .child(corner_frame(behind, corners))
        .into_any_element()
}

// Rounds an image-filled box's corners. gpui clips only to rectangles, and a covering image paints past its
// own bounds, so its square corners would poke out of the row's rounded shape. A frame in the colour
// behind, set just outside the box, has an inner edge that is the row's corner and paints over them.
fn corner_frame(behind: Hsla, [tl, tr, br, bl]: [f32; 4]) -> Div {
    const FRAME: f32 = 16.;
    div()
        .absolute()
        .top(px(-FRAME))
        .left(px(-FRAME))
        .right(px(-FRAME))
        .bottom(px(-FRAME))
        .border(px(FRAME))
        .border_color(behind)
        .rounded_tl(px(FRAME + tl))
        .rounded_tr(px(FRAME + tr))
        .rounded_br(px(FRAME + br))
        .rounded_bl(px(FRAME + bl))
        .corner_shape(ui::SQUIRCLE)
}

/// An avatar with Discord's status shape cut into its bottom-right corner: a dot for online, a moon for
/// idle, a bar for do-not-disturb, a ring for offline. Cut-outs are painted in the surface colour.
pub fn avatar_with_status(url: &str, size: f32, status: Option<discord::Status>, surface: Hsla) -> AnyElement {
    decorated_avatar(url, None, size, status, surface)
}

/// The same, with the user's avatar decoration framing it (drawn at 1.2× the avatar, as Discord does).
pub fn decorated_avatar(url: &str, decoration: Option<&str>, size: f32, status: Option<discord::Status>, surface: Hsla) -> AnyElement {
    // .status: 30% of the avatar (9..16px), ringed 2px in the surface, its corner just past the avatar's.
    let dot = (size * 0.3).clamp(9., 16.).round();
    let inset = size * 0.04 - 2. - STATUS_RING;
    let frame = size * 1.2;
    div()
        .relative()
        .size(px(size))
        .flex_none()
        .child(img(url.to_owned()).size_full().rounded_full())
        .children(decoration.map(|d| img(d.to_owned()).absolute().size(px(frame)).left(px((size - frame) / 2.)).top(px((size - frame) / 2.))))
        .children(status.map(|status| {
            div()
                .absolute()
                .right(px(inset))
                .bottom(px(inset))
                .size(px(dot + 2. * STATUS_RING))
                .rounded_full()
                .bg(surface)
                .flex()
                .items_center()
                .justify_center()
                .child(status_mark(status, dot, surface))
        }))
        .into_any_element()
}

const STATUS_RING: f32 = 2.;

pub fn status_mark(status: discord::Status, dot: f32, surface: Hsla) -> AnyElement {
    let mark = div().relative().size(px(dot)).rounded_full().bg(palette::status(status)).overflow_hidden();
    let cut = |w: f32, h: f32| div().absolute().w(px(w)).h(px(h)).bg(surface);
    match status {
        discord::Status::Online => mark,
        discord::Status::Idle => mark.child(cut(dot * 0.62, dot * 0.62).rounded_full().left(px(-dot * 0.12)).top(px(-dot * 0.12))),
        discord::Status::Dnd => mark.flex().items_center().justify_center().child(cut(dot * 0.62, (dot * 0.24).max(2.)).relative().rounded(px(dot))),
        discord::Status::Offline => mark.flex().items_center().justify_center().child(cut(dot * 0.46, dot * 0.46).relative().rounded_full()),
    }
    .into_any_element()
}

// A title bar control (.titlebar .iconbtn): 28px, quiet until hovered; disabled ones fade to 40%.
// Each occludes the bar's drag area: on Windows a drag area is the native caption, which takes the
// click before the app sees it.
fn title_button(id: &'static str, glyph: Glyph, enabled: bool, label: &'static str, cx: &App) -> Stateful<Div> {
    let (hover, strong) = (palette::hover(cx), palette::strong(cx));
    div()
        .id(id)
        .occlude()
        .relative()
        .size(px(28.))
        .flex_none()
        .squircle(ui::RADIUS_SM)
        .flex()
        .items_center()
        .justify_center()
        .text_color(palette::muted(cx))
        .when(!enabled, |b| b.opacity(0.4))
        .when(enabled, |b| b.cursor_pointer().hover(move |s| s.bg(hover).text_color(strong)))
        .child(Icon::new(glyph).size(px(18.)))
        .tooltip(move |window, cx| Tooltip::new(label).build(window, cx))
}

// Wave's mark and name, leading the bar where the system draws no traffic lights.
fn brand(cx: &App) -> Div {
    div()
        .flex_none()
        .pl_1()
        .pr_2()
        .flex()
        .items_center()
        .gap_2()
        .font_weight(FontWeight::MEDIUM)
        .text_color(palette::strong(cx))
        .child(img(crate::icons::MARK).size(px(18.)))
        .child("Wave")
}

// A user area control (.me .iconbtn): 30px; `on` (muted, deafened) turns it coral. The glyph pops in on
// every flip, keyed by state.
fn me_button(id: &'static str, glyph: Nav, on: bool, label: &'static str, cx: &App) -> Stateful<Div> {
    let (hover, strong) = (palette::hover(cx), palette::strong(cx));
    let icon = glyph.icon().size(px(20.)).with_animation(SharedString::from(format!("{id}-{on}")), Animation::new(ui::DUR_POP).with_easing(crate::motion::back_out), |icon, t| icon.size(px(20. * (0.7 + 0.3 * t))));
    div()
        .id(id)
        .size(px(30.))
        .flex_none()
        .squircle(ui::RADIUS_SM)
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .text_color(if on { palette::danger_ink(cx) } else { palette::muted(cx) })
        .hover(move |s| if on { s.bg(hover) } else { s.bg(hover).text_color(strong) })
        .child(icon)
        .tooltip(move |window, cx| Tooltip::new(label).build(window, cx))
}
