use gpui_kit::{
    App, Hsla, Window,
    component::{ActiveTheme, Theme, ThemeMode},
    rgb,
};

// Discord's blurple. Accent only for the primary action, focus, links and selection marks.
const BLURPLE: u32 = 0x5865F2;
const BLURPLE_HOVER: u32 = 0x4752C4;
const LIVE: u32 = 0xED4245;
const POSITIVE: u32 = 0x41A063;
const DANGER: u32 = 0xDA3E44;
const DANGER_HOVER: u32 = 0xA2212B;
const WARNING: u32 = 0xCE8100;
const CALL_TILE: u32 = 0x1E1F22;
// The translucent grey hairlines and washes are built from.
const NEUTRAL: u32 = 0x97979F;
const WASH: f32 = 0.12;
const WASH_HOVER: f32 = 0.2;

// One theme's colours by role. Accent, LIVE, positive and danger fills are shared by both themes.
struct Tokens {
    page: u32,
    frame: u32,
    sidebar: u32,
    surface: u32,
    raised: u32,
    hairline: u32,
    hairline_alpha: f32,
    edge: u32,
    text: u32,
    strong: u32,
    muted: u32,
    faint: u32,
    accent_ink: u32,
    positive_ink: u32,
    danger_ink: u32,
    warning: u32,
}

// The Midnight mock the user picked (2026-11-14). The ink tokens are text-safe shades of the fills:
// blurple, positive and danger fills sit under 4.5:1 as text on these surfaces.
const MIDNIGHT: Tokens = Tokens {
    page: 0x0B0B0E,
    frame: 0x000000,
    sidebar: 0x0A0A0C,
    surface: 0x121214,
    raised: 0x1A1A1E,
    hairline: NEUTRAL,
    hairline_alpha: 0.16,
    edge: 0x2A2A2E,
    text: 0xDCDCDF,
    strong: 0xFBFBFB,
    muted: 0x8F9099,
    faint: 0x6B6C74,
    accent_ink: 0x949CF7,
    positive_ink: 0x62B77C,
    danger_ink: 0xEB5F5E,
    warning: WARNING,
};

// Discord's light theme from memory: Not verified. Inks darkened to clear 4.5:1 on white and #EBEDEF.
const LIGHT: Tokens = Tokens {
    page: 0xFFFFFF,
    frame: 0xFFFFFF,
    sidebar: 0xFFFFFF,
    surface: 0xEBEDEF,
    raised: 0xEBEDEF,
    hairline: 0xE1E2E4,
    hairline_alpha: 1.,
    edge: 0xD7D9DC,
    text: 0x313338,
    strong: 0x060607,
    muted: 0x5C5E66,
    faint: 0x80848E,
    accent_ink: BLURPLE_HOVER,
    positive_ink: 0x1F7A45,
    danger_ink: 0xC4262E,
    warning: 0x945300,
};

fn tokens(cx: &App) -> &'static Tokens {
    if cx.theme().is_dark() { &MIDNIGHT } else { &LIGHT }
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
    let t = tokens(cx);
    // `update`, not `global_mut`: gpui-component's own widgets paint from `theme.tokens`, which only
    // `update` reconciles with the colours set here.
    Theme::update(cx, |theme| {
        // Figtree (OFL, bundled in assets/fonts), the user's pick for Rustcord's interface type.
        theme.font_family = "Figtree".into();
        let text = hex(t.text);
        let hairline = hex(t.hairline).opacity(t.hairline_alpha);
        theme.background = hex(t.page);
        theme.title_bar = hex(t.frame);
        theme.sidebar = hex(t.sidebar);
        theme.popover = hex(t.surface);
        theme.muted = hex(t.raised);
        theme.border = hairline;
        theme.sidebar_border = hairline;
        theme.input = hex(t.edge);
        theme.foreground = text;
        theme.sidebar_foreground = text;
        theme.popover_foreground = text;
        theme.caret = text;
        theme.muted_foreground = hex(t.muted);
        theme.sidebar_accent = wash(WASH_HOVER);
        theme.sidebar_accent_foreground = hex(t.strong);
        theme.list_hover = wash(WASH);
        // White on #5865F2 is 4.6:1, so the label passes AA in both themes.
        theme.primary = blurple();
        theme.primary_hover = blurple_hover();
        theme.primary_active = hex(0x3C45A5);
        theme.primary_foreground = gpui_kit::white();
        theme.ring = blurple().opacity(0.6);
        theme.selection = blurple().opacity(0.3);
        theme.link = hex(if theme.is_dark() { 0x2781E7 } else { 0x006CE7 });
        theme.link_hover = theme.link;
        theme.danger = danger();
        theme.danger_hover = danger_hover();
        theme.danger_active = danger_hover();
        theme.danger_foreground = gpui_kit::white();
        theme.success = hex(POSITIVE);
        theme.warning = hex(t.warning);
        theme.scrollbar_thumb = hex(0x595A63);
    });
}

pub fn blurple() -> Hsla {
    hex(BLURPLE)
}

pub fn blurple_hover() -> Hsla {
    hex(BLURPLE_HOVER)
}

pub fn danger() -> Hsla {
    hex(DANGER)
}

pub fn danger_hover() -> Hsla {
    hex(DANGER_HOVER)
}

// The LIVE badge. White on it is 3.8:1, under AA for its small label: the user's pick, kept as given.
pub fn live() -> Hsla {
    hex(LIVE)
}

#[expect(dead_code, reason = "call.rs adopts it when the call view takes the palette")]
pub fn call_tile() -> Hsla {
    hex(CALL_TILE)
}

// The neutral grey at `alpha`, for edges and washes without a theme role of their own.
pub fn wash(alpha: f32) -> Hsla {
    hex(NEUTRAL).opacity(alpha)
}

// Names and headings.
pub fn strong(cx: &App) -> Hsla {
    hex(tokens(cx).strong)
}

// De-emphasised labels (muted channels). Under 4.5:1, so never the only copy of something to read.
pub fn faint(cx: &App) -> Hsla {
    hex(tokens(cx).faint)
}

pub fn accent_ink(cx: &App) -> Hsla {
    hex(tokens(cx).accent_ink)
}

pub fn positive_ink(cx: &App) -> Hsla {
    hex(tokens(cx).positive_ink)
}

pub fn danger_ink(cx: &App) -> Hsla {
    hex(tokens(cx).danger_ink)
}

// Presence dots; same in both themes.
pub fn status(status: crate::discord::Status) -> Hsla {
    use crate::discord::Status;
    hex(match status {
        Status::Online => POSITIVE,
        Status::Idle => WARNING,
        Status::Dnd => DANGER,
        Status::Offline => 0x81828A,
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
        for t in [&MIDNIGHT, &LIGHT] {
            let selected_row = over(NEUTRAL, WASH_HOVER, t.sidebar);
            let surfaces = [t.page, t.frame, t.sidebar, t.surface, t.raised, selected_row];
            for ink in [t.text, t.strong, t.muted] {
                for surface in surfaces {
                    let ratio = contrast(ink, surface);
                    assert!(ratio >= 4.5, "{ink:06X} on {surface:06X} is {ratio:.2}:1");
                }
            }
            for ink in [t.accent_ink, t.positive_ink, t.danger_ink] {
                for surface in [t.page, t.sidebar, t.surface] {
                    let ratio = contrast(ink, surface);
                    assert!(ratio >= 4.5, "{ink:06X} on {surface:06X} is {ratio:.2}:1");
                }
            }
        }
        assert!(contrast(0xFFFFFF, BLURPLE) >= 4.5);
        assert!(contrast(MIDNIGHT.text, CALL_TILE) >= 4.5);
    }
}
