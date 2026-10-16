//! In-process sound playback — the port of v2's native audio engine
//! (`third_party/opentui/packages/native/src/audio.zig` over vendored
//! miniaudio 0.11.22, wired through `@opentui/core`'s `Audio` by
//! `packages/tui/src/audio.ts`). Linux-only: the process-player mirror
//! stays the fallback on other targets.
//!
//! Parity map (v2 source → this module):
//!
//! | v2 | port |
//! | --- | --- |
//! | `Audio.create({autoStart:false})` → `ma_engine_init(noDevice=TRUE)` (`tui/src/audio.ts:9-21`, `audio.zig` `createEngine`) | [`Engine::new`] — the no-device mixer cannot fail for lack of hardware |
//! | `play()`: `!isStarted() && !start()` → null (`audio.ts:41-42`) | [`Engine::ensure_device`] — attempted lazily on every play, retried on the next play after a failure, `false` on failure |
//! | `loadSoundFile` decode cache keyed by path (`audio.ts:23-36`) | [`Engine::load`] |
//! | `decodeSoundFromMemory`: `ma_decoder_config_init(f32, 0, 0)` + `ma_decoder_init_memory` + read-all (`audio.zig`) | [`decode_mp3`] |
//! | `audioPlay`: `ma_audio_buffer_ref_init` + sampleRate patch + `ma_sound_init_from_data_source(NO_SPATIALIZATION\|NO_PITCH)` + `ma_sound_set_volume` + `ma_sound_start` (`audio.zig`) | [`Engine::play_decoded`] |
//! | native volume clamp `0..=4`; TUI clamps `0..=1` first (`attention.ts:65-76`) | `play_decoded` clamps `0.0..=4.0` |
//! | every failure = silent no-op returning null/false (`audio.ts:7-21,38-43`) | every failure here returns `false`; nothing panics, nothing spawns |
//!
//! v2 has **no env kill-switch** for the audio channel (only the
//! `config.attention.sound` boolean + per-request `sound: false`,
//! `attention.ts:70-76`) — the port keeps the existing config gate and
//! adds none.
//!
//! Threading: the engine lives in a process-global `Mutex` (v2's native
//! side serializes API calls with its own lock). The audio-thread callback
//! only calls `ma_engine_read_pcm_frames` — the documented audio-thread
//! entry point; sounds are created/reaped from caller threads under the
//! mutex (miniaudio supports cross-thread sound init/uninit).

// miniaudio is only reachable through C FFI; each site carries its own
// SAFETY note, reviewed per-site (repo convention: unsafe_code = warn).
#![allow(unsafe_code)]

use std::collections::HashMap;
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::{Arc, Mutex, OnceLock};

use miniaudio_sys as sys;

use crate::attention::SoundCandidate;

/// The decoded half of v2's `Sound` (`audio.zig`: samples + channels +
/// sample rate): interleaved f32 frames at the file's native channel count
/// and rate — the engine's data converters handle the rest per voice.
pub(crate) struct DecodedSound {
    samples: Vec<f32>,
    channels: u32,
    sample_rate: u32,
}

impl DecodedSound {
    fn frame_count(&self) -> u64 {
        if self.channels == 0 {
            0
        } else {
            (self.samples.len() / self.channels as usize) as u64
        }
    }

    /// Playback duration in seconds (decode-path sanity tests).
    fn duration_seconds(&self) -> f64 {
        if self.sample_rate == 0 {
            0.0
        } else {
            self.frame_count() as f64 / f64::from(self.sample_rate)
        }
    }

    /// Native channel count of the decoded file (decode tests).
    fn channels(&self) -> u32 {
        self.channels
    }

    /// Native sample rate of the decoded file (decode tests).
    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
}

/// One `ma_sound` bound to a zero-copy reference over the decoded samples
/// (v2's `Voice`: `buffer_ref` + `sound`, reaped via `ma_sound_at_end`).
struct Voice {
    sound: sys::ma_sound,
    buffer_ref: sys::ma_audio_buffer_ref,
    sound_ready: bool,
    buffer_ready: bool,
}

impl Voice {
    fn finished(&self) -> bool {
        // SAFETY: only called on voices whose sound was initialized.
        self.sound_ready && unsafe { sys::ma_sound_at_end(&self.sound) != 0 }
    }

    /// v2's `clearVoice` order: sound first, then the buffer reference it
    /// reads from; each half only when it was initialized.
    fn clear(&mut self) {
        // SAFETY: the `*_ready` flags mirror v2's bookkeeping and gate the
        // matching uninitialization.
        unsafe {
            if self.sound_ready {
                sys::ma_sound_uninit(&mut self.sound);
                self.sound_ready = false;
            }
            if self.buffer_ready {
                sys::ma_audio_buffer_ref_uninit(&mut self.buffer_ref);
                self.buffer_ready = false;
            }
        }
    }
}

/// The audio-thread callback: pull mixed frames out of the engine (v2's
/// `audioCallback` → `ma_engine_read_pcm_frames`; the engine pointer is
/// recovered from `pDevice->pUserData`, miniaudio's callback convention).
unsafe extern "C" fn device_callback(
    device: *mut sys::ma_device,
    output: *mut c_void,
    _input: *const c_void,
    frame_count: sys::ma_uint32,
) {
    // SAFETY: `device` is the initialized playback device that invoked the
    // callback; `pUserData` was set to the engine core in `ensure_device`.
    let engine = unsafe { (*device).pUserData }.cast::<sys::ma_engine>();
    // SAFETY: `engine` points at the initialized engine core; this is the
    // documented audio-thread entry point.
    unsafe { sys::ma_engine_read_pcm_frames(engine, output, frame_count as u64, ptr::null_mut()) };
}

/// v2's `MAX_VOICES` guard: a voice-pool cap so a runaway trigger loop
/// cannot grow the mixer unbounded (`audio.zig`:
/// `activeVoiceAndStreamCount() >= max_voices` → `err_no_space`).
const MAX_VOICES: usize = 64;

/// The in-process engine (v2's native `Engine`): a no-device `ma_engine`
/// mixer core, a lazily started playback `ma_device`, a path-keyed decode
/// cache, and the voice pool.
struct Engine {
    /// `None` = the engine core failed to init (v2's `Audio.create` catch →
    /// the null audio object): every operation is a silent no-op.
    core: Option<sys::ma_engine>,
    device: Option<sys::ma_device>,
    /// `Some` forces a specific playback sink for the next device attempt
    /// (test seam: point the output at a nonexistent sink). `None` = the
    /// default device, like v2's `selected_playback_index == null`.
    forced_device_id: Option<Box<sys::ma_device_id>>,
    sounds: HashMap<PathBuf, Arc<DecodedSound>>,
    voices: Vec<Voice>,
}

// SAFETY: the engine is created on the caller thread; the audio thread
// only touches the `ma_engine` core through `device_callback` (the
// documented audio-thread entry point), every other access is serialized
// by the containing `Mutex`.
unsafe impl Send for Engine {}

impl Engine {
    /// v2 `createEngine`: `ma_engine_init` with `noDevice = TRUE` — a pure
    /// in-memory mixer that cannot fail for lack of audio hardware; the
    /// only failure is an allocation-style error, mapped to the tombstone
    /// (`core = None`) like v2's caught `Audio.create` exception.
    fn new() -> Self {
        // SAFETY: `ma_engine_config_init` is a pure config constructor.
        let mut config = unsafe { sys::ma_engine_config_init() };
        config.noDevice = 1;
        config.channels = 2;
        // SAFETY: `zeroed()` is a fresh, unaliased out-value (v2 uses
        // `undefined`); on failure miniaudio leaves it untouched and the
        // value is never used.
        let mut core: sys::ma_engine = unsafe { std::mem::zeroed() };
        // SAFETY: as above.
        let core = if unsafe { sys::ma_engine_init(&config, &mut core) } == sys::ma_result::MA_SUCCESS
        {
            Some(core)
        } else {
            None
        };
        Self {
            core,
            device: None,
            forced_device_id: None,
            sounds: HashMap::new(),
            voices: Vec::new(),
        }
    }

    /// v2 `startPlayback`'s device half: `ma_device_init` +
    /// `ma_device_start`, attempted lazily on the first play and retried
    /// after every failure (v2's `playbackStarted` only latches on
    /// success). `false` = the silent no-op.
    fn ensure_device(&mut self) -> bool {
        if self.core.is_none() {
            return false;
        }
        if self.device.is_some() {
            return true;
        }
        // SAFETY: `ma_device_config_init` is a pure config constructor.
        let mut config = unsafe { sys::ma_device_config_init(sys::ma_device_type::ma_device_type_playback) };
        let Some(core) = self.core.as_mut() else {
            return false;
        };
        // SAFETY: `core` is the initialized engine.
        config.sampleRate = unsafe { sys::ma_engine_get_sample_rate(core) };
        config.playback.format = sys::ma_format::ma_format_f32;
        config.playback.channels = 2;
        config.playback.pDeviceID = self
            .forced_device_id
            .as_deref()
            .map_or(ptr::null(), |id| ptr::from_ref(id));
        config.dataCallback = Some(device_callback);
        config.pUserData = ptr::from_mut(core).cast::<c_void>();

        // SAFETY: `zeroed()` is a fresh out-value; a NULL context makes
        // miniaudio create the device's own context internally (the same
        // dlopen backend discovery v2's explicit context performs).
        let mut device: sys::ma_device = unsafe { std::mem::zeroed() };
        if unsafe { sys::ma_device_init(ptr::null_mut(), &config, &mut device) } != sys::ma_result::MA_SUCCESS
        {
            return false;
        }
        // SAFETY: `device` was just initialized; on start failure v2
        // uninitialized it and cleared `has_device`.
        if unsafe { sys::ma_device_start(&mut device) } != sys::ma_result::MA_SUCCESS {
            unsafe { sys::ma_device_uninit(&mut device) };
            return false;
        }
        self.device = Some(device);
        true
    }

    /// v2's reap loop: finished voices are cleared on later API calls
    /// (`reapFinishedVoices`).
    fn reap_voices(&mut self) {
        self.voices.retain_mut(finished_negated);
    }

    /// v2 `audioPlay` over a decoded sound. `false` mirrors v2's `play()`
    /// → null (device unavailable / voice pool full / start failure): a
    /// silent no-op.
    fn play_decoded(&mut self, sound: &DecodedSound, volume: f64) -> bool {
        if self.core.is_none() {
            return false;
        }
        self.reap_voices();
        if self.voices.len() >= MAX_VOICES {
            return false;
        }
        if !self.ensure_device() {
            return false;
        }
        let frame_count = sound.frame_count();
        if frame_count == 0 || sound.channels == 0 || sound.samples.is_empty() {
            return false;
        }
        let mut voice = Voice {
            sound: unsafe { std::mem::zeroed() },
            buffer_ref: unsafe { std::mem::zeroed() },
            sound_ready: false,
            buffer_ready: false,
        };
        // SAFETY: `self.core` is initialized (checked above); the decoded
        // samples live in this engine's cache for the process lifetime (v2
        // never unloads sounds either), so the zero-copy reference outlives
        // the voice.
        let Some(core) = self.core.as_mut() else {
            return false;
        };
        unsafe {
            if sys::ma_audio_buffer_ref_init(
                sys::ma_format::ma_format_f32,
                sound.channels,
                ptr::from_ref(sound.samples.as_slice()).cast::<c_void>(),
                frame_count,
                &mut voice.buffer_ref,
            ) != sys::ma_result::MA_SUCCESS
            {
                return false;
            }
            voice.buffer_ready = true;
            // v2: `slot.buffer_ref.sampleRate = sound.sample_rate` — the
            // reference carries the file's native rate so the engine's
            // per-voice converter resamples to the device rate.
            voice.buffer_ref.sampleRate = sound.sample_rate;
            // `ma_audio_buffer_ref`'s first member is its data-source
            // header (v2: `@ptrCast(&slot.buffer_ref)`).
            let data_source = ptr::from_mut(&mut voice.buffer_ref).cast::<sys::ma_data_source>();
            let flags = (sys::ma_sound_flags::MA_SOUND_FLAG_NO_SPATIALIZATION as u32)
                | (sys::ma_sound_flags::MA_SOUND_FLAG_NO_PITCH as u32);
            if sys::ma_sound_init_from_data_source(
                core,
                data_source,
                flags,
                ptr::null_mut(),
                &mut voice.sound,
            ) != sys::ma_result::MA_SUCCESS
            {
                voice.clear();
                return false;
            }
            voice.sound_ready = true;
            // v2: `slot.volume = clamp(options.volume, 0, 4)` then
            // `ma_sound_set_volume` (the TUI already clamped 0..=1).
            let clamped = volume.clamp(0.0, 4.0) as f32;
            sys::ma_sound_set_volume(&mut voice.sound, clamped);
            if sys::ma_sound_start(&mut voice.sound) != sys::ma_result::MA_SUCCESS {
                voice.clear();
                return false;
            }
        }
        self.voices.push(voice);
        true
    }

    /// v2 `loadSoundFile` → `loadSound(bytes)`: read + decode + cache by
    /// path. `None` is the silent no-op (unreadable/undecodable file →
    /// the candidate loop falls through, `attention.ts:112-120`).
    fn load(&mut self, path: &Path) -> Option<Arc<DecodedSound>> {
        if let Some(cached) = self.sounds.get(path) {
            return Some(Arc::clone(cached));
        }
        let bytes = std::fs::read(path).ok()?;
        let decoded = Arc::new(decode_mp3(&bytes)?);
        self.sounds.insert(path.to_path_buf(), Arc::clone(&decoded));
        Some(decoded)
    }

    /// v2's `playSound` candidate body over the in-process engine.
    fn play_file(&mut self, path: &Path, volume: f64) -> bool {
        match self.load(path) {
            Some(decoded) => self.play_decoded(&decoded, volume),
            None => false,
        }
    }
}

/// `retain_mut` adapter: keep unfinished voices (clearing the finished
/// ones, v2's reap order).
fn finished_negated(voice: &mut Voice) -> bool {
    if voice.finished() {
        voice.clear();
        false
    } else {
        true
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        for voice in &mut self.voices {
            voice.clear();
        }
        if let Some(device) = self.device.as_mut() {
            // SAFETY: the device is initialized (invariant of `device`).
            unsafe {
                sys::ma_device_stop(device);
                sys::ma_device_uninit(device);
            }
        }
        if let Some(core) = self.core.as_mut() {
            // SAFETY: the core is initialized (invariant of `core`).
            unsafe { sys::ma_engine_uninit(core) };
        }
    }
}

impl Engine {
    fn decode_mp3_config() -> sys::ma_decoder_config {
        // SAFETY: pure config constructor.
        unsafe { sys::ma_decoder_config_init(sys::ma_format::ma_format_f32, 0, 0) }
    }
}

/// Decode an MP3 to interleaved f32 at its native channel count and rate —
/// Decode an MP3 to interleaved f32 at its native channel count and rate —
/// v2's `decodeSoundFromMemory`: `ma_decoder_config_init(ma_format_f32, 0,
/// 0)` (0 = keep the file's native channels/rate), then read every frame.
fn decode_mp3(bytes: &[u8]) -> Option<DecodedSound> {
    if bytes.is_empty() {
        return None;
    }
    let config = Engine::decode_mp3_config();
    // SAFETY: `zeroed()` is a fresh out-value (v2: `undefined`).
    let mut decoder: sys::ma_decoder = unsafe { std::mem::zeroed() };
    // SAFETY: `bytes` outlives the decoder's lifetime below; on failure the
    // decoder stays uninitialized and is never used.
    let init = unsafe {
        sys::ma_decoder_init_memory(
            ptr::from_ref(bytes).cast::<c_void>(),
            bytes.len(),
            &config,
            &mut decoder,
        )
    };
    if init != sys::ma_result::MA_SUCCESS {
        return None;
    }
    // `ma_decoder`'s first member is its `ma_data_source` (v2's
    // `streamFromDecoder` cast).
    // SAFETY: the decoder is initialized.
    let decoded = unsafe { decode_data_source(ptr::from_mut(&mut decoder).cast::<sys::ma_data_source>()) };
    // SAFETY: the decoder was initialized above regardless of the read
    // outcome.
    unsafe { sys::ma_decoder_uninit(&mut decoder) };
    decoded
}

/// v2's `decodeSound*`: query the source's native format, then read all
/// frames until `MA_AT_END` (v2's exact-length fast path and chunked
/// fallback collapse into the same chunked read here — same output).
///
/// # Safety
/// `source` must be an initialized miniaudio data source (a decoder here).
unsafe fn decode_data_source(source: *mut sys::ma_data_source) -> Option<DecodedSound> {
    let mut format = sys::ma_format::ma_format_unknown;
    let mut channels: sys::ma_uint32 = 0;
    let mut sample_rate: sys::ma_uint32 = 0;
    if sys::ma_data_source_get_data_format(
        source,
        &mut format,
        &mut channels,
        &mut sample_rate,
        ptr::null_mut(),
        0,
    ) != sys::ma_result::MA_SUCCESS
        || channels == 0
        || sample_rate == 0
        || channels > sys::MA_MAX_CHANNELS
    {
        return None;
    }
    if sys::ma_data_source_seek_to_pcm_frame(source, 0) != sys::ma_result::MA_SUCCESS {
        return None;
    }
    const CHUNK_FRAMES: usize = 4096;
    let mut samples: Vec<f32> = Vec::new();
    let mut chunk = vec![0_f32; CHUNK_FRAMES * channels as usize];
    loop {
        let mut frames_read: sys::ma_uint64 = 0;
        let result = sys::ma_data_source_read_pcm_frames(
            source,
            ptr::from_mut(chunk.as_mut_slice()).cast::<c_void>(),
            CHUNK_FRAMES as u64,
            &mut frames_read,
        );
        let read = frames_read as usize * channels as usize;
        samples.extend_from_slice(&chunk[..read]);
        if result == sys::ma_result::MA_AT_END || frames_read == 0 {
            break;
        }
        if result != sys::ma_result::MA_SUCCESS {
            return None;
        }
    }
    if samples.is_empty() {
        return None;
    }
    Some(DecodedSound {
        samples,
        channels,
        sample_rate,
    })
}

/// The process-global engine (v2's module-level `let audio` — created
/// once; every failure turns the channel into a silent no-op).
fn engine() -> &'static Mutex<Engine> {
    static ENGINE: OnceLock<Mutex<Engine>> = OnceLock::new();
    ENGINE.get_or_init(|| Mutex::new(Engine::new()))
}

/// Decode a file through the engine's decode path without touching any
/// device (the decode-path unit tests: format/channels/duration sanity
/// per vendored asset).
pub(crate) fn decode_file_for_test(path: &Path) -> Option<DecodedSound> {
    let bytes = std::fs::read(path).ok()?;
    decode_mp3(&bytes)
}

/// Play a sound file in-process. `false` = v2's silent no-op (no device,
/// undecodable file, voice pool full). Never spawns a process.
#[must_use]
pub fn play_file(file: &Path, volume: f64) -> bool {
    match engine().lock() {
        Ok(mut engine) => engine.play_file(file, volume),
        Err(poisoned) => poisoned.into_inner().play_file(file, volume),
    }
}

/// The production sound half of the attention emitter: the candidate loop
/// (first file that plays wins — v2's `playSound` loop) over the
/// in-process engine. Notifications are the OSC notifier's half.
#[derive(Debug, Default)]
pub struct InprocSoundPlayer;

impl crate::attention::AttentionEmitter for InprocSoundPlayer {
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

/// Point the next device attempt at a nonexistent playback sink (the
/// no-device graceful test seam — v2's device selection uses exactly this
/// `pPlaybackDeviceID`/`pDeviceID` path).
pub(crate) fn force_bogus_device_for_test() {
    match engine().lock() {
        Ok(mut engine) => engine.forced_device_id = Some(Box::new(unsafe { std::mem::zeroed() })),
        Err(poisoned) => poisoned.into_inner().forced_device_id = Some(Box::new(unsafe { std::mem::zeroed() })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attention::AttentionSoundName;

    /// Every vendored asset decodes to sane f32 stereo-rate PCM: channels
    /// 1..=2, rate within the MP3 family (22.05k/44.1k/48k), duration in
    /// the notification-chime band. Per-asset, exercising the exact
    /// production decode path (`ma_decoder_config_init(f32, 0, 0)`).
    #[test]
    fn decodes_every_builtin_asset() {
        let cache = std::env::temp_dir().join(format!("oc-tui-sound-decode-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&cache);
        let resolver = crate::sound::builtin_sound_resolver(cache.clone());
        for name in [
            AttentionSoundName::Default,
            AttentionSoundName::Question,
            AttentionSoundName::Permission,
            AttentionSoundName::Error,
            AttentionSoundName::SubagentDone,
        ] {
            let path = resolver(name).expect("asset extracts");
            let decoded = decode_file_for_test(&path)
                .unwrap_or_else(|| panic!("{name:?} decodes through the miniaudio path"));
            let (ch, rate, secs) =
                (decoded.channels(), decoded.sample_rate(), decoded.duration_seconds());
            assert!((1..=2).contains(&ch), "{name:?}: channels {ch}");
            assert!(
                [22_050, 44_100, 48_000].contains(&rate),
                "{name:?}: sample rate {rate}"
            );
            assert!(
                (0.05..10.0).contains(&secs),
                "{name:?}: duration {secs}s outside the chime band"
            );
        }
        let _ = std::fs::remove_dir_all(&cache);
    }

    /// The MP3 magic survives embedding (the first bytes of every asset
    /// are an MPEG frame sync or ID3 header) — a regression tripwire for
    /// asset corruption that would silently turn playback into a no-op.
    #[test]
    fn assets_keep_mp3_magic() {
        fn check(bytes: &[u8], name: &str) {
            let head = &bytes[..3.min(bytes.len())];
            let id3 = head[0] == b'I' && head[1] == b'D' && head[2] == b'3';
            let sync = head[0] == 0xFF && head[1] & 0xE0 == 0xE0;
            assert!(id3 || sync, "{name}: not MP3 (head {head:02X?})");
        }
        check(include_bytes!("../../assets/audio/bip-bop-01.mp3"), "bip-bop-01");
        check(include_bytes!("../../assets/audio/bip-bop-03.mp3"), "bip-bop-03");
        check(include_bytes!("../../assets/audio/staplebops-06.mp3"), "staplebops-06");
        check(include_bytes!("../../assets/audio/nope-03.mp3"), "nope-03");
        check(include_bytes!("../../assets/audio/yup-01.mp3"), "yup-01");
    }

    /// No-device graceful: with the output pointed at a nonexistent sink,
    /// playback is a silent no-op — `false`, no panic, no process spawn
    /// (in-process by construction: the engine is the only callee).
    #[test]
    fn bogus_sink_is_a_silent_no_op() {
        force_bogus_device_for_test();
        let cache = std::env::temp_dir().join(format!("oc-tui-sound-bogus-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&cache);
        let resolver = crate::sound::builtin_sound_resolver(cache.clone());
        let path = resolver(AttentionSoundName::Done).expect("asset extracts");
        // v2 retries the device on every play (playbackStarted latches only
        // on success): two calls prove the retry path also stays graceful.
        assert!(!play_file(&path, 0.4), "nonexistent sink → silent no-op");
        assert!(!play_file(&path, 0.4), "retry after failure stays graceful");
        let _ = std::fs::remove_dir_all(&cache);
    }

    /// Undecodable input: garbage bytes decode to `None`, a missing file
    /// to `None` — v2's `loadSoundFile` catch → candidate falls through.
    #[test]
    fn undecodable_input_is_none() {
        assert!(decode_mp3(b"not an mp3 at all").is_none());
        assert!(decode_mp3(b"").is_none());
        assert!(decode_file_for_test(Path::new("/nonexistent/asset.mp3")).is_none());
    }

    /// The candidate loop over the in-process player: unreadable
    /// candidates all fall through → `false` (v2's playSound loop gives
    /// up), with no panic and no spawn.
    #[test]
    fn candidate_loop_gives_up_gracefully() {
        let player = InprocSoundPlayer;
        let candidates = [SoundCandidate {
            name: AttentionSoundName::Done,
            path: PathBuf::from("/definitely/not/here.mp3"),
        }];
        assert!(!crate::attention::AttentionEmitter::play(&player, &candidates, 0.4));
    }
}
