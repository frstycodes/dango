// Checks the vendored gpui patch that holds hover back while the wheel scrolls under a still pointer.
// Drop with the vendored gpui once upstream ships the same behaviour.
use std::time::Duration;

use gpui_kit::prelude::*;
use gpui_kit::{
    Context, Entity, MouseMoveEvent, Pixels, Point, ScrollDelta, ScrollWheelEvent, TestAppContext, TouchPhase,
    VisualTestContext, Window, div, point, px,
};

struct Row {
    hovered: bool,
}

impl Render for Row {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(div().id("row").size(px(200.)).on_hover(cx.listener(|this, hovered: &bool, _, cx| {
            this.hovered = *hovered;
            cx.notify();
        })))
    }
}

#[gpui_kit::test]
fn wheel_holds_hover_until_the_pointer_moves_or_the_scroll_ends(cx: &mut TestAppContext) {
    let (row, cx) = cx.add_window_view(|_, _| Row { hovered: false });

    move_to(cx, point(px(50.), px(50.)));
    assert!(hovered(&row, cx), "pointer over the row");

    wheel(cx, TouchPhase::Moved);
    assert!(!hovered(&row, cx), "hover held while scrolling");
    move_to(cx, point(px(50.), px(50.)));
    assert!(!hovered(&row, cx), "a move that goes nowhere is the platform echoing the scroll");
    wait(cx);
    assert!(hovered(&row, cx), "hover back once wheel events stop");

    wheel(cx, TouchPhase::Started);
    wait(cx);
    assert!(!hovered(&row, cx), "a finger resting on the trackpad is still scrolling");
    wheel(cx, TouchPhase::Ended);
    wait(cx);
    assert!(hovered(&row, cx), "hover back once the finger lifts and nothing follows");

    wheel(cx, TouchPhase::Moved);
    move_to(cx, point(px(60.), px(60.)));
    assert!(hovered(&row, cx), "hover back as soon as the pointer moves");
}

fn wait(cx: &mut VisualTestContext) {
    cx.executor().advance_clock(Duration::from_millis(200));
    cx.run_until_parked();
}

fn hovered(row: &Entity<Row>, cx: &mut VisualTestContext) -> bool {
    row.read_with(cx, |row, _| row.hovered)
}

fn move_to(cx: &mut VisualTestContext, position: Point<Pixels>) {
    cx.simulate_event(MouseMoveEvent { position, pressed_button: None, modifiers: Default::default() });
    cx.run_until_parked();
}

fn wheel(cx: &mut VisualTestContext, touch_phase: TouchPhase) {
    cx.simulate_event(ScrollWheelEvent {
        position: point(px(50.), px(50.)),
        delta: ScrollDelta::Pixels(point(px(0.), px(-20.))),
        modifiers: Default::default(),
        touch_phase,
    });
    cx.run_until_parked();
}
