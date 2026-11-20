// A popover that grows with its list up to a cap, then scrolls (Pinned Messages). gpui-component's
// `overflow_y_scrollbar` can't do both: its scroll area is `size_full` of a wrapper whose height is auto, so the
// area takes the content's full height and the wrapper's cap only clips it; nothing scrolls. And with `flex_1`
// instead of a cap, the area gives the list no height at all. The shape that works is a plain gpui scroll
// container with the cap on it, its scrollbar laid over it.
use std::{cell::Cell, rc::Rc};

use gpui_kit::component::scroll::{ScrollableElement as _, Scrollbar};
use gpui_kit::prelude::*;
use gpui_kit::{Context, ScrollDelta, ScrollHandle, ScrollWheelEvent, TestAppContext, TouchPhase, VisualTestContext, Window, canvas, div, point, px};

const HEADER: f32 = 44.;
const CAP: f32 = 500.;

#[derive(Clone, Copy)]
enum Shape {
    // What shipped in 0.13.0: the cap on a list in `overflow_y_scrollbar`.
    KitScrollbar,
    // A plain scroll container carrying the cap, with the kit's scrollbar over it.
    Plain,
}

struct Card {
    shape: Shape,
    content: f32,
    scroll: ScrollHandle,
    // The card's height and the content's top, as they paint.
    height: Rc<Cell<f32>>,
    top: Rc<Cell<f32>>,
}

impl Render for Card {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let (height, top) = (self.height.clone(), self.top.clone());
        let card_probe = canvas(move |bounds, _, _| height.set(f32::from(bounds.size.height)), |_, _, _, _| {}).absolute().size_full();
        let content_probe = canvas(move |bounds, _, _| top.set(f32::from(bounds.origin.y)), |_, _, _, _| {}).absolute().size_full();
        let content = div().relative().flex_none().h(px(self.content)).child(content_probe);
        let list = match self.shape {
            Shape::KitScrollbar => div().id("list").max_h(px(CAP)).child(content).overflow_y_scrollbar().into_any_element(),
            Shape::Plain => div()
                .relative()
                .child(div().id("list").max_h(px(CAP)).overflow_y_scroll().track_scroll(&self.scroll).child(content))
                .child(Scrollbar::vertical(&self.scroll))
                .into_any_element(),
        };
        div().size_full().child(div().relative().w(px(420.)).flex().flex_col().child(div().flex_none().h(px(HEADER))).child(list).child(card_probe))
    }
}

struct Painted {
    height: f32,
    top: f32,
}

struct Probes {
    height: Rc<Cell<f32>>,
    top: Rc<Cell<f32>>,
}

fn open(shape: Shape, content: f32, cx: &mut TestAppContext) -> (&mut VisualTestContext, Probes) {
    let probes = Probes { height: Rc::new(Cell::new(-1.)), top: Rc::new(Cell::new(-1.)) };
    let (height, top) = (probes.height.clone(), probes.top.clone());
    let (_, cx) = cx.add_window_view(move |_, _| Card { shape, content, scroll: ScrollHandle::new(), height, top });
    cx.run_until_parked();
    (cx, probes)
}

fn painted(cx: &mut VisualTestContext, probes: &Probes) -> Painted {
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    Painted { height: probes.height.get(), top: probes.top.get() }
}

fn wheel_over_list(cx: &mut VisualTestContext) {
    let delta = ScrollDelta::Pixels(point(px(0.), px(-200.)));
    cx.simulate_event(ScrollWheelEvent { position: point(px(100.), px(HEADER + 100.)), delta, modifiers: Default::default(), touch_phase: TouchPhase::Moved });
    cx.run_until_parked();
}

#[gpui_kit::test]
fn short_list_sizes_the_card(cx: &mut TestAppContext) {
    let (cx, probes) = open(Shape::Plain, 80., cx);
    assert_eq!(painted(cx, &probes).height, HEADER + 80., "header and the whole list");
}

#[gpui_kit::test]
fn long_list_caps_and_scrolls(cx: &mut TestAppContext) {
    let (cx, probes) = open(Shape::Plain, 800., cx);
    let before = painted(cx, &probes);
    assert_eq!(before.height, HEADER + CAP, "capped");
    wheel_over_list(cx);
    assert_eq!(painted(cx, &probes).top, before.top - 200., "the wheel scrolls the list");
}

#[gpui_kit::test]
fn kit_scrollbar_caps_but_never_scrolls(cx: &mut TestAppContext) {
    let (cx, probes) = open(Shape::KitScrollbar, 800., cx);
    let before = painted(cx, &probes);
    assert_eq!(before.height, HEADER + CAP, "capped");
    wheel_over_list(cx);
    assert_eq!(painted(cx, &probes).top, before.top, "gpui-component's layout: what shipped in 0.13.0");
}
