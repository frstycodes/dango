// Discord's GIFs (Tenor, Klipy, Giphy) arrive as silent looping MP4s, which Discord plays inline.
//
// Each GIF on screen gets a small player: a decoder thread runs a few pictures ahead of the clock
// and loops the clip, and the player shows whichever picture is due. Only a handful of pictures
// exist per GIF at any moment, and each is freed from the GPU when the next replaces it. When the
// GIF scrolls away the player stops and lets go of its pictures, keeping nothing but the still.
// The downloaded clips themselves (small, compressed) sit in a shared cache with a size cap, so
// scrolling back doesn't download them again.
//
// Before this, every GIF was decoded whole into one gpui image (up to 300 pictures, ~100 MB for a
// long one) and kept in gpui's asset cache, and on the GPU, until the app quit.
use std::{
    cell::Cell,
    collections::{HashMap, VecDeque},
    ops::ControlFlow,
    rc::Rc,
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};

use gpui_kit::{AnyElement, App, Bounds, Context, Corners, Entity, Global, ImageSource, ObjectFit, Pixels, Render, RenderImage, Window, canvas, div, prelude::*};

use crate::h264::{Track, bgra};

// Wide enough for Discord's largest inline GIF.
const MAX_WIDTH: u32 = 400;
// Pictures decoded ahead of the clock: enough to ride out a slow one.
const AHEAD: usize = 4;
const TICK: Duration = Duration::from_millis(15);
// Not painted for this long: scrolled away, another channel opened, or the window hidden.
const OFFSCREEN: Duration = Duration::from_millis(250);
// Downloaded clips kept for scrolling back (compressed MP4s, typically 0.2–3 MB each).
const CLIP_CACHE_BYTES: usize = 48 << 20;
// A clip whose pictures fit in this much is decoded once and replayed from memory while it's on
// screen (most GIFs: a few seconds at 400 px); longer ones are decoded again each loop.
const REPLAY_BYTES: usize = 6 << 20;
// The fastest a GIF animates. Every new picture redraws the whole window, so a 30 fps clip at full
// speed costs a large share of a core; most GIFs are 10-15 fps anyway.
const MIN_FRAME: Duration = Duration::from_millis(66);
// A broken clip that reports no length would otherwise loop as fast as it decodes.
const MIN_LOOP: Duration = Duration::from_millis(100);

// The players in the chat and the viewer, by where they're shown.
#[derive(Default)]
struct Gifs {
    players: HashMap<String, Entity<GifPlayer>>,
}

impl Global for Gifs {}

// The GIF for `url`, playing while it's on screen; `key` must be unique to where it's shown. Shows
// `poster` until the first picture is ready, and for good if the clip can't be played.
pub fn tile(key: String, url: &str, poster: &str, cx: &mut App) -> AnyElement {
    rounded_tile(key, url, poster, Corners::default(), cx)
}

// A tile that rounds its own pictures to `corners`, since gpui can't clip them to a rounded parent.
pub fn rounded_tile(key: String, url: &str, poster: &str, corners: Corners<Pixels>, cx: &mut App) -> AnyElement {
    if let Some(player) = cx.try_global::<Gifs>().and_then(|g| g.players.get(&key)).cloned() {
        return player.into_any_element();
    }
    let (url, poster) = (url.to_owned(), poster.to_owned());
    let player = cx.new(|_| GifPlayer { url, poster, corners, state: State::Idle, frame: None, unpainted_since: Rc::default(), bounds: Rc::default(), ticking: false });
    cx.default_global::<Gifs>().players.insert(key, player.clone());
    player.into_any_element()
}

pub struct GifPlayer {
    url: String,
    poster: String,
    corners: Corners<Pixels>,
    state: State,
    frame: Option<Arc<RenderImage>>,
    // Set when the ticker asks for a render, cleared when the player paints: still set a while later
    // means it isn't on screen.
    unpainted_since: Rc<Cell<Option<Instant>>>,
    // Where it was last painted, in window coordinates, to tell whether another window covers it.
    bounds: Rc<Cell<Bounds<Pixels>>>,
    ticking: bool,
}

enum State {
    // Not playing: never shown yet, or stopped after scrolling away. Starts on the next render.
    Idle,
    Loading,
    Playing(Run),
    // Can't be played: the poster stays.
    Failed,
}

struct Run {
    pictures: mpsc::Receiver<Picture>,
    next: Option<Picture>,
    started: Instant,
    shown_at: Option<Instant>,
    // Pictures of a replayed clip: they stay on the GPU while it plays, so later loops don't upload
    // them again, and are freed when it stops.
    kept: Vec<Arc<RenderImage>>,
}

struct Picture {
    at: Duration,
    image: Arc<RenderImage>,
    // Part of a replayed clip (see `Run::kept`).
    kept: bool,
}

impl GifPlayer {
    fn start(&mut self, cx: &mut Context<Self>) {
        if let Some(clip) = cached_clip(&self.url) {
            return self.play(clip, cx);
        }
        self.state = State::Loading;
        let url = self.url.clone();
        cx.spawn(async move |this, cx| {
            let fetch = crate::discord::runtime().spawn(async move {
                let res = reqwest::get(&url).await.ok()?;
                res.status().is_success().then_some(())?;
                Some((url, res.bytes().await.ok()?))
            });
            let fetched = fetch.await.ok().flatten();
            this.update(cx, |this, cx| {
                if !matches!(this.state, State::Loading) {
                    return;
                }
                match fetched {
                    Some((url, bytes)) => {
                        let clip: Arc<[u8]> = Arc::from(bytes.as_ref());
                        cache_clip(url, clip.clone());
                        this.play(clip, cx);
                    }
                    None => this.state = State::Failed,
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn play(&mut self, clip: Arc<[u8]>, cx: &mut Context<Self>) {
        if !matches!(self.state, State::Playing(_)) {
            PLAYING.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        self.state = State::Playing(Run { pictures: decode_looping(clip), next: None, started: Instant::now(), shown_at: None, kept: Vec::new() });
        if self.ticking {
            return;
        }
        self.ticking = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(TICK).await;
                let Ok(true) = this.update(cx, |this, cx| this.tick(cx)) else { break };
            }
        })
        .detach();
    }

    // Whether this GIF should be moving: the window focused and its spot not covered by another window.
    fn wanted(&self) -> bool {
        crate::presence::may_animate() && crate::presence::is_visible(self.bounds.get())
    }

    // Shows the picture that's due; stops and frees everything once the GIF is off screen, covered,
    // or the window loses focus.
    fn tick(&mut self, cx: &mut Context<Self>) -> bool {
        let offscreen = self.unpainted_since.get().is_some_and(|t| t.elapsed() > OFFSCREEN) || !self.wanted();
        let State::Playing(run) = &mut self.state else {
            self.ticking = false;
            return false;
        };
        if offscreen {
            self.stop(cx);
            return false;
        }
        if run.shown_at.is_some_and(|at| at.elapsed() < MIN_FRAME) {
            return true;
        }
        let now = run.started.elapsed();
        let mut ended = false;
        let mut due: Option<Picture> = None;
        loop {
            let picture = match run.next.take() {
                Some(picture) => picture,
                None => match run.pictures.try_recv() {
                    Ok(picture) => picture,
                    Err(error) => {
                        ended = error == mpsc::TryRecvError::Disconnected;
                        break;
                    }
                },
            };
            if picture.at > now {
                run.next = Some(picture);
                break;
            }
            if picture.kept && !run.kept.iter().any(|k| Arc::ptr_eq(k, &picture.image)) {
                run.kept.push(picture.image.clone());
            }
            // A picture the clock has already passed never reached the GPU; dropping it is enough.
            due = Some(picture);
        }
        if ended && run.next.is_none() && due.is_none() {
            // The decoder gave up (a clip it can't read): keep the last picture, or the poster.
            PLAYING.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
            self.state = if self.frame.is_none() { State::Failed } else { State::Idle };
            self.ticking = false;
            cx.notify();
            return false;
        }
        if let Some(picture) = due {
            run.shown_at = Some(Instant::now());
            if let Some(old) = self.frame.replace(picture.image)
                && !run.kept.iter().any(|k| Arc::ptr_eq(k, &old))
            {
                // Each picture is its own GPU texture; the old one must be freed or the atlas grows for good.
                cx.drop_image(old, None);
            }
            if self.unpainted_since.get().is_none() {
                self.unpainted_since.set(Some(Instant::now()));
            }
            cx.notify();
        }
        true
    }

    // Drops the decoder (its thread ends at its next send) and every picture, keeping the clip cached.
    fn stop(&mut self, cx: &mut Context<Self>) {
        if let State::Playing(run) = std::mem::replace(&mut self.state, State::Idle) {
            PLAYING.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
            drop(run.pictures);
            if let Some(next) = run.next {
                cx.drop_image(next.image, None);
            }
            for image in run.kept {
                cx.drop_image(image, None);
            }
        }
        if let Some(frame) = self.frame.take() {
            // Harmless if it was a kept picture already freed above.
            cx.drop_image(frame, None);
        }
        self.unpainted_since.set(None);
        self.ticking = false;
    }
}

impl Render for GifPlayer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Starts again when shown: scrolled back, or the window focused or uncovered (presence
        // changes redraw the window). Until then the still shows.
        if matches!(self.state, State::Idle) && self.wanted() {
            self.start(cx);
        }
        let source = match &self.frame {
            Some(frame) => ImageSource::Render(frame.clone()),
            None => self.poster.clone().into(),
        };
        let picture = crate::ui::media(source, self.corners);
        let (painted, at) = (self.unpainted_since.clone(), self.bounds.clone());
        div()
            .relative()
            .size_full()
            .overflow_hidden()
            .child(
                canvas(
                    move |bounds, _, _| {
                        painted.set(None);
                        at.set(bounds);
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .child(picture.size_full().object_fit(ObjectFit::Cover))
    }
}

// Plays the clip in a loop on its own thread, each loop's times following on from the last, staying
// `AHEAD` pictures in front. The first loop is decoded; if its pictures fit in `REPLAY_BYTES` they're
// kept and replayed, so a short GIF costs no CPU after one loop. Ends when the receiver is dropped
// (taking any kept pictures with it) or the clip can't be decoded.
fn decode_looping(clip: Arc<[u8]>) -> mpsc::Receiver<Picture> {
    let (send, receive) = mpsc::sync_channel(AHEAD);
    let decode = move || {
        let mut offset = Duration::ZERO;
        let mut replay: Option<(Duration, Vec<(Duration, Arc<RenderImage>)>)> = None;
        loop {
            if let Some((length, pictures)) = &replay {
                for (at, image) in pictures {
                    if send.send(Picture { at: offset + *at, image: image.clone(), kept: true }).is_err() {
                        return;
                    }
                }
                offset += *length;
                continue;
            }
            let Ok(mut track) = Track::open(clip.clone()) else { return };
            let length = track.duration().max(MIN_LOOP);
            let (mut shown, mut receiver_gone) = (0usize, false);
            let (mut kept, mut kept_bytes) = (Some(Vec::new()), 0usize);
            let _ = track.decode(0, |at, yuv| {
                let Some(picture) = bgra(yuv, MAX_WIDTH) else { return ControlFlow::Continue(()) };
                kept_bytes += picture.as_raw().len();
                let image = Arc::new(RenderImage::new([image::Frame::new(picture)]));
                if kept_bytes > REPLAY_BYTES {
                    kept = None;
                }
                if let Some(kept) = &mut kept {
                    kept.push((at, image.clone()));
                }
                if send.send(Picture { at: offset + at, image, kept: kept.is_some() }).is_err() {
                    receiver_gone = true;
                    return ControlFlow::Break(());
                }
                shown += 1;
                ControlFlow::Continue(())
            });
            if receiver_gone || shown == 0 {
                return;
            }
            replay = kept.map(|pictures| (length, pictures));
            offset += length;
        }
    };
    let _ = std::thread::Builder::new().name("gif-decode".into()).spawn(decode);
    receive
}

// GIFs decoding right now (each holds up to `AHEAD` + 2 pictures), for the memory report.
static PLAYING: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

pub struct Stats {
    pub playing: usize,
    pub clip_cache_bytes: usize,
}

pub fn stats() -> Stats {
    let clip_cache_bytes = CLIPS.lock().map(|c| c.iter().map(|(_, clip)| clip.len()).sum()).unwrap_or(0);
    Stats { playing: PLAYING.load(std::sync::atomic::Ordering::Relaxed), clip_cache_bytes }
}

// Downloaded clips, least recently used first, within `CLIP_CACHE_BYTES`.
static CLIPS: Mutex<VecDeque<(String, Arc<[u8]>)>> = Mutex::new(VecDeque::new());

fn cached_clip(url: &str) -> Option<Arc<[u8]>> {
    let mut clips = CLIPS.lock().ok()?;
    let index = clips.iter().position(|(u, _)| u == url)?;
    let entry = clips.remove(index)?;
    let clip = entry.1.clone();
    clips.push_back(entry);
    Some(clip)
}

fn cache_clip(url: String, clip: Arc<[u8]>) {
    let Ok(mut clips) = CLIPS.lock() else { return };
    clips.retain(|(u, _)| *u != url);
    clips.push_back((url, clip));
    while clips.iter().map(|(_, c)| c.len()).sum::<usize>() > CLIP_CACHE_BYTES && clips.len() > 1 {
        clips.pop_front();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Collects the first `count` pictures the looping decoder produces.
    fn first(clip: &'static [u8], count: usize) -> Vec<Picture> {
        let pictures = decode_looping(Arc::from(clip));
        (0..count).map(|_| pictures.recv_timeout(Duration::from_secs(5)).expect("a picture")).collect()
    }

    // A Discord-style GIF: H.264 in MP4, 480 wide, 15 frames over one second; it loops seamlessly.
    #[test]
    fn decodes_scaled_pictures_and_loops() {
        let pictures = first(include_bytes!("../testdata/gifv.mp4"), 31);
        let size = |p: &Picture| p.image.size(0);
        assert_eq!((size(&pictures[0]).width.0, size(&pictures[0]).height.0), (400, 225));
        assert!(pictures.windows(2).all(|w| w[1].at > w[0].at), "times only move forward, across loops too");
        assert_eq!(pictures[15].at, Duration::from_secs(1), "the second loop starts where the first ended");
        assert!(pictures[30].at >= Duration::from_secs(2));
    }

    // Reordered B-frames used to make the decoder bail halfway; every frame still comes out, in order.
    #[test]
    fn b_frame_clips_play_every_frame() {
        let pictures = first(include_bytes!("../testdata/gifv-bframes.mp4"), 16);
        assert!(pictures.windows(2).all(|w| w[1].at > w[0].at));
        assert_eq!(pictures[15].at, Duration::from_secs(1));
    }

    // A short clip is decoded once: the second loop replays the same pictures.
    #[test]
    fn short_clips_replay_instead_of_decoding_again() {
        let pictures = first(include_bytes!("../testdata/gifv.mp4"), 30);
        assert!(Arc::ptr_eq(&pictures[0].image, &pictures[15].image), "loop two reuses loop one's pictures");
        assert_eq!(pictures[15].at, Duration::from_secs(1));
    }

    // The decoder stays a few pictures ahead instead of decoding the whole clip.
    #[test]
    fn decoder_holds_only_a_few_pictures() {
        let pictures = decode_looping(Arc::from(&include_bytes!("../testdata/gifv.mp4")[..]));
        std::thread::sleep(Duration::from_millis(300));
        // Draining lets the decoder slip in one or two more; a whole loop would be 15.
        let held = pictures.try_iter().count();
        assert!((AHEAD..AHEAD + 3).contains(&held), "bounded by the channel, not the clip: {held}");
    }

    #[test]
    fn clip_cache_keeps_recent_clips_within_its_cap() {
        let big: Arc<[u8]> = Arc::from(vec![0u8; CLIP_CACHE_BYTES / 2 + 1]);
        cache_clip("test://a".into(), big.clone());
        cache_clip("test://b".into(), big.clone());
        assert!(cached_clip("test://a").is_none(), "the older clip made room");
        assert!(cached_clip("test://b").is_some());
    }
}
