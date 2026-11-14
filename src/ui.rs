// Rustcord's shared controls. Each returns a styled element the caller keeps composing (`.on_click`,
// layout, extra children), so call sites never restate a control's look.
//
// Clipping: gpui's `overflow_hidden` clips to a plain rectangle, never to rounded corners. Anything
// painted against a rounded edge (an image, a header strip, a hover fill) must round itself to
// match, or sit inset by the surface's padding. `surface` insets its content; `media` and `edge`
// round theirs.
use gpui_kit::{
    App, Corners, Div, ElementId, FontWeight, Hsla, ImageSource, Img, InteractiveElement, ParentElement, Pixels, SharedString, Stateful, StatefulInteractiveElement, Styled, StyledImage, Window,
    component::{ActiveTheme, Icon, tooltip::Tooltip},
    div, img, prelude::FluentBuilder, px,
};

use crate::{icons::Glyph, palette};

// The radius tiers, Discord's: small marks, inset items, controls, surfaces.
pub const RADIUS_SM: f32 = 4.;
pub const RADIUS_MD: f32 = 6.;
pub const RADIUS_LG: f32 = 8.;
pub const RADIUS_XL: f32 = 12.;
// Padding a surface keeps between its rounded edge and its rows.
pub const SURFACE_INSET: f32 = 8.;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tone {
    // Blurple, the one accent: the action the surface is for.
    Primary,
    // The grey wash: every other action.
    Secondary,
    // Red: irreversible actions.
    Danger,
    // Translucent black over video and images.
    OnMedia,
}

// A text button, 32px (38px with `large`). Callers add the label and `.on_click`.
pub fn button(id: impl Into<ElementId>, tone: Tone, cx: &App) -> Stateful<Div> {
    let (fill, hover, ink) = tone_colors(tone, cx);
    div()
        .id(id)
        .h(px(32.))
        .px(px(16.))
        .flex_none()
        .rounded(px(RADIUS_LG))
        .flex()
        .items_center()
        .justify_center()
        .gap_2()
        .cursor_pointer()
        .bg(fill)
        .text_color(ink)
        .text_size(px(14.))
        .font_weight(FontWeight::MEDIUM)
        .hover(move |s| s.bg(hover))
        .active(move |s| s.bg(hover).opacity(0.9))
}

pub fn large(button: Stateful<Div>) -> Stateful<Div> {
    button.h(px(38.)).text_size(px(16.))
}

fn tone_colors(tone: Tone, cx: &App) -> (Hsla, Hsla, Hsla) {
    match tone {
        Tone::Primary => (palette::blurple(), palette::blurple_hover(), gpui_kit::white()),
        Tone::Secondary => (palette::wash(0.12), palette::wash(0.2), cx.theme().foreground),
        Tone::Danger => (palette::danger(), palette::danger_hover(), gpui_kit::white()),
        Tone::OnMedia => (gpui_kit::black().opacity(0.6), gpui_kit::black().opacity(0.8), gpui_kit::white()),
    }
}

// A square icon-only button that fills on hover, with its tooltip.
pub fn icon_button(id: impl Into<ElementId>, glyph: Glyph, tooltip: impl Into<SharedString>, cx: &App) -> Stateful<Div> {
    let theme = cx.theme();
    let (muted, ink) = (theme.muted_foreground, theme.foreground);
    let tooltip: SharedString = tooltip.into();
    div()
        .id(id)
        .size(px(32.))
        .flex_none()
        .rounded(px(RADIUS_LG))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .text_color(muted)
        .hover(move |s| s.bg(palette::wash(0.12)).text_color(ink))
        .active(|s| s.bg(palette::wash(0.2)))
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

// The red LIVE pill. White on it is 3.8:1, the user's pick for its small label.
pub fn live_badge() -> Div {
    badge(palette::live(), gpui_kit::white()).child("LIVE")
}

// A small solid label (quality, tags, NEW).
pub fn badge(fill: Hsla, ink: Hsla) -> Div {
    div().h(px(18.)).px(px(5.)).flex_none().rounded(px(RADIUS_SM)).bg(fill).text_color(ink).text_size(px(11.)).font_weight(FontWeight::BOLD).flex().items_center()
}

// Discord's red mention count, a capsule that grows with the number.
pub fn count_badge(count: u32) -> Div {
    let text = if count > 99 { "99+".to_owned() } else { count.to_string() };
    div()
        .min_w(px(16.))
        .h(px(16.))
        .px(px(4.))
        .flex_none()
        .rounded_full()
        .bg(palette::danger())
        .text_color(gpui_kit::white())
        .text_size(px(11.))
        .font_weight(FontWeight::BOLD)
        .flex()
        .items_center()
        .justify_center()
        .child(text)
}

// The joined track a row of `segment`s sits in. Segments are inset 2px with a radius 2px smaller,
// so the selected fill is concentric with the track.
pub fn segmented() -> Div {
    div().p(px(2.)).rounded(px(RADIUS_LG)).bg(palette::wash(0.12)).flex()
}

pub fn segment(id: impl Into<ElementId>, label: impl Into<SharedString>, on: bool, cx: &App) -> Stateful<Div> {
    let theme = cx.theme();
    let (strong, muted) = (palette::strong(cx), theme.muted_foreground);
    let raised = theme.popover;
    div()
        .id(id)
        .flex_1()
        .min_w_0()
        .h(px(28.))
        .px_3()
        .rounded(px(RADIUS_MD))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .text_size(px(14.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(if on { strong } else { muted })
        .when(on, |s| s.bg(raised).shadow_sm())
        .when(!on, |s| s.hover(move |s| s.text_color(strong)))
        .child(label.into())
}

// Discord's toggle switch: 40×24, blurple when on.
pub fn switch(on: bool, cx: &App) -> Div {
    let off = cx.theme().input;
    div()
        .w(px(40.))
        .h(px(24.))
        .flex_none()
        .p(px(3.))
        .rounded_full()
        .border_1()
        .border_color(if on { palette::blurple() } else { palette::wash(0.3) })
        .bg(if on { palette::blurple() } else { off })
        .flex()
        .items_center()
        .when(on, |d| d.justify_end())
        .child(div().size(px(16.)).rounded_full().bg(gpui_kit::white()))
}

pub fn checkbox(on: bool) -> Div {
    div()
        .size(px(20.))
        .flex_none()
        .rounded(px(RADIUS_SM))
        .border_1()
        .border_color(if on { palette::blurple() } else { palette::wash(0.64) })
        .bg(if on { palette::blurple() } else { gpui_kit::transparent_black() })
        .text_color(gpui_kit::white())
        .flex()
        .items_center()
        .justify_center()
        .when(on, |d| d.child(Icon::new(Glyph::Checkmark).size(px(14.))))
}

pub fn radio(on: bool, cx: &App) -> Div {
    let ring = if on { palette::blurple() } else { cx.theme().muted_foreground };
    div()
        .size(px(18.))
        .flex_none()
        .rounded_full()
        .border_2()
        .border_color(ring)
        .flex()
        .items_center()
        .justify_center()
        .when(on, |d| d.child(div().size(px(8.)).rounded_full().bg(gpui_kit::white())))
        .when(on, |d| d.bg(palette::blurple()))
}

pub fn avatar(url: impl Into<SharedString>, size: f32) -> Img {
    img(url.into()).size(px(size)).flex_none().rounded_full()
}

// Overlapping avatars, each ringed in `ring` (the surface colour) so they read apart.
pub fn avatar_stack(urls: impl IntoIterator<Item = String>, size: f32, ring: Hsla) -> Div {
    div().flex().items_center().pl(px(size * 0.3)).children(urls.into_iter().map(move |url| avatar(url, size).border_2().border_color(ring).ml(px(-size * 0.3))))
}

// A floating surface (menu, popover, picker): rounded, bordered, shadowed, with its content inset
// so nothing reaches the rounded edge.
pub fn surface(cx: &App) -> Div {
    let theme = cx.theme();
    div().rounded(px(RADIUS_XL)).border_1().border_color(palette::wash(0.2)).bg(theme.popover).shadow_lg().p(px(SURFACE_INSET)).flex().flex_col()
}

// A row inside a surface: hover fills blurple like Discord's menus, and stays inset by the
// surface's padding, never flush with its edge.
pub fn menu_row(id: impl Into<ElementId>, cx: &App) -> Stateful<Div> {
    let ink = cx.theme().foreground;
    div()
        .id(id)
        .min_h(px(32.))
        .px_2()
        .rounded(px(RADIUS_SM))
        .flex()
        .items_center()
        .gap_2()
        .cursor_pointer()
        .text_size(px(14.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(ink)
        .hover(|s| s.bg(palette::blurple()).text_color(gpui_kit::white()))
}

// A list row in a sidebar or panel: Discord's grey hover, a stronger fill when selected.
pub fn list_row(id: impl Into<ElementId>, selected: bool, cx: &App) -> Stateful<Div> {
    let accent = cx.theme().sidebar_accent;
    div()
        .id(id)
        .px_2()
        .rounded(px(RADIUS_LG))
        .flex()
        .items_center()
        .gap_2()
        .cursor_pointer()
        .when(selected, |d| d.bg(accent))
        .when(!selected, |d| d.hover(move |s| s.bg(accent.opacity(0.6))))
}

// A small heading over a group ("Roles", "Recent"), sentence case.
pub fn section_label(text: impl Into<SharedString>, cx: &App) -> Div {
    div().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(cx.theme().muted_foreground).child(text.into())
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
pub fn edge(el: Div, corners: Corners<Pixels>) -> Div {
    el.rounded_tl(corners.top_left).rounded_tr(corners.top_right).rounded_bl(corners.bottom_left).rounded_br(corners.bottom_right)
}
