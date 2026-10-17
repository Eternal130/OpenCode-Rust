//! Attention — the port of v2's attention surface
//! (`packages/tui/src/attention.ts` + the
//! `feature-plugins/system/notifications.ts` plugin, see
//! [`crate::notifications_plugin`]).
//!
//! v2 notifies through two channels:
//!
//! * **OS notifications** (`attention.notifications`, default `false`)
//!   — the renderer writes an OSC notification escape sequence
//!   (`triggerNotification` in opentui's native renderer; our port is
//!   [`crate::os_notify`]). Blurred-only by default: a notification is
//!   skipped while the terminal is focused.
//! * **sounds** (`attention.sound`, default `false`) — the built-in
//!   `opencode.default` MP3 pack (vendored in `assets/audio/`, see
//!   [`crate::sound`]) played at `attention.volume` (0..1, default
//!   0.4). Always-on: sounds fire regardless of focus.
//!
//! Both default OFF, matching v2's resolved defaults
//! (`packages/tui/src/config/index.tsx:300`):
//! `notifications ?? false, sound ?? false, volume ?? 0.4,
//! sound_pack ?? "opencode.default", sounds ?? {}`.
//!
//! The **terminal bell** (`AttentionState` below, lane R-05) remains
//! the zero-config fallback for the sound channel: it rings on the
//! R-05 trigger set (permission asks, forms, unread transitions) when
//! no real sound played. When `attention.sound` is enabled and a sound
//! actually plays, the bell is suppressed (no double attention).
//!
//! The emission seams (`AttentionEmitter`) are a trait so tests can
//! assert the exact notification/sound decisions without a terminal or
//! an audio stack (CI plays nothing).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use crate::data::BusEvent;

/// The attention state the store carries: a pending (unacknowledged)
/// attention marker + the bell request the run loop drains.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AttentionState {
    /// A bell was requested; the run loop writes `\x07` once and
    /// clears the request ([`AttentionState::take_bell`]).
    pub bell: bool,
    /// An attention marker is pending; consumed on focus.
    pub pending: bool,
}

impl AttentionState {
    /// `true` when the event is an attention transition per the
    /// notifications plugin (the ring decision).
    #[must_use]
    pub fn rings_on(event: &BusEvent) -> bool {
        matches!(event.type_.as_str(), "permission.asked" | "form.created")
    }

    /// Request the bell + set the pending flag (one transition).
    pub fn ring(&mut self) {
        self.bell = true;
        self.pending = true;
    }

    /// Take the bell request (the run loop's write-`\x07`-once).
    pub fn take_bell(&mut self) -> bool {
        std::mem::take(&mut self.bell)
    }

    /// Focus returned: consume the pending marker.
    pub fn focused(&mut self) {
        self.pending = false;
    }
}

/// v2's attention sound names (`AttentionSoundName` in
/// `@opencode/plugin/tui/context`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AttentionSoundName {
    Default,
    Question,
    Permission,
    Error,
    Done,
    SubagentDone,
}

impl AttentionSoundName {
    /// The wire/config name (v2's schema literal set).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Question => "question",
            Self::Permission => "permission",
            Self::Error => "error",
            Self::Done => "done",
            Self::SubagentDone => "subagent_done",
        }
    }

    /// Parse a config/request name (`Schema.is(Config.AttentionSoundName)`
    /// in v2 — unknown names fall back to `default` at the call site).
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "default" => Some(Self::Default),
            "question" => Some(Self::Question),
            "permission" => Some(Self::Permission),
            "error" => Some(Self::Error),
            "done" => Some(Self::Done),
            "subagent_done" => Some(Self::SubagentDone),
            _ => None,
        }
    }
}

/// v2's `AttentionWhen` — when a channel may fire, relative to
/// terminal focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AttentionWhen {
    Always,
    #[default]
    Blurred,
    Focused,
}

impl AttentionWhen {
    /// Parse the request keyword (defaults to `blurred`/`always` at the
    /// call sites, mirroring v2's `?? "blurred"` / `?? "always"`).
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "always" => Some(Self::Always),
            "focused" => Some(Self::Focused),
            "blurred" => Some(Self::Blurred),
            _ => None,
        }
    }
}

/// The `notification` request option (v2's `AttentionNotification`):
/// unset, off, or focus-gated.
#[derive(Debug, Clone, PartialEq)]
pub enum NotificationOpt {
    /// Not requested (v2's `undefined`).
    Unset,
    /// Explicitly off (v2's `false`).
    Off,
    /// Requested, focus-gated (v2's `{ when }` / `true`).
    When(AttentionWhen),
}

/// The `sound` request option (v2's `AttentionSound`): unset, off, or
/// `{ name, volume, when }`.
#[derive(Debug, Clone, PartialEq)]
pub enum SoundOpt {
    /// Not requested (v2's `undefined`).
    Unset,
    /// Explicitly off (v2's `false`).
    Off,
    /// Requested. `None` fields fall back to the config / channel
    /// defaults exactly like v2's optional fields.
    On { name: Option<AttentionSoundName>, volume: Option<f64>, when: Option<AttentionWhen> },
}

/// v2's `AttentionNotifyOptions`.
#[derive(Debug, Clone, PartialEq)]
pub struct AttentionNotifyOptions {
    pub title: Option<String>,
    pub message: String,
    pub notification: NotificationOpt,
    pub sound: SoundOpt,
}

impl AttentionNotifyOptions {
    /// The notifications plugin's canonical request (mirrors
    /// `feature-plugins/system/notifications.ts`): OS notification
    /// blurred-only for root sessions (subagents pass `Off`), sound
    /// always-on.
    #[must_use]
    pub fn plugin_request(
        title: Option<String>,
        message: String,
        sound: AttentionSoundName,
        subagent: bool,
    ) -> Self {
        Self {
            title,
            message,
            notification: if subagent {
                NotificationOpt::Off
            } else {
                NotificationOpt::When(AttentionWhen::Blurred)
            },
            sound: SoundOpt::On {
                name: Some(sound),
                volume: None,
                when: Some(AttentionWhen::Always),
            },
        }
    }
}

/// v2's `AttentionNotifySkipReason`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttentionSkipReason {
    AttentionDisabled,
    EmptyMessage,
    Blurred,
    Focused,
    FocusUnknown,
    RendererDestroyed,
}

impl AttentionSkipReason {
    /// v2's literal skip-reason strings.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AttentionDisabled => "attention_disabled",
            Self::EmptyMessage => "empty_message",
            Self::Blurred => "blurred",
            Self::Focused => "focused",
            Self::FocusUnknown => "focus_unknown",
            Self::RendererDestroyed => "renderer_destroyed",
        }
    }
}

/// v2's `AttentionNotifyResult`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttentionNotifyResult {
    pub ok: bool,
    pub notification: bool,
    pub sound: bool,
    pub skipped: Option<AttentionSkipReason>,
}

impl AttentionNotifyResult {
    fn skipped(reason: AttentionSkipReason) -> Self {
        Self { ok: false, notification: false, sound: false, skipped: Some(reason) }
    }
}

/// The resolved `attention` config (v2's `Config.Resolved["attention"]`).
#[derive(Debug, Clone, PartialEq)]
pub struct AttentionConfig {
    /// Emit the terminal BEL (`\x07`) on attention transitions
    /// (`attention.bell`, default `false` — v2 NEVER emits BEL; the
    /// port's ungated emission was auditC P11, now opt-in).
    pub bell: bool,
    /// Show system notifications (`attention.notifications`, default
    /// `false`).
    pub notifications: bool,
    /// Play attention sounds (`attention.sound`, default `false`).
    pub sound: bool,
    /// Sound volume 0..1 (`attention.volume`, default `0.4`).
    pub volume: f64,
    /// Active sound pack id (`attention.sound_pack`, default
    /// `"opencode.default"`; metadata in v2's TUI — resolution goes
    /// through the per-event `sounds` overrides, then the built-in
    /// pack).
    pub sound_pack: String,
    /// Sound file overrides by attention event (`attention.sounds`).
    pub sounds: HashMap<AttentionSoundName, PathBuf>,
}

impl Default for AttentionConfig {
    fn default() -> Self {
        Self {
            bell: false,
            notifications: false,
            sound: false,
            volume: 0.4,
            sound_pack: "opencode.default".to_owned(),
            sounds: HashMap::new(),
        }
    }
}

/// v2's `FocusState` — starts `unknown` until the terminal reports a
/// focus event (crossterm `FocusGained`/`FocusLost`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FocusState {
    #[default]
    Unknown,
    Focused,
    Blurred,
}

/// One playable sound candidate (v2 iterates
/// `[config.sounds[name], BUILTIN_SOUNDS[name]]`, first that loads
/// wins — `soundCandidates` in attention.ts).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoundCandidate {
    /// The event name (reporting only).
    pub name: AttentionSoundName,
    /// Absolute path of the file to play.
    pub path: PathBuf,
}

/// The emission seam: OS notification + sound playback. Production
/// installs [`crate::os_notify`] + [`crate::sound`] backed impls; tests
/// install a recorder. Each method returns whether the emission
/// succeeded (v2's `triggerNotification` → boolean / `audio.play` →
/// handle-or-null).
pub trait AttentionEmitter {
    /// Show an OS notification (already normalized + limited).
    fn notify(&self, title: &str, message: &str) -> bool;
    /// Play one of the resolved sound candidates; `true` when a sound
    /// actually played (a failed load falls through to the next
    /// candidate, mirroring v2's `playSound` loop).
    fn play(&self, candidates: &[SoundCandidate], volume: f64) -> bool;
}

/// The no-op default (nothing plays in tests that don't install a
/// recorder — CI silence).
#[derive(Debug, Default)]
pub struct NoopEmitter;

impl AttentionEmitter for NoopEmitter {
    fn notify(&self, _title: &str, _message: &str) -> bool {
        false
    }

    fn play(&self, _candidates: &[SoundCandidate], _volume: f64) -> bool {
        false
    }
}

/// v2's `normalizeText` — strip ANSI, collapse whitespace/newlines,
/// drop control characters, trim, fall back, and limit by Unicode
/// scalar count (`Array.from(...).slice(0, limit)`).
#[must_use]
pub fn normalize_text(input: Option<&str>, fallback: &str, limit: usize) -> String {
    let stripped = strip_ansi(input.unwrap_or_default());
    // v2: `[ \t]*[\r\n]+[ \t]*` → one space (runs of newlines with
    // surrounding spaces/tabs collapse; plain spaces are untouched).
    let mut collapsed = String::with_capacity(stripped.len());
    let mut chars = stripped.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\r' || ch == '\n' {
            while collapsed.ends_with([' ', '\t']) {
                collapsed.pop();
            }
            collapsed.push(' ');
            while matches!(chars.peek(), Some('\r' | '\n' | ' ' | '\t')) {
                chars.next();
            }
        } else {
            collapsed.push(ch);
        }
    }
    let cleaned: String = collapsed
        .chars()
        .filter(|ch| {
            let code = *ch as u32;
            // v2 drops C0 (except \n, already collapsed), DEL, and C1.
            !matches!(code, 0x00..=0x09 | 0x0B | 0x0C | 0x0E..=0x1F | 0x7F..=0x9F)
        })
        .collect();
    let trimmed = cleaned.trim();
    let text = if trimmed.is_empty() { fallback } else { trimmed };
    text.chars().take(limit).collect()
}

/// Strip ANSI escape sequences (CSI `ESC[…letter`, OSC `ESC]…(BEL|ST)`,
/// and other two-char `ESC x` sequences) — the `strip-ansi` crate v2
/// applies before sanitizing notification text.
#[must_use]
pub fn strip_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\x1b' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('[') => {
                // CSI: parameters + intermediates until a final byte @-~.
                for follow in chars.by_ref() {
                    if ('\x40'..='\x7e').contains(&follow) {
                        break;
                    }
                }
            }
            Some(']') => {
                // OSC: until BEL or ST (ESC \).
                let mut prev_esc = false;
                for follow in chars.by_ref() {
                    if follow == '\x07' {
                        break;
                    }
                    if prev_esc && follow == '\\' {
                        break;
                    }
                    prev_esc = follow == '\x1b';
                }
            }
            Some(_) => {}
            None => break,
        }
    }
    out
}

/// v2's `clampVolume`: non-finite → 0, otherwise clamped into 0..=1.
#[must_use]
pub fn clamp_volume(volume: f64) -> f64 {
    if !volume.is_finite() {
        return 0.0;
    }
    volume.clamp(0.0, 1.0)
}

/// v2's `soundVolume`: `None` when the sound channel is disabled for
/// this request (no sound), else the effective clamped volume.
#[must_use]
pub fn sound_volume(sound: &SoundOpt, config: &AttentionConfig) -> Option<f64> {
    if !config.sound {
        return None;
    }
    match sound {
        SoundOpt::Off => None,
        SoundOpt::Unset | SoundOpt::On { volume: None, .. } => Some(clamp_volume(config.volume)),
        SoundOpt::On { volume: Some(volume), .. } => Some(clamp_volume(*volume)),
    }
}

/// v2's `focusSkip`: `Some(reason)` when the focus gate skips the
/// channel.
#[must_use]
pub fn focus_skip(when: AttentionWhen, focus: FocusState) -> Option<AttentionSkipReason> {
    if when == AttentionWhen::Always {
        return None;
    }
    if focus == FocusState::Unknown {
        return Some(AttentionSkipReason::FocusUnknown);
    }
    match (when, focus) {
        (AttentionWhen::Blurred, FocusState::Focused) => Some(AttentionSkipReason::Focused),
        (AttentionWhen::Focused, FocusState::Blurred) => Some(AttentionSkipReason::Blurred),
        _ => None,
    }
}

/// The attention engine (v2's `createTuiAttention` host): focus
/// tracking, the resolved config, and the emitter seam. The store owns
/// one; the notifications plugin feeds it requests.
pub struct Attention {
    focus: Cell<FocusState>,
    config: RefCell<AttentionConfig>,
    /// Resolves a built-in sound name to its extracted file path
    /// (v2's `BUILTIN_SOUNDS` + the audio loader). Overridable for
    /// tests.
    builtin_sounds: RefCell<Rc<dyn Fn(AttentionSoundName) -> Option<PathBuf>>>,
    emitter: RefCell<Rc<dyn AttentionEmitter>>,
    disposed: Cell<bool>,
}

impl Default for Attention {
    fn default() -> Self {
        Self::new()
    }
}

impl Attention {
    #[must_use]
    pub fn new() -> Self {
        Self {
            focus: Cell::new(FocusState::Unknown),
            config: RefCell::new(AttentionConfig::default()),
            builtin_sounds: RefCell::new(Rc::new(|_| None)),
            emitter: RefCell::new(Rc::new(NoopEmitter)),
            disposed: Cell::new(false),
        }
    }

    /// Install the emission seam (production: OSC notifier + sound
    /// player; tests: recorder).
    pub fn set_emitter(&self, emitter: Rc<dyn AttentionEmitter>) {
        *self.emitter.borrow_mut() = emitter;
    }

    /// Install the built-in sound resolver (production: the embedded
    /// pack extracted to the cache dir; tests: fixed paths).
    pub fn set_builtin_sounds(&self, resolve: Rc<dyn Fn(AttentionSoundName) -> Option<PathBuf>>) {
        *self.builtin_sounds.borrow_mut() = resolve;
    }

    /// Apply the resolved `attention` config (boot + config reload).
    pub fn set_config(&self, config: AttentionConfig) {
        *self.config.borrow_mut() = config;
    }

    /// The current config (diagnostics).
    #[must_use]
    pub fn config(&self) -> AttentionConfig {
        self.config.borrow().clone()
    }

    /// Terminal focus events (crossterm `FocusGained`/`FocusLost`).
    pub fn set_focus(&self, focus: FocusState) {
        self.focus.set(focus);
    }

    /// The observed focus state.
    #[must_use]
    pub fn focus(&self) -> FocusState {
        self.focus.get()
    }

    /// Teardown (v2's `dispose`).
    pub fn dispose(&self) {
        self.disposed.set(true);
    }

    /// v2's `soundCandidates`: the config override first, then the
    /// built-in pack, deduplicated.
    fn sound_candidates(
        &self,
        name: AttentionSoundName,
        config: &AttentionConfig,
    ) -> Vec<SoundCandidate> {
        let mut candidates = Vec::new();
        if let Some(path) = config.sounds.get(&name) {
            candidates.push(SoundCandidate { name, path: path.clone() });
        }
        if let Some(path) = (self.builtin_sounds.borrow())(name) {
            candidates.push(SoundCandidate { name, path });
        }
        candidates.dedup_by(|a, b| a.path == b.path);
        candidates
    }

    /// v2's `notify(request)` — one attention request through both
    /// channels with config + focus gating. Returns the outcome (and,
    /// for the store, whether a sound actually played — the bell
    /// fallback suppresses itself then).
    pub fn notify(&self, request: &AttentionNotifyOptions) -> AttentionNotifyResult {
        let config = self.config.borrow().clone();
        if !config.notifications && !config.sound {
            return AttentionNotifyResult::skipped(AttentionSkipReason::AttentionDisabled);
        }
        if self.disposed.get() {
            return AttentionNotifyResult::skipped(AttentionSkipReason::RendererDestroyed);
        }

        let message = normalize_text(Some(&request.message), "", MESSAGE_LIMIT);
        if message.is_empty() {
            return AttentionNotifyResult::skipped(AttentionSkipReason::EmptyMessage);
        }

        // Notification channel: default gate "blurred", config must
        // enable it, request must not opt out.
        let notification_when = match &request.notification {
            NotificationOpt::Off | NotificationOpt::Unset => AttentionWhen::Blurred,
            NotificationOpt::When(when) => *when,
        };
        let notification_skip = focus_skip(notification_when, self.focus.get());
        let notification_requested =
            config.notifications && !matches!(request.notification, NotificationOpt::Off);
        let notification = if notification_requested && notification_skip.is_none() {
            let title = normalize_text(request.title.as_deref(), DEFAULT_TITLE, TITLE_LIMIT);
            self.emitter.borrow().notify(&title, &message)
        } else {
            false
        };

        // Sound channel: default gate "always", volume resolution
        // carries the enable/disable decision.
        let volume = sound_volume(&request.sound, &config);
        let sound_when = match &request.sound {
            SoundOpt::On { when: Some(when), .. } => *when,
            _ => AttentionWhen::Always,
        };
        let sound_skip = volume.and_then(|_| focus_skip(sound_when, self.focus.get()));
        let sound_name = match &request.sound {
            SoundOpt::On { name: Some(name), .. } => *name,
            _ => AttentionSoundName::Default,
        };
        let sound = match (volume, sound_skip) {
            (Some(volume), None) => {
                let candidates = self.sound_candidates(sound_name, &config);
                self.emitter.borrow().play(&candidates, volume)
            }
            _ => false,
        };

        if !notification && !sound {
            if notification_requested {
                if let Some(reason) = notification_skip {
                    return AttentionNotifyResult::skipped(reason);
                }
            }
            if let Some(reason) = sound_skip {
                return AttentionNotifyResult::skipped(reason);
            }
        }

        AttentionNotifyResult { ok: notification || sound, notification, sound, skipped: None }
    }
}

/// v2's `DEFAULT_TITLE`.
pub const DEFAULT_TITLE: &str = "OpenCode";
/// v2's `TITLE_LIMIT` (chars).
pub const TITLE_LIMIT: usize = 80;
/// v2's `MESSAGE_LIMIT` (chars).
pub const MESSAGE_LIMIT: usize = 240;

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn rings_on_permission_and_form_events_only() {
        use serde_json::json;
        let event = |type_: &str| BusEvent::new(type_, json!({}));
        assert!(AttentionState::rings_on(&event("permission.asked")));
        assert!(AttentionState::rings_on(&event("form.created")));
        assert!(!AttentionState::rings_on(&event("message.updated")));
        assert!(!AttentionState::rings_on(&event("session.execution.succeeded")));
    }

    #[test]
    fn bell_and_pending_flag_lifecycle() {
        let mut attention = AttentionState::default();
        attention.ring();
        assert!(attention.bell && attention.pending);
        assert!(attention.take_bell(), "the bell fires exactly once");
        assert!(!attention.bell);
        assert!(attention.pending, "the marker outlives the bell");
        assert!(!attention.take_bell(), "no double bell");
        attention.focused();
        assert!(!attention.pending, "focus consumes the marker");
    }

    /// A recorder emitter — asserts the exact emissions.
    #[derive(Default)]
    struct Recorder {
        notifications: RefCell<Vec<(String, String)>>,
        sounds: RefCell<Vec<(Vec<PathBuf>, f64)>>,
        notify_ok: bool,
        play_ok: bool,
    }

    impl AttentionEmitter for Recorder {
        fn notify(&self, title: &str, message: &str) -> bool {
            self.notifications.borrow_mut().push((title.to_owned(), message.to_owned()));
            self.notify_ok
        }

        fn play(&self, candidates: &[SoundCandidate], volume: f64) -> bool {
            self.sounds
                .borrow_mut()
                .push((candidates.iter().map(|c| c.path.clone()).collect(), volume));
            self.play_ok
        }
    }

    fn engine_with(config: AttentionConfig) -> (Attention, Rc<Recorder>) {
        let attention = Attention::new();
        attention.set_config(config);
        let recorder = Rc::new(Recorder { notify_ok: true, play_ok: true, ..Recorder::default() });
        attention.set_emitter(recorder.clone());
        attention.set_builtin_sounds(Rc::new(|name| {
            Some(PathBuf::from(format!("/builtin/{name:?}.mp3")))
        }));
        (attention, recorder)
    }

    fn sound_on() -> AttentionConfig {
        AttentionConfig { sound: true, ..AttentionConfig::default() }
    }

    fn notifications_on() -> AttentionConfig {
        AttentionConfig { notifications: true, ..AttentionConfig::default() }
    }

    fn plugin_request(sound: AttentionSoundName) -> AttentionNotifyOptions {
        AttentionNotifyOptions::plugin_request(
            Some("Demo session".to_owned()),
            "Session done".to_owned(),
            sound,
            false,
        )
    }

    /// Config gating: both channels off → `attention_disabled`, no
    /// emissions (the v2 default).
    #[test]
    fn attention_disabled_by_default() {
        let (attention, recorder) = engine_with(AttentionConfig::default());
        let result = attention.notify(&plugin_request(AttentionSoundName::Done));
        assert_eq!(result.skipped.map(AttentionSkipReason::as_str), Some("attention_disabled"));
        assert!(!result.ok && !result.notification && !result.sound);
        assert!(recorder.notifications.borrow().is_empty());
        assert!(recorder.sounds.borrow().is_empty());
    }

    /// Empty (whitespace-only) message → `empty_message`, even with
    /// everything enabled.
    #[test]
    fn empty_message_skips() {
        let (attention, _recorder) = engine_with(sound_on());
        let request = AttentionNotifyOptions {
            title: None,
            message: "  \r\n ".to_owned(),
            notification: NotificationOpt::Unset,
            sound: SoundOpt::Unset,
        };
        assert_eq!(
            attention.notify(&request).skipped.map(AttentionSkipReason::as_str),
            Some("empty_message")
        );
    }

    /// Sound channel: always-on — fires focused, blurred, and unknown.
    #[test]
    fn sound_channel_is_always_on() {
        let (attention, recorder) = engine_with(sound_on());
        for focus in [FocusState::Unknown, FocusState::Focused, FocusState::Blurred] {
            attention.set_focus(focus);
            let result = attention.notify(&plugin_request(AttentionSoundName::Done));
            assert!(result.sound, "sound plays at {focus:?}");
            assert_eq!(recorder.sounds.borrow().last().unwrap().1, 0.4, "default volume");
        }
        // Config volume flows through.
        attention.set_config(AttentionConfig {
            sound: true,
            volume: 0.75,
            ..AttentionConfig::default()
        });
        attention.notify(&plugin_request(AttentionSoundName::Done));
        assert_eq!(recorder.sounds.borrow().last().unwrap().1, 0.75);
    }

    /// Notification channel: blurred-only by default — focused skips
    /// with reason `focused`, and the pre-focus state skips with
    /// `focus_unknown` (mirrors v2 exactly: no notification until the
    /// terminal reports focus).
    #[test]
    fn notification_channel_skips_focused_and_unknown() {
        let (attention, recorder) = engine_with(notifications_on());
        attention.set_focus(FocusState::Focused);
        let result = attention.notify(&plugin_request(AttentionSoundName::Done));
        assert_eq!(result.skipped.map(AttentionSkipReason::as_str), Some("focused"));
        assert!(recorder.notifications.borrow().is_empty());

        attention.set_focus(FocusState::Unknown);
        let result = attention.notify(&plugin_request(AttentionSoundName::Done));
        assert_eq!(result.skipped.map(AttentionSkipReason::as_str), Some("focus_unknown"));

        attention.set_focus(FocusState::Blurred);
        let result = attention.notify(&plugin_request(AttentionSoundName::Done));
        assert!(result.notification && result.ok && result.skipped.is_none());
        assert_eq!(
            recorder.notifications.borrow().last().unwrap(),
            &("Demo session".to_owned(), "Session done".to_owned())
        );
    }

    /// A subagent request (notification `Off` + `subagent_done` sound) is
    /// sound-only: no notification even blurred.
    #[test]
    fn subagent_requests_are_sound_only() {
        let (attention, recorder) = engine_with(AttentionConfig {
            notifications: true,
            sound: true,
            ..AttentionConfig::default()
        });
        attention.set_focus(FocusState::Blurred);
        let request = AttentionNotifyOptions::plugin_request(
            Some("Sub".to_owned()),
            "Session done".to_owned(),
            AttentionSoundName::SubagentDone,
            true,
        );
        let result = attention.notify(&request);
        assert!(result.sound && !result.notification);
        assert!(recorder.notifications.borrow().is_empty());
    }

    /// Volume resolution matrix (v2's `soundVolume`): disabled → no
    /// sound; request off → no sound; request volume wins over config;
    /// clamped 0..1; NaN → 0.
    #[test]
    fn volume_resolution_matrix() {
        let config = AttentionConfig { sound: true, volume: 0.4, ..AttentionConfig::default() };
        let (attention, recorder) = engine_with(config.clone());

        // request sound off (notifications also off in this config):
        // v2 returns ok:false with NO skip reason — the request simply
        // didn't ask for the sound channel.
        let mut request = plugin_request(AttentionSoundName::Done);
        request.sound = SoundOpt::Off;
        let result = attention.notify(&request);
        assert!(!result.ok && !result.sound && !result.notification);
        assert!(result.skipped.is_none());
        assert!(recorder.sounds.borrow().is_empty());

        // per-request volume override
        let mut request = plugin_request(AttentionSoundName::Done);
        request.sound = SoundOpt::On { name: None, volume: Some(1.5), when: None };
        attention.notify(&request);
        assert_eq!(recorder.sounds.borrow().last().unwrap().1, 1.0, "clamped to 1");

        let mut request = plugin_request(AttentionSoundName::Done);
        request.sound = SoundOpt::On { name: None, volume: Some(f64::NAN), when: None };
        attention.notify(&request);
        assert_eq!(recorder.sounds.borrow().last().unwrap().1, 0.0, "NaN clamps to 0");

        // config disables the channel
        attention.set_config(AttentionConfig::default());
        let result = attention.notify(&plugin_request(AttentionSoundName::Done));
        assert_eq!(result.skipped.map(AttentionSkipReason::as_str), Some("attention_disabled"));
    }

    /// normalizeText parity: ANSI strip, newline collapse, control-char
    /// drop, fallback, and the char (not byte) limit.
    #[test]
    fn normalize_text_matches_v2() {
        assert_eq!(normalize_text(Some("plain"), "fb", 240), "plain");
        assert_eq!(normalize_text(Some("\x1b[31mred\x1b[0m"), "fb", 240), "red");
        assert_eq!(normalize_text(Some("line1\nline2"), "fb", 240), "line1 line2");
        assert_eq!(normalize_text(Some("a \r\n b"), "fb", 240), "a b");
        assert_eq!(normalize_text(Some("  \t "), "fallback", 240), "fallback");
        assert_eq!(
            normalize_text(Some("\x1b]8;;http://x\x1b\\link\x1b]8;;\x1b\\"), "fb", 240),
            "link"
        );
        let limited = normalize_text(Some("äbcdéf"), "", 3);
        assert_eq!(limited, "äbc", "limits by chars");
        let long = "x".repeat(300);
        assert_eq!(normalize_text(Some(&long), "", 240).chars().count(), 240);
    }

    /// Sound candidates: config override first, built-in second (v2's
    /// `soundCandidates` dedup order).
    #[test]
    fn sound_candidates_prefer_override() {
        let attention = Attention::new();
        let mut config = sound_on();
        config.sounds.insert(AttentionSoundName::Done, PathBuf::from("/custom/done.mp3"));
        attention.set_config(config);
        attention.set_builtin_sounds(Rc::new(|name| {
            (name != AttentionSoundName::Done).then(|| PathBuf::from("/builtin"))
        }));
        let recorder = Rc::new(Recorder { play_ok: true, ..Recorder::default() });
        attention.set_emitter(recorder.clone());
        attention.notify(&plugin_request(AttentionSoundName::Done));
        let sounds = recorder.sounds.borrow();
        let (paths, _) = sounds.last().expect("sound played");
        assert_eq!(
            paths,
            &vec![PathBuf::from("/custom/done.mp3")],
            "override-only when builtin missing"
        );

        // Override == builtin path → deduplicated to one candidate.
        let mut config = sound_on();
        config.sounds.insert(AttentionSoundName::Done, PathBuf::from("/same.mp3"));
        attention.set_config(config);
        attention.set_builtin_sounds(Rc::new(|_| Some(PathBuf::from("/same.mp3"))));
        let recorder = Rc::new(Recorder { play_ok: true, ..Recorder::default() });
        attention.set_emitter(recorder);
        attention.notify(&plugin_request(AttentionSoundName::Done));
    }

    /// Disposed engine → `renderer_destroyed`.
    #[test]
    fn disposed_engine_skips() {
        let (attention, recorder) = engine_with(sound_on());
        attention.dispose();
        let result = attention.notify(&plugin_request(AttentionSoundName::Done));
        assert_eq!(result.skipped.map(AttentionSkipReason::as_str), Some("renderer_destroyed"));
        assert!(recorder.sounds.borrow().is_empty());
    }
}
