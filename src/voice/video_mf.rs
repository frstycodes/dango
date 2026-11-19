// Hardware H.264 encoding on Windows through a Media Foundation encoder MFT: the GPU vendor's
// (NVENC, Quick Sync, AMF) when one is installed. video.rs falls back to OpenH264 when none is
// found or it fails. Output matches what OpenH264 gives the rest of the pipeline: Annex-B access
// units, Constrained Baseline, no B-frames, SPS/PPS ahead of every IDR, and a keyframe on request.
//
// Frames are converted BGRA → NV12 on the CPU and handed over in system memory, which every
// hardware MFT accepts. ponytail: keep frames on the GPU (Windows.Graphics.Capture texture →
// D3D11 video processor → MFT with a DXGI device manager) if CPU use while streaming matters.
//
// Hardware MFTs are asynchronous: they ask for input and announce output through events. `encode`
// hides that behind the same one-in, at-most-one-out call as OpenH264.

use std::{collections::VecDeque, ffi::c_void, mem::ManuallyDrop, time::{Duration, Instant}};

use windows::{
    Win32::{
        Foundation::VARIANT_TRUE,
        Graphics::Dxgi::{CreateDXGIFactory1, DXGI_GPU_PREFERENCE_HIGH_PERFORMANCE, IDXGIAdapter1, IDXGIFactory6},
        Media::MediaFoundation::*,
        System::{
            Com::{COINIT_MULTITHREADED, CoInitializeEx, CoTaskMemFree},
            Variant::{VARIANT, VT_BOOL, VT_UI4},
        },
    },
    core::{GUID, Interface, PWSTR},
};

use super::video::VideoFrame;

const HNS_PER_SECOND: i64 = 10_000_000;
// How long one encode waits for the MFT to hand back the frame it was given.
const OUTPUT_WAIT: Duration = Duration::from_millis(40);

pub struct MfEncoder {
    transform: IMFTransform,
    codec: Option<ICodecAPI>,
    events: Option<IMFMediaEventGenerator>,
    pub name: String,
    pub width: u32,
    pub height: u32,
    fps: u32,
    frame_index: i64,
    // Async MFTs grant input slots and announce output through events; these count unclaimed ones.
    input_credits: u32,
    output_ready: u32,
    pending: VecDeque<Vec<u8>>,
    // SPS/PPS from the output type, prepended to an IDR that arrives without them.
    sequence_header: Vec<u8>,
    nv12: Vec<u8>,
}

impl MfEncoder {
    pub fn new(width: u32, height: u32, fps: u32, bitrate: u32) -> Result<Self, String> {
        // SAFETY: plain COM / Media Foundation calls on this thread; every pointer passed in lives
        // for the call, and the enumerated activates are owned Vec entries.
        unsafe {
            // S_FALSE / RPC_E_CHANGED_MODE just mean COM is already set up on this thread.
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            MFStartup(MF_VERSION, MFSTARTUP_LITE).map_err(|e| format!("MFStartup: {e}"))?;
            match Self::open(width, height, fps, bitrate) {
                Ok(encoder) => Ok(encoder),
                Err(e) => {
                    let _ = MFShutdown();
                    Err(e)
                }
            }
        }
    }

    unsafe fn open(width: u32, height: u32, fps: u32, bitrate: u32) -> Result<Self, String> {
        let err = |what: &'static str| move |e: windows::core::Error| format!("{what}: {e}");
        // On a laptop with an iGPU and a dedicated GPU, the unfiltered list may lead with the iGPU's
        // encoder; ask for the high-performance adapter's first and take any adapter's otherwise.
        let on_preferred = unsafe { high_performance_adapter() }.and_then(|(luid, adapter)| {
            let found = unsafe { hardware_encoders(Some(luid)) }.ok()?;
            (!found.is_empty()).then_some((found, adapter))
        });
        let (activates, adapter) = match on_preferred {
            Some(found) => found,
            None => (unsafe { hardware_encoders(None) }.map_err(err("MFTEnumEx"))?, "any adapter".to_owned()),
        };
        let activate = activates.first().ok_or("No hardware H.264 encoder")?;
        let name = unsafe { friendly_name(activate) }.unwrap_or_else(|| "hardware H.264 encoder".into());
        if super::debug() {
            eprintln!("voice: hardware encoder {name} on {adapter}");
        }
        let transform: IMFTransform = unsafe { activate.ActivateObject() }.map_err(err("ActivateObject"))?;

        let attributes = unsafe { transform.GetAttributes() }.ok();
        let is_async = attributes.as_ref().is_some_and(|a| unsafe { a.GetUINT32(&MF_TRANSFORM_ASYNC) }.unwrap_or(0) != 0);
        if let Some(attributes) = &attributes {
            if is_async {
                unsafe { attributes.SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1) }.map_err(err("unlock async MFT"))?;
            }
            let _ = unsafe { attributes.SetUINT32(&MF_LOW_LATENCY, 1) };
        }
        let codec = transform.cast::<ICodecAPI>().ok();
        if let Some(codec) = &codec {
            // Each knob is best effort: vendors support different subsets. Not verified per vendor.
            let _ = unsafe { codec.SetValue(&CODECAPI_AVLowLatencyMode, &variant_bool(true)) };
            let _ = unsafe { codec.SetValue(&CODECAPI_AVEncCommonRateControlMode, &variant_u32(eAVEncCommonRateControlMode_CBR.0 as u32)) };
            let _ = unsafe { codec.SetValue(&CODECAPI_AVEncCommonMeanBitRate, &variant_u32(bitrate)) };
            let _ = unsafe { codec.SetValue(&CODECAPI_AVEncMPVDefaultBPictureCount, &variant_u32(0)) };
            // A keyframe every few seconds lets late viewers start even if keyframe requests are lost.
            let _ = unsafe { codec.SetValue(&CODECAPI_AVEncMPVGOPSize, &variant_u32(fps * 3)) };
        }

        // Output type first (encoders require it), Constrained Baseline, then plain Baseline for
        // encoders that only list that; without B-frames the two produce the same stream.
        let mut profile_set = Err("no profile accepted".to_owned());
        for profile in [eAVEncH264VProfile_ConstrainedBase.0, eAVEncH264VProfile_Base.0] {
            let out = unsafe { video_type(MFVideoFormat_H264, width, height, fps) }.map_err(err("output type"))?;
            unsafe {
                out.SetUINT32(&MF_MT_AVG_BITRATE, bitrate).map_err(err("bitrate"))?;
                out.SetUINT32(&MF_MT_MPEG2_PROFILE, profile as u32).map_err(err("profile"))?;
            }
            profile_set = unsafe { transform.SetOutputType(0, &out, 0) }.map_err(|e| format!("SetOutputType: {e}"));
            if profile_set.is_ok() {
                break;
            }
        }
        profile_set?;
        let input_type = unsafe { video_type(MFVideoFormat_NV12, width, height, fps) }.map_err(err("input type"))?;
        unsafe { transform.SetInputType(0, &input_type, 0) }.map_err(err("SetInputType"))?;
        let sequence_header = unsafe { sequence_header(&transform) };
        unsafe {
            transform.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0).map_err(err("begin streaming"))?;
            transform.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0).map_err(err("start of stream"))?;
        }
        let events = if is_async { Some(transform.cast::<IMFMediaEventGenerator>().map_err(err("event generator"))?) } else { None };
        Ok(Self {
            transform,
            codec,
            events,
            name,
            width,
            height,
            fps: fps.max(1),
            frame_index: 0,
            input_credits: 0,
            output_ready: 0,
            pending: VecDeque::new(),
            sequence_header,
            nv12: Vec::new(),
        })
    }

    pub fn force_keyframe(&mut self) {
        if let Some(codec) = &self.codec {
            // SAFETY: a COM call with a VARIANT that lives for the call.
            let _ = unsafe { codec.SetValue(&CODECAPI_AVEncVideoForceKeyFrame, &variant_u32(1)) };
        }
    }

    /// One Annex-B access unit, or None while the encoder is still working on it. Err means the
    /// hardware path broke and the caller should switch to software.
    pub fn encode(&mut self, frame: &VideoFrame) -> Result<Option<Vec<u8>>, String> {
        bgra_to_nv12(frame, &mut self.nv12);
        let duration = HNS_PER_SECOND / i64::from(self.fps);
        // SAFETY: COM calls on objects this struct owns; the media buffer is locked only while
        // the NV12 bytes are copied in, and unlocked before the sample is handed over.
        unsafe {
            let sample = MFCreateSample().map_err(|e| e.to_string())?;
            let buffer = MFCreateMemoryBuffer(self.nv12.len() as u32).map_err(|e| e.to_string())?;
            let mut data = std::ptr::null_mut();
            buffer.Lock(&mut data, None, None).map_err(|e| e.to_string())?;
            std::ptr::copy_nonoverlapping(self.nv12.as_ptr(), data, self.nv12.len());
            buffer.Unlock().map_err(|e| e.to_string())?;
            buffer.SetCurrentLength(self.nv12.len() as u32).map_err(|e| e.to_string())?;
            sample.AddBuffer(&buffer).map_err(|e| e.to_string())?;
            sample.SetSampleTime(self.frame_index * duration).map_err(|e| e.to_string())?;
            sample.SetSampleDuration(duration).map_err(|e| e.to_string())?;
            self.frame_index += 1;

            if self.events.is_some() {
                self.encode_async(&sample)?;
            } else {
                self.transform.ProcessInput(0, &sample, 0).map_err(|e| format!("ProcessInput: {e}"))?;
                while let Some(unit) = self.take_output()? {
                    self.pending.push_back(unit);
                }
            }
        }
        Ok(self.pending.pop_front())
    }

    unsafe fn encode_async(&mut self, sample: &IMFSample) -> Result<(), String> {
        // Wait for an input slot, feed the frame, then give the encoder a moment to return it.
        let deadline = Instant::now() + OUTPUT_WAIT * 2;
        while self.input_credits == 0 {
            if !unsafe { self.pump_events(deadline) }? {
                // No slot in time: drop this frame rather than stall the stream.
                return Ok(());
            }
        }
        self.input_credits -= 1;
        unsafe { self.transform.ProcessInput(0, sample, 0) }.map_err(|e| format!("ProcessInput: {e}"))?;
        let deadline = Instant::now() + OUTPUT_WAIT;
        loop {
            while self.output_ready > 0 {
                self.output_ready -= 1;
                if let Some(unit) = unsafe { self.take_output() }? {
                    self.pending.push_back(unit);
                }
            }
            if !self.pending.is_empty() || !unsafe { self.pump_events(deadline) }? {
                return Ok(());
            }
        }
    }

    // Drains queued MFT events into the counters; waits (polling) until at least one arrives or the
    // deadline passes. Returns whether any event arrived.
    unsafe fn pump_events(&mut self, deadline: Instant) -> Result<bool, String> {
        let Some(events) = &self.events else { return Ok(false) };
        let mut got = false;
        loop {
            match unsafe { events.GetEvent(MF_EVENT_FLAG_NO_WAIT) } {
                Ok(event) => {
                    got = true;
                    let kind = unsafe { event.GetType() }.unwrap_or(0) as i32;
                    if kind == METransformNeedInput.0 {
                        self.input_credits += 1;
                    } else if kind == METransformHaveOutput.0 {
                        self.output_ready += 1;
                    }
                }
                // MF_E_NO_EVENTS_AVAILABLE: nothing queued right now.
                Err(_) if got => return Ok(true),
                Err(_) if Instant::now() >= deadline => return Ok(false),
                Err(_) => std::thread::sleep(Duration::from_millis(1)),
            }
        }
    }

    // One ProcessOutput call: Some(unit), or None when the encoder needs more input.
    unsafe fn take_output(&mut self) -> Result<Option<Vec<u8>>, String> {
        loop {
            let info = unsafe { self.transform.GetOutputStreamInfo(0) }.map_err(|e| e.to_string())?;
            let provides = info.dwFlags & (MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32) != 0;
            let sample = if provides {
                None
            } else {
                let sample = unsafe { MFCreateSample() }.map_err(|e| e.to_string())?;
                let buffer = unsafe { MFCreateMemoryBuffer(info.cbSize.max(self.width * self.height * 3 / 2)) }.map_err(|e| e.to_string())?;
                unsafe { sample.AddBuffer(&buffer) }.map_err(|e| e.to_string())?;
                Some(sample)
            };
            let mut buffers = [MFT_OUTPUT_DATA_BUFFER { dwStreamID: 0, pSample: ManuallyDrop::new(sample), dwStatus: 0, pEvents: ManuallyDrop::new(None) }];
            let mut status = 0u32;
            let result = unsafe { self.transform.ProcessOutput(0, &mut buffers, &mut status) };
            let [buffer] = buffers;
            let sample = ManuallyDrop::into_inner(buffer.pSample);
            drop(ManuallyDrop::into_inner(buffer.pEvents));
            match result {
                Ok(()) => {
                    let Some(sample) = sample else { return Ok(None) };
                    return Ok(Some(self.annex_b(unsafe { sample_bytes(&sample) }?)));
                }
                Err(e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => return Ok(None),
                Err(e) if e.code() == MF_E_TRANSFORM_STREAM_CHANGE => {
                    // The encoder revised its output type (e.g. final SPS); accept it and retry.
                    let revised = unsafe { self.transform.GetOutputAvailableType(0, 0) }.map_err(|e| e.to_string())?;
                    unsafe { self.transform.SetOutputType(0, &revised, 0) }.map_err(|e| e.to_string())?;
                    self.sequence_header = unsafe { sequence_header(&self.transform) };
                }
                Err(e) => return Err(format!("ProcessOutput: {e}")),
            }
        }
    }

    // Hardware encoders already emit Annex-B. Drop access unit delimiters (9) and CBR filler (12),
    // which only cost bandwidth over RTP (AMD's encoder sends both), and make sure every IDR carries
    // SPS and PPS so a viewer can start from any keyframe.
    fn annex_b(&self, unit: Vec<u8>) -> Vec<u8> {
        let kept = super::video::without_nal_types(&unit, &[9, 12]);
        let types = super::video::nal_types(&kept);
        if types.contains(&5) && !types.contains(&7) && !self.sequence_header.is_empty() {
            return [self.sequence_header.as_slice(), &kept].concat();
        }
        kept
    }
}

impl Drop for MfEncoder {
    fn drop(&mut self) {
        // SAFETY: matches the MFStartup in `new`; the transform is released with this struct.
        unsafe {
            let _ = self.transform.ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0);
            let _ = MFShutdown();
        }
    }
}

// Hardware H.264 encoders taking NV12, best first; limited to one adapter when its LUID is given.
unsafe fn hardware_encoders(adapter_luid: Option<u64>) -> windows::core::Result<Vec<IMFActivate>> {
    let input = MFT_REGISTER_TYPE_INFO { guidMajorType: MFMediaType_Video, guidSubtype: MFVideoFormat_NV12 };
    let output = MFT_REGISTER_TYPE_INFO { guidMajorType: MFMediaType_Video, guidSubtype: MFVideoFormat_H264 };
    let flags = MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_SORTANDFILTER;
    let mut list: *mut Option<IMFActivate> = std::ptr::null_mut();
    let mut count = 0u32;
    match adapter_luid {
        Some(luid) => {
            let mut attributes = None;
            unsafe { MFCreateAttributes(&mut attributes, 1) }?;
            if let Some(attributes) = &attributes {
                unsafe { attributes.SetUINT64(&MFT_ENUM_ADAPTER_LUID, luid) }?;
            }
            unsafe { MFTEnum2(MFT_CATEGORY_VIDEO_ENCODER, flags, Some(&input), Some(&output), attributes.as_ref(), &mut list, &mut count) }?;
        }
        None => unsafe { MFTEnumEx(MFT_CATEGORY_VIDEO_ENCODER, flags, Some(&input), Some(&output), &mut list, &mut count) }?,
    }
    if list.is_null() {
        return Ok(Vec::new());
    }
    let found = unsafe { std::slice::from_raw_parts_mut(list, count as usize) }.iter_mut().filter_map(Option::take).collect();
    unsafe { CoTaskMemFree(Some(list as *const c_void)) };
    Ok(found)
}

// The adapter Windows ranks first for high performance (the dedicated GPU when there is one), as
// (LUID packed the way MFT_ENUM_ADAPTER_LUID wants it, description). None before Windows 10 1803.
unsafe fn high_performance_adapter() -> Option<(u64, String)> {
    let factory: IDXGIFactory6 = unsafe { CreateDXGIFactory1() }.ok()?;
    let adapter: IDXGIAdapter1 = unsafe { factory.EnumAdapterByGpuPreference(0, DXGI_GPU_PREFERENCE_HIGH_PERFORMANCE) }.ok()?;
    let desc = unsafe { adapter.GetDesc1() }.ok()?;
    let luid = (u64::from(desc.AdapterLuid.HighPart as u32) << 32) | u64::from(desc.AdapterLuid.LowPart);
    let len = desc.Description.iter().position(|&c| c == 0).unwrap_or(desc.Description.len());
    Some((luid, String::from_utf16_lossy(&desc.Description[..len])))
}

unsafe fn video_type(subtype: GUID, width: u32, height: u32, fps: u32) -> windows::core::Result<IMFMediaType> {
    let kind = unsafe { MFCreateMediaType() }?;
    unsafe {
        kind.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        kind.SetGUID(&MF_MT_SUBTYPE, &subtype)?;
        kind.SetUINT64(&MF_MT_FRAME_SIZE, (u64::from(width) << 32) | u64::from(height))?;
        kind.SetUINT64(&MF_MT_FRAME_RATE, (u64::from(fps.max(1)) << 32) | 1)?;
        kind.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, (1 << 32) | 1)?;
        kind.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
    }
    Ok(kind)
}

unsafe fn sequence_header(transform: &IMFTransform) -> Vec<u8> {
    let Ok(kind) = (unsafe { transform.GetOutputCurrentType(0) }) else { return Vec::new() };
    let Ok(size) = (unsafe { kind.GetBlobSize(&MF_MT_MPEG_SEQUENCE_HEADER) }) else { return Vec::new() };
    let mut header = vec![0u8; size as usize];
    match unsafe { kind.GetBlob(&MF_MT_MPEG_SEQUENCE_HEADER, &mut header, None) } {
        Ok(()) => header,
        Err(_) => Vec::new(),
    }
}

unsafe fn sample_bytes(sample: &IMFSample) -> Result<Vec<u8>, String> {
    let buffer = unsafe { sample.ConvertToContiguousBuffer() }.map_err(|e| e.to_string())?;
    let mut data = std::ptr::null_mut();
    let mut length = 0u32;
    unsafe { buffer.Lock(&mut data, None, Some(&mut length)) }.map_err(|e| e.to_string())?;
    let bytes = unsafe { std::slice::from_raw_parts(data, length as usize) }.to_vec();
    let _ = unsafe { buffer.Unlock() };
    Ok(bytes)
}

unsafe fn friendly_name(activate: &IMFActivate) -> Option<String> {
    let mut name = PWSTR::null();
    let mut length = 0u32;
    unsafe { activate.GetAllocatedString(&MFT_FRIENDLY_NAME_Attribute, &mut name, &mut length) }.ok()?;
    let text = unsafe { name.to_string() }.ok();
    unsafe { CoTaskMemFree(Some(name.0 as *const c_void)) };
    text
}

fn variant_u32(value: u32) -> VARIANT {
    let mut variant = VARIANT::default();
    // SAFETY: writes the tag and the matching union member of a zeroed VARIANT.
    unsafe {
        let inner = &mut *variant.Anonymous.Anonymous;
        inner.vt = VT_UI4;
        inner.Anonymous.ulVal = value;
    }
    variant
}

fn variant_bool(value: bool) -> VARIANT {
    let mut variant = VARIANT::default();
    // SAFETY: as above, for VT_BOOL.
    unsafe {
        let inner = &mut *variant.Anonymous.Anonymous;
        inner.vt = VT_BOOL;
        inner.Anonymous.boolVal = if value { VARIANT_TRUE } else { windows::Win32::Foundation::VARIANT_FALSE };
    }
    variant
}

// BGRA → NV12 (BT.601 limited range, the same matrix OpenH264's converter uses). Chroma is the
// average of each 2x2 block. Sides are even (capture and cameras guarantee it).
pub fn bgra_to_nv12(frame: &VideoFrame, out: &mut Vec<u8>) {
    let (w, h) = (frame.width as usize, frame.height as usize);
    out.clear();
    out.resize(w * h * 3 / 2, 0);
    let (luma, chroma) = out.split_at_mut(w * h);
    let px = |x: usize, y: usize| {
        let i = (y * w + x) * 4;
        (i32::from(frame.bgra[i + 2]), i32::from(frame.bgra[i + 1]), i32::from(frame.bgra[i]))
    };
    for y in 0..h {
        for x in 0..w {
            let (r, g, b) = px(x, y);
            luma[y * w + x] = (((66 * r + 129 * g + 25 * b + 128) >> 8) + 16) as u8;
        }
    }
    for y in (0..h).step_by(2) {
        for x in (0..w).step_by(2) {
            let (mut r, mut g, mut b) = (0, 0, 0);
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let (pr, pg, pb) = px(x + dx, y + dy);
                (r, g, b) = (r + pr, g + pg, b + pb);
            }
            let (r, g, b) = (r / 4, g / 4, b / 4);
            let at = (y / 2) * w + x;
            chroma[at] = (((-38 * r - 74 * g + 112 * b + 128) >> 8) + 128) as u8;
            chroma[at + 1] = (((112 * r - 94 * g - 18 * b + 128) >> 8) + 128) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nv12_of_known_colours() {
        let frame = VideoFrame { width: 2, height: 2, bgra: [[0, 0, 255, 255], [0, 0, 255, 255], [0, 0, 255, 255], [0, 0, 255, 255]].concat() };
        let mut nv12 = Vec::new();
        bgra_to_nv12(&frame, &mut nv12);
        // Pure red in BT.601 limited range: Y 82, U 90, V 240.
        assert_eq!(nv12, [82, 82, 82, 82, 90, 240]);
    }
}
