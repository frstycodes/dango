// The image cache every `img()` in the window loads through (avatars, emoji, attachments, embeds,
// GIF stills). gpui's default cache keeps every image ever shown, decoded, in RAM and on the GPU
// until the app quits; this one lets go of images that are no longer drawn.
//
// An image is drawn by loading it, every frame it's on screen, so each load stamps the current frame.
// An image not drawn for `KEEP_FRAMES` frames and `KEEP_FOR` is freed (RAM and GPU). Both conditions
// matter: a window that isn't redrawing draws nothing, so time alone would evict what's on screen,
// and a burst of frames (scrolling) shouldn't drop what was just scrolled past. Reloading an evicted
// image skips the network when its file is still in discord::Images' download cache.
//
// `clear()` drops everything; the app calls it when it goes out of sight (see main.rs).
use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
    sync::Arc,
    time::{Duration, Instant},
};

use gpui_kit::{App, AppContext, Entity, Global, ImageCache, ImageCacheError, ImageCacheItem, RenderImage, Resource, Window};

const KEEP_FRAMES: u64 = 120;
const KEEP_FOR: Duration = Duration::from_secs(10);
// How often eviction looks through the cache.
const SWEEP_EVERY: Duration = Duration::from_secs(2);

pub struct Images {
    items: HashMap<u64, Item>,
    frame: u64,
    swept_at: Instant,
}

struct Item {
    load: ImageCacheItem,
    drawn_frame: u64,
    drawn_at: Instant,
}

struct Shared(Entity<Images>);

impl Global for Shared {}

// The app's image cache, created on first use.
pub fn cache(cx: &mut App) -> Entity<Images> {
    if let Some(shared) = cx.try_global::<Shared>() {
        return shared.0.clone();
    }
    let images = cx.new(|_| Images { items: HashMap::new(), frame: 0, swept_at: Instant::now() });
    cx.set_global(Shared(images.clone()));
    images
}

pub struct Stats {
    pub images: usize,
    pub decoded_bytes: usize,
}

// The last sweep's numbers, for the memory report (which runs off the UI thread).
static LAST: std::sync::Mutex<Stats> = std::sync::Mutex::new(Stats { images: 0, decoded_bytes: 0 });

pub fn last_stats() -> Stats {
    LAST.lock().map(|s| Stats { images: s.images, decoded_bytes: s.decoded_bytes }).unwrap_or(Stats { images: 0, decoded_bytes: 0 })
}

impl Images {
    fn publish(&self) {
        if let Ok(mut last) = LAST.lock() {
            *last = self.stats();
        }
    }
}

impl Images {
    // Called once per window render, before children load their images.
    pub fn next_frame(&mut self, cx: &mut App) {
        self.frame += 1;
        if self.swept_at.elapsed() >= SWEEP_EVERY {
            self.swept_at = Instant::now();
            self.evict(cx);
            self.publish();
        }
    }

    fn evict(&mut self, cx: &mut App) {
        let frame = self.frame;
        let stale: Vec<u64> = self
            .items
            .iter()
            .filter(|(_, item)| frame.saturating_sub(item.drawn_frame) > KEEP_FRAMES && item.drawn_at.elapsed() > KEEP_FOR)
            .map(|(key, _)| *key)
            .collect();
        for key in stale {
            if let Some(item) = self.items.remove(&key) {
                free(item, cx);
            }
        }
    }

    // Frees every image; whatever is on screen loads again (from the download cache) when next drawn.
    pub fn clear(&mut self, cx: &mut App) {
        for (_, item) in std::mem::take(&mut self.items) {
            free(item, cx);
        }
        self.publish();
    }

    // Drops a load still in flight, which stops its download. Finished images are left to eviction.
    pub fn cancel(&mut self, url: &str) {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        Resource::Uri(url.to_owned().into()).hash(&mut hasher);
        let key = hasher.finish();
        if self.items.get(&key).is_some_and(|item| item.load.get().is_none()) {
            self.items.remove(&key);
        }
    }

    pub fn stats(&self) -> Stats {
        let decoded_bytes = self
            .items
            .values()
            .filter_map(|item| item.load.get()?.ok())
            .map(|image| (0..image.frame_count()).map(|i| image.as_bytes(i).map_or(0, <[u8]>::len)).sum::<usize>())
            .sum();
        Stats { images: self.items.len(), decoded_bytes }
    }
}

fn free(item: Item, cx: &mut App) {
    if let Some(Ok(image)) = item.load.get() {
        // Deferred: gpui takes a window out of `App` while it renders or runs its callbacks, and
        // `drop_image(_, None)` only reaches windows still in `App`, so its atlas pages would never be freed.
        cx.defer(move |cx| cx.drop_image(image, None));
    }
}

impl ImageCache for Images {
    fn load(&mut self, resource: &Resource, window: &mut Window, cx: &mut App) -> Option<Result<Arc<RenderImage>, ImageCacheError>> {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        resource.hash(&mut hasher);
        let frame = self.frame;
        let item = self.items.entry(hasher.finish()).or_insert_with(|| Item { load: ImageCacheItem::new(resource, cx), drawn_frame: frame, drawn_at: Instant::now() });
        item.drawn_frame = frame;
        item.drawn_at = Instant::now();
        item.load.use_image(window)
    }
}
