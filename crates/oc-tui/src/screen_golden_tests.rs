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
    paint_session_screen, session_composer_frame, ComposerSlot, SessionScreen, FIXTURE_LOCATION,
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

/// `PARITY_UPDATE=1` (re)writes the goldens instead of comparing —
/// the sanctioned refresh path for intentionally changed/new scenarios
/// (same contract as `parity::golden::update_mode`).
fn update_mode() -> bool {
    std::env::var("PARITY_UPDATE").is_ok_and(|value| value == "1")
}

fn theme() -> ThemeState {
    ThemeState::v2_dark()
}

fn assert_text_grid_match(id: &str, width: usize, frame: &Frame) {
    let path = goldens().join(format!("{id}.w{width}.txt"));
    if update_mode() || !path.is_file() {
        std::fs::write(&path, frame.text_grid().join("\n") + "\n").expect("write golden txt");
        return;
    }
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
    let path = goldens().join(format!("{id}.w{width}.ansi"));
    if update_mode() || !path.is_file() {
        std::fs::write(&path, serialize_canonical(frame)).expect("write golden ansi");
        return;
    }
    let golden = std::fs::read_to_string(&path).expect("golden ansi");
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

/// The real composer chrome ([`ComposerSlot::Sibling`]): the wave-2
/// composer component mounted at the session route's shape — the
/// goldens' bottom 7 rows flow through the production renderer, not
/// the placeholder stub. `transcript_width` is the composer column's
/// width (the sidebar already subtracted).
fn sibling_composer(transcript_width: u16, theme: &ThemeState) -> Frame {
    let inset = if usize::from(transcript_width) < 44 { 1 } else { 2 };
    session_composer_frame(
        &crate::components::composer::PromptDraft::default(),
        transcript_width - inset * 2,
        theme,
        FIXTURE_LOCATION,
    )
}

fn vscroll_frame(follow: FollowState, appended: bool) -> Frame {
    let theme = theme();
    let mut messages = vscroll_messages_fixture();
    if appended {
        messages.push(vscroll_appended_turn());
    }
    let screen = vscroll_screen(follow);
    let composer = sibling_composer(80, &theme);
    paint_session_screen(
        &messages,
        &TranscriptOptions::default(),
        &screen,
        &theme,
        80,
        24,
        ComposerSlot::Sibling(&composer),
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
    let composer = sibling_composer(118, &theme);
    let frame = paint_session_screen(
        &crate::components::message::list::message_groups_fixture(),
        &TranscriptOptions::default(),
        &screen,
        &theme,
        160,
        30,
        ComposerSlot::Sibling(&composer),
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
    let composer = sibling_composer(160, &theme);
    let frame = paint_session_screen(
        &crate::components::message::list::message_groups_fixture(),
        &TranscriptOptions::default(),
        &screen,
        &theme,
        160,
        30,
        ComposerSlot::Sibling(&composer),
    );
    assert_text_grid_match("sidebar-collapsed", 160, &frame);
    assert_ansi_byte_match("sidebar-collapsed", 160, &frame);
}

/// Wave 3 — the sidebar's plugin section renders IN the session
/// screen's golden shape (the same 160-wide assembly as
/// `sidebar-expanded`, plus one plugin section under the title).
/// Pinned LOCALLY: plugins are not in the reference corpus, so the
/// expected grid is asserted inline (text-grid compare against a
/// locally-pinned expected, not the reference goldens).
#[test]
fn sidebar_plugin_section_renders_in_the_session_screen_shape() {
    let theme = theme();
    let mut sidebar_view =
        crate::components::sidebar::SidebarView::fixture("Sidebar fixture", FIXTURE_LOCATION);
    sidebar_view.sections.plugins = vec![crate::components::sidebar::SidebarSection {
        plugin_id: "tuislots".into(),
        title: "Fixture".into(),
        rows: vec!["preset: zhipuai".into(), "sessions: 7".into()],
        clickable: Vec::new(),
    }];
    let screen = SessionScreen {
        tabs: None,
        sidebar: SidebarMode::Auto,
        sidebar_view,
        follow: FollowState::Sticky,
        location: FIXTURE_LOCATION,
    };
    let composer = sibling_composer(118, &theme);
    let frame = paint_session_screen(
        &crate::components::message::list::message_groups_fixture(),
        &TranscriptOptions::default(),
        &screen,
        &theme,
        160,
        30,
        ComposerSlot::Sibling(&composer),
    );
    let grid = frame.text_grid();
    // The sidebar pane occupies columns 118..160; the section lands
    // under the title (row 2 + rows 3-4), 2 columns into the pane.
    let pane: Vec<String> =
        grid.iter().map(|row| row.chars().skip(118 + 2).take(30).collect()).collect();
    assert!(pane[1].starts_with("Sidebar fixture"), "the title: {:?}", pane[1]);
    assert!(pane[2].starts_with("Fixture"), "the section title: {:?}", pane[2]);
    assert!(pane[3].starts_with("preset: zhipuai"), "row 1: {:?}", pane[3]);
    assert!(pane[4].starts_with("sessions: 7"), "row 2: {:?}", pane[4]);
    assert!(pane[5].trim().is_empty(), "the gap row: {:?}", pane[5]);
    // The reference corpus shape is UNCHANGED with no sections (the
    // empty-seam golden above still passes byte-exact) — and the
    // transcript column keeps its bytes here too (the section only
    // paints inside the pane).
    let left: String = grid[10].chars().take(118).collect();
    assert_eq!(
        left,
        {
            let bare_screen = SessionScreen {
                tabs: None,
                sidebar: SidebarMode::Auto,
                sidebar_view: crate::components::sidebar::SidebarView::fixture(
                    "Sidebar fixture",
                    FIXTURE_LOCATION,
                ),
                follow: FollowState::Sticky,
                location: FIXTURE_LOCATION,
            };
            let bare = paint_session_screen(
                &crate::components::message::list::message_groups_fixture(),
                &TranscriptOptions::default(),
                &bare_screen,
                &theme,
                160,
                30,
                ComposerSlot::Sibling(&composer),
            );
            bare.text_grid()[10].chars().take(118).collect::<String>()
        },
        "the transcript column is untouched by the sidebar section"
    );
}

#[test]
fn session_screen_40_matches_text_grid_and_ansi() {
    let theme = theme();
    let screen = SessionScreen::narrow(FIXTURE_LOCATION);
    let composer = sibling_composer(40, &theme);
    let frame = paint_session_screen(
        &crate::components::message::list::message_groups_fixture(),
        &TranscriptOptions::default(),
        &screen,
        &theme,
        40,
        30,
        ComposerSlot::Sibling(&composer),
    );
    assert_text_grid_match("session-screen-40", 40, &frame);
    assert_ansi_byte_match("session-screen-40", 40, &frame);
}

// -- wave-6 live-route scenarios: send-error part + slash palette -------

use crate::components::message::model::{Message, PartTime};

/// `session-error-part` (w80): a turn whose assistant step FAILED —
/// the `Error: …` row renders in `text.feedback.error.base` above the
/// muted-label footer (`AssistantFooter`, ref session/index.tsx:1879).
/// The composer's blinking block cursor is pinned in its visible
/// phase (store clock 0 — the run loop's ticks toggle it).
#[test]
fn session_error_part_matches_text_grid_and_ansi() {
    let store = Store::new(Runtime::new());
    store.set_theme(theme());
    store.open_tab("ses_err", Some("Error session".to_owned()));
    store.set_messages(
        "ses_err",
        vec![
            Message::User {
                id: "u1".into(),
                text: "please answer".into(),
                created: 0,
                files: Vec::new(),
            },
            Message::Assistant {
                id: "a1".into(),
                agent: "build".into(),
                model: "fixture/fixture".into(),
                parts: Vec::new(),
                finish: Some("error".into()),
                error: Some("unexpected status 500".into()),
                time: PartTime { created: Some(1), completed: Some(2) },
                tokens: None,
                usage_total: None,
            },
        ],
    );
    let app = crate::app::App::with_store(store, (80, 24));
    let frame = app.frame();
    let grid = frame.text_grid().join("\n");
    assert!(grid.contains("Error: unexpected status 500"), "the error row: {grid}");
    assert_text_grid_match("session-error-part", 80, &frame);
    assert_ansi_byte_match("session-error-part", 80, &frame);
}

/// `session-slash-palette` (w80): typing `/` opens the command palette
/// OVER the composer with each command's DESCRIPTION in the preview
/// column (`component/prompt/autocomplete.tsx` — the wave-6 live
/// mount), the selected row highlighted, the rail in the resolved
/// agent color (blue — categorical[0]) and the block cursor after the
/// typed slash.
#[test]
fn session_slash_palette_matches_text_grid_and_ansi() {
    let store = Store::new(Runtime::new());
    store.set_theme(theme());
    store.open_tab("ses_pal", Some("Palette session".to_owned()));
    // Agents loaded → the rail resolves to the build agent's blue.
    store.set_agents(crate::store::ListState::Loaded(vec![crate::store::ServerAgent {
        id: "build".into(),
        name: "Build".into(),
        description: Some(String::new()),
        mode: "primary".into(),
        hidden: false,
    }]));
    let mut app = crate::app::App::with_store(store, (80, 24));
    app.press("/");
    let frame = app.frame();
    let grid = frame.text_grid().join("\n");
    assert!(grid.contains("/agents"), "palette rows list commands: {grid}");
    assert!(grid.contains("Switch agent"), "the preview/description column: {grid}");
    assert!(grid.contains("/exit"), "the full v2 slash table");
    assert_text_grid_match("session-slash-palette", 80, &frame);
    assert_ansi_byte_match("session-slash-palette", 80, &frame);
}

/// `session-sidebar-live` (w160 + w80): the LIVE route's sidebar
/// mount (`render_session` through the store — the session-frame
/// port). At 160 `session.sidebar = auto` docks the pane at the right
/// edge (42 cols, `ui/layout.ts:1`): the bold session title, the
/// getting-started card (height ≥ 22), and the transcript + composer
/// reflow to 118. At 80 the pane is absent (auto shows only >120 —
/// `session-frame.tsx:106-111`) and the route renders the
/// pre-sidebar full-width shape.
#[test]
fn session_sidebar_live_matches_text_grid_and_ansi() {
    for (width, height) in [(160u16, 30u16), (80, 24)] {
        let store = Store::new(Runtime::new());
        store.set_theme(theme());
        store.open_tab("ses_side", Some("Sidebar session".to_owned()));
        store.set_messages(
            "ses_side",
            vec![
                Message::User {
                    id: "u1".into(),
                    text: "what does the parser do?".into(),
                    created: 0,
                    files: Vec::new(),
                },
                Message::Assistant {
                    id: "a1".into(),
                    agent: "build".into(),
                    model: "fixture/fixture".into(),
                    parts: Vec::new(),
                    finish: Some("stop".into()),
                    error: None,
                    time: PartTime { created: Some(1), completed: Some(2) },
                    tokens: None,
                    usage_total: None,
                },
            ],
        );
        let app = crate::app::App::with_store(store, (width, height));
        let frame = app.frame();
        let grid = frame.text_grid().join("\n");
        if usize::from(width) > crate::components::sidebar::SIDEBAR_AUTO_BREAKPOINT {
            assert!(grid.contains("Sidebar session"), "the pane's title at {width}");
            assert!(grid.contains("Getting started"), "the onboarding card at {width}");
        } else {
            assert!(!grid.contains("Getting started"), "no pane at {width}");
        }
        assert_text_grid_match("session-sidebar-live", usize::from(width), &frame);
        assert_ansi_byte_match("session-sidebar-live", usize::from(width), &frame);
    }
}

/// SURF-1 — the transcript detail states as one golden assembly: the
/// shell tool's detail cell (rail, `$ cmd`, collapsed preview), the
/// skill row, the queued and completed compaction dividers (with usage
/// and summary markdown) and the staged-revert banner with its file
/// rows. Pinned LOCALLY like the plugin-section golden (the detail
/// states are fixture-shaped, not in the reference corpus).
#[test]
fn surf1_detail_states_match_the_golden_assembly() {
    use crate::components::message::model::{Part, PartTime, ToolState, ToolStatus};
    let theme = theme();
    let mut messages = vscroll_messages_fixture();
    messages.truncate(2);
    messages.push(crate::components::message::Message::Assistant {
        id: "surf-a".into(),
        agent: "build".into(),
        model: "fixture/fixture".into(),
        parts: vec![Part::Tool {
            id: "surf-sh".into(),
            name: "bash".into(),
            time: PartTime::default(),
            state: ToolState {
                status: ToolStatus::Completed,
                input: [("command".to_owned(), "echo hi".to_owned())].into_iter().collect(),
                metadata: std::collections::BTreeMap::new(),
                metadata_json: None,
                output: Some("hi\nsecond line".to_owned()),
                error: None,
            },
        }],
        finish: None,
        error: None,
        time: PartTime::default(),
        tokens: None,
        usage_total: None,
    });
    messages.push(crate::components::message::Message::Compaction {
        id: "surf-comp".into(),
        created: 3,
        running: false,
        failed: false,
        cancelled: false,
        error: None,
        summary: Some("Condensed the early turns.".into()),
        tokens_in: Some(1_200),
        tokens_out: Some(200),
        provider_compaction: false,
    });
    messages.push(crate::components::message::Message::Skill {
        id: "surf-skill".into(),
        name: "commitlint".into(),
        created: 4,
    });
    let options = TranscriptOptions {
        pending_compactions: vec!["surf-inbox".into()],
        revert: Some(crate::store::RevertState {
            message_id: "vs-user-1".into(),
            files: vec![crate::store::RevertFile {
                file: "src/a.py".into(),
                status: "modified".into(),
                additions: 3,
                deletions: 1,
            }],
        }),
        ..TranscriptOptions::default()
    };
    let screen = SessionScreen {
        tabs: None,
        sidebar: SidebarMode::Hide,
        sidebar_view: crate::components::sidebar::SidebarView::fixture(
            "Surf fixture",
            FIXTURE_LOCATION,
        ),
        follow: FollowState::Sticky,
        location: FIXTURE_LOCATION,
    };
    let composer = sibling_composer(80, &theme);
    let frame = paint_session_screen(
        &messages,
        &options,
        &screen,
        &theme,
        80,
        30,
        ComposerSlot::Sibling(&composer),
    );
    assert_text_grid_match("surf1-detail-states", 80, &frame);
    assert_ansi_byte_match("surf1-detail-states", 80, &frame);
}

/// SURF-1 — the sidebar's LIVE sections in the golden assembly: the
/// context meter (tokens + % + cost), the MCP rows with status dots
/// (collapsed past two) and the location footer.
#[test]
fn surf1_sidebar_live_sections_match_the_golden_assembly() {
    let theme = theme();
    let sidebar_view = crate::components::sidebar::SidebarView {
        title: "Surf fixture".into(),
        title_pending_since: None,
        clock_ms: 0,
        sections: crate::components::sidebar::SidebarSections {
            context: crate::components::sidebar::SidebarContextWidget {
                tokens: Some(60_400),
                percent: Some(12),
                cost: Some("$0.42".to_owned()),
            },
            mcp: crate::components::sidebar::SidebarMcpWidget {
                servers: vec![
                    crate::store::McpServerRow {
                        name: "github".into(),
                        status: "connected",
                        error: None,
                    },
                    crate::store::McpServerRow {
                        name: "filesystem".into(),
                        status: "connected",
                        error: None,
                    },
                    crate::store::McpServerRow {
                        name: "broken".into(),
                        status: "failed",
                        error: Some("ECONNREFUSED".into()),
                    },
                ],
                open: false,
            },
            plugins: Vec::new(),
        },
        onboarding: false,
        location: Some("/tmp/opencode/surf:main".to_owned()),
    };
    let screen = SessionScreen {
        tabs: None,
        sidebar: SidebarMode::Auto,
        sidebar_view,
        follow: FollowState::Sticky,
        location: FIXTURE_LOCATION,
    };
    let composer = sibling_composer(118, &theme);
    let frame = paint_session_screen(
        &crate::components::message::list::message_groups_fixture(),
        &TranscriptOptions::default(),
        &screen,
        &theme,
        160,
        30,
        ComposerSlot::Sibling(&composer),
    );
    assert_text_grid_match("surf1-sidebar-live", 160, &frame);
    assert_ansi_byte_match("surf1-sidebar-live", 160, &frame);
}
