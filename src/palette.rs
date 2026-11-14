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
const DANGER: u32 = 0xCC2A1B;
const DANGER_HOVER: u32 = 0xB71C0E;
const SUCCESS: u32 = 0x00753E;
const DND: u32 = 0xF05136;
const OFFLINE: u32 = 0x83868C;

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
    // The accent tint, a touch deeper on light so it still reads on white.
    accent_soft: (u32, f32),
    shade: u32,
    shimmer: (u32, f32),
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
    accent_soft: (0xF33D7D, 0.14),
    shade: 0x000000,
    shimmer: (0xFFFFFF, 0.06),
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
    accent_soft: (0xE1276E, 0.1),
    shade: 0x3B2922,
    shimmer: (0xFFFFFF, 0.6),
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

// The accent as a tint: mention rows, your own reactions, selected chips, the focus ring.
pub fn accent_soft(cx: &App) -> Hsla {
    let t = tokens(cx);
    hex(t.accent_soft.0).opacity(t.accent_soft.1)
}

// The tint behind a callout of each tone (design/css/primitives.css, `.callout--*`). Same in both themes.
pub fn info_tint() -> Hsla {
    hex(0x3D98D1).opacity(0.12)
}

pub fn warning_tint() -> Hsla {
    hex(0xF2B036).opacity(0.13)
}

pub fn danger_tint() -> Hsla {
    hex(0xDE3E2D).opacity(0.12)
}

pub fn success_tint() -> Hsla {
    hex(0x3BB974).opacity(0.12)
}

// What drop shadows are made of, at full strength: black on dark, a warm near-black on light.
pub fn shade(cx: &App) -> Hsla {
    hex(tokens(cx).shade)
}

// What dims the app behind a dialog (tokens.css --scrim): a deep plum veil on dark, a lighter one on light.
#[expect(dead_code, reason = "promoted from menu.rs, which switches over after the merge")]
pub fn scrim(cx: &App) -> Hsla {
    if cx.theme().is_dark() {
        return oklch(0.08, 0.01, 350.).opacity(0.7);
    }
    oklch(0.2, 0.02, 350.).opacity(0.4)
}

// The skeleton's sweep.
pub fn shimmer(cx: &App) -> Hsla {
    let (c, a) = tokens(cx).shimmer;
    hex(c).opacity(a)
}

// An OKLCH colour (hue in degrees), for colours the design computes: avatar gradients, role dots, swatches.
pub fn oklch(l: f32, c: f32, hue: f32) -> Hsla {
    hex(from_oklch(l, c, hue.to_radians()))
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

// A role's colour as a name colour: same hue, lightness clamped per theme (0.72–0.92 on dark,
// 0.30–0.45 on light, as design/css/navigation.css's .role-ink) and chroma capped at 0.16, so even a
// pure yellow or blue role reads at 4.5:1.
pub fn role_ink(color: u32, dark: bool) -> Hsla {
    let (l, c, h) = to_oklch(color);
    let l = if dark { l.clamp(0.72, 0.92) } else { l.clamp(0.30, 0.45) };
    hex(from_oklch(l, c.min(0.16), h))
}

fn to_oklch(color: u32) -> (f32, f32, f32) {
    let lin = |shift: u32| {
        let c = ((color >> shift) & 0xFF) as f32 / 255.;
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    let (r, g, b) = (lin(16), lin(8), lin(0));
    let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
    let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
    let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
    let (lab_l, a, bb) = (0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s, 1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s, 0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s);
    (lab_l, a.hypot(bb), bb.atan2(a))
}

fn from_oklch(l: f32, c: f32, h: f32) -> u32 {
    let (a, b) = (c * h.cos(), c * h.sin());
    let l_ = (l + 0.3963377774 * a + 0.2158037573 * b).powi(3);
    let m_ = (l - 0.1055613458 * a - 0.0638541728 * b).powi(3);
    let s_ = (l - 0.0894841775 * a - 1.2914855480 * b).powi(3);
    let channel = |v: f32| {
        let v = v.clamp(0., 1.);
        let srgb = if v <= 0.0031308 { 12.92 * v } else { 1.055 * v.powf(1. / 2.4) - 0.055 };
        (srgb * 255.).round() as u32
    };
    let r = channel(4.0767416621 * l_ - 3.3077115913 * m_ + 0.2309699292 * s_);
    let g = channel(-1.2684380046 * l_ + 2.6097574011 * m_ - 0.3413193965 * s_);
    let b = channel(-0.0041960863 * l_ - 0.7034186147 * m_ + 1.7076147010 * s_);
    (r << 16) | (g << 8) | b
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
    }

    #[test]
    fn role_colours_stay_readable_in_both_themes() {
        for role in [0xFFFF00, 0x0000FF, 0xE67E22, 0x1ABC9C, 0xFFFFFF, 0x000001] {
            let ink = |dark| {
                let (l, c, h) = to_oklch(role);
                let l = if dark { l.clamp(0.72, 0.92) } else { l.clamp(0.30, 0.45) };
                from_oklch(l, c.min(0.16), h)
            };
            for surface in [DARK.page, DARK.sidebar] {
                assert!(contrast(ink(true), surface) >= 4.5, "role {role:06X} on dark {surface:06X}");
            }
            for surface in [LIGHT.page, LIGHT.sidebar] {
                assert!(contrast(ink(false), surface) >= 4.5, "role {role:06X} on light {surface:06X}");
            }
        }
    }
}
