// Wave's launch window: a small graphite card with the wave mark and name carved into it, one line
// naming the real loading stage, and a bar for how far along it is, shown while the hidden main window
// signs in and loads. `Rustcord` advances it and closes it, then brings the main window forward.
use gpui_kit::{
    App, AppContext, Bounds, Context, FontWeight, Hsla, IntoElement, ParentElement, Render, SharedString, Styled, Window, WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind, WindowOptions,
    component::ActiveTheme, div, px, size,
};

use crate::{
    palette::{self, oklch},
    ui::{self, Squircle, shadow},
};

// Signing in, loading your servers, opening Wave.
pub const STAGES: usize = 3;

// The wave mark's five bars, tallest in the middle.
const BARS: [f32; 5] = [19., 36., 52., 31., 18.];
const TRACK: f32 = 120.;

pub struct Splash {
    stage: SharedString,
    progress: f32,
}

impl Splash {
    pub fn set(&mut self, step: usize, stage: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.stage = stage.into();
        self.progress = (step + 1) as f32 / STAGES as f32;
        cx.notify();
    }

    pub fn set_progress(&mut self, version: &str, progress: f32, cx: &mut Context<Self>) {
        self.stage = format!("Updating to {version}…").into();
        self.progress = progress.clamp(0., 1.);
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
        cx.new(|_| Splash { stage: stage.into(), progress: 1. / STAGES as f32 })
    })
    .ok()
}

impl Render for Splash {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let card = Card::new(cx.theme().is_dark());
        // Opened without gpui-kit's root view, so the app font is set here.
        // GAP: the design's 0 18 40 drop shadow falls outside this borderless window's bounds.
        div().size_full().font_family(cx.theme().font_family.clone()).child(
            div()
                .size_full()
                .px_4()
                .squircle(ui::RADIUS_MD)
                .bg(card.bg)
                // One inset hairline: a border on a squircle in a transparent window doubled up at the corners.
                .shadow(vec![shadow(card.edge, 0., 0., 0., 1., true)])
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .child(div().h(px(52.)).flex().items_center().gap(px(4.)).children(BARS.map(|h| carved_bar(h, &card))))
                .child(div().mt(px(14.)).child(carved_name(&card)))
                .child(div().mt(px(18.)).max_w_full().truncate().text_size(px(12.)).font_weight(FontWeight::MEDIUM).text_color(card.muted).child(self.stage.clone()))
                .child(progress_bar(self.progress, &card)),
        )
    }
}

// Pressed into the card: the top edge in shadow, the bottom lip catching light.
fn carved_bar(height: f32, card: &Card) -> gpui_kit::Div {
    div().w(px(8.)).h(px(height)).rounded(px(4.)).bg(card.carve).shadow(vec![shadow(card.shade, 0., 2., 2., 0., true), shadow(card.light, 0., -1., 0., 0., true)])
}

// The same carving for type: a shadow line above each letter and a light line below, under the face.
fn carved_name(card: &Card) -> gpui_kit::Div {
    let word = |color: Hsla| div().text_size(px(22.)).line_height(px(26.)).font_weight(FontWeight::EXTRA_BOLD).text_color(color).child("Wave");
    div().relative().child(word(card.shade).absolute().top(px(-1.))).child(word(card.light).absolute().top(px(1.))).child(word(card.carve).relative())
}

fn progress_bar(progress: f32, card: &Card) -> gpui_kit::Div {
    div().mt(px(10.)).w(px(TRACK)).h(px(4.)).rounded(px(2.)).bg(card.track).child(div().h_full().w(px(TRACK * progress)).rounded(px(2.)).bg(palette::accent()))
}

// The splash opens before the main window, so it carries its own two materials, like the call stage.
struct Card {
    bg: Hsla,
    carve: Hsla,
    shade: Hsla,
    light: Hsla,
    muted: Hsla,
    track: Hsla,
    edge: Hsla,
}

impl Card {
    // The design's graphite neutrals (hue 260), dark and light.
    fn new(dark: bool) -> Self {
        let white = oklch(1., 0., 0.);
        if dark {
            return Self {
                bg: oklch(0.210, 0.004, 260.),
                carve: oklch(0.180, 0.004, 260.),
                shade: oklch(0., 0., 0.).opacity(0.55),
                light: white.opacity(0.07),
                muted: oklch(0.72, 0.008, 260.),
                track: white.opacity(0.08),
                edge: white.opacity(0.10),
            };
        }
        let ink = oklch(0.30, 0.01, 260.);
        Self {
            bg: oklch(0.982, 0.0024, 260.),
            carve: oklch(0.935, 0.004, 260.),
            shade: ink.opacity(0.28),
            light: white.opacity(0.95),
            muted: oklch(0.46, 0.010, 260.),
            track: ink.opacity(0.10),
            edge: ink.opacity(0.12),
        }
    }
}
