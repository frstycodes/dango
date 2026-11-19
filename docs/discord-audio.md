# Discord's voice processing, as read from Discord's own code

Mapped 2026-11-14 for Waffle's parity work (`src/voice/`). Only code was read; no message
content, tokens or account data.

## Sources and limits

- **Web bundle** (plain minified JS, cached from discord.com): `web.25de09cac6a6078d.js` (main),
  `189deeeaee71ca56.js` (settings pages chunk), `7ca0707e311cb44e.js` (English strings). Offsets
  below are byte offsets into these files. Desktop and web share this JS; the desktop client hands
  the same options to its native engine.
- **Desktop app**: `/Applications/Discord.app/Contents/Resources/app.asar` contains only the
  bootstrap/updater; none of the strings `discord_voice`, `discord_krisp` or `krisp` occur in it.
  The voice engine (`discord_voice`) and Krisp (`discord_krisp`) live under
  `~/Library/Application Support/discord/…/modules`, which macOS keeps from other apps. A copy
  made from Finder (`app-0.0.422`, 2026-11-19) was read: `discord_voice.node` (arm64 slice, C++
  symbols intact) through `nm`, `objdump` and REA (Hopper). Only the native engine's code was read;
  Krisp's `.kef` models were not loaded, copied or called, and neither was `discord_krisp.node`.
  Waffle uses DeepFilterNet3 in Krisp's place.
- Where behaviour lives only in code that wasn't read, it's marked *Not verified*.

## Settings: names, defaults, options

Defaults are the MediaEngineStore's `e5()` (`web…js@11356806`). Labels are the English strings,
with their key.

| Setting (UI label) | Stored as | Default | Options / range |
|---|---|---|---|
| Input Mode: Voice Activity / Push-to-talk | `mode` | `VOICE_ACTIVITY` | `VOICE_ACTIVITY`, `PUSH_TO_TALK` |
| Automatically Adjust Input Sensitivity (`lY6j47`) | `modeOptions.autoThreshold` | `true` on desktop (`isPlatformEmbedded`) | bool |
| Input Sensitivity threshold | `modeOptions.threshold` | `-60` dB | slider `-100…0` dB, shown as `"{n}dB"` (`189…js@160908`) |
| Advanced Voice Activity (`BbESsg`) | `modeOptions.vadUseKrisp` | `true` | bool. Subtitle (`LoOB1F`): "Turning this off may help if your voice is not being detected by the automatic input sensitivity." |
| Krisp VAD activation threshold | `modeOptions.vadKrispActivationThreshold` | `0.5` (raised to `0.8` when AGC is on, see below) | not in UI |
| VAD leading / trailing | `modeOptions.vadLeading` / `vadTrailing` | `5` / `25` | not in UI. `vadLeading` is forced to `0` for the Go Live (stream) context |
| Push-to-talk Release Delay (`GCNMM8`) | `modeOptions.delay` | `20` ms | `0…2000` ms; shown as ms, or seconds from 1000 ms |
| Noise Suppression (`t8Qhib`) | `noiseCancellation` (Krisp) + `noiseSuppression` (Standard) | Krisp: `noiseCancellation: true`, `noiseSuppression: false` | Krisp / Standard / None |
| Echo Cancellation (`iWTwu6`) | `echoCancellation` | `true` | bool |
| Automatic Gain Control (`cUMdH0`) | `automaticGainControl` | `true` | bool. Subtitle (`6EjbvA`): "Automatically adjust microphone volume to keep it clear and consistent." |
| Bypass System Audio Input Processing (`DFPXIG`) | `bypassSystemInputProcessing` | `true` | bool, only where the engine supports it |
| No Audio Detected Warning (`4rsOPQ`) | `silenceWarning` | `true` | bool. "Show a warning when Discord is not detecting audio from your mic" |
| Global Attenuation: Attenuation Strength (`AlybXj`) | `attenuation` | `0` | `0…100` % |
| Global Attenuation: When I speak (`9dHxRY`) | `attenuateWhileSpeakingSelf` | `false` | bool |
| Global Attenuation: When others speak (`SMt0Gr`) | `attenuateWhileSpeakingOthers` | `true` | bool |
| Quality of Service (`uancuJ`) | `qos` | `false` (an old `true` is migrated to `false`) | bool. Subtitle (`I1Eoqq`): "Hints to your router that the packets Discord is transmitting are high priority. Some routers or internet service providers may misbehave when this is set." |
| Input Profile (`LM3U3k`) | `activeInputProfile` | `null` (Custom) | Voice Isolation / Studio / Custom |
| Input / Output Volume | `inputVolume` / `outputVolume` | `100` / `100` | `0…200` % |
| Stream Attenuation (`/jwMtn`) | `sidechainCompression` | `true` | bool, with `sidechainCompressionStrength` `50`. "Automatically reduce stream volume when people are talking." (Streaming settings, not Voice) |
| Audio Subsystem | `automaticAudioSubsystem` | `true` | Standard / Legacy / Next Gen / Automatic (Windows) |

### Input profiles overlay the stored values

`e7` (`web…js@11357904`) holds per-profile overrides. `t3()` (`@11359491`) merges
`stored ∪ profile ∪ (Krisp available ? {} : e8)`, so stored Custom values survive a profile
switch.

- **Voice Isolation**: `autoThreshold: true`, `vadUseKrisp: true`, `echoCancellation: true`,
  `noiseSuppression: false`, `automaticGainControl: true`, `noiseCancellation: true`,
  `bypassSystemInputProcessing: true`.
- **Studio**: `mode: VOICE_ACTIVITY`, `threshold: -84`, `autoThreshold: false`,
  `vadUseKrisp: false`, and echo cancellation, noise suppression, AGC and Krisp all off.
- **Custom**: no overrides.
- **Krisp unavailable** (`e8`, `@11358336`): `vadUseKrisp: false`, `noiseCancellation: false`,
  `noiseSuppression: true`. Standard suppression takes over.

UI predicates (`189…js`): Input Sensitivity, Noise Suppression, Echo Cancellation, AGC and
Advanced Voice Activity show only under Custom. Push-to-talk is hidden under Studio.

## Interactions

- **Noise Suppression is one choice over two flags.** Choosing Krisp sets
  `noiseCancellation=true, noiseSuppression=false`. Standard is the reverse, and None clears both
  (`r8`, `189…js@163337`). The value shown is `noiseCancellation ? KRISP : noiseSuppression ?
  STANDARD : NONE`, so Krisp wins if both are set. One-time migrations
  (`ncUseKrispSettingVersion`, `@11383343`) force `noiseSuppression=false,
  noiseCancellation=true`.
- **Advanced Voice Activity is disabled** unless the mode is Voice Activity *and* automatic
  sensitivity is on (`rF.useDisabled`, `189…js@156268`). Its value is only sent as
  `vadUseKrisp && krispLoaded` (`t6`, `@11359956`).
- **AGC raises the Krisp VAD activation threshold**: if `automaticGainControl` is on (or the
  profile turns it on and no threshold is set), `vadKrispActivationThreshold = 0.8`; otherwise
  `0.5` (`t3`).
- **Automatic sensitivity without Advanced Voice Activity** is sent as
  `vadAutoThreshold: VERY_AGGRESSIVE` (enum `DISABLED -1, NORMAL 0, LOW_BITRATE 1, AGGRESSIVE 2,
  VERY_AGGRESSIVE 3`, `web…js@1025527`; `createInputModeOptions`, `@1063493`). These are
  WebRTC VAD's four aggressiveness modes. Manual sensitivity sends `DISABLED` plus `vadThreshold`.
- **A certified mic's own processing wins**: echo cancellation, noise suppression and AGC are each
  `hasX(device) || setting`, and the matching toggles are disabled (`ns`, `@11366169`).
- **Krisp falling behind or failing**: the engine reports per-frame process time. If Krisp's
  average is over 8 (per frame, more than 50 frames), the client raises `KRISP_CPU_OVERUSE`. If
  the Krisp VAD's average is over 4, it raises `KRISP_VAD_CPU_OVERUSE`. Krisp init errors mark
  Krisp fatally unavailable, and the `e8` fallback (Standard) applies (`@1071278`, `ne()`).
  Strings: "We disabled noise suppression to improve performance." (`i+SO/U`).
- **System noise suppression** (an OS mic mode) can force Krisp off: `nr()` passes the setting
  through `getSystemMicrophoneMode()`.
- **No input**: the engine gets `setNoInputThreshold(-100)` (`-150` with Krisp on web), and the
  client debounces its silence callback: 1.5 s to report input detected, 5 s to report it lost
  (`handleSilence`, `@11353412`). The in-call notice shows when no input has been detected this
  connection.
- **Attenuation** goes to the engine as `attenuationFactor = (100 - attenuation) / 100`, with
  `attenuation: factor < 1`, `attenuateWhileSpeakingSelf` and `attenuateWhileSpeakingOthers`
  (`setAttenuation`, `@1052045`). The fieldset only shows where `supports(ATTENUATION)`. What gets
  lowered (other applications) is in the subtitle `0A/8Rt`. *Not verified*: how the native engine
  ducks, and its ramp.

## Processing chain and tuning

- **AGC config** (`eQ`, `@11356334`): `useAGC2: true, enableAnalog: false, enableDigital: true,
  headroom_db: 5, max_gain_db: 50, initial_gain_db: 15, max_gain_change_db_per_second: 6,
  max_output_noise_level_dbfs: -50, fixed_gain_db: 0`. AGC off sends `{enabled:false,
  useAGC2:false}`. These are WebRTC AGC2 adaptive-digital fields.
- **Krisp placement**: the engine gets both `noiseCancellation` and
  `noiseCancellationDuringProcessing` set to the same value. In `discord_voice.node`,
  `discord::media::CapturedAudioProcessor::Process` (`0x8b854c`) runs Krisp
  (`NoiseCanceller::Process`, int16) on the raw frame *before* APM only when the during-processing
  flag is off; with it on, Krisp runs as `NoiseCanceller::ProcessFloat(webrtc::AudioBuffer&)`
  inside APM. APM's `ProcessCaptureStreamLocked` (`0x699ee0`) calls, in order: high-pass filter,
  band split, AEC3, NS, AGC1, band merge, the custom-processing hook at `this+0x758`,
  `GainController2::Process`, then `webrtc::PostFilter::Process`. So the chain is HPF → AEC3 →
  (NS) → Krisp → AGC2 → post filter. The hook's position is read from the disassembly; that it
  holds Krisp is inferred from the flag and `ProcessFloat`'s `AudioBuffer` argument.
- **APM rate and post filter**: `PostFilter::CreateIfNeeded` (`0x6ace98`) builds a filter only at
  48 kHz: four biquads (`kPostFilterCoefficients48kHz`, `0xc9f97c`) forming a low-pass at
  19.2 kHz (-3 dB), -20 dB by 19.5 kHz, flat below 18 kHz. So Discord's APM processes at 48 kHz.
  It isn't in upstream WebRTC as far as known; Waffle doesn't copy it.
- **Krisp on web**: the worklet is registered from a runtime blob as `"processor"`, with models
  `model_8`, `model_16`, `model_32` from `cdn.discordapp.com/assets/krisp_browser_models/` and
  `modelName: "model32"` in a 48 kHz context. In a voice call on Chrome 157 with a fake mic, the
  sent track was the raw `getUserMedia` track (browser echo cancellation and AGC on, browser NS
  off) and the worklet's output stayed silent. *Not verified*: whether web Krisp was simply
  inactive in that setup.
- **Krisp models** (`eo`, `@11353727`): `krisp-nc-o-nb-v2.kef` (8 kHz), `krisp-nc-o-lite-v1.kef`
  (16 kHz), `krisp-nc-o-med-v7-fp16.kef` (up to 96 kHz). Suppression level from
  `getSuppressionLevel() ?? 100`. Not used by Waffle.
- **Web VAD** (`class F`, `@1104582`): every 20 ms it takes the loudest FFT bin (fftSize 512,
  smoothing 0.1). Speech is on while any of the last 10 ticks was over the threshold, plus 10 more
  ticks. That's roughly 200–400 ms of hangover. Web's default threshold is `-40`.
- **Native VAD** gets `vadLeading: 5`, `vadTrailing: 25` and the activation threshold. *Not
  verified*: the frame unit (10 ms APM blocks assumed) and whether "leading" is pre-roll (assumed,
  since the stream context zeroes it) or an onset debounce.
- **Opus SDP** (web, `@1136448`): `minptime=10;useinbandfec=1;usedtx=1` (`usedtx=0` when DTX is
  off), `stereo=1;sprop-stereo=1` for stereo, `maxptime=60`, 48 kHz, 2 channels.
- **Voice gateway**: processing settings are not sent. Speaking is op 5 `{speaking, delay, ssrc}`
  (`@4082279`), with flags `VOICE=1, SOUNDSHARE=2, PRIORITY=4, HIDDEN=8` (`@1006213`). Analytics
  record `vad_use_advanced_voice_activity` per connection.

## Settings page order (Voice & Video › Voice)

`VOICE_AND_VIDEO_PANEL` (`189…js@185100`) lays out: Voice (input device, output device, input
volume, output volume, Mic Test), then Input Profile category `[Input Profile, Input Sensitivity,
Noise Suppression, Echo Cancellation, Spatial Audio, Push-to-talk, PTT keybind, PTT release
delay, Advanced accordion]` (`@169631`). The Advanced accordion holds `[Automatic Gain Control,
Advanced Voice Activity, Bypass System Audio Input Processing, Audio Device Suggestions, No Audio
Detected Warning, Switch Voice Channel Warning, Global Attenuation (strength, When I speak, When
others speak), Audio Subsystem, Quality of Service]` (`@158925`). Collapsed, it lists the titles
of its first 3 visible settings under Custom, or 2 otherwise. Mic test notice when silent:
"Discord isn't detecting any input from your mic." / "Please check that you've selected the right
input device." (`bgn75v`, `MA+OZh`).

## How Waffle maps it (`src/voice/`)

| Discord | Waffle |
|---|---|
| Krisp noise cancellation | DeepFilterNet3 (bundled model, tract, 48 kHz, 10 ms hops) on its own worker thread, both stages always on (thresholds -15/35/35 dB; libDF's defaults hard-mute blocks it rates below -10 dB). RNNoise covers while it loads and replaces it for the call if it averages over 8 ms per 10 ms block (Discord's overuse rule) |
| Standard noise suppression | WebRTC NS (`sonora`), Moderate. *Not verified*: Discord's level |
| Echo cancellation | WebRTC AEC3 (`sonora`) at 48 kHz like Discord's APM (sonora defaults to 32 kHz, which drops everything above 16 kHz). No suppressor after it, as in Discord. The played audio reaches it through a bounded queue, not a lock on the output callback |
| AGC | WebRTC AGC2 with Discord's exact `eQ` values, run after the neural suppressor |
| Krisp VAD (Advanced Voice Activity) | RNNoise's speech probability, at Discord's 0.5 / 0.8-with-AGC activation |
| `vadAutoThreshold: VERY_AGGRESSIVE` | WebRTC GMM VAD (`earshot` 0.1 port), very aggressive profile |
| vadLeading 5 / vadTrailing 25 | 5 × 10 ms pre-roll sent at onset (3 Opus frames); 25 × 10 ms hangover |
| Global attenuation | Other apps' sessions on Windows (Core Audio), with our own ramp. Hidden on macOS, which has no public per-app volume API |
| QoS | DSCP EF (TOS `0xB8`) on the voice socket, set when the call connects |
| No Audio Detected | Same −100 dBFS / 1.5 s / 5 s logic, shown in Settings (mic test, and in calls when the warning is on) |
| Input profiles | Overlays exactly as `e7`, stored Custom switches untouched |
