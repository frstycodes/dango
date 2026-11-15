use std::{f32::consts::TAU, time::Duration};

use gpui_kit::{Animation, AnimationExt, AnyElement, App, ElementId, div, prelude::*, px};

use crate::palette;

// Wave's mascot is its own logo come alive: the five bars of the voice wave, moving with the mood.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mood {
    // Breathes gently.
    Idle,
    // A slow ripple runs across the bars: waiting on something outside the app.
    Waiting,
    // Bounces like speech.
    Happy,
}

// The logo's bar heights, scaled to a 64px-wide mascot.
const BARS: [f32; 5] = [24., 45., 64., 39., 22.];
const BAR_WIDTH: f32 = 8.;
const GAP: f32 = 5.;
const HEIGHT: f32 = 66.;

// The mascot drawn at `scale` (1 is 64px wide), for tighter places such as the login card.
pub fn mascot_at(id: impl Into<ElementId>, mood: Mood, scale: f32, cx: &App) -> AnyElement {
    let frame = move || div().h(px(HEIGHT * scale)).flex().items_center().gap(px(GAP * scale)).flex_none();
    let bars = move |heights: [f32; 5]| heights.map(|h| div().w(px(BAR_WIDTH * scale)).h(px(h * scale)).flex_none().rounded_full().bg(palette::accent()));
    if cx.reduce_motion() {
        return frame().children(bars(BARS)).into_any_element();
    }
    let period = match mood {
        Mood::Idle => 2.6,
        Mood::Waiting => 2.2,
        Mood::Happy => 1.8,
    };
    frame()
        .with_animation(id, Animation::new(Duration::from_secs_f32(period)).repeat(), move |el, t| el.children(bars(heights(mood, t))))
        .into_any_element()
}

// Each bar's height at time `t` (0..1 through one period).
fn heights(mood: Mood, t: f32) -> [f32; 5] {
    let mut out = BARS;
    for (i, h) in out.iter_mut().enumerate() {
        let phase = i as f32;
        let scale = match mood {
            Mood::Idle => 0.88 + 0.12 * (t * TAU + phase * 0.4).sin(),
            Mood::Waiting => 0.7 + 0.3 * (t * TAU - phase * 0.9).sin(),
            Mood::Happy => 0.45 + 0.55 * (t * TAU + phase * 1.3).sin().abs(),
        };
        *h = (*h * scale).max(BAR_WIDTH);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bars_stay_round_and_within_their_height() {
        for mood in [Mood::Idle, Mood::Waiting, Mood::Happy] {
            for step in 0..50 {
                for (h, base) in heights(mood, step as f32 / 50.).iter().zip(BARS) {
                    assert!(*h >= BAR_WIDTH && *h <= base, "{h} for a {base} bar");
                }
            }
        }
    }
}
