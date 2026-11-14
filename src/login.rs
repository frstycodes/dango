use std::{rc::Rc, time::Duration};

use gpui_kit::{
    Animation, AnimationExt, AnyElement, Context, ElementId, FontWeight, SharedString, Window,
    component::{ActiveTheme, button::{Button, ButtonVariants}},
    canvas, div, fill, img, point, prelude::*, px, size,
};
use qrcode::QrCode;

use crate::{
    Rustcord,
    discord::User,
    mascot::{Mood, clyde},
    motion,
};

const MODULE_PX: f32 = 5.;

#[derive(Clone)]
pub enum Login {
    Preparing,
    Qr(Rc<Modules>),
    Scanned(User),
    Failed(SharedString),
}

pub struct Modules {
    width: usize,
    dark: Vec<bool>,
}

impl Login {
    pub fn qr(url: &str) -> Self {
        match QrCode::new(url.as_bytes()) {
            Ok(code) => Login::Qr(Rc::new(Modules {
                width: code.width(),
                dark: code.to_colors().into_iter().map(|c| c == qrcode::Color::Dark).collect(),
            })),
            Err(error) => Login::Failed(error.to_string().into()),
        }
    }
}

pub fn render(login: &Login, _: &mut Window, cx: &mut Context<Rustcord>) -> AnyElement {
    let theme = cx.theme().clone();
    let (phase, mood, title, detail): (&str, Mood, SharedString, SharedString) = match login {
        Login::Preparing => ("preparing", Mood::Idle, "Sign in with your phone".into(), "Getting a fresh code from Discord…".into()),
        Login::Qr(_) => (
            "qr",
            Mood::Waiting,
            "Sign in with your phone".into(),
            "In the Discord app on your phone, open Settings, choose Scan QR Code, and point the camera here.".into(),
        ),
        Login::Scanned(user) => ("scanned", Mood::Happy, format!("Hi, {}!", user.name).into(), "Approve the sign-in on your phone to finish.".into()),
        Login::Failed(error) => ("failed", Mood::Idle, "Sign-in didn't finish".into(), error.clone()),
    };
    let card = match login {
        Login::Preparing => div().size(px(232.)).rounded(px(24.)).bg(theme.muted).into_any_element(),
        Login::Qr(modules) => qr_card(modules.clone()),
        Login::Scanned(user) => avatar_pop(user),
        Login::Failed(_) => Button::new("retry")
            .primary()
            .label("Try again")
            .on_click(cx.listener(|this, _, _, cx| this.sign_in(cx)))
            .into_any_element(),
    };
    let card = pop_in(ElementId::from(SharedString::from(format!("card-{phase}"))), card);
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_4()
        .px_6()
        .window_control_area(gpui_kit::WindowControlArea::Drag)
        .child(clyde("login", mood, cx))
        .child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap_1p5()
                .max_w(px(380.))
                .child(div().text_size(px(22.)).font_weight(FontWeight::BOLD).text_center().child(title))
                .child(div().text_center().text_color(theme.muted_foreground).child(detail)),
        )
        .child(div().mt_2().child(card))
        .child(div().mt_2().text_size(px(12.)).text_color(theme.muted_foreground).child("Rustcord never sees your password."))
        .into_any_element()
}

// QR codes need dark modules on a light field to scan, so the card stays white in both themes.
fn qr_card(modules: Rc<Modules>) -> AnyElement {
    let side = modules.width as f32 * MODULE_PX;
    div()
        .p_4()
        .rounded(px(24.))
        .bg(gpui_kit::white())
        .shadow_lg()
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    let ink = gpui_kit::hsla(235. / 360., 0.4, 0.12, 1.);
                    for (i, _) in modules.dark.iter().enumerate().filter(|(_, dark)| **dark) {
                        let (x, y) = ((i % modules.width) as f32, (i / modules.width) as f32);
                        let origin = point(bounds.origin.x + px(x * MODULE_PX), bounds.origin.y + px(y * MODULE_PX));
                        window.paint_quad(fill(gpui_kit::Bounds::new(origin, size(px(MODULE_PX), px(MODULE_PX))), ink));
                    }
                },
            )
            .size(px(side)),
        )
        .into_any_element()
}

fn avatar_pop(user: &User) -> AnyElement {
    img(user.avatar.clone())
        .size(px(96.))
        .rounded_full()
        .with_animation("avatar-pop", Animation::new(Duration::from_millis(520)).with_easing(motion::back_out), |avatar, t| {
            avatar.size(px(96. * (0.5 + 0.5 * t)))
        })
        .into_any_element()
}

// Each phase's card springs up into place, so every step of sign-in feels like it arrived.
fn pop_in(id: ElementId, card: AnyElement) -> AnyElement {
    div()
        .relative()
        .child(card)
        .with_animation(id, Animation::new(Duration::from_millis(480)).with_easing(motion::back_out), |el, t| {
            el.top(px(16. * (1. - t))).opacity(t.clamp(0., 1.))
        })
        .into_any_element()
}
