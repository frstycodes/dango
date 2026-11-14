// Grouped settings (design/css/settings.css `.set-group`, `.set-card`, `.set-row`): a heading over a card
// of rows, each a title and description with its control at the end, hairlines between them.
use gpui_kit::{App, Div, FontWeight, InteractiveElement, IntoElement, ParentElement, SharedString, Stateful, Styled, div, prelude::FluentBuilder, px};

use super::{RADIUS_LG, RADIUS_MD, Squircle};
use crate::palette;

// A heading over a card; the caller adds the card and any caption under it.
pub fn settings_group(title: impl Into<SharedString>, cx: &App) -> Div {
    div().min_w_0().flex().flex_col().gap(px(8.)).child(div().px(px(4.)).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(title.into()))
}

// The note under a group's card: what the settings above it affect.
pub fn settings_caption(text: impl Into<SharedString>, cx: &App) -> Div {
    div().px(px(4.)).text_size(px(12.)).line_height(px(17.)).text_color(palette::muted(cx)).child(text.into())
}

// Rows on the raised surface, inset 4px so their fills clear its edge.
pub fn settings_card(cx: &App) -> Div {
    div().min_w_0().p(px(4.)).squircle(RADIUS_LG).bg(palette::surface(palette::Level::Raised, cx)).border_1().border_color(palette::line(cx)).flex().flex_col()
}

// A row in a settings card: title and description first, the control last. `line` draws the inset hairline
// above it (every row but the first), hidden while the row itself is hovered.
// GAP: CSS also hides the hairline under a hovered row (the next row's); gpui can't style a sibling.
pub fn settings_row(id: impl Into<SharedString>, line: bool, cx: &App) -> Stateful<Div> {
    let id: SharedString = id.into();
    div()
        .id(id.clone())
        .group(id.clone())
        .relative()
        .min_h(px(56.))
        .py(px(10.))
        .px(px(12.))
        .squircle(RADIUS_MD)
        .flex()
        .items_center()
        .gap(px(16.))
        .text_color(palette::text(cx))
        .when(line, |d| d.child(hairline(id, cx)))
}

fn hairline(group: SharedString, cx: &App) -> impl IntoElement {
    div().absolute().top(px(-0.5)).left(px(12.)).right(px(12.)).h(px(1.)).bg(palette::line(cx)).group_hover(group, |s| s.invisible())
}
