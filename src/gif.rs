// Discord's GIFs (Tenor, Klipy, Giphy) arrive as silent looping MP4s, which Discord plays inline.
// gpui animates multi-frame images, so each MP4 is decoded once (OpenH264) into one, scaled to the
// size it is shown at, and cached as a gpui asset.
use std::{io::Cursor, sync::Arc};

use gpui_kit::{App, Asset, RenderImage};
use image::{Delay, Frame, RgbaImage, imageops::FilterType};
use openh264::{decoder::Decoder, formats::YUVSource};

// Wide enough for Discord's largest inline GIF; keeps a 200-frame GIF around 100 MB at worst.
const MAX_WIDTH: u32 = 400;
const MAX_FRAMES: u32 = 300;

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
    let err = |e: mp4::Error| e.to_string();
    let mut reader = mp4::Mp4Reader::read_header(Cursor::new(bytes), bytes.len() as u64).map_err(err)?;
    let track = reader.tracks().values().find(|t| matches!(t.media_type(), Ok(mp4::MediaType::H264))).ok_or("no H.264 track")?;
    let (id, timescale, count) = (track.track_id(), track.timescale().max(1), track.sample_count());
    let mut header = Vec::new();
    for set in [track.sequence_parameter_set().map_err(err)?, track.picture_parameter_set().map_err(err)?] {
        header.extend_from_slice(&[0, 0, 0, 1]);
        header.extend_from_slice(set);
    }
    let mut decoder = Decoder::new().map_err(|e| e.to_string())?;
    // The decoder can hold pictures back (reordered frames), so whatever it still holds is flushed
    // at the end. It can't decode B-frames (High-profile clips have them): those are skipped, and
    // the clip's full length is spread over the frames that did decode, so it plays at true speed.
    let mut total: u64 = 0;
    let mut pictures = Vec::new();
    for sample_id in 1..=count.min(MAX_FRAMES) {
        let Some(sample) = reader.read_sample(id, sample_id).map_err(err)? else { continue };
        total += u64::from(sample.duration);
        let mut unit = if sample_id == 1 { header.clone() } else { Vec::new() };
        unit.extend(annex_b(&sample.bytes));
        if let Ok(Some(yuv)) = decoder.decode(&unit) {
            pictures.extend(picture(&yuv));
        }
    }
    for yuv in decoder.flush_remaining().unwrap_or_default() {
        pictures.extend(picture(&yuv));
    }
    let ms = (total * 1000 / u64::from(timescale) / pictures.len().max(1) as u64).clamp(20, 1000) as u32;
    let frames: Vec<Frame> = pictures.into_iter().map(|p| Frame::from_parts(p, 0, 0, Delay::from_numer_denom_ms(ms, 1))).collect();
    if frames.is_empty() {
        return Err("no frames decoded".into());
    }
    Ok(frames)
}

fn picture(yuv: &openh264::decoder::DecodedYUV) -> Option<RgbaImage> {
    let (w, h) = yuv.dimensions();
    let mut pixels = vec![0u8; yuv.rgba8_len()];
    yuv.write_rgba8(&mut pixels);
    let mut picture = RgbaImage::from_raw(w as u32, h as u32, pixels)?;
    if picture.width() > MAX_WIDTH {
        let height = picture.height() * MAX_WIDTH / picture.width();
        picture = image::imageops::resize(&picture, MAX_WIDTH, height.max(1), FilterType::Triangle);
    }
    // gpui draws RenderImage pixels as BGRA.
    picture.pixels_mut().for_each(|p| p.0.swap(0, 2));
    Some(picture)
}

// MP4 stores NAL units behind 4-byte big-endian lengths; the decoder wants start codes.
fn annex_b(mut avcc: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(avcc.len() + 16);
    while avcc.len() >= 4 {
        let len = u32::from_be_bytes([avcc[0], avcc[1], avcc[2], avcc[3]]) as usize;
        let Some(unit) = avcc.get(4..4 + len) else { break };
        out.extend_from_slice(&[0, 0, 0, 1]);
        out.extend_from_slice(unit);
        avcc = &avcc[4 + len..];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // A Discord-style GIF: H.264 in MP4, 480 wide, 15 frames over one second.
    #[test]
    fn decodes_mp4_into_scaled_frames() {
        let frames = decode(include_bytes!("../testdata/gifv.mp4")).expect("decodes");
        assert_eq!(frames.len(), 15);
        assert_eq!((frames[0].buffer().width(), frames[0].buffer().height()), (400, 225));
        assert_eq!(frames[0].delay().numer_denom_ms(), (66, 1));
    }

    // With B-frames only some frames decode, but the clip still lasts its full second.
    #[test]
    fn b_frame_clips_keep_their_length() {
        let frames = decode(include_bytes!("../testdata/gifv-bframes.mp4")).expect("decodes");
        assert!(frames.len() >= 8, "{}", frames.len());
        let total: u32 = frames.iter().map(|f| f.delay().numer_denom_ms().0).sum();
        assert!((900..=1100).contains(&total), "{total} ms");
    }

    #[test]
    fn avcc_to_annex_b() {
        assert_eq!(annex_b(&[0, 0, 0, 2, 9, 9, 0, 0, 0, 1, 7]), [0, 0, 0, 1, 9, 9, 0, 0, 0, 1, 7]);
        assert_eq!(annex_b(&[0, 0, 0, 9, 1]), Vec::<u8>::new());
    }
}
