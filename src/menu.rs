// Wave's context menus (design/menus.html, design/css/overlays.css `.menu`, `.mi`): a 248px overlay
// card, 34px rows with a muted leading icon, a faint line under the label for what the row says
// about itself, checkboxes on the right, radio rows that take a fill instead of a dot, one accent row
// and red danger rows that fill solid on hover, and submenus that fly out beside their row. Every
// right-click in Wave opens one of these.
use std::{cell::RefCell, rc::Rc, time::Duration};

use gpui_kit::{
    Animation, AnimationExt, AnyElement, App, BoxShadow, Context, Div, FocusHandle, FontWeight, Hsla, KeyDownEvent, MouseButton, Pixels, Point, SharedString, Stateful, Window,
    component::{Icon, input::Input},
    div, img, point, prelude::*, px,
};

use crate::{
    Rustcord,
    discord::{Emoji, Status},
    icons::Glyph,
    motion, palette,
    settings::Fill,
    ui::{self, RADIUS_LG, RADIUS_SM, RADIUS_XL, RADIUS_XS, SURFACE_INSET, Squircle},
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

// The entries of the card a submenu path opens; the top level when the path no longer fits.
fn level_at<'a>(top: &'a [Entry], path: &[usize]) -> &'a [Entry] {
    let mut level = top;
    for &i in path {
        match level.get(i) {
            Some(Entry::Item(item)) if !item.sub.is_empty() => level = &item.sub,
            _ => return top,
        }
    }
    level
}

// The next row the keyboard can land on, wrapping; disabled rows and non-rows are skipped.
fn step(entries: &[Entry], from: Option<usize>, down: bool) -> Option<usize> {
    let rows: Vec<usize> = entries.iter().enumerate().filter(|(_, e)| matches!(e, Entry::Item(item) if !item.disabled)).map(|(i, _)| i).collect();
    let at = from.and_then(|f| rows.iter().position(|&r| r == f));
    let n = rows.len();
    if n == 0 {
        return None;
    }
    let next = match (at, down) {
        (None, true) => 0,
        (None, false) => n - 1,
        (Some(at), true) => (at + 1) % n,
        (Some(at), false) => (at + n - 1) % n,
    };
    Some(rows[next])
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
    // The keyboard's row in the deepest open card; the pointer clears it.
    cursor: Option<usize>,
    focus: FocusHandle,
    // Whatever had the keyboard before the menu took it, to hand it back on close.
    restore: RefCell<Option<FocusHandle>>,
}

// A one-field dialog (nickname, note) or a confirmation (kick, ban, remove friend).
pub struct Dialog {
    title: SharedString,
    body: SharedString,
    confirm: SharedString,
    danger: bool,
    // Leads the title with the warning mark: for what can't be undone.
    warn: bool,
    input: bool,
    on_confirm: OnConfirm,
}

impl Rustcord {
    pub(crate) fn open_menu(&mut self, at: Point<Pixels>, entries: Vec<Entry>, cx: &mut Context<Self>) {
        self.open_live_menu(at, move |_| entries.clone(), cx);
    }

    // A menu built from state each time it's drawn, e.g. one whose checkboxes change.
    pub(crate) fn open_live_menu(&mut self, at: Point<Pixels>, build: impl Fn(&Rustcord) -> Vec<Entry> + 'static, cx: &mut Context<Self>) {
        self.menu = Some(OpenMenu { at, build: Rc::new(move |this| tidy(build(this))), sub: Vec::new(), cursor: None, focus: cx.focus_handle(), restore: RefCell::new(None) });
        cx.notify();
    }

    // Demo scenes open a submenu chain without hovering.
    #[cfg(debug_assertions)]
    pub(crate) fn open_submenus(&mut self, path: Vec<usize>) {
        if let Some(menu) = &mut self.menu {
            menu.sub = path;
        }
    }

    // Closes the menu and gives the keyboard back to what had it.
    fn shut_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = self.menu.take() else { return };
        if let Some(previous) = menu.restore.take() {
            window.focus(&previous, cx);
        }
        cx.notify();
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
        self.dialog = Some(Dialog { title: title.into(), body: body.into(), confirm: confirm.into(), danger: false, warn: false, input: true, on_confirm: Rc::new(on_confirm) });
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
            warn: false,
            input: false,
            on_confirm: Rc::new(move |this, _, window, cx| on_confirm(this, window, cx)),
        });
        cx.notify();
    }

    // A confirm for a deletion that can't be undone: the title leads with the warning mark.
    pub(crate) fn confirm_delete(
        &mut self,
        title: impl Into<SharedString>,
        body: impl Into<SharedString>,
        confirm: impl Into<SharedString>,
        on_confirm: impl Fn(&mut Rustcord, &mut Window, &mut Context<Rustcord>) + 'static,
        cx: &mut Context<Self>,
    ) {
        self.confirm(title, body, confirm, on_confirm, cx);
        if let Some(dialog) = &mut self.dialog {
            dialog.warn = true;
        }
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
            self.shut_menu(window, cx);
        }
        act(self, window, cx);
        cx.notify();
    }

    // Arrows move through the deepest open card, Right or Enter opens a submenu, Left closes it,
    // Enter or Space runs a row, Escape closes the menu.
    fn on_menu_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = &self.menu else { return };
        let top = (menu.build)(self);
        let (mut sub, cursor) = (menu.sub.clone(), menu.cursor);
        let level = level_at(&top, &sub);
        let has_sub = |i: usize| matches!(level.get(i), Some(Entry::Item(item)) if !item.sub.is_empty() && !item.disabled);
        let next_cursor = match (event.keystroke.key.as_str(), cursor) {
            ("down", _) => step(level, cursor, true),
            ("up", _) => step(level, cursor, false),
            ("right" | "enter" | "space", Some(i)) if has_sub(i) => {
                sub.push(i);
                step(level_at(&top, &sub), None, true)
            }
            ("left", _) if !sub.is_empty() => sub.pop(),
            ("enter" | "space", Some(i)) => {
                sub.push(i);
                cx.stop_propagation();
                return self.run_menu_item(&sub, window, cx);
            }
            ("escape", _) => {
                cx.stop_propagation();
                return self.shut_menu(window, cx);
            }
            _ => return,
        };
        if let Some(menu) = &mut self.menu {
            menu.sub = sub;
            menu.cursor = next_cursor;
        }
        cx.stop_propagation();
        cx.notify();
    }

    pub(crate) fn render_menu(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let menu = self.menu.as_ref()?;
        // The menu takes the keyboard while it's open.
        if !menu.focus.is_focused(window) {
            *menu.restore.borrow_mut() = window.focused(cx);
            window.focus(&menu.focus, cx);
        }
        let entries = (menu.build)(self);
        let viewport = window.viewport_size();
        let (vw, vh) = (f32::from(viewport.width), f32::from(viewport.height));
        // Opens toward the side with room, rising from the corner at the pointer (overlays.html's live menu).
        let (at_x, at_y, height) = (f32::from(menu.at.x), f32::from(menu.at.y), card_height(&entries));
        let (flip_x, flip_y) = (at_x + WIDTH > vw - 8., at_y + height > vh - 8.);
        let x = if flip_x { (at_x - WIDTH).max(8.) } else { at_x };
        let y = if flip_y { (at_y - height).max(8.) } else { at_y };
        // Keyed by where it opened, so a fresh right-click replays the rise.
        let open_key = format!("{at_x:.0}-{at_y:.0}");
        let top = self.menu_card(&entries, &[], menu.sub.first().copied(), menu.cursor.filter(|_| menu.sub.is_empty()), WIDTH, cx).left(px(x)).top(px(y)).max_h(px(vh - 16.)).overflow_y_scroll();
        let mut cards = vec![ui::rise(top.transform_origin(ui::corner(flip_x, flip_y)), SharedString::from(format!("menu-rise-{open_key}")), Duration::ZERO).into_any_element()];
        // One flyout per open level beside its parent row, its first row level with that row; it
        // flips to the left at the window's edge.
        let (mut level, mut card_x, mut card_y, mut card_w) = (&entries, x, y, WIDTH);
        for (depth, &i) in menu.sub.iter().enumerate() {
            let Some(Entry::Item(item)) = level.get(i) else { break };
            if item.sub.is_empty() {
                break;
            }
            let sub_w = if item.sub.iter().any(Entry::has_check) { SUB_WIDE } else { SUB_WIDTH };
            let flip = card_x + card_w + 4. + sub_w >= vw;
            card_x = if flip { card_x - sub_w - 4. } else { card_x + card_w + 4. };
            card_y = (card_y + offset(level, i)).min(vh - card_height(&item.sub) - 8.).max(8.);
            card_w = sub_w;
            let path = &menu.sub[..=depth];
            let cursor = menu.cursor.filter(|_| depth + 1 == menu.sub.len());
            let card = self.menu_card(&item.sub, path, menu.sub.get(depth + 1).copied(), cursor, sub_w, cx).left(px(card_x)).top(px(card_y));
            cards.push(ui::rise(card.transform_origin(ui::corner(flip, false)), SharedString::from(format!("menu-rise-{open_key}-{path:?}")), SUB_DELAY).into_any_element());
            level = &item.sub;
        }
        Some(
            div()
                .id("menu-backdrop")
                .absolute()
                .inset_0()
                .occlude()
                .track_focus(&menu.focus)
                .on_key_down(cx.listener(Self::on_menu_key))
                .on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, cx| this.shut_menu(window, cx)))
                .on_mouse_down(MouseButton::Right, cx.listener(|this, _, window, cx| this.shut_menu(window, cx)))
                .children(cards)
                .into_any_element(),
        )
    }

    fn menu_card(&self, entries: &[Entry], path: &[usize], open_sub: Option<usize>, cursor: Option<usize>, width: f32, cx: &mut Context<Self>) -> Stateful<Div> {
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
                    self.menu_row(item, item_path, open_sub == Some(i), cursor == Some(i), lead_slot, cx).into_any_element()
                }
            })
            .collect();
        ui::surface(cx).w(px(width)).id(SharedString::from(format!("menu-{path:?}"))).absolute().on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()).children(rows)
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
                    .font_weight(FontWeight::NORMAL)
                    .text_color(palette::text(cx))
                    .child(label)
                    .child(div().text_color(palette::muted(cx)).child(format!("{:.0}%", value * 200.))),
            )
            .child(self.range(key, value, Fill::Accent, move |this, f, cx| on_set(this, f.clamp(0., 1.), cx), cx))
    }

    fn menu_row(&self, item: &Item, path: Vec<usize>, sub_open: bool, keyboard: bool, lead_slot: bool, cx: &mut Context<Self>) -> ui::Control {
        let has_sub = !item.sub.is_empty();
        let run_path = path.clone();
        // Keyboard focus: the hover fill and a 2px accent ring inside the row.
        let ring = BoxShadow { color: palette::accent_ink(cx), offset: point(px(0.), px(0.)), blur_radius: px(0.), spread_radius: px(2.), inset: true };
        menu_item_row(SharedString::from(format!("menu-item-{path:?}")), item, sub_open || keyboard, lead_slot, cx)
            .when(keyboard, |row| row.shadow(vec![ring]))
            // Hovering a row opens its submenu and closes any deeper one from a sibling.
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                let Some(menu) = &mut this.menu else { return };
                if !*hovered {
                    return;
                }
                let next = if has_sub { path.clone() } else { path[..path.len() - 1].to_vec() };
                if menu.sub != next || menu.cursor.is_some() {
                    menu.sub = next;
                    menu.cursor = None;
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
            .squircle(RADIUS_XL)
            .bg(palette::surface(palette::Level::Raised, cx))
            .shadow(ui::shadow_pop(cx))
            .flex()
            .flex_col()
            .gap(px(16.))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .text_size(px(20.))
                    .line_height(px(24.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(palette::strong(cx))
                    .when(dialog.warn, |d| d.child(div().text_color(palette::danger_ink(cx)).child(Icon::new(Glyph::Warning).size(px(22.)))))
                    .child(dialog.title.clone()),
            )
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
        // Pops from the centre: scale 0.95 with its fade (CSS `pop`, 260ms ease-out).
        let card = card.with_animation("dialog-pop", Animation::new(ui::DUR_PANEL).with_easing(|t| motion::ease_out().sample(t)), |card, t| card.opacity(t).scale(0.95 + 0.05 * t));
        Some(
            div()
                .id("dialog-backdrop")
                .absolute()
                .inset_0()
                .occlude()
                .bg(palette::scrim(cx))
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

// One menu row built on `ui::menu_row`, styled by the item's tone, mark and lead; callers add `.on_click`
// (and hover for submenus).
pub(crate) fn menu_item_row(id: SharedString, item: &Item, sub_open: bool, lead_slot: bool, cx: &App) -> ui::Control {
    let has_sub = !item.sub.is_empty();
    let chosen = item.chosen();
    let row = match item.tone {
        Tone::Danger => ui::menu_row(id.clone(), cx).danger(cx),
        Tone::Accent => ui::menu_row(id.clone(), cx).accent(cx),
        Tone::Plain => ui::menu_row(id.clone(), cx).toggled(chosen, cx),
    };
    let lead = item.lead.as_ref().map(|lead| match lead {
        Lead::Glyph(glyph) => div().when_some(plain_icon(item, cx), |d, ink| d.text_color(ink)).child(Icon::new(*glyph).size(px(LEAD))).into_any_element(),
        Lead::Emoji(emoji) => emoji_face(emoji, LEAD).into_any_element(),
        // The 14px presence mark with no ring of its own; its cut-outs take the menu's colour.
        Lead::Status(status) => ui::status_mark(*status, 14., palette::surface(palette::Level::Overlay, cx), cx).border(px(0.)).into_any_element(),
        Lead::Dot(color) => div().size(px(12.)).rounded_full().bg(gpui_kit::rgb(*color)).into_any_element(),
        Lead::Server { icon, name } => server_icon(icon.as_deref(), name, 20., 6., cx),
    });
    let lead = lead.or_else(|| lead_slot.then(|| div().size(px(LEAD)).into_any_element()));
    let (hint, hint_hover) = hint_inks(item, cx);
    row.group(id.clone())
        .min_h(px(item.height()))
        .line_height(px(20.))
        .highlighted(sub_open)
        .disabled(item.disabled)
        .children(lead.map(|lead| div().size(px(LEAD)).flex_none().flex().items_center().justify_center().child(lead)))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(div().truncate().child(item.label.clone()))
                .children(item.hint.clone().map(|text| {
                    div()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .font_weight(FontWeight::NORMAL)
                        .text_color(if sub_open { hint_hover } else { hint })
                        .group_hover(id.clone(), move |s| s.text_color(hint_hover))
                        .child(text)
                })),
        )
        .when_some(item.mark, |d, mark| match mark {
            Mark::Check(on) => d.child(ui::check(if on { ui::Check::On } else { ui::Check::Off }, cx).size(px(18.))),
            Mark::Radio(_) => d,
        })
        .when(has_sub, |d| d.child(div().text_color(palette::faint(cx)).child(Icon::new(Glyph::ChevronRight).size(px(14.)))))
}

// A plain row's icon stays muted under the pointer (strong on the chosen radio row, faint when disabled);
// danger and accent icons follow the row's ink.
fn plain_icon(item: &Item, cx: &App) -> Option<Hsla> {
    if item.disabled {
        return Some(palette::faint(cx));
    }
    match item.tone {
        Tone::Plain if item.chosen() => Some(palette::strong(cx)),
        Tone::Plain => Some(palette::muted(cx)),
        Tone::Danger | Tone::Accent => None,
    }
}

// A row's sub-line at rest and under the pointer: faint, white on a danger row's fill, muted when disabled.
fn hint_inks(item: &Item, cx: &App) -> (Hsla, Hsla) {
    let (muted, faint) = (palette::muted(cx), palette::faint(cx));
    if item.disabled {
        return (muted, muted);
    }
    match item.tone {
        Tone::Danger => (ui::mix(palette::danger_ink(cx), muted, 0.2), gpui_kit::white().opacity(0.88)),
        Tone::Accent | Tone::Plain => (faint, faint),
    }
}

pub(crate) fn separator(cx: &App) -> Div {
    div().h(px(SEPARATOR)).flex_none().py(px(4.)).px(px(8.)).child(div().h(px(1.)).bg(palette::line(cx)))
}

fn menu_header(lead: &HeadLead, title: SharedString, subtitle: Option<SharedString>, cx: &App) -> Div {
    let overlay = palette::surface(palette::Level::Overlay, cx);
    let lead = match lead {
        HeadLead::Avatar { url, status } => crate::layout::avatar_with_status(url, 32., *status, overlay),
        HeadLead::Server { icon, name } => server_icon(icon.as_deref(), name, 32., RADIUS_XS, cx),
        HeadLead::Tile(glyph) => div().size(px(32.)).squircle(RADIUS_SM).bg(palette::selected(cx)).text_color(palette::strong(cx)).flex().items_center().justify_center().child(Icon::new(*glyph).size(px(18.))).into_any_element(),
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
                .child(div().truncate().text_size(px(14.)).line_height(px(18.)).font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(title))
                .children(subtitle.map(|s| div().truncate().text_size(px(12.)).line_height(px(16.)).text_color(palette::muted(cx)).child(s))),
        )
}

// A server's icon, or its initials on a tile until it has one.
pub(crate) fn server_icon(icon: Option<&str>, name: &str, size: f32, radius: f32, cx: &App) -> AnyElement {
    if let Some(url) = icon {
        return img(url.to_owned()).size(px(size)).squircle(radius).into_any_element();
    }
    div()
        .size(px(size))
        .squircle(radius)
        .bg(palette::selected(cx))
        .text_color(palette::strong(cx))
        .text_size(px((size * 0.38).round()))
        .font_weight(FontWeight::SEMIBOLD)
        .flex()
        .items_center()
        .justify_center()
        .child(crate::sidebar::initials(name))
        .into_any_element()
}

fn quick_reactions(quick: &[(Emoji, Act)], more: Act, cx: &mut Context<Rustcord>) -> Div {
    let (fill, press, muted, strong) = (palette::selected(cx), palette::press(cx), palette::muted(cx), palette::strong(cx));
    let cell = |id: SharedString, ink: Hsla| {
        ui::pressable(div().id(id), 0.97)
            .flex_1()
            .h(px(38.))
            .squircle(RADIUS_SM)
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
        cell(SharedString::from(format!("menu-emoji-{j}")), strong).child(emoji_face(emoji, 20.)).on_click(cx.listener(move |this, _, window, cx| {
            this.shut_menu(window, cx);
            act(this, window, cx);
        }))
    });
    let picker = cell("menu-emoji-more".into(), strong)
        .child(Icon::new(Glyph::Emoji).size(px(18.)))
        .on_click(cx.listener(move |this, _, window, cx| {
            this.shut_menu(window, cx);
            more(this, window, cx);
        }));
    div().h(px(EMOJI_ROW)).flex_none().pt(px(2.)).px(px(2.)).pb(px(4.)).flex().gap(px(4.)).children(faces).child(picker)
}

fn emoji_face(emoji: &Emoji, size: f32) -> AnyElement {
    match emoji.url() {
        Some(url) => img(url).size(px(size)).into_any_element(),
        None => div().text_size(px(size)).line_height(px(size)).child(emoji.name.clone()).into_any_element(),
    }
}

// The field with its focus ring, which it holds while its dialog is open: the dialog focuses it on open.
fn field(cx: &App) -> Div {
    let ring = BoxShadow { color: palette::accent_soft(cx), offset: point(px(0.), px(0.)), blur_radius: px(0.), spread_radius: px(3.), inset: false };
    ui::field_frame(cx).border_color(palette::accent()).shadow(vec![ring])
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
            .squircle(RADIUS_LG)
            .bg(palette::surface(palette::Level::Overlay, cx))
            .shadow(ui::shadow_pop(cx))
            .flex()
            .items_center()
            .text_size(px(14.))
            .line_height(px(20.))
            .font_weight(FontWeight::NORMAL)
            .text_color(palette::strong(cx))
            .child(text.clone());
        // Rises 8px into place from scale 0.97 with its fade (CSS `toast-in`, 240ms).
        // GAP: and from blur(4px); gpui has no filter blur on an element's own painting.
        let toast = toast.with_animation(SharedString::from(format!("toast-{text}")), Animation::new(Duration::from_millis(240)).with_easing(|t| motion::ease_out().sample(t)), |toast, t| {
            toast.translate_y(px(8. * (1. - t))).scale(0.97 + 0.03 * t).opacity(t)
        });
        Some(div().absolute().left_0().right_0().bottom(px(24.)).flex().justify_center().child(toast).into_any_element())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyboard_steps_over_separators_and_disabled_rows_and_wraps() {
        let noop = |_: &mut Rustcord, _: &mut Window, _: &mut Context<Rustcord>| {};
        let entries = vec![item("a", noop).entry(), Entry::Separator, item("b", noop).disabled().entry(), item("c", noop).entry()];
        assert_eq!(step(&entries, None, true), Some(0));
        assert_eq!(step(&entries, Some(0), true), Some(3));
        assert_eq!(step(&entries, Some(3), true), Some(0));
        assert_eq!(step(&entries, None, false), Some(3));
        assert_eq!(step(&[Entry::Separator], None, true), None);
    }

    #[test]
    fn tidy_drops_stray_separators() {
        let noop = |_: &mut Rustcord, _: &mut Window, _: &mut Context<Rustcord>| {};
        let entries = vec![Entry::Separator, item("a", noop).entry(), Entry::Separator, Entry::Separator, item("b", noop).entry(), Entry::Separator];
        let kinds: Vec<bool> = tidy(entries).iter().map(|e| matches!(e, Entry::Separator)).collect();
        assert_eq!(kinds, [false, true, false]);
    }
}
