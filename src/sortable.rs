// Sortable lists, as the web's sortable drag-and-drop works: while a drag hovers a list, the items the
// dragged one passes slide aside and its slot opens at the drop point; a list it leaves closes up behind
// it. Every reorderable list in the app (the rail and each folder in it, the channel list's categories,
// roles) is built from this, so they all move, aim and drop alike.
//
// - Locked to the vertical axis: only the pointer's height picks the drop point, so wandering sideways out
//   of a list still sorts, and the drop lands where the list shows it whatever is under the pointer.
// - Items move by a relative layout offset, not a paint transform (a transform leaves clipping, such as a
//   truncated label, behind at the layout spot). The drop point is judged against where items would be
//   at rest, so items sliding under a still pointer never change it.
// - The hint holds across gaps and past a list's ends; hovering onto an item (a combine, such as a server
//   onto a folder) keeps the opening where it was.
//
// A surface keeps one `Board` per drag type, gives each list `Board::list` and each row `List::item`,
// makes rows drop targets with `Board::target` (a list's header with `Board::head`), lifts with
// `Board::lift`, and applies `Board::finish` once the drag is let go.
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    time::Duration,
};

use gpui_kit::{
    AnyElement, App, Bounds, Context, DragMoveEvent, Entity, InteractiveElement, IntoElement, Pixels, Point, Render, SharedString, WeakEntity, Window, canvas, div, prelude::*, px,
};

// Items slide on a spring that keeps its speed when its target moves (the pointer crossing rows, a folder
// folding while the list measures it). A tween restarts its full length on every new target and crawls.
const SLIDE: Duration = Duration::from_millis(220);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Place {
    Before,
    After,
    // Onto the item (a combine), or into the list from its header.
    Into,
}

impl Place {
    // `fraction` is how far down the target the pointer is, 0 at its top. A target that combines gives its
    // middle half to Into.
    pub fn at(fraction: f32, combines: bool) -> Self {
        match (combines, fraction) {
            (false, f) if f < 0.5 => Place::Before,
            (false, _) => Place::After,
            (true, f) if f < 0.25 => Place::Before,
            (true, f) if f > 0.75 => Place::After,
            _ => Place::Into,
        }
    }
}

// Where letting go would drop: beside or onto `target` in `list`, or with no target, first in `list`
// (its header).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Hint {
    pub list: SharedString,
    pub target: Option<String>,
    pub place: Place,
}

// A drag that was let go of: what was carried and, if it was over a list, where it goes.
pub struct Finished {
    pub dragged: String,
    pub hint: Option<Hint>,
}

#[derive(Default)]
pub struct Board {
    dragged: Option<String>,
    hint: Option<Hint>,
    shared: Rc<Shared>,
}

#[derive(Default)]
struct Shared {
    // Each item's layout slot at rest, measured every frame: rows vary (a voice channel's people, an open
    // folder).
    slots: RefCell<HashMap<String, Bounds<Pixels>>>,
    // Each item's offset this frame, taken back off its layout bounds to get its slot at rest.
    shown: RefCell<HashMap<String, f32>>,
    // Per list, the last opening, kept while the pointer is onto an item.
    openings: RefCell<HashMap<SharedString, usize>>,
    // Bumped when a drag ends, so offsets start at rest instead of easing back from where the list was
    // (the drop has already reordered it).
    generation: Cell<u32>,
}

impl Board {
    pub fn dragged(&self) -> Option<&str> {
        self.dragged.as_deref()
    }

    pub fn hint(&self) -> Option<&Hint> {
        self.hint.as_ref()
    }

    // Once gpui has let go of the drag: what to apply. Call before the lists render, so what was carried
    // lands in its new place.
    pub fn finish(&mut self, cx: &App) -> Option<Finished> {
        if cx.has_active_drag() {
            return None;
        }
        let hint = self.hint.take();
        let dragged = self.dragged.take()?;
        self.shared.generation.set(self.shared.generation.get().wrapping_add(1));
        self.shared.openings.borrow_mut().clear();
        Some(Finished { dragged, hint })
    }

    // Wraps a drag's ghost constructor (`on_drag`'s second argument) so the board knows what's carried
    // from the moment it's picked up.
    pub fn lift<V: 'static, T: 'static, W: Render>(
        view: WeakEntity<V>,
        board: fn(&mut V) -> &mut Board,
        key: impl Into<String>,
        ghost: impl Fn(&T, Point<Pixels>, &mut Window, &mut App) -> Entity<W> + 'static,
    ) -> impl Fn(&T, Point<Pixels>, &mut Window, &mut App) -> Entity<W> + 'static {
        let key = key.into();
        move |value, offset, window, cx| {
            view.update(cx, |view, cx| {
                let board = board(view);
                board.dragged = Some(key.clone());
                board.hint = None;
                cx.notify();
            })
            .ok();
            ghost(value, offset, window, cx)
        }
    }

    // Makes `el`, the row `key` in `list`, a drop point for drags of `T`; `combines` says whether the
    // carried value can go onto it.
    pub fn target<V: 'static, T: 'static, E: InteractiveElement>(
        el: E,
        board: fn(&mut V) -> &mut Board,
        list: impl Into<SharedString>,
        key: impl Into<String>,
        combines: impl Fn(&T) -> bool + 'static,
        cx: &mut Context<V>,
    ) -> E {
        let (list, key) = (list.into(), key.into());
        el.on_drag_move::<T>(cx.listener(move |view, event: &DragMoveEvent<T>, _, cx| {
            let combines = combines(event.drag(cx));
            let board = board(view);
            let bounds = unmoved(&board.shared.shown.borrow(), &key, event.bounds);
            if !level_with(bounds, event.event.position) {
                return;
            }
            let fraction = (event.event.position.y - bounds.top()) / bounds.size.height;
            board.aim(Hint { list: list.clone(), target: Some(key.clone()), place: Place::at(fraction, combines) }, cx);
        }))
    }

    // Makes `el` (a list's header) drop first into `list`.
    pub fn head<V: 'static, T: 'static, E: InteractiveElement>(el: E, board: fn(&mut V) -> &mut Board, list: impl Into<SharedString>, cx: &mut Context<V>) -> E {
        let list = list.into();
        el.on_drag_move::<T>(cx.listener(move |view, event: &DragMoveEvent<T>, _, cx| {
            if level_with(event.bounds, event.event.position) {
                board(view).aim(Hint { list: list.clone(), target: None, place: Place::Into }, cx);
            }
        }))
    }

    fn aim<V: 'static>(&mut self, hint: Hint, cx: &mut Context<V>) {
        if self.dragged.is_none() || self.hint.as_ref() == Some(&hint) {
            return;
        }
        self.hint = Some(hint);
        cx.notify();
    }

    // `list`'s items, top to bottom, positioned for the drag in progress.
    pub fn list(&self, list: impl Into<SharedString>, order: Vec<String>, window: &mut Window, cx: &mut App) -> List {
        let list = list.into();
        let shared = &self.shared;
        let slots = shared.slots.borrow();
        let heights = slot_heights(&order, &slots);
        let source = self.dragged.as_ref().and_then(|d| order.iter().position(|k| k == d));
        let here = self.hint.as_ref().filter(|h| h.list == list);
        let opening = here.and_then(|h| match (&h.target, h.place) {
            (None, _) => Some(0),
            (Some(target), place) => {
                let at = order.iter().position(|k| k == target)?;
                match place {
                    Place::Before => Some(at),
                    Place::After => Some(at + 1),
                    Place::Into => shared.openings.borrow().get(&list).copied(),
                }
            }
        });
        match opening {
            Some(at) => _ = shared.openings.borrow_mut().insert(list.clone(), at),
            None => _ = shared.openings.borrow_mut().remove(&list),
        }
        // Carried in from another list: room for its own height and this list's gap.
        let gap = match (order.first().and_then(|k| slots.get(k)), order.get(1).and_then(|k| slots.get(k))) {
            (Some(a), Some(b)) => f32::from(b.top() - a.bottom()),
            _ => 0.,
        };
        let incoming = self.dragged.as_ref().and_then(|d| slots.get(d)).map_or(0., |b| f32::from(b.size.height) + gap);
        let elsewhere = self.hint.is_some() && here.is_none();
        let shifts = preview(&heights, source, incoming, opening, elsewhere);
        drop(slots);
        let away = (elsewhere && source.is_some()).then(|| self.dragged.clone()).flatten();
        let generation = shared.generation.get();
        let offsets: HashMap<String, f32> = order
            .iter()
            .zip(shifts)
            .map(|(key, shift)| {
                let id = SharedString::from(format!("sortable-{generation}-{key}"));
                // Whole pixels, so measurement jitter never counts as a new target.
                (key.clone(), gpui_kit::base::spring(id, shift.round(), gpui_kit::base::Spring::new(SLIDE), window, cx))
            })
            .collect();
        shared.shown.borrow_mut().extend(offsets.iter().map(|(k, v)| (k.clone(), *v)));
        List { offsets, away, shared: shared.clone() }
    }

    // `key`'s slot at rest, measured last frame.
    pub fn slot(&self, key: &str) -> Option<Bounds<Pixels>> {
        self.shared.slots.borrow().get(key).copied()
    }
}

pub struct List {
    offsets: HashMap<String, f32>,
    // The carried item while it hovers another list: this list has closed up over its slot.
    away: Option<String>,
    shared: Rc<Shared>,
}

impl List {
    // The row `key`, placed for the drag and measured.
    pub fn item(&self, key: &str, el: impl IntoElement) -> AnyElement {
        let (shared, measured) = (self.shared.clone(), key.to_owned());
        let measure = canvas(
            move |bounds, _, _| {
                let at_rest = unmoved(&shared.shown.borrow(), &measured, bounds);
                shared.slots.borrow_mut().insert(measured.clone(), at_rest);
            },
            |_, _, _, _| {},
        )
        .absolute()
        .inset_0();
        self.follow(key, div().relative().child(measure).child(el))
    }

    // A row that belongs to the item `key` (a channel's people, its threads): it moves with it, unmeasured,
    // since `key`'s slot already runs to the next item.
    pub fn follow(&self, key: &str, el: impl IntoElement) -> AnyElement {
        let offset = self.offsets.get(key).copied().unwrap_or(0.);
        div().relative().top(px(offset)).when(self.away.as_deref() == Some(key), |d| d.invisible()).child(el).into_any_element()
    }
}

fn unmoved(shown: &HashMap<String, f32>, key: &str, mut bounds: Bounds<Pixels>) -> Bounds<Pixels> {
    bounds.origin.y -= px(shown.get(key).copied().unwrap_or(0.));
    bounds
}

fn level_with(bounds: Bounds<Pixels>, at: Point<Pixels>) -> bool {
    at.y >= bounds.top() && at.y < bounds.bottom()
}

// Each item's slot: from its top to the next item's, the last its height plus the gap above it.
fn slot_heights(order: &[String], slots: &HashMap<String, Bounds<Pixels>>) -> Vec<f32> {
    let tops: Vec<Option<f32>> = order.iter().map(|k| slots.get(k).map(|b| f32::from(b.top()))).collect();
    (0..order.len())
        .map(|i| {
            let own = slots.get(&order[i]).map_or(0., |b| f32::from(b.size.height));
            match (tops[i], tops.get(i + 1).copied().flatten()) {
                (Some(top), Some(next)) => next - top,
                _ => own + tops[i].zip(i.checked_sub(1).and_then(|p| slots.get(&order[p]))).map_or(0., |(top, prev)| top - f32::from(prev.bottom())),
            }
        })
        .collect()
}

// How far each item moves (down positive). With an opening, the item at `source` goes in before index
// `opening`: the items it passes move toward its old place and it moves past them all; carried in from
// another list (`incoming` tall), everything from the opening down makes room. With none, a list whose
// item hovers `elsewhere` closes up over it.
fn preview(heights: &[f32], source: Option<usize>, incoming: f32, opening: Option<usize>, elsewhere: bool) -> Vec<f32> {
    let mut out = vec![0.; heights.len()];
    match (opening.map(|at| at.min(heights.len())), source) {
        (Some(at), Some(s)) if at > s + 1 => {
            out[s + 1..at].fill(-heights[s]);
            out[s] = heights[s + 1..at].iter().sum();
        }
        (Some(at), Some(s)) if at < s => {
            out[at..s].fill(heights[s]);
            out[s] = -heights[at..s].iter().sum::<f32>();
        }
        (Some(_), Some(_)) => {}
        (Some(at), None) => out[at..].fill(incoming),
        (None, Some(s)) if elsewhere => out[s + 1..].fill(-heights[s]),
        (None, _) => {}
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_slide_aside_and_close_up() {
        let h = [10., 20., 30., 40.];
        // Item 0 to before item 3: items 1 and 2 move up by its slot, it moves down past both.
        assert_eq!(preview(&h, Some(0), 0., Some(3), false), vec![50., -10., -10., 0.]);
        // Item 3 to the top: everything above moves down by its slot, it moves up past all of them.
        assert_eq!(preview(&h, Some(3), 0., Some(0), false), vec![40., 40., 40., -60.]);
        // Dropping right where it is, either side of itself, moves nothing.
        assert_eq!(preview(&h, Some(1), 0., Some(1), false), vec![0.; 4]);
        assert_eq!(preview(&h, Some(1), 0., Some(2), false), vec![0.; 4]);
        // From another list: room opens from the opening down; past the end, nothing moves.
        assert_eq!(preview(&h, None, 15., Some(2), false), vec![0., 0., 15., 15.]);
        assert_eq!(preview(&h, None, 15., Some(9), false), vec![0.; 4]);
        // Its own list, while it hovers another: the items below close up over its slot.
        assert_eq!(preview(&h, Some(1), 0., None, true), vec![0., 0., -20., -20.]);
        // No drag over any list: at rest.
        assert_eq!(preview(&h, Some(1), 0., None, false), vec![0.; 4]);
    }

    #[test]
    fn a_combining_target_gives_its_middle_to_into() {
        assert_eq!(Place::at(0.1, true), Place::Before);
        assert_eq!(Place::at(0.5, true), Place::Into);
        assert_eq!(Place::at(0.9, true), Place::After);
        assert_eq!(Place::at(0.4, false), Place::Before);
        assert_eq!(Place::at(0.6, false), Place::After);
    }
}
