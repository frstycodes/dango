// Hover tunnels (safe triangles): a surface opened by hover stays open while the pointer heads for it,
// i.e. while it stays inside the triangle from its previous position to the surface's near edge.
use std::time::Duration;

use gpui_kit::{Bounds, Pixels, Point};

// How long a tunnel holds a surface open after the pointer leaves its trigger.
pub const TUNNEL_GRACE: Duration = Duration::from_millis(300);

// The pointer's last two distinct positions. Hover and move listeners run in either order within one
// event, so a check at the current position measures from the sample before it.
#[derive(Default)]
pub struct Tunnel {
    prev: Option<Point<Pixels>>,
    last: Option<Point<Pixels>>,
}

impl Tunnel {
    pub fn track(&mut self, at: Point<Pixels>) {
        if self.last != Some(at) {
            self.prev = self.last;
            self.last = Some(at);
        }
    }

    pub fn heading_to(&self, at: Point<Pixels>, target: Bounds<Pixels>) -> bool {
        let apex = if self.last == Some(at) { self.prev } else { self.last };
        apex.is_some_and(|apex| in_tunnel(apex, at, target))
    }
}

// Whether `at` lies in the triangle from `apex` to the edge of `target` facing it, or in `target` itself.
pub fn in_tunnel(apex: Point<Pixels>, at: Point<Pixels>, target: Bounds<Pixels>) -> bool {
    if target.contains(&at) {
        return true;
    }
    if target.contains(&apex) {
        return false;
    }
    let (tl, br) = (target.origin, target.bottom_right());
    let (a, b) = if apex.x < tl.x {
        (tl, Point::new(tl.x, br.y))
    } else if apex.x > br.x {
        (Point::new(br.x, tl.y), br)
    } else if apex.y < tl.y {
        (tl, Point::new(br.x, tl.y))
    } else {
        (Point::new(tl.x, br.y), br)
    };
    in_triangle(at, apex, a, b)
}

fn in_triangle(p: Point<Pixels>, a: Point<Pixels>, b: Point<Pixels>, c: Point<Pixels>) -> bool {
    let cross = |o: Point<Pixels>, u: Point<Pixels>| f32::from(u.x - o.x) * f32::from(p.y - o.y) - f32::from(u.y - o.y) * f32::from(p.x - o.x);
    let (d1, d2, d3) = (cross(a, b), cross(b, c), cross(c, a));
    let has_neg = d1 < 0. || d2 < 0. || d3 < 0.;
    let has_pos = d1 > 0. || d2 > 0. || d3 > 0.;
    !(has_neg && has_pos)
}

#[cfg(test)]
mod tests {
    use gpui_kit::{point, px, size};

    use super::*;

    // A 100x200 card to the right of the pointer, from (100, 0) to (200, 200).
    fn card() -> Bounds<Pixels> {
        Bounds::new(point(px(100.), px(0.)), size(px(100.), px(200.)))
    }

    #[test]
    fn heading_for_the_near_edge_is_inside() {
        let apex = point(px(0.), px(100.));
        assert!(in_tunnel(apex, point(px(50.), px(60.)), card()));
        assert!(in_tunnel(apex, point(px(50.), px(140.)), card()));
        assert!(in_tunnel(apex, point(px(150.), px(10.)), card()));
    }

    #[test]
    fn veering_away_is_outside() {
        let apex = point(px(0.), px(100.));
        assert!(!in_tunnel(apex, point(px(50.), px(-10.)), card()));
        assert!(!in_tunnel(apex, point(px(-5.), px(100.)), card()));
        assert!(!in_tunnel(apex, point(px(20.), px(160.)), card()));
    }

    #[test]
    fn faces_whichever_edge_is_near() {
        let above = point(px(150.), px(-50.));
        assert!(in_tunnel(above, point(px(150.), px(-20.)), card()));
        assert!(!in_tunnel(above, point(px(260.), px(-20.)), card()));
        let right = point(px(300.), px(100.));
        assert!(in_tunnel(right, point(px(250.), px(100.)), card()));
    }

    #[test]
    fn tracks_from_the_sample_before_the_current_one() {
        let mut tunnel = Tunnel::default();
        tunnel.track(point(px(0.), px(100.)));
        tunnel.track(point(px(10.), px(100.)));
        assert!(tunnel.heading_to(point(px(10.), px(100.)), card()));
        assert!(!tunnel.heading_to(point(px(10.), px(90.)), Bounds::new(point(px(100.), px(150.)), size(px(10.), px(10.)))));
    }
}
