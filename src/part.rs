// The window's columns (the sidebar, the main column, the side panel), each a cached view of its own, so a
// list scrolling or an animation running in one column rebuilds and lays out that column alone: gpui
// notifies the view being drawn, and replays the cached views beside it (see the patch in gpui's view.rs).
//
// A part re-renders whenever the app notifies, so it never shows older state than the rest of the window.
// Each loads its images through a cache stamped by its own renders: a shared cache stamped by the app's
// would evict a replaying column's images while they are still on screen.
use gpui_kit::{AnyElement, AnyView, Context, Entity, IntoElement, Render, StyleRefinement, Subscription, WeakEntity, Window, div, image_cache, prelude::*};

use crate::{Waffle, images::Images};

type RenderFn = fn(&mut Waffle, &mut Window, &mut Context<Waffle>) -> AnyElement;

pub struct Part {
    waffle: WeakEntity<Waffle>,
    render: RenderFn,
    images: Entity<Images>,
    _observe: Subscription,
}

impl Render for Part {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.images.update(cx, |images, cx| images.next_frame(cx));
        let render = self.render;
        let content = self.waffle.update(cx, |waffle, cx| render(waffle, window, cx)).unwrap_or_else(|_| div().into_any_element());
        image_cache(self.images.clone()).size_full().child(content)
    }
}

// The app's parts, made on first draw.
#[derive(Default)]
pub struct Parts {
    nav: Option<Entity<Part>>,
    main: Option<Entity<Part>>,
    panel: Option<Entity<Part>>,
}

#[derive(Clone, Copy)]
pub enum Column {
    Nav,
    Main,
    Panel,
}

impl Waffle {
    // `column` as a cached view laid out by `style`; its own content can't size it.
    pub fn part(&mut self, column: Column, style: StyleRefinement, cx: &mut Context<Self>) -> AnyElement {
        let (slot, render): (&mut Option<Entity<Part>>, RenderFn) = match column {
            Column::Nav => (&mut self.parts.nav, |w, window, cx| w.render_nav(window, cx).into_any_element()),
            Column::Main => (&mut self.parts.main, Waffle::render_main),
            Column::Panel => (&mut self.parts.panel, |w, window, cx| w.render_panel(window, cx).unwrap_or_else(|| div().into_any_element())),
        };
        let waffle = cx.entity();
        let part = slot.get_or_insert_with(|| {
            cx.new(|cx: &mut Context<Part>| Part {
                waffle: waffle.downgrade(),
                render,
                images: crate::images::new_cache(cx),
                _observe: cx.observe(&waffle, |_, _, cx| cx.notify()),
            })
        });
        AnyView::from(part.clone()).cached(style).into_any_element()
    }
}
