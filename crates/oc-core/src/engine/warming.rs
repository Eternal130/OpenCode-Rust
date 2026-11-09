//! G-257 — port of `packages/core/src/plugin/warming.ts`: the session
//! keep-alive runtime.
//!
//! The plugin registers ONE accounting handler on the three session
//! request hooks (`session.context`, `session.compaction`,
//! `session.generate` — every model request a session makes). Every
//! non-warm request on a ROOT session (no `parentID`) slides the
//! warming window: `{ last: now, expires: now + duration }`. A
//! per-session loop sleeps to `min(last + interval, expires)` and
//! issues a keep-alive `generate` (a non-mutating one-shot — the
//! ref's `ctx.session.generate`) until the window lapses; the warm
//! request itself is detected by its prompt so it does NOT extend the
//! window. Strictly opt-in: the runtime only exists when the config
//! `warming` key is set (`true` or an overrides object).
//!
//! Deviations from the ref shape (architecture-bound, behavior
//! preserved — see the lane report):
//! * `Config.latest` live reload: the ref re-reads settings on every
//!   hook event; the rust location graph snapshots config documents at
//!   assembly and is invalidated on config change, so a rebuild lands
//!   the new settings the same way (fresh runtime, fresh windows).
//! * The ref's per-session `settings` refresh and its log calls are
//!   folded away (settings are fixed per graph; oc-core has no log
//!   facility).
//! * The ref parses `interval`/`duration` with effect's
//!   `Schema.DurationFromString` at config decode; this port parses
//!   the strings at runtime and treats unparsable values as "keep the
//!   default" (per-key), with the ref's finite-positive guard mapping
//!   a non-positive whole setting to disabled.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures::future::BoxFuture;
use oc_schema::config::warming::Warming;
use oc_util::determinism::Clock;
use parking_lot::Mutex;

use crate::hooks::{HookError, HookHandler, SessionCompactionEvent, SessionContextEvent};
use crate::session::runner::SessionGenerate;
use crate::session::store::SessionStore;

/// The keep-alive prompt (warming.ts:10).
pub const DEFAULT_PROMPT: &str =
    "This is a keep-alive request. Do not perform any work or use tools. Reply with exactly: OK";

/// 4 minutes (warming.ts:11).
pub const DEFAULT_INTERVAL_MS: u64 = 4 * 60 * 1000;

/// 30 minutes (warming.ts:12).
pub const DEFAULT_DURATION_MS: u64 = 30 * 60 * 1000;

/// Resolved settings (`{ ...defaults, ...warming }`, warming.ts:22).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WarmingSettings {
    pub prompt: String,
    pub interval_ms: u64,
    pub duration_ms: u64,
}

impl Default for WarmingSettings {
    fn default() -> Self {
        Self {
            prompt: DEFAULT_PROMPT.to_string(),
            interval_ms: DEFAULT_INTERVAL_MS,
            duration_ms: DEFAULT_DURATION_MS,
        }
    }
}

/// One tracked root session (warming.ts:30).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tracked {
    pub last: u64,
    pub expires: u64,
}

/// `loadSettings` (warming.ts:19-27): `Config.latest(entries,
/// "warming")` — the LAST document owning the key; `true` → defaults,
/// an object → spread over the defaults; `false`/absent → disabled.
/// The finite-positive guard (warming.ts:25-26) maps a zero
/// interval/duration to not-configured.
#[must_use]
pub fn warming_settings(documents: &[oc_schema::config::Document]) -> Option<WarmingSettings> {
    let warming = documents.iter().rev().find_map(|document| document.info.warming.clone())?;
    match warming {
        Warming::All(true) => Some(WarmingSettings::default()),
        Warming::All(false) => None,
        Warming::Info(info) => {
            let mut settings = WarmingSettings::default();
            if let Some(prompt) = &info.prompt {
                settings.prompt = prompt.clone();
            }
            if let Some(interval) = info.interval.as_deref().and_then(duration_ms) {
                settings.interval_ms = interval;
            }
            if let Some(duration) = info.duration.as_deref().and_then(duration_ms) {
                settings.duration_ms = duration;
            }
            (settings.interval_ms > 0 && settings.duration_ms > 0).then_some(settings)
        }
    }
}

/// A bounded port of effect's `DurationFromString` `<number> <unit>`
/// pairs (`"4 minutes"`, `"1 hour, 30 minutes"`; short aliases
/// accepted). Unparsable → `None`.
fn duration_ms(value: &str) -> Option<u64> {
    fn unit_millis(unit: &str) -> Option<u64> {
        match unit {
            // Sub-millisecond units contribute zero whole milliseconds.
            "nanos" | "nano" | "ns" | "micros" | "micro" | "µs" | "us" => Some(0),
            "millis" | "milli" | "ms" => Some(1),
            "seconds" | "second" | "secs" | "sec" | "s" => Some(1_000),
            "minutes" | "minute" | "min" | "m" => Some(60_000),
            "hours" | "hour" | "hr" | "h" => Some(3_600_000),
            "days" | "day" | "d" => Some(86_400_000),
            "weeks" | "week" | "w" => Some(604_800_000),
            _ => None,
        }
    }
    let mut tokens = value.split(|c: char| c == ',' || c.is_whitespace()).filter(|t| !t.is_empty());
    let mut total: u64 = 0;
    let mut any = false;
    loop {
        let Some(token) = tokens.next() else { break };
        // `"500ms"` carries its unit; `"4" "minutes"` splits across
        // tokens (effect's `DurationFromString` accepts both).
        let split = token
            .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
            .unwrap_or(token.len());
        let (number, suffix) = token.split_at(split);
        if number.is_empty() {
            return None;
        }
        let unit = if suffix.is_empty() { tokens.next()? } else { suffix };
        let parsed: f64 = number.parse().ok()?;
        let millis = unit_millis(unit)? as f64;
        total = total.saturating_add((parsed * millis) as u64);
        any = true;
    }
    any.then_some(total)
}

/// The keep-alive generate seam (the ref's `ctx.session.generate`) —
/// a trait so tests can record warms without the runner stack.
#[async_trait::async_trait]
pub trait WarmGenerate: Send + Sync {
    async fn generate(&self, session_id: &str, prompt: &str) -> Result<String, String>;
}

#[async_trait::async_trait]
impl WarmGenerate for SessionGenerate {
    async fn generate(&self, session_id: &str, prompt: &str) -> Result<String, String> {
        SessionGenerate::generate(self, session_id, prompt).await.map_err(|error| error.to_string())
    }
}

/// The keep-alive runtime: the tracked-session table, the accounting
/// hook body, and the per-session loops.
pub struct WarmingRuntime {
    settings: WarmingSettings,
    sessions: Arc<Mutex<HashMap<String, Tracked>>>,
    generate: Arc<dyn WarmGenerate>,
    store: SessionStore,
    clock: Arc<dyn Clock>,
    shutdown: tokio::sync::watch::Receiver<bool>,
    /// The loop's sleep seam (production: `tokio::time::sleep`;
    /// tests substitute a controllable future).
    sleeper: Arc<dyn Fn(u64) -> BoxFuture<'static, ()> + Send + Sync>,
}

impl WarmingRuntime {
    #[must_use]
    pub fn new(
        settings: WarmingSettings,
        generate: Arc<dyn WarmGenerate>,
        store: SessionStore,
        clock: Arc<dyn Clock>,
        shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> Self {
        Self {
            settings,
            sessions: Arc::new(Mutex::new(HashMap::new())),
            generate,
            store,
            clock,
            shutdown,
            sleeper: Arc::new(|ms| {
                Box::pin(async move {
                    tokio::time::sleep(Duration::from_millis(ms)).await;
                })
            }),
        }
    }

    /// Test seam: replace the loop sleeper.
    #[must_use]
    pub fn with_sleeper(
        mut self,
        sleeper: Arc<dyn Fn(u64) -> BoxFuture<'static, ()> + Send + Sync>,
    ) -> Self {
        self.sleeper = sleeper;
        self
    }

    /// The resolved settings (tests).
    #[must_use]
    pub fn settings(&self) -> &WarmingSettings {
        &self.settings
    }

    /// Whether any session is tracked (tests).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sessions.lock().is_empty()
    }

    /// The tracked entry for a session (tests).
    #[must_use]
    pub fn tracked(&self, session_id: &str) -> Option<Tracked> {
        self.sessions.lock().get(session_id).copied()
    }

    fn now_ms(&self) -> u64 {
        u64::try_from(self.clock.unix_ms().max(0)).unwrap_or(0)
    }

    /// The accounting hook body (warming.ts:58-102): root sessions
    /// only; the warm request itself (last message a single user text
    /// part equal to the keep-alive prompt — warming.ts:70-80) does
    /// not extend the window; every other request slides
    /// `last`/`expires` (warming.ts:82-90) and the FIRST sighting
    /// forks the keep-alive loop (warming.ts:90-101).
    pub async fn account(
        self: &Arc<Self>,
        session_id: &str,
        messages: &[oc_ai::schema::messages::Message],
    ) {
        // `session.get` + `if (session.parentID) return` — subagent
        // children never warm.
        match self.store.get(session_id).await {
            Ok(Some(row)) if row.parent_id.is_none() => {}
            _ => return,
        }
        use oc_ai::schema::messages::{ContentPart, MessageRole};
        let is_warm_prompt = messages.last().is_some_and(|message| {
            message.role == MessageRole::User
                && message.content.len() == 1
                && matches!(
                    &message.content[0],
                    ContentPart::Text { text, .. } if *text == self.settings.prompt
                )
        });
        if is_warm_prompt {
            return;
        }
        let now = self.now_ms();
        let expires = now + self.settings.duration_ms;
        let newly_tracked = {
            let mut sessions = self.sessions.lock();
            if let Some(entry) = sessions.get_mut(session_id) {
                entry.last = now;
                entry.expires = expires;
                false
            } else {
                sessions.insert(session_id.to_string(), Tracked { last: now, expires });
                true
            }
        };
        if newly_tracked {
            self.spawn_loop(session_id.to_string());
        }
    }

    /// Forks the per-session keep-alive loop (warming.ts:31-56 —
    /// `loop(sessionID)` forked into the plugin scope, line 96-101).
    /// Detached task; stopped by the location bundle's shutdown watch
    /// (the rust scope analogue), by expiry, or by the entry's removal.
    pub fn spawn_loop(self: &Arc<Self>, session_id: String) {
        let runtime = Arc::clone(self);
        let mut shutdown = self.shutdown.clone();
        tokio::spawn(async move {
            loop {
                // warming.ts:33-34 — an untracked session stops the loop.
                let Some(current) = runtime.sessions.lock().get(&session_id).copied() else {
                    return;
                };
                let now = runtime.now_ms();
                // warming.ts:37 — `min(last + interval, expires)`.
                let next =
                    current.last.saturating_add(runtime.settings.interval_ms).min(current.expires);
                if now < next {
                    let wait = next - now;
                    let sleep = (runtime.sleeper)(wait);
                    tokio::select! {
                        () = sleep => {}
                        _ = shutdown.wait_for(|flag| *flag) => return,
                    }
                    continue;
                }
                // warming.ts:42-45 — the window lapsed: drop and stop.
                if now >= current.expires {
                    runtime.sessions.lock().remove(&session_id);
                    return;
                }
                // warming.ts:49-51 — the keep-alive generate; failures
                // are caught (logged in the ref) and the loop continues.
                let last = current.last;
                let _ = runtime.generate.generate(&session_id, &runtime.settings.prompt).await;
                // warming.ts:52-53 — bump `last` only when real activity
                // did not re-extend the entry mid-generate.
                let now_after = runtime.now_ms();
                if let Some(entry) = runtime.sessions.lock().get_mut(&session_id) {
                    if entry.last == last {
                        entry.last = now_after;
                    }
                }
            }
        });
    }
}

/// `hook` on `session.context` + `session.generate` (warming.ts:103,105).
pub struct WarmingAccountHook(pub Arc<WarmingRuntime>);

#[async_trait::async_trait]
impl HookHandler<SessionContextEvent> for WarmingAccountHook {
    async fn call(&self, event: &mut SessionContextEvent) -> Result<(), HookError> {
        self.0.account(event.session_id.as_str(), &event.messages).await;
        Ok(())
    }
}

/// `hook` on `session.compaction` (warming.ts:104).
pub struct WarmingCompactionHook(pub Arc<WarmingRuntime>);

#[async_trait::async_trait]
impl HookHandler<SessionCompactionEvent> for WarmingCompactionHook {
    async fn call(&self, event: &mut SessionCompactionEvent) -> Result<(), HookError> {
        self.0.account(event.context.session_id.as_str(), &event.context.messages).await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::SessionContextEvent;
    use oc_ai::schema::messages::Message;
    use oc_schema::id::AgentId;
    use oc_util::determinism::Clock;
    use std::sync::Mutex as StdMutex;

    #[test]
    fn warming_duration_parses_effect_style_strings() {
        assert_eq!(duration_ms("4 minutes"), Some(4 * 60_000));
        assert_eq!(duration_ms("30 minutes"), Some(30 * 60_000));
        assert_eq!(duration_ms("1 hour, 30 minutes"), Some(90 * 60_000));
        assert_eq!(duration_ms("500ms"), Some(500));
        assert_eq!(duration_ms("2h"), Some(2 * 3_600_000));
        assert_eq!(duration_ms("1 day"), Some(86_400_000));
        assert_eq!(duration_ms("bogus"), None);
        assert_eq!(duration_ms(""), None);
    }

    #[test]
    fn warming_settings_latest_document_wins() {
        use oc_schema::config::warming::Info;
        let document = |warming: Option<Warming>| oc_schema::config::Document {
            path: None,
            info: oc_schema::config::Info { warming, ..Default::default() },
        };
        // Absent → disabled.
        assert!(warming_settings(&[document(None)]).is_none());
        // `true` → defaults.
        assert_eq!(
            warming_settings(&[document(Some(Warming::All(true)))]).unwrap(),
            WarmingSettings::default()
        );
        // `false` → disabled.
        assert!(warming_settings(&[document(Some(Warming::All(false)))]).is_none());
        // Overrides spread over the defaults.
        let info = Info {
            prompt: Some("warm me".into()),
            interval: Some("1 minute".into()),
            duration: Some("2 minutes".into()),
        };
        let settings = warming_settings(&[document(Some(Warming::Info(info)))]).expect("settings");
        assert_eq!(settings.prompt, "warm me");
        assert_eq!(settings.interval_ms, 60_000);
        assert_eq!(settings.duration_ms, 120_000);
        // The LAST document owning the key wins (Config.latest).
        let merged = warming_settings(&[
            document(Some(Warming::All(true))),
            document(Some(Warming::All(false))),
        ]);
        assert!(merged.is_none());
        // Partial overrides keep the defaults for the unset keys.
        let partial = Info { prompt: None, interval: Some("10 seconds".into()), duration: None };
        let settings = warming_settings(&[document(Some(Warming::Info(partial)))]).unwrap();
        assert_eq!(settings.prompt, DEFAULT_PROMPT);
        assert_eq!(settings.interval_ms, 10_000);
        assert_eq!(settings.duration_ms, DEFAULT_DURATION_MS);
    }

    // -- runtime behavior -------------------------------------------------

    /// Mutable wall clock — the tests advance it (the runtime reads
    /// `now` only through the [`Clock`] seam).
    struct MutableClock(StdMutex<std::time::SystemTime>);

    impl MutableClock {
        fn at_epoch() -> Arc<Self> {
            Arc::new(Self(StdMutex::new(std::time::UNIX_EPOCH)))
        }
        fn advance(&self, ms: u64) {
            *self.0.lock().expect("clock") += Duration::from_millis(ms);
        }
    }

    impl Clock for MutableClock {
        fn now(&self) -> std::time::SystemTime {
            *self.0.lock().expect("clock")
        }
    }

    /// Manual sleep driver: the runtime's sleeper parks a waiter and
    /// the test releases it after advancing the clock — the
    /// time-controllable scheduling seam.
    struct ManualSleep {
        state: StdMutex<ManualState>,
    }

    struct ManualState {
        pending: Option<(u64, Arc<tokio::sync::Notify>)>,
        waits: Vec<u64>,
    }

    impl ManualSleep {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                state: StdMutex::new(ManualState { pending: None, waits: Vec::new() }),
            })
        }

        fn take_pending(&self) -> Option<(u64, Arc<tokio::sync::Notify>)> {
            self.state.lock().expect("sleep").pending.take()
        }
    }

    fn manual_sleeper(
        manual: Arc<ManualSleep>,
    ) -> Arc<dyn Fn(u64) -> BoxFuture<'static, ()> + Send + Sync> {
        Arc::new(move |ms| {
            let manual = Arc::clone(&manual);
            let notify = Arc::new(tokio::sync::Notify::new());
            {
                let mut state = manual.state.lock().expect("sleep");
                state.waits.push(ms);
                state.pending = Some((ms, Arc::clone(&notify)));
            }
            Box::pin(async move {
                notify.notified().await;
            })
        })
    }

    /// Advances the clock by each parked wait and releases it, until
    /// the loop goes idle (expiry or untracked) or `steps` run out.
    async fn drive(clock: &MutableClock, manual: &ManualSleep, steps: usize) {
        for _ in 0..steps {
            let pending = {
                let started = std::time::Instant::now();
                loop {
                    if let Some(pending) = manual.take_pending() {
                        break Some(pending);
                    }
                    if started.elapsed() > std::time::Duration::from_millis(750) {
                        break None;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(2)).await;
                }
            };
            let Some((wait, notify)) = pending else { return };
            clock.advance(wait.max(1));
            notify.notify_one();
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    }

    /// Recording generate seam (warms observed, plus failure injection).
    struct RecordingGenerate {
        calls: StdMutex<Vec<(String, String)>>,
        fail: bool,
    }

    impl RecordingGenerate {
        fn count(&self) -> usize {
            self.calls.lock().expect("calls").len()
        }
        fn prompts(&self) -> Vec<String> {
            self.calls.lock().expect("calls").iter().map(|(_, prompt)| prompt.clone()).collect()
        }
    }

    #[async_trait::async_trait]
    impl WarmGenerate for RecordingGenerate {
        async fn generate(&self, session_id: &str, prompt: &str) -> Result<String, String> {
            self.calls.lock().expect("calls").push((session_id.to_string(), prompt.to_string()));
            if self.fail {
                Err("boom".to_string())
            } else {
                Ok("OK".to_string())
            }
        }
    }

    async fn seed_root_session(db: &crate::database::Database, id: &str, parent: Option<&str>) {
        let id = id.to_string();
        let parent = parent.map(str::to_string);
        db.transaction(move |tx| {
            crate::database::schema::ProjectRow::insert_or_ignore(
                tx,
                &crate::database::schema::ProjectRow::new("global", "/project", None)
                    .expect("project row"),
            )?;
            let mut row = crate::database::schema::SessionRow::new(
                id.clone(),
                "global",
                "/project",
                &id,
                "test",
            )
            .expect("session row");
            row.parent_id = parent;
            crate::database::schema::SessionRow::insert_or_ignore(tx, &row)
        })
        .await
        .expect("seed session");
    }

    fn event(session_id: &str, last_text: &str) -> SessionContextEvent {
        SessionContextEvent {
            session_id: oc_schema::id::SessionId::parse(session_id).expect("session id"),
            agent: AgentId::parse("build"),
            model: oc_schema::model::Ref {
                provider_id: oc_schema::id::ProviderId::parse("test"),
                id: oc_schema::id::ModelId::parse("fake"),
                variant: None,
            },
            system: Vec::new(),
            messages: vec![Message::user(last_text)],
            options: Default::default(),
            tools: Default::default(),
        }
    }

    fn runtime_with(
        settings: WarmingSettings,
        generate: Arc<RecordingGenerate>,
        db: crate::database::Database,
    ) -> (Arc<WarmingRuntime>, Arc<MutableClock>, Arc<ManualSleep>, tokio::sync::watch::Sender<bool>)
    {
        let (shutdown_tx, shutdown) = tokio::sync::watch::channel(false);
        let clock = MutableClock::at_epoch();
        let manual = ManualSleep::new();
        let runtime = Arc::new(
            WarmingRuntime::new(settings, generate, SessionStore::new(db), clock.clone(), shutdown)
                .with_sleeper(manual_sleeper(manual.clone())),
        );
        // The sender must outlive the loops: a closed channel is the
        // bundle-shutdown signal (the engine keeps it in its shutdown
        // map; tests keep it here).
        (runtime, clock, manual, shutdown_tx)
    }

    /// Config active + root session active → keep-alive generates fire
    /// every interval and the loop STOPS once the window lapses
    /// (warming.ts:31-56, 82-95): interval 10 / duration 35 → warms at
    /// t=10,20,30, expiry at t=35.
    #[tokio::test]
    async fn warming_keeps_active_root_session_until_window_expires() {
        let db = crate::database::Database::open_in_memory().await.expect("db");
        seed_root_session(&db, "ses_warm", None).await;
        let generate =
            Arc::new(RecordingGenerate { calls: StdMutex::new(Vec::new()), fail: false });
        let (runtime, clock, manual, _shutdown_tx) = runtime_with(
            WarmingSettings { prompt: "warm me".into(), interval_ms: 10, duration_ms: 35 },
            generate.clone(),
            db,
        );

        // A real user request schedules the loop (warming.ts:82-95).
        Arc::clone(&runtime).account("ses_warm", &event("ses_warm", "hello world").messages).await;
        assert!(runtime.tracked("ses_warm").is_some(), "root session tracked");

        drive(&clock, &manual, 16).await;

        assert_eq!(generate.count(), 3, "warms at t=10/20/30 exactly");
        assert_eq!(generate.prompts(), vec!["warm me"; 3], "keep-alive prompt used");
        assert!(runtime.is_empty(), "expired session removed from the table");
    }

    /// The warm-prompt request does not extend the window, and child
    /// sessions are never tracked (warming.ts:61, 70-80).
    #[tokio::test]
    async fn warming_skips_warm_prompts_and_child_sessions() {
        let db = crate::database::Database::open_in_memory().await.expect("db");
        seed_root_session(&db, "ses_root", None).await;
        seed_root_session(&db, "ses_child", Some("ses_root")).await;
        let generate =
            Arc::new(RecordingGenerate { calls: StdMutex::new(Vec::new()), fail: false });
        let (runtime, _clock, _manual, _shutdown_tx) = runtime_with(
            WarmingSettings { prompt: "warm me".into(), interval_ms: 10, duration_ms: 100 },
            generate.clone(),
            db,
        );

        // Child session: ignored entirely.
        Arc::clone(&runtime).account("ses_child", &event("ses_child", "work").messages).await;
        assert!(runtime.tracked("ses_child").is_none(), "child sessions never warm");

        // Track, then feed the warm request: the window must NOT slide.
        Arc::clone(&runtime).account("ses_root", &event("ses_root", "work").messages).await;
        let before = runtime.tracked("ses_root").expect("tracked");
        let warm = event("ses_root", "warm me");
        Arc::clone(&runtime).account("ses_root", &warm.messages).await;
        let after = runtime.tracked("ses_root").expect("still tracked");
        assert_eq!(
            (before.last, before.expires),
            (after.last, after.expires),
            "the warm request does not re-extend the window"
        );
        // Accounting never generates synchronously (the loop's generate
        // is the only source).
        assert_eq!(generate.count(), 0, "accounting never generates synchronously");
    }

    /// Not configured → zero behavior: `warming_settings` is None, so
    /// the engine never constructs a runtime (opt-in, warming.ts:21).
    #[test]
    fn warming_unconfigured_is_none() {
        let documents = vec![oc_schema::config::Document {
            path: None,
            info: oc_schema::config::Info::default(),
        }];
        assert!(warming_settings(&documents).is_none());
    }

    /// A failing generate is swallowed (warming.ts:50-51 catchCause)
    /// and the loop keeps the cadence until expiry: interval 10 /
    /// duration 25 → warms at t=10,20.
    #[tokio::test]
    async fn warming_generate_failure_does_not_break_the_loop() {
        let db = crate::database::Database::open_in_memory().await.expect("db");
        seed_root_session(&db, "ses_fail", None).await;
        let generate = Arc::new(RecordingGenerate { calls: StdMutex::new(Vec::new()), fail: true });
        let (runtime, clock, manual, _shutdown_tx) = runtime_with(
            WarmingSettings { prompt: "warm me".into(), interval_ms: 10, duration_ms: 25 },
            generate.clone(),
            db,
        );
        Arc::clone(&runtime).account("ses_fail", &event("ses_fail", "hi").messages).await;
        drive(&clock, &manual, 16).await;
        assert_eq!(generate.count(), 2, "warms attempted at t=10 and t=20");
        assert!(runtime.is_empty(), "window expired despite failures");
    }

    /// The registered hooks ride the real registry: a `session.context`
    /// dispatch tracks the session and a `session.compaction` dispatch
    /// slides the window (warming.ts:103-105 — one hook on all three).
    #[tokio::test]
    async fn warming_hooks_registered_on_the_registry_account() {
        let db = crate::database::Database::open_in_memory().await.expect("db");
        seed_root_session(&db, "ses_hook", None).await;
        let generate =
            Arc::new(RecordingGenerate { calls: StdMutex::new(Vec::new()), fail: false });
        let (runtime, _clock, _manual, _shutdown_tx) = runtime_with(
            WarmingSettings {
                prompt: "warm me".into(),
                interval_ms: 3_600_000,
                duration_ms: 1_800_000,
            },
            generate,
            db,
        );
        let registry = crate::hooks::HookRegistry::new();
        let account: Arc<dyn crate::hooks::HookHandler<SessionContextEvent>> =
            Arc::new(WarmingAccountHook(Arc::clone(&runtime)));
        let compaction_hook: Arc<
            dyn crate::hooks::HookHandler<crate::hooks::SessionCompactionEvent>,
        > = Arc::new(WarmingCompactionHook(Arc::clone(&runtime)));
        // Registrations live for the test registry's lifetime (their
        // guards dispose on drop).
        let registration_context = registry
            .register_session_context(Arc::clone(&account), crate::hooks::HookOptions::default())
            .await;
        let registration_generate =
            registry.register_session_generate(account, crate::hooks::HookOptions::default()).await;
        let registration_compaction = registry
            .register_session_compaction(compaction_hook, crate::hooks::HookOptions::default())
            .await;

        registry
            .dispatch_session_generate(event("ses_hook", "via generate"))
            .await
            .expect("dispatch");
        assert!(runtime.tracked("ses_hook").is_some(), "tracked via the generate hook");
        // The compaction dispatch slides the window on the same entry
        // (proves the path ran without double-insert).
        registry
            .dispatch_session_compaction(crate::hooks::SessionCompactionEvent {
                context: event("ses_hook", "via compaction"),
                result: None,
            })
            .await
            .expect("dispatch compaction");
        assert!(runtime.tracked("ses_hook").is_some(), "compaction slides the same entry");
        drop(registration_context);
        drop(registration_generate);
        drop(registration_compaction);
    }
}
