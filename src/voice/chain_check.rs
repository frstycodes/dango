// Offline mic-chain run: a recording goes through the real mic chain (`process_mic`), the
// voice-activity gate and an Opus round trip at our send settings, and every stage is written as a
// WAV (taps.rs) for listening and scoring.
//
// CHAIN_IN=noisy.wav CHAIN_OUT=dir [CHAIN=echo,advanced,agc] \
//   cargo test --release chain_check -- --nocapture --include-ignored
// CHAIN_IN is 48 kHz mono float WAV. CHAIN picks the processing: any of echo, agc, and one of
// advanced / standard (noise suppression); empty means all off. Tuning switches, read only in tests:
// DFN_MIN_DB, DFN_MAX_ERB_DB, DFN_MAX_DF_DB, DFN_ATTEN_DB, DFN_POST_FILTER (DeepFilterNet), APM_32K
// (the old 32 kHz WebRTC rate), OPUS_APP=audio and OPUS_BITRATE (the encoder).

use opus::{Application, Channels, Decoder, Encoder};

use super::{
    DEFAULT_INPUT_MODE, NoiseSuppression,
    audio::{AudioSettings, FRAME_LEN, SAMPLE_RATE},
    dsp::Processor,
    media::BITRATE,
    taps::Taps,
    vad::Gate,
};

#[test]
#[ignore = "needs CHAIN_IN and CHAIN_OUT"]
fn chain_check() {
    let input = std::env::var("CHAIN_IN").expect("CHAIN_IN");
    let out = std::path::PathBuf::from(std::env::var("CHAIN_OUT").expect("CHAIN_OUT"));
    let chain = std::env::var("CHAIN").unwrap_or_else(|_| "echo,advanced,agc".into());
    let has = |stage: &str| chain.split(',').any(|s| s.trim() == stage);
    let noise = if has("advanced") {
        NoiseSuppression::Advanced
    } else if has("standard") {
        NoiseSuppression::Standard
    } else {
        NoiseSuppression::Off
    };
    let processor = Processor::new(has("echo"), noise, has("agc"));
    if noise == NoiseSuppression::Advanced {
        processor.wait_for_model();
    }
    processor.tap(Taps::new(out.clone()));
    let settings = AudioSettings::new(None, None, 1.0, 1.0, processor);

    let application = match std::env::var("OPUS_APP").as_deref() {
        Ok("audio") => Application::Audio,
        _ => Application::Voip,
    };
    let mut encoder = Encoder::new(SAMPLE_RATE, Channels::Stereo, application).unwrap();
    let bitrate = std::env::var("OPUS_BITRATE").ok().and_then(|b| b.parse().ok()).unwrap_or(BITRATE);
    encoder.set_bitrate(opus::Bitrate::Bits(bitrate)).unwrap();
    let mut decoder = Decoder::new(SAMPLE_RATE, Channels::Stereo).unwrap();
    let (mut gate, mut taps) = (Gate::default(), Taps::new(out));
    let (mut packet, mut decoded) = ([0u8; 4000], [0f32; FRAME_LEN]);

    for chunk in read_wav(&input).as_chunks::<{ FRAME_LEN / 2 }>().0 {
        let mut frame = [0.0; FRAME_LEN];
        frame.as_chunks_mut::<2>().0.iter_mut().zip(chunk).for_each(|(pair, &s)| *pair = [s, s]);
        let mic = settings.process_mic(&mut frame);
        let open = gate.update(DEFAULT_INPUT_MODE, settings.processor.gain(), &mic.blocks, false);
        let n = encoder.encode_float(&frame, &mut packet).unwrap();
        decoder.decode_float(&packet[..n], &mut decoded, false).unwrap();
        let left = |s: &[f32; FRAME_LEN]| s.as_chunks::<2>().0.iter().map(|[l, _]| *l).collect::<Vec<_>>();
        let sent = left(&decoded);
        taps.record("5-opus", &sent);
        taps.record("6-sent", &if open { sent } else { vec![0.0; FRAME_LEN / 2] });
    }
}

// The samples of a mono float WAV.
fn read_wav(path: &str) -> Vec<f32> {
    let bytes = std::fs::read(path).expect("CHAIN_IN readable");
    let at = bytes.windows(4).position(|w| w == b"data").expect("a data chunk") + 8;
    bytes[at..].as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect()
}
