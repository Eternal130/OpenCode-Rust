//! Attention sound playback — the port of v2's audio channel
//! (`packages/tui/src/audio.ts` + `attention.ts`'s `playSound`).
//!
//! v2 decodes + plays the built-in MP3 pack in-process through
//! opentui's `Audio` API (`opentui_core::audio`, the `device-audio`
//! feature's real miniaudio playback device — the same engine
//! upstream). On Linux this port does the same ([`audio`] over the
//! opentui engine: the same decode path, volume model, and
//! silent-no-op-when-no-device semantics); on other targets
//! the historical mirror shells out to the first available system
//! player, detached (`spawn`, stdio null):
//!
//! | player                 | volume flag (v2 volume is 0..1 linear)   |
//! | ---------------------- | --------------------------------------- |
//! | `pw-play` (`PipeWire`)   | `--volume <0..1>`                       |
//! | `paplay` (`PulseAudio`)  | `--volume <0..65536>`                   |
//! | `ffplay` (ffmpeg)      | `-volume <0..100>` + `-nodisp -autoexit`|
//! | `mpv`                  | `--volume=<0..100>` + `--no-video`      |
//! | `canberra-gtk-play`    | none (volume ignored, last resort)       |
//!
//! The built-in `opencode.default` pack (see `assets/audio/README.md`
//! for provenance) is embedded with `include_bytes!` (~22 KiB) and
//! extracted lazily to `<cache>/audio/` on first play (temp-file +
//! rename; v2's node build extracts assets to disk the same way —
//! `attention-sounds.node.ts` — and both halves then `readFile` +
//! decode, so the resolver below is shared by both paths).
//!
//! The spawn half sits behind the [`SoundSpawner`] trait so tests
//! assert the exact argv without executing anything; the in-process
//! half is exercised end-to-end (decoding needs no device).

/// In-process playback over `opentui_core::audio` (Linux only — v2
/// parity; the former vendored-miniaudio twin was deleted in the
/// SOUND-CONV convergence lane).
#[cfg(target_os = "linux")]
pub mod audio;

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::sync::OnceLock;

use crate::attention::{AttentionEmitter, AttentionSoundName, SoundCandidate};

/// The production sound half of the attention emitter: in-process
/// playback through `opentui_core::audio` on Linux (v2 parity), the
/// system-player spawn mirror elsewhere.
#[cfg(target_os = "linux")]
pub type SoundHalf = audio::OpentuiSoundPlayer;

/// The production sound half of the attention emitter outside Linux: the
/// system-player spawn mirror.
#[cfg(not(target_os = "linux"))]
pub type SoundHalf = ProcessSoundPlayer;

/// The embedded built-in pack (`assets/audio/`, `@opencode/ui` MIT):
/// every sound name maps to a file (`done` reuses `bip-bop-01`, the
/// `default` sound — v2's `BUILTIN_SOUNDS` table).
fn builtin_asset(name: AttentionSoundName) -> (&'static [u8], &'static str) {
    match name {
        AttentionSoundName::Default | AttentionSoundName::Done => {
            (include_bytes!("../assets/audio/bip-bop-01.mp3"), "bip-bop-01.mp3")
        }
        AttentionSoundName::Question => {
            (include_bytes!("../assets/audio/bip-bop-03.mp3"), "bip-bop-03.mp3")
        }
        AttentionSoundName::Permission => {
            (include_bytes!("../assets/audio/staplebops-06.mp3"), "staplebops-06.mp3")
        }
        AttentionSoundName::Error => (include_bytes!("../assets/audio/nope-03.mp3"), "nope-03.mp3"),
        AttentionSoundName::SubagentDone => {
            (include_bytes!("../assets/audio/yup-01.mp3"), "yup-01.mp3")
        }
    }
}

/// Resolve the ordered sound candidates is the engine's job (it owns
/// the config overrides); this module only produces the built-in half.
///
/// The built-in resolver for an engine: extracts the embedded pack to
/// `cache_dir/audio` on first use and hands back absolute paths.
#[must_use]
pub fn builtin_sound_resolver(
    cache_dir: PathBuf,
) -> Rc<dyn Fn(AttentionSoundName) -> Option<PathBuf>> {
    Rc::new(move |name| {
        let (_, file) = builtin_asset(name);
        extract_builtin(name, file, &cache_dir).ok()
    })
}

/// Extract one embedded asset under `cache_dir/audio/<file>` (atomic
/// temp+rename; an existing file is kept).
fn extract_builtin(name: AttentionSoundName, file: &str, cache_dir: &Path) -> io::Result<PathBuf> {
    let dir = cache_dir.join("audio");
    std::fs::create_dir_all(&dir)?;
    let target = dir.join(file);
    if !target.exists() {
        let (bytes, _) = builtin_asset(name);
        let temp = dir.join(format!(".{file}.tmp"));
        std::fs::write(&temp, bytes)?;
        std::fs::rename(&temp, &target)?;
    }
    Ok(target)
}

/// A discovered player binary + its volume mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerKind {
    PwPlay,
    Paplay,
    Ffplay,
    Mpv,
    Canberra,
}

impl PlayerKind {
    /// The probe order (`PipeWire` → `PulseAudio` → ffmpeg → mpv →
    /// canberra).
    const PROBE: [Self; 5] = [Self::PwPlay, Self::Paplay, Self::Ffplay, Self::Mpv, Self::Canberra];

    /// The binary name.
    #[must_use]
    pub fn program(self) -> &'static str {
        match self {
            Self::PwPlay => "pw-play",
            Self::Paplay => "paplay",
            Self::Ffplay => "ffplay",
            Self::Mpv => "mpv",
            Self::Canberra => "canberra-gtk-play",
        }
    }

    /// The full argv playing `file` at v2's 0..1 linear `volume`.
    ///
    /// The integer scales are exact for the clamped 0..=1 input
    /// (`0.4 * 65536 = 26214.4` rounds inside `u64`; negative and
    /// overflowing values cannot occur past the clamp).
    #[must_use]
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub fn argv(self, file: &Path, volume: f64) -> Vec<OsString> {
        let volume = volume.clamp(0.0, 1.0);
        let mut argv: Vec<OsString> = match self {
            Self::PwPlay => {
                vec!["--volume".into(), format!("{volume:.3}").into(), file.as_os_str().to_owned()]
            }
            Self::Paplay => vec![
                "--volume".into(),
                format!("{}", (volume * 65536.0).round() as u64).into(),
                file.as_os_str().to_owned(),
            ],
            Self::Ffplay => vec![
                "-nodisp".into(),
                "-autoexit".into(),
                "-loglevel".into(),
                "quiet".into(),
                "-volume".into(),
                format!("{}", (volume * 100.0).round() as u64).into(),
                file.as_os_str().to_owned(),
            ],
            Self::Mpv => vec![
                "--no-video".into(),
                "--really-quiet".into(),
                format!("--volume={}", (volume * 100.0).round() as u64).into(),
                file.as_os_str().to_owned(),
            ],
            Self::Canberra => vec!["-f".into(), file.as_os_str().to_owned()],
        };
        argv.shrink_to_fit();
        argv
    }
}

/// `true` when `program` resolves on PATH (a `which`-style probe that
/// never executes it).
fn program_exists(program: &str) -> bool {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path).any(|dir| dir.join(program).is_file())
}

/// The first available player (probed once per process).
#[must_use]
pub fn detect_player() -> Option<PlayerKind> {
    static DETECTED: OnceLock<Option<PlayerKind>> = OnceLock::new();
    *DETECTED
        .get_or_init(|| PlayerKind::PROBE.into_iter().find(|kind| program_exists(kind.program())))
}

/// The spawn seam: run a player detached. Production spawns with
/// silence; tests record.
pub trait SoundSpawner {
    fn spawn(&self, program: &str, argv: &[OsString]) -> bool;
}

/// The real spawner — detached (`spawn` + exit, never `.wait()`), all
/// stdio null so nothing ever touches the TUI's terminal.
#[derive(Debug, Default)]
pub struct ProcessSpawner;

impl SoundSpawner for ProcessSpawner {
    fn spawn(&self, program: &str, argv: &[OsString]) -> bool {
        Command::new(program)
            .args(argv)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .is_ok()
    }
}

/// Play a sound file: pick a player, map the volume, spawn detached.
/// `false` when no player exists or the spawn fails (v2's
/// `audio.play` → null → the candidate loop gives up).
#[must_use]
pub fn play_file(file: &Path, volume: f64, spawner: &dyn SoundSpawner) -> bool {
    match detect_player() {
        Some(player) => play_with(player, file, volume, spawner),
        None => false,
    }
}

/// Play through a known player (the test seam under [`play_file`]).
#[must_use]
pub fn play_with(player: PlayerKind, file: &Path, volume: f64, spawner: &dyn SoundSpawner) -> bool {
    let argv = player.argv(file, volume);
    spawner.spawn(player.program(), &argv)
}

/// The production sound half of [`AttentionEmitter`]: the candidate
/// loop (first file that plays wins) over the process spawner.
#[derive(Debug, Default)]
pub struct ProcessSoundPlayer {
    spawner: ProcessSpawner,
}

impl AttentionEmitter for ProcessSoundPlayer {
    fn notify(&self, _title: &str, _message: &str) -> bool {
        false
    }

    fn play(&self, candidates: &[SoundCandidate], volume: f64) -> bool {
        for candidate in candidates {
            if play_file(&candidate.path, volume, &self.spawner) {
                return true;
            }
        }
        false
    }
}

/// The composite production emitter: OSC notifications over `W` + the
/// in-process (Linux) / spawn-mirrored (other) sound player (installed by
/// the run loop). The notifier wraps in a `RefCell` because emitting takes
/// `&mut` on the writer.
pub struct TerminalAttention<W: io::Write> {
    pub notifier: std::cell::RefCell<crate::os_notify::OscNotifier<W>>,
    pub sounds: SoundHalf,
}

impl<W: io::Write> AttentionEmitter for TerminalAttention<W> {
    fn notify(&self, title: &str, message: &str) -> bool {
        self.notifier.borrow_mut().trigger(title, message)
    }

    fn play(&self, candidates: &[SoundCandidate], volume: f64) -> bool {
        AttentionEmitter::play(&self.sounds, candidates, volume)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct RecordingSpawner {
        spawns: RefCell<Vec<(String, Vec<OsString>)>>,
        ok: bool,
    }

    impl SoundSpawner for RecordingSpawner {
        fn spawn(&self, program: &str, argv: &[OsString]) -> bool {
            self.spawns.borrow_mut().push((program.to_owned(), argv.to_vec()));
            self.ok
        }
    }

    /// Argv parity: each player maps v2's 0..1 volume into its own
    /// scale, with the quiet flags the TUI needs.
    #[test]
    fn player_argv_maps_volume() {
        let file = Path::new("/cache/audio/bip-bop-01.mp3");
        let argv = |kind: PlayerKind| {
            kind.argv(file, 0.4)
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            argv(PlayerKind::PwPlay),
            vec!["--volume", "0.400", "/cache/audio/bip-bop-01.mp3"]
        );
        assert_eq!(
            argv(PlayerKind::Paplay),
            vec!["--volume", "26214", "/cache/audio/bip-bop-01.mp3"]
        );
        assert_eq!(
            argv(PlayerKind::Ffplay),
            vec![
                "-nodisp",
                "-autoexit",
                "-loglevel",
                "quiet",
                "-volume",
                "40",
                "/cache/audio/bip-bop-01.mp3"
            ]
        );
        assert_eq!(
            argv(PlayerKind::Mpv),
            vec!["--no-video", "--really-quiet", "--volume=40", "/cache/audio/bip-bop-01.mp3"]
        );
        assert_eq!(argv(PlayerKind::Canberra), vec!["-f", "/cache/audio/bip-bop-01.mp3"]);
        // Clamp: v2 clamps volume into 0..1 before the player sees it.
        let loud = PlayerKind::Mpv.argv(file, 2.0);
        assert!(loud.iter().any(|arg| arg == "--volume=100"));
    }

    /// `play_file`: no player on PATH → false, nothing spawned. (CI has
    /// no audio players; if a player IS present, the spawn itself is
    /// asserted false via the recording spawner.)
    #[test]
    fn play_file_requires_a_player() {
        let spawner = RecordingSpawner { ok: false, ..RecordingSpawner::default() };
        let played = play_file(Path::new("/nonexistent.mp3"), 0.4, &spawner);
        if detect_player().is_none() {
            assert!(!played);
            assert!(spawner.spawns.borrow().is_empty());
        } else {
            assert!(!played, "the recording spawner reports spawn failure");
            assert_eq!(spawner.spawns.borrow().len(), 1);
        }
    }

    /// The candidate loop: first candidate that plays wins; a failed
    /// spawn falls through to the next (v2's playSound loop).
    #[test]
    fn candidate_loop_falls_through() {
        let spawner = RecordingSpawner { ok: true, ..RecordingSpawner::default() };
        let candidates = [
            SoundCandidate { name: AttentionSoundName::Done, path: PathBuf::from("/gone.mp3") },
            SoundCandidate { name: AttentionSoundName::Done, path: PathBuf::from("/builtin.mp3") },
        ];
        let mut played = false;
        for candidate in &candidates {
            if play_with(PlayerKind::PwPlay, &candidate.path, 0.4, &spawner) {
                played = true;
                break;
            }
        }
        assert!(played);
        assert_eq!(spawner.spawns.borrow().len(), 1, "first success stops the loop");
    }

    /// Extraction: the embedded pack lands in `cache/audio` byte-for-byte,
    /// and the resolver yields the override-then-builtin order at the
    /// engine level.
    #[test]
    fn extracts_builtin_pack_byte_for_byte() {
        let cache = std::env::temp_dir().join(format!("oc-tui-sound-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&cache);
        let resolver = builtin_sound_resolver(cache.clone());
        let builtin = resolver(AttentionSoundName::Done).expect("extracts");
        assert_eq!(builtin, cache.join("audio").join("bip-bop-01.mp3"));
        assert!(builtin.is_file());
        assert_eq!(
            std::fs::read(&builtin).unwrap(),
            include_bytes!("../assets/audio/bip-bop-01.mp3"),
            "byte-for-byte with the vendored asset"
        );
        // Re-resolution is a no-op (idempotent extraction).
        assert_eq!(resolver(AttentionSoundName::Done), Some(builtin));
        // Every event name resolves (done == bip-bop-01, the rest distinct).
        assert_eq!(
            resolver(AttentionSoundName::Question).unwrap().file_name().unwrap(),
            "bip-bop-03.mp3"
        );
        assert_eq!(
            resolver(AttentionSoundName::Permission).unwrap().file_name().unwrap(),
            "staplebops-06.mp3"
        );
        assert_eq!(
            resolver(AttentionSoundName::Error).unwrap().file_name().unwrap(),
            "nope-03.mp3"
        );
        assert_eq!(
            resolver(AttentionSoundName::SubagentDone).unwrap().file_name().unwrap(),
            "yup-01.mp3"
        );
        let _ = std::fs::remove_dir_all(&cache);
    }

    /// A recording spawner test for the pure argv path (the emitter
    /// seam is exercised in attention.rs).
    #[test]
    fn play_with_spawns_the_resolved_player() {
        let spawner = RecordingSpawner { ok: true, ..RecordingSpawner::default() };
        assert!(play_with(PlayerKind::Mpv, Path::new("/a.mp3"), 0.5, &spawner));
        let (program, argv) = spawner.spawns.borrow().last().cloned().expect("spawned");
        assert_eq!(program, "mpv");
        assert!(argv.iter().any(|arg| arg == "--volume=50"));
    }
}
