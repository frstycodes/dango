// Grouped settings (design/css/settings.css `.set-group`, `.set-card`, `.set-row`), iOS inset-grouped style: a
// heading over a borderless card of rows, each a title and description with its control at the end, inset
// hairlines between them.
use gpui_kit::{App, ClickEvent, Div, ElementId, ScrollHandle, FontWeight, InteractiveElement, IntoElement, ParentElement, SharedString, Stateful, StatefulInteractiveElement, Styled, Window, component::Icon, div, prelude::FluentBuilder, px};

use super::{Control, RADIUS_XL, Squircle, menu_row, pressable};
use crate::{icons::Glyph, palette};

// A heading over a card; the caller adds the card and any caption under it.
pub fn settings_group(title: impl Into<SharedString>, cx: &App) -> Div {
    div().min_w_0().flex().flex_col().gap(px(8.)).child(div().px(px(4.)).font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(title.into()))
}

// The note under a group's card: what the settings above it affect.
pub fn settings_caption(text: impl Into<SharedString>, cx: &App) -> Div {
    div().px(px(4.)).text_size(px(12.)).line_height(px(17.)).text_color(palette::muted(cx)).child(text.into())
}

const CARD_INSET: f32 = 4.;

// Rows on the raised surface, one step from the page with no border, inset so their fills clear its edge.
pub fn settings_card(cx: &App) -> Div {
    div().min_w_0().p(px(CARD_INSET)).squircle(RADIUS_XL).bg(palette::surface(palette::Level::Group, cx)).flex().flex_col()
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
        .squircle(RADIUS_XL - CARD_INSET)
        .flex()
        .items_center()
        .gap(px(16.))
        .text_color(palette::text(cx))
        .when(line, |d| d.child(hairline(id, cx)))
}

fn hairline(group: SharedString, cx: &App) -> impl IntoElement {
    div().absolute().top(px(-0.5)).left(px(12.)).right(px(12.)).h(px(1.)).bg(palette::line(cx)).group_hover(group, |s| s.invisible())
}

// A settings page's big title.
pub fn settings_title(text: impl Into<SharedString>, cx: &App) -> Div {
    div().text_size(px(28.)).line_height(px(34.)).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).truncate().child(text.into())
}

// A settings row's title with its muted description under it.
pub fn settings_text(title: impl Into<SharedString>, detail: Option<impl Into<SharedString>>, cx: &App) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(2.))
        .child(div().truncate().font_weight(FontWeight::NORMAL).text_color(palette::strong(cx)).child(title.into()))
        .children(detail.map(|d| div().text_size(px(12.)).line_height(px(17.)).text_color(palette::muted(cx)).child(d.into())))
}

// A whole-row control: the row fills on hover.
pub fn settings_choice(row: Stateful<Div>, cx: &App) -> Stateful<Div> {
    let hover = palette::hover(cx);
    row.cursor_pointer().hover(move |s| s.bg(hover))
}

// A settings window's nav entry: its icon, then its label; the open page carries the selected fill.
pub fn settings_nav_item(id: impl Into<ElementId>, glyph: Glyph, label: impl Into<SharedString>, selected: bool, cx: &App) -> Control {
    let ink = if selected { palette::strong(cx) } else { palette::muted(cx) };
    menu_row(id, cx).toggled(selected, cx).child(div().text_color(ink).child(Icon::new(glyph).size(px(18.)))).child(div().flex_1().min_w_0().truncate().child(label.into()))
}

// Pinned top right as a settings page scrolls: a ringed circle with "Esc" under it.
pub fn settings_close(id: impl Into<ElementId>, on_close: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static, cx: &App) -> Div {
    let (muted, strong, hover) = (palette::muted(cx), palette::strong(cx), palette::hover(cx));
    // The Windows caption buttons take the window's top right corner.
    let top = if cfg!(target_os = "windows") { crate::layout::TITLE_HEIGHT + 8. } else { 32. };
    div()
        .absolute()
        .top(px(top))
        .right(px(24.))
        .flex()
        .flex_col()
        .items_center()
        .gap(px(4.))
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(muted)
        .child(
            pressable(div().id(id.into()), 0.94)
                .size(px(36.))
                .rounded_full()
                .border(px(1.5))
                .border_color(palette::line_strong(cx))
                .bg(palette::surface(palette::Level::Page, cx))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .text_color(muted)
                .hover(move |s| s.bg(hover).text_color(strong))
                .child(Icon::new(Glyph::X).size(px(18.)))
                .on_click(on_close),
        )
        .child("Esc")
}

// A settings window: its nav on the sidebar tone, then the page, over the whole app window.
pub fn settings_window(id: impl Into<ElementId>, nav: impl IntoElement, main: impl IntoElement, cx: &App) -> Stateful<Div> {
    div()
        .id(id.into())
        .absolute()
        .inset_0()
        .occlude()
        .flex()
        .text_size(px(14.))
        .text_color(palette::text(cx))
        .on_mouse_down(gpui_kit::MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(nav)
        .child(main)
}

// A settings window's nav column. On macOS the traffic lights sit over its top, as in System Settings,
// and that strip drags the window.
pub fn settings_sidebar(id: impl Into<ElementId>, cx: &App) -> Stateful<Div> {
    div()
        .id(id.into())
        .w(px(240.))
        .flex_none()
        .h_full()
        .overflow_y_scroll()
        .bg(palette::surface(palette::Level::Sidebar, cx))
        .pt(px(if cfg!(target_os = "macos") { 0. } else { 16. }))
        .px(px(8.))
        .pb(px(12.))
        .flex()
        .flex_col()
        .gap(px(1.))
        .when(cfg!(target_os = "macos"), |d| d.child(div().h(px(crate::layout::TITLE_HEIGHT + 12.)).flex_none().window_control_area(gpui_kit::WindowControlArea::Drag)))
}

// The scrolling column of a settings page: title, then its groups.
// Children go in `settings_column`s, so the content centres beside the nav.
pub fn settings_page(id: impl Into<ElementId>, scroll: &ScrollHandle) -> Stateful<Div> {
    div().id(id.into()).size_full().overflow_y_scroll().track_scroll(scroll).pt(px(32.)).px(px(40.)).pb(px(96.)).flex().flex_col().items_center().gap(px(32.))
}

pub const SETTINGS_WIDTH: f32 = 680.;

// The centred content column a settings page's title and every group sit in.
pub fn settings_column(child: impl IntoElement) -> Div {
    div().w_full().max_w(px(SETTINGS_WIDTH)).flex_none().child(child)
}
