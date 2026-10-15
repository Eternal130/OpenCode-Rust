//! Foundation acceptance: text-grid frame comparisons against the
//! re-pinned golden corpus (`tools/parity/goldens/tui-frames`,
//! `tui-frame-golden/2` under v2-dark), wired through
//! `tools/parity::tui_frame` with the manifest's per-scenario
//! comparison depth (`compare_ansi_frames_at` / `compare_ansi_bytes_at`).
//!
//! Coverage:
//! * `session-tabs-horizontal` / `session-tabs-vertical` — text grid
//!   AND canonical ANSI **byte-exact** at 80/160 since 7.2 wave 2:
//!   the resting unread glow (per-cell pulse backgrounds, per-char
//!   title tints, separator falloff) ports the deterministic math in
//!   `components::tab_pulse` (gate-9 finding 1, the Tier-A
//!   obligation); only time-varying envelopes stay C2.
//! * `tool-wrap-rows` — text grid **and** canonical ANSI byte-exact at
//!   the manifest depth (v2-dark tokens painted through
//!   [`crate::theme`]).
//! * 7.2 wave 0 adds the styled-segment instruments: the tool-wrap
//!   migration proof (styled leaves reproduce the plain-leaf frames
//!   byte-for-byte) and markdown-style cross-boundary span frames at
//!   80/40 with a golden-style ANSI roundtrip.
//!
//! ## TERM independence (S0 lane)
//!
//! These goldens serialize component frames (`App::frame()` /
//! `paint_*` output) through [`crate::render::serialize_canonical`]
//! in TRUECOLOR — they never pass through the terminal-draw seam, so
//! the runner's `TERM`/`COLORTERM` cannot influence them (CI and
//! local runs are byte-identical by construction). The live TUI's
//! 256-color degradation (`render::color_mode`, applied only inside
//! `app::run`'s ratatui draw) is unit-tested separately; the parity
//! rig exercises it end-to-end under its pinned
//! `TERM=xterm-256color`.

#![cfg(test)]
// Frame dimensions come from the manifest (bounded by terminal sizes),
// so the usize→u16/f32 casts cannot truncate real inputs.
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_precision_loss)]

use parity::tui_frame as pf;
use ratatui::style::{Modifier, Style};

use crate::components::session_tabs::{TabStatus, TabView, ADD_TAB_WIDTH};
use crate::components::session_tabs_solid::paint_solid_bar;
use crate::components::tool_rows::render_tool_wrap;
use crate::layout::{BoxSpec, LayoutTree, StyledSpan};
use crate::render::{paint_placed, serialize_canonical, Frame};
use crate::store::{SessionTab, Unread};
use crate::theme::ThemeState;
use taffy::AvailableSpace;

fn goldens() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/parity/goldens/tui-frames")
}

/// The golden scenario fixture (`tui-dump/scenarios/session_tabs.tsx`):
/// two tabs, first unread-activity, second active, detail "project",
/// the add affordance enabled.
fn golden_tab_views() -> Vec<TabView> {
    vec![
        TabView {
            tab: SessionTab { session_id: "first".into(), title: Some("First".into()) },
            status: TabStatus { unread: Some(Unread::Activity), ..TabStatus::EMPTY },
            detail: Some("project".into()),
        },
        TabView {
            tab: SessionTab { session_id: "second".into(), title: Some("Second".into()) },
            status: TabStatus::EMPTY,
            detail: Some("project".into()),
        },
    ]
}

fn manifest() -> pf::Manifest {
    pf::load_manifest(&goldens()).expect("golden manifest")
}

fn scenario(id: &str) -> pf::ManifestScenario {
    manifest()
        .scenarios
        .into_iter()
        .find(|scenario| scenario.id == id)
        .unwrap_or_else(|| panic!("scenario {id} in manifest"))
}

fn assert_text_grid_match(id: &str, width: usize, frame: &AnsiFrameRender) {
    let golden = pf::load_text_grid(&goldens(), id, width).expect("golden text grid");
    let rendered =
        pf::TextGrid { rows: frame.text_grid.clone(), height: frame.height, width: frame.width };
    let diff = pf::compare_text_grids(&golden, &rendered);
    assert!(
        diff.first_divergence.is_none(),
        "{id}@{width} text grid diverged: {:?} ({:.2}% matching)",
        diff.first_divergence,
        diff.match_pct()
    );
}

/// Canonical ANSI bytes at the manifest's comparison depth — since the
/// 7.2 glow upgrade this is exact for the session-tabs scenarios (the
/// resting-glow tint math closes the last style divergence).
fn assert_ansi_match(id: &str, width: usize, frame: &AnsiFrameRender) {
    let scenario = scenario(id);
    let depth = manifest().comparison_depth_for(&scenario);
    let golden_ansi = std::fs::read_to_string(goldens().join(format!("{id}.w{width}.ansi")))
        .expect("golden ansi");
    let diff = pf::compare_ansi_bytes_at(&golden_ansi, &frame.ansi, depth).expect("ansi parses");
    assert!(
        diff.equal,
        "{id}@{width} ansi differs at manifest depth {depth}: first divergence {:?}",
        diff.first_divergent_offset
    );
}

/// The retired `paint_bar` sized a FULL frame (height rows) with the
/// base fill and painted the strip at row 0 — the shape the
/// session-tabs goldens pin. The solid strip paints its own 1-row
/// frame; this helper rebuilds the sized golden shape around it (the
/// goldens now pin the opentui-solid path's bytes).
fn paint_solid_bar_sized(
    tabs: &[TabView],
    active: Option<&str>,
    width: u16,
    height: u16,
    styles: &crate::theme::ThemeStyles,
) -> crate::render::Frame {
    let strip = paint_solid_bar(tabs, active, true, usize::from(width), styles, None, false);
    let base = crate::render::Cell {
        ch: ' ',
        attrs: 0,
        fg: crate::render::CellColor::from(crate::components::tool_rows::DEFAULT_FOREGROUND),
        bg: crate::render::CellColor::from(styles.background),
    };
    let mut cells = vec![vec![base; usize::from(width)]; usize::from(height)];
    for (x, cell) in strip.cells[0].iter().enumerate() {
        cells[0][x] = *cell;
    }
    crate::render::Frame { cells, width: usize::from(width), height: usize::from(height) }
}

/// Adapter so tests can talk about one rendered thing.
pub struct AnsiFrameRender {
    pub text_grid: Vec<String>,
    pub height: usize,
    pub width: usize,
    pub ansi: String,
}

impl AnsiFrameRender {
    fn from_frame(frame: &crate::render::Frame) -> Self {
        let (rows, height, width) = (frame.text_grid(), frame.height, frame.width);
        Self { text_grid: rows, height, width, ansi: crate::render::serialize_canonical(frame) }
    }
}

#[test]
fn session_tabs_horizontal_matches_golden_at_80() {
    let theme = ThemeState::v2_dark();
    let frame = paint_solid_bar_sized(&golden_tab_views(), Some("second"), 80, 10, &theme.styles());
    let render = AnsiFrameRender::from_frame(&frame);
    assert_text_grid_match("session-tabs-horizontal", 80, &render);
    assert_ansi_match("session-tabs-horizontal", 80, &render);
}

#[test]
fn session_tabs_horizontal_matches_golden_at_160() {
    let theme = ThemeState::v2_dark();
    let frame =
        paint_solid_bar_sized(&golden_tab_views(), Some("second"), 160, 10, &theme.styles());
    let render = AnsiFrameRender::from_frame(&frame);
    assert_text_grid_match("session-tabs-horizontal", 160, &render);
    assert_ansi_match("session-tabs-horizontal", 160, &render);
}

#[test]
fn tool_wrap_rows_matches_text_grid_and_ansi_at_80() {
    let scenario = scenario("tool-wrap-rows");
    let variant = scenario.variants.iter().find(|v| v.width == 80).expect("w80 variant");
    let theme = ThemeState::v2_dark();
    let frame = render_tool_wrap(&theme.styles(), 80, variant.height as u16);
    let render = AnsiFrameRender::from_frame(&frame);
    assert_text_grid_match("tool-wrap-rows", 80, &render);
    // ANSI at the manifest's comparison depth (P1 re-pin: v2-dark).
    let depth = manifest().comparison_depth_for(&scenario);
    let golden_ansi =
        std::fs::read_to_string(goldens().join("tool-wrap-rows.w80.ansi")).expect("golden ansi");
    let diff = pf::compare_ansi_bytes_at(&golden_ansi, &render.ansi, depth).expect("ansi parses");
    assert!(
        diff.equal,
        "tool-wrap-rows@80 ansi differs at manifest depth {depth}: first divergence {:?}",
        diff.first_divergent_offset
    );
}

#[test]
fn tool_wrap_rows_matches_text_grid_and_ansi_at_160() {
    let scenario = scenario("tool-wrap-rows");
    let variant = scenario.variants.iter().find(|v| v.width == 160).expect("w160 variant");
    let theme = ThemeState::v2_dark();
    let frame = render_tool_wrap(&theme.styles(), 160, variant.height as u16);
    let render = AnsiFrameRender::from_frame(&frame);
    assert_text_grid_match("tool-wrap-rows", 160, &render);
    let depth = manifest().comparison_depth_for(&scenario);
    let golden_ansi =
        std::fs::read_to_string(goldens().join("tool-wrap-rows.w160.ansi")).expect("golden ansi");
    let diff = pf::compare_ansi_bytes_at(&golden_ansi, &render.ansi, depth).expect("ansi parses");
    assert!(
        diff.equal,
        "tool-wrap-rows@160 ansi differs at manifest depth {depth}: first divergence {:?}",
        diff.first_divergent_offset
    );
}

/// `tool-wrap-error-expanded` (Tier A): the same rows with the failed
/// read's LSP error rendered under it (`errorExpanded`).
#[test]
fn tool_wrap_error_expanded_matches_text_grid_and_ansi_at_80() {
    let scenario = scenario("tool-wrap-error-expanded");
    let variant = scenario.variants.iter().find(|v| v.width == 80).expect("w80 variant");
    let theme = ThemeState::v2_dark();
    let frame = crate::components::tool_rows::render_tool_wrap_error_expanded(
        &theme.styles(),
        80,
        variant.height as u16,
    );
    let render = AnsiFrameRender::from_frame(&frame);
    assert_text_grid_match("tool-wrap-error-expanded", 80, &render);
    let depth = manifest().comparison_depth_for(&scenario);
    let golden_ansi = std::fs::read_to_string(goldens().join("tool-wrap-error-expanded.w80.ansi"))
        .expect("golden ansi");
    let diff = pf::compare_ansi_bytes_at(&golden_ansi, &render.ansi, depth).expect("ansi parses");
    assert!(
        diff.equal,
        "tool-wrap-error-expanded@80 ansi differs at manifest depth {depth}: first divergence {:?}",
        diff.first_divergent_offset
    );
}

#[test]
fn tool_wrap_error_expanded_matches_text_grid_and_ansi_at_160() {
    let scenario = scenario("tool-wrap-error-expanded");
    let variant = scenario.variants.iter().find(|v| v.width == 160).expect("w160 variant");
    let theme = ThemeState::v2_dark();
    let frame = crate::components::tool_rows::render_tool_wrap_error_expanded(
        &theme.styles(),
        160,
        variant.height as u16,
    );
    let render = AnsiFrameRender::from_frame(&frame);
    assert_text_grid_match("tool-wrap-error-expanded", 160, &render);
    let depth = manifest().comparison_depth_for(&scenario);
    let golden_ansi = std::fs::read_to_string(goldens().join("tool-wrap-error-expanded.w160.ansi"))
        .expect("golden ansi");
    let diff = pf::compare_ansi_bytes_at(&golden_ansi, &render.ansi, depth).expect("ansi parses");
    assert!(
        diff.equal,
        "tool-wrap-error-expanded@160 ansi differs at manifest depth {depth}: first divergence {:?}",
        diff.first_divergent_offset
    );
}

/// Fidelity-classified comparison through the full parity pipeline
/// (`compare_ansi_frames_at` at the manifest depth) for the
/// session-tabs scenarios — since the 7.2 glow upgrade the frames are
/// **fully equal** at the manifest depth (glyphs and styles): this
/// instrument now pins that nothing regresses to style-only parity.
#[test]
fn session_tabs_ansi_full_fidelity_at_manifest_depth() {
    // The vertical rail was retired with the old tab renderer (a
    // golden-only legacy shape); the horizontal goldens now pin the
    // opentui-solid path's bytes.
    for id in ["session-tabs-horizontal"] {
        let scenario = scenario(id);
        let depth = manifest().comparison_depth_for(&scenario);
        for variant in &scenario.variants {
            let golden = pf::load_ansi_frame(&goldens(), id, variant.width).expect("ansi golden");
            let theme = ThemeState::v2_dark();
            let frame = paint_solid_bar_sized(
                &golden_tab_views(),
                Some("second"),
                variant.width as u16,
                variant.height as u16,
                &theme.styles(),
            );
            let render = pf::parse_ansi(
                &crate::render::serialize_canonical(&frame),
                Some((variant.height, variant.width)),
            )
            .expect("render parses");
            let fidelity = pf::compare_ansi_frames_at(&golden, &render, depth);
            assert_eq!(
                fidelity.glyph_match_pct(),
                100.0,
                "{id}@{} glyphs must match exactly",
                variant.width
            );
            assert!(
                fidelity.first_divergence().is_none(),
                "{id}@{} diverged at {:?} — expected full fidelity after the glow upgrade",
                variant.width,
                fidelity.first_divergence()
            );
        }
    }
}

/// The strip reserves room for the " + " affordance exactly like the
/// reference (`ADD_TAB_WIDTH`).
#[test]
fn bar_plus_affordance_alignment() {
    let theme = ThemeState::v2_dark();
    let frame = paint_solid_bar_sized(&golden_tab_views(), Some("second"), 80, 10, &theme.styles());
    let row = &frame.text_grid()[0];
    // Tabs occupy [0, 64); the plus glyph sits at 65 inside " + ".
    assert_eq!(row.chars().nth(65), Some('+'), "row: {row:?}");
    assert_eq!(ADD_TAB_WIDTH, 3);
}

// -- 7.2 wave 0: styled-segment leaf instruments --------------------------------

/// The frozen plain-leaf construction of `tool-wrap-rows` (what
/// `layout_tool_wrap` looked like before the styled-leaf migration) —
/// kept here as the reference side of the migration proof.
fn layout_tool_wrap_plain_leaves(
    fixture: &[crate::components::tool_rows::ToolRowFixture],
    theme: &crate::theme::ThemeStyles,
) -> Vec<crate::layout::Placed> {
    use crate::components::tool_rows::{row_style, BOX_WIDTH, ICON_WIDTH};
    let mut tree = LayoutTree::new();
    let wrappers: Vec<_> = fixture
        .iter()
        .map(|row| {
            let style = row_style(theme, row.failed);
            let icon = tree.text_sized(
                &row.icon.to_string(),
                style,
                &BoxSpec::default().width(ICON_WIDTH).shrink(0.0),
            );
            let label = tree.text_sized(
                &row.label,
                style,
                &BoxSpec::default().grow(1.0).shrink(1.0).min_width(0.0),
            );
            let content = tree.box_node(&BoxSpec::row(), &[icon, label]);
            tree.box_node(&BoxSpec::default().padding_left(3.0), &[content])
        })
        .collect();
    let column = tree.box_node(&BoxSpec::column(), &wrappers);
    let root = tree.box_node(&BoxSpec::column().width(BOX_WIDTH), &[column]);
    tree.set_root(root);
    tree.compute((BOX_WIDTH, AvailableSpace::MaxContent)).placed
}

/// Migration proof: the styled-leaf path (production since wave 0)
/// reproduces the plain-leaf frames **byte-for-byte** — text grid and
/// canonical ANSI — so the new leaf type subsumes the old one at the
/// gated depths.
#[test]
fn tool_wrap_rows_styled_path_is_byte_identical_to_plain_leaves() {
    let theme = ThemeState::v2_dark();
    let fixture = crate::components::tool_rows::tool_wrap_fixture();
    let plain_placed = layout_tool_wrap_plain_leaves(&fixture, &theme.styles());
    let styled_placed = crate::components::tool_rows::layout_tool_wrap(&fixture, &theme.styles());
    for (width, height) in [(80u16, 12u16), (160, 12)] {
        let plain = paint_placed(
            &plain_placed,
            (width, height),
            crate::components::tool_rows::fill_style(&theme.styles()),
        );
        let styled = paint_placed(
            &styled_placed,
            (width, height),
            crate::components::tool_rows::fill_style(&theme.styles()),
        );
        assert_eq!(plain.text_grid(), styled.text_grid(), "{width}: text grid");
        assert_eq!(
            serialize_canonical(&plain),
            serialize_canonical(&styled),
            "{width}: canonical ANSI bytes"
        );
    }
}

/// Markdown-style spans crossing wrap points at 80 and 40 columns,
/// hand-derived: plain prefix, one bold span covering the wrap
/// boundary, plain tail. The bold attribute must follow the span
/// across the line break (per-cell styles from segments).
fn markdown_spans() -> Vec<StyledSpan> {
    let base = Style::new().fg(ratatui::style::Color::Indexed(252));
    let bold = base.add_modifier(Modifier::BOLD);
    vec![
        StyledSpan {
            text: "alpha bravo charlie delta echo foxtrot golf hotel india ".into(),
            style: base,
        },
        StyledSpan { text: "juliet kilo lima mike november oscar papa".into(), style: bold },
        StyledSpan { text: " quebec".into(), style: base },
    ]
}

#[test]
fn markdown_bold_spans_wrap_boundaries_at_80_and_40() {
    let spans = markdown_spans();
    // Expectations updated for the retention seam (7.2 lane C): a
    // breaking space that FITS stays at the line end — the markdown
    // goldens pin its styled trailing cell in the canonical ANSI.
    for (width, expected) in [
        (
            80usize,
            vec![
                "alpha bravo charlie delta echo foxtrot golf hotel india juliet kilo lima mike ",
                "november oscar papa quebec",
            ],
        ),
        (
            40,
            vec![
                "alpha bravo charlie delta echo foxtrot ",
                "golf hotel india juliet kilo lima mike ",
                "november oscar papa quebec",
            ],
        ),
    ] {
        let mut tree = LayoutTree::new();
        let leaf = tree.text_styled_flex(spans.clone());
        let root = tree.box_node(&BoxSpec::column().width(width as f32), &[leaf]);
        tree.set_root(root);
        let placed = tree.compute((width as f32, AvailableSpace::MaxContent)).placed.remove(0);
        assert_eq!(placed.line_texts(), expected, "hand-derived wrap at {width}");

        let frame = paint_placed(std::slice::from_ref(&placed), (width as u16, 6), Style::new());
        // The bold bits land exactly on the bold span's fragments
        // (bold starts 56 cols into the text at 80; at 40 the boundary
        // splits the span on line 2 and the fragment continues line 3).
        let bold = |row: usize, from: usize, to: usize| {
            for col in from..to {
                assert!(
                    frame.cell(row, col).attrs & 1 != 0,
                    "{width}: cell ({row},{col}) must be bold — row {:?}",
                    frame.text_grid()[row]
                );
            }
            for col in 0..from {
                assert_eq!(
                    frame.cell(row, col).attrs & 1,
                    0,
                    "{width}: cell ({row},{col}) must not be bold"
                );
            }
        };
        if width == 80 {
            bold(0, 56, 77); // "juliet kilo lima mike" — mid-span wrap
            bold(1, 0, 19); // "november oscar papa" — span tail
        } else {
            bold(1, 17, 38); // "juliet kilo lima mike" (span opens mid-line)
            bold(2, 0, 19); // "november oscar papa"
        }
        // Golden-style roundtrip: the canonical ANSI re-parses to the
        // same per-cell styles and re-serializes byte-stably.
        let serialized = serialize_canonical(&frame);
        let parsed = pf::parse_ansi(&serialized, Some((6, width))).expect("parses");
        assert_eq!(pf::render_ansi(&parsed), serialized, "{width}: roundtrip stability");
        for (row, line) in frame.text_grid().iter().enumerate() {
            for (col, _) in line.char_indices() {
                assert_eq!(
                    parsed.cell(row, col).style.attrs,
                    frame.cell(row, col).attrs,
                    "{width}: ({row},{col}) attrs survive the roundtrip"
                );
            }
        }
    }
}

// -- 7.2 wave 1 Lane A: dialog goldens ---------------------------------------

use crate::components::command_palette::{palette_options, PaletteCommand};
use crate::components::dialog::render_overlay_with;
use crate::components::dialog_select::{
    frame_from_view, render_select, DialogStyles, SelectAction, SelectOption, SelectProps,
    SelectState,
};
use crate::store::{DialogEntry, DialogKind, DialogSize};

/// The `dialog-select` scenario fixture (`tui-dump/scenarios/
/// session_ui.tsx`): three described items, Beta current, the delete
/// and new row actions.
fn dialog_select_fixture() -> (SelectProps, SelectState) {
    let options = vec![
        SelectOption::titled("Alpha", "a").described("First item"),
        SelectOption::titled("Beta", "b").described("Second item"),
        SelectOption::titled("Gamma", "c").described("Third item"),
    ];
    let actions = vec![
        SelectAction::new("dialog.move_session.delete", "delete", "ctrl+d"),
        SelectAction::new("dialog.move_session.new", "new", "ctrl+a"),
    ];
    let props =
        SelectProps::new("Items", options).with_current(Some("b".to_owned())).with_actions(actions);
    let mut state = SelectState::default();
    state.focus_current(&props);
    (props, state)
}

#[test]
fn dialog_select_matches_text_grid_and_ansi_at_80() {
    let theme = ThemeState::v2_dark();
    let styles = DialogStyles::from_theme(&theme);
    let base = theme.resolved.tokens.background.base;
    let surface = ratatui::style::Color::Rgb(base.r, base.g, base.b);
    let (props, state) = dialog_select_fixture();
    let view = render_select(&props, &state, &styles, surface, 80, 20, Some(20));
    let frame = frame_from_view(&view, (80, 20), surface, styles.default_fg);
    let render = AnsiFrameRender::from_frame(&frame);
    assert_text_grid_match("dialog-select", 80, &render);
    let depth = manifest().comparison_depth_for(&scenario("dialog-select"));
    let golden_ansi =
        std::fs::read_to_string(goldens().join("dialog-select.w80.ansi")).expect("ansi");
    let diff = pf::compare_ansi_bytes_at(&golden_ansi, &render.ansi, depth).expect("parses");
    assert!(
        diff.equal,
        "dialog-select@80 ansi differs at depth {depth}: first divergence {:?}",
        diff.first_divergent_offset
    );
}

#[test]
fn dialog_select_matches_text_grid_and_ansi_at_160() {
    let theme = ThemeState::v2_dark();
    let styles = DialogStyles::from_theme(&theme);
    let base = theme.resolved.tokens.background.base;
    let surface = ratatui::style::Color::Rgb(base.r, base.g, base.b);
    let (props, state) = dialog_select_fixture();
    let view = render_select(&props, &state, &styles, surface, 160, 20, Some(20));
    let frame = frame_from_view(&view, (160, 20), surface, styles.default_fg);
    let render = AnsiFrameRender::from_frame(&frame);
    assert_text_grid_match("dialog-select", 160, &render);
    let depth = manifest().comparison_depth_for(&scenario("dialog-select"));
    let golden_ansi =
        std::fs::read_to_string(goldens().join("dialog-select.w160.ansi")).expect("ansi");
    let diff = pf::compare_ansi_bytes_at(&golden_ansi, &render.ansi, depth).expect("parses");
    assert!(
        diff.equal,
        "dialog-select@160 ansi differs at depth {depth}: first divergence {:?}",
        diff.first_divergent_offset
    );
}

/// The `command-palette` fixture: two custom palette commands (New
/// session / Switch model), no suggested section — the Dialog wrapper
/// composites over an empty app (base background + backdrop).
fn command_palette_frame(width: u16, height: u16) -> Frame {
    let theme = ThemeState::v2_dark();
    let registry = [
        PaletteCommand {
            command: "session.new",
            title: "New session",
            group: "Session",
            bindings: &["ctrl+x n"],
        },
        PaletteCommand {
            command: "model.list",
            title: "Switch model",
            group: "Agent",
            bindings: &["ctrl+x m"],
        },
    ];
    let props = SelectProps::new("Commands", palette_options(&registry)).flat(true).threshold(0.7);
    let entry = DialogEntry {
        kind: DialogKind::CommandPalette,
        size: DialogSize::Medium,
        select: SelectState::default(),
        value: String::new(),
        theme_initial: None,
    };
    // Empty app base: renderer default fg over the theme base bg.
    let base_bg = theme.resolved.tokens.background.base;
    let mut base = crate::render::Frame {
        cells: vec![
            vec![
                crate::render::Cell {
                    ch: ' ',
                    attrs: 0,
                    fg: crate::render::CellColor::Rgb(255, 255, 255),
                    bg: crate::render::CellColor::Rgb(base_bg.r, base_bg.g, base_bg.b),
                };
                usize::from(width)
            ];
            usize::from(height)
        ],
        width: usize::from(width),
        height: usize::from(height),
    };
    render_overlay_with(&mut base, &entry, &props, None, &theme, (width, height));
    base
}

#[test]
fn command_palette_matches_text_grid_and_ansi_at_80() {
    let frame = command_palette_frame(80, 24);
    let render = AnsiFrameRender::from_frame(&frame);
    assert_text_grid_match("command-palette", 80, &render);
    let depth = manifest().comparison_depth_for(&scenario("command-palette"));
    let golden_ansi =
        std::fs::read_to_string(goldens().join("command-palette.w80.ansi")).expect("ansi");
    let diff = pf::compare_ansi_bytes_at(&golden_ansi, &render.ansi, depth).expect("parses");
    assert!(
        diff.equal,
        "command-palette@80 ansi differs at depth {depth}: first divergence {:?}",
        diff.first_divergent_offset
    );
}

#[test]
fn command_palette_matches_text_grid_and_ansi_at_160() {
    let frame = command_palette_frame(160, 24);
    let render = AnsiFrameRender::from_frame(&frame);
    assert_text_grid_match("command-palette", 160, &render);
    let depth = manifest().comparison_depth_for(&scenario("command-palette"));
    let golden_ansi =
        std::fs::read_to_string(goldens().join("command-palette.w160.ansi")).expect("ansi");
    let diff = pf::compare_ansi_bytes_at(&golden_ansi, &render.ansi, depth).expect("parses");
    assert!(
        diff.equal,
        "command-palette@160 ansi differs at depth {depth}: first divergence {:?}",
        diff.first_divergent_offset
    );
}
