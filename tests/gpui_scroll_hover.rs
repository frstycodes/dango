// Checks the vendored gpui patch that holds hover back while the wheel scrolls under a still pointer.
// Drop with the vendored gpui once upstream ships the same behaviour.
use std::time::Duration;

use gpui_kit::prelude::*;
use gpui_kit::{
    Context, Entity, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, ScrollDelta, ScrollWheelEvent,
    TestAppContext, TouchPhase, VisualTestContext, Window, div, point, px,
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

struct Button {
    clicks: usize,
}

impl Render for Button {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(div().id("button").size(px(200.)).on_click(cx.listener(|this, _, _, cx| {
            this.clicks += 1;
            cx.notify();
        })))
    }
}

// A wheel or trackpad-inertia event between press and release used to leave the window scrolling, where
// nothing counts as hovered, so the release missed the button and the click was dropped.
#[gpui_kit::test]
fn a_wheel_event_between_press_and_release_keeps_the_click(cx: &mut TestAppContext) {
    let (button, cx) = cx.add_window_view(|_, _| Button { clicks: 0 });
    let at = point(px(50.), px(50.));
    move_to(cx, at);
    cx.simulate_event(MouseDownEvent { position: at, button: MouseButton::Left, modifiers: Default::default(), click_count: 1, first_mouse: false });
    wheel(cx, TouchPhase::Moved);
    cx.simulate_event(MouseUpEvent { position: at, button: MouseButton::Left, modifiers: Default::default(), click_count: 1 });
    cx.run_until_parked();
    assert_eq!(button.read_with(cx, |b, _| b.clicks), 1);
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
