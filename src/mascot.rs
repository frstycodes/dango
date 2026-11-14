use std::{f32::consts::TAU, time::Duration};

use gpui_kit::{Animation, AnimationExt, AnyElement, App, ElementId, div, img, prelude::*, px};

use crate::icons::CLYDE;

// Discord's Clyde, drawn from the official file as-is: the art is never recoloured or reshaped,
// only moved, so the moods live in how it bobs.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mood {
    // Breathes gently.
    Idle,
    // Sways side to side: waiting on something outside the app (the phone, the network).
    Waiting,
    // Hops.
    Happy,
}

// The symbol's own aspect ratio (126.644 x 96).
const WIDTH: f32 = 64.;
const HEIGHT: f32 = WIDTH * 96. / 126.644;

pub fn clyde(id: impl Into<ElementId>, mood: Mood, cx: &App) -> AnyElement {
    let mark = div().relative().w(px(WIDTH)).h(px(HEIGHT)).flex_none().child(img(CLYDE).size_full());
    if cx.reduce_motion() {
        return mark.into_any_element();
    }
    let period = match mood {
        Mood::Idle => 2.6,
        Mood::Waiting => 2.2,
        Mood::Happy => 0.7,
    };
    mark.with_animation(id, Animation::new(Duration::from_secs_f32(period)).repeat(), move |el, t| match mood {
        Mood::Idle => el.top(px(-2. * (t * TAU).sin())),
        Mood::Waiting => el.left(px(3. * (t * TAU).sin())).top(px(-1.5 * (t * 2. * TAU).sin().abs())),
        // A hop: quick rise, gravity back down, a beat on the floor.
        Mood::Happy => {
            let p = (t * 1.25).min(1.);
            el.top(px(-28. * p * (1. - p)))
        }
    })
    .into_any_element()
}
