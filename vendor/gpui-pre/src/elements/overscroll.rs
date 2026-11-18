//! Rubber-band overscroll for scroll containers, after AppKit/UIKit.
//!
//! The container's real scroll offset stays clamped to its content; [`Overscroll`] holds a
//! separate, purely visual offset that the container adds when it positions its children.

use crate::{
    App, Pixels, Point, ScrollWheelEvent, Size, SpringConfig, SpringState, TouchPhase, point, px,
};
use std::{f32::consts::E, time::Instant};

const RESISTANCE: f32 = 0.55;
/// Natural frequency (rad/s) of [`RETURN_SPRING`].
const STIFFNESS: f32 = 20.;
/// The critically damped spring that returns a stretch to the edge: a 100 px stretch is
/// within half a pixel of rest after ~370 ms, and one released from rest never crosses it.
pub const RETURN_SPRING: SpringConfig =
    SpringConfig::new(STIFFNESS * STIFFNESS, 2. * STIFFNESS, 1.);
const REST_DISTANCE: f32 = 0.5;
const MIN_EVENT_INTERVAL: f32 = 1. / 240.;
const MAX_EVENT_INTERVAL: f32 = 1. / 30.;

/// Apple's rubber-band function: the displayed overshoot for a raw overshoot `x` along an axis
/// whose viewport is `d` long. Odd in `x`, and its magnitude never reaches `d`.
pub fn rubber_band(x: f32, d: f32) -> f32 {
    if d <= 0. {
        return 0.;
    }
    (1. - 1. / (x.abs() * RESISTANCE / d + 1.)) * d * x.signum()
}

/// The raw overshoot that [`rubber_band`] maps to `shown`, so a gesture can resume a stretch
/// that is already on screen.
pub fn rubber_band_inverse(shown: f32, d: f32) -> f32 {
    if d <= 0. {
        return 0.;
    }
    let f = shown.abs().min(d * 0.999);
    f * d / ((d - f) * RESISTANCE) * shown.signum()
}

/// The return spring's initial velocity for momentum hitting an edge at `velocity` px/s.
/// [`RETURN_SPRING`] starting at 0 with velocity `v` peaks at `v / (STIFFNESS * e)`; that peak
/// goes through [`rubber_band`] so a fast fling still overshoots less than the viewport `d`.
pub fn bounce_velocity(velocity: f32, d: f32) -> f32 {
    rubber_band(velocity / (STIFFNESS * E), d) * STIFFNESS * E
}

/// Whether this scroll event may stretch past an edge. Wheel notches clamp, and the gesture
/// phases this relies on are only told apart on macOS: Windows' Direct Manipulation keeps
/// inertia inside the touch and ends it only once inertia stops.
pub(crate) fn rubber_bands(event: &ScrollWheelEvent, cx: &App) -> bool {
    cfg!(target_os = "macos") && event.delta.precise() && !cx.reduce_motion()
}

#[derive(Clone, Copy, Debug, Default)]
struct Axis {
    spring: SpringState,
    momentum_spent: bool,
}

impl Axis {
    fn is_moving(&self) -> bool {
        self.spring != SpringState::default()
    }

    fn grab(&mut self) {
        self.spring.velocity = 0.;
        self.momentum_spent = false;
    }

    fn release(&mut self) {
        self.momentum_spent |= self.spring.position != 0.;
    }

    /// `position` runs from `-max` to 0 and `delta` is added to it. Returns the part of `delta`
    /// the real offset takes; the rest goes into the stretch.
    fn scroll(
        &mut self,
        position: f32,
        delta: f32,
        max: f32,
        d: f32,
        touching: bool,
        interval: f32,
    ) -> f32 {
        if max <= 0. || d <= 0. {
            return delta;
        }
        if touching {
            let logical = position + rubber_band_inverse(self.spring.position, d) + delta;
            let next = logical.clamp(-max, 0.);
            self.spring.position = rubber_band(logical - next, d);
            return next - position;
        }
        let next = (position + delta).clamp(-max, 0.);
        let hit_edge = next != position + delta;
        if hit_edge && !self.is_moving() && !self.momentum_spent {
            self.spring.velocity = bounce_velocity(delta / interval, d);
            self.momentum_spent = true;
        }
        next - position
    }

    fn step(&mut self, dt: f32) {
        if !self.is_moving() {
            return;
        }
        self.spring = RETURN_SPRING.step(self.spring, 0., dt);
        if RETURN_SPRING.is_settled(self.spring, 0., REST_DISTANCE) {
            self.spring = SpringState::default();
        }
    }
}

/// One scroll container's stretch past its edges.
///
/// macOS sends the finger's scroll events between `Started` and `Ended`, then momentum as
/// phaseless `Moved` events, so a `Moved` outside a touch is momentum: it bounces off an edge
/// once instead of stretching.
#[derive(Debug, Default)]
pub struct Overscroll {
    x: Axis,
    y: Axis,
    touching: bool,
    max: Point<Pixels>,
    viewport: Size<Pixels>,
    last_event: Option<Instant>,
    last_tick: Option<Instant>,
}

impl Overscroll {
    /// Records the scroll range and viewport, both measured at prepaint.
    pub fn set_extent(&mut self, max: Point<Pixels>, viewport: Size<Pixels>) {
        self.max = max;
        self.viewport = viewport;
    }

    /// The visual offset to add to the real scroll offset.
    pub fn offset(&self) -> Point<Pixels> {
        point(px(self.x.spring.position), px(self.y.spring.position))
    }

    /// Whether the stretch is returning or bouncing, so frames must keep coming.
    pub fn is_springing(&self) -> bool {
        !self.touching && (self.x.is_moving() || self.y.is_moving())
    }

    /// Feeds one scroll event at the real offset `position` (`-max..=0` per axis). Returns the
    /// part of `delta` to add to the real offset.
    pub fn scroll(
        &mut self,
        position: Point<Pixels>,
        delta: Point<Pixels>,
        phase: TouchPhase,
        now: Instant,
    ) -> Point<Pixels> {
        let was_springing = self.is_springing();
        let interval = self
            .last_event
            .map_or(MAX_EVENT_INTERVAL, |last| (now - last).as_secs_f32())
            .clamp(MIN_EVENT_INTERVAL, MAX_EVENT_INTERVAL);
        self.last_event = Some(now);
        match phase {
            TouchPhase::Started => {
                self.touching = true;
                self.x.grab();
                self.y.grab();
            }
            TouchPhase::Ended | TouchPhase::Cancelled => {
                self.touching = false;
                self.x.release();
                self.y.release();
            }
            TouchPhase::Moved => {}
        }
        let applied = point(
            px(self.x.scroll(
                position.x.0,
                delta.x.0,
                self.max.x.0,
                self.viewport.width.0,
                self.touching,
                interval,
            )),
            px(self.y.scroll(
                position.y.0,
                delta.y.0,
                self.max.y.0,
                self.viewport.height.0,
                self.touching,
                interval,
            )),
        );
        if !was_springing && self.is_springing() {
            self.last_tick = Some(now);
        }
        applied
    }

    /// Advances the return spring to `now`. Returns whether another frame is needed.
    pub fn tick(&mut self, now: Instant) -> bool {
        if !self.is_springing() {
            return false;
        }
        let dt = self
            .last_tick
            .map_or(0., |last| now.saturating_duration_since(last).as_secs_f32());
        self.last_tick = Some(now);
        self.x.step(dt);
        self.y.step(dt);
        self.is_springing()
    }
}
