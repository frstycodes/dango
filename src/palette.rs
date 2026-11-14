use gpui_kit::{App, Hsla, Window, component::Theme, rgb};

// Discord's blurple (#5865F2). Accent only for the primary action, focus, links and selection marks.
const BLURPLE: u32 = 0x5865F2;

pub fn blurple() -> Hsla {
    rgb(BLURPLE).into()
}

// Discord's own surfaces. Dark is Midnight, read from Discord's CSS tokens in the user's session
// (docs/discord-tokens.json, 2026-11-14). Light is Discord's light theme from memory: Not verified.
struct Surfaces {
    surface: u32,
    raised: u32,
    raised_edge: u32,
    hairline: u32,
    ink: u32,
    muted: u32,
}

// --background-base-lowest, --channeltextarea-background, --border-normal over black, --text-default, --text-subtle.
const MIDNIGHT: Surfaces = Surfaces { surface: 0x000000, raised: 0x131416, raised_edge: 0x2A2A2E, hairline: 0x1F1F21, ink: 0xD4D5D8, muted: 0x96979E };
const LIGHT: Surfaces = Surfaces { surface: 0xFFFFFF, raised: 0xEBEDEF, raised_edge: 0xD7D9DC, hairline: 0xE1E2E4, ink: 0x313338, muted: 0x5C5E66 };

pub fn sync(window: &mut Window, cx: &mut App) {
    Theme::sync_system_appearance(Some(window), cx);
    match crate::settings::Prefs::load().theme {
        crate::settings::ThemePref::Dark => Theme::change(gpui_kit::component::ThemeMode::Dark, Some(window), cx),
        crate::settings::ThemePref::Light => Theme::change(gpui_kit::component::ThemeMode::Light, Some(window), cx),
        crate::settings::ThemePref::System => {}
    }
    #[cfg(debug_assertions)]
    if let Ok(mode) = std::env::var("CHORD_THEME") {
        let mode = if mode == "dark" { gpui_kit::component::ThemeMode::Dark } else { gpui_kit::component::ThemeMode::Light };
        Theme::change(mode, Some(window), cx);
    }
    let theme = Theme::global_mut(cx);
    // Figtree (OFL, bundled in assets/fonts), the user's pick for Rustcord's interface type.
    theme.font_family = "Figtree".into();
    let dark = theme.is_dark();
    let s = if dark { MIDNIGHT } else { LIGHT };
    let color = |hex: u32| -> Hsla { rgb(hex).into() };
    let ink = color(s.ink);
    let blurple = blurple();
    theme.background = color(s.surface);
    theme.sidebar = color(s.surface);
    theme.foreground = ink;
    theme.sidebar_foreground = ink;
    theme.muted = color(s.raised);
    theme.muted_foreground = color(s.muted);
    theme.border = color(s.hairline);
    theme.sidebar_border = color(s.hairline);
    theme.input = color(s.raised_edge);
    theme.popover = color(s.raised);
    theme.popover_foreground = ink;
    theme.ring = blurple.opacity(0.6);
    // Discord's row states: --interactive-background-selected / -hover, translucent #97979F.
    let wash: Hsla = rgb(0x97979F).into();
    theme.sidebar_accent = wash.opacity(if dark { 0.24 } else { 0.2 });
    theme.sidebar_accent_foreground = color(if dark { 0xDCDCDF } else { 0x060607 });
    theme.list_hover = wash.opacity(if dark { 0.08 } else { 0.1 });
    // White on #5865F2 is 4.6:1, so the label passes AA in both themes.
    theme.primary = blurple;
    theme.primary_hover = rgb(0x4752C4).into();
    theme.primary_active = rgb(0x3C45A5).into();
    theme.primary_foreground = gpui_kit::white();
    theme.link = if dark { rgb(0x2781E7).into() } else { rgb(0x006CE7).into() };
    theme.link_hover = theme.link;
    theme.caret = ink;
    // Discord's semantic colours; gpui-component's defaults read washed out beside them.
    // --status-danger, --red-500, --status-positive, --status-warning.
    theme.danger = rgb(0xDA3E44).into();
    theme.danger_hover = rgb(0xD6363F).into();
    theme.danger_active = rgb(0xA2212B).into();
    theme.danger_foreground = gpui_kit::white();
    theme.success = rgb(0x3D9E60).into();
    theme.warning = rgb(0xFDB833).into();
    theme.scrollbar_thumb = rgb(0x595A63).into();
    theme.selection = blurple.opacity(0.3);
}

// Strong text for names and headings (--text-strong).
pub fn strong(cx: &App) -> Hsla {
    use gpui_kit::component::ActiveTheme;
    if cx.theme().is_dark() { rgb(0xDCDCDF).into() } else { rgb(0x060607).into() }
}

// Discord's status colours (--status-positive, --status-warning, --status-danger); same in both themes.
pub fn status(status: crate::discord::Status) -> Hsla {
    use crate::discord::Status;
    rgb(match status {
        Status::Online => 0x3D9E60,
        Status::Idle => 0xFDB833,
        Status::Dnd => 0xDA3E44,
        Status::Offline => 0x81828A,
    })
    .into()
}
