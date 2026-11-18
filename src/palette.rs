use gpui_kit::{
    App, Hsla, Window,
    component::{ActiveTheme, Theme, ThemeMode},
    rgb,
};

// Dango's colours, ported from the design system (design/tokens.css, Graphite neutrals). The OKLCH
// tokens there are converted to sRGB here; keep the two in step.
//
// Chili is the one accent: primary actions, current selection and focus. The fill is a shade deeper
// than the logo's #E25B26 so white on it clears 4.5:1. What asks for you (counts, LIVE, Do Not
// Disturb) is a true red, oklch(0.58 0.21 25), since chili beside it read as orange. Danger is crimson,
// a clearly different hue, so Delete never looks like the primary action.
const ACCENT: u32 = 0xC8471A;
// The mark's own chili, brighter than the fill: only for the logo and mascot, never behind text.
const LOGO: u32 = 0xE25B26;
const ACCENT_HOVER: u32 = 0xB23C14;
const ACCENT_PRESS: u32 = 0x9A330F;
const DANGER: u32 = 0xC21F3A;
const DANGER_HOVER: u32 = 0xA8172F;
const SUCCESS: u32 = 0x00753E;
const ALERT: u32 = 0xDB2B33;
const DND: u32 = ALERT;
const OFFLINE: u32 = 0x83868C;
// Graphite's cast, for the few neutrals computed from OKLCH rather than listed below.
const NEUTRAL_HUE: f32 = 260.;
const NEUTRAL_CHROMA: f32 = 0.004;

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
    accent_ink: 0xFF8A57,
    danger_ink: 0xFF7A8C,
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
    accent_soft: (0xF2652E, 0.14),
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
    accent_ink: 0xA83A12,
    danger_ink: 0xB0182F,
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
    accent_soft: (0xD9531F, 0.1),
    shade: 0x2B2E32,
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

// Paints Dango's tokens onto the current theme mode (call after `Theme::change`).
pub fn apply(cx: &mut App) {
    let t = tokens(cx);
    // `update`, not `global_mut`: gpui-component's own widgets paint from `theme.tokens`, which only
    // `update` reconciles with the colours set here.
    Theme::update(cx, |theme| {
        // The picked interface font (Figtree, bundled, by default) and Roboto Mono, both OFL in assets/fonts.
        theme.font_family = crate::settings::Prefs::load().ui_font.into();
        theme.mono_font_family = "Roboto Mono".into();
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

pub fn logo() -> Hsla {
    hex(LOGO)
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

// What drop shadows are made of, at full strength: black on dark, Graphite's near-black on light.
pub fn shade(cx: &App) -> Hsla {
    hex(tokens(cx).shade)
}

// What dims the app behind a dialog (tokens.css --scrim): a deep Graphite veil on dark, a lighter one on light.
pub fn scrim(cx: &App) -> Hsla {
    if cx.theme().is_dark() {
        return oklch(0.08, NEUTRAL_CHROMA, NEUTRAL_HUE).opacity(0.7);
    }
    oklch(0.2, NEUTRAL_CHROMA, NEUTRAL_HUE).opacity(0.4)
}

// The skeleton's sweep.
pub fn shimmer(cx: &App) -> Hsla {
    let (c, a) = tokens(cx).shimmer;
    hex(c).opacity(a)
}

// The call stage is dark in both themes (design/css/voice.css `.call`), so whatever sits on it reads
// Graphite's dark tokens instead of the current theme's.
pub mod stage {
    use super::{DARK, DND, Hsla, LIGHT, hex};

    pub fn bg() -> Hsla {
        hex(DARK.rail)
    }

    pub fn overlay() -> Hsla {
        hex(DARK.overlay)
    }

    pub fn ended() -> Hsla {
        hex(DARK.sidebar)
    }

    pub fn strong() -> Hsla {
        hex(DARK.strong)
    }

    pub fn text() -> Hsla {
        hex(DARK.text)
    }

    pub fn muted() -> Hsla {
        hex(DARK.muted)
    }

    pub fn faint() -> Hsla {
        hex(DARK.faint)
    }

    pub fn online() -> Hsla {
        hex(DARK.online)
    }

    pub fn idle() -> Hsla {
        hex(DARK.idle)
    }

    pub fn dnd() -> Hsla {
        hex(DND)
    }

    // Translucent white: hover .05, press .09, selected .08, line .08, strong line .14.
    pub fn wash(alpha: f32) -> Hsla {
        hex(DARK.ink_wash).opacity(alpha)
    }

    // Off states (muted, camera off) are soft coral, apart from the red that leaves.
    pub fn off(hover: bool) -> Hsla {
        hex(DND).opacity(if hover { 0.3 } else { 0.2 })
    }

    pub fn off_ink() -> Hsla {
        hex(0xFF9079)
    }

    // On states (camera on) are a light fill with dark ink.
    pub fn on(hover: bool) -> Hsla {
        hex(if hover { DARK.text } else { DARK.strong })
    }

    pub fn on_ink() -> Hsla {
        hex(LIGHT.strong)
    }
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
    alert()
}

// Counts, unread marks and LIVE: what asks for you.
pub fn alert() -> Hsla {
    hex(ALERT)
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

// A role's colour as a name colour: the role's own hex, as Discord shows it. Only a role too dark for the
// dark theme (or too light for the light one) moves, in lightness alone, to the floor that still reads.
// Deviates from design/css/navigation.css's .role-ink, whose clamp turned every role pastel.
pub fn role_ink(color: u32, dark: bool) -> Hsla {
    let (l, c, h) = to_oklch(color);
    let readable = if dark { l.max(0.62) } else { l.min(0.55) };
    if readable == l {
        return hex(color);
    }
    hex(from_oklch(readable, c, h))
}

fn to_oklch(color: u32) -> (f32, f32, f32) {
    let lin = |shift: u32| {
        let c = ((color >> shift) & 0xFF) as f32 / 255.;
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    let (r, g, b) = (lin(16), lin(8), lin(0));
    let l = (0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    let (lab_l, a, bb) = (0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s, 1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s, 0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s);
    (lab_l, a.hypot(bb), bb.atan2(a))
}

fn from_oklch(l: f32, c: f32, h: f32) -> u32 {
    let (a, b) = (c * h.cos(), c * h.sin());
    let l_ = (l + 0.396_337_78 * a + 0.215_803_76 * b).powi(3);
    let m_ = (l - 0.105_561_346 * a - 0.063_854_17 * b).powi(3);
    let s_ = (l - 0.089_484_18 * a - 1.291_485_5 * b).powi(3);
    let channel = |v: f32| {
        let v = v.clamp(0., 1.);
        let srgb = if v <= 0.0031308 { 12.92 * v } else { 1.055 * v.powf(1. / 2.4) - 0.055 };
        (srgb * 255.).round() as u32
    };
    let r = channel(4.076_741_7 * l_ - 3.307_711_6 * m_ + 0.230_969_94 * s_);
    let g = channel(-1.268_438 * l_ + 2.609_757_4 * m_ - 0.341_319_38 * s_);
    let b = channel(-0.0041960863 * l_ - 0.703_418_6 * m_ + 1.707_614_7 * s_);
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
        for fill in [ACCENT, ALERT, DANGER, SUCCESS] {
            assert!(contrast(0xFFFFFF, fill) >= 4.5, "white on {fill:06X}");
        }
    }

    // Graphite is the only neutral: a near-grey with a warm cast (the old ink-and-bone or plum ramps)
    // anywhere in src/ fails here. The dango's stick colours are the mark's own material, so they stay.
    #[test]
    fn no_warm_neutrals_in_src() {
        const MARK_STICKS: [u32; 3] = [0x8C7A6A, 0xC9A27A, 0xC79A6B];
        let mut warm = Vec::new();
        for entry in std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/src")).unwrap() {
            let path = entry.unwrap().path();
            let Ok(source) = std::fs::read_to_string(&path) else { continue };
            for (i, line) in source.lines().enumerate() {
                for hex in line.split("0x").skip(1).filter_map(|rest| rest.get(..6)) {
                    let Ok(value) = u32::from_str_radix(hex, 16) else { continue };
                    let (_, chroma, hue) = to_oklch(value);
                    let hue = hue.to_degrees().rem_euclid(360.);
                    let is_warm = (0.005..=0.045).contains(&chroma) && !(110. ..290.).contains(&hue);
                    if is_warm && !MARK_STICKS.contains(&value) {
                        warm.push(format!("{}:{}: 0x{hex}", path.display(), i + 1));
                    }
                }
            }
        }
        assert!(warm.is_empty(), "warm neutrals left in src/:\n{}", warm.join("\n"));
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
