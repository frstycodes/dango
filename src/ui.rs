// Wave's shared controls, ported from the design system (design/css/primitives.css). Each returns a
// styled element the caller keeps composing (`.on_click`, layout, extra children), so call sites never
// restate a control's look. `design_system.rs` shows every one of them in both themes.
//
// Clipping: gpui's `overflow_hidden` clips to a plain rectangle, never to rounded corners. Anything
// painted against a rounded edge (an image, a header strip, a hover fill) must round itself to
// match, or sit inset by the surface's padding. `surface` insets its content; `media` and `edge`
// round theirs.
//
// GAP: the design draws corners as squircles (CSS superellipse 1.4) at larger radii; gpui only has
// circular corners, so these are the design's circular-equivalent radii.
use std::time::Duration;

use gpui_kit::{
    Animation, AnimationExt, AnyElement, App, BoxShadow, Corners, Div, ElementId, FontWeight, Hsla, ImageSource, Img, InteractiveElement, IntoElement, ParentElement, Pixels, SharedString, Stateful,
    StatefulInteractiveElement, Styled, Window,
    component::{Icon, tooltip::Tooltip},
    div, img, point, prelude::FluentBuilder, px, relative,
};

use crate::{discord::Status, icons::Glyph, mascot, palette};

// Radius tiers: tags and tooltips, rows and menu items, controls, cards and tiles, dialogs.
pub const RADIUS_XS: f32 = 6.;
pub const RADIUS_SM: f32 = 8.;
pub const RADIUS_MD: f32 = 10.;
pub const RADIUS_LG: f32 = 14.;
pub const RADIUS_XL: f32 = 20.;
// Padding a surface keeps between its rounded edge and its rows.
pub const SURFACE_INSET: f32 = 6.;

// Motion tokens (design/tokens.css): press, hover, pop, panel.
pub const DUR_PRESS: Duration = Duration::from_millis(120);
pub const DUR_HOVER: Duration = Duration::from_millis(150);
pub const DUR_POP: Duration = Duration::from_millis(200);
pub const DUR_PANEL: Duration = Duration::from_millis(260);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tone {
    // Magenta, the one accent: the action the surface is for.
    Primary,
    // The quiet fill: every other action.
    Secondary,
    // No fill until hovered: low-stakes actions in dense rows.
    Ghost,
    // The destructive step itself.
    Danger,
    // Translucent black over video and images.
    OnMedia,
}

// A text button, 36px (`large` 44, `small` 30). Callers add the label and `.on_click`.
pub fn button(id: impl Into<ElementId>, tone: Tone, cx: &App) -> Stateful<Div> {
    let (fill, hover, press, ink) = tone_colors(tone, cx);
    div()
        .id(id)
        .h(px(36.))
        .px(px(14.))
        .flex_none()
        .rounded(px(RADIUS_MD))
        .flex()
        .items_center()
        .justify_center()
        .gap_2()
        .cursor_pointer()
        .bg(fill)
        .text_color(ink)
        .text_size(px(14.))
        .font_weight(FontWeight::SEMIBOLD)
        .whitespace_nowrap()
        .hover(move |s| s.bg(hover))
        .active(move |s| s.bg(press))
}

pub fn large(button: Stateful<Div>) -> Stateful<Div> {
    button.h(px(44.)).px(px(20.)).text_size(px(16.))
}

pub fn small(button: Stateful<Div>) -> Stateful<Div> {
    button.h(px(30.)).px(px(10.)).rounded(px(RADIUS_SM))
}

fn tone_colors(tone: Tone, cx: &App) -> (Hsla, Hsla, Hsla, Hsla) {
    let white = gpui_kit::white();
    match tone {
        Tone::Primary => (palette::accent(), palette::accent_hover(), palette::accent_press(), white),
        Tone::Secondary => (palette::selected(cx), palette::press(cx), palette::press(cx), palette::strong(cx)),
        Tone::Ghost => (gpui_kit::transparent_black(), palette::hover(cx), palette::press(cx), palette::text(cx)),
        Tone::Danger => (palette::danger(), palette::danger_hover(), palette::danger_hover(), white),
        Tone::OnMedia => (gpui_kit::black().opacity(0.6), gpui_kit::black().opacity(0.75), gpui_kit::black().opacity(0.85), white),
    }
}

// A square icon-only button that fills on hover, with its tooltip.
pub fn icon_button(id: impl Into<ElementId>, glyph: Glyph, tooltip: impl Into<SharedString>, cx: &App) -> Stateful<Div> {
    let (muted, ink, hover, press) = (palette::muted(cx), palette::strong(cx), palette::hover(cx), palette::press(cx));
    let tooltip: SharedString = tooltip.into();
    div()
        .id(id)
        .size(px(32.))
        .flex_none()
        .rounded(px(RADIUS_SM))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .text_color(muted)
        .hover(move |s| s.bg(hover).text_color(ink))
        .active(move |s| s.bg(press))
        .child(Icon::new(glyph).size(px(20.)))
        .tooltip(move |window: &mut Window, cx: &mut App| Tooltip::new(tooltip.clone()).build(window, cx))
}

// A round icon button over video or the call stage (`size` 28 or 40).
pub fn round_button(id: impl Into<ElementId>, glyph: Glyph, tooltip: impl Into<SharedString>, size: f32) -> Stateful<Div> {
    let tooltip: SharedString = tooltip.into();
    div()
        .id(id)
        .size(px(size))
        .flex_none()
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .bg(gpui_kit::white().opacity(0.12))
        .text_color(gpui_kit::white())
        .hover(|s| s.bg(gpui_kit::white().opacity(0.2)))
        .active(|s| s.bg(gpui_kit::white().opacity(0.28)))
        .child(Icon::new(glyph).size(px(size / 2.)))
        .tooltip(move |window: &mut Window, cx: &mut App| Tooltip::new(tooltip.clone()).build(window, cx))
}

// The LIVE badge: magenta, with the mini wave.
pub fn live_badge() -> Div {
    badge(palette::live(), gpui_kit::white()).gap(px(5.)).child(bars_still(gpui_kit::white(), 9.)).child("LIVE")
}

// A small solid label (quality, tags, NEW).
pub fn badge(fill: Hsla, ink: Hsla) -> Div {
    div().h(px(20.)).px(px(7.)).flex_none().rounded(px(RADIUS_XS)).bg(fill).text_color(ink).text_size(px(11.)).font_weight(FontWeight::BOLD).flex().items_center()
}

// What something is (App, Host, New): accent ink on the accent tint, uppercase 11px.
pub fn tag(text: impl Into<SharedString>, cx: &App) -> Div {
    let text: SharedString = text.into();
    badge(palette::accent_soft(cx), palette::accent_ink(cx)).h(px(18.)).px(px(6.)).child(text.to_uppercase())
}

// A count asks for you, so it takes the accent. A capsule that grows with the number.
pub fn count_badge(count: u32) -> Div {
    let text = if count > 99 { "99+".to_owned() } else { count.to_string() };
    div()
        .min_w(px(18.))
        .h(px(18.))
        .px(px(5.))
        .flex_none()
        .rounded_full()
        .bg(palette::accent())
        .text_color(gpui_kit::white())
        .text_size(px(12.))
        .font_weight(FontWeight::BOLD)
        .flex()
        .items_center()
        .justify_center()
        .child(text)
}

// A keycap.
pub fn kbd(key: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .min_w(px(22.))
        .h(px(22.))
        .px(px(6.))
        .flex_none()
        .rounded(px(RADIUS_XS))
        .bg(palette::surface(palette::Level::Raised, cx))
        .border_1()
        .border_color(palette::line_strong(cx))
        // A deeper bottom edge, so the cap reads as a key you could press.
        .shadow(vec![BoxShadow { color: palette::line_strong(cx), offset: point(px(0.), px(-1.)), blur_radius: px(0.), spread_radius: px(0.), inset: true }])
        .text_color(palette::strong(cx))
        .text_size(px(12.))
        .font_weight(FontWeight::SEMIBOLD)
        .flex()
        .items_center()
        .justify_center()
        .child(key.into())
}

// The recessed track a row of `segment`s sits in; the pressed one is a raised thumb.
pub fn segmented(cx: &App) -> Div {
    div().p(px(3.)).gap(px(2.)).rounded(px(RADIUS_MD)).bg(palette::seg_track(cx)).flex()
}

pub fn segment(id: impl Into<ElementId>, label: impl Into<SharedString>, on: bool, cx: &App) -> Stateful<Div> {
    let (strong, muted, thumb) = (palette::strong(cx), palette::muted(cx), palette::seg_thumb(cx));
    div()
        .id(id)
        .flex_1()
        .min_w_0()
        .h(px(28.))
        .px_3()
        .rounded(px(RADIUS_MD - 3.))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .text_size(px(14.))
        .font_weight(FontWeight::SEMIBOLD)
        .whitespace_nowrap()
        .text_color(if on { strong } else { muted })
        .when(on, |s| s.bg(thumb).shadow_sm())
        .when(!on, |s| s.hover(move |s| s.text_color(strong)))
        .child(label.into())
}

// A wide pill with a capsule knob, magenta when on (iOS-style). GAP: the press-to-glass swell and
// the spring across need gpui animation keyed on the toggle; the knob moves in one step for now.
pub fn switch(on: bool, cx: &App) -> Div {
    div()
        .w(px(52.))
        .h(px(28.))
        .flex_none()
        .p(px(2.))
        .rounded_full()
        .bg(if on { palette::accent() } else { palette::track(cx) })
        .flex()
        .items_center()
        .when(on, |d| d.justify_end())
        .child(div().w(px(28.)).h(px(24.)).rounded_full().bg(gpui_kit::white()).shadow_sm())
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Check {
    Off,
    On,
    // Some of a group chosen: a minus bar in place of the tick.
    Mixed,
}

pub fn checkbox(on: bool) -> Div {
    let line = palette::wash(0.64);
    check_box(if on { Check::On } else { Check::Off }, line)
}

pub fn check(state: Check, cx: &App) -> Div {
    check_box(state, palette::line_strong(cx))
}

fn check_box(state: Check, line: Hsla) -> Div {
    let filled = state != Check::Off;
    div()
        .size(px(20.))
        .flex_none()
        .rounded(px(6.))
        .border_1()
        .border_color(if filled { palette::accent() } else { line })
        .bg(if filled { palette::accent() } else { gpui_kit::transparent_black() })
        .text_color(gpui_kit::white())
        .flex()
        .items_center()
        .justify_center()
        .when(state == Check::On, |d| d.child(Icon::new(Glyph::Checkmark).size(px(14.))))
        .when(state == Check::Mixed, |d| d.child(div().w(px(10.)).h(px(2.)).rounded(px(1.)).bg(gpui_kit::white())))
}

// Off: a thin ring. On: the ring thickens to a magenta band around the surface's colour.
pub fn radio(on: bool, cx: &App) -> Div {
    div()
        .size(px(20.))
        .flex_none()
        .rounded_full()
        .when(on, |d| d.border(px(6.)).border_color(palette::accent()))
        .when(!on, |d| d.border(px(1.5)).border_color(palette::line_strong(cx)))
}

// A filter or toggle chip; pressed takes the accent tint.
pub fn chip(id: impl Into<ElementId>, label: impl Into<SharedString>, on: bool, cx: &App) -> Stateful<Div> {
    let (soft, ink, hover) = (palette::accent_soft(cx), palette::accent_ink(cx), palette::hover(cx));
    div()
        .id(id)
        .h(px(30.))
        .px(px(12.))
        .flex_none()
        .rounded(px(RADIUS_SM))
        .border_1()
        .flex()
        .items_center()
        .gap(px(6.))
        .cursor_pointer()
        .text_size(px(14.))
        .font_weight(FontWeight::MEDIUM)
        .when(on, |d| d.bg(soft).border_color(palette::accent().opacity(0.5)).text_color(ink))
        .when(!on, |d| d.border_color(palette::line_strong(cx)).text_color(palette::text(cx)).hover(move |s| s.bg(hover)))
        .child(label.into())
}

pub fn avatar(url: impl Into<SharedString>, size: f32) -> Img {
    img(url.into()).size(px(size)).flex_none().rounded_full()
}

// Overlapping avatars, each ringed in `ring` (the surface colour) so they read apart.
pub fn avatar_stack(urls: impl IntoIterator<Item = String>, size: f32, ring: Hsla) -> Div {
    div().flex().items_center().pl(px(size * 0.3)).children(urls.into_iter().map(move |url| avatar(url, size).border_2().border_color(ring).ml(px(-size * 0.3))))
}

// A presence mark for an avatar's corner. Never colour alone: online is a dot, idle a moon, do not
// disturb a bar, offline a ring. `ring` is the surface it sits on, so the mark reads apart.
pub fn status_mark(status: Status, size: f32, ring: Hsla, cx: &App) -> Div {
    let color = palette::status_in(status, cx);
    let base = div().size(px(size)).flex_none().rounded_full().border(px(2.)).border_color(ring).relative();
    match status {
        Status::Online => base.bg(color),
        Status::Idle => base.bg(color).child(div().absolute().top(px(-1.)).left(px(-1.)).size(px(size * 0.55)).rounded_full().bg(ring)),
        Status::Dnd => base.bg(color).flex().items_center().justify_center().child(div().w(px(size * 0.45)).h(px(2.)).rounded(px(1.)).bg(ring)),
        Status::Offline => base.bg(ring).child(div().size_full().rounded_full().border(px(size * 0.16)).border_color(color)),
    }
}

// A floating surface (menu, popover, picker): rounded, hairlined, shadowed, with its content inset
// so nothing reaches the rounded edge.
pub fn surface(cx: &App) -> Div {
    div()
        .rounded(px(RADIUS_LG))
        .border_1()
        .border_color(palette::line_strong(cx))
        .bg(palette::surface(palette::Level::Overlay, cx))
        .shadow_lg()
        .p(px(SURFACE_INSET))
        .flex()
        .flex_col()
}

// A row inside a surface: a neutral hover fill, inset by the surface's padding.
pub fn menu_row(id: impl Into<ElementId>, cx: &App) -> Stateful<Div> {
    let (ink, strong, hover) = (palette::text(cx), palette::strong(cx), palette::hover(cx));
    div()
        .id(id)
        .min_h(px(34.))
        .px(px(10.))
        .rounded(px(RADIUS_SM))
        .flex()
        .items_center()
        .gap(px(10.))
        .cursor_pointer()
        .text_size(px(14.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(ink)
        .hover(move |s| s.bg(hover).text_color(strong))
}

// A list row in a sidebar or panel: a neutral hover, a stronger fill when selected.
pub fn list_row(id: impl Into<ElementId>, selected: bool, cx: &App) -> Stateful<Div> {
    let (sel, hover) = (palette::selected(cx), palette::hover(cx));
    div()
        .id(id)
        .px_2()
        .rounded(px(RADIUS_SM))
        .flex()
        .items_center()
        .gap_2()
        .cursor_pointer()
        .when(selected, |d| d.bg(sel))
        .when(!selected, |d| d.hover(move |s| s.bg(hover)))
}

// A small heading over a group ("Roles", "Recent"), sentence case.
pub fn section_label(text: impl Into<SharedString>, cx: &App) -> Div {
    div().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(palette::faint(cx)).child(text.into())
}

// Progress: the done part, a 4px gap, then what's left; the ends facing the gap get small corners.
pub fn progress(fraction: f32, cx: &App) -> Div {
    let f = fraction.clamp(0., 1.);
    let seg = |grow: f32, fill: Hsla, left: f32, right: f32| {
        div().h(px(6.)).flex_basis(px(0.)).flex_grow(grow).rounded_tl(px(left)).rounded_bl(px(left)).rounded_tr(px(right)).rounded_br(px(right)).bg(fill)
    };
    div()
        .w_full()
        .flex()
        .gap(px(4.))
        .when(f > 0., |d| d.child(seg(f, palette::accent(), 3., if f < 1. { 2. } else { 3. })))
        .when(f < 1., |d| d.child(seg(1. - f, palette::track(cx), if f > 0. { 2. } else { 3. }, 3.)))
}

// A slider's look: filled, gap, a tall narrow thumb, gap, the rest. Drag handling is the caller's.
pub fn slider(fraction: f32, cx: &App) -> Div {
    let f = fraction.clamp(0., 1.);
    let bar = |grow: f32, fill: Hsla, outer_left: bool| {
        let (l, r) = if outer_left { (4., 2.5) } else { (2.5, 4.) };
        div().h(px(8.)).flex_basis(px(0.)).flex_grow(grow).rounded_tl(px(l)).rounded_bl(px(l)).rounded_tr(px(r)).rounded_br(px(r)).bg(fill)
    };
    div()
        .w_full()
        .h(px(24.))
        .flex()
        .items_center()
        .gap(px(4.))
        .when(f > 0., |d| d.child(bar(f, palette::accent(), true)))
        .child(div().w(px(6.)).h(px(24.)).flex_none().rounded(px(3.)).bg(palette::accent()))
        .when(f < 1., |d| d.child(bar(1. - f, palette::track(cx), false)))
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Notice {
    Accent,
    Info,
    Warning,
    Danger,
    Success,
}

// An inline notice inside the view it's about: a tinted surface, a leading icon, a title and a line.
pub fn callout(kind: Notice, glyph: Glyph, title: impl Into<SharedString>, body: impl Into<SharedString>, cx: &App) -> Div {
    let tone = match kind {
        Notice::Accent => palette::accent_ink(cx),
        Notice::Info => palette::info_ink(cx),
        Notice::Warning => palette::warning_ink(cx),
        Notice::Danger => palette::danger_ink(cx),
        Notice::Success => palette::positive_ink(cx),
    };
    div()
        .w_full()
        .p(px(12.))
        .px(px(14.))
        .rounded(px(RADIUS_LG))
        .bg(tone.opacity(0.12))
        .flex()
        .gap(px(12.))
        .items_start()
        .child(div().text_color(tone).flex_none().child(Icon::new(glyph).size(px(20.))))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .text_size(px(14.))
                .child(div().font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(title.into()))
                .child(div().text_color(palette::text(cx)).child(body.into())),
        )
}

// An empty or waiting state: the mascot, what's missing, and room for one action.
pub fn empty_state(id: &'static str, mood: mascot::Mood, title: impl Into<SharedString>, body: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(8.))
        .py(px(32.))
        .px(px(16.))
        .child(div().mb(px(8.)).child(mascot::mascot_at(id, mood, 0.6, cx)))
        .child(div().text_size(px(20.)).font_weight(FontWeight::BOLD).text_color(palette::strong(cx)).child(title.into()))
        .child(div().max_w(px(320.)).text_center().text_color(palette::muted(cx)).child(body.into()))
}

// A divider, with an optional label naming what follows.
pub fn divider(label: Option<&'static str>, cx: &App) -> Div {
    let line = || div().flex_1().h(px(1.)).bg(palette::line(cx));
    div()
        .w_full()
        .flex()
        .items_center()
        .gap(px(12.))
        .child(line())
        .when_some(label, |d, label| d.child(div().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(palette::faint(cx)).child(label)).child(line()))
}

// A squircle holding an icon, for row leads and settings.
pub fn icon_tile(glyph: Glyph, fill: Hsla, ink: Hsla) -> Div {
    div().size(px(36.)).flex_none().rounded(px(RADIUS_MD)).bg(fill).text_color(ink).flex().items_center().justify_center().child(Icon::new(glyph).size(px(20.)))
}

// The mini wave: three bars, the mark for speaking, typing, loading and live.
pub fn bars(id: impl Into<ElementId>, color: Hsla, height: f32) -> AnyElement {
    let frame = move |t: f32| {
        div().h(px(height)).flex().items_center().gap(px(2.)).children([0.55_f32, 1., 0.7].iter().enumerate().map(move |(i, base)| {
            let phase = (t * std::f32::consts::TAU + i as f32 * 2.1).sin().abs();
            div().w(px(3.)).h(px(height * base * (0.35 + 0.65 * phase))).rounded_full().bg(color)
        }))
    };
    frame(0.).with_animation(id, Animation::new(Duration::from_millis(900)).repeat(), move |_, t| frame(t)).into_any_element()
}

pub fn bars_still(color: Hsla, height: f32) -> Div {
    div().h(px(height)).flex().items_center().gap(px(1.5)).children([0.55_f32, 1., 0.7].map(|base| div().w(px(2.)).h(px(height * base)).rounded_full().bg(color)))
}

// An image placed against a rounded container's edge, rounded to `corners` itself because gpui
// can't clip children to rounded corners.
pub fn media(source: impl Into<ImageSource>, corners: Corners<Pixels>) -> Img {
    img(source).rounded_tl(corners.top_left).rounded_tr(corners.top_right).rounded_bl(corners.bottom_left).rounded_br(corners.bottom_right)
}

// Corner sets for `media` and edge strips: all four, or only the top or bottom pair.
pub fn all(radius: f32) -> Corners<Pixels> {
    Corners::all(px(radius))
}

pub fn top(radius: f32) -> Corners<Pixels> {
    Corners { top_left: px(radius), top_right: px(radius), bottom_left: px(0.), bottom_right: px(0.) }
}

pub fn bottom(radius: f32) -> Corners<Pixels> {
    Corners { top_left: px(0.), top_right: px(0.), bottom_left: px(radius), bottom_right: px(radius) }
}

// A strip (header, footer, scroll area) that runs edge to edge of a rounded container, rounded to
// match on the corners it touches.
pub fn edge<E: Styled>(el: E, corners: Corners<Pixels>) -> E {
    el.rounded_tl(corners.top_left).rounded_tr(corners.top_right).rounded_bl(corners.bottom_left).rounded_br(corners.bottom_right)
}

// Motion: the design's touch spring (small overshoot, settled inside ~250ms), from gpui-base. Keyed per
// element and retargeted from wherever it is, keeping its velocity, so a reversed toggle never jumps.
const TOUCH: gpui_kit::base::Spring = gpui_kit::base::Spring::new(Duration::from_millis(260)).with_damping(0.62);

pub fn spring(id: impl Into<gpui_kit::base::TransitionId>, target: f32, window: &mut Window, cx: &mut App) -> f32 {
    gpui_kit::base::spring(id, target, TOUCH, window, cx)
}

// The switch, alive: the knob springs across, and while pressed it swells into a glassy capsule.
pub fn switch_live(id: &'static str, on: bool, window: &mut Window, cx: &mut App) -> Stateful<Div> {
    let travel = spring((id, "knob"), if on { 1. } else { 0. }, window, cx);
    let group: SharedString = format!("switch-{id}").into();
    let (track_off, accent) = (palette::track(cx), palette::accent());
    let knob_shadow = vec![BoxShadow { color: gpui_kit::black().opacity(0.22), offset: point(px(0.), px(2.)), blur_radius: px(6.), spread_radius: px(0.), inset: false }];
    let glass = vec![
        BoxShadow { color: gpui_kit::white().opacity(0.7), offset: point(px(0.), px(0.)), blur_radius: px(0.), spread_radius: px(1.), inset: true },
        BoxShadow { color: gpui_kit::black().opacity(0.25), offset: point(px(0.), px(4.)), blur_radius: px(12.), spread_radius: px(0.), inset: false },
    ];
    div()
        .id(id)
        .group(group.clone())
        .w(px(52.))
        .h(px(28.))
        .flex_none()
        .relative()
        .rounded_full()
        .cursor_pointer()
        // The fill fades with the knob's travel rather than flipping.
        .bg(mix(track_off, accent, travel.clamp(0., 1.)))
        .child(
            div()
                .id((id, 1usize))
                .absolute()
                .top(px(2.))
                .left(px(2. + 20. * travel))
                .w(px(28.))
                .h(px(24.))
                .rounded_full()
                .bg(gpui_kit::white())
                .shadow(knob_shadow)
                .group_active(group, move |s| s.bg(gpui_kit::white().opacity(0.55)).shadow(glass.clone())),
        )
}

// A segmented control whose raised thumb slides between equal options on the spring.
pub fn segmented_live(id: &'static str, labels: &[&'static str], selected: usize, on_select: impl Fn(usize, &mut Window, &mut App) + Clone + 'static, window: &mut Window, cx: &mut App) -> Div {
    let n = labels.len().max(1) as f32;
    let at = spring((id, "thumb"), selected as f32, window, cx);
    let (thumb, strong, muted) = (palette::seg_thumb(cx), palette::strong(cx), palette::muted(cx));
    let dark = cx.theme_is_dark();
    let lift = if dark {
        vec![
            BoxShadow { color: gpui_kit::black().opacity(0.4), offset: point(px(0.), px(1.)), blur_radius: px(3.), spread_radius: px(0.), inset: false },
            BoxShadow { color: gpui_kit::white().opacity(0.14), offset: point(px(0.), px(0.5)), blur_radius: px(0.), spread_radius: px(0.), inset: true },
        ]
    } else {
        vec![
            BoxShadow { color: gpui_kit::black().opacity(0.14), offset: point(px(0.), px(1.)), blur_radius: px(3.), spread_radius: px(0.), inset: false },
            BoxShadow { color: gpui_kit::black().opacity(0.06), offset: point(px(0.), px(0.)), blur_radius: px(0.), spread_radius: px(0.5), inset: false },
        ]
    };
    segmented(cx).relative().child(
        // The thumb lives in a layer inset by the track's padding, so its fraction maps onto the options.
        div().absolute().top(px(3.)).bottom(px(3.)).left(px(3.)).right(px(3.)).child(
            div().absolute().top_0().bottom_0().left(relative(at / n)).w(relative(1. / n)).rounded(px(RADIUS_MD - 3.)).bg(thumb).shadow(lift),
        ),
    )
    .children(labels.iter().enumerate().map(move |(i, label)| {
        let on = i == selected;
        div()
            .id((id, i + 1))
            .relative()
            .flex_1()
            .min_w_0()
            .h(px(28.))
            .px_3()
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .text_size(px(14.))
            .font_weight(FontWeight::SEMIBOLD)
            .whitespace_nowrap()
            .text_color(if on { strong } else { muted })
            .when(!on, |s| s.hover(move |s| s.text_color(strong)))
            .on_click({
                let on_select = on_select.clone();
                move |_, window, cx| on_select(i, window, cx)
            })
            .child(*label)
    }))
}

trait ThemeIsDark {
    fn theme_is_dark(&self) -> bool;
}

impl ThemeIsDark for App {
    fn theme_is_dark(&self) -> bool {
        use gpui_kit::component::ActiveTheme;
        self.theme().is_dark()
    }
}

// Linear mix of two colours in RGB, for fills that follow a spring.
fn mix(from: Hsla, to: Hsla, t: f32) -> Hsla {
    let (a, b) = (gpui_kit::Rgba::from(from), gpui_kit::Rgba::from(to));
    let lerp = |x: f32, y: f32| x + (y - x) * t;
    gpui_kit::Rgba { r: lerp(a.r, b.r), g: lerp(a.g, b.g), b: lerp(a.b, b.b), a: lerp(a.a, b.a) }.into()
}

