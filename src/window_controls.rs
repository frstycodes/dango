// Minimize, maximize and close for Windows. The title bar is drawn by the app (`appears_transparent`), and
// unlike macOS, Windows then shows no caption buttons of its own. Each button is tagged with its
// WindowControlArea, so Windows does the hit-testing: clicks, Snap Layouts on maximize hover, and
// the close button all behave like native ones. Nothing is drawn on other platforms.
use gpui_kit::{AnyElement, App, Hsla, Window, WindowControlArea, component::ActiveTheme, div, prelude::*, px, rgb};

use crate::layout::TITLE_HEIGHT;

// Windows 11 caption buttons are 46px wide.
const BUTTON_WIDTH: f32 = 46.;
/// Room the controls take at the right of the title bar.
pub const WIDTH: f32 = if cfg!(target_os = "windows") { BUTTON_WIDTH * 3. } else { 0. };

// Segoe MDL2 Assets ships with Windows 10 and 11 and has the system's caption glyphs.
const ICON_FONT: &str = "Segoe MDL2 Assets";
const MINIMIZE: &str = "\u{E921}";
const MAXIMIZE: &str = "\u{E922}";
const RESTORE: &str = "\u{E923}";
const CLOSE: &str = "\u{E8BB}";

pub fn render(window: &Window, cx: &App) -> Option<AnyElement> {
    if !cfg!(target_os = "windows") {
        return None;
    }
    let theme = cx.theme();
    let foreground = theme.foreground;
    let hover = theme.foreground.opacity(0.08);
    let button = |id: &'static str, glyph: &'static str, area: WindowControlArea, hover_bg: Hsla, hover_fg: Hsla| {
        div()
            .id(id)
            .w(px(BUTTON_WIDTH))
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .font_family(ICON_FONT)
            .text_size(px(10.))
            .text_color(foreground)
            .hover(move |s| s.bg(hover_bg).text_color(hover_fg))
            .window_control_area(area)
            .child(glyph)
    };
    let maximize = if window.is_maximized() { RESTORE } else { MAXIMIZE };
    let close_red: Hsla = rgb(0xC42B1C).into();
    Some(
        div()
            .absolute()
            .top_0()
            .right_0()
            .h(px(TITLE_HEIGHT))
            .flex()
            .child(button("window-minimize", MINIMIZE, WindowControlArea::Min, hover, foreground))
            .child(button("window-maximize", maximize, WindowControlArea::Max, hover, foreground))
            .child(button("window-close", CLOSE, WindowControlArea::Close, close_red, gpui_kit::white()))
            .into_any_element(),
    )
}
