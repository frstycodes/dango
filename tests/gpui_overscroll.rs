// Checks the vendored gpui patch's rubber-band overscroll from outside the crate: gpui-pre's own lib tests
// don't build standalone. Drop with the vendored gpui once upstream ships the feature.
use std::time::{Duration, Instant};

use gpui_kit::{
    Overscroll, Point, RETURN_SPRING, Size, SpringState, TouchPhase, bounce_velocity, point, px, rubber_band,
    rubber_band_inverse,
};

const D: f32 = 800.;

#[test]
fn rubber_band_starts_at_zero_and_stays_below_the_viewport() {
    assert_eq!(rubber_band(0., D), 0.);
    let mut previous = 0.;
    for x in (1..2000).map(|i| i as f32 * 10.) {
        let shown = rubber_band(x, D);
        assert!(shown > previous, "not increasing at {x}");
        assert!(shown < D, "reached the viewport at {x}");
        previous = shown;
    }
    assert_eq!(rubber_band(-120., D), -rubber_band(120., D));
}

#[test]
fn rubber_band_stiffens_with_distance() {
    let near = rubber_band(20., D) - rubber_band(10., D);
    let far = rubber_band(510., D) - rubber_band(500., D);
    assert!(far < near);
    assert!(rubber_band(10., D) < 10. * 0.55);
}

#[test]
fn rubber_band_inverse_round_trips() {
    for x in [-600., -40., 0., 3., 75., 900.] {
        let back = rubber_band_inverse(rubber_band(x, D), D);
        assert!((back - x).abs() < 0.01 * f32::max(x.abs(), 1.), "{x} came back as {back}");
    }
}

#[test]
fn return_spring_settles_to_zero_without_crossing_it() {
    for start in [-300., -12., 4., 150., 700.] {
        let mut state = SpringState { position: start, velocity: 0. };
        for _ in 0..48 {
            state = RETURN_SPRING.step(state, 0., 1. / 120.);
            assert!(state.position * start >= 0., "crossed zero from {start}");
        }
        assert!(state.position.abs() < 0.005 * f32::abs(start), "{start} still at {}", state.position);
    }
}

#[test]
fn bounce_grows_with_velocity_and_stays_below_the_viewport() {
    assert_eq!(bounce_velocity(0., D), 0.);
    let mut previous_peak = 0.;
    for speed in [500., 2000., 8000., 40000., 1e6] {
        let peak = peak_of(bounce_velocity(speed, D));
        assert!(peak > previous_peak);
        assert!(peak < D);
        previous_peak = peak;
    }
    assert!(bounce_velocity(-3000., D) < 0.);
}

fn peak_of(velocity: f32) -> f32 {
    let mut state = SpringState { position: 0., velocity };
    let mut peak = 0f32;
    for _ in 0..240 {
        state = RETURN_SPRING.step(state, 0., 1. / 240.);
        assert!(state.position >= 0., "bounce came back past the edge");
        peak = peak.max(state.position);
    }
    peak
}

fn scroller(max: f32) -> Overscroll {
    let mut overscroll = Overscroll::default();
    overscroll.set_extent(point(px(0.), px(max)), Size { width: px(400.), height: px(D) });
    overscroll
}

fn settle(overscroll: &mut Overscroll, from: Instant) -> Duration {
    let mut now = from;
    while overscroll.tick(now) {
        now += Duration::from_millis(8);
        assert!(now - from < Duration::from_secs(2), "never settled");
    }
    now - from
}

#[test]
fn dragging_past_the_top_stretches_then_springs_back_on_release() {
    let mut overscroll = scroller(1000.);
    let t = Instant::now();
    let at_top = Point::default();
    overscroll.scroll(at_top, Point::default(), TouchPhase::Started, t);
    let applied = overscroll.scroll(at_top, point(px(0.), px(100.)), TouchPhase::Moved, t);
    assert_eq!(applied.y, px(0.));
    let stretched = overscroll.offset().y;
    assert!(stretched > px(0.) && stretched < px(100.));
    assert!(!overscroll.tick(t + Duration::from_millis(100)), "sprang back while the finger was down");

    overscroll.scroll(at_top, Point::default(), TouchPhase::Ended, t);
    let took = settle(&mut overscroll, t);
    assert_eq!(overscroll.offset().y, px(0.));
    assert!(took <= Duration::from_millis(400), "took {took:?}");
}

#[test]
fn dragging_back_unwinds_the_stretch_before_scrolling() {
    let mut overscroll = scroller(1000.);
    let t = Instant::now();
    let at_top = Point::default();
    overscroll.scroll(at_top, Point::default(), TouchPhase::Started, t);
    overscroll.scroll(at_top, point(px(0.), px(60.)), TouchPhase::Moved, t);
    let applied = overscroll.scroll(at_top, point(px(0.), px(-100.)), TouchPhase::Moved, t);
    assert_eq!(overscroll.offset().y, px(0.));
    assert!((applied.y - px(-40.)).abs() < px(0.01), "applied {applied:?}");
}

#[test]
fn momentum_into_an_edge_bounces_once() {
    let mut overscroll = scroller(1000.);
    let t = Instant::now();
    let frame = Duration::from_millis(16);
    let at_bottom = point(px(0.), px(-1000.));
    overscroll.scroll(at_bottom, Point::default(), TouchPhase::Ended, t);
    overscroll.scroll(at_bottom, point(px(0.), px(-40.)), TouchPhase::Moved, t + frame);
    assert!(overscroll.tick(t + frame * 4));
    assert!(overscroll.offset().y < px(0.));
    settle(&mut overscroll, t + frame * 4);
    assert_eq!(overscroll.offset().y, px(0.));

    let later = t + Duration::from_secs(1);
    overscroll.scroll(at_bottom, point(px(0.), px(-5.)), TouchPhase::Moved, later);
    assert!(!overscroll.tick(later + frame), "the same fling bounced twice");
}

// Rubber-banding only runs on macOS, where scroll phases tell the finger from momentum.
#[cfg(target_os = "macos")]
mod headless {
    use gpui_kit::prelude::*;
    use gpui_kit::{
        Context, ListAlignment, ListState, Pixels, ScrollDelta, ScrollWheelEvent, TestAppContext, TouchPhase,
        VisualTestContext, Window, div, list, point, px,
    };

    struct Feed;

    impl Render for Feed {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(
                div()
                    .id("feed")
                    .h(px(200.))
                    .w(px(200.))
                    .overflow_y_scroll()
                    .child(div().debug_selector(|| "content".into()).h(px(1000.)).w_full()),
            )
        }
    }

    fn wheel(phase: TouchPhase, dy: f32) -> ScrollWheelEvent {
        ScrollWheelEvent {
            position: point(px(100.), px(100.)),
            delta: ScrollDelta::Pixels(point(px(0.), px(dy))),
            modifiers: Default::default(),
            touch_phase: phase,
        }
    }

    fn content_top(cx: &mut VisualTestContext) -> Pixels {
        cx.debug_bounds("content").expect("content painted").top()
    }

    #[gpui_kit::test]
    fn a_scroll_div_stretches_past_its_top_and_springs_back(cx: &mut TestAppContext) {
        let (_, cx) = cx.add_window_view(|_, _| Feed);
        let rest = content_top(cx);
        cx.simulate_event(wheel(TouchPhase::Started, 0.));
        cx.simulate_event(wheel(TouchPhase::Moved, 80.));
        cx.run_until_parked();
        let stretched = content_top(cx);
        assert!(stretched > rest, "content at {stretched:?}, rest {rest:?}");
        cx.simulate_event(wheel(TouchPhase::Ended, 0.));
        let mut previous = stretched;
        for _ in 0..50 {
            next_frame(cx);
            let top = content_top(cx);
            assert!(top <= previous && top >= rest, "{top:?} after {previous:?}");
            previous = top;
        }
        assert_eq!(content_top(cx), rest);
    }

    #[gpui_kit::test]
    fn a_bottom_aligned_list_stretches_past_its_end_and_springs_back(cx: &mut TestAppContext) {
        let (_, cx) = cx.add_window_view(|_, _| Messages(ListState::new(40, ListAlignment::Bottom, px(100.))));
        let rest = last_row_bottom(cx);
        cx.simulate_event(wheel(TouchPhase::Started, 0.));
        cx.simulate_event(wheel(TouchPhase::Moved, -80.));
        cx.run_until_parked();
        let stretched = last_row_bottom(cx);
        assert!(stretched < rest, "last row at {stretched:?}, rest {rest:?}");
        cx.simulate_event(wheel(TouchPhase::Ended, 0.));
        for _ in 0..50 {
            next_frame(cx);
        }
        assert_eq!(last_row_bottom(cx), rest);
    }

    struct Messages(ListState);

    impl Render for Messages {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(
                div().h(px(200.)).w(px(200.)).child(
                    list(self.0.clone(), |ix, _, _| {
                        let row = div().h(px(30.)).w_full();
                        if ix == 39 { row.debug_selector(|| "last".into()).into_any_element() } else { row.into_any_element() }
                    })
                    .size_full(),
                ),
            )
        }
    }

    fn last_row_bottom(cx: &mut VisualTestContext) -> Pixels {
        cx.debug_bounds("last").expect("last row painted").bottom()
    }

    // The test platform has no display link: wait out a frame (the spring runs on wall-clock time), then
    // redraw as the display link would.
    fn next_frame(cx: &mut VisualTestContext) {
        std::thread::sleep(std::time::Duration::from_millis(10));
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
    }
}

#[test]
fn content_that_fits_does_not_stretch() {
    let mut overscroll = scroller(0.);
    let t = Instant::now();
    overscroll.scroll(Point::default(), Point::default(), TouchPhase::Started, t);
    let applied = overscroll.scroll(Point::default(), point(px(0.), px(80.)), TouchPhase::Moved, t);
    assert_eq!(applied.y, px(80.));
    assert_eq!(overscroll.offset(), Point::default());
}
