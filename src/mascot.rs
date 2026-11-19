use std::{f32::consts::TAU, time::Duration};

use gpui_kit::{Animation, AnimationExt, AnyElement, App, ElementId, Hsla, Radians, div, prelude::*, px, rgb};

use crate::palette;

// The mascot: three dumplings on a skewer, moving with the mood.
// GAP: still the previous identity's dumplings; to be redrawn from the waffle mark (assets/brand/waffle-mark.svg).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mood {
    // Breathes gently.
    Idle,
    // Each dumpling swells in turn, like typing dots: waiting on something outside the app.
    Waiting,
    // Hops along the skewer.
    Happy,
}

// Dumplings in a 100-unit box, drawn 64px wide at scale 1. Spaced just apart rather than overlapping
// like the mark (assets/brand/waffle-mark.svg): a moving mascot can't cut gaps into whatever is behind it.
const CENTERS: [(f32, f32); 3] = [(28., 72.), (50., 50.), (72., 28.)];
const RADIUS: f32 = 14.5;
const STICK: (f32, f32) = (113., 6.);
const STICK_COLOR: u32 = 0xC79A6B;
const SIZE: f32 = 64.;
// How far a happy dumpling hops, in units.
const HOP: f32 = 6.;

// One dumpling's pose: how far it's lifted (units) and how much it's scaled.
#[derive(Clone, Copy, Debug)]
struct Pose {
    lift: f32,
    scale: f32,
}

// The mascot drawn at `scale` (1 is 64px wide), for tighter places such as the login card.
pub fn mascot_at(id: impl Into<ElementId>, mood: Mood, scale: f32, cx: &App) -> AnyElement {
    let unit = SIZE * scale / 100.;
    let frame = move || div().relative().size(px(SIZE * scale)).flex_none().child(skewer(unit, STICK.0).bg(rgb(STICK_COLOR)));
    let rest = [Pose { lift: 0., scale: 1. }; 3];
    if cx.reduce_motion() {
        return frame().children(dumplings(rest, unit)).into_any_element();
    }
    let period = match mood {
        Mood::Idle => 2.6,
        Mood::Waiting => 1.4,
        Mood::Happy => 1.2,
    };
    frame()
        .with_animation(id, Animation::new(Duration::from_secs_f32(period)).repeat(), move |el, t| el.children(dumplings(poses(mood, t), unit)))
        .into_any_element()
}

// The skewer, placed and turned in a box of `unit`-sized units; the caller colours it.
pub fn skewer(unit: f32, length: f32) -> gpui_kit::Div {
    let width = STICK.1;
    div()
        .absolute()
        .left(px((50. - length / 2.) * unit))
        .top(px((50. - width / 2.) * unit))
        .w(px(length * unit))
        .h(px(width * unit))
        .rounded_full()
        .rotate(Radians(-45f32.to_radians()))
}

fn dumplings(poses: [Pose; 3], unit: f32) -> [gpui_kit::Div; 3] {
    let mut i = 0;
    poses.map(|pose| {
        let (x, y) = CENTERS[i];
        i += 1;
        let r = RADIUS * pose.scale;
        let shine = div().absolute().left(px(r * 0.38 * unit)).top(px(r * 0.42 * unit)).w(px(r * 0.6 * unit)).h(px(r * 0.4 * unit)).rounded_full().bg(gpui_kit::white().opacity(0.5));
        div()
            .absolute()
            .left(px((x - r) * unit))
            .top(px((y - r - pose.lift) * unit))
            .size(px(r * 2. * unit))
            .rounded_full()
            .bg(palette::logo())
            .child(shine)
    })
}

// Each dumpling's pose at time `t` (0..1 through one period).
fn poses(mood: Mood, t: f32) -> [Pose; 3] {
    let mut out = [Pose { lift: 0., scale: 1. }; 3];
    for (i, pose) in out.iter_mut().enumerate() {
        let phase = i as f32;
        *pose = match mood {
            Mood::Idle => Pose { lift: 0., scale: 0.95 + 0.05 * (t * TAU + phase * 0.7).sin() },
            Mood::Waiting => Pose { lift: 0., scale: 0.86 + 0.14 * (t * TAU - phase * TAU / 3.).sin().max(0.) },
            Mood::Happy => Pose { lift: HOP * (t * TAU - phase * 0.9).sin().max(0.), scale: 1. },
        };
    }
    out
}

// The loader: three overlapping dumplings, each cut
// from the one behind by a ring of `ring` (the backdrop's colour). They step down the skewer: the
// bottom one slides off the end and shrinks away while a new one grows in at the top.
pub fn loader(id: impl Into<ElementId>, size: f32, colors: LoaderColors, cx: &App) -> AnyElement {
    let unit = size / 100.;
    let frame = move || div().relative().size(px(size)).flex_none().child(skewer(unit, LOADER_STICK).bg(colors.stick));
    if cx.reduce_motion() {
        return frame().children(loader_dumplings(0., unit, colors)).into_any_element();
    }
    frame()
        .with_animation(id, Animation::new(LOADER_STEP).repeat(), move |el, t| el.children(loader_dumplings(step_progress(t), unit, colors)))
        .into_any_element()
}

#[derive(Clone, Copy)]
pub struct LoaderColors {
    pub ink: Hsla,
    pub ring: Hsla,
    pub stick: Hsla,
}

// One step down the skewer: a quick slide, then a rest.
const LOADER_STEP: Duration = Duration::from_millis(900);
const LOADER_SLIDE: f32 = 0.45;
// The top slot, and the move from one slot to the next, in units.
const LOADER_TOP: (f32, f32) = (64., 36.);
const LOADER_PITCH: (f32, f32) = (-14., 14.);
const LOADER_RADIUS: f32 = 15.;
const LOADER_RING: f32 = 2.5;
const LOADER_STICK: f32 = 85.;

// How far through the slide we are at `t` (0..1 through one step): eased in and out, then held.
fn step_progress(t: f32) -> f32 {
    let x = (t / LOADER_SLIDE).min(1.);
    if x < 0.5 { 4. * x * x * x } else { 1. - (-2. * x + 2.).powi(3) / 2. }
}

// Where each dumpling sits (in slots: 0 top, 2 bottom) and how big it is, `moved` of the way through a step.
// Four are in play: one growing in above the top slot, three on the skewer, the last one leaving.
fn loader_slots(moved: f32) -> [(f32, f32); 4] {
    [2., 1., 0., -1.].map(|slot: f32| {
        let at = slot + moved;
        let scale = if at < 0. { 1. + at } else if at > 2. { 3. - at } else { 1. };
        (at, scale.clamp(0., 1.))
    })
}

fn loader_dumplings(moved: f32, unit: f32, colors: LoaderColors) -> Vec<gpui_kit::Div> {
    // Bottom first, so each one above cuts its ring into the one below.
    loader_slots(moved)
        .into_iter()
        .filter(|(_, scale)| *scale > 0.)
        .flat_map(|(at, scale)| {
            let (x, y) = (LOADER_TOP.0 + LOADER_PITCH.0 * at, LOADER_TOP.1 + LOADER_PITCH.1 * at);
            let r = LOADER_RADIUS * scale;
            let disc = move |radius: f32, color: Hsla| div().absolute().left(px((x - radius) * unit)).top(px((y - radius) * unit)).size(px(radius * 2. * unit)).rounded_full().bg(color);
            let shine = div().absolute().left(px(r * 0.38 * unit)).top(px(r * 0.42 * unit)).w(px(r * 0.6 * unit)).h(px(r * 0.4 * unit)).rounded_full().bg(gpui_kit::white().opacity(0.5));
            [disc(r + LOADER_RING * scale, colors.ring), disc(r, colors.ink).child(shine)]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dumplings_stay_inside_the_box_and_never_grow() {
        for mood in [Mood::Idle, Mood::Waiting, Mood::Happy] {
            for step in 0..50 {
                for (pose, (x, y)) in poses(mood, step as f32 / 50.).iter().zip(CENTERS) {
                    let r = RADIUS * pose.scale;
                    assert!(pose.scale <= 1. && pose.lift >= 0. && pose.lift <= HOP);
                    assert!(x - r >= 0. && x + r <= 100. && y - r - pose.lift >= 0. && y + r <= 100.);
                }
            }
        }
    }

    #[test]
    fn the_loader_steps_one_slot_and_lines_up_with_the_next_step() {
        assert_eq!(step_progress(0.), 0.);
        assert_eq!(step_progress(LOADER_SLIDE), 1.);
        assert_eq!(step_progress(0.99), 1.);
        let start = loader_slots(0.);
        let end = loader_slots(1.);
        // After a full step every dumpling sits where the one above it started.
        for i in 0..3 {
            assert_eq!(end[i + 1], start[i]);
        }
        // At rest exactly three show, full size, in slots 0..2; the entering and leaving ones are gone.
        let shown: Vec<_> = start.iter().filter(|(_, s)| *s > 0.).collect();
        assert_eq!(shown.len(), 3);
        assert!(shown.iter().all(|(at, s)| *s == 1. && (0. ..=2.).contains(at)));
    }
}
