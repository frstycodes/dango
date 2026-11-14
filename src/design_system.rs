// The design system inside the app: every token and shared control from `palette` and `ui`, live, in
// both themes. Open it with `WAVE_DESIGN=1 cargo run`; `WAVE_DESIGN=buttons,choices` shows only those sections. It mirrors design/primitives.html, the source
// these were ported from, so a drift between the two shows up side by side.
use gpui_kit::{
    AnyElement, App, Context, FontWeight, Hsla, IntoElement, ParentElement, Render, SharedString, Styled, Window, WindowBounds, WindowOptions,
    component::{ActiveTheme, Theme, ThemeMode},
    div, prelude::*, px, size, svg,
};

use crate::{
    discord::Status,
    icons::{Glyph, MARK},
    mascot::Mood,
    palette::{self, Level},
    ui::{self, Check, Notice, Tone},
};

pub fn open(cx: &mut App) {
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(gpui_kit::Bounds::centered(None, size(px(1180.), px(860.)), cx))),
        titlebar: Some(gpui_kit::TitlebarOptions { title: Some("Wave Design System".into()), ..Default::default() }),
        ..Default::default()
    };
    gpui_kit::open_window(options, cx, |window, cx| {
        palette::sync(window, cx);
        let only = std::env::var("WAVE_DESIGN").ok().filter(|v| v != "1").map(|v| v.split(',').map(str::to_owned).collect());
        cx.new(|_| DesignSystem { only, volume: 0.6, ..Default::default() })
    })
    .expect("open design system window");
}

#[derive(Default)]
pub struct DesignSystem {
    only: Option<Vec<String>>,
    noise: bool,
    echo: bool,
    quality: usize,
    checks: [bool; 2],
    voice_mode: usize,
    chips: [bool; 3],
    volume: f32,
}

impl DesignSystem {
    fn shows(&self, section: &str) -> bool {
        self.only.as_ref().is_none_or(|only| only.iter().any(|s| s == section))
    }

    fn set_theme(&mut self, dark: bool, window: &mut Window, cx: &mut Context<Self>) {
        Theme::change(if dark { ThemeMode::Dark } else { ThemeMode::Light }, Some(window), cx);
        palette::apply(cx);
        cx.notify();
    }
}

impl Render for DesignSystem {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dark = cx.theme().is_dark();
        div()
            .id("design-system")
            .size_full()
            .overflow_y_scroll()
            .bg(palette::surface(Level::Page, cx))
            .text_color(palette::text(cx))
            .font_family(cx.theme().font_family.clone())
            .text_size(px(14.))
            .child(
                div()
                    .max_w(px(1040.))
                    .mx_auto()
                    .px(px(32.))
                    .py(px(28.))
                    .flex()
                    .flex_col()
                    .gap(px(40.))
                    .child(self.header(dark, window, cx))
                    .when(self.shows("colour"), |d| d.child(colours(cx)))
                    .when(self.shows("type"), |d| d.child(type_scale(cx)))
                    .when(self.shows("shape"), |d| d.child(shape(cx)))
                    .when(self.shows("buttons"), |d| d.child(buttons(cx)))
                    .when(self.shows("choices"), |d| d.child(self.choices(window, cx)))
                    .when(self.shows("badges"), |d| d.child(badges(cx)))
                    .when(self.shows("people"), |d| d.child(people(cx)))
                    .when(self.shows("feedback"), |d| d.child(feedback(cx)))
                    .when(self.shows("surfaces"), |d| d.child(surfaces(cx))),
            )
    }
}

impl DesignSystem {
    fn header(&self, dark: bool, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap(px(12.))
            .child(svg().path(MARK).size(px(28.)).text_color(gpui_kit::rgb(0xE8326F)))
            .child(div().flex().flex_col().child(div().text_size(px(28.)).font_weight(FontWeight::BOLD).text_color(palette::strong(cx)).child("Wave")).child(div().text_color(palette::muted(cx)).child("Design system, ported from design/")))
            .child(div().flex_1())
            .child({
                let this = cx.entity().downgrade();
                ui::segmented_live("theme", &["Light", "Dark"], dark as usize, move |i, window, cx| {
                    this.update(cx, |this, cx| this.set_theme(i == 1, window, cx)).ok();
                }, window, cx)
                .w(px(180.))
            })
    }

    fn choices(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let quality = ["720p", "1080p", "Source"];
        // Live controls keep spring state in the window, so they're built before the rows borrow `cx`.
        let noise = ui::switch_live("sw-noise", self.noise, window, cx);
        let echo = ui::switch_live("sw-echo", self.echo, window, cx);
        let quality_seg = {
            let this = cx.entity().downgrade();
            ui::segmented_live("quality", &quality, self.quality, move |i, _, cx| {
                this.update(cx, |this, cx| {
                    this.quality = i;
                    cx.notify();
                })
                .ok();
            }, window, cx)
        };
        let row = |id: &'static str, title: &'static str, desc: &'static str| {
            div().id(id).flex().items_center().gap(px(12.)).p(px(10.)).rounded(px(ui::RADIUS_MD)).cursor_pointer().hover(|s| s.bg(palette::hover(cx))).child(
                div().flex_1().min_w_0().flex().flex_col().child(div().font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(title)).when(!desc.is_empty(), |d| {
                    d.child(div().text_size(px(12.)).text_color(palette::muted(cx)).child(desc))
                }),
            )
        };
        section("Choices", "Switches apply at once; checkboxes pick many; radios pick one; segmented controls switch a value in place; chips filter.", cx).child(
            board(cx).child(
                div()
                    .flex()
                    .gap(px(24.))
                    .flex_wrap()
                    .child(
                        column()
                            .child(row("c-noise", "Noise suppression", "Removes background sound from your mic").child(noise).on_click(cx.listener(|this, _, _, cx| {
                                this.noise = !this.noise;
                                cx.notify();
                            })))
                            .child(row("c-echo", "Echo cancellation", "Stops others hearing themselves").child(echo).on_click(cx.listener(|this, _, _, cx| {
                                this.echo = !this.echo;
                                cx.notify();
                            }))),
                    )
                    .child(
                        column()
                            .child(row("k-0", "Show game activity", "Lets friends see what you play").child(ui::check(if self.checks[0] { Check::On } else { Check::Off }, cx)).on_click(cx.listener(|this, _, _, cx| {
                                this.checks[0] = !this.checks[0];
                                cx.notify();
                            })))
                            .child(row("k-1", "Allow DMs from server members", "").child(ui::check(if self.checks[1] { Check::On } else { Check::Off }, cx)).on_click(cx.listener(|this, _, _, cx| {
                                this.checks[1] = !this.checks[1];
                                cx.notify();
                            })))
                            .child(row("k-2", "Notify for every channel", "Some channels on, some off").child(ui::check(Check::Mixed, cx))),
                    )
                    .child(
                        column()
                            .child(row("r-0", "Voice activity", "Transmits when you speak").child(ui::radio(self.voice_mode == 0, cx)).on_click(cx.listener(|this, _, _, cx| {
                                this.voice_mode = 0;
                                cx.notify();
                            })))
                            .child(row("r-1", "Push to talk", "Hold a key to transmit").child(ui::radio(self.voice_mode == 1, cx)).on_click(cx.listener(|this, _, _, cx| {
                                this.voice_mode = 1;
                                cx.notify();
                            }))),
                    )
                    .child(
                        column()
                            .gap(px(16.))
                            .child(quality_seg)
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(6.))
                                    .child(label("Mira's volume", cx))
                                    .child(
                                        div()
                                            .id("volume")
                                            .w(px(240.))
                                            .cursor_pointer()
                                            .child(ui::slider(self.volume, cx))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.volume = if this.volume >= 0.9 { 0.1 } else { this.volume + 0.2 };
                                                cx.notify();
                                            })),
                                    )
                                    .child(div().text_size(px(12.)).text_color(palette::faint(cx)).child("Click to step it; dragging lands with the real slider")),
                            )
                            .child(div().flex().gap(px(8.)).flex_wrap().children(["Images", "Links", "Files"].iter().enumerate().map(|(i, name)| {
                                ui::chip(SharedString::from(format!("chip-{i}")), *name, self.chips[i], cx).on_click(cx.listener(move |this, _, _, cx| {
                                    this.chips[i] = !this.chips[i];
                                    cx.notify();
                                }))
                            }))),
                    ),
            ),
        )
    }
}

fn section(title: &'static str, lede: &'static str, cx: &App) -> gpui_kit::Div {
    div()
        .flex()
        .flex_col()
        .gap(px(12.))
        .child(div().text_size(px(20.)).font_weight(FontWeight::BOLD).text_color(palette::strong(cx)).child(title))
        .child(div().max_w(px(640.)).text_color(palette::muted(cx)).child(lede))
}

fn board(cx: &App) -> gpui_kit::Div {
    div().p(px(24.)).rounded(px(ui::RADIUS_LG)).bg(palette::surface(Level::Sidebar, cx)).border_1().border_color(palette::line(cx)).flex().flex_col().gap(px(20.))
}

fn column() -> gpui_kit::Div {
    div().flex().flex_col().gap(px(4.)).w(px(300.))
}

fn row() -> gpui_kit::Div {
    div().flex().items_center().gap(px(8.)).flex_wrap()
}

fn label(text: &'static str, cx: &App) -> gpui_kit::Div {
    div().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(palette::muted(cx)).child(text)
}

fn note(text: &'static str, cx: &App) -> gpui_kit::Div {
    div().text_size(px(12.)).text_color(palette::faint(cx)).child(text)
}

fn colours(cx: &App) -> impl IntoElement {
    let surface = |name: &'static str, level: Level| {
        div().flex_1().h(px(92.)).p(px(10.)).bg(palette::surface(level, cx)).flex().flex_col().justify_end().child(div().font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(name))
    };
    let ink = |name: &'static str, color: Hsla, use_: &'static str| {
        div().flex().items_baseline().gap(px(8.)).px(px(12.)).py(px(8.)).rounded(px(ui::RADIUS_SM)).bg(palette::surface(Level::Page, cx)).child(div().font_weight(FontWeight::SEMIBOLD).text_color(color).child(name)).child(note(use_, cx))
    };
    let swatch = |name: &'static str, color: Hsla| div().flex().flex_col().gap(px(6.)).w(px(120.)).child(div().h(px(48.)).rounded(px(ui::RADIUS_MD)).bg(color)).child(note(name, cx));
    section("Colour", "Graphite neutrals plus one magenta. Depth in the dark theme comes from lighter surfaces, never shadows.", cx)
        .child(
            div()
                .flex()
                .rounded(px(ui::RADIUS_LG))
                .border_1()
                .border_color(palette::line(cx))
                .child(surface("Rail", Level::Rail).rounded_tl(px(ui::RADIUS_LG)).rounded_bl(px(ui::RADIUS_LG)))
                .child(surface("Sidebar", Level::Sidebar))
                .child(surface("Page", Level::Page))
                .child(surface("Raised", Level::Raised))
                .child(surface("Overlay", Level::Overlay).rounded_tr(px(ui::RADIUS_LG)).rounded_br(px(ui::RADIUS_LG))),
        )
        .child(
            board(cx)
                .gap(px(6.))
                .p(px(12.))
                .child(ink("Strong", palette::strong(cx), "Names, titles, selected rows"))
                .child(ink("Default", palette::text(cx), "Message body"))
                .child(ink("Muted", palette::muted(cx), "Channel rows, descriptions"))
                .child(ink("Faint", palette::faint(cx), "Timestamps, placeholders"))
                .child(ink("Accent ink", palette::accent_ink(cx), "Links, mentions"))
                .child(ink("Danger ink", palette::danger_ink(cx), "Errors, destructive rows")),
        )
        .child(
            row()
                .gap(px(16.))
                .child(swatch("Accent", palette::accent()))
                .child(swatch("Accent hover", palette::accent_hover()))
                .child(swatch("Danger", palette::danger()))
                .child(swatch("Online", palette::status_in(Status::Online, cx)))
                .child(swatch("Idle", palette::status_in(Status::Idle, cx)))
                .child(swatch("Do not disturb", palette::status_in(Status::Dnd, cx)))
                .child(swatch("Offline", palette::status_in(Status::Offline, cx))),
        )
}

fn type_scale(cx: &App) -> impl IntoElement {
    let spec = |meta: &'static str, size: f32, weight: FontWeight, text: &'static str| {
        div()
            .flex()
            .items_baseline()
            .gap(px(16.))
            .py(px(10.))
            .border_t_1()
            .border_color(palette::line(cx))
            .child(div().w(px(140.)).flex_none().child(note(meta, cx)))
            .child(div().text_size(px(size)).font_weight(weight).text_color(palette::strong(cx)).child(text))
    };
    section("Type", "Gabarito for the interface, one family; sizes only from this scale.", cx).child(
        board(cx)
            .gap(px(0.))
            .child(spec("28 · bold", 28., FontWeight::BOLD, "Welcome to Night Shift"))
            .child(spec("20 · bold", 20., FontWeight::BOLD, "Voice and video"))
            .child(spec("16 · regular", 16., FontWeight::NORMAL, "anyone up for a few rounds after dinner?"))
            .child(spec("14 · medium", 14., FontWeight::MEDIUM, "general · looking-for-group · Lounge"))
            .child(spec("12 · semibold", 12., FontWeight::SEMIBOLD, "Today at 9:41 PM · Online · 3")),
    )
}

fn shape(cx: &App) -> impl IntoElement {
    let tile = |name: &'static str, radius: f32| {
        div()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(6.))
            .child(div().size(px(76.)).rounded(px(radius)).bg(palette::surface(Level::Raised, cx)).border_1().border_color(palette::line_strong(cx)))
            .child(note(name, cx))
    };
    section("Shape", "Controls 10, rows 8, cards 14, dialogs 20. The design draws these as squircles; gpui draws circular corners at these equivalents.", cx).child(
        row()
            .gap(px(20.))
            .child(tile("xs · 6", ui::RADIUS_XS))
            .child(tile("sm · 8", ui::RADIUS_SM))
            .child(tile("md · 10", ui::RADIUS_MD))
            .child(tile("lg · 14", ui::RADIUS_LG))
            .child(tile("xl · 20", ui::RADIUS_XL)),
    )
}

fn buttons(cx: &App) -> impl IntoElement {
    section("Buttons", "Verb plus object. One primary per surface; ghost for low-stakes actions in dense rows; danger only for the destructive step.", cx).child(
        board(cx)
            .child(
                row()
                    .child(ui::button("b-primary", Tone::Primary, cx).child("Join voice"))
                    .child(ui::button("b-secondary", Tone::Secondary, cx).child("Invite friends"))
                    .child(ui::button("b-ghost", Tone::Ghost, cx).child("Mark as read"))
                    .child(ui::button("b-danger", Tone::Danger, cx).child("Leave server")),
            )
            .child(
                row()
                    .child(ui::large(ui::button("b-lg", Tone::Primary, cx)).child(Icon::glyph(Glyph::Stream)).child("Go live"))
                    .child(ui::button("b-md", Tone::Primary, cx).child(Icon::glyph(Glyph::Stream)).child("Go live"))
                    .child(ui::small(ui::button("b-sm", Tone::Primary, cx)).child("Go live"))
                    .child(note("44 / 36 / 30 tall", cx)),
            )
            .child(
                row()
                    .child(ui::icon_button("i-pin", Glyph::Pin, "Pin", cx))
                    .child(ui::icon_button("i-reply", Glyph::Reply, "Reply", cx))
                    .child(ui::icon_button("i-settings", Glyph::Settings, "Settings", cx))
                    .child(note("Icon buttons: 32px, the hover fill, a tooltip", cx)),
            ),
    )
}

fn badges(cx: &App) -> impl IntoElement {
    section("Badges and keys", "A count asks for you, so it takes the accent. Tags say what something is.", cx).child(
        board(cx).child(
            row()
                .gap(px(14.))
                .child(ui::count_badge(3))
                .child(ui::count_badge(42))
                .child(ui::count_badge(120))
                .child(ui::tag("App", cx))
                .child(ui::tag("Host", cx))
                .child(ui::live_badge())
                .child(div().flex().gap(px(4.)).child(ui::kbd("=", cx)).child(ui::kbd("Backspace", cx)))
                .child(note("Mute", cx)),
        ),
    )
}

fn people(cx: &App) -> impl IntoElement {
    let ring = palette::surface(Level::Sidebar, cx);
    let person = |who: &'static str, name: &'static str, hue: u32, status: Status| {
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .relative()
                    .size(px(40.))
                    .child(div().size_full().rounded_full().bg(gpui_kit::rgb(hue)).flex().items_center().justify_center().text_color(gpui_kit::white()).font_weight(FontWeight::BOLD).child(&who[..1]))
                    .child(div().absolute().right(px(-2.)).bottom(px(-2.)).child(ui::status_mark(status, 14., ring, cx))),
            )
            .child(div().text_color(palette::muted(cx)).child(name))
    };
    section("Avatars and status", "Status is never colour alone: online is a dot, idle a moon, do not disturb a bar, offline a ring.", cx).child(
        board(cx).child(
            row()
                .gap(px(24.))
                .child(person("Mira", "Online", 0x5A4FB8, Status::Online))
                .child(person("Ana", "Idle", 0x1F7A8C, Status::Idle))
                .child(person("Theo", "Do not disturb", 0x8C6A1F, Status::Dnd))
                .child(person("Jun", "Offline", 0x7A5A2E, Status::Offline))
                .child(div().flex().items_center().gap(px(8.)).child(ui::bars("speaking", palette::accent_ink(cx), 12.)).child(div().text_color(palette::muted(cx)).child("Speaking"))),
        ),
    )
}

fn feedback(cx: &App) -> impl IntoElement {
    section("Feedback", "Progress splits done from left with a gap. Callouts sit inside the view they're about. Waits look alive.", cx).child(
        board(cx)
            .child(
                row()
                    .gap(px(24.))
                    .child(div().w(px(240.)).flex().flex_col().gap(px(8.)).child(label("clip.mp4 · 12.4 / 31 MB", cx)).child(ui::progress(0.4, cx)))
                    .child(div().w(px(240.)).flex().flex_col().gap(px(8.)).child(label("Almost done", cx)).child(ui::progress(0.92, cx))),
            )
            .child(ui::callout(Notice::Info, Glyph::Help, "Slowmode is on", "You can send one message every 10 seconds.", cx))
            .child(ui::callout(Notice::Warning, Glyph::ConnectionAverage, "Your connection is unstable", "Others may hear you cut out. Try a wired connection.", cx))
            .child(ui::callout(Notice::Danger, Glyph::ConnectionBad, "Couldn't send 2 messages", "Wave lost the connection. They're kept below.", cx))
            .child(ui::callout(Notice::Success, Glyph::Checkmark, "Two-factor is on", "Keep your backup codes somewhere safe.", cx))
            .child(ui::callout(Notice::Accent, Glyph::Inbox, "3 mentions while you were away", "The oldest is in #looking-for-group.", cx))
            .child(ui::divider(Some("Today"), cx))
            .child(
                row()
                    .gap(px(12.))
                    .child(ui::icon_tile(Glyph::Hashtag, palette::selected(cx), palette::strong(cx)))
                    .child(ui::icon_tile(Glyph::VolumeHigh, palette::selected(cx), palette::strong(cx)))
                    .child(ui::icon_tile(Glyph::Stream, palette::accent(), gpui_kit::white())),
            )
            .child(
                div()
                    .flex()
                    .gap(px(16.))
                    .child(div().flex_1().rounded(px(ui::RADIUS_LG)).bg(palette::surface(Level::Page, cx)).child(ui::empty_state("empty-idle", Mood::Idle, "No one's around", "None of your friends are online. Invite someone to play.", cx).child(ui::button("e-add", Tone::Primary, cx).mt(px(8.)).child("Add friend"))))
                    .child(div().flex_1().rounded(px(ui::RADIUS_LG)).bg(palette::surface(Level::Page, cx)).child(ui::empty_state("empty-wait", Mood::Waiting, "Loading messages…", "Fetching the last 50 messages in #general.", cx))),
            ),
    )
}

fn surfaces(cx: &App) -> impl IntoElement {
    let item = |id: &'static str, glyph: Glyph, text: &'static str| ui::menu_row(id, cx).child(Icon::glyph(glyph)).child(div().flex_1().child(text));
    section("Surfaces", "Menus and popovers float on the overlay surface with a hairline and shadow; rows inset by its padding.", cx).child(
        board(cx).child(
            ui::surface(cx)
                .w(px(260.))
                .child(item("m-reply", Glyph::Reply, "Reply"))
                .child(item("m-react", Glyph::EmojiAdd, "Add reaction"))
                .child(item("m-thread", Glyph::Thread, "Create thread"))
                .child(item("m-copy", Glyph::Copy, "Copy text"))
                .child(div().my(px(4.)).mx(px(8.)).h(px(1.)).bg(palette::line(cx)))
                .child(ui::menu_row("m-delete", cx).text_color(palette::danger_ink(cx)).child(Icon::glyph(Glyph::Trash)).child("Delete message")),
        ),
    )
}

// Glyphs at the size buttons and menu rows use.
struct Icon;
impl Icon {
    fn glyph(glyph: Glyph) -> AnyElement {
        gpui_kit::component::Icon::new(glyph).size(px(18.)).into_any_element()
    }
}
