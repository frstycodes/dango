// A popover capped by a max height, its list in gpui-component's `overflow_y_scrollbar`: the cap goes on the list.
// `flex_1` under a card with only a max height lays the list out at zero height, since the wrapper's scroll area
// gives it no content height (the Pinned Messages popover drew its title over nothing).
use std::{cell::Cell, rc::Rc};

use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::prelude::*;
use gpui_kit::{Context, TestAppContext, Window, canvas, div, px};

struct Card {
    flex_list: bool,
    height: Rc<Cell<f32>>,
}

impl Render for Card {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let height = self.height.clone();
        let probe = canvas(move |bounds, _, _| height.set(f32::from(bounds.size.height)), |_, _, _, _| {}).absolute().size_full();
        let list = div().id("list").child(div().h(px(80.)));
        let list = if self.flex_list { list.flex_1().min_h_0() } else { list.max_h(px(500.)) };
        let card = div().relative().w(px(420.)).flex().flex_col().child(div().h(px(44.)));
        let card = if self.flex_list { card.max_h(px(560.)) } else { card };
        div().size_full().child(card.child(list.overflow_y_scrollbar()).child(probe))
    }
}

fn card_height(flex_list: bool, cx: &mut TestAppContext) -> f32 {
    let height = Rc::new(Cell::new(-1.));
    let (_, cx) = cx.add_window_view(|_, _| Card { flex_list, height: height.clone() });
    cx.run_until_parked();
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    height.get()
}

#[gpui_kit::test]
fn capped_list_keeps_its_content_height(cx: &mut TestAppContext) {
    assert_eq!(card_height(false, cx), 124., "header and the whole list");
}

#[gpui_kit::test]
fn flex_list_under_a_max_height_card_collapses(cx: &mut TestAppContext) {
    assert_eq!(card_height(true, cx), 44., "only the header: the old pins card");
}
