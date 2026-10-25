//! In-process sound playback over `opentui_core::audio` — the
//! convergence lane SOUND-CONV. This replaces the former vendored
//! miniaudio engine (`sound/inproc.rs` + `third_party/miniaudio-sys`,
//! deleted): opentui-core rev 18e2904 ships the real playback device
//! (`device-audio` feature) plus the `AudioLib`/`Audio` surface v2's
//! TUI consumes, so the port now sits on the same engine as upstream
//! instead of a local twin.
//!
//! Parity map (v2 `packages/tui/src/audio.ts` → this module):
//!
//! | v2 | port |
//! | --- | --- |
//! | `Audio.create({autoStart:false})`, catch → `null` audio | [`engine`] — `Audio::create_with_lib`; `Err` tombstones the channel (every later call a silent no-op) |
//! | `.on("error", …)` registered immediately (`audio.ts:11`) | the no-op error listener installed at create (upstream `Audio` *panics* on an error with no listener — the port must never) |
//! | `loadSoundFile` decode cache keyed by path (`audio.ts:23-36`) | the `sounds` map beside the engine |
//! | `play()`: `!isStarted() && !start()` → null (`audio.ts:41-42`) | the lazy `start` in [`play_file`], retried on every play (started latches only on success) |
//! | `audioPlay(sound, {volume})` (`audio.ts:44-48`) | `Audio::play` with `AudioPlayOptions` — the TUI clamps `0..=1` first (`attention.ts:65-76`), the native side clamps `0..=4` |
//! | every failure = silent no-op (`audio.ts:7-21,38-43`) | every failure here returns `false`; nothing panics, nothing spawns |
//!
//! Threading: the engine is process-global behind a `Mutex` (v2's
//! native side serializes API calls with its own lock); `Audio` is
//! `Send` (the injected lib is `Box<dyn AudioLib + Send>`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use opentui_core::audio::{Audio, AudioLib, AudioPlayOptions, AudioSetupOptions};

use crate::attention::SoundCandidate;

/// The engine half: v2's module-level `let audio` — `None` is the
/// caught-`Audio.create` tombstone (a silent no-op channel).
struct EngineHalf {
    audio: Option<Audio>,
    /// v2's `loadSoundFile` cache: path → decoded sound handle.
    sounds: HashMap<PathBuf, opentui_core::audio::AudioSound>,
}

impl EngineHalf {
    fn new(lib: Option<Box<dyn AudioLib + Send>>) -> Self {
        let audio = lib
            .map_or_else(
                || {
                    Audio::create_with_lib(
                        Box::new(opentui_core::audio::HeadlessAudioLib::new()),
                        &setup_options(),
                    )
                },
                |lib| Audio::create_with_lib(lib, &setup_options()),
            )
            .ok()
            .map(|mut audio| {
                // v2 registers the error listener immediately after
                // create (`audio.ts:11`): upstream `Audio` PANICS on an
                // error emission with no listener, and a no-device
                // `start` failure emits one — the listener is what keeps
                // headless degradation silent.
                audio.on_error(|_error, _context| {});
                audio
            });
        Self { audio, sounds: HashMap::new() }
    }
}

/// v2's `Audio.create({ autoStart: false })` (`tui/src/audio.ts:9-21`).
fn setup_options() -> AudioSetupOptions {
    AudioSetupOptions { auto_start: Some(false), ..AudioSetupOptions::default() }
}

/// The process-global engine (v2's module-level `let audio`, created
/// lazily on the first play; creation never opens a device — the
/// device is `start`'s job, per native `create`'s `noDevice = TRUE`).
fn engine() -> &'static Mutex<EngineHalf> {
    static ENGINE: OnceLock<Mutex<EngineHalf>> = OnceLock::new();
    ENGINE.get_or_init(|| Mutex::new(EngineHalf::new(None)))
}

/// Lock the global engine, absorbing poisoning (the channel must keep
/// serving, never panic the TUI).
fn lock() -> std::sync::MutexGuard<'static, EngineHalf> {
    engine()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Replace the global engine with one over an injected lib (the test
/// seam mirroring upstream's own `replaceMethod(lib, …)` tests).
#[cfg(test)]
fn install_lib_for_test(lib: Box<dyn AudioLib + Send>) {
    let _ = engine(); // initialize the OnceLock first
    let mut guard = lock();
    *guard = EngineHalf::new(Some(lib));
}

/// Play a sound file through the opentui engine. `false` = v2's
/// silent no-op (engine tombstoned, unreadable/undecodable file, no
/// device). Never spawns a process.
#[must_use]
pub fn play_file(file: &Path, volume: f64) -> bool {
    let mut guard = lock();
    let Some(audio) = guard.audio.as_mut() else {
        return false;
    };
    // v2 `loadSoundFile` → `loadSound(bytes)`: read + decode + cache
    // by path (`audio.ts:23-36`).
    let sound = match guard.sounds.get(file).copied() {
        Some(sound) => sound,
        None => {
            let Ok(bytes) = std::fs::read(file) else {
                return false;
            };
            let Some(sound) = audio.load_sound(&bytes) else {
                return false; // decode failure → the candidate loop falls through
            };
            guard.sounds.insert(file.to_path_buf(), sound);
            sound
        }
    };
    // v2 `play()`: `!isStarted() && !start()` → null (`audio.ts:41-42`)
    // — retried on every play, exactly like native's lazy device start.
    if !audio.is_started() && !audio.start(None) {
        return false;
    }
    let options = AudioPlayOptions {
        // the TUI clamps 0..=1 before the engine sees it
        // (`attention.ts:65-76`); the native side clamps 0..=4.
        volume: Some(volume.clamp(0.0, 1.0)),
        ..AudioPlayOptions::default()
    };
    audio.play(sound, Some(&options)).is_some()
}

/// The production sound half of the attention emitter: the candidate
/// loop (first file that plays wins — v2's `playSound` loop) over the
/// opentui audio engine. Notifications are the OSC notifier's half.
#[derive(Debug, Default)]
pub struct OpentuiSoundPlayer;

impl crate::attention::AttentionEmitter for OpentuiSoundPlayer {
    fn notify(&self, _title: &str, _message: &str) -> bool {
        false
    }

    fn play(&self, candidates: &[SoundCandidate], volume: f64) -> bool {
        for candidate in candidates {
            if play_file(&candidate.path, volume) {
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attention::AttentionSoundName;
    use opentui_core::audio::{
        AudioEngineHandle, AudioError, AudioLoadResult, AudioPlayResult,
        HeadlessAudioLib, NativeAudioVoiceOptions, AUDIO_STATUS_ERR_DEVICE,
        AUDIO_STATUS_ERR_DECODE, AUDIO_STATUS_OK,
    };
    use std::sync::{Arc, Mutex};

    /// The recording fake (mirrors upstream's injected-lib tests): a
    /// start status knob + a log of every play's resolved voice
    /// options. Shares state with the test through `Arc<Mutex>` (the
    /// injected lib must be `Send`).
    struct FakeLib {
        state: Arc<Mutex<FakeState>>,
    }

    #[derive(Default)]
    struct FakeState {
        start_status: i32,
        start_calls: usize,
        plays: Vec<NativeAudioVoiceOptions>,
    }

    impl AudioLib for FakeLib {
        fn create_audio_engine(
            &mut self,
            _options: Option<opentui_core::audio::AudioEngineCreateOptions>,
        ) -> Option<AudioEngineHandle> {
            Some(AudioEngineHandle(1))
        }

        fn destroy_audio_engine(&mut self, _engine: AudioEngineHandle) -> Result<(), AudioError> {
            Ok(())
        }

        fn audio_start(
            &mut self,
            _engine: AudioEngineHandle,
            _options: Option<&opentui_core::audio::AudioStartOptions>,
        ) -> i32 {
            self.state.lock().unwrap().start_calls += 1;
            self.state.lock().unwrap().start_status
        }

        fn audio_stop(&mut self, _engine: AudioEngineHandle) -> i32 {
            AUDIO_STATUS_OK
        }

        fn audio_load(&mut self, _engine: AudioEngineHandle, data: &[u8]) -> AudioLoadResult {
            if data.is_empty() || data.starts_with(b"garbage") {
                return AudioLoadResult { status: AUDIO_STATUS_ERR_DECODE, sound_id: None };
            }
            AudioLoadResult { status: AUDIO_STATUS_OK, sound_id: Some(1) }
        }

        fn audio_play(
            &mut self,
            _engine: AudioEngineHandle,
            _sound: u32,
            options: Option<NativeAudioVoiceOptions>,
        ) -> AudioPlayResult {
            let options = options.unwrap_or_else(NativeAudioVoiceOptions::defaults);
            self.state.lock().unwrap().plays.push(options);
            AudioPlayResult { status: AUDIO_STATUS_OK, voice_id: Some(1) }
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }

    fn install(start_status: i32) -> Arc<Mutex<FakeState>> {
        let state = Arc::new(Mutex::new(FakeState { start_status, ..FakeState::default() }));
        install_lib_for_test(Box::new(FakeLib { state: Arc::clone(&state) }));
        state
    }

    /// A real asset extracted to a temp cache (the shared resolver).
    fn extracted_asset(name: AttentionSoundName) -> (std::path::PathBuf, std::path::PathBuf) {
        let cache = std::env::temp_dir().join(format!(
            "oc-tui-sound-audio-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let resolver = crate::sound::builtin_sound_resolver(cache.clone());
        let path = resolver(name).expect("asset extracts");
        (cache, path)
    }

    /// The happy path: asset loads, the device starts lazily (once,
    /// on the first play), and the volume arrives as the resolved
    /// native options — the TUI's 0..=1 clamp applied verbatim.
    #[test]
    fn plays_through_the_opentui_engine() {
        let state = install(AUDIO_STATUS_OK);
        let (cache, path) = extracted_asset(AttentionSoundName::Done);
        assert!(play_file(&path, 0.4), "fake lib accepts start + play");
        let guard = state.lock().unwrap();
        assert_eq!(guard.start_calls, 1, "the device starts lazily on first play");
        assert_eq!(guard.plays.len(), 1);
        assert_eq!(guard.plays[0].volume, 0.4);
        assert_eq!(guard.plays[0].pan, 0.0);
        assert!(!guard.plays[0].loop_);
        // Second play: started latched — no re-start, cache hit on load.
        drop(guard);
        assert!(play_file(&path, 0.7));
        let guard = state.lock().unwrap();
        assert_eq!(guard.start_calls, 1, "started latches, start is not retried");
        assert_eq!(guard.plays.len(), 2);
        assert_eq!(guard.plays[1].volume, 0.7);
        let _ = std::fs::remove_dir_all(&cache);
    }

    /// The TUI-side volume clamp (v2 `attention.ts:65-76`): 2.0 and
    /// −1 both arrive at the engine clamped into 0..=1.
    #[test]
    fn volume_is_clamped_to_0_1() {
        let state = install(AUDIO_STATUS_OK);
        let (cache, path) = extracted_asset(AttentionSoundName::Default);
        assert!(play_file(&path, 2.0));
        assert!(play_file(&path, -1.0));
        let volumes: Vec<f32> = state.lock().unwrap().plays.iter().map(|p| p.volume).collect();
        assert_eq!(volumes, vec![1.0, 0.0]);
        let _ = std::fs::remove_dir_all(&cache);
    }

    /// No-device graceful: `err_device` (−5) from `start` (exactly
    /// what the `device-audio` backend reports with no audio
    /// hardware) is a silent no-op — `false`, no panic — and the
    /// retry on the next play (v2's `playbackStarted` latches only on
    /// success) also stays graceful. No audio device is touched: the
    /// injected lib never opens one.
    #[test]
    fn no_device_is_a_silent_no_op() {
        let state = install(AUDIO_STATUS_ERR_DEVICE);
        let (cache, path) = extracted_asset(AttentionSoundName::Done);
        assert!(!play_file(&path, 0.4), "err_device → silent no-op");
        assert!(!play_file(&path, 0.4), "retry after failure stays graceful");
        assert_eq!(state.lock().unwrap().start_calls, 2, "start retried per play");
        assert!(state.lock().unwrap().plays.is_empty(), "play is never reached");
        let _ = std::fs::remove_dir_all(&cache);
    }

    /// Undecodable input: a garbage payload fails `loadSound`, the
    /// candidate falls through — `false`, no start, no play.
    #[test]
    fn undecodable_input_falls_through() {
        let state = install(AUDIO_STATUS_OK);
        let garbage = std::env::temp_dir().join(format!("oc-sound-garbage-{}.mp3", std::process::id()));
        std::fs::write(&garbage, b"garbage not an mp3").unwrap();
        assert!(!play_file(&garbage, 0.4));
        assert!(!play_file(std::path::Path::new("/nonexistent/asset.mp3"), 0.4));
        let state = state.lock().unwrap();
        assert!(state.plays.is_empty());
        assert_eq!(state.start_calls, 0, "start happens after a successful load");
        let _ = std::fs::remove_file(&garbage);
    }

    /// The candidate loop over the opentui player: unreadable
    /// candidates all fall through → `false` (v2's playSound loop
    /// gives up), with no panic and no spawn.
    #[test]
    fn candidate_loop_gives_up_gracefully() {
        install(AUDIO_STATUS_OK);
        let player = OpentuiSoundPlayer;
        let candidates = [SoundCandidate {
            name: AttentionSoundName::Done,
            path: PathBuf::from("/definitely/not/here.mp3"),
        }];
        assert!(!crate::attention::AttentionEmitter::play(&player, &candidates, 0.4));
    }

    /// Decoded-shape introspection through the real `HeadlessAudioLib`
    /// decode path (minimp3 — the same decoder family upstream's
    /// miniaudio uses): one vendored asset loads with a sane f32
    /// shape. No device is opened (`create` + `load` only).
    #[test]
    fn headless_lib_decodes_a_vendored_asset() {
        let mut lib = HeadlessAudioLib::new();
        let engine = lib.create_audio_engine(None).expect("engine creates (no device)");
        let load = lib.audio_load(engine, include_bytes!("../../assets/audio/bip-bop-01.mp3"));
        assert_eq!(load.status, AUDIO_STATUS_OK);
        let id = load.sound_id.expect("sound id on success");
        let (samples, channels, sample_rate) =
            lib.sound_shape(engine, id).expect("loaded sound has a shape");
        assert!((1..=2).contains(&channels), "channels {channels}");
        assert!(
            [22_050, 44_100, 48_000].contains(&sample_rate),
            "sample rate {sample_rate}"
        );
        let frames = samples / channels as usize;
        #[allow(clippy::cast_precision_loss)]
        let secs = frames as f64 / f64::from(sample_rate);
        assert!((0.05..10.0).contains(&secs), "duration {secs}s outside the chime band");
    }
}
