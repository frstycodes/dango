// Checks the vendored gpui patch that lets a cached view replay when a cached view around it re-renders
// (part.rs relies on it: the app re-renders as the parent of whichever column changed, and the others
// must replay). Drop with the vendored gpui once upstream does the same.
use gpui_kit::prelude::*;
use gpui_kit::{AnyView, Context, Entity, StyleRefinement, TestAppContext, Window, div};

struct Inner {
    renders: usize,
}

impl Render for Inner {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.renders += 1;
        div().size_full()
    }
}

struct Outer {
    renders: usize,
    inner: Entity<Inner>,
}

impl Render for Outer {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.renders += 1;
        div().size_full().child(AnyView::from(self.inner.clone()).cached(StyleRefinement::default().size_full()))
    }
}

struct Root {
    outer: Entity<Outer>,
}

impl Render for Root {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(AnyView::from(self.outer.clone()).cached(StyleRefinement::default().size_full()))
    }
}

#[gpui_kit::test]
fn a_cached_view_replays_when_the_one_around_it_re_renders(cx: &mut TestAppContext) {
    let (root, cx) = cx.add_window_view(|_, cx| {
        let inner = cx.new(|_| Inner { renders: 0 });
        Root { outer: cx.new(|_| Outer { renders: 0, inner }) }
    });
    cx.run_until_parked();
    let (outer, inner) = root.read_with(cx, |root, cx| (root.outer.clone(), root.outer.read(cx).inner.clone()));
    let counts = |cx: &mut gpui_kit::VisualTestContext| (outer.read_with(cx, |o, _| o.renders), inner.read_with(cx, |i, _| i.renders));
    assert_eq!(counts(cx), (1, 1));

    outer.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    assert_eq!(counts(cx), (2, 1), "the outer view re-renders, the inner one replays");

    inner.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    assert_eq!(counts(cx), (3, 2), "a notified inner view re-renders, and the views around it with it");
}
