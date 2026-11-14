// Discord's GIFs (Tenor, Klipy, Giphy) arrive as silent looping MP4s, which Discord plays inline.
// gpui animates multi-frame images, so each MP4 is decoded once (OpenH264) into one, scaled to the
// size it is shown at, and cached as a gpui asset.
use std::{ops::ControlFlow, sync::Arc, time::Duration};

use gpui_kit::{App, Asset, RenderImage};
use image::{Delay, Frame};

use crate::h264::{Track, bgra};

// Wide enough for Discord's largest inline GIF; keeps a 200-frame GIF around 100 MB at worst.
const MAX_WIDTH: u32 = 400;
const MAX_FRAMES: usize = 300;
// gpui would spin on a zero delay; no GIF shows a frame for less.
const MIN_DELAY: Duration = Duration::from_millis(10);

pub enum Gif {}

impl Asset for Gif {
    type Source = String;
    type Output = Option<Arc<RenderImage>>;

    // Not `async fn`: the future must not borrow the `App` it's given.
    #[allow(clippy::manual_async_fn)]
    fn load(url: String, _: &mut App) -> impl Future<Output = Self::Output> + Send + 'static {
        async move {
            let bytes = crate::discord::runtime()
                .spawn(async move {
                    let res = reqwest::get(&url).await.ok()?;
                    res.status().is_success().then_some(())?;
                    res.bytes().await.ok()
                })
                .await
                .ok()??;
            let frames = crate::discord::runtime().spawn_blocking(move || decode(&bytes)).await.ok()?.ok()?;
            Some(Arc::new(RenderImage::new(frames)))
        }
    }
}

// An img() source that plays the GIF once decoded; None while it loads, an error if it can't.
pub fn source(url: String) -> gpui_kit::ImageSource {
    gpui_kit::ImageSource::Custom(Arc::new(move |window, cx| {
        let loaded = window.use_asset::<Gif>(&url, cx)?;
        Some(loaded.ok_or_else(|| gpui_kit::ImageCacheError::Asset("GIF could not be decoded".into())))
    }))
}

pub fn decode(bytes: &[u8]) -> Result<Vec<Frame>, String> {
    let mut track = Track::open(bytes)?;
    let mut pictures = Vec::new();
    track.decode(0, |at, yuv| {
        pictures.extend(bgra(yuv, MAX_WIDTH).map(|p| (at, p)));
        if pictures.len() >= MAX_FRAMES { ControlFlow::Break(()) } else { ControlFlow::Continue(()) }
    })?;
    if pictures.is_empty() {
        return Err("no frames decoded".into());
    }
    // Each picture stays up until the next one's time; the last until the clip ends.
    let ends: Vec<Duration> = pictures.iter().skip(1).map(|(at, _)| *at).chain([track.duration()]).collect();
    Ok(pictures
        .into_iter()
        .zip(ends)
        .map(|((at, picture), end)| Frame::from_parts(picture, 0, 0, Delay::from_saturating_duration(end.saturating_sub(at).max(MIN_DELAY))))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn total_ms(frames: &[Frame]) -> f64 {
        frames.iter().map(|f| f.delay().numer_denom_ms()).map(|(n, d)| f64::from(n) / f64::from(d)).sum()
    }

    // A Discord-style GIF: H.264 in MP4, 480 wide, 15 frames over one second.
    #[test]
    fn decodes_mp4_into_scaled_frames() {
        let frames = decode(include_bytes!("../testdata/gifv.mp4")).expect("decodes");
        assert_eq!(frames.len(), 15);
        assert_eq!((frames[0].buffer().width(), frames[0].buffer().height()), (400, 225));
        let (n, d) = frames[0].delay().numer_denom_ms();
        assert!((f64::from(n) / f64::from(d) - 66.67).abs() < 0.1, "{n}/{d}");
        assert!((total_ms(&frames) - 1000.).abs() < 1., "{}", total_ms(&frames));
    }

    // Reordered B-frames used to make the decoder bail halfway; now every frame plays, at true speed.
    #[test]
    fn b_frame_clips_play_every_frame() {
        let frames = decode(include_bytes!("../testdata/gifv-bframes.mp4")).expect("decodes");
        assert_eq!(frames.len(), 15);
        assert!((total_ms(&frames) - 1000.).abs() < 1., "{}", total_ms(&frames));
    }
}
