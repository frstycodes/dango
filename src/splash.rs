// Dango's launch window (design/names.html): a small bone or midnight card with the dango loader, the
// name, one line naming the real loading stage and a bar for how far along it is, shown while the hidden
// main window signs in and loads. `Rustcord` advances it and closes it, then brings the main window forward.
use gpui_kit::{
    App, AppContext, Bounds, Context, FontWeight, Hsla, IntoElement, ParentElement, Render, SharedString, Styled, Window, WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind, WindowOptions,
    component::ActiveTheme, div, px, rgb, size,
};

use crate::{
    mascot::{self, LoaderColors},
    palette,
    ui::{self, Squircle, shadow},
};

// Signing in, loading your servers, opening Dango.
pub const STAGES: usize = 3;

const MARK: f32 = 56.;
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
        let card = if cx.theme().is_dark() { &MIDNIGHT } else { &BONE };
        let loader = LoaderColors { ink: rgb(card.mark).into(), ring: rgb(card.bg).into(), stick: rgb(card.stick).into() };
        // Opened without gpui-kit's root view, so the app font is set here.
        // GAP: the design's 0 18 40 drop shadow falls outside this borderless window's bounds.
        div().size_full().font_family(cx.theme().font_family.clone()).child(
            div()
                .size_full()
                .px_4()
                .squircle(ui::RADIUS_MD)
                .bg(rgb(card.bg))
                // One inset hairline: a border on a squircle in a transparent window doubled up at the corners.
                .shadow(vec![shadow(card.edge, 0., 0., 0., 1., true)])
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(10.))
                .child(mascot::loader("splash-loader", MARK, loader, cx))
                .child(div().mt(px(4.)).text_size(px(20.)).line_height(px(20.)).font_weight(FontWeight::BOLD).text_color(rgb(card.ink)).child("Dango"))
                .child(div().max_w_full().truncate().text_size(px(12.)).font_weight(FontWeight::MEDIUM).text_color(rgb(card.muted)).child(self.stage.clone()))
                .child(progress_bar(self.progress, card)),
        )
    }
}

fn progress_bar(progress: f32, card: &Card) -> gpui_kit::Div {
    div().w(px(TRACK)).h(px(6.)).rounded(px(3.)).bg(card.track).child(div().h_full().w(px(TRACK * progress)).rounded(px(3.)).bg(palette::accent()))
}

// The splash opens before the main window, so it carries its own two materials, like the call stage.
struct Card {
    bg: u32,
    ink: u32,
    muted: u32,
    mark: u32,
    stick: u32,
    track: Hsla,
    edge: Hsla,
}

// Midnight, oklch(0.13 0.008 350), with a 16% white hairline; the mark in light chili.
const MIDNIGHT: Card = Card {
    bg: 0x0A0608,
    ink: 0xF7F4F5,
    muted: 0xB1A8AC,
    mark: 0xFF9A82,
    stick: 0x8C7A6A,
    track: Hsla { h: 0., s: 0., l: 1., a: 0.16 },
    edge: Hsla { h: 0., s: 0., l: 1., a: 0.16 },
};
// Warm bone, oklch(0.962 0.012 80), with a 16% warm-ink hairline (#49352F); the mark in chili.
const BONE: Card = Card {
    bg: 0xF7F2EA,
    ink: 0x1B1317,
    muted: 0x5F5559,
    mark: 0xE25B26,
    stick: 0xC9A27A,
    track: Hsla { h: 13. / 360., s: 0.22, l: 0.236, a: 0.18 },
    edge: Hsla { h: 13. / 360., s: 0.22, l: 0.236, a: 0.16 },
};
