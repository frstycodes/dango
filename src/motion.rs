use std::time::Duration;

use gpui_motion_kit::core::{Bezier, Easing, Spring, Transition};

// Critically damped and quick (Apple's damping 1.0, ~220ms): settles without wobble and retargets from
// its live value, so it can be interrupted mid-flight.
pub fn spring() -> Transition {
    Transition::Spring(Spring::visual(Duration::from_millis(220), 0.).expect("valid spring"))
}

// Folds are one block, height and fade together, on the house enter curve.
pub fn fold() -> Transition {
    Transition::Tween(Duration::from_millis(140), Easing::Bezier(Bezier::new(0.23, 1., 0.32, 1.).expect("monotonic bezier")))
}

// Ease-out-back: a one-shot pop for gpui's built-in animations, overshooting ~10% before settling.
pub fn back_out(t: f32) -> f32 {
    let (c1, c3) = (1.70158, 2.70158);
    1. + c3 * (t - 1.).powi(3) + c1 * (t - 1.).powi(2)
}

#[cfg(test)]
mod tests {
    #[test]
    fn back_out_overshoots_then_lands() {
        assert_eq!(super::back_out(0.), 0.);
        assert!((super::back_out(1.) - 1.).abs() < 1e-6);
        let peak = (1..10).map(|i| super::back_out(i as f32 / 10.)).fold(0., f32::max);
        assert!(peak > 1.05 && peak < 1.15, "peak {peak}");
    }
}
