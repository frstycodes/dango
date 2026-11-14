// Wave's context menus (design/menus.html, design/css/overlays.css `.menu`, `.mi`): a 248px overlay
// card, 34px rows with a muted leading icon, a faint line under the label for what the row says
// about itself, checkboxes on the right, radio rows that take a fill instead of a dot, one accent row
// and red danger rows that fill solid on hover, and submenus that fly out beside their row. Every
// right-click in Wave opens one of these.
use std::{rc::Rc, time::Duration};

use gpui_kit::{
    Animation, AnimationExt, AnyElement, App, BoxShadow, Context, Div, FontWeight, Hsla, MouseButton, Pixels, Point, SharedString, Stateful, Window,
    component::{ActiveTheme, Icon, input::Input},
    div, img, point, prelude::*, px,
};

use crate::{
    Rustcord,
    discord::{Emoji, Status},
    icons::Glyph,
    palette,
    settings::Fill,
    ui::{self, RADIUS_LG, RADIUS_MD, RADIUS_SM, RADIUS_XL, RADIUS_XS, SURFACE_INSET},
};

pub type Act = Rc<dyn Fn(&mut Rustcord, &mut Window, &mut Context<Rustcord>)>;
type Build = Rc<dyn Fn(&Rustcord) -> Vec<Entry>>;
type OnConfirm = Rc<dyn Fn(&mut Rustcord, String, &mut Window, &mut Context<Rustcord>)>;
type SetValue = Rc<dyn Fn(&mut Rustcord, f32, &mut Context<Rustcord>)>;

pub(crate) const WIDTH: f32 = 248.;
const SUB_WIDTH: f32 = 208.;
// A submenu holding checkbox rows widens so their labels fit beside the box.
const SUB_WIDE: f32 = 236.;
// Rows are 34px: 6px padding around a 14px label on its 20px line; a sub-line adds 16.
const ROW: f32 = 34.;
const HINT_ROW: f32 = 48.;
const SEPARATOR: f32 = 9.;
const SLIDER_ROW: f32 = 68.;
const EMOJI_ROW: f32 = 44.;
const HEADER: f32 = 52.;
// Rows sit 1px apart.
const GAP: f32 = 1.;
const LEAD: f32 = 18.;
// Submenus open on hover after a short intent delay, then fade in on the pop duration.
const SUB_DELAY: Duration = Duration::from_millis(80);

#[derive(Clone)]
pub enum Entry {
    Item(Item),
    Separator,
    // The target the menu is about: an avatar, server or channel tile, its name and one line.
    Header { lead: HeadLead, title: SharedString, subtitle: Option<SharedString> },
    // Quick reactions on top of the message menu: recent emoji, one click each, then the full picker.
    Emojis { quick: Vec<(Emoji, Act)>, more: Act },
    Slider { label: SharedString, key: SharedString, value: f32, on_set: SetValue },
}

#[derive(Clone)]
pub enum HeadLead {
    Avatar { url: String, status: Option<Status> },
    Server { icon: Option<String>, name: String },
    Tile(Glyph),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tone {
    Plain,
    // The one action the menu exists to offer (Invite people).
    Accent,
    Danger,
}

#[derive(Clone, Copy)]
enum Mark {
    Check(bool),
    // One of a group: the chosen row takes a fill a step above hover, no dot.
    Radio(bool),
}

#[derive(Clone)]
enum Lead {
    Glyph(Glyph),
    Emoji(Emoji),
    Status(Status),
    // A role's colour as a 12px dot.
    Dot(u32),
    Server { icon: Option<String>, name: String },
}

#[derive(Clone)]
pub struct Item {
    label: SharedString,
    hint: Option<SharedString>,
    tone: Tone,
    mark: Option<Mark>,
    lead: Option<Lead>,
    sub: Vec<Entry>,
    act: Option<Act>,
    disabled: bool,
}

pub fn item(label: impl Into<SharedString>, act: impl Fn(&mut Rustcord, &mut Window, &mut Context<Rustcord>) + 'static) -> Item {
    Item { label: label.into(), hint: None, tone: Tone::Plain, mark: None, lead: None, sub: Vec::new(), act: Some(Rc::new(act)), disabled: false }
}

pub fn submenu(label: impl Into<SharedString>, entries: Vec<Entry>) -> Item {
    Item { label: label.into(), hint: None, tone: Tone::Plain, mark: None, lead: None, sub: entries, act: None, disabled: false }
}

pub fn slider(label: impl Into<SharedString>, key: impl Into<SharedString>, value: f32, on_set: impl Fn(&mut Rustcord, f32, &mut Context<Rustcord>) + 'static) -> Entry {
    Entry::Slider { label: label.into(), key: key.into(), value, on_set: Rc::new(on_set) }
}

pub fn header(lead: HeadLead, title: impl Into<SharedString>, subtitle: Option<SharedString>) -> Entry {
    Entry::Header { lead, title: title.into(), subtitle }
}

impl Item {
    pub fn hint(mut self, hint: impl Into<SharedString>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub fn danger(mut self) -> Self {
        self.tone = Tone::Danger;
        self
    }

    pub fn checked(mut self, on: bool) -> Self {
        self.mark = Some(Mark::Check(on));
        self
    }

    pub fn radio(mut self, on: bool) -> Self {
        self.mark = Some(Mark::Radio(on));
        self
    }

    pub fn brand(mut self) -> Self {
        self.tone = Tone::Accent;
        self
    }

    pub fn disabled(mut self) -> Self {
        self.disabled = true;
        self
    }

    pub fn icon(mut self, glyph: Glyph) -> Self {
        self.lead = Some(Lead::Glyph(glyph));
        self
    }

    pub fn emoji(mut self, emoji: Emoji) -> Self {
        self.lead = Some(Lead::Emoji(emoji));
        self
    }

    pub fn status(mut self, status: Status) -> Self {
        self.lead = Some(Lead::Status(status));
        self
    }

    // A role row's colour; a role without one keeps the slot empty.
    pub fn dot(mut self, color: Option<u32>) -> Self {
        self.lead = color.map(Lead::Dot);
        self
    }

    pub fn server(mut self, icon: Option<String>, name: String) -> Self {
        self.lead = Some(Lead::Server { icon, name });
        self
    }

    pub fn entry(self) -> Entry {
        Entry::Item(self)
    }

    fn height(&self) -> f32 {
        if self.hint.is_some() { HINT_ROW } else { ROW }
    }

    fn chosen(&self) -> bool {
        matches!(self.mark, Some(Mark::Radio(true)))
    }
}

impl Entry {
    pub fn is_copy_id(&self) -> bool {
        matches!(self, Entry::Item(item) if item.label.starts_with("Copy ") && item.label.ends_with(" ID"))
    }

    fn height(&self) -> f32 {
        match self {
            Entry::Item(item) => item.height(),
            Entry::Separator => SEPARATOR,
            Entry::Header { .. } => HEADER,
            Entry::Slider { .. } => SLIDER_ROW,
            Entry::Emojis { .. } => EMOJI_ROW,
        }
    }

    fn has_lead(&self) -> bool {
        matches!(self, Entry::Item(Item { lead: Some(_), .. }))
    }

    fn has_check(&self) -> bool {
        matches!(self, Entry::Item(Item { mark: Some(Mark::Check(_)), .. }))
    }
}

// Rows above entry `i`, with the gaps between them: where its top sits below the card's first row.
fn offset(entries: &[Entry], i: usize) -> f32 {
    entries[..i].iter().map(|e| e.height() + GAP).sum()
}

fn card_height(entries: &[Entry]) -> f32 {
    offset(entries, entries.len()) - GAP + 2. * SURFACE_INSET
}

// Drops separators that would sit at the top, bottom or next to another, so menus can be built
// from optional groups without stray lines.
pub fn tidy(entries: Vec<Entry>) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    for entry in entries {
        let after_separator = matches!(out.last(), None | Some(Entry::Separator) | Some(Entry::Header { .. }));
        if matches!(entry, Entry::Separator) && after_separator {
            continue;
        }
        out.push(entry);
    }
    if matches!(out.last(), Some(Entry::Separator)) {
        out.pop();
    }
    out
}

// An open menu rebuilds its entries from current state every frame, so checkboxes, sliders and
// labels stay true after each click.
pub struct OpenMenu {
    at: Point<Pixels>,
    build: Build,
    // The open submenu at each level, as indexes from the top-level entries down.
    sub: Vec<usize>,
}

// A one-field dialog (nickname, note) or a confirmation (kick, ban, remove friend).
pub struct Dialog {
    title: SharedString,
    body: SharedString,
    confirm: SharedString,
    danger: bool,
    input: bool,
    on_confirm: OnConfirm,
}

impl Rustcord {
    pub(crate) fn open_menu(&mut self, at: Point<Pixels>, entries: Vec<Entry>, cx: &mut Context<Self>) {
        self.open_live_menu(at, move |_| entries.clone(), cx);
    }

    // A menu built from state each time it's drawn, e.g. one whose checkboxes change.
    pub(crate) fn open_live_menu(&mut self, at: Point<Pixels>, build: impl Fn(&Rustcord) -> Vec<Entry> + 'static, cx: &mut Context<Self>) {
        self.menu = Some(OpenMenu { at, build: Rc::new(move |this| tidy(build(this))), sub: Vec::new() });
        cx.notify();
    }

    // Demo scenes open a submenu chain without hovering.
    #[cfg(debug_assertions)]
    pub(crate) fn open_submenus(&mut self, path: Vec<usize>) {
        if let Some(menu) = &mut self.menu {
            menu.sub = path;
        }
    }

    pub(crate) fn close_menu(&mut self) -> bool {
        self.menu.take().is_some() | self.dialog.take().is_some()
    }

    // gpui's window and context ride along with the dialog's five fields.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn prompt(
        &mut self,
        title: impl Into<SharedString>,
        body: impl Into<SharedString>,
        value: &str,
        confirm: impl Into<SharedString>,
        on_confirm: impl Fn(&mut Rustcord, String, &mut Window, &mut Context<Rustcord>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.menu = None;
        self.dialog_input.update(cx, |state, cx| {
            state.set_value(value.to_owned(), window, cx);
            state.focus(window, cx);
        });
        self.dialog = Some(Dialog { title: title.into(), body: body.into(), confirm: confirm.into(), danger: false, input: true, on_confirm: Rc::new(on_confirm) });
        cx.notify();
    }

    pub(crate) fn confirm(
        &mut self,
        title: impl Into<SharedString>,
        body: impl Into<SharedString>,
        confirm: impl Into<SharedString>,
        on_confirm: impl Fn(&mut Rustcord, &mut Window, &mut Context<Rustcord>) + 'static,
        cx: &mut Context<Self>,
    ) {
        self.menu = None;
        let on_confirm = Rc::new(on_confirm);
        self.dialog = Some(Dialog {
            title: title.into(),
            body: body.into(),
            confirm: confirm.into(),
            danger: true,
            input: false,
            on_confirm: Rc::new(move |this, _, window, cx| on_confirm(this, window, cx)),
        });
        cx.notify();
    }

    fn run_menu_item(&mut self, path: &[usize], window: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = &self.menu else { return };
        let top = (menu.build)(self);
        let mut entries = &top;
        let mut found: Option<&Item> = None;
        for &i in path {
            let Some(Entry::Item(item)) = entries.get(i) else { return };
            found = Some(item);
            entries = &item.sub;
        }
        let Some(item) = found else { return };
        let Some(act) = item.act.clone().filter(|_| !item.disabled) else { return };
        // Checkbox and radio rows stay open so several can be flipped.
        let keep_open = item.mark.is_some();
        if !keep_open {
            self.menu = None;
        }
        act(self, window, cx);
        cx.notify();
    }

    pub(crate) fn render_menu(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let menu = self.menu.as_ref()?;
        let entries = (menu.build)(self);
        let viewport = window.viewport_size();
        let (vw, vh) = (f32::from(viewport.width), f32::from(viewport.height));
        let x = f32::from(menu.at.x).min(vw - WIDTH - 8.).max(8.);
        let y = f32::from(menu.at.y).min(vh - card_height(&entries) - 8.).max(8.);
        // Keyed by where it opened, so a fresh right-click replays the rise.
        let open_key = format!("{:.0}-{:.0}", f32::from(menu.at.x), f32::from(menu.at.y));
        let top = self.menu_card(&entries, &[], menu.sub.first().copied(), WIDTH, cx).left(px(x)).top(px(y)).max_h(px(vh - 16.)).overflow_y_scroll();
        let mut cards = vec![rise(top, format!("menu-rise-{open_key}"), Duration::ZERO).into_any_element()];
        // One flyout per open level beside its parent row, its first row level with that row; it
        // flips to the left at the window's edge.
        let (mut level, mut card_x, mut card_y, mut card_w) = (&entries, x, y, WIDTH);
        for (depth, &i) in menu.sub.iter().enumerate() {
            let Some(Entry::Item(item)) = level.get(i) else { break };
            if item.sub.is_empty() {
                break;
            }
            let sub_w = if item.sub.iter().any(Entry::has_check) { SUB_WIDE } else { SUB_WIDTH };
            card_x = if card_x + card_w + 4. + sub_w < vw { card_x + card_w + 4. } else { card_x - sub_w - 4. };
            card_y = (card_y + offset(level, i)).min(vh - card_height(&item.sub) - 8.).max(8.);
            card_w = sub_w;
            let path = &menu.sub[..=depth];
            let card = self.menu_card(&item.sub, path, menu.sub.get(depth + 1).copied(), sub_w, cx).left(px(card_x)).top(px(card_y));
            cards.push(rise(card, format!("menu-rise-{open_key}-{path:?}"), SUB_DELAY).into_any_element());
            level = &item.sub;
        }
        Some(
            div()
                .id("menu-backdrop")
                .absolute()
                .inset_0()
                .occlude()
                .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| {
                    this.menu = None;
                    cx.notify();
                }))
                .on_mouse_down(MouseButton::Right, cx.listener(|this, _, _, cx| {
                    this.menu = None;
                    cx.notify();
                }))
                .children(cards)
                .into_any_element(),
        )
    }

    fn menu_card(&self, entries: &[Entry], path: &[usize], open_sub: Option<usize>, width: f32, cx: &mut Context<Self>) -> Stateful<Div> {
        // Rows line their labels up: when any row has a leading icon, the others keep its slot.
        let lead_slot = entries.iter().any(Entry::has_lead);
        let rows: Vec<AnyElement> = entries
            .iter()
            .enumerate()
            .map(|(i, entry)| match entry {
                Entry::Separator => separator(cx).into_any_element(),
                Entry::Header { lead, title, subtitle } => menu_header(lead, title.clone(), subtitle.clone(), cx).into_any_element(),
                Entry::Slider { label, key, value, on_set } => self.menu_slider(label.clone(), key.clone(), *value, on_set.clone(), cx).into_any_element(),
                Entry::Emojis { quick, more } => quick_reactions(quick, more.clone(), cx).into_any_element(),
                Entry::Item(item) => {
                    let mut item_path = path.to_vec();
                    item_path.push(i);
                    self.menu_row(item, item_path, open_sub == Some(i), lead_slot, cx).into_any_element()
                }
            })
            .collect();
        menu_surface(width, cx).id(SharedString::from(format!("menu-{path:?}"))).absolute().on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()).children(rows)
    }

    fn menu_slider(&self, label: SharedString, key: SharedString, value: f32, on_set: SetValue, cx: &mut Context<Self>) -> Div {
        div()
            .h(px(SLIDER_ROW))
            .pt(px(6.))
            .pb(px(10.))
            .px(px(10.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .justify_between()
                    .text_size(px(14.))
                    .line_height(px(20.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(palette::text(cx))
                    .child(label)
                    .child(div().text_color(palette::muted(cx)).child(format!("{:.0}%", value * 200.))),
            )
            .child(self.range(key, value, Fill::Accent, move |this, f, cx| on_set(this, f.clamp(0., 1.), cx), cx))
    }

    fn menu_row(&self, item: &Item, path: Vec<usize>, sub_open: bool, lead_slot: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        let has_sub = !item.sub.is_empty();
        let run_path = path.clone();
        menu_item_row(SharedString::from(format!("menu-item-{path:?}")), item, sub_open, lead_slot, cx)
            // Hovering a row opens its submenu and closes any deeper one from a sibling.
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                let Some(menu) = &mut this.menu else { return };
                if !*hovered {
                    return;
                }
                let next = if has_sub { path.clone() } else { path[..path.len() - 1].to_vec() };
                if menu.sub != next {
                    menu.sub = next;
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |this, _, window, cx| this.run_menu_item(&run_path, window, cx)))
    }

    pub(crate) fn render_dialog(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let dialog = self.dialog.as_ref()?;
        let submit = cx.listener(|this, _, window, cx| {
            let Some(dialog) = this.dialog.take() else { return };
            let value = this.dialog_input.read(cx).value().to_string();
            (dialog.on_confirm)(this, value, window, cx);
            cx.notify();
        });
        let tone = if dialog.danger { ui::Tone::Danger } else { ui::Tone::Primary };
        let card = div()
            .id("dialog")
            .w(px(420.))
            .p(px(24.))
            .rounded(px(RADIUS_XL))
            .bg(palette::surface(palette::Level::Raised, cx))
            .shadow(pop_shadow(cx))
            .flex()
            .flex_col()
            .gap(px(16.))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            // GAP: a destructive confirm leads its title with Phosphor's filled warning in danger ink;
            // that glyph isn't in `icons.rs` yet.
            .child(div().text_size(px(20.)).line_height(px(24.)).font_weight(FontWeight::BOLD).text_color(palette::strong(cx)).child(dialog.title.clone()))
            .child(div().text_size(px(14.)).line_height(px(20.)).text_color(palette::muted(cx)).child(dialog.body.clone()))
            .when(dialog.input, |d| d.child(field(cx).child(div().flex_1().min_w_0().child(Input::new(&self.dialog_input).appearance(false)))))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.))
                    .child(ui::button("dialog-cancel", ui::Tone::Secondary, cx).child("Cancel").on_click(cx.listener(|this, _, _, cx| {
                        this.dialog = None;
                        cx.notify();
                    })))
                    .child(ui::button("dialog-confirm", tone, cx).child(dialog.confirm.clone()).on_click(submit)),
            );
        // PATCH(transform): the card pops from scale 0.95 with its fade (CSS `pop`, 260ms ease-out).
        let card = card.with_animation("dialog-pop", Animation::new(ui::DUR_PANEL).with_easing(ease_out), |card, t| card.opacity(t));
        Some(
            div()
                .id("dialog-backdrop")
                .absolute()
                .inset_0()
                .occlude()
                .bg(scrim(cx))
                .flex()
                .items_center()
                .justify_center()
                .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| {
                    this.dialog = None;
                    cx.notify();
                }))
                .child(card)
                .into_any_element(),
        )
    }
}

// The menu card: overlay surface, 6px inset, rows 1px apart, the pop shadow with its hairline.
// MOVE(ui): menu_surface
pub(crate) fn menu_surface(width: f32, cx: &App) -> Div {
    // The hairline is the shadow's 1px ring outside the edge, as in CSS, not a border inside it.
    // PATCH(squircle): the card's corners are the design's 20px squircle.
    ui::surface(cx).border_0().w(px(width)).rounded(px(RADIUS_LG)).shadow(pop_shadow(cx)).gap(px(GAP))
}

// The look of one menu row, without its behaviour: callers add `.on_click` (and hover for submenus).
// MOVE(ui): menu_item_row
pub(crate) fn menu_item_row(id: SharedString, item: &Item, sub_open: bool, lead_slot: bool, cx: &App) -> Stateful<Div> {
    let colors = RowColors::of(item, cx);
    let has_sub = !item.sub.is_empty();
    let chosen = item.chosen();
    let (hover_bg, hover_ink, hover_icon, hover_hint) = (colors.hover_bg, colors.hover_ink, colors.hover_icon, colors.hover_hint);
    let group = id.clone();
    let lead = item.lead.as_ref().map(|lead| match lead {
        Lead::Glyph(glyph) => div().group_hover(group.clone(), move |s| s.text_color(hover_icon)).text_color(colors.icon).child(Icon::new(*glyph).size(px(LEAD))).into_any_element(),
        Lead::Emoji(emoji) => emoji_face(emoji, LEAD).into_any_element(),
        // The 14px presence mark with no ring of its own; its cut-outs take the menu's colour.
        Lead::Status(status) => ui::status_mark(*status, 14., palette::surface(palette::Level::Overlay, cx), cx).border(px(0.)).into_any_element(),
        Lead::Dot(color) => div().size(px(12.)).rounded_full().bg(gpui_kit::rgb(*color)).into_any_element(),
        Lead::Server { icon, name } => server_icon(icon.as_deref(), name, 20., 6., cx),
    });
    let lead = lead.or_else(|| lead_slot.then(|| div().size(px(LEAD)).into_any_element()));
    div()
        .id(id.clone())
        .group(group.clone())
        .w_full()
        .min_h(px(item.height()))
        .py(px(6.))
        .px(px(10.))
        .flex_none()
        .rounded(px(RADIUS_SM))
        .flex()
        .items_center()
        .gap(px(10.))
        .text_size(px(14.))
        .line_height(px(20.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(colors.ink)
        .when(!item.disabled, |d| d.cursor_pointer())
        .when(chosen, |d| d.bg(palette::press(cx)))
        .when(sub_open, |d| d.bg(hover_bg).text_color(hover_ink))
        .when(!item.disabled, |d| d.hover(move |s| s.bg(hover_bg).text_color(hover_ink)))
        .children(lead.map(|lead| div().size(px(LEAD)).flex_none().flex().items_center().justify_center().child(lead)))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(div().truncate().child(item.label.clone()))
                .children(item.hint.clone().map(|hint| {
                    div()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .font_weight(FontWeight::NORMAL)
                        .text_color(colors.hint)
                        .when(!item.disabled, |d| d.group_hover(group.clone(), move |s| s.text_color(hover_hint)))
                        .child(hint)
                })),
        )
        .children(match item.mark {
            Some(Mark::Check(on)) => Some(ui::check(if on { ui::Check::On } else { ui::Check::Off }, cx).size(px(18.)).into_any_element()),
            _ => None,
        })
        .when(has_sub, |d| d.child(div().text_color(palette::faint(cx)).child(Icon::new(Glyph::ChevronRight).size(px(14.)))))
}

// A row's inks at rest and under the pointer, by tone; disabled rows don't change.
struct RowColors {
    ink: Hsla,
    icon: Hsla,
    hint: Hsla,
    hover_bg: Hsla,
    hover_ink: Hsla,
    hover_icon: Hsla,
    hover_hint: Hsla,
}

impl RowColors {
    fn of(item: &Item, cx: &App) -> Self {
        let (strong, muted, faint) = (palette::strong(cx), palette::muted(cx), palette::faint(cx));
        let white = gpui_kit::white();
        if item.disabled {
            return RowColors { ink: faint, icon: faint, hint: muted, hover_bg: gpui_kit::transparent_black(), hover_ink: faint, hover_icon: faint, hover_hint: muted };
        }
        match item.tone {
            Tone::Danger => {
                let ink = palette::danger_ink(cx);
                RowColors { ink, icon: ink, hint: mix(ink, muted, 0.2), hover_bg: palette::danger(), hover_ink: white, hover_icon: white, hover_hint: white.opacity(0.88) }
            }
            Tone::Accent => {
                let ink = palette::accent_ink(cx);
                RowColors { ink, icon: ink, hint: faint, hover_bg: palette::accent_soft(cx), hover_ink: ink, hover_icon: ink, hover_hint: faint }
            }
            Tone::Plain => {
                let icon = if item.chosen() { strong } else { muted };
                // The chosen radio row's hover goes a step past its own fill.
                let hover_bg = if item.chosen() { strong.opacity(0.13) } else { palette::hover(cx) };
                RowColors { ink: if item.chosen() { strong } else { palette::text(cx) }, icon, hint: faint, hover_bg, hover_ink: strong, hover_icon: icon, hover_hint: faint }
            }
        }
    }
}

fn separator(cx: &App) -> Div {
    div().h(px(SEPARATOR)).flex_none().py(px(4.)).px(px(8.)).child(div().h(px(1.)).bg(palette::line(cx)))
}

fn menu_header(lead: &HeadLead, title: SharedString, subtitle: Option<SharedString>, cx: &App) -> Div {
    let overlay = palette::surface(palette::Level::Overlay, cx);
    let lead = match lead {
        HeadLead::Avatar { url, status } => crate::layout::avatar_with_status(url, 32., *status, overlay),
        HeadLead::Server { icon, name } => server_icon(icon.as_deref(), name, 32., RADIUS_XS, cx),
        HeadLead::Tile(glyph) => div().size(px(32.)).rounded(px(RADIUS_SM)).bg(palette::selected(cx)).text_color(palette::strong(cx)).flex().items_center().justify_center().child(Icon::new(*glyph).size(px(18.))).into_any_element(),
    };
    div()
        .h(px(HEADER))
        .flex_none()
        .pt(px(8.))
        .pb(px(10.))
        .px(px(10.))
        .flex()
        .items_center()
        .gap(px(10.))
        .child(div().flex_none().child(lead))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(div().truncate().text_size(px(14.)).line_height(px(18.)).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(title))
                .children(subtitle.map(|s| div().truncate().text_size(px(12.)).line_height(px(16.)).text_color(palette::muted(cx)).child(s))),
        )
}

// A server's icon, or its initials on a tile until it has one.
fn server_icon(icon: Option<&str>, name: &str, size: f32, radius: f32, cx: &App) -> AnyElement {
    if let Some(url) = icon {
        return img(url.to_owned()).size(px(size)).rounded(px(radius)).into_any_element();
    }
    div()
        .size(px(size))
        .rounded(px(radius))
        .bg(palette::selected(cx))
        .text_color(palette::strong(cx))
        .text_size(px((size * 0.38).round()))
        .font_weight(FontWeight::BOLD)
        .flex()
        .items_center()
        .justify_center()
        .child(crate::sidebar::initials(name))
        .into_any_element()
}

fn quick_reactions(quick: &[(Emoji, Act)], more: Act, cx: &mut Context<Rustcord>) -> Div {
    let (fill, press, muted, strong) = (palette::selected(cx), palette::press(cx), palette::muted(cx), palette::strong(cx));
    let cell = |id: SharedString, ink: Hsla| {
        div()
            .id(id)
            .flex_1()
            .h(px(38.))
            .rounded(px(RADIUS_SM))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .bg(fill)
            .text_color(muted)
            .hover(move |s| s.bg(press).text_color(ink))
    };
    let faces = quick.iter().enumerate().map(|(j, (emoji, act))| {
        let act = act.clone();
        // PATCH(transform): the button presses to scale 0.97.
        cell(SharedString::from(format!("menu-emoji-{j}")), strong).child(emoji_face(emoji, 20.)).on_click(cx.listener(move |this, _, window, cx| {
            this.menu = None;
            act(this, window, cx);
            cx.notify();
        }))
    });
    let picker = cell("menu-emoji-more".into(), strong)
        .child(Icon::new(Glyph::Emoji).size(px(18.)))
        .on_click(cx.listener(move |this, _, window, cx| {
            this.menu = None;
            more(this, window, cx);
            cx.notify();
        }));
    div().h(px(EMOJI_ROW)).flex_none().pt(px(2.)).px(px(2.)).pb(px(4.)).flex().gap(px(4.)).children(faces).child(picker)
}

fn emoji_face(emoji: &Emoji, size: f32) -> AnyElement {
    match emoji.url() {
        Some(url) => img(url).size(px(size)).into_any_element(),
        None => div().text_size(px(size)).line_height(px(size)).child(emoji.name.clone()).into_any_element(),
    }
}

// A floating surface's entrance: a 200ms ease-out fade, after `delay` (a submenu's intent delay).
// MOVE(ui): rise
pub(crate) fn rise<E: IntoElement + Styled + 'static>(el: E, id: String, delay: Duration) -> gpui_kit::AnimationElement<E> {
    let lead = delay.as_secs_f32() / (delay + ui::DUR_POP).as_secs_f32();
    // PATCH(transform): scale from 0.95 at the corner nearest the trigger (top left for a context
    // menu, the parent row's side for a submenu) alongside this fade.
    el.with_animation(SharedString::from(id), Animation::new(delay + ui::DUR_POP), move |el, t| el.opacity(ease_out(((t - lead) / (1. - lead)).clamp(0., 1.))))
}

// The pop shadow: a soft drop and a 1px hairline ring (tokens.css --shadow-pop).
// MOVE(ui): pop_shadow
pub(crate) fn pop_shadow(cx: &App) -> Vec<BoxShadow> {
    let drop = if cx.theme().is_dark() { gpui_kit::black().opacity(0.45) } else { gpui_kit::rgb(0x3B2A22).into() };
    let drop = if cx.theme().is_dark() { drop } else { Hsla { a: 0.16, ..drop } };
    vec![
        BoxShadow { color: drop, offset: point(px(0.), px(12.)), blur_radius: px(32.), spread_radius: px(0.), inset: false },
        BoxShadow { color: palette::line_strong(cx), offset: point(px(0.), px(0.)), blur_radius: px(0.), spread_radius: px(1.), inset: false },
    ]
}

// What sits behind a dialog (tokens.css --scrim).
// MOVE(ui): scrim
pub(crate) fn scrim(cx: &App) -> Hsla {
    if cx.theme().is_dark() { Hsla { a: 0.7, ..gpui_kit::rgb(0x0B0709).into() } } else { Hsla { a: 0.4, ..gpui_kit::rgb(0x24171C).into() } }
}

// A 40px text field frame on the page colour (primitives.css `.input`).
// MOVE(ui): field_frame
pub(crate) fn field_frame(cx: &App) -> Div {
    div()
        .h(px(40.))
        .px(px(12.))
        .rounded(px(RADIUS_MD))
        .bg(palette::surface(palette::Level::Page, cx))
        .border_1()
        .border_color(palette::line_strong(cx))
        .flex()
        .items_center()
        .gap(px(8.))
        .text_size(px(16.))
        .text_color(palette::strong(cx))
}

// The field with its focus ring, which it holds while its dialog is open: the dialog focuses it on open.
fn field(cx: &App) -> Div {
    let ring = BoxShadow { color: palette::accent_soft(cx), offset: point(px(0.), px(0.)), blur_radius: px(0.), spread_radius: px(3.), inset: false };
    field_frame(cx).border_color(palette::accent()).shadow(vec![ring])
}

// The house ease-out, cubic-bezier(0.23, 1, 0.32, 1) (tokens.css --ease-out).
// MOVE(ui): ease_out
pub(crate) fn ease_out(t: f32) -> f32 {
    gpui_kit::base::motion::Easing::cubic_bezier(0.23, 1., 0.32, 1.).map(|e| e.sample(t)).unwrap_or(t)
}

// Linear mix in RGB, for a tint between two inks.
fn mix(from: Hsla, to: Hsla, t: f32) -> Hsla {
    let (a, b) = (gpui_kit::Rgba::from(from), gpui_kit::Rgba::from(to));
    let lerp = |x: f32, y: f32| x + (y - x) * t;
    gpui_kit::Rgba { r: lerp(a.r, b.r), g: lerp(a.g, b.g), b: lerp(a.b, b.b), a: lerp(a.a, b.a) }.into()
}

impl Rustcord {
    // Runs a REST call in the background; a failure shows as a toast, success runs `then`.
    pub(crate) fn rest(
        &mut self,
        method: reqwest::Method,
        path: String,
        body: Option<serde_json::Value>,
        then: impl FnOnce(&mut Rustcord, &mut Context<Rustcord>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let Some(client) = self.client.clone() else { return };
        cx.spawn(async move |this, cx| {
            let result = client.call(method, path, body).await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(_) => then(this, cx),
                    Err(error) => this.show_toast(error, cx),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    // Discord's optimistic actions: `apply` changes what you see at once, the request goes out
    // in the background, and if Discord refuses, `revert` puts it back and a toast says why.
    pub(crate) fn rest_optimistic(
        &mut self,
        method: reqwest::Method,
        path: String,
        body: Option<serde_json::Value>,
        apply: impl FnOnce(&mut Rustcord),
        revert: impl FnOnce(&mut Rustcord) + 'static,
        cx: &mut Context<Self>,
    ) {
        apply(self);
        cx.notify();
        let Some(client) = self.client.clone() else { return };
        cx.spawn(async move |this, cx| {
            let Err(error) = client.call(method, path, body).await else { return };
            this.update(cx, |this, cx| {
                revert(this);
                this.show_toast(error, cx);
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn show_toast(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        let text = text.into();
        self.toast = Some(text.clone());
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(std::time::Duration::from_secs(4)).await;
            this.update(cx, |this, cx| {
                if this.toast.as_ref() == Some(&text) {
                    this.toast = None;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    pub(crate) fn render_toast(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let text = self.toast.clone()?;
        // GAP: the design leads a toast with its tone's icon and can carry one action; `show_toast`
        // carries only text, so neither shows yet.
        let toast = div()
            .max_w(px(380.))
            .min_h(px(40.))
            .py(px(10.))
            .pl(px(14.))
            .pr(px(10.))
            .rounded(px(RADIUS_LG))
            .bg(palette::surface(palette::Level::Overlay, cx))
            .shadow(pop_shadow(cx))
            .flex()
            .items_center()
            .text_size(px(14.))
            .line_height(px(20.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(palette::strong(cx))
            .child(text.clone());
        // Rises 8px into place with its fade (CSS `toast-in`, 240ms).
        // PATCH(transform): also from scale 0.97, and from blur(4px) once gpui has a filter blur.
        Some(
            div()
                .absolute()
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .child(toast)
                .with_animation(SharedString::from(format!("toast-{text}")), Animation::new(Duration::from_millis(240)).with_easing(ease_out), |wrap, t| wrap.bottom(px(24. - 8. * (1. - t))).opacity(t))
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tidy_drops_stray_separators() {
        let noop = |_: &mut Rustcord, _: &mut Window, _: &mut Context<Rustcord>| {};
        let entries = vec![Entry::Separator, item("a", noop).entry(), Entry::Separator, Entry::Separator, item("b", noop).entry(), Entry::Separator];
        let kinds: Vec<bool> = tidy(entries).iter().map(|e| matches!(e, Entry::Separator)).collect();
        assert_eq!(kinds, [false, true, false]);
    }
}
