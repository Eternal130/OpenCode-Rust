//! The session route: the transcript pane (lane C's renderer) with
//! the composer docked underneath (`routes/session/index.tsx` +
//! `component/session-frame.tsx` shape), replacing the wave-1 chrome
//! stub. The composer is the wave-2 component
//! ([`crate::components::composer`]) — golden-exact on its own, docked
//! here over the store's per-session draft.
//!
//! Footer seam: the composer's status row ([`FooterHints`]) is the
//! prompt-idle integration point — lane B's
//! [`crate::components::footer::prompt_idle`] composes the same
//! `{directory}:{branch}` + keybind-hints inputs when its panel mounts
//! over the composer.

use crate::components::composer::autocomplete;
use crate::components::composer::render::{
    render_prompt_frame, AutocompleteView, FooterHints, PromptView,
};
use crate::components::message::list::{render_transcript, TranscriptOptions};
use crate::components::session_tabs::{self, Attention, TabStatus, TabView};
use crate::render::{paint_placed_with_fills, Frame};
use crate::store::Store;
use ratatui::style::Style;

/// The agent label for the composer's statusline hint: the server
/// agent list's display name, else the raw id titlecased (the
/// footer-prompt-idle golden's "Build" shape).
fn statusline_agent_label(store: &Store, agent: &str) -> String {
    if let Some(rows) = store.server_lists().agents.rows() {
        if let Some(found) = rows.iter().find(|row| row.id == agent) {
            return found.name.clone();
        }
    }
    let mut chars = agent.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// The composer statusline inputs for one session key: the staged
/// agent · model selection (v2's persistent model hint — the
/// footer-prompt-idle statusline). The model falls back through the
/// resolution chain (F8: the footer shows what a submit WOULD commit,
/// `currentSelection` — local.tsx:225-236 — never an empty "unset").
#[must_use]
pub fn composer_statusline(store: &Store, session_id: &str) -> (String, String) {
    let selection = store.selection_for(session_id);
    let agent =
        selection.agent.map(|agent| statusline_agent_label(store, &agent)).unwrap_or_default();
    let model = store.effective_model(session_id).map(|model| model.model_id).unwrap_or_default();
    (agent, model)
}

/// The horizontal tabs strip over the store's tab list (the live mount
/// of `component/session-tabs.tsx` — active pill, unread `•` on
/// inactive tabs, permission `!` on the active tab, the ` + `
/// affordance). `active` marks the active tab (None on the home
/// route — no pill).
#[must_use]
pub fn paint_live_tabs_bar(
    store: &Store,
    active: Option<&str>,
    width: usize,
    styles: &crate::theme::ThemeStyles,
) -> Frame {
    let tabs = store.tabs();
    let permission_live =
        matches!(store.footer().view, crate::components::footer::FooterView::Permission { .. });
    let views: Vec<TabView> = tabs
        .tabs
        .iter()
        .map(|tab| TabView {
            tab: tab.clone(),
            status: TabStatus {
                unread: tabs.unread.get(&tab.session_id).copied(),
                busy: false,
                attention: (permission_live && Some(tab.session_id.as_str()) == active)
                    .then_some(Attention::Permission),
            },
            detail: None,
        })
        .collect();
    let render = session_tabs::render_bar(
        &views,
        active,
        true,
        width,
        styles,
        tabs.hover.as_deref(),
        tabs.add_hovered,
    );
    paint_placed_with_fills(
        &render.placed,
        &render.fills,
        (u16::try_from(width).unwrap_or(u16::MAX), 1),
        Style::new().fg(crate::components::tool_rows::DEFAULT_FOREGROUND).bg(styles.background),
    )
}

/// Render the session route: the tabs strip, the transcript (lane C's
/// renderer over the store's messages), and the composer docked at the
/// bottom with one blank spacing row above it (the session column's
/// gap).
///
/// v2's frame has NO session title row: the strip (`SessionTabs`,
/// app.tsx:1353-1355) is the ONLY surface that shows the active tab's
/// title (`session-frame.tsx` mounts the `Session` scrollbox directly
/// under it — `routes/session/index.tsx:1366-1371` has no title node),
/// so the strip is exactly ONE row and the transcript starts beneath
/// it. (beta.8 bug 2: our port rendered the title twice — the strip
/// pill AND a muted title row.)
#[allow(clippy::too_many_lines)] // the route assembly: strip, transcript, dock, composer
#[must_use]
pub fn render_session(
    store: &Store,
    session_id: &str,
    styles: &crate::theme::ThemeStyles,
    size: (u16, u16),
) -> Frame {
    // The composer's renderer consumes the resolved token tree
    // (decrease/action surfaces); the store carries the live theme.
    let theme = store.theme();
    let (width, height) = (usize::from(size.0), usize::from(size.1));
    let fill =
        Style::new().fg(crate::components::tool_rows::DEFAULT_FOREGROUND).bg(styles.background);

    // The tabs strip (height 1) leads the column when the tabs config
    // is enabled (v2's session-frame renders the strip behind the
    // same `enabled()` gate); the transcript starts directly under
    // it. With tabs off the frame starts at the transcript.
    let show_strip = store.tabs_enabled();
    let strip = show_strip.then(|| paint_live_tabs_bar(store, Some(session_id), width, styles));

    // The composer's docked mount (popup rows above it when the
    // slash palette is open) + the transcript's height budget.
    let (mount, prompt_height, mount_height) =
        composer_mount_frame(store, session_id, &theme, width, height);
    // The queued-prompt dock (v2's `QueuedPromptDock`,
    // index.tsx:2312-2342): one row above the composer —
    // `┃ {n} queued · {next}` — while prompts wait in the serial
    // queue; absent (and byte-invisible) when the queue is empty.
    let queued = store.queued_prompts(session_id);
    let dock_height = usize::from(!queued.is_empty());
    let spacing = usize::from(mount_height + dock_height + 1 < height);
    // Without the strip the chrome is one row shorter (no title row
    // exists in v2 either way).
    let chrome = usize::from(show_strip);
    let transcript_height = height.saturating_sub(mount_height + dock_height + spacing + chrome);
    // The transcript's rows start under the strip (row 0) — no title
    // row shifts them.
    let content_top = chrome;

    let transcript = render_transcript(
        &store.messages(session_id),
        &TranscriptOptions {
            // The store's clock drives the running-tool spinner frames
            // (the vt-temporal harness caught this frozen at 0 when the
            // default leaked in here).
            clock_ms: store.clock(),
            group_rules: store.plugin_group_rules(),
            expanded_groups: store
                .expanded_group_keys(session_id)
                .into_iter()
                .collect::<Vec<String>>(),
            image_strip: crate::components::message::rows::ImageStripOptions {
                enabled: store.image_preview(),
                protocol: store.graphics_protocol(),
                terminal_rows: size.1,
            },
            ..TranscriptOptions::default()
        },
        styles,
        width,
    );
    // Wave A: the scroll driver — record this frame's metrics (the
    // scroll commands' viewport/content inputs) and apply the live
    // offset (sticky sits at the bottom, free keeps the scrolled
    // offset; the scroll keys move it — see `Store::scroll_command`).
    store.set_transcript_metrics(
        session_id,
        crate::store::TranscriptMetrics {
            viewport: transcript_height,
            content_height: transcript.content_height,
        },
    );
    store.scroll_appended(session_id, transcript.content_height);
    let scroll_offset = store.scroll_offset(session_id);
    // The clickable reasoning headers (beta.8 bug 4): record each
    // group row's ABSOLUTE rect for the mouse layer — the same
    // content_top shift the rows below paint under, minus the scroll
    // offset (only rows inside the viewport are hit-testable).
    store.set_transcript_hit_rows(
        transcript
            .rows
            .iter()
            .filter(|row| row.key.starts_with("group:Reasoning:"))
            .filter_map(|row| {
                let screen_y = content_top + row.y.checked_sub(scroll_offset)?;
                (screen_y < content_top + transcript_height).then(|| {
                    crate::store::TranscriptHitRow {
                        session_id: session_id.to_owned(),
                        key: row.key.clone(),
                        y: screen_y,
                        height: row.height,
                    }
                })
            })
            .collect(),
    );
    // The transcript's image placements (R-07) shift under the title
    // row with the rest of the rows, clipped to the viewport like the
    // placed runs are; the store carries them for the post-draw
    // graphics emission.
    let frame_images = transcript
        .images
        .iter()
        .filter_map(|image| {
            // The visible window in transcript coordinates is
            // `[scroll_offset, scroll_offset + transcript_height)`.
            let content_y = usize::from(image.y);
            if content_y < scroll_offset || content_y >= scroll_offset + transcript_height {
                return None;
            }
            let top = content_y - scroll_offset;
            let clamped_rows = image
                .rows
                .min(u16::try_from(transcript_height.saturating_sub(top)).unwrap_or(image.rows));
            Some(crate::image::ImagePlacement {
                x: image.x,
                y: u16::try_from(content_top + top).unwrap_or(u16::MAX),
                id: image.id,
                data: image.data.clone(),
                cols: image.cols,
                rows: clamped_rows,
            })
        })
        .collect::<Vec<_>>();
    store.set_frame_images(frame_images);
    let mut placed = Vec::new();
    let mut fills = Vec::new();
    for row in &transcript.rows {
        // Content coordinates: the viewport shows
        // `[scroll_offset, scroll_offset + transcript_height)` of the
        // transcript (sticky sits at the bottom). The rows' placed
        // lines/fills carry ABSOLUTE transcript coordinates.
        if row.y + row.height <= scroll_offset {
            continue;
        }
        if row.y.saturating_sub(scroll_offset) >= transcript_height {
            break;
        }
        for line in &row.placed {
            if line.y < scroll_offset {
                continue; // the line sits above the viewport window
            }
            let mut shifted = line.clone();
            shifted.y = (line.y + content_top).saturating_sub(scroll_offset);
            placed.push(shifted);
        }
        fills.extend(row.fills.iter().filter(|fill| usize::from(fill.y) >= scroll_offset).map(
            |fill| {
                crate::render::Fill {
                    x: fill.x,
                    y: (u16::try_from(content_top).unwrap_or(0).saturating_add(fill.y))
                        .saturating_sub(u16::try_from(scroll_offset).unwrap_or(0)),
                    width: fill.width,
                    height: fill.height,
                    style: fill.style,
                }
            },
        ));
    }
    let mut frame = paint_placed_with_fills(&placed, &fills, size, fill);
    if let Some(strip) = strip {
        paste(&mut frame, &strip, 0, 0);
    }
    if let Some(dock) = (!queued.is_empty()).then(|| paint_queued_dock(&queued, width, styles)) {
        paste(&mut frame, &dock, 0, height - prompt_height - dock_height);
    }
    paste(&mut frame, &mount, 0, height - prompt_height);
    frame
}

/// The queued-prompt dock row (`QueuedPromptDock`,
/// routes/session/index.tsx:2312-2342): a left border bar, the count
/// in `text.base`, ` · ` then the next prompt's text (newlines
/// flattened) muted, truncated to the width.
fn paint_queued_dock(
    queued: &[crate::store::QueuedPrompt],
    width: usize,
    styles: &crate::theme::ThemeStyles,
) -> Frame {
    let background = ratatui::style::Style::new()
        .fg(crate::components::tool_rows::DEFAULT_FOREGROUND)
        .bg(styles.background);
    let mut frame =
        crate::render::paint_placed(&[], (u16::try_from(width).unwrap_or(u16::MAX), 1), background);
    let count = format!("{} queued", queued.len());
    let next = queued.first().map(|prompt| prompt.text.replace('\n', " ")).unwrap_or_default();
    let base = styles.base.fg.unwrap_or(crate::components::tool_rows::DEFAULT_FOREGROUND);
    let muted = styles.text_muted.fg.unwrap_or(crate::components::tool_rows::DEFAULT_FOREGROUND);
    let mut x = 0usize;
    for (text, color) in
        [("┃", styles.border), (count.as_str(), base), (" · ", muted), (next.as_str(), muted)]
    {
        if x >= width {
            break;
        }
        let budget = width - x;
        let segment = crate::wrap::take_width(text, budget);
        let mut at = x;
        for (col, ch) in segment.chars().enumerate() {
            let column = x + col;
            if column >= width {
                break;
            }
            if let Some(slot) = frame.cells.first_mut().and_then(|row| row.get_mut(column)) {
                slot.ch = ch;
                slot.fg = crate::render::CellColor::from(color);
            }
            at = column + 1;
        }
        x = at;
    }
    frame
}

/// The session composer's docked prompt frame ([`render_session`]'s
/// composer half): the live slash-palette popup (wave-6 — typing `/`
/// opens the command list WITH each command's description), the
/// agent-colored rail, the blinking block caret, and the mount/prompt
/// heights the route's layout needs.
#[must_use]
fn composer_mount_frame(
    store: &Store,
    session_id: &str,
    theme: &crate::theme::ThemeState,
    width: usize,
    height: usize,
) -> (Frame, usize, usize) {
    let composer = store.composer(session_id);
    let (status_agent, status_model) = composer_statusline(store, session_id);
    // The popup over the composer: live slash-palette state
    // (`component/prompt/autocomplete.tsx`'s mount).
    let popup_options = match composer.autocomplete.visible {
        Some(autocomplete::Mode::Command) => Some(composer.autocomplete.options.clone()),
        _ => None,
    };
    let autocomplete = popup_options.as_ref().map(|options| AutocompleteView {
        options: options.as_slice(),
        selected: composer.autocomplete.selected,
        title_width: composer.autocomplete.registry.display_width(),
    });
    // The rail: the ACTIVE AGENT's categorical color once the agent
    // list resolved (v2 `borderHighlight()` at fade alpha 1); while
    // the list loads the no-agent fallback `border.base` stands
    // (promptDisplay keeps its previous undefined color).
    let rail = store.composer_rail_color(session_id).map(crate::render::CellColor::from);
    // The blinking block cursor: painted only in its visible phase
    // (the run loop's clock tick re-renders every 100 ms).
    let caret = crate::components::composer::render::cursor_blink_on(store.clock())
        .then_some(composer.caret());
    let mount_height =
        crate::components::composer::render::mount_height(&composer.draft, width, height)
            .min(height);
    // The popup floats ABOVE the composer mount: the prompt frame
    // renders `popup_rows` popup rows with the composer at
    // `top = popup_rows` inside it, pasted so the composer still docks
    // at the bottom (no popup ⇒ the previous single-mount shape).
    let anchor = height.saturating_sub(mount_height);
    let popup_rows = if matches!(composer.autocomplete.visible, Some(autocomplete::Mode::Command)) {
        crate::components::composer::autocomplete::popup_height(
            composer.autocomplete.options.len(),
            anchor,
        )
    } else {
        0
    };
    let prompt_height = (mount_height + popup_rows).min(height);
    let view = PromptView {
        draft: &composer.draft,
        placeholder: composer.placeholder_text(),
        autocomplete,
        footer: FooterHints {
            location: String::new(),
            agent_shortcut: "shift+tab".into(),
            command_shortcut: "ctrl+p".into(),
            status_agent,
            status_model,
            plugin_hints: store.plugin_footer_hints(),
            // Wave A: the interrupt hint while the session's turn
            // runs (v2 prompt/index.tsx:1887-1903 — the spinner +
            // `esc interrupt` / `esc again to interrupt` status).
            interrupt: store
                .interrupt_hint_armed(session_id, store.clock())
                .map(|armed| {
                    if armed { "esc again to interrupt" } else { "esc interrupt" }.to_owned()
                })
                .unwrap_or_default(),
        },
        top: prompt_height - mount_height,
        rail,
        caret,
    };
    (render_prompt_frame(&view, width, prompt_height, theme), prompt_height, mount_height)
}

/// Paste `region`'s cells at (`x`, `y`); anything left keeps the fill.
fn paste(target: &mut Frame, region: &Frame, x: usize, y: usize) {
    for (row, cells) in region.cells.iter().enumerate() {
        let Some(slot) = target.cells.get_mut(y + row) else { break };
        let count = cells.len().min(slot.len().saturating_sub(x));
        slot[x..x + count].clone_from_slice(&cells[..count]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reactive::Runtime;

    #[test]
    fn session_frame_docks_the_composer_under_the_transcript() {
        let store = Store::new(Runtime::new());
        store.open_tab("s1", Some("Bug hunt".to_owned()));
        let theme = crate::theme::ThemeState::v2_dark();
        let frame = render_session(&store, "s1", &theme.styles(), (40, 12));
        let grid = frame.text_grid().join("\n");
        assert!(grid.contains("Bug hunt"), "title present: {grid}");
        // The docked composer's chrome: left rail + bottom transition
        // on the LAST rows (docked, not floating).
        assert!(grid.contains('┃'), "composer rail present: {grid}");
        assert!(grid.contains('╹'), "composer bottom corner present: {grid}");
        let docked: Vec<String> = frame
            .text_grid()
            .into_iter()
            .rev()
            .take(3)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        assert!(
            docked.iter().any(|row| row.contains('╹')),
            "composer docked at the bottom: {grid}"
        );
    }

    /// D2: the live session route mounts the tabs strip — both tabs'
    /// titles visible in ONE capture, the active tab selected, the
    /// ` + ` affordance present, and an unread inactive tab carries
    /// the `•` marker. beta.8 bug 2 pin: the title renders on exactly
    /// ONE row (v2 has no separate title row under the strip).
    #[test]
    fn session_frame_mounts_the_live_tabs_strip() {
        let store = Store::new(Runtime::new());
        store.open_tab("s1", Some("Alpha notes".to_owned()));
        store.open_tab("s2", Some("Beta second".to_owned()));
        store.set_unread("s2", Some(crate::store::Unread::Activity));
        let theme = crate::theme::ThemeState::v2_dark();
        let frame = render_session(&store, "s1", &theme.styles(), (80, 24));
        let grid = frame.text_grid();
        let strip = &grid[0];
        assert!(
            strip.contains("Alpha notes") && strip.contains("Beta second"),
            "both tab titles in the strip: {strip}"
        );
        assert!(strip.contains(" + "), "the add affordance: {strip}");
        assert!(
            strip.contains(crate::components::session_tabs::UNREAD_MARKER),
            "the unread dot on the inactive tab: {strip}"
        );
        // The strip is ONE row: the active tab's title appears on
        // exactly one screen row (the strip), never duplicated below.
        let rows_with_title =
            grid.iter().filter(|row| row.contains("Alpha notes")).collect::<Vec<_>>();
        assert_eq!(rows_with_title.len(), 1, "title on one row only: {grid:?}");
        assert!(!grid[1].contains("Alpha notes"), "no title row under the strip: {}", grid[1]);
    }

    /// `tabs.enabled = false`: the strip is absent AND no title row
    /// takes its place (v2 renders neither — the title lives only in
    /// the strip), and routing still works.
    #[test]
    fn session_frame_hides_the_strip_when_tabs_disabled() {
        let store = Store::new(Runtime::new());
        store.open_tab("s1", Some("Alpha notes".to_owned()));
        store.set_tabs_enabled(false);
        let theme = crate::theme::ThemeState::v2_dark();
        let frame = render_session(&store, "s1", &theme.styles(), (80, 24));
        let grid = frame.text_grid();
        assert!(!grid[0].contains(" + "), "no add affordance: {}", grid[0]);
        assert!(
            !grid.iter().any(|row| row.contains("Alpha notes")),
            "no title anywhere with tabs off: {grid:?}"
        );
    }

    /// V2: the composer's footer row carries the persistent
    /// agent · model hint from the session's selection.
    #[test]
    fn session_composer_carries_the_model_hint() {
        let store = Store::new(Runtime::new());
        store.open_tab("s1", Some("Bug hunt".to_owned()));
        store.select_model(
            "s1",
            &crate::store::ModelSelection {
                provider_id: "parityfake".into(),
                model_id: "fake-model-a".into(),
                variant: None,
            },
        );
        store.select_agent("s1", "build");
        let theme = crate::theme::ThemeState::v2_dark();
        let frame = render_session(&store, "s1", &theme.styles(), (80, 24));
        let joined = frame.text_grid().join("\n");
        assert!(joined.contains("Build · fake-model-a"), "the hint row: {joined}");
    }

    #[test]
    fn submit_prompt_records_history_and_clears_the_draft() {
        let store = Store::new(Runtime::new());
        let mut composer = store.composer("s1");
        composer.draft.text = "hello".into();
        store.set_composer("s1", &composer);
        let submitted = store.submit_prompt("s1").expect("submitted");
        assert_eq!(submitted.text, "hello");
        assert!(store.prompt_draft("s1").is_empty(), "draft cleared");
        assert_eq!(store.composer("s1").history.entries, vec!["hello".to_owned()]);
    }

    /// F7 — cross-session render purity: switching to another tab's
    /// route and back repaints the SAME session frame byte-for-byte
    /// (no per-session state leaks between buckets; the dogfood's
    /// staircase fragments pinned at the store level).
    #[test]
    fn transcript_renders_identically_across_tab_switches() {
        use crate::components::message::model::{Message, Part, PartTime};
        let store = Store::new(Runtime::new());
        let turn = |n: usize| {
            vec![
                Message::User {
                    id: format!("u{n}"),
                    text: format!("ask number {n}"),
                    created: n as u64,
                    files: Vec::new(),
                },
                Message::Assistant {
                    id: format!("a{n}"),
                    agent: "build".into(),
                    model: "p/m".into(),
                    parts: vec![Part::Text {
                        text: format!("answer number {n} with some longer body text"),
                        time: PartTime::default(),
                    }],
                    finish: Some("stop".into()),
                    error: None,
                    time: PartTime { created: Some(n as u64), completed: Some(n as u64 + 1) },
                },
            ]
        };
        let mut long = Vec::new();
        for n in 0..12 {
            long.extend(turn(n));
        }
        store.set_messages("s_long", long);
        store.set_messages("s_short", turn(0));
        store.open_tab("s_long", Some("Long".to_owned()));
        store.open_tab("s_short", Some("Short".to_owned()));

        let theme = crate::theme::ThemeState::v2_dark().styles();
        let size = (80, 24);
        store.runtime.run_effects();
        let first = render_session(&store, "s_long", &theme, size);
        // Visit the other session, then come back — byte-identical.
        let _ = render_session(&store, "s_short", &theme, size);
        store.runtime.run_effects();
        let back = render_session(&store, "s_long", &theme, size);
        assert_eq!(
            first.text_grid(),
            back.text_grid(),
            "switch-away-and-back repaints the same frame (no cross-session contamination)"
        );
        assert!(
            back.text_grid().iter().any(|row| row.contains("answer number 11")),
            "the long session's tail stays visible (sticky bottom)"
        );
        // A background append then advances only by the new turn: the
        // prior tail rows scroll up, the fresh turn renders at bottom.
        store.set_messages("s_long", {
            let mut grown = store.messages("s_long");
            grown.extend(turn(99));
            grown
        });
        store.runtime.run_effects();
        let after = render_session(&store, "s_long", &theme, size);
        assert!(
            after.text_grid().iter().any(|row| row.contains("answer number 99")),
            "the appended background turn renders (the F19 store-side contract)"
        );
        assert_ne!(after.text_grid(), back.text_grid(), "the frame moved");
        // The SHORT session's transcript never bleeds into the long
        // one's frame: its rows exist only on the strip (row 0).
        assert!(
            !back.text_grid()[1..].iter().any(|row| row.contains("Short")),
            "the other tab's title stays on the strip row only"
        );
    }
}
