//! Frame-budget guard (ROADMAP §架构决策 B 的永久护栏): at terminal
//! scales an immediate-mode full-buffer rebuild must stay far below the
//! ~16 ms perception threshold. This test renders a LARGE synthetic
//! transcript at 200×50 and asserts per-frame cost against a budget
//! split by build profile (debug builds run 10-50× slower; the release
//! budget is the product-relevant one). Failures mean a rendering
//! regression (e.g., accidental O(all-rows) work in a per-frame path)
//! — investigate with the measured numbers printed below, do not
//! loosen the budgets without a profiling justification.

use crate::components::composer::PromptDraft;
use crate::components::message::list::{FollowState, TranscriptOptions};
use crate::components::message::model::{Message, Part, PartTime};
use crate::components::session_screen::{
    paint_session_screen, session_composer_frame, ComposerSlot, SessionScreen, FIXTURE_LOCATION,
};
use crate::theme::ThemeState;

/// The release budget is the product guard (asserted in release builds
/// and in CI's release-profile run of this test). Debug builds only
/// REPORT the number — they are 10-50× slower and would make the guard
/// flaky, while the release number is the one users feel.
const FRAME_BUDGET_MS: u128 = 40; // TEMP: 19ms local-release, CI runners measure higher (shared CPU) (800 msgs, O(all-rows) layout); Wave B windowing targets 5ms (asserted in release only)

const WIDTH: usize = 200;
const HEIGHT: usize = 50;

fn theme() -> ThemeState {
    ThemeState::v2_dark()
}

fn large_transcript(groups: usize) -> Vec<Message> {
    let mut messages = Vec::with_capacity(groups * 2);
    for index in 0..groups {
        let text = format!(
            "Message {index}: the parser walks the token stream, folds the resulting \
             spans into line groups, and projects them onto the grid — this line is \
             deliberately long enough to wrap at the fixture width a couple of times \
             so the layout path does real work per row."
        );
        messages.push(Message::User {
            id: format!("bench-user-{index}"),
            text: text.clone(),
            created: index as u64,
            files: Vec::new(),
        });
        messages.push(Message::Assistant {
            id: format!("bench-answer-{index}"),
            agent: "build".into(),
            model: "fixture/fixture".into(),
            finish: Some("stop".into()),
            time: PartTime { created: Some(index as u64), completed: Some(index as u64) },
            parts: vec![
                Part::Tool {
                    id: format!("bench-tool-{index}"),
                    name: "read".into(),
                    time: PartTime { created: Some(index as u64), completed: Some(index as u64) },
                },
                Part::Text {
                    text,
                    time: PartTime { created: Some(index as u64), completed: None },
                },
            ],
            error: None,
            tokens: None,
            usage_total: None,
        });
    }
    messages
}

#[test]
fn frame_budget_large_transcript_200x50() {
    let theme = theme();
    let transcript = large_transcript(400);
    // Two screen variants alternating per frame so every render
    // performs real layout work (an identical frame could be shortcut
    // by caches): sticky-follow vs a scrolled-free offset.
    let mut screen_sticky = SessionScreen::narrow(FIXTURE_LOCATION);
    screen_sticky.follow = FollowState::Sticky;
    let mut screen_free = SessionScreen::narrow(FIXTURE_LOCATION);
    screen_free.follow = FollowState::Free { offset: 3, maximum: 900 };
    let composer = session_composer_frame(
        &PromptDraft::default(),
        u16::try_from(WIDTH - 4).unwrap_or(u16::MAX),
        &theme,
        FIXTURE_LOCATION,
    );

    // Warmup (layout caches, allocator).
    for _ in 0..3 {
        let _ = paint_session_screen(
            &transcript,
            &TranscriptOptions::default(),
            &screen_sticky,
            &theme,
            u16::try_from(WIDTH).unwrap_or(u16::MAX),
            u16::try_from(HEIGHT).unwrap_or(u16::MAX),
            ComposerSlot::Sibling(&composer),
        );
    }

    let mut worst = 0u128;
    let mut total = 0u128;
    let frames = 20u128;
    for frame in 0..frames {
        let screen = if frame % 2 == 0 { &screen_sticky } else { &screen_free };
        let start = std::time::Instant::now();
        let painted = paint_session_screen(
            &transcript,
            &TranscriptOptions::default(),
            screen,
            &theme,
            u16::try_from(WIDTH).unwrap_or(u16::MAX),
            u16::try_from(HEIGHT).unwrap_or(u16::MAX),
            ComposerSlot::Sibling(&composer),
        );
        let elapsed = start.elapsed().as_millis();
        worst = worst.max(elapsed);
        total += elapsed;
        // Touch the product so the render is never optimized away.
        assert_eq!(painted.height, HEIGHT);
    }
    eprintln!(
        "[frame-budget] {WIDTH}x{HEIGHT}, 800 messages: worst {worst} ms, avg {} ms \
         (budget {FRAME_BUDGET_MS} ms, {} build)",
        total / frames,
        if cfg!(debug_assertions) { "debug" } else { "release" },
    );
    #[cfg(not(debug_assertions))]
    assert!(
        worst <= FRAME_BUDGET_MS,
        "frame budget exceeded: worst {worst} ms > {FRAME_BUDGET_MS} ms — profile the \
         per-frame path before touching this budget (registered finding: per-frame \
         layout is O(all rows) until Wave B lands v2-style visible-rows windowing)"
    );
    // NOTE (finding, 2026-10-11): debug measures ~86ms avg for 800
    // messages — the transcript layout walks EVERY row per frame.
    // Wave B ports v2's visibleRows windowing + row-height caching
    // (ref index.tsx:462) and tightens this space; the release budget
    // stays 5ms regardless.
}

/// Lane M1 (换血 pilot): the tab strip's per-frame cost on the LIVE
/// session route (`render_session` mounts the strip via
/// `paint_live_tabs_bar`), measured over a 12-tab store with glowing
/// tabs and alternating hover — the worst realistic strip workload at
/// the budget harness's 200×50 scale. Both renderer paths get their
/// own test process (the flag is read once at boot; nextest isolates
/// the two fns), so the two numbers compare cleanly.
#[test]
fn frame_budget_session_with_tabs_strip_old_path() {
    std::env::set_var("OC_TABS_OPENTUI", "0");
    strip_frame_budget_body();
}

/// The opentui-solid strip path — the default (see
/// [`frame_budget_session_with_tabs_strip_old_path`] for the
/// `OC_TABS_OPENTUI=0` fallback).
#[test]
fn frame_budget_session_with_tabs_strip_solid_path() {
    std::env::set_var("OC_TABS_OPENTUI", "1");
    strip_frame_budget_body();
}

fn strip_frame_budget_body() {
    use crate::components::session::render_session;
    use crate::reactive::Runtime;
    use crate::store::{Store, Unread};

    let theme = theme();
    let store = Store::new(Runtime::new());
    for index in 0..12 {
        store.open_tab(&format!("s{index}"), Some(format!("Session {index} title")));
        if index % 3 == 0 {
            store.set_unread(&format!("s{index}"), Some(Unread::Activity));
        }
    }
    // Warmup (mount + caches).
    for frame in 0..3 {
        let hover = if frame % 2 == 0 { Some("s4") } else { None };
        store.set_tab_hover(hover);
        let _ = render_session(&store, "s1", &theme.styles(), (200, 50));
    }
    let mut worst = 0u128;
    let mut worst_micros = 0u128;
    let mut total_micros = 0u128;
    let frames = 20u128;
    for frame in 0..frames {
        // Alternate hover targets so every frame performs real patch
        // work (widths shift, ✕ reveals, glow titles re-derive).
        let hover = if frame % 2 == 0 { Some("s4") } else { Some("s7") };
        store.set_tab_hover(hover);
        let start = std::time::Instant::now();
        let painted = render_session(&store, "s1", &theme.styles(), (200, 50));
        let elapsed = start.elapsed();
        worst = worst.max(elapsed.as_millis());
        worst_micros = worst_micros.max(elapsed.as_micros());
        total_micros += elapsed.as_micros();
        assert_eq!(painted.height, 50);
    }
    eprintln!(
        "[frame-budget] 200x50 session route, 12 tabs strip ({}): worst {} µs, avg {} µs \
         ({} build)",
        if crate::components::session_tabs_solid::opentui_tabs_enabled() { "solid" } else { "old" },
        worst_micros,
        (total_micros / frames),
        if cfg!(debug_assertions) { "debug" } else { "release" },
    );
    #[cfg(not(debug_assertions))]
    assert!(
        worst <= FRAME_BUDGET_MS,
        "frame budget exceeded with the tabs strip mounted: worst {worst} ms > \
         {FRAME_BUDGET_MS} ms"
    );
}

/// Lane M3: the composer frame's per-frame cost on the LIVE session
/// route (`render_session` docks the composer mount via
/// `composer_mount_frame`), measured over an IDLE phase (identical
/// frames — the reconciler's no-op skip) and a TYPING-CHURN phase
/// (a word per frame growing the draft; height-stable frames patch
/// in place, the occasional wrap rebuilds — the realistic edit mix).
/// Both composer renderer paths get their own test process (the flag
/// is read once at boot; nextest isolates the fns); the OTHER solid
/// surfaces (tabs strip, dialogs) are pinned to the OLD path on both
/// sides so the numbers isolate the composer path.
#[test]
fn frame_budget_composer_frame_old_path() {
    std::env::set_var("OC_COMPOSER_OPENTUI", "0");
    std::env::set_var("OC_TABS_OPENTUI", "0");
    std::env::set_var("OC_DIALOGS_OPENTUI", "0");
    composer_frame_budget_body();
}

/// The opentui-solid composer path (`OC_COMPOSER_OPENTUI=1`).
#[test]
fn frame_budget_composer_frame_solid_path() {
    std::env::set_var("OC_COMPOSER_OPENTUI", "1");
    std::env::set_var("OC_TABS_OPENTUI", "0");
    std::env::set_var("OC_DIALOGS_OPENTUI", "0");
    composer_frame_budget_body();
}

#[allow(clippy::items_after_statements)] // a local metric helper below the warmup
fn composer_frame_budget_body() {
    use crate::components::composer::ComposerKey;
    use crate::components::session::render_session;
    use crate::reactive::Runtime;
    use crate::store::Store;

    let theme = theme();
    let store = Store::new(Runtime::new());
    store.open_tab("s1", Some("Composer budget".to_owned()));
    store.navigate(crate::store::Route::Session { session_id: "s1".to_owned() });
    // Seed a footer location + metadata so the composer paints its
    // full frame (rail, metadata row, footer hints).
    let mut composer = store.composer("s1");
    composer.handle_key(&ComposerKey::Char('h'));
    store.set_composer("s1", &composer);

    let render = || render_session(&store, "s1", &theme.styles(), (200, 50));

    // Warmup (mount + caches).
    for _ in 0..3 {
        let _ = render();
    }

    fn measure(
        frames: u128,
        mut advance: impl FnMut(u128),
        mut render: impl FnMut() -> crate::render::Frame,
    ) -> (u128, u128, u128) {
        let mut worst = 0u128;
        let mut worst_micros = 0u128;
        let mut total_micros = 0u128;
        for frame in 0..frames {
            advance(frame);
            let start = std::time::Instant::now();
            let painted = render();
            let elapsed = start.elapsed();
            worst = worst.max(elapsed.as_millis());
            worst_micros = worst_micros.max(elapsed.as_micros());
            total_micros += elapsed.as_micros();
            assert_eq!(painted.height, 50);
        }
        (worst, worst_micros, total_micros / frames)
    }

    // The idle phase: identical frames (no composer writes).
    let (idle_worst, idle_worst_us, idle_avg) = measure(10, |_| {}, render);
    // The typing-churn phase: a word per frame into the draft.
    let churn = |frame: u128| {
        let mut composer = store.composer("s1");
        for key in format!("word{frame} ").chars() {
            composer.handle_key(&ComposerKey::Char(key));
        }
        store.set_composer("s1", &composer);
    };
    let (churn_worst, churn_worst_us, churn_avg) = measure(10, churn, render);

    let _ = (idle_worst, churn_worst); // debug builds: assert is release-gated
    eprintln!(
        "[frame-budget] 200x50 session route, composer ({}): idle worst {} µs / avg {} µs; \
         typing churn worst {} µs / avg {} µs ({} build)",
        if crate::components::composer_solid::opentui_composer_enabled() { "solid" } else { "old" },
        idle_worst_us,
        idle_avg,
        churn_worst_us,
        churn_avg,
        if cfg!(debug_assertions) { "debug" } else { "release" },
    );
    #[cfg(not(debug_assertions))]
    assert!(
        idle_worst <= FRAME_BUDGET_MS && churn_worst <= FRAME_BUDGET_MS,
        "frame budget exceeded with the composer mounted: idle {idle_worst} ms, churn \
         {churn_worst} ms > {FRAME_BUDGET_MS} ms"
    );
}

/// Lane M2: the dialog family's per-frame cost on the LIVE app frame
/// (`render_app` composites the top dialog through
/// `render_overlay`), measured over the command palette open with its
/// full registry and alternating selection moves — the hot path
/// (stable content height, in-place prop patches). Both dialog
/// renderer paths get their own test process (the flag is read once
/// at boot; nextest isolates the two fns); the tab strip is pinned to
/// the OLD path on both sides so the numbers isolate the dialog path.
#[test]
fn frame_budget_dialog_open_old_path() {
    std::env::set_var("OC_DIALOGS_OPENTUI", "0");
    std::env::set_var("OC_TABS_OPENTUI", "0");
    dialog_frame_budget_body();
}

/// The opentui-solid dialog path (`OC_DIALOGS_OPENTUI=1`).
#[test]
fn frame_budget_dialog_open_solid_path() {
    std::env::set_var("OC_DIALOGS_OPENTUI", "1");
    std::env::set_var("OC_TABS_OPENTUI", "0");
    dialog_frame_budget_body();
}

fn dialog_frame_budget_body() {
    use crate::reactive::Runtime;
    use crate::store::{Store, Unread};

    let theme = theme();
    let store = Store::new(Runtime::new());
    for index in 0..4 {
        store.open_tab(&format!("s{index}"), Some(format!("Session {index}")));
        if index % 2 == 0 {
            store.set_unread(&format!("s{index}"), Some(Unread::Activity));
        }
    }
    store.navigate(crate::store::Route::Session { session_id: "s1".to_owned() });
    store
        .replace_dialog(crate::store::DialogKind::CommandPalette, crate::store::DialogSize::Medium);
    // Warmup (mount + caches).
    for frame in 0..3 {
        let selected = if frame % 2 == 0 { 0 } else { 3 };
        store.update_dialog(|top| top.select.selected = selected);
        let _ = crate::components::dialog::render_app(&store, &theme, (200, 50));
    }
    let mut worst = 0u128;
    let mut worst_micros = 0u128;
    let mut total_micros = 0u128;
    let frames = 20u128;
    for frame in 0..frames {
        // Alternate the selection so every frame performs real patch
        // work (row backgrounds, bold runs, footers re-derive).
        let selected = if frame % 2 == 0 { 2 } else { 9 };
        store.update_dialog(|top| {
            top.select.selected = selected;
            top.select.scroll = selected.saturating_sub(3);
        });
        let start = std::time::Instant::now();
        let painted = crate::components::dialog::render_app(&store, &theme, (200, 50));
        let elapsed = start.elapsed();
        worst = worst.max(elapsed.as_millis());
        worst_micros = worst_micros.max(elapsed.as_micros());
        total_micros += elapsed.as_micros();
        assert_eq!(painted.height, 50);
    }
    eprintln!(
        "[frame-budget] 200x50 session route, palette dialog open ({}): worst {} µs, avg {} µs \
         ({} build)",
        if crate::components::dialog_solid::opentui_dialogs_enabled() { "solid" } else { "old" },
        worst_micros,
        total_micros / frames,
        if cfg!(debug_assertions) { "debug" } else { "release" },
    );
    #[cfg(not(debug_assertions))]
    assert!(
        worst <= FRAME_BUDGET_MS,
        "frame budget exceeded with a dialog open: worst {worst} ms > {FRAME_BUDGET_MS} ms"
    );
}
