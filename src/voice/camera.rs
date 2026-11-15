// Webcam capture via `nokhwa` (AVFoundation on macOS, Media Foundation on Windows, V4L2 on
// Linux). Frames come out as BGRA scaled to fit the camera stream size.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
};

use nokhwa::{
    Camera,
    pixel_format::RgbAFormat,
    utils::{ApiBackend, CameraIndex, RequestedFormat, RequestedFormatType},
};

use super::{audio::Device, video::VideoFrame};

// How long to wait for the camera permission prompt (macOS only).
#[cfg(target_os = "macos")]
const PERMISSION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

pub fn cameras() -> Vec<Device> {
    let found = nokhwa::query(ApiBackend::Auto).unwrap_or_default();
    found.iter().enumerate().map(|(i, info)| Device { id: index_id(info.index()), name: info.human_name(), default: i == 0 }).collect()
}

fn index_id(index: &CameraIndex) -> String {
    match index {
        CameraIndex::Index(i) => i.to_string(),
        CameraIndex::String(s) => s.clone(),
    }
}

fn parse_index(id: &str) -> CameraIndex {
    id.parse().map(CameraIndex::Index).unwrap_or_else(|_| CameraIndex::String(id.to_owned()))
}

// Stops the camera when dropped.
pub struct CameraCapture {
    stop: Arc<AtomicBool>,
}

impl Drop for CameraCapture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

// Opens camera `id` (asking for camera permission on macOS) and calls `on_frame` per frame.
pub fn start(id: &str, max_size: (u32, u32), on_frame: impl Fn(VideoFrame) + Send + 'static) -> Result<CameraCapture, String> {
    let stop = Arc::new(AtomicBool::new(false));
    let (ready_tx, ready_rx) = mpsc::channel();
    let (index, flag) = (parse_index(id), stop.clone());
    thread::Builder::new()
        .name("rustcord-camera".into())
        .spawn(move || {
            let camera = ensure_permission().and_then(|()| open(index));
            let mut camera = match camera {
                Ok(camera) => camera,
                Err(e) => return drop(ready_tx.send(Err(e))),
            };
            let _ = ready_tx.send(Ok(()));
            while !flag.load(Ordering::Relaxed) {
                let Ok(buffer) = camera.frame() else { continue };
                let Ok(image) = buffer.decode_image::<RgbAFormat>() else { continue };
                on_frame(fit_bgra(image.as_raw(), image.width(), image.height(), max_size));
            }
            let _ = camera.stop_stream();
        })
        .map_err(|e| e.to_string())?;
    ready_rx.recv().map_err(|e| e.to_string())??;
    Ok(CameraCapture { stop })
}

fn open(index: CameraIndex) -> Result<Camera, String> {
    let format = RequestedFormat::new::<RgbAFormat>(RequestedFormatType::AbsoluteHighestFrameRate);
    let mut camera = Camera::new(index, format).map_err(|e| e.to_string())?;
    camera.open_stream().map_err(|e| e.to_string())?;
    Ok(camera)
}

#[cfg(target_os = "macos")]
fn ensure_permission() -> Result<(), String> {
    if nokhwa::nokhwa_check() {
        return Ok(());
    }
    let (tx, rx) = mpsc::channel();
    nokhwa::nokhwa_initialize(move |granted| {
        let _ = tx.send(granted);
    });
    match rx.recv_timeout(PERMISSION_TIMEOUT) {
        Ok(true) => Ok(()),
        _ => Err("Camera permission was not granted".into()),
    }
}

#[cfg(not(target_os = "macos"))]
fn ensure_permission() -> Result<(), String> {
    Ok(())
}

// RGBA → BGRA, nearest-neighbour scaled to fit `max` with even sides (H.264 needs even sizes).
fn fit_bgra(rgba: &[u8], width: u32, height: u32, (max_w, max_h): (u32, u32)) -> VideoFrame {
    let scale = (max_w as f64 / width as f64).min(max_h as f64 / height as f64).min(1.0);
    let even = |v: f64| ((v as u32) & !1).max(2);
    let (out_w, out_h) = (even(width as f64 * scale), even(height as f64 * scale));
    let mut bgra = Vec::with_capacity((out_w * out_h * 4) as usize);
    for y in 0..out_h {
        let src_y = (y as u64 * height as u64 / out_h as u64) as usize;
        for x in 0..out_w {
            let src_x = (x as u64 * width as u64 / out_w as u64) as usize;
            let i = (src_y * width as usize + src_x) * 4;
            let px = rgba.get(i..i + 4).unwrap_or(&[0, 0, 0, 255]);
            bgra.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
        }
    }
    VideoFrame { width: out_w, height: out_h, bgra }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fits_and_swaps_channels() {
        let rgba: Vec<u8> = [10u8, 20, 30, 255].repeat(1920 * 1080);
        let frame = fit_bgra(&rgba, 1920, 1080, (1280, 720));
        assert_eq!((frame.width, frame.height), (1280, 720));
        assert_eq!(&frame.bgra[..4], &[30, 20, 10, 255]);
        let small = fit_bgra(&[1, 2, 3, 4].repeat(640 * 480), 640, 480, (1280, 720));
        assert_eq!((small.width, small.height), (640, 480), "never upscaled");
        assert_eq!(parse_index("2"), CameraIndex::Index(2));
    }
}
