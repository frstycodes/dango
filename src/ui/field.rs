// Fields: the labelled column, text and textarea frames with the focus ring, the dropdown, the stepper,
// one-time-code cells and the drop zone. Labels sit above, help and errors below. Focus is the accent
// border plus a soft 3px ring, fading in over the hover timing.
use std::f32::consts::PI;

use gpui_kit::{
    AnyElement, App, BoxShadow, Div, ElementId, FocusHandle, FontWeight, Hsla, InteractiveElement, IntoElement, ParentElement, RenderOnce, SharedString, Styled, Window,
    base::{Transition, transition},
    component::{ActiveTheme, Icon},
    div, point, prelude::FluentBuilder, px, radians,
};

use super::{
    DUR_POP, RADIUS_LG, RADIUS_MD, Squircle,
    control::{Control, hover_fade, icon_control, part},
    mix,
};
use crate::{icons::Glyph, motion, palette};

// A labelled field: the label, then whatever the caller adds (a frame, a hint, an error).
pub fn field(label: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .min_w_0()
        .child(div().text_size(px(12.)).font_weight(FontWeight::MEDIUM).text_color(palette::muted(cx)).child(label.into()))
}

// Help under a field, with an optional count or note at the far end.
pub fn field_hint(text: impl Into<SharedString>, end: Option<SharedString>, cx: &App) -> Div {
    div().flex().justify_between().gap(px(8.)).text_size(px(12.)).text_color(palette::faint(cx)).child(text.into()).when_some(end, |d, end| d.child(end))
}

// What went wrong and how to fix it, under the field, led by the filled warning mark.
pub fn field_error(text: impl Into<SharedString>, cx: &App) -> Div {
    div().flex().items_center().gap(px(4.)).text_size(px(12.)).text_color(palette::danger_ink(cx)).child(Icon::new(Glyph::WarningCircle).size(px(14.))).child(text.into())
}

// The static frame of a text input (`.input`): 40px, the page fill, a strong hairline, 16px strong text.
// For a field that needs the focus fade, use `text_field`.
pub fn field_frame(cx: &App) -> Div {
    div()
        .h(px(40.))
        .px(px(12.))
        .squircle(RADIUS_MD)
        .bg(palette::surface(palette::Level::Page, cx))
        .border_1()
        .border_color(palette::line_strong(cx))
        .flex()
        .items_center()
        .gap(px(8.))
        .text_size(px(16.))
        .text_color(palette::strong(cx))
}

// The frame around a text input with its focus fade; focus takes the accent edge and ring. `input` is
// the editor (gpui-component's `Input` with `.appearance(false)`), `focus` its handle.
pub fn text_field(id: impl Into<ElementId>, input: impl IntoElement, focus: &FocusHandle) -> TextField {
    TextField { id: id.into(), input: input.into_any_element(), focus: focus.clone(), leading: Vec::new(), trailing: Vec::new(), error: false, disabled: false, multiline: false }
}

// The same frame for a multi-line input: at least 88px, 10px top and bottom.
pub fn text_area(id: impl Into<ElementId>, input: impl IntoElement, focus: &FocusHandle) -> TextField {
    TextField { multiline: true, ..text_field(id, input, focus) }
}

#[derive(IntoElement)]
pub struct TextField {
    id: ElementId,
    input: AnyElement,
    focus: FocusHandle,
    leading: Vec<AnyElement>,
    trailing: Vec<AnyElement>,
    error: bool,
    disabled: bool,
    multiline: bool,
}

impl TextField {
    // Before the text: a search glyph, a `#` affix.
    pub fn leading(mut self, el: impl IntoElement) -> Self {
        self.leading.push(el.into_any_element());
        self
    }

    // After the text: shortcut keys, a show-password button.
    pub fn trailing(mut self, el: impl IntoElement) -> Self {
        self.trailing.push(el.into_any_element());
        self
    }

    pub fn error(mut self, error: bool) -> Self {
        self.error = error;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl RenderOnce for TextField {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let focused = self.focus.contains_focused(window, cx) && !self.disabled;
        let t = hover_fade(part(&self.id, "focus"), focused, window, cx);
        let rest_edge = if self.error { palette::danger_ink(cx) } else { palette::line_strong(cx) };
        let edge = if self.disabled { gpui_kit::transparent_black() } else { mix(rest_edge, palette::accent(), t) };
        field_frame(cx)
            .id(self.id)
            .w_full()
            .border_color(edge)
            .when(self.disabled, |d| d.bg(palette::selected(cx)).text_color(palette::faint(cx)))
            .shadow(focus_ring(palette::accent_soft(cx), t))
            .when(self.multiline, |d| d.h_auto().min_h(px(88.)).py(px(10.)).items_start())
            .children(self.leading.into_iter().map(|el| div().flex_none().text_color(palette::faint(cx)).child(el)))
            .child(div().flex_1().min_w_0().child(self.input))
            .children(self.trailing.into_iter().map(|el| div().flex_none().flex().items_center().gap(px(4.)).text_color(palette::faint(cx)).child(el)))
    }
}

// The soft ring outside a focused field, grown in by `t`.
pub(super) fn focus_ring(soft: Hsla, t: f32) -> Vec<BoxShadow> {
    if t <= 0. {
        return Vec::new();
    }
    vec![BoxShadow { color: soft.opacity(t), offset: point(px(0.), px(0.)), blur_radius: px(0.), spread_radius: px(3. * t), inset: false }]
}

// A dropdown's trigger: the chosen value, a caret that turns over while its list is open, the accent edge.
pub fn select(id: impl Into<ElementId>, glyph: Option<Glyph>, value: impl Into<SharedString>, open: bool, cx: &App) -> Control {
    let id = id.into();
    super::control::field_trigger(id.clone(), open, cx)
        .when_some(glyph, |c, glyph| c.child(div().flex_none().text_color(palette::muted(cx)).child(Icon::new(glyph).size(px(18.)))))
        .child(div().flex_1().min_w_0().truncate().child(value.into()))
        .child(Caret { id: part(&id, "caret"), open, ink: palette::faint(cx) })
}

// The dropdown's caret, rotating half a turn over the pop timing as the list opens and closes.
#[derive(IntoElement)]
struct Caret {
    id: ElementId,
    open: bool,
    ink: Hsla,
}

impl RenderOnce for Caret {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let turn = transition(self.id, if self.open { 1. } else { 0. }, Transition::new(DUR_POP).easing(motion::ease_out()), window, cx);
        div().flex_none().flex().text_color(self.ink).rotate(radians(PI * turn)).child(Icon::new(Glyph::ChevronDown).size(px(14.)))
    }
}

// A number nudged in steps: minus, the value, plus. Callers wire the two buttons' clicks.
pub fn stepper(id: impl Into<ElementId>, value: impl Into<SharedString>, less: impl Fn(&gpui_kit::ClickEvent, &mut Window, &mut App) + 'static, more: impl Fn(&gpui_kit::ClickEvent, &mut Window, &mut App) + 'static, cx: &App) -> Div {
    use gpui_kit::StatefulInteractiveElement;
    let id = id.into();
    let side = |name, cx: &App| icon_control(part(&id, name), cx).w(px(36.)).h_full().rounded(px(0.));
    div()
        .h(px(40.))
        .flex()
        .items_center()
        .squircle(RADIUS_MD)
        .border_1()
        .border_color(palette::line_strong(cx))
        .bg(palette::surface(palette::Level::Page, cx))
        .child(side("less", cx).rounded_l(px(RADIUS_MD - 1.)).child(Icon::new(Glyph::MinusBold).size(px(18.))).on_click(less))
        .child(div().min_w(px(44.)).text_center().font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(value.into()))
        .child(side("more", cx).rounded_r(px(RADIUS_MD - 1.)).child(Icon::new(Glyph::Plus).size(px(18.))).on_click(more))
}

// One cell of a one-time code: 44x52, the digit in mono bold 20, the focus edge and ring on the cell typed next.
pub fn code_cell(digit: Option<char>, focused: bool, cx: &App) -> Div {
    div()
        .w(px(44.))
        .h(px(52.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .squircle(RADIUS_MD)
        .border_1()
        .border_color(if focused { palette::accent() } else { palette::line_strong(cx) })
        .shadow(focus_ring(palette::accent_soft(cx), if focused { 1. } else { 0. }))
        .bg(palette::surface(palette::Level::Page, cx))
        .font_family(cx.theme().mono_font_family.clone())
        .text_size(px(20.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(palette::strong(cx))
        .when_some(digit, |d, digit| d.child(SharedString::from(digit.to_string())))
}

// Where files can be dropped: a dashed edge that takes the accent while something is over it.
pub fn drop_zone(over: bool, glyph: Glyph, title: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .w_full()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(6.))
        .p(px(24.))
        .squircle(RADIUS_LG)
        .border(px(1.5))
        .border_dashed()
        .border_color(if over { palette::accent() } else { palette::line_strong(cx) })
        .when(over, |d| d.bg(palette::accent_soft(cx)))
        .text_color(palette::muted(cx))
        .text_center()
        .child(div().text_color(if over { palette::accent_ink(cx) } else { palette::faint(cx) }).child(Icon::new(glyph).size(px(28.))))
        .child(div().font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(title.into()))
}
