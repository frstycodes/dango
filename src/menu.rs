// Discord's context menus, measured from Discord web (2026-11-14): Midnight popover, 8px radius,
// 20% wash border, 8px padding, 34px rows of 14/18 medium text, checkboxes on the right, red
// destructive rows with a red-tinted hover, and submenus that fly out on hover. Every right-click
// in Rustcord opens one of these, so the options can match Discord's exactly.
use std::rc::Rc;

use gpui_kit::{
    AnyElement, Context, Div, FontWeight, Hsla, MouseButton, Pixels, Point, SharedString, Window,
    component::{ActiveTheme, Icon, input::Input},
    div, prelude::*, px, rgb,
};

use crate::{Rustcord, icons::Glyph, palette, settings::Fill};

pub type Act = Rc<dyn Fn(&mut Rustcord, &mut Window, &mut Context<Rustcord>)>;
type SetValue = Rc<dyn Fn(&mut Rustcord, f32, &mut Context<Rustcord>)>;

const WIDTH: f32 = 220.;
const ROW: f32 = 34.;
const CHECK_ROW: f32 = 36.;
const HINT_ROW: f32 = 52.;
const SEPARATOR: f32 = 9.;
const SLIDER_ROW: f32 = 60.;
const EMOJI_ROW: f32 = 44.;
const PADDING: f32 = 8.;

pub enum Entry {
    Item(Item),
    Separator,
    // Discord's quick-react row at the top of the message menu: emoji buttons in one line.
    Emojis(Vec<(crate::discord::Emoji, Act)>),
    Slider { label: SharedString, key: SharedString, value: f32, on_set: SetValue },
}

pub struct Item {
    label: SharedString,
    hint: Option<SharedString>,
    danger: bool,
    check: Option<bool>,
    icon: Option<Glyph>,
    sub: Vec<Entry>,
    act: Option<Act>,
    disabled: bool,
}

pub fn item(label: impl Into<SharedString>, act: impl Fn(&mut Rustcord, &mut Window, &mut Context<Rustcord>) + 'static) -> Item {
    Item { label: label.into(), hint: None, danger: false, check: None, icon: None, sub: Vec::new(), act: Some(Rc::new(act)), disabled: false }
}

pub fn submenu(label: impl Into<SharedString>, entries: Vec<Entry>) -> Item {
    Item { label: label.into(), hint: None, danger: false, check: None, icon: None, sub: entries, act: None, disabled: false }
}

pub fn slider(label: impl Into<SharedString>, key: impl Into<SharedString>, value: f32, on_set: impl Fn(&mut Rustcord, f32, &mut Context<Rustcord>) + 'static) -> Entry {
    Entry::Slider { label: label.into(), key: key.into(), value, on_set: Rc::new(on_set) }
}

impl Item {
    pub fn hint(mut self, hint: impl Into<SharedString>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }

    pub fn checked(mut self, on: bool) -> Self {
        self.check = Some(on);
        self
    }

    pub fn disabled(mut self) -> Self {
        self.disabled = true;
        self
    }

    pub fn icon(mut self, glyph: Glyph) -> Self {
        self.icon = Some(glyph);
        self
    }

    pub fn entry(self) -> Entry {
        Entry::Item(self)
    }

    fn height(&self) -> f32 {
        match (&self.hint, self.check) {
            (Some(_), _) => HINT_ROW,
            (None, Some(_)) => CHECK_ROW,
            _ => ROW,
        }
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
            Entry::Slider { .. } => SLIDER_ROW,
            Entry::Emojis(_) => EMOJI_ROW,
        }
    }
}

// Drops separators that would sit at the top, bottom or next to another, so menus can be built
// from optional groups without stray lines.
pub fn tidy(entries: Vec<Entry>) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    for entry in entries {
        let after_separator = matches!(out.last(), None | Some(Entry::Separator));
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

pub struct OpenMenu {
    at: Point<Pixels>,
    entries: Vec<Entry>,
    sub: Option<usize>,
}

// A one-field dialog (nickname, note) or a confirmation (kick, ban, remove friend).
pub struct Dialog {
    title: SharedString,
    body: SharedString,
    confirm: SharedString,
    danger: bool,
    input: bool,
    on_confirm: Rc<dyn Fn(&mut Rustcord, String, &mut Window, &mut Context<Rustcord>)>,
}

impl Rustcord {
    pub(crate) fn open_menu(&mut self, at: Point<Pixels>, entries: Vec<Entry>, cx: &mut Context<Self>) {
        self.menu = Some(OpenMenu { at, entries: tidy(entries), sub: None });
        cx.notify();
    }

    pub(crate) fn close_menu(&mut self) -> bool {
        self.menu.take().is_some() | self.dialog.take().is_some()
    }

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
        let mut entries = &menu.entries;
        let mut found: Option<&Item> = None;
        for &i in path {
            let Some(Entry::Item(item)) = entries.get(i) else { return };
            found = Some(item);
            entries = &item.sub;
        }
        let Some(item) = found else { return };
        let Some(act) = item.act.clone().filter(|_| !item.disabled) else { return };
        // Checkboxes stay open so several can be flipped, as in Discord.
        let keep_open = item.check.is_some();
        if !keep_open {
            self.menu = None;
        }
        act(self, window, cx);
        cx.notify();
    }

    pub(crate) fn render_menu(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let menu = self.menu.as_ref()?;
        let viewport = window.viewport_size();
        let height = menu.entries.iter().map(Entry::height).sum::<f32>() + PADDING * 2. + 2.;
        let x = f32::from(menu.at.x).min(f32::from(viewport.width) - WIDTH - 8.).max(8.);
        let y = f32::from(menu.at.y).min(f32::from(viewport.height) - height - 8.).max(8.);
        let max_h = f32::from(viewport.height) - 16.;
        let card = self.menu_card(&menu.entries, &[], menu.sub, cx).left(px(x)).top(px(y)).max_h(px(max_h)).overflow_y_scroll();
        let flyout = menu.sub.and_then(|i| match menu.entries.get(i) {
            Some(Entry::Item(item)) if !item.sub.is_empty() => {
                let offset: f32 = menu.entries[..i].iter().map(Entry::height).sum::<f32>() + PADDING;
                let sub_height = item.sub.iter().map(Entry::height).sum::<f32>() + PADDING * 2. + 2.;
                let left = if x + WIDTH * 2. + 4. < f32::from(viewport.width) { x + WIDTH + 4. } else { x - WIDTH - 4. };
                let top = (y + offset - PADDING).min(f32::from(viewport.height) - sub_height - 8.).max(8.);
                Some(self.menu_card(&item.sub, &[i], None, cx).left(px(left)).top(px(top)))
            }
            _ => None,
        });
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
                .child(card)
                .children(flyout)
                .into_any_element(),
        )
    }

    fn menu_card(&self, entries: &[Entry], path: &[usize], open_sub: Option<usize>, cx: &mut Context<Self>) -> gpui_kit::Stateful<Div> {
        let theme = cx.theme().clone();
        let rows: Vec<AnyElement> = entries
            .iter()
            .enumerate()
            .map(|(i, entry)| match entry {
                Entry::Separator => div().h(px(SEPARATOR)).flex().items_end().child(div().mx_2().w_full().h(px(1.)).bg(wash(0.2))).pb(px(4.)).into_any_element(),
                Entry::Slider { label, key, value, on_set } => {
                    let on_set = on_set.clone();
                    div()
                        .h(px(SLIDER_ROW))
                        .px_2()
                        .flex()
                        .flex_col()
                        .justify_center()
                        .gap_1()
                        .child(div().flex().justify_between().text_size(px(14.)).font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(label.clone()).child(format!("{:.0}%", value * 200.)))
                        .child(self.range(key.clone(), *value, Fill::Accent, move |this, f, cx| on_set(this, f.clamp(0., 1.), cx), cx))
                        .into_any_element()
                }
                Entry::Emojis(emojis) => div()
                    .h(px(EMOJI_ROW))
                    .flex()
                    .items_center()
                    .justify_between()
                    .children(emojis.iter().enumerate().map(|(j, (emoji, act))| {
                        let act = act.clone();
                        let face: AnyElement = match emoji.url() {
                            Some(url) => gpui_kit::img(url).size(px(24.)).into_any_element(),
                            None => div().text_size(px(22.)).child(emoji.name.clone()).into_any_element(),
                        };
                        div()
                            .id(SharedString::from(format!("menu-emoji-{j}")))
                            .size(px(40.))
                            .rounded(px(8.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .bg(wash(0.08))
                            .hover(|s| s.bg(wash(0.16)))
                            .child(face)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.menu = None;
                                act(this, window, cx);
                                cx.notify();
                            }))
                    }))
                    .into_any_element(),
                Entry::Item(item) => {
                    let mut item_path = path.to_vec();
                    item_path.push(i);
                    self.menu_row(item, item_path, open_sub == Some(i), &theme, cx).into_any_element()
                }
            })
            .collect();
        div()
            .id(SharedString::from(format!("menu-{path:?}")))
            .absolute()
            .w(px(WIDTH))
            .p(px(PADDING))
            .rounded(px(8.))
            .border_1()
            .border_color(wash(0.2))
            .bg(theme.popover)
            .shadow_lg()
            .flex()
            .flex_col()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .children(rows)
    }

    fn menu_row(&self, item: &Item, path: Vec<usize>, sub_open: bool, theme: &gpui_kit::component::Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let danger: Hsla = rgb(0xEB5F5E).into();
        let ink = if item.danger { danger } else { palette::strong(cx) };
        let hover_bg = if item.danger { danger.opacity(0.08) } else { wash(0.12) };
        let top_level = path.len() == 1;
        let index = path[0];
        let has_sub = !item.sub.is_empty();
        let checkbox = item.check.map(|on| {
            div()
                .size(px(20.))
                .flex_none()
                .rounded(px(4.))
                .border_1()
                .border_color(if on { wash(0.2) } else { wash(0.64) })
                .bg(if on { palette::blurple() } else { gpui_kit::black().opacity(0.08) })
                .text_color(gpui_kit::white())
                .flex()
                .items_center()
                .justify_center()
                .children(on.then(|| Icon::new(Glyph::Checkmark).size(px(14.))))
        });
        let run_path = path.clone();
        div()
            .id(SharedString::from(format!("menu-item-{path:?}")))
            .min_h(px(item.height()))
            .px_2()
            .rounded(px(2.))
            .flex()
            .items_center()
            .gap_2()
            .when(!item.disabled, |d| d.cursor_pointer())
            .when(item.disabled, |d| d.opacity(0.5))
            .text_size(px(14.))
            .line_height(px(18.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(ink)
            .when(sub_open, |d| d.bg(hover_bg))
            .when(!item.disabled, |d| d.hover(move |s| s.bg(hover_bg)))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(div().truncate().child(item.label.clone()))
                    .children(item.hint.clone().map(|hint| div().text_size(px(12.)).font_weight(FontWeight::NORMAL).text_color(theme.muted_foreground).child(hint))),
            )
            .children(checkbox)
            .children(item.icon.map(|glyph| Icon::new(glyph).size(px(18.))))
            .when(has_sub, |d| d.child(Icon::new(Glyph::ChevronRight).size(px(16.))))
            .when(top_level, |d| {
                d.on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                    let Some(menu) = &mut this.menu else { return };
                    let next = match (*hovered, has_sub) {
                        (true, true) => Some(index),
                        (true, false) => None,
                        (false, _) => return,
                    };
                    if menu.sub != next {
                        menu.sub = next;
                        cx.notify();
                    }
                }))
            })
            .on_click(cx.listener(move |this, _, window, cx| this.run_menu_item(&run_path, window, cx)))
    }

    pub(crate) fn render_dialog(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let dialog = self.dialog.as_ref()?;
        let theme = cx.theme().clone();
        let fill: Hsla = if dialog.danger { rgb(0xDA3E44).into() } else { palette::blurple() };
        let submit = cx.listener(|this, _, window, cx| {
            let Some(dialog) = this.dialog.take() else { return };
            let value = this.dialog_input.read(cx).value().to_string();
            (dialog.on_confirm)(this, value, window, cx);
            cx.notify();
        });
        let card = div()
            .id("dialog")
            .w(px(440.))
            .rounded(px(12.))
            .border_1()
            .border_color(wash(0.2))
            .bg(theme.popover)
            .shadow_lg()
            .flex()
            .flex_col()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .p_4()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(div().text_size(px(20.)).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(dialog.title.clone()))
                    .child(div().text_size(px(16.)).text_color(theme.foreground).child(dialog.body.clone()))
                    .when(dialog.input, |d| {
                        d.child(div().h(px(40.)).px_3().rounded(px(8.)).border_1().border_color(wash(0.24)).bg(gpui_kit::black().opacity(0.12)).flex().items_center().child(Input::new(&self.dialog_input).appearance(false)))
                    }),
            )
            .child(
                div()
                    .p_4()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        div()
                            .id("dialog-cancel")
                            .h(px(38.))
                            .px_4()
                            .rounded(px(8.))
                            .flex()
                            .items_center()
                            .cursor_pointer()
                            .font_weight(FontWeight::MEDIUM)
                            .hover(|s| s.underline())
                            .child("Cancel")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.dialog = None;
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .id("dialog-confirm")
                            .h(px(38.))
                            .px_4()
                            .rounded(px(8.))
                            .flex()
                            .items_center()
                            .cursor_pointer()
                            .bg(fill)
                            .text_color(gpui_kit::white())
                            .font_weight(FontWeight::MEDIUM)
                            .hover(|s| s.opacity(0.9))
                            .child(dialog.confirm.clone())
                            .on_click(submit),
                    ),
            );
        Some(
            div()
                .id("dialog-backdrop")
                .absolute()
                .inset_0()
                .occlude()
                .bg(gpui_kit::black().opacity(0.7))
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
        let theme = cx.theme().clone();
        Some(
            div()
                .absolute()
                .bottom(px(24.))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .child(div().max_w(px(480.)).px_4().py_2().rounded(px(8.)).bg(theme.popover).border_1().border_color(wash(0.2)).shadow_lg().text_size(px(14.)).text_color(theme.foreground).child(text))
                .into_any_element(),
        )
    }
}

fn wash(alpha: f32) -> Hsla {
    Hsla::from(rgb(0x97979F)).opacity(alpha)
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
