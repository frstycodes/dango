// Wave's launch window, like Discord's: a small borderless card with the mascot speaking, the
// name, and one line naming the real loading stage, shown while the hidden main window signs in
// and loads. `Rustcord` advances it and closes it, then brings the main window forward.
use gpui_kit::{
    App, AppContext, Bounds, Context, FontWeight, Hsla, IntoElement, ParentElement, Render, SharedString, Styled, Window, WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind, WindowOptions,
    component::ActiveTheme, div, px, rgb, size,
};

use crate::{
    mascot::{Mood, mascot_at},
    palette,
    ui::{self, Squircle},
};

// Signing in, loading your servers, opening Wave.
pub const STAGES: usize = 3;

pub struct Splash {
    stage: SharedString,
    step: usize,
    // Set while an update downloads: a bar replaces the step dots.
    progress: Option<f32>,
}

impl Splash {
    pub fn set(&mut self, step: usize, stage: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.step = step;
        self.stage = stage.into();
        cx.notify();
    }

    pub fn set_progress(&mut self, version: &str, progress: f32, cx: &mut Context<Self>) {
        self.stage = format!("Updating to {version}…").into();
        self.progress = Some(progress.clamp(0., 1.));
        cx.notify();
    }
}

pub fn open(stage: &'static str, cx: &mut App) -> Option<WindowHandle<Splash>> {
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
        cx.new(|_| Splash { stage: stage.into(), step: 0, progress: None })
    })
    .ok()
}

impl Render for Splash {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let card = if cx.theme().is_dark() { &MIDNIGHT } else { &BONE };
        let dots = (0..STAGES).map(|i| div().size(px(6.)).rounded_full().bg(if i <= self.step { palette::accent() } else { rgb(card.rest).into() }));
        let indicator = match self.progress {
            Some(progress) => progress_bar(progress, card),
            None => div().flex().gap(px(5.)).children(dots),
        };
        // Opened without gpui-kit's root view, so the app font is set here.
        // GAP: the design's 0 18 40 drop shadow falls outside this borderless window's bounds.
        div().size_full().font_family(cx.theme().font_family.clone()).child(
            div()
                .size_full()
                .px_4()
                .squircle(ui::RADIUS_MD)
                .bg(rgb(card.bg))
                .border_1()
                .border_color(card.edge)
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(10.))
                .child(mascot_at("splash", Mood::Happy, 0.6, cx))
                .child(div().mt(px(4.)).text_size(px(20.)).line_height(px(20.)).font_weight(FontWeight::BOLD).text_color(rgb(card.ink)).child("Wave"))
                .child(div().max_w_full().truncate().text_size(px(12.)).font_weight(FontWeight::MEDIUM).text_color(rgb(card.muted)).child(self.stage.clone()))
                .child(indicator),
        )
    }
}

// The step dots' height, so the card doesn't shift when the bar takes their place.
fn progress_bar(progress: f32, card: &Card) -> gpui_kit::Div {
    let track = px(120.);
    div().w(track).h(px(6.)).rounded_full().bg(rgb(card.rest)).child(div().h_full().w(track * progress).rounded_full().bg(palette::accent()))
}

// The splash opens before the main window, so it carries its own two materials, like the call stage.
struct Card {
    bg: u32,
    ink: u32,
    muted: u32,
    // Step dots still to come.
    rest: u32,
    edge: Hsla,
}

// Midnight, oklch(0.13 0.008 350), with a 16% white hairline.
const MIDNIGHT: Card = Card { bg: 0x0A0608, ink: 0xF7F4F5, muted: 0xB1A8AC, rest: 0x696164, edge: Hsla { h: 0., s: 0., l: 1., a: 0.16 } };
// Warm bone, oklch(0.962 0.012 80), with a 16% warm-ink hairline (#49352F).
const BONE: Card = Card { bg: 0xF7F2EA, ink: 0x1B1317, muted: 0x5F5559, rest: 0x8D8387, edge: Hsla { h: 13. / 360., s: 0.22, l: 0.236, a: 0.16 } };
