// Wave's shared controls, ported from the design system (design/css/primitives.css, overlays.css). Each
// returns an element the caller keeps composing (`.on_click`, layout, extra children), so call sites never
// restate a control's look. `design_system.rs` shows every one of them in both themes, section for
// section with design/primitives.html.
//
// Pressable controls (buttons, icon buttons, chips, swatches, tab options) live in `control`, fields in
// `field`, the sliding strips in `tabs`, tracks and waits in `feedback`; this file holds the marks,
// badges, people, surfaces and the live switch, check and radio.
//
// Clipping: gpui's `overflow_hidden` clips to a plain rectangle, never to rounded corners. Anything
// painted against a rounded edge (an image, a header strip, a hover fill) must round itself to
// match, or sit inset by the surface's padding. `surface` insets its content; `media` and `edge`
// round theirs.
//
// GAP: the design draws corners as squircles (CSS superellipse 1.4) at larger radii; gpui only has
// circular corners, so these are the design's circular-equivalent radii.
// PATCH(squircle): once `Style` has a corner shape, switch every control and surface to superellipse(1.4)
// and these radii to the design's tokens (xs 8, sm 11, md 14, lg 20, xl 28); people, status marks, the
// switch and the bars stay round.
use std::time::Duration;

use gpui_kit::{
    Animation, AnimationExt, AnyElement, App, BoxShadow, Corners, Div, ElementId, FontWeight, Hsla, ImageSource, Img, InteractiveElement, IntoElement, MouseButton, ParentElement, Pixels, SharedString,
    Stateful, Styled, Window,
    base::{Transition, transition},
    component::Icon,
    div, img, linear_color_stop, linear_gradient, point, prelude::FluentBuilder, px,
};

use crate::{discord::Status, icons::Glyph, mascot, motion, palette};

mod control;
mod feedback;
mod field;
mod tabs;

pub use control::{Control, button, chip, hover_fade, icon_button, round_button, swatch, tip};
pub use feedback::{meter, progress_indeterminate, progress_live, ring, skeleton, slider_live, slider_track};
pub use field::{code_cell, drop_zone, field, field_error, field_hint, select, stepper, text_area, text_field};
pub use tabs::{pill_tabs, segmented_live, tabs};

// Radius tiers: tags and tooltips, rows and menu items, controls, cards and tiles, dialogs. Sized for the
// squircle, whose curve hugs the corner tighter than a circular arc of the same radius.
pub const RADIUS_XS: f32 = 8.;
pub const RADIUS_SM: f32 = 11.;
pub const RADIUS_MD: f32 = 14.;
pub const RADIUS_LG: f32 = 20.;
pub const RADIUS_XL: f32 = 28.;
// CSS `corner-shape: superellipse(1.4)`. People, status marks and pills stay round.
pub const SQUIRCLE: gpui_kit::CornerShape = gpui_kit::CornerShape::Superellipse(1.4);

/// Rounds an element with the design's soft squircle.
pub trait Squircle: Styled + Sized {
    fn squircle(self, radius: f32) -> Self {
        self.rounded(px(radius)).corner_shape(SQUIRCLE)
    }
}

impl<T: Styled> Squircle for T {}
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
    // Just the accent ink, underlined on hover: "View all members".
    Link,
}

// A main action and its options chevron, joined: inner corners tighten to 4px with a 1px seam.
pub fn split(main: Control, options: Control) -> Div {
    div()
        .flex()
        .gap(px(1.))
        .child(main.rounded_l(px(RADIUS_MD)).rounded_r(px(4.)))
        .child(options.px(px(8.)).rounded_r(px(RADIUS_MD)).rounded_l(px(4.)))
}

// The LIVE badge: magenta, with the mini wave.
pub fn live_badge() -> Div {
    badge(palette::live(), gpui_kit::white()).gap(px(5.)).child(bars_still(gpui_kit::white(), 9.)).child("LIVE")
}

// A small solid label (quality, tags, NEW). GAP: the design tracks these 0.04–0.06em; gpui text has no
// letter spacing.
pub fn badge(fill: Hsla, ink: Hsla) -> Div {
    div().h(px(20.)).px(px(7.)).flex_none().rounded(px(RADIUS_XS)).bg(fill).text_color(ink).text_size(px(11.)).font_weight(FontWeight::BOLD).flex().items_center()
}

// What something is (App, Host, New): accent ink on the accent tint, uppercase 11px.
pub fn tag(text: impl Into<SharedString>, cx: &App) -> Div {
    let text: SharedString = text.into();
    badge(palette::accent_soft(cx), palette::accent_ink(cx)).h(px(18.)).px(px(6.)).gap(px(3.)).child(text.to_uppercase())
}

// A tag led by a small glyph (Host's crown).
pub fn tag_with(glyph: Glyph, text: impl Into<SharedString>, cx: &App) -> Div {
    let text: SharedString = text.into();
    badge(palette::accent_soft(cx), palette::accent_ink(cx)).h(px(18.)).px(px(6.)).gap(px(3.)).child(Icon::new(glyph).size(px(11.))).child(text.to_uppercase())
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
        .line_height(px(18.))
        .flex()
        .items_center()
        .justify_center()
        .child(text)
}

// Something unread, without a count.
pub fn unread_dot() -> Div {
    div().size(px(8.)).flex_none().rounded_full().bg(palette::accent())
}

// A role: its colour as a dot, never as the text's colour.
pub fn role_chip(name: impl Into<SharedString>, color: Hsla, cx: &App) -> Div {
    div()
        .h(px(24.))
        .px(px(8.))
        .flex_none()
        .flex()
        .items_center()
        .gap(px(6.))
        .rounded(px(RADIUS_XS))
        .bg(palette::selected(cx))
        .text_color(palette::text(cx))
        .text_size(px(12.))
        .font_weight(FontWeight::SEMIBOLD)
        .whitespace_nowrap()
        .child(div().size(px(10.)).rounded_full().bg(color))
        .child(name.into())
}

// A keycap: the raised fill, a hairline ring and a deeper bottom edge, so it reads as a key you could press.
pub fn kbd(key: impl Into<SharedString>, cx: &App) -> Div {
    let line = palette::line_strong(cx);
    div()
        .min_w(px(22.))
        .h(px(22.))
        .px(px(6.))
        .flex_none()
        .rounded(px(RADIUS_XS))
        .bg(palette::surface(palette::Level::Raised, cx))
        .shadow(vec![shadow(line, 0., -1., 0., 0., true), shadow(line, 0., 0., 0., 1., false)])
        .text_color(palette::strong(cx))
        .text_size(px(12.))
        .font_weight(FontWeight::SEMIBOLD)
        .whitespace_nowrap()
        .flex()
        .items_center()
        .justify_center()
        .child(key.into())
}

// A shortcut: its keys side by side.
pub fn keys<I: Into<SharedString>>(keys: impl IntoIterator<Item = I>, cx: &App) -> Div {
    div().flex().items_center().gap(px(4.)).children(keys.into_iter().map(|key| kbd(key, cx)))
}

// The recessed track a row of `segment`s sits in; the pressed one is a raised thumb.
pub fn segmented(cx: &App) -> Div {
    div().p(px(3.)).gap(px(2.)).rounded(px(RADIUS_MD)).bg(palette::seg_track(cx)).flex()
}

// One option of a static `segmented` row. `segmented_live` slides its thumb instead.
pub fn segment(id: impl Into<ElementId>, label: impl Into<SharedString>, on: bool, cx: &App) -> Stateful<Div> {
    let (strong, muted, thumb) = (palette::strong(cx), palette::muted(cx), palette::seg_thumb(cx));
    let lift = seg_lift(cx);
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
        .when(on, |s| s.bg(thumb).shadow(lift))
        .when(!on, |s| s.hover(move |s| s.text_color(strong)))
        .child(label.into())
}

// The segmented thumb's lift: a soft drop plus, on dark, a lit top edge; on light, a hairline.
pub(crate) fn seg_lift(cx: &App) -> Vec<BoxShadow> {
    let shade = palette::shade(cx);
    if dark(cx) {
        return vec![shadow(shade.opacity(0.4), 0., 1., 3., 0., false), shadow(gpui_kit::white().opacity(0.14), 0., 0.5, 0., 0., true)];
    }
    vec![shadow(shade.opacity(0.2), 0., 1., 3., 0., false), shadow(shade.opacity(0.1), 0., 0., 0., 0.5, false)]
}

// What floats (menus, popovers, toasts): a long soft drop and a hairline ring in place of a border.
pub fn shadow_pop(cx: &App) -> Vec<BoxShadow> {
    let drop = palette::shade(cx).opacity(if dark(cx) { 0.45 } else { 0.16 });
    vec![shadow(drop, 0., 12., 32., 0., false), shadow(palette::line_strong(cx), 0., 0., 0., 1., false)]
}

fn shadow(color: Hsla, x: f32, y: f32, blur: f32, spread: f32, inset: bool) -> BoxShadow {
    BoxShadow { color, offset: point(px(x), px(y)), blur_radius: px(blur), spread_radius: px(spread), inset }
}

// A wide pill with a capsule knob, magenta when on (iOS-style). `switch_live` animates it.
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
        .child(div().w(px(28.)).h(px(24.)).rounded_full().bg(gpui_kit::white()).shadow(knob_shadow()))
}

fn knob_shadow() -> Vec<BoxShadow> {
    let black = gpui_kit::black();
    vec![shadow(black.opacity(0.22), 0., 2., 6., 0., false), shadow(black.opacity(0.06), 0., 0., 0., 0.5, false)]
}

// The switch, alive: the knob springs across (300ms on the touch spring) while the track's fill fades
// to the accent, and while pressed the knob swells into a glassy capsule.
pub fn switch_live(id: impl Into<ElementId>, on: bool, window: &mut Window, cx: &mut App) -> Stateful<Div> {
    let id: ElementId = id.into();
    let held = window.use_keyed_state(control::part(&id, "held"), cx, |_, _| false);
    let pressed = *held.read(cx);
    let spring = || Transition::new(Duration::from_millis(300)).easing(motion::ease_spring());
    let travel = transition(control::part(&id, "travel"), if on { 1. } else { 0. }, spring(), window, cx);
    let swell = transition(control::part(&id, "swell"), if pressed { 1. } else { 0. }, spring(), window, cx);
    let fill = transition(control::part(&id, "fill"), if on { 1. } else { 0. }, Transition::new(DUR_POP).easing(motion::ease_out()), window, cx);
    let glass = hover_fade(control::part(&id, "glass"), pressed, window, cx);
    // PATCH(transform): the swell is scale(1.18, 1.25) about the knob's centre; until then it grows the
    // knob's box about the same centre, which reads the same.
    let (w, h) = (28. * (1. + 0.18 * swell), 24. * (1. + 0.25 * swell));
    let set = {
        let held = held.clone();
        move |value: bool| {
        let held = held.clone();
        move |_: &gpui_kit::MouseUpEvent, _: &mut Window, cx: &mut App| held.update(cx, |h, cx| {
            *h = value;
            cx.notify();
        })
    }};
    div()
        .id(id.clone())
        .w(px(52.))
        .h(px(28.))
        .flex_none()
        .relative()
        .rounded_full()
        .cursor_pointer()
        .bg(mix(palette::track(cx), palette::accent(), fill.clamp(0., 1.)))
        .on_mouse_down(MouseButton::Left, {
            let held = held.clone();
            move |_, _, cx| held.update(cx, |h, cx| {
                *h = true;
                cx.notify();
            })
        })
        .on_mouse_up(MouseButton::Left, set(false))
        .on_mouse_up_out(MouseButton::Left, set(false))
        .child(
            div()
                .absolute()
                .top(px(2. - (h - 24.) / 2.))
                .left(px(2. + 20. * travel - (w - 28.) / 2.))
                .w(px(w))
                .h(px(h))
                .rounded_full()
                .bg(mix(gpui_kit::white(), gpui_kit::white().opacity(0.42), glass))
                .shadow(knob_glass(glass)),
        )
}

// The knob's shadows, from resting (`t` 0) to pressed glass (`t` 1): a lit inner rim and a deeper drop.
// PATCH(blur): the pressed knob also blurs what's behind it (backdrop-filter: blur(2px) saturate(180%)).
fn knob_glass(t: f32) -> Vec<BoxShadow> {
    if t <= 0. {
        return knob_shadow();
    }
    let (white, black) = (gpui_kit::white(), gpui_kit::black());
    vec![
        shadow(white.opacity(0.7 * t), 0., 0., 0., t, true),
        shadow(white.opacity(0.9 * t), 0., t, t, 0., true),
        shadow(black.opacity(0.22 + 0.03 * t), 0., 2. + 2. * t, 6. + 6. * t, 0., false),
    ]
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

// A checkbox that fades its fill and pops its tick in on the spring (200ms), as the design's does.
pub fn check_live(id: impl Into<ElementId>, state: Check, window: &mut Window, cx: &mut App) -> Div {
    let id: ElementId = id.into();
    let filled = hover_fade(control::part(&id, "fill"), state != Check::Off, window, cx);
    let tick = transition(control::part(&id, "tick"), if state == Check::On { 1. } else { 0. }, Transition::new(DUR_POP).easing(motion::ease_spring()), window, cx);
    let line = palette::line_strong(cx);
    let accent = palette::accent();
    div()
        .size(px(20.))
        .flex_none()
        .rounded(px(6.))
        .border(px(1.5))
        .border_color(mix(line, accent, filled))
        .bg(accent.opacity(filled))
        .flex()
        .items_center()
        .justify_center()
        .when(state == Check::Mixed, |d| d.child(div().w(px(10.)).h(px(2.)).rounded(px(1.)).bg(gpui_kit::white())))
        .when(state != Check::Mixed && tick > 0.01, |d| d.child(Icon::new(Glyph::Checkmark).size(px(13. * (0.5 + 0.5 * tick))).text_color(gpui_kit::white().opacity(tick.min(1.)))))
}

fn check_box(state: Check, line: Hsla) -> Div {
    let filled = state != Check::Off;
    div()
        .size(px(20.))
        .flex_none()
        .rounded(px(6.))
        .border(px(1.5))
        .border_color(if filled { palette::accent() } else { line })
        .bg(if filled { palette::accent() } else { gpui_kit::transparent_black() })
        .text_color(gpui_kit::white())
        .flex()
        .items_center()
        .justify_center()
        .when(state == Check::On, |d| d.child(Icon::new(Glyph::Checkmark).size(px(13.))))
        .when(state == Check::Mixed, |d| d.child(div().w(px(10.)).h(px(2.)).rounded(px(1.)).bg(gpui_kit::white())))
}

// Off: a thin ring. On: the ring thickens to a magenta band around the surface's colour.
pub fn radio(on: bool, cx: &App) -> Div {
    radio_ring(if on { 1. } else { 0. }, cx)
}

// A radio whose band thickens on the spring when chosen.
pub fn radio_live(id: impl Into<ElementId>, on: bool, window: &mut Window, cx: &mut App) -> Div {
    let id: ElementId = id.into();
    let band = transition(control::part(&id, "band"), if on { 1. } else { 0. }, Transition::new(DUR_POP).easing(motion::ease_spring()), window, cx);
    radio_ring(band, cx)
}

fn radio_ring(band: f32, cx: &App) -> Div {
    let color = mix(palette::line_strong(cx), palette::accent(), band.clamp(0., 1.));
    div().size(px(20.)).flex_none().rounded_full().border(px((1.5 + 4.5 * band).max(1.))).border_color(color)
}

// A setting that applies at once: its title and a line of detail, the switch at the end. The whole row
// is the hit area and fills on hover; the caller toggles on click.
pub fn toggle_row(id: impl Into<ElementId>, title: impl Into<SharedString>, detail: Option<SharedString>, switch: impl IntoElement, cx: &App) -> Control {
    control::choice(id, cx).child(choice_text(title, detail, cx)).child(switch)
}

// One of a set to pick from: the check or radio first, then its title and detail.
pub fn pick_row(id: impl Into<ElementId>, mark: impl IntoElement, title: impl Into<SharedString>, detail: Option<SharedString>, cx: &App) -> Control {
    control::choice(id, cx).child(mark).child(choice_text(title, detail, cx))
}

fn choice_text(title: impl Into<SharedString>, detail: Option<SharedString>, cx: &App) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .child(div().font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(title.into()))
        .when_some(detail, |d, detail| d.child(div().text_size(px(12.)).text_color(palette::muted(cx)).child(detail)))
}

// A photo avatar.
pub fn avatar(url: impl Into<SharedString>, size: f32) -> Img {
    img(url.into()).size(px(size)).flex_none().rounded_full()
}

// An avatar without a photo: the name's initial on a gradient picked from the name. Below 24px it
// drops the initial.
pub fn monogram(name: &str, size: f32) -> Div {
    let hue = name_hue(name);
    let initial: String = name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
    div()
        .size(px(size))
        .flex_none()
        .rounded_full()
        .bg(linear_gradient(135., linear_color_stop(palette::oklch(0.53, 0.12, hue), 0.), linear_color_stop(palette::oklch(0.41, 0.13, hue + 35.), 1.)))
        .flex()
        .items_center()
        .justify_center()
        .text_color(gpui_kit::white())
        .font_weight(FontWeight::BOLD)
        .text_size(px(size * 0.38))
        .when(size >= 24., |d| d.child(initial))
}

// The design's avatar hue: a stable hash of the name onto the colour wheel.
pub fn name_hue(name: &str) -> f32 {
    name.chars().fold(7_u32, |h, c| (h * 31 + c as u32) % 360) as f32
}

// An avatar with its presence mark at the corner. `ring` is the surface behind it.
pub fn with_status(avatar: impl IntoElement, status: Status, size: f32, ring: Hsla, cx: &App) -> Div {
    let mark = (size * 0.3).clamp(9., 16.);
    let inset = size * 0.04 - 2.;
    div().relative().flex_none().size(px(size)).child(avatar).child(div().absolute().right(px(inset)).bottom(px(inset)).child(status_mark(status, mark, ring, cx)))
}

// Speaking: a gap in the surface's colour, then the accent ring.
pub fn speaking_ring(ring: Hsla) -> Vec<BoxShadow> {
    vec![shadow(palette::accent(), 0., 0., 0., 4., false), shadow(ring, 0., 0., 0., 2., false)]
}

// Overlapping avatars, each ringed in `ring` (the surface colour) so they read apart.
pub fn avatar_stack(urls: impl IntoIterator<Item = String>, size: f32, ring: Hsla) -> Div {
    stacked(urls.into_iter().map(move |url| avatar(url, size).into_any_element()), size, ring)
}

// Avatars of one `size` (photos, monograms, a "+3"), overlapped 6px and ringed in the surface's colour.
// Placed absolutely in a box sized to the overlap: a flex row with negative margins laid out one avatar
// wide, so text after it overlapped the stack (cause in gpui's layout not traced).
pub fn stacked(avatars: impl IntoIterator<Item = AnyElement>, size: f32, ring: Hsla) -> Div {
    let avatars: Vec<AnyElement> = avatars.into_iter().collect();
    let step = size - 6.;
    let width = size + step * avatars.len().saturating_sub(1) as f32;
    div().relative().flex_none().w(px(width)).h(px(size)).children(avatars.into_iter().enumerate().map(move |(i, avatar)| {
        div().absolute().top_0().left(px(step * i as f32)).size(px(size)).rounded_full().shadow(vec![shadow(ring, 0., 0., 0., 2., false)]).child(avatar)
    }))
}

// A presence mark for an avatar's corner. Never colour alone: online is a dot, idle a moon, do not
// disturb a bar, offline a ring. `ring` is the surface it sits on: a 2px gap around the mark.
pub fn status_mark(status: Status, size: f32, ring: Hsla, cx: &App) -> Div {
    let color = palette::status_in(status, cx);
    let base = div().size(px(size)).flex_none().rounded_full().relative().shadow(vec![shadow(ring, 0., 0., 0., 2., false)]);
    match status {
        Status::Online => base.bg(color),
        // The moon's bite runs past the disc's edge into the ring, which is the same colour.
        Status::Idle => base.bg(color).child(div().absolute().top(px(size * (0.28 - 0.346))).left(px(size * (0.28 - 0.346))).size(px(size * 0.692)).rounded_full().bg(ring)),
        Status::Dnd => base.bg(color).flex().items_center().justify_center().child(div().w(px(size * 0.55)).h(px(size * 0.22)).bg(ring)),
        Status::Offline => base.bg(ring).child(div().size_full().rounded_full().border(px(2.5)).border_color(color)),
    }
}

// A floating surface (menu, popover, picker): rounded, ringed and shadowed, with its content inset
// so nothing reaches the rounded edge.
pub fn surface(cx: &App) -> Div {
    div().rounded(px(RADIUS_LG)).bg(palette::surface(palette::Level::Overlay, cx)).shadow(shadow_pop(cx)).p(px(SURFACE_INSET)).flex().flex_col().gap(px(1.))
}

// A row inside a surface: a neutral hover fill, inset by the surface's padding. GAP: the design fades
// this fill over 150ms; rows stay a plain `Stateful<Div>` because screens build on that type.
pub fn menu_row(id: impl Into<ElementId>, cx: &App) -> Stateful<Div> {
    let (ink, strong, hover) = (palette::text(cx), palette::strong(cx), palette::hover(cx));
    div()
        .id(id)
        .min_h(px(34.))
        .px(px(10.))
        .py(px(6.))
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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Notice {
    Accent,
    Info,
    Warning,
    Danger,
    Success,
}

// An inline notice inside the view it's about: a tinted surface, a leading icon, a title and a line.
// An action button, if any, goes last and centres itself.
pub fn callout(kind: Notice, glyph: Glyph, title: impl Into<SharedString>, body: impl Into<SharedString>, cx: &App) -> Div {
    let (tone, tint) = match kind {
        Notice::Accent => (palette::accent_ink(cx), palette::accent_soft(cx)),
        Notice::Info => (palette::info_ink(cx), palette::info_tint()),
        Notice::Warning => (palette::warning_ink(cx), palette::warning_tint()),
        Notice::Danger => (palette::danger_ink(cx), palette::danger_tint()),
        Notice::Success => (palette::positive_ink(cx), palette::success_tint()),
    };
    div()
        .w_full()
        .py(px(12.))
        .px(px(14.))
        .rounded(px(RADIUS_LG))
        .bg(tint)
        .flex()
        .gap(px(12.))
        .items_start()
        .child(div().mt(px(1.)).text_color(tone).flex_none().child(Icon::new(glyph).size(px(20.))))
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

// A toast: one line (and an optional detail) on the floating surface, rising in over 240ms.
pub fn toast(id: impl Into<ElementId>, glyph: Glyph, ink: Hsla, title: impl Into<SharedString>, detail: Option<SharedString>, cx: &App) -> AnyElement {
    let (strong, muted) = (palette::strong(cx), palette::muted(cx));
    div()
        .relative()
        .w_full()
        .max_w(px(380.))
        .flex()
        .items_center()
        .gap(px(12.))
        .py(px(10.))
        .pr(px(10.))
        .pl(px(14.))
        .rounded(px(RADIUS_LG))
        .bg(palette::surface(palette::Level::Overlay, cx))
        .shadow(shadow_pop(cx))
        .child(div().flex_none().text_color(ink).child(Icon::new(glyph).size(px(20.))))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .font_weight(FontWeight::MEDIUM)
                .text_color(strong)
                .child(title.into())
                .when_some(detail, |d, detail| d.child(div().text_size(px(12.)).font_weight(FontWeight::NORMAL).text_color(muted).child(detail))),
        )
        // PATCH(transform): rise from scale(0.97) too. PATCH(blur): and from a 4px blur.
        .with_animation(id, Animation::new(Duration::from_millis(240)).with_easing(|t| motion::ease_out().sample(t)), |toast, t| toast.opacity(t).top(px(8. * (1. - t))))
        .into_any_element()
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
        .text_center()
        .child(div().mb(px(8.)).child(mascot::mascot_at(id, mood, 0.6, cx)))
        .child(div().text_size(px(20.)).font_weight(FontWeight::BOLD).text_color(palette::strong(cx)).child(title.into()))
        .child(div().max_w(px(320.)).text_color(palette::muted(cx)).child(body.into()))
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

fn dark(cx: &App) -> bool {
    use gpui_kit::component::ActiveTheme;
    cx.theme().is_dark()
}

// Linear mix of two colours in RGB, for fills that follow a transition.
fn mix(from: Hsla, to: Hsla, t: f32) -> Hsla {
    let (a, b) = (gpui_kit::Rgba::from(from), gpui_kit::Rgba::from(to));
    let lerp = |x: f32, y: f32| x + (y - x) * t;
    gpui_kit::Rgba { r: lerp(a.r, b.r), g: lerp(a.g, b.g), b: lerp(a.b, b.b), a: lerp(a.a, b.a) }.into()
}
