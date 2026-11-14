use gpui_kit::{
    App, Hsla, Window,
    component::{ActiveTheme, Theme, ThemeMode},
    rgb,
};

// Wave's colours, ported from the design system (design/tokens.css, Graphite neutrals). The OKLCH
// tokens there are converted to sRGB here; keep the two in step.
//
// Magenta is the one accent: primary actions, current selection, focus, and what asks for you
// (mentions, unread counts, live). The fill is a shade deeper than the logo's #E8326F so white on it
// clears 4.5:1.
const ACCENT: u32 = 0xDB2B65;
const ACCENT_HOVER: u32 = 0xC2215A;
const ACCENT_PRESS: u32 = 0xA81C4E;
const ACCENT_SOFT: u32 = 0xF33D7D;
const DANGER: u32 = 0xCC2A1B;
const DANGER_HOVER: u32 = 0xB71C0E;
const SUCCESS: u32 = 0x00753E;
const DND: u32 = 0xF05136;
const OFFLINE: u32 = 0x83868C;
// The call stage stays dark in both themes, so streams read like a screen.
const CALL_TILE: u32 = 0x05070B;
// The translucent grey older surfaces build hairlines and washes from.
const NEUTRAL: u32 = 0x97979F;

// One theme's colours by role.
struct Tokens {
    // Surfaces, deepest to highest: server rail, sidebar, page, raised (inputs, cards), overlay (menus).
    rail: u32,
    sidebar: u32,
    page: u32,
    raised: u32,
    overlay: u32,
    strong: u32,
    text: u32,
    muted: u32,
    faint: u32,
    accent_ink: u32,
    danger_ink: u32,
    online: u32,
    online_ink: u32,
    idle: u32,
    info_ink: u32,
    warning_ink: u32,
    // The unfilled part of sliders, progress and switches; opaque so overlapping pieces never double up.
    track: u32,
    // Translucent fills over any surface: white in the dark theme, ink in the light one.
    ink_wash: u32,
    hover: f32,
    press: f32,
    selected: f32,
    line: f32,
    line_strong: f32,
    accent_soft: f32,
    seg_track: (u32, f32),
    seg_thumb: (u32, f32),
}

const DARK: Tokens = Tokens {
    rail: 0x0A0B0D,
    sidebar: 0x111213,
    page: 0x17181A,
    raised: 0x202224,
    overlay: 0x26282A,
    strong: 0xF3F5F8,
    text: 0xD4D8DD,
    muted: 0xA0A5AC,
    faint: 0x8B8F97,
    accent_ink: 0xFF6B9C,
    danger_ink: 0xFF8068,
    online: 0x3EC87D,
    online_ink: 0x59D38C,
    idle: 0xF2B036,
    info_ink: 0x65B4E9,
    warning_ink: 0xF3B94C,
    track: 0x33353A,
    ink_wash: 0xFFFFFF,
    hover: 0.05,
    press: 0.09,
    selected: 0.08,
    line: 0.08,
    line_strong: 0.14,
    accent_soft: 0.14,
    seg_track: (0x000000, 0.3),
    seg_thumb: (0xFFFFFF, 0.15),
};

const LIGHT: Tokens = Tokens {
    rail: 0xDEE0E3,
    sidebar: 0xECEEF0,
    page: 0xF8F9FB,
    raised: 0xFDFDFE,
    overlay: 0xFEFEFF,
    strong: 0x14161A,
    text: 0x2B2E32,
    muted: 0x55585E,
    faint: 0x5D6166,
    accent_ink: 0xB81F55,
    danger_ink: 0xB71C0E,
    online: 0x00A159,
    online_ink: 0x00723B,
    idle: 0xDA950B,
    info_ink: 0x0068A7,
    warning_ink: 0x975800,
    track: 0xD1D4DA,
    ink_wash: 0x2B2E32,
    hover: 0.06,
    press: 0.1,
    selected: 0.09,
    line: 0.1,
    line_strong: 0.18,
    accent_soft: 0.1,
    seg_track: (0x2B2E32, 0.1),
    seg_thumb: (0xFFFFFF, 1.),
};

fn tokens(cx: &App) -> &'static Tokens {
    if cx.theme().is_dark() { &DARK } else { &LIGHT }
}

fn hex(value: u32) -> Hsla {
    rgb(value).into()
}

pub fn sync(window: &mut Window, cx: &mut App) {
    Theme::sync_system_appearance(Some(window), cx);
    match crate::settings::Prefs::load().theme {
        crate::settings::ThemePref::Dark => Theme::change(ThemeMode::Dark, Some(window), cx),
        crate::settings::ThemePref::Light => Theme::change(ThemeMode::Light, Some(window), cx),
        crate::settings::ThemePref::System => {}
    }
    #[cfg(debug_assertions)]
    if let Ok(mode) = std::env::var("CHORD_THEME") {
        let mode = if mode == "dark" { ThemeMode::Dark } else { ThemeMode::Light };
        Theme::change(mode, Some(window), cx);
    }
    apply(cx);
}

// Paints Wave's tokens onto the current theme mode (call after `Theme::change`).
pub fn apply(cx: &mut App) {
    let t = tokens(cx);
    // `update`, not `global_mut`: gpui-component's own widgets paint from `theme.tokens`, which only
    // `update` reconciles with the colours set here.
    Theme::update(cx, |theme| {
        // Gabarito (OFL, bundled in assets/fonts). Code stays in gg sans mono until a mono is bundled.
        theme.font_family = "Gabarito".into();
        theme.mono_font_family = "gg sans mono".into();
        let text = hex(t.text);
        let line = hex(t.ink_wash).opacity(t.line);
        theme.background = hex(t.page);
        theme.title_bar = hex(t.rail);
        theme.sidebar = hex(t.sidebar);
        theme.popover = hex(t.overlay);
        theme.muted = hex(t.raised);
        theme.border = line;
        theme.sidebar_border = line;
        theme.input = hex(t.ink_wash).opacity(t.line_strong);
        theme.foreground = text;
        theme.sidebar_foreground = text;
        theme.popover_foreground = text;
        theme.caret = text;
        theme.muted_foreground = hex(t.muted);
        theme.sidebar_accent = hex(t.ink_wash).opacity(t.selected);
        theme.sidebar_accent_foreground = hex(t.strong);
        theme.list_hover = hex(t.ink_wash).opacity(t.hover);
        theme.primary = accent();
        theme.primary_hover = accent_hover();
        theme.primary_active = hex(ACCENT_PRESS);
        theme.primary_foreground = gpui_kit::white();
        theme.ring = hex(t.accent_ink);
        theme.selection = accent().opacity(0.3);
        theme.link = hex(t.accent_ink);
        theme.link_hover = theme.link;
        theme.danger = danger();
        theme.danger_hover = danger_hover();
        theme.danger_active = danger_hover();
        theme.danger_foreground = gpui_kit::white();
        theme.success = hex(SUCCESS);
        theme.warning = hex(t.idle);
        theme.scrollbar_thumb = hex(t.ink_wash).opacity(t.line_strong);
    });
}

pub fn accent() -> Hsla {
    hex(ACCENT)
}

pub fn accent_hover() -> Hsla {
    hex(ACCENT_HOVER)
}

pub fn accent_press() -> Hsla {
    hex(ACCENT_PRESS)
}

// The accent as a tint: mention rows, your own reactions, selected chips.
pub fn accent_soft(cx: &App) -> Hsla {
    hex(ACCENT_SOFT).opacity(tokens(cx).accent_soft)
}

pub fn danger() -> Hsla {
    hex(DANGER)
}

pub fn danger_hover() -> Hsla {
    hex(DANGER_HOVER)
}

// LIVE asks for you, so it takes the accent.
pub fn live() -> Hsla {
    accent()
}

pub fn call_tile() -> Hsla {
    hex(CALL_TILE)
}

// The neutral grey at `alpha`, for older edges and washes without a theme role of their own.
pub fn wash(alpha: f32) -> Hsla {
    hex(NEUTRAL).opacity(alpha)
}

// The surface ramp, deepest first.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level {
    Rail,
    Sidebar,
    Page,
    Raised,
    Overlay,
}

pub fn surface(level: Level, cx: &App) -> Hsla {
    let t = tokens(cx);
    hex(match level {
        Level::Rail => t.rail,
        Level::Sidebar => t.sidebar,
        Level::Page => t.page,
        Level::Raised => t.raised,
        Level::Overlay => t.overlay,
    })
}

// Translucent fills that work over any surface.
pub fn hover(cx: &App) -> Hsla {
    let t = tokens(cx);
    hex(t.ink_wash).opacity(t.hover)
}

pub fn press(cx: &App) -> Hsla {
    let t = tokens(cx);
    hex(t.ink_wash).opacity(t.press)
}

pub fn selected(cx: &App) -> Hsla {
    let t = tokens(cx);
    hex(t.ink_wash).opacity(t.selected)
}

pub fn line(cx: &App) -> Hsla {
    let t = tokens(cx);
    hex(t.ink_wash).opacity(t.line)
}

pub fn line_strong(cx: &App) -> Hsla {
    let t = tokens(cx);
    hex(t.ink_wash).opacity(t.line_strong)
}

pub fn track(cx: &App) -> Hsla {
    hex(tokens(cx).track)
}

pub fn seg_track(cx: &App) -> Hsla {
    let (c, a) = tokens(cx).seg_track;
    hex(c).opacity(a)
}

pub fn seg_thumb(cx: &App) -> Hsla {
    let (c, a) = tokens(cx).seg_thumb;
    hex(c).opacity(a)
}

// Text, strongest to faintest. Faint still clears 4.5:1 on every surface.
pub fn strong(cx: &App) -> Hsla {
    hex(tokens(cx).strong)
}

pub fn text(cx: &App) -> Hsla {
    hex(tokens(cx).text)
}

pub fn muted(cx: &App) -> Hsla {
    hex(tokens(cx).muted)
}

pub fn faint(cx: &App) -> Hsla {
    hex(tokens(cx).faint)
}

pub fn accent_ink(cx: &App) -> Hsla {
    hex(tokens(cx).accent_ink)
}

pub fn positive_ink(cx: &App) -> Hsla {
    hex(tokens(cx).online_ink)
}

pub fn danger_ink(cx: &App) -> Hsla {
    hex(tokens(cx).danger_ink)
}

pub fn info_ink(cx: &App) -> Hsla {
    hex(tokens(cx).info_ink)
}

pub fn warning_ink(cx: &App) -> Hsla {
    hex(tokens(cx).warning_ink)
}

pub fn success() -> Hsla {
    hex(SUCCESS)
}

// Presence colours. Status is never colour alone: `ui::status` gives each its shape.
pub fn status_in(status: crate::discord::Status, cx: &App) -> Hsla {
    use crate::discord::Status;
    let t = tokens(cx);
    hex(match status {
        Status::Online => t.online,
        Status::Idle => t.idle,
        Status::Dnd => DND,
        Status::Offline => OFFLINE,
    })
}

// Presence colours for callers without a theme at hand (the dark values).
pub fn status(status: crate::discord::Status) -> Hsla {
    use crate::discord::Status;
    hex(match status {
        Status::Online => DARK.online,
        Status::Idle => DARK.idle,
        Status::Dnd => DND,
        Status::Offline => OFFLINE,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luminance(value: u32) -> f32 {
        let channel = |shift: u32| {
            let c = ((value >> shift) & 0xFF) as f32 / 255.;
            if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
    }

    fn contrast(a: u32, b: u32) -> f32 {
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    // `top` at `alpha` composited over `bottom`.
    fn over(top: u32, alpha: f32, bottom: u32) -> u32 {
        [16, 8, 0].iter().fold(0, |acc, shift| {
            let mix = ((top >> shift) & 0xFF) as f32 * alpha + ((bottom >> shift) & 0xFF) as f32 * (1. - alpha);
            acc | ((mix.round() as u32) << shift)
        })
    }

    #[test]
    fn text_meets_aa_on_every_surface() {
        for t in [&DARK, &LIGHT] {
            let selected_row = over(t.ink_wash, t.selected, t.sidebar);
            let surfaces = [t.rail, t.sidebar, t.page, t.raised, t.overlay, selected_row];
            for ink in [t.text, t.strong, t.muted, t.faint] {
                for surface in surfaces {
                    let ratio = contrast(ink, surface);
                    assert!(ratio >= 4.5, "{ink:06X} on {surface:06X} is {ratio:.2}:1");
                }
            }
            for ink in [t.accent_ink, t.online_ink, t.danger_ink, t.info_ink, t.warning_ink] {
                for surface in [t.sidebar, t.page, t.raised] {
                    let ratio = contrast(ink, surface);
                    assert!(ratio >= 4.5, "{ink:06X} on {surface:06X} is {ratio:.2}:1");
                }
            }
        }
        for fill in [ACCENT, DANGER, SUCCESS] {
            assert!(contrast(0xFFFFFF, fill) >= 4.5, "white on {fill:06X}");
        }
        assert!(contrast(DARK.text, CALL_TILE) >= 4.5);
    }
}
