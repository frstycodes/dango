// Wave's launch window, like Discord's: a small borderless card with the mascot speaking, the
// name, and one line naming the real loading stage, shown while the hidden main window signs in
// and loads. `Rustcord` advances it and closes it, then brings the main window forward.
use gpui_kit::{
    App, AppContext, Bounds, Context, FontWeight, IntoElement, ParentElement, Render, SharedString, Styled, Window, WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind, WindowOptions,
    component::ActiveTheme, div, px, size,
};

use crate::{mascot::{Mood, mascot_at}, palette};

// Signing in, loading your servers, opening Wave.
pub const STAGES: usize = 3;

pub struct Splash {
    stage: SharedString,
    step: usize,
}

impl Splash {
    pub fn set(&mut self, step: usize, stage: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.step = step;
        self.stage = stage.into();
        cx.notify();
    }
}

pub fn open(cx: &mut App) -> Option<WindowHandle<Splash>> {
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(220.), px(248.)), cx))),
        titlebar: None,
        kind: WindowKind::PopUp,
        focus: true,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        window_background: WindowBackgroundAppearance::Transparent,
        ..Default::default()
    };
    cx.open_window(options, |window, cx| {
        palette::sync(window, cx);
        cx.new(|_| Splash { stage: "Signing in…".into(), step: 0 })
    })
    .ok()
}

impl Render for Splash {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dark = cx.theme().is_dark();
        // Midnight black, or warm bone in the light theme.
        let (card, ink, muted) = if dark { (gpui_kit::black(), palette::strong(cx), cx.theme().muted_foreground) } else { (gpui_kit::rgb(0xF6F2EA).into(), palette::strong(cx), cx.theme().muted_foreground) };
        let dots = (0..STAGES).map(|i| div().size(px(5.)).rounded_full().bg(if i <= self.step { palette::accent() } else { muted.opacity(0.35) }));
        // Opened without gpui-kit's root view, so the app font is set here.
        div().size_full().font_family(cx.theme().font_family.clone()).child(
            div()
                .size_full()
                .rounded(px(14.))
                .bg(card)
                .border_1()
                .border_color(palette::wash(0.16))
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(10.))
                .child(mascot_at("splash", Mood::Happy, 0.6, cx))
                .child(div().mt(px(4.)).text_size(px(18.)).font_weight(FontWeight::EXTRA_BOLD).text_color(ink).child("Wave"))
                .child(div().px_4().max_w_full().truncate().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(muted).child(self.stage.clone()))
                .child(div().flex().gap(px(5.)).children(dots)),
        )
    }
}
