// The stream picture-in-picture (design/voice.html .pip): a Go Live we're watching, floating over the
// whole app while we're away from the call. It rests in a corner of the window; dragged, it follows the
// pointer and on release springs to the nearest corner, as Discord's does. Clicking it opens the call.
use std::time::Duration;

use gpui_kit::{Animation, AnimationExt, AnyElement, Context, FontWeight, MouseButton, Pixels, Point, SharedString, Window, div, prelude::*, px};

use crate::{
    Waffle,
    ui::{self, RADIUS_MD, Squircle},
};

const WIDTH: f32 = 320.;
const HEIGHT: f32 = WIDTH * 9. / 16.;
const MARGIN: f32 = 16.;
const SNAP: Duration = Duration::from_millis(260);

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub(crate) enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    #[default]
    BottomRight,
}

#[derive(Default)]
pub(crate) struct Pip {
    corner: Corner,
    // Where the pointer took hold, inside the player.
    grab: Point<Pixels>,
    // Top-left within the window while carried.
    carried: Option<(f32, f32)>,
    // The window's size, from the last drag move.
    area: (f32, f32),
    // Where it was let go, and a count so each landing replays the snap.
    landed: Option<((f32, f32), usize)>,
}

// The player being carried; the real one follows the pointer, so this draws nothing.
#[derive(Clone)]
pub(crate) struct PipDrag;

impl Render for PipDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

// Insets from the window's top and bottom edges.
type Insets = (f32, f32);
const INSETS: Insets = (MARGIN, MARGIN);

// Top-left of the player resting in `corner` of an `area`-sized window.
fn rest(corner: Corner, area: (f32, f32), insets: Insets) -> (f32, f32) {
    let x = match corner {
        Corner::TopLeft | Corner::BottomLeft => MARGIN,
        Corner::TopRight | Corner::BottomRight => area.0 - WIDTH - MARGIN,
    };
    let y = match corner {
        Corner::TopLeft | Corner::TopRight => insets.0,
        Corner::BottomLeft | Corner::BottomRight => area.1 - HEIGHT - insets.1,
    };
    (x, y)
}

// The corner whose quarter of the window holds the player's centre.
fn nearest(at: (f32, f32), area: (f32, f32)) -> Corner {
    let (left, top) = (at.0 + WIDTH / 2. < area.0 / 2., at.1 + HEIGHT / 2. < area.1 / 2.);
    match (left, top) {
        (true, true) => Corner::TopLeft,
        (false, true) => Corner::TopRight,
        (true, false) => Corner::BottomLeft,
        (false, false) => Corner::BottomRight,
    }
}

fn anchored<E: Styled>(d: E, corner: Corner, insets: Insets) -> E {
    let d = match corner {
        Corner::TopLeft | Corner::BottomLeft => d.left(px(MARGIN)),
        Corner::TopRight | Corner::BottomRight => d.right(px(MARGIN)),
    };
    match corner {
        Corner::TopLeft | Corner::TopRight => d.top(px(insets.0)),
        Corner::BottomLeft | Corner::BottomRight => d.bottom(px(insets.1)),
    }
}

impl Waffle {
    pub(crate) fn stream_pip(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let call = self.call.as_ref()?;
        let watching = call.focused()?;
        let channel = call.channel.clone();
        let name = self.user_name(&watching.streamer_id);
        let white = gpui_kit::white();
        // Inset absolutely, so a 4:3 picture letterboxes inside the frame instead of growing past it; square,
        // only the frame is rounded.
        let picture: AnyElement = match &watching.frame {
            Some(frame) => ui::media(gpui_kit::ImageSource::Render(frame.clone()), ui::all(0.)).absolute().inset_0().size_full().object_fit(gpui_kit::ObjectFit::Contain).into_any_element(),
            None => crate::call::stream_fill(0.).absolute().inset_0().flex().items_center().justify_center().text_size(px(12.)).text_color(white.opacity(0.7)).child(format!("Joining {name}'s stream…")).into_any_element(),
        };
        let bar = div()
            .absolute()
            .left_0()
            .right_0()
            .bottom_0()
            .p(px(8.))
            .flex()
            .items_center()
            .gap(px(8.))
            .bg(gpui_kit::linear_gradient(180., gpui_kit::linear_color_stop(gpui_kit::transparent_black(), 0.), gpui_kit::linear_color_stop(gpui_kit::black().opacity(0.6), 1.)))
            .opacity(0.)
            .group_hover("stream-pip", |s| s.opacity(1.))
            .child(div().flex_1().min_w_0().truncate().text_size(px(12.)).font_weight(FontWeight::MEDIUM).text_color(white).child(format!("{name}'s screen")))
            .child(ui::button("pip-stop", ui::Tone::OnMedia, cx).small().child("Stop watching").on_click(cx.listener(|this, _, _, cx| {
                cx.stop_propagation();
                this.stop_watching(cx);
            })));
        let this = cx.weak_entity();
        let player = div()
            .id("stream-pip")
            .group("stream-pip")
            .absolute()
            .w(px(WIDTH))
            .h(px(HEIGHT))
            .squircle(RADIUS_MD)
            .overflow_hidden()
            .bg(gpui_kit::black())
            .shadow(ui::shadow_pop(cx))
            .cursor_pointer()
            .child(picture)
            .child(bar)
            .on_drag(PipDrag, move |drag, grab, _, cx| {
                this.update(cx, |this, _| this.pip.grab = grab).ok();
                cx.new(|_| drag.clone())
            })
            .on_click(cx.listener(move |this, _, window, cx| this.show_call(channel.clone(), window, cx)));
        let pip = &self.pip;
        let insets = INSETS;
        let placed: AnyElement = match (pip.carried, pip.landed) {
            (Some((x, y)), _) => player.left(px(x)).top(px(y)).into_any_element(),
            (None, Some((from, n))) => {
                let (corner, to) = (pip.corner, rest(pip.corner, pip.area, insets));
                player
                    .with_animation(SharedString::from(format!("pip-snap-{n}")), Animation::new(SNAP), move |d, t| {
                        if t >= 1. {
                            return anchored(d, corner, insets);
                        }
                        let e = crate::motion::back_out(t);
                        d.left(px(from.0 + (to.0 - from.0) * e)).top(px(from.1 + (to.1 - from.1) * e))
                    })
                    .into_any_element()
            }
            _ => anchored(player, pip.corner, insets).into_any_element(),
        };
        Some(placed)
    }

    // The drag's moves and drop, on the app's root: the player can go to any edge of the window.
    pub(crate) fn pip_drop_zone<E: InteractiveElement>(&self, zone: E, cx: &mut Context<Self>) -> E {
        zone.on_drag_move::<PipDrag>(cx.listener(|this, e: &gpui_kit::DragMoveEvent<PipDrag>, _, cx| {
            let area = (f32::from(e.bounds.size.width), f32::from(e.bounds.size.height));
            let at = e.event.position - e.bounds.origin - this.pip.grab;
            this.pip.area = area;
            this.pip.carried = Some((f32::from(at.x).clamp(0., (area.0 - WIDTH).max(0.)), f32::from(at.y).clamp(0., (area.1 - HEIGHT).max(0.))));
            cx.notify();
        }))
        .on_drop(cx.listener(|this, _: &PipDrag, _, cx| this.land_pip(cx)))
        .on_mouse_up_out(MouseButton::Left, cx.listener(|this, _, _, cx| this.land_pip(cx)))
    }

    fn land_pip(&mut self, cx: &mut Context<Self>) {
        let Some(at) = self.pip.carried.take() else { return };
        self.pip.corner = nearest(at, self.pip.area);
        let n = self.pip.landed.map_or(0, |(_, n)| n + 1);
        self.pip.landed = Some((at, n));
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lands_in_the_nearest_corner() {
        let area = (1000., 800.);
        assert_eq!(nearest((10., 10.), area), Corner::TopLeft);
        assert_eq!(nearest((900., 20.), area), Corner::TopRight);
        assert_eq!(nearest((0., 700.), area), Corner::BottomLeft);
        assert_eq!(nearest((700., 600.), area), Corner::BottomRight);
        // Its centre decides: the left edge just past the middle still lands left.
        assert_eq!(nearest((500. - WIDTH / 2. - 1., 0.), area), Corner::TopLeft);
        assert_eq!(rest(Corner::BottomRight, area, (64., 120.)), (1000. - WIDTH - MARGIN, 800. - HEIGHT - 120.));
        assert_eq!(rest(Corner::TopLeft, area, (64., 120.)), (MARGIN, 64.));
    }
}
