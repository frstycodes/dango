// Checks the vendored gpui patch's transform and corner-shape math from outside the crate: gpui-pre's own
// lib tests don't build standalone. Drop with the vendored gpui once upstream ships the feature.
use gpui_kit::{Bounds, CornerShape, Point, ScaledPixels, Size, TransformationMatrix, radians};

fn scaled_point(x: f32, y: f32) -> Point<ScaledPixels> {
    Point::new(ScaledPixels(x), ScaledPixels(y))
}

fn assert_near(actual: Point<ScaledPixels>, expected: Point<ScaledPixels>) {
    let distance = (actual.x.0 - expected.x.0).hypot(actual.y.0 - expected.y.0);
    assert!(distance < 1e-4, "{actual:?} != {expected:?}");
}

#[test]
fn corner_shape_exponents_follow_css_superellipse() {
    assert_eq!(CornerShape::Round.exponent(), 2.);
    assert_eq!(CornerShape::Squircle.exponent(), 4.);
    assert_eq!(CornerShape::Bevel.exponent(), 1.);
    assert_eq!(CornerShape::Scoop.exponent(), -2.);
    assert!((CornerShape::Superellipse(1.4).exponent() - 1.4f32.exp2()).abs() < 1e-6);
    assert_eq!(CornerShape::Superellipse(f32::INFINITY).exponent(), 1024.);
}

#[test]
fn rotation_about_a_center_keeps_the_center_fixed() {
    let center = scaled_point(50., 20.);
    let transformation = TransformationMatrix::unit()
        .translate(center)
        .rotate(radians(std::f32::consts::FRAC_PI_2))
        .translate(scaled_point(-50., -20.));

    assert_near(transformation.apply_scaled(center), center);
    // Clockwise on screen, where y points down: right of the center ends up below it.
    assert_near(transformation.apply_scaled(scaled_point(60., 20.)), scaled_point(50., 30.));
}

#[test]
fn transformed_bounds_cover_every_corner() {
    let bounds = Bounds::new(scaled_point(0., 0.), Size::new(ScaledPixels(20.), ScaledPixels(10.)));
    let scaled = TransformationMatrix::unit().scale(Size::new(2., 3.));
    assert_eq!(scaled.transform_bounds(bounds), Bounds::new(scaled_point(0., 0.), Size::new(ScaledPixels(40.), ScaledPixels(30.))));

    let rotated = TransformationMatrix::unit().rotate(radians(std::f32::consts::FRAC_PI_2));
    let covered = rotated.transform_bounds(bounds);
    assert_near(covered.origin, scaled_point(-10., 0.));
    assert_near(covered.bottom_right(), scaled_point(0., 20.));

    assert_eq!(TransformationMatrix::unit().transform_bounds(bounds), bounds);
}
