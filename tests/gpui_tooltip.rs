// Checks the vendored gpui patch that anchors tooltips to their element: gpui-pre's own lib tests don't
// build standalone. Drop with the vendored gpui once upstream ships the feature.
use gpui_kit::{Bounds, Pixels, Size, TooltipPlacement, anchored_tooltip_bounds, point, px, size};

fn window() -> Bounds<Pixels> {
    Bounds::new(point(px(0.), px(0.)), size(px(800.), px(600.)))
}

fn tip() -> Size<Pixels> {
    size(px(100.), px(40.))
}

fn at(x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
    Bounds::new(point(px(x), px(y)), size(px(w), px(h)))
}

#[test]
fn side_opens_right_centred_and_flips_left_at_the_edge() {
    let placed = anchored_tooltip_bounds(at(12., 100., 40., 40.), tip(), TooltipPlacement::Side, window());
    assert_eq!(placed.origin, point(px(52.), px(100.)));
    let placed = anchored_tooltip_bounds(at(750., 100., 40., 40.), tip(), TooltipPlacement::Side, window());
    assert_eq!(placed.origin.x, px(650.));
}

#[test]
fn side_slides_to_stay_inside_vertically() {
    let placed = anchored_tooltip_bounds(at(12., 0., 40., 10.), tip(), TooltipPlacement::Side, window());
    assert_eq!(placed.origin.y, px(0.));
    let placed = anchored_tooltip_bounds(at(12., 590., 40., 10.), tip(), TooltipPlacement::Side, window());
    assert_eq!(placed.origin.y, px(560.));
}

#[test]
fn auto_opens_below_centred_and_flips_above_at_the_bottom() {
    let placed = anchored_tooltip_bounds(at(300., 100., 40., 20.), tip(), TooltipPlacement::Auto, window());
    assert_eq!(placed.origin, point(px(270.), px(120.)));
    let placed = anchored_tooltip_bounds(at(300., 570., 40., 20.), tip(), TooltipPlacement::Auto, window());
    assert_eq!(placed.origin.y, px(530.));
}

#[test]
fn auto_slides_to_stay_inside_horizontally() {
    let placed = anchored_tooltip_bounds(at(0., 100., 20., 20.), tip(), TooltipPlacement::Auto, window());
    assert_eq!(placed.origin.x, px(0.));
    let placed = anchored_tooltip_bounds(at(790., 100., 10., 20.), tip(), TooltipPlacement::Auto, window());
    assert_eq!(placed.origin.x, px(700.));
}
