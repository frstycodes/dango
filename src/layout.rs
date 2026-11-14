// Discord's frame: a title bar over the rail, channel list, chat and right panel, with the
// user panel floating across the bottom of the rail and channel list.
use std::time::Duration;

use gpui_kit::{
    Animation, AnimationExt, AnyElement, Context, FontWeight, Hsla, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent,
    ObjectFit, Render, SharedString, Window, WindowControlArea,
    component::{ActiveTheme, Icon, tooltip::Tooltip},
    div, img, linear_color_stop, linear_gradient, prelude::*, px,
};

use crate::{
    Rustcord, Drag, Edge, Status,
    discord::{self, MediaKind},
    icons::Glyph,
    login, palette,
};

pub const TITLE_HEIGHT: f32 = 32.;
pub const RAIL_WIDTH: f32 = 72.;
pub const CHANNELS_WIDTH: f32 = 264.;
pub const PANEL_WIDTH: f32 = 264.;
pub const PANEL_HEIGHT: f32 = 58.;
const CHANNELS_RANGE: (f32, f32) = (200., 440.);
const PANEL_RANGE: (f32, f32) = (200., 380.);

impl Render for Rustcord {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let root = div()
            .id("chord")
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .bg(theme.sidebar)
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
            return root
                .bg(theme.background)
                .child(login::render(&login, window, cx))
                .children(crate::window_controls::render(window, cx))
                .into_any_element();
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
            .border_t_1()
            .border_color(theme.border)
            .child(self.render_rail(window, cx))
            .child(self.render_channels(window, cx))
            .child(match self.call_view.then(|| self.render_call_with_chat(window, cx)).flatten() {
                Some(call) => div().flex_1().min_w_0().h_full().child(call).into_any_element(),
                None if self.guild_id.is_none() && self.channel.is_none() => self.render_home(cx),
                None if self.channel.as_ref().is_some_and(|c| c.kind == discord::ChannelKind::Forum) => {
                    let forum = self.channel.clone().expect("checked above");
                    self.render_forum(forum, cx)
                }
                None => self.render_chat(window, cx).into_any_element(),
            })
            .children(self.me.clone().map(|me| self.user_panel(me, cx)));
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

    /// A thin grab strip on a column's edge; dragging it resizes the column, as in Discord.
    pub(crate) fn resize_handle(&self, edge: Edge, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.drag.as_ref().is_some_and(|d| d.edge == edge);
        let accent = palette::blurple();
        div()
            .id(match edge {
                Edge::Channels => "resize-channels",
                Edge::Panel => "resize-panel",
            })
            .absolute()
            .top_0()
            .bottom_0()
            .w(px(6.))
            .when(edge == Edge::Channels, |h| h.right(px(-3.)))
            .when(edge == Edge::Panel, |h| h.left(px(-3.)))
            .cursor_col_resize()
            .child(div().mx_auto().w(px(2.)).h_full().when(active, |l| l.bg(accent)).hover(|l| l.bg(accent.opacity(0.6))))
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

    fn title_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let (icon, title): (AnyElement, SharedString) = match self.guild() {
            Some(guild) => (
                match &guild.icon {
                    Some(url) => img(url.clone()).size(px(18.)).rounded(px(5.)).into_any_element(),
                    None => crate::sidebar::identity_tile(&guild.id, crate::sidebar::initials(&guild.name), 8.).size(px(18.)).rounded(px(5.)).into_any_element(),
                },
                guild.name.clone().into(),
            ),
            None if self.channel.is_none() => (Icon::new(Glyph::Friends).size(px(16.)).text_color(theme.muted_foreground).into_any_element(), "Friends".into()),
            None => (Icon::new(Glyph::ChatRound).size(px(16.)).text_color(theme.muted_foreground).into_any_element(), "Direct Messages".into()),
        };
        let arrow = |id: &'static str, glyph: Glyph, enabled: bool, label: &'static str| {
            div()
                .id(id)
                .size(px(26.))
                .rounded(px(6.))
                .flex()
                .items_center()
                .justify_center()
                .text_color(if enabled { theme.foreground.opacity(0.8) } else { theme.muted_foreground.opacity(0.4) })
                .when(enabled, |b| b.cursor_pointer().hover(|s| s.bg(theme.foreground.opacity(0.08))))
                .child(Icon::new(glyph).size(px(18.)))
                .tooltip(move |window, cx| Tooltip::new(label).build(window, cx))
        };
        let back = arrow("back", Glyph::ArrowLeft, !self.back.is_empty(), "Back").on_click(cx.listener(|this, _, window, cx| this.navigate(false, window, cx)));
        let forward = arrow("forward", Glyph::ArrowRight, !self.forward.is_empty(), "Forward").on_click(cx.listener(|this, _, window, cx| this.navigate(true, window, cx)));
        div()
            .relative()
            .h(px(TITLE_HEIGHT))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .window_control_area(WindowControlArea::Drag)
            .child(div().absolute().left(px(84.)).flex().gap_1().child(back).child(forward))
            .child(
                div()
                    .absolute()
                    .right(px(12. + crate::window_controls::WIDTH))
                    .flex()
                    .gap_1()
                    .child(arrow("inbox", Glyph::Inbox, true, "Inbox").on_click(cx.listener(|this, _, _, cx| this.open_inbox(cx))))
                    .child(arrow("help", Glyph::Help, true, "Help").on_click(|_, _, cx| cx.open_url("https://support.discord.com"))),
            )
            .child(div().flex().items_center().gap_2().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).child(icon).child(div().truncate().max_w(px(480.)).child(title)))
    }

    // Full width across rail and channel list, like Discord: identity on the left (nameplate behind it),
    // voice controls and settings on the right.
    fn user_panel(&mut self, me: discord::User, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let status_line: SharedString = self.custom_status.clone().unwrap_or_else(|| self.my_status.label().into()).into();
        let sharing = self.call.as_ref().is_some_and(|c| c.sharing.is_some());
        let (muted, deafened) = (self.muted || self.deafened, self.deafened);
        // Each toggle pops its icon in (keyed by state, so it replays on every flip), with Discord's cue.
        let control = |id: &'static str, glyph: Glyph, off: bool, label: &'static str| {
            div()
                .id(id)
                .relative()
                .size(px(32.))
                .flex_none()
                .rounded(px(8.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .text_color(if off { theme.danger } else { theme.foreground.opacity(0.85) })
                .hover(|s| s.bg(theme.foreground.opacity(0.1)))
                .child(Icon::new(glyph).size(px(20.)).with_animation(SharedString::from(format!("{id}-{off}")), Animation::new(Duration::from_millis(260)).with_easing(crate::motion::back_out), |icon, t| icon.size(px(20. * (0.7 + 0.3 * t)))))
                .tooltip(move |window, cx| Tooltip::new(label).build(window, cx))
        };
        let mic = control("mic", if muted { Glyph::MicOff } else { Glyph::Mic }, muted, if muted { "Unmute" } else { "Mute" }).on_click(cx.listener(|this, _, _, cx| this.toggle_mute(cx)));
        let headphones = control("deafen", if deafened { Glyph::HeadphonesOff } else { Glyph::Headphones }, deafened, if deafened { "Undeafen" } else { "Deafen" })
            .on_click(cx.listener(|this, _, _, cx| this.toggle_deafen(cx)));
        // Discord's device chevrons sit in the same pill as mic and headphones.
        let caret = |id: &'static str, kind: crate::call::DeviceKind| {
            div()
                .id(id)
                .w(px(16.))
                .h(px(32.))
                .flex_none()
                .ml(px(-6.))
                .rounded(px(6.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .text_color(theme.foreground.opacity(0.6))
                .hover(|s| s.bg(theme.foreground.opacity(0.1)).text_color(theme.foreground))
                .child(Icon::new(Glyph::ChevronDown).size(px(12.)))
                .on_mouse_down(MouseButton::Left, cx.listener(move |this, e: &MouseDownEvent, _, cx| this.open_device_menu(kind, e.position, cx)))
        };
        let mic_caret = caret("mic-devices", crate::call::DeviceKind::Input);
        let headphones_caret = caret("headphones-devices", crate::call::DeviceKind::Output);
        let settings = control("settings", Glyph::Settings, false, "User Settings")
            .on_click(cx.listener(|this, _, window, cx| this.open_settings(crate::settings::Page::Account, window, cx)));
        let surface = theme.popover;
        let nameplate = me.nameplate.as_ref().map(|plate| nameplate(plate, surface, false, theme.is_dark()));
        let voice = self.voice_panel(cx);
        div()
            .absolute()
            .left(px(8.))
            .bottom(px(8.))
            .w(px(RAIL_WIDTH + self.channels_width - 16.))
            .h(px(self.panel_stack_height()))
            .flex()
            .flex_col()
            .rounded(px(8.))
            .overflow_hidden()
            .bg(surface)
            .border_1()
            .border_color(theme.input)
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
                    .px_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .id("me")
                            .flex_1()
                            .min_w_0()
                            .h(px(44.))
                            .pl_1()
                            .pr_2()
                            .rounded(px(8.))
                            .flex()
                            .items_center()
                            .gap_2()
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.foreground.opacity(0.08)))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.status_menu = !this.status_menu;
                                cx.notify();
                            }))
                            .child(decorated_avatar(&me.avatar, me.decoration.as_deref(), 32., Some(self.my_status), surface))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .child(div().truncate().font_weight(FontWeight::SEMIBOLD).child(me.name.clone()))
                                    .child(match sharing {
                                        // Discord's status line while live: a green screen and "Sharing their screen".
                                        true => div()
                                            .flex()
                                            .items_center()
                                            .gap_1()
                                            .text_size(px(12.))
                                            .text_color(theme.muted_foreground)
                                            .child(Icon::new(Glyph::Screen).size(px(14.)).text_color(gpui_kit::rgb(0x41A063)))
                                            .child(div().truncate().child("Sharing their screen")),
                                        false => div().truncate().text_size(px(12.)).text_color(theme.muted_foreground).child(status_line),
                                    }),
                            ),
                    )
                    .child(mic)
                    .child(mic_caret)
                    .child(headphones)
                    .child(headphones_caret)
                    .child(settings),
                    ),
            )
            .into_any_element()
    }

    /// The floating card's height: the user row, plus Discord's "Voice Connected" row while in a call.
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
            MediaKind::Image => img(item.url.clone()).w(px(w)).h(px(h)).rounded(px(6.)).into_any_element(),
            MediaKind::Gif { poster } => crate::chat::gif_image(&item.url, poster).w(px(w)).h(px(h)).rounded(px(6.)).into_any_element(),
            // ponytail: video plays in the system player; an in-app AVPlayer view is the upgrade.
            MediaKind::Video { poster } => {
                let open = item.url.clone();
                div()
                    .id("viewer-video")
                    .relative()
                    .w(px(w))
                    .h(px(h))
                    .cursor_pointer()
                    .child(img(poster.clone()).size_full().rounded(px(6.)))
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
                    .rounded(px(8.))
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
                    .rounded(px(8.))
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
pub fn nameplate(plate: &discord::Nameplate, surface: Hsla, member_row: bool, dark: bool) -> AnyElement {
    let tint: Hsla = gpui_kit::rgb(if dark { plate.tint.0 } else { plate.tint.1 }).into();
    let art = div().absolute().top_0().bottom_0().right_0().left(px(if member_row { 0. } else { 50. })).flex().justify_end().child(
        img(plate.art.clone()).size_full().object_fit(ObjectFit::Cover).opacity(if member_row { 1. } else { 0.4 }),
    );
    div()
        .absolute()
        .inset_0()
        .rounded(px(8.))
        .overflow_hidden()
        .bg(linear_gradient(90., linear_color_stop(tint.opacity(0.1), 0.), linear_color_stop(tint.opacity(0.4), 1.)))
        .child(art)
        .when(member_row, |p| p.child(div().absolute().inset_0().bg(linear_gradient(90., linear_color_stop(surface.opacity(0.7), 0.45), linear_color_stop(surface.opacity(0.), 0.75)))))
        .into_any_element()
}

/// An avatar with Discord's status shape cut into its bottom-right corner: a dot for online, a moon for
/// idle, a bar for do-not-disturb, a ring for offline. Cut-outs are painted in the surface colour.
pub fn avatar_with_status(url: &str, size: f32, status: Option<discord::Status>, surface: Hsla) -> AnyElement {
    decorated_avatar(url, None, size, status, surface)
}

/// The same, with the user's avatar decoration framing it (drawn at 1.2× the avatar, as Discord does).
pub fn decorated_avatar(url: &str, decoration: Option<&str>, size: f32, status: Option<discord::Status>, surface: Hsla) -> AnyElement {
    let dot = (size * 0.3).round();
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
                .right(px(-3.))
                .bottom(px(-3.))
                .size(px(dot + 6.))
                .rounded_full()
                .bg(surface)
                .flex()
                .items_center()
                .justify_center()
                .child(status_mark(status, dot, surface))
        }))
        .into_any_element()
}

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
