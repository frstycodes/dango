// Option strips with one sliding indicator: underline tabs, pill tabs and the segmented control. Like
// the design (site.js measures the chosen option and slides one element there), each strip measures
// its options after layout and moves a single indicator to the chosen one, so a change slides across
// rather than one mark collapsing while another grows.
use std::{rc::Rc, time::Duration};

use gpui_kit::{
    App, Bounds, Div, ElementId, Entity, ParentElement, Pixels, SharedString, StatefulInteractiveElement, Styled, Window,
    base::{Easing, Transition, transition},
    div, prelude::FluentBuilder, px,
};

use super::{RADIUS_MD, RADIUS_SM, Squircle, control::{TabKind, part, tab_option}, count_badge, seg_lift};
use crate::{motion, palette};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Style {
    Underline,
    Pills,
    Segmented,
}

// Underline tabs split a page into views. `items` are labels with an optional count.
pub fn tabs(id: impl Into<ElementId>, items: &[(SharedString, Option<u32>)], selected: usize, on_select: impl Fn(usize, &mut Window, &mut App) + 'static, window: &mut Window, cx: &mut App) -> Div {
    strip(id.into(), Style::Underline, items, selected, Rc::new(on_select), window, cx)
}

// Pill tabs filter a list inside one view.
pub fn pill_tabs(id: impl Into<ElementId>, items: &[(SharedString, Option<u32>)], selected: usize, on_select: impl Fn(usize, &mut Window, &mut App) + 'static, window: &mut Window, cx: &mut App) -> Div {
    strip(id.into(), Style::Pills, items, selected, Rc::new(on_select), window, cx)
}

// A recessed track with one raised thumb that springs between options sized to their labels.
pub fn segmented_live(id: impl Into<ElementId>, labels: &[&'static str], selected: usize, on_select: impl Fn(usize, &mut Window, &mut App) + 'static, window: &mut Window, cx: &mut App) -> Div {
    let items: Vec<(SharedString, Option<u32>)> = labels.iter().map(|l| (SharedString::from(*l), None)).collect();
    strip(id.into(), Style::Segmented, &items, selected, Rc::new(on_select), window, cx)
}

type OnSelect = Rc<dyn Fn(usize, &mut Window, &mut App)>;

// The options' bounds from the last layout, relative to the strip; the first entry is the strip itself.
#[derive(Default)]
struct Measured(Vec<Bounds<Pixels>>);

fn strip(id: ElementId, style: Style, items: &[(SharedString, Option<u32>)], selected: usize, on_select: OnSelect, window: &mut Window, cx: &mut App) -> Div {
    let measured = window.use_keyed_state(part(&id, "measured"), cx, |_, _| Measured::default());
    let indicator = slide(&id, style, &measured, selected, window, cx);
    let kind = match style {
        Style::Underline => TabKind::Underline,
        Style::Pills => TabKind::Pill,
        Style::Segmented => TabKind::Segment,
    };
    let options = items.iter().enumerate().map(|(i, (label, count))| {
        let on_select = on_select.clone();
        tab_option(part_n(&id, i), kind, i == selected, cx)
            .child(label.clone())
            .when_some(*count, |o, n| o.child(count_badge(n)))
            .on_click(move |_, window, cx| on_select(i, window, cx))
    });
    let frame = match style {
        Style::Underline => div().flex().gap(px(16.)).border_b_1().border_color(palette::line(cx)),
        Style::Pills => div().flex().flex_wrap().gap(px(4.)),
        Style::Segmented => div().flex().p(px(3.)).gap(px(2.)).squircle(RADIUS_MD).bg(palette::seg_track(cx)),
    };
    frame
        .relative()
        // The measuring layer comes first: its bounds are the strip's, the options' follow.
        .child(div().absolute().top_0().left_0().size_full().children(indicator))
        .children(options)
        .on_children_prepainted(move |bounds, _, cx| {
            measured.update(cx, |m, cx| {
                if m.0 != bounds {
                    m.0 = bounds;
                    cx.notify();
                }
            })
        })
}

// The indicator at the chosen option, sliding there over the strip's timing once it has been measured.
fn slide(id: &ElementId, style: Style, measured: &Entity<Measured>, selected: usize, window: &mut Window, cx: &mut App) -> Option<Div> {
    let target = {
        let bounds = &measured.read(cx).0;
        let (strip, option) = (bounds.first()?, bounds.get(selected + 1)?);
        option.origin - strip.origin
    };
    let size = measured.read(cx).0[selected + 1].size;
    let (duration, easing): (Duration, Easing) = match style {
        Style::Segmented => (Duration::from_millis(280), motion::ease_spring()),
        _ => (Duration::from_millis(220), motion::ease_move()),
    };
    let channel = |name: &'static str, value: f32, window: &mut Window, cx: &mut App| transition(part(id, name), value, Transition::new(duration).easing(easing.clone()), window, cx);
    let x = channel("x", target.x.into(), window, cx);
    let w = channel("w", size.width.into(), window, cx);
    let y = channel("y", target.y.into(), window, cx);
    let h = channel("h", size.height.into(), window, cx);
    let base = div().absolute().left(px(x)).w(px(w));
    Some(match style {
        Style::Underline => base.bottom_0().h(px(2.)).squircle(2.).bg(palette::accent()),
        Style::Pills => base.top(px(y)).h(px(h)).squircle(RADIUS_SM).bg(palette::selected(cx)),
        Style::Segmented => base.top(px(y)).h(px(h)).squircle(RADIUS_MD - 3.).bg(palette::seg_thumb(cx)).shadow(seg_lift(cx)),
    })
}

fn part_n(id: &ElementId, i: usize) -> ElementId {
    ElementId::NamedChild(std::sync::Arc::new(id.clone()), SharedString::from(format!("option-{i}")))
}
