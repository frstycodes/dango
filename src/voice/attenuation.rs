// Discord's Global Attenuation: "Lower the volume of other applications by this percent when
// someone is speaking." The media engine gets `attenuationFactor = (100 - attenuation) / 100`
// plus the "When I speak" / "When others speak" switches (docs/discord-audio.md). Other apps'
// volume is an OS control: Windows audio sessions here. macOS has no public per-app volume API,
// so the setting is hidden there, as Discord hides it where `supports(ATTENUATION)` is false.
// Not verified: the ramp speeds are ours, Discord's native ramp is unread; the Windows backend
// has not been compiled or run on Windows from this change.

use std::time::Duration;

pub const SUPPORTED: bool = cfg!(target_os = "windows");
// Gain change per second while ducking and while restoring.
const DUCK_PER_SECOND: f32 = 10.0;
const RESTORE_PER_SECOND: f32 = 2.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Attenuation {
    // 0..=100 percent; 0 disables it.
    pub strength: u8,
    pub when_self_speaks: bool,
    pub when_others_speak: bool,
}

impl Default for Attenuation {
    // Discord's defaults: attenuation 0, attenuateWhileSpeakingSelf false, attenuateWhileSpeakingOthers true.
    fn default() -> Self {
        Self { strength: 0, when_self_speaks: false, when_others_speak: true }
    }
}

impl Attenuation {
    // The volume factor other applications should be at right now.
    pub fn target(&self, self_speaking: bool, others_speaking: bool) -> f32 {
        let triggered = (self_speaking && self.when_self_speaks) || (others_speaking && self.when_others_speak);
        if !triggered || self.strength == 0 {
            return 1.0;
        }
        f32::from(100 - self.strength.min(100)) / 100.0
    }
}

// Moves the applied factor toward the target: quickly down, slowly back up.
pub struct Ramp {
    current: f32,
}

impl Default for Ramp {
    fn default() -> Self {
        Self { current: 1.0 }
    }
}

impl Ramp {
    pub fn step(&mut self, target: f32, elapsed: Duration) -> f32 {
        let rate = if target < self.current { DUCK_PER_SECOND } else { RESTORE_PER_SECOND };
        let max_step = rate * elapsed.as_secs_f32();
        self.current += (target - self.current).clamp(-max_step, max_step);
        self.current
    }
}

#[cfg(target_os = "windows")]
pub use windows_sessions::Ducking;

// Where other apps' volume can't be set; never started, since `SUPPORTED` is false there.
#[cfg(not(target_os = "windows"))]
pub struct Ducking;

#[cfg(not(target_os = "windows"))]
impl Ducking {
    pub fn new() -> Self {
        Self
    }

    pub fn apply(&mut self, _factor: f32) {}
}

// Scales every other process's session on the default output device, restoring the volumes it
// found once the factor returns to 1 or it is dropped.
#[cfg(target_os = "windows")]
mod windows_sessions {
    use windows::{
        Win32::{
            Foundation::S_OK,
            Media::Audio::{IAudioSessionControl2, IAudioSessionManager2, IMMDeviceEnumerator, ISimpleAudioVolume, MMDeviceEnumerator, eConsole, eRender},
            System::Com::{CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx},
        },
        core::Interface,
    };

    pub struct Ducking {
        originals: Vec<(ISimpleAudioVolume, f32)>,
    }

    impl Ducking {
        // Call on the thread that will use it.
        pub fn new() -> Self {
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            }
            Self { originals: Vec::new() }
        }

        pub fn apply(&mut self, factor: f32) {
            if factor >= 1.0 {
                return self.restore();
            }
            if self.originals.is_empty() {
                self.originals = other_sessions().unwrap_or_default();
            }
            for (volume, original) in &self.originals {
                unsafe {
                    let _ = volume.SetMasterVolume(original * factor, std::ptr::null());
                }
            }
        }

        fn restore(&mut self) {
            for (volume, original) in self.originals.drain(..) {
                unsafe {
                    let _ = volume.SetMasterVolume(original, std::ptr::null());
                }
            }
        }
    }

    impl Drop for Ducking {
        fn drop(&mut self) {
            self.restore();
        }
    }

    fn other_sessions() -> windows::core::Result<Vec<(ISimpleAudioVolume, f32)>> {
        let me = std::process::id();
        let mut sessions = Vec::new();
        unsafe {
            let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)?;
            let manager: IAudioSessionManager2 = device.Activate(CLSCTX_ALL, None)?;
            let all = manager.GetSessionEnumerator()?;
            for i in 0..all.GetCount()? {
                let control = all.GetSession(i)?;
                let control2: IAudioSessionControl2 = control.cast()?;
                if control2.IsSystemSoundsSession() == S_OK || control2.GetProcessId().is_ok_and(|pid| pid == me) {
                    continue;
                }
                let volume: ISimpleAudioVolume = control.cast()?;
                let level = volume.GetMasterVolume()?;
                sessions.push((volume, level));
            }
        }
        Ok(sessions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_follows_strength_and_who_is_speaking() {
        let defaults = Attenuation::default();
        assert_eq!(defaults.target(true, true), 1.0, "Discord ships attenuation at 0%: off");

        let half = Attenuation { strength: 50, ..Attenuation::default() };
        assert_eq!(half.target(false, true), 0.5, "others speaking ducks by default");
        assert_eq!(half.target(true, false), 1.0, "speaking yourself doesn't, by default");
        assert_eq!(half.target(false, false), 1.0);

        let both = Attenuation { strength: 80, when_self_speaks: true, when_others_speak: false };
        assert!((both.target(true, false) - 0.2).abs() < 1e-6);
        assert_eq!(both.target(false, true), 1.0, "others switched off");
        assert_eq!(Attenuation { strength: 100, ..half }.target(false, true), 0.0);
    }

    #[test]
    fn ramp_ducks_fast_and_restores_slowly() {
        let mut ramp = Ramp::default();
        let tick = Duration::from_millis(20);
        let ducked = (0..5).map(|_| ramp.step(0.5, tick)).last().unwrap();
        assert!((ducked - 0.5).abs() < 1e-6, "reaches -50% within 100 ms: {ducked}");
        let back = ramp.step(1.0, Duration::from_millis(100));
        assert!((back - 0.7).abs() < 1e-6, "comes back at 2/s: {back}");
        assert_eq!(ramp.step(1.0, Duration::from_secs(1)), 1.0, "and never overshoots");
    }
}
