//! 7.2 wave-2 screen goldens: the virtual-scroll offsets, the sidebar
//! pair and the complete session route — all through the full
//! assembly ([`crate::components::session_screen::paint_session_screen`]),
//! compared text-grid AND canonical-ANSI byte-exact at the manifest's
//! `truecolor-canonical` depth.
//!
//! * **`vscroll-{top,middle,bottom,stick-bottom}`** — one transcript ≫
//!   viewport (10 turns × 8 rows = 80 rows in a 16-row viewport,
//!   maximum scrollTop 64) pinned at four offsets; the stick-bottom
//!   variant streams the appended turn through the STORE's SSE
//!   reducers (`session.step.started` → `text.ended` → `step.ended`)
//!   and proves the follow machine keeps the tail visible.
//! * **`sidebar-expanded` / `sidebar-collapsed`** — the session route
//!   at 160 with the right-hand pane (`sidebar=auto` > 120) and
//!   hidden (`sidebar=hide`): the pane carries the bold title, the
//!   getting-started card and the location footer.
//! * **`session-screen-40`** — the complete narrow route: tabs bar +
//!   transcript (scrolled by 2 under the smaller viewport) + composer
//!   chrome.

#![cfg(test)]
#![allow(clippy::cast_possible_truncation)]

use parity::tui_frame as pf;

use crate::components::message::list::{
    render_transcript, vscroll_appended_turn, vscroll_messages_fixture, FollowState,
    TranscriptOptions,
};
use crate::components::session_screen::{
    paint_session_screen, ComposerSlot, SessionScreen, FIXTURE_LOCATION,
};
use crate::components::sidebar::SidebarMode;
use crate::data::{BusEvent, Ingest};
use crate::reactive::Runtime;
use crate::render::{serialize_canonical, Frame};
use crate::store::Store;
use crate::theme::ThemeState;

fn goldens() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/parity/goldens/tui-frames")
}

fn theme() -> ThemeState {
    ThemeState::v2_dark()
}

fn assert_text_grid_match(id: &str, width: usize, frame: &Frame) {
    let golden = pf::load_text_grid(&goldens(), id, width).expect("golden text grid");
    let rendered =
        pf::TextGrid { rows: frame.text_grid(), height: frame.height, width: frame.width };
    let diff = pf::compare_text_grids(&golden, &rendered);
    assert!(
        diff.first_divergence.is_none(),
        "{id}@{width} text grid diverged: {:?} ({:.2}% matching)\nfirst divergence row:\n  golden: {:?}\n  actual: {:?}",
        diff.first_divergence,
        diff.match_pct(),
        golden
            .rows
            .get(diff.first_divergence.as_ref().map_or(0, |cell| cell.row))
            .map(String::as_str),
        rendered
            .rows
            .get(diff.first_divergence.as_ref().map_or(0, |cell| cell.row))
            .map(String::as_str),
    );
}

fn assert_ansi_byte_match(id: &str, width: usize, frame: &Frame) {
    let golden = std::fs::read_to_string(goldens().join(format!("{id}.w{width}.ansi")))
        .expect("golden ansi");
    let diff = pf::compare_ansi_bytes_at(
        &golden,
        &serialize_canonical(frame),
        pf::ComparisonDepth::TruecolorCanonical,
    )
    .expect("ansi parses");
    assert!(
        diff.equal,
        "{id}@{width} canonical ansi differs: first divergence {:?}",
        diff.first_divergent_offset
    );
}

/// The vscroll session screen (tabs off, no sidebar at 80).
fn vscroll_screen(follow: FollowState) -> SessionScreen<'static> {
    SessionScreen {
        tabs: None,
        sidebar: SidebarMode::Hide,
        sidebar_view: crate::components::sidebar::SidebarView::fixture(
            "Virtual scroll",
            FIXTURE_LOCATION,
        ),
        follow,
        location: FIXTURE_LOCATION,
    }
}

/// The scroll offsets the manifest records: the reference pins
/// `scrollTo(target)` with `maximum = scrollHeight − viewport.height`
/// — top 0, middle `floor(max/2)`, bottom `maximum`.
#[test]
fn vscroll_scroll_offsets_follow_the_reference_pinning() {
    let theme = theme();
    let transcript = render_transcript(
        &vscroll_messages_fixture(),
        &TranscriptOptions::default(),
        &theme.styles(),
        80,
    );
    assert_eq!(transcript.content_height, 80, "10 turns × 8 rows");
    let viewport = 16; // 24-row frame − 7 chrome rows − jump slot
    let maximum = transcript.content_height - viewport;
    assert_eq!(maximum, 64);
    assert_eq!(
        FollowState::Sticky.scrolled_to(0, maximum),
        FollowState::Free { offset: 0, maximum: 64 },
        "top pins Free at 0 — indicator armed"
    );
    assert_eq!(
        FollowState::Free { offset: 0, maximum }
            .scrolled_to(maximum / 2, maximum)
            .offset_value_for_test(),
        32
    );
    assert_eq!(
        FollowState::Free { offset: 32, maximum }.scrolled_to(maximum, maximum),
        FollowState::Sticky,
        "bottom re-arms sticky"
    );
}

// `FollowState` keeps its offset private; a tiny test hook.
trait OffsetForTest {
    fn offset_value_for_test(&self) -> usize;
}
impl OffsetForTest for FollowState {
    fn offset_value_for_test(&self) -> usize {
        match self {
            FollowState::Sticky => usize::MAX,
            FollowState::Free { offset, .. } => *offset,
        }
    }
}

fn vscroll_frame(follow: FollowState, appended: bool) -> Frame {
    let theme = theme();
    let mut messages = vscroll_messages_fixture();
    if appended {
        messages.push(vscroll_appended_turn());
    }
    let screen = vscroll_screen(follow);
    paint_session_screen(
        &messages,
        &TranscriptOptions::default(),
        &screen,
        &theme,
        80,
        24,
        ComposerSlot::Placeholder,
    )
}

#[test]
fn vscroll_top_matches_text_grid_and_ansi() {
    let frame = vscroll_frame(FollowState::Free { offset: 0, maximum: 64 }, false);
    assert_text_grid_match("vscroll-top", 80, &frame);
    assert_ansi_byte_match("vscroll-top", 80, &frame);
}

#[test]
fn vscroll_middle_matches_text_grid_and_ansi() {
    let frame = vscroll_frame(FollowState::Free { offset: 32, maximum: 64 }, false);
    assert_text_grid_match("vscroll-middle", 80, &frame);
    assert_ansi_byte_match("vscroll-middle", 80, &frame);
}

#[test]
fn vscroll_bottom_matches_text_grid_and_ansi() {
    // Scrolled back to the maximum after being away: sticky re-armed,
    // indicator cleared.
    let frame =
        vscroll_frame(FollowState::Free { offset: 0, maximum: 64 }.scrolled_to(64, 64), false);
    assert_text_grid_match("vscroll-bottom", 80, &frame);
    assert_ansi_byte_match("vscroll-bottom", 80, &frame);
}

/// The stick-bottom golden drives the append through the STORE's SSE
/// reducers — the streamed turn lands exactly like the fixture
/// message, and sticky keeps the tail (offset 68 = 84 − 16) visible.
#[test]
fn vscroll_stick_bottom_matches_text_grid_and_ansi() {
    let theme = theme();
    let store = Store::new(Runtime::new());
    store.set_messages("ses_vscroll", vscroll_messages_fixture());
    let mut started = BusEvent::new(
        "session.step.started",
        serde_json::json!({
            "sessionID": "ses_vscroll",
            "assistantMessageID": "vs-answer-appended",
            "agent": "build",
            "model": { "providerID": "fixture", "id": "fixture" },
        }),
    );
    started.created = Some(101);
    let mut text_ended = BusEvent::new(
        "session.text.ended",
        serde_json::json!({
            "sessionID": "ses_vscroll",
            "assistantMessageID": "vs-answer-appended",
            "ordinal": 0,
            "text": "Appended while pinned to the bottom.",
        }),
    );
    text_ended.created = Some(102);
    let mut step_ended = BusEvent::new(
        "session.step.ended",
        serde_json::json!({
            "sessionID": "ses_vscroll",
            "assistantMessageID": "vs-answer-appended",
            "finish": "stop",
        }),
    );
    step_ended.created = Some(103);
    store.ingest_all([
        Ingest::Event(started),
        Ingest::Event(text_ended),
        Ingest::Event(step_ended),
    ]);
    let messages = store.messages("ses_vscroll");
    // The streamed turn equals the static fixture (same reducer path
    // the golden's event stream takes).
    let mut expected = vscroll_messages_fixture();
    expected.push(vscroll_appended_turn());
    assert_eq!(messages, expected, "the store stream reproduces the fixture turn");
    let transcript =
        render_transcript(&messages, &TranscriptOptions::default(), &theme.styles(), 80);
    assert_eq!(transcript.content_height, 84, "80 + the appended 4 rows");
    let frame = vscroll_frame(FollowState::Sticky.appended(84), true);
    assert_text_grid_match("vscroll-stick-bottom", 80, &frame);
    assert_ansi_byte_match("vscroll-stick-bottom", 80, &frame);
}

#[test]
fn sidebar_expanded_matches_text_grid_and_ansi() {
    let theme = theme();
    let screen = SessionScreen {
        tabs: None,
        sidebar: SidebarMode::Auto,
        sidebar_view: crate::components::sidebar::SidebarView::fixture(
            "Sidebar fixture",
            FIXTURE_LOCATION,
        ),
        follow: FollowState::Sticky,
        location: FIXTURE_LOCATION,
    };
    let frame = paint_session_screen(
        &crate::components::message::list::message_groups_fixture(),
        &TranscriptOptions::default(),
        &screen,
        &theme,
        160,
        30,
        ComposerSlot::Placeholder,
    );
    assert_text_grid_match("sidebar-expanded", 160, &frame);
    assert_ansi_byte_match("sidebar-expanded", 160, &frame);
}

#[test]
fn sidebar_collapsed_matches_text_grid_and_ansi() {
    let theme = theme();
    let screen = SessionScreen {
        tabs: None,
        sidebar: SidebarMode::Hide,
        sidebar_view: crate::components::sidebar::SidebarView::fixture(
            "Sidebar fixture",
            FIXTURE_LOCATION,
        ),
        follow: FollowState::Sticky,
        location: FIXTURE_LOCATION,
    };
    let frame = paint_session_screen(
        &crate::components::message::list::message_groups_fixture(),
        &TranscriptOptions::default(),
        &screen,
        &theme,
        160,
        30,
        ComposerSlot::Placeholder,
    );
    assert_text_grid_match("sidebar-collapsed", 160, &frame);
    assert_ansi_byte_match("sidebar-collapsed", 160, &frame);
}

#[test]
fn session_screen_40_matches_text_grid_and_ansi() {
    let theme = theme();
    let screen = SessionScreen::narrow(FIXTURE_LOCATION);
    let frame = paint_session_screen(
        &crate::components::message::list::message_groups_fixture(),
        &TranscriptOptions::default(),
        &screen,
        &theme,
        40,
        30,
        ComposerSlot::Placeholder,
    );
    assert_text_grid_match("session-screen-40", 40, &frame);
    assert_ansi_byte_match("session-screen-40", 40, &frame);
}
