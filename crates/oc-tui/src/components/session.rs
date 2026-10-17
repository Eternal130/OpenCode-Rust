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
use crate::components::composer::render::{AutocompleteView, FooterHints, PromptView};
use crate::components::message::list::{render_transcript, TranscriptOptions};
use crate::components::message::model::ToolStatus;
use crate::components::session_tabs::{Attention, TabStatus, TabView};
use crate::components::sidebar::{
    SidebarContextWidget, SidebarMcpWidget, SidebarSections, SidebarView, SIDEBAR_AUTO_BREAKPOINT,
};
use crate::render::{paint_placed_with_fills, Frame};
use crate::store::{SessionTab, Store};
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
///
/// On the HOME route the strip appends the `New session` pseudo-tab
/// and SELECTS it (`items = [...ordered, NEW_SESSION_TAB]` +
/// `activeID = "new"` while `newTab()`, session-tabs.tsx:1339-1343),
/// hiding the idle ` + ` affordance (`showPlus`, :1344) — the census
/// "new-tab padding" family: after an add-release the strip reads
/// `+ New session` (bold, `✕` on hover) instead of a bare ` + `.
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
    // The animated phases' transition stamps: one track per tab,
    // synced at the store clock (busy on/off, glow on/off, the
    // completion arm) — the strip derives every envelope level from
    // these + the clock, stateless per frame.
    let mut tracks = std::collections::HashMap::new();
    let mut views: Vec<TabView> = store
        .strip_tabs_ordered()
        .iter()
        .map(|tab| {
            let busy = store.session_running(&tab.session_id);
            let selected = Some(tab.session_id.as_str()) == active;
            let status = TabStatus {
                unread: tabs.unread.get(&tab.session_id).copied(),
                // `status().busy` (context/session-tabs.tsx:170-174):
                // the session (or family) is working — the spinner +
                // sweep phase's gate.
                busy,
                attention: (permission_live && Some(tab.session_id.as_str()) == active)
                    .then_some(Attention::Permission),
            };
            let complete = status.unread == Some(crate::store::Unread::Activity) && !status.busy;
            tracks.insert(
                tab.session_id.clone(),
                store.tab_pulse_track(
                    &tab.session_id,
                    status.runs(),
                    status.glows(selected),
                    complete,
                ),
            );
            TabView {
                // Session-title-first (session-tabs.tsx:754): the live
                // session registry shadows the tab's persisted title, so
                // a rename that raced the tab's opening still shows.
                tab: SessionTab { title: store.display_title(tab), ..tab.clone() },
                status,
                detail: None,
            }
        })
        .collect();
    // The promoted new-session slot: home + at least one open tab
    // (`tabsAvailable` gates the whole strip on tabs.length > 0).
    let home_pseudo = matches!(store.route(), crate::store::Route::Home) && !views.is_empty();
    let (active, show_plus) = if home_pseudo {
        views.push(TabView {
            tab: SessionTab {
                session_id: crate::components::session_tabs::NEW_SESSION_TAB_ID.to_owned(),
                title: Some(crate::components::session_tabs::NEW_SESSION_TAB_TITLE.to_owned()),
            },
            status: TabStatus::EMPTY,
            detail: None,
        });
        (Some(crate::components::session_tabs::NEW_SESSION_TAB_ID), false)
    } else {
        (active, true)
    };
    // Lane M1 (retired): the strip renders through the opentui-solid
    // tree — the only renderer since the retirement lane deleted the
    // hand-rolled bar walk.
    let close_hover = tabs.hover_close.then(|| tabs.hover.clone()).flatten();
    // The close-hold's frozen layout (lane KM): `heldSessionTabLayout`
    // over the SAME view list (a mismatched list releases the hold).
    let owned: Vec<SessionTab> = views.iter().map(|view| view.tab.clone()).collect();
    let held = store
        .close_hold()
        .and_then(|hold| crate::components::session_tabs::held_session_tab_layout(&hold, &owned));
    crate::components::session_tabs_solid::paint_solid_bar_clock(
        &views,
        active,
        show_plus,
        width,
        styles,
        tabs.hover.as_deref(),
        close_hover.as_deref(),
        tabs.add_hovered,
        held,
        store.clock(),
        tracks,
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
    // it. With tabs off the frame starts at the transcript. The
    // active pill keys on the routed session's family ROOT
    // (session-tabs.tsx:138 — a child-session route keeps the root's
    // pill active, #45).
    let show_strip = store.tabs_enabled();
    let strip_active = show_strip.then(|| store.session_root(session_id));
    let strip =
        show_strip.then(|| paint_live_tabs_bar(store, strip_active.as_deref(), width, styles));

    // The session sidebar (session-frame.tsx:106-117): v2's right
    // pane — `auto` shows it above 120 cols, the manual toggle forces
    // it on (at narrow widths as a scrim overlay, 385-397), subagent
    // sessions never. Docked (wide), the pane takes 42 columns and
    // the transcript column reflows to `width − 42`
    // (SESSION_SIDEBAR_WIDTH, ui/layout.ts:1).
    let sidebar_shown = store.sidebar_shown(session_id, width);
    // The session column's padding (v2's prompt mounts inside
    // paddingLeft/Right 2): the composer `┃` and footer paste at x2.
    let inset = 2;
    let sidebar_docked = sidebar_shown && width > SIDEBAR_AUTO_BREAKPOINT;
    let sidebar_overlay = sidebar_shown && !sidebar_docked;
    let column = if sidebar_docked {
        width - crate::components::session_tabs::SESSION_SIDEBAR_WIDTH
    } else {
        width
    };

    // The composer's docked mount (popup rows above it when the
    // slash palette is open) + the transcript's height budget. With
    // the subagent picker open (or on a child session — #44/#45) the
    // picker panel REPLACES the whole prompt mount (v2's Composer:
    // `open={composer.open || parentID}` at index.tsx:1451 — the
    // ╹▀ transition and status row belong to the prompt, not the
    // panel). Lane M6 (retired): the panel renders through the
    // opentui-solid tree (`picker_solid`) — the only renderer since
    // the retirement lane; it consumes the same `picker_panel_paint`
    // walk. The panel spans the FULL column
    // (its own geometry carries the border col 2), so it pastes at
    // x=0, unlike the prompt mount's inset.
    let picker_live = store.picker_live(session_id);
    let mount_x = if picker_live { 0 } else { inset };
    // The picker panel's entry-row pointer targets (beta.13 bug 2 —
    // v2 subagents-tab.tsx:221-227: hover moves the selection, a
    // release selects + navigates into the child). The panel pastes
    // at (0, height − mount); body row i sits at panel-local y 3+i
    // over content columns [4, column−5] — the same walk
    // `picker_panel_paint` solves.
    let mut picker_hits = crate::store::PickerEntryHits::default();
    let (mount, prompt_height, mount_height, caret_screen) = if picker_live {
        let entries = store.picker_entries(session_id);
        let selected = store.picker_selection(session_id, entries.len());
        let column_u16 = u16::try_from(column).unwrap_or(u16::MAX);
        let panel = crate::components::picker_solid::picker_panel_frame_dispatch(
            &store.picker(),
            &entries,
            selected,
            &theme,
            column_u16,
        );
        let top = height.saturating_sub(crate::components::subagent_picker::PICKER_MOUNT_ROWS);
        let visible = entries.len().min(crate::components::subagent_picker::BODY_ROWS);
        picker_hits = crate::store::PickerEntryHits {
            session_id: session_id.to_owned(),
            rows: (0..visible)
                .map(|index| crate::store::PickerEntryHit {
                    index,
                    session_id: entries[index].session_id.clone(),
                    rect: crate::store::HitRect {
                        x: 4,
                        y: top + 3 + index,
                        width: column.saturating_sub(9).max(1),
                        height: 1,
                    },
                })
                .collect(),
        };
        (
            panel,
            crate::components::subagent_picker::PICKER_MOUNT_ROWS,
            // The height BUDGET is the panel alone (11 rows): the
            // mount's 12th row is the pad BELOW the panel — the ref's
            // transcript viewport runs one row longer than the prompt
            // mount's arithmetic would give (rig-pinned: the parent
            // view keeps `Main flow final reply.` visible).
            crate::components::subagent_picker::PANEL_ROWS,
            None,
        )
    } else {
        composer_mount_frame(store, session_id, &theme, column - inset, width, height)
    };
    store.set_picker_entry_hits(picker_hits);
    // NEW-S1a: position the terminal cursor at the caret cell while
    // the composer owns the frame (cleared when a dialog overlays —
    // v2 blurs the prompt while any dialog is open). The picker
    // panel unmounts the prompt — no caret while it shows.
    store.set_caret_cell(
        caret_screen
            .filter(|_| store.top_dialog().is_none())
            .map(|(x, y)| (u16::try_from(x + inset).unwrap_or(0), u16::try_from(y).unwrap_or(0))),
    );
    // The queued-prompt dock (v2's `QueuedPromptDock`,
    // index.tsx:2312-2342): one row above the composer —
    // `┃ {n} queued · {next}` — while prompts wait in the serial
    // queue; absent (and byte-invisible) when the queue is empty or
    // the composer panel is open (index.tsx:1445's `!composer.open`).
    let queued = if picker_live { Vec::new() } else { store.queued_prompts(session_id) };
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
            collapsed_thoughts: store.collapsed_thought_keys(session_id),
            image_strip: crate::components::message::rows::ImageStripOptions {
                enabled: store.image_preview(),
                protocol: store.graphics_protocol(),
                terminal_rows: size.1,
            },
            // #13: the footer row's model display names (the server
            // model catalog).
            model_names: model_display_names(store),
            // Lane KM (`session.toggle.exploration_grouping`,
            // index.tsx:233 `config.session.grouping !== "none"`): the
            // toggle the transcript walk consults.
            group_exploration: store.exploration_grouping(),
            // #40: the live child-session statuses (the subagent
            // card's running oracle).
            subagent_running: store.subagent_running_map(),
            // The user-block rail keys on the SESSION's agent (the
            // child view's `general` hue, session/index.tsx:2200).
            user_rail: store.session_agent_rail_color(session_id),
            // SURF-1: the tool cells' inline disclosure keys.
            expanded_tools: store.expanded_tool_keys(),
            // SURF-1: the staged revert (the boundary filter + the
            // tail banner), the queued-compaction dividers and the
            // redo shortcut label (index.tsx:1413-1418/2153 — the
            // `session.redo` binding label, "R" until the keymap
            // registry carries it).
            revert: store.revert(session_id),
            pending_compactions: store.pending_compactions(session_id),
            redo_key: "R".to_owned(),
            ..TranscriptOptions::default()
        },
        styles,
        column,
    );
    // Wave A: the scroll driver — record this frame's metrics (the
    // scroll commands' viewport/content inputs) and apply the live
    // offset (sticky sits at the bottom, free keeps the scrolled
    // offset; the scroll keys move it — see `Store::scroll_command`).
    // The VIEWPORT is the scrollbox alone: v2's transcript pane rows
    // are `scrollbox` + the 1-row jump-to-latest slot under it
    // (`session-frame.tsx`: the scrollbox is `flex: 1` beside a
    // `height: 1` indicator row), so the scroll math sees
    // `transcript_height - 1` — the long-reply corpus pins the page
    // steps (-7/-7/+8 at 24 rows ⇒ viewport 15, not the pane's 16).
    store.set_transcript_metrics(
        session_id,
        crate::store::TranscriptMetrics {
            viewport: transcript_height.saturating_sub(1),
            content_height: transcript.content_height,
        },
    );
    store.scroll_appended(session_id, transcript.content_height);
    let scroll_offset = store.scroll_offset(session_id);
    // The transcript's click surface (absolute rects from the last
    // render pass — the same content_top shift the rows paint under,
    // minus the scroll offset; only rows inside the viewport are
    // hit-testable):
    // * reasoning group headers toggle (beta.8 bug 4) with a TIGHT
    //   rect — v2's toggle box is the `InlineToolRow` content
    //   (paddingLeft 3 + the 2-wide icon slot + the label's own
    //   glyphs, `group-view.tsx:176-190`), not the whole row;
    // * user-message blocks open the Message Actions select
    //   (`UserMessage`'s `onMouseUp`, session/index.tsx:2210-2250).
    store.set_transcript_hit_rows(
        transcript
            .rows
            .iter()
            .flat_map(|row| {
                let Some(unclipped) = row.y.checked_sub(scroll_offset) else {
                    return Vec::new();
                };
                let screen_y = content_top + unclipped;
                if screen_y >= content_top + transcript_height {
                    return Vec::new();
                }
                // The head-extent helper: the widest first-line run
                // extent of the row's head placed items (the tight
                // glyph rect the disclosure toggles arm).
                let head_extent = || -> (usize, usize) {
                    let heads = row.placed.iter().filter(|item| item.y == row.y);
                    let x = heads.clone().map(|item| item.x).min().unwrap_or_default();
                    let end = heads
                        .map(|item| {
                            item.x
                                + item
                                    .lines
                                    .first()
                                    .map(|line| {
                                        line.runs
                                            .iter()
                                            .map(|run| crate::wrap::display_width(&run.text))
                                            .sum::<usize>()
                                    })
                                    .unwrap_or_default()
                        })
                        .max()
                        .unwrap_or_default();
                    (x, end)
                };
                // The disclosure toggles: EVERY group kind arms
                // (lane/if item 4 — v2's `useDisclosure` toggle rides
                // `GroupContent`'s InlineToolRow for Reasoning AND
                // Exploration, Activity's and Instructions' own rows:
                // group-view.tsx:152/183/219/256) with a TIGHT rect:
                // v2's toggle box is the InlineToolRow content
                // (paddingLeft 3 + the 2-wide icon slot + the label's
                // own glyphs, `group-view.tsx:176-190`), not the whole
                // row. The thought rows arm as the minimal-mode
                // ReasoningHeader toggle (message-parts.tsx:80-86) —
                // the first line's glyphs.
                let primary = if row.key.starts_with("group:") {
                    let (x, end) = head_extent();
                    (end > x).then(|| crate::store::TranscriptHitRow {
                        session_id: session_id.to_owned(),
                        action: crate::store::TranscriptHitAction::ToggleGroup(row.key.clone()),
                        y: screen_y,
                        height: row.height,
                        x,
                        width: end - x,
                    })
                } else if let Some(rest) = row.key.strip_prefix("thought:") {
                    // `thought:{message_id}:{part_id}` — the part ids
                    // may contain colons themselves; split at the
                    // FIRST (the message id never does).
                    rest.split_once(':').and_then(|(message_id, part_id)| {
                        let (x, end) = head_extent();
                        (end > x).then(|| crate::store::TranscriptHitRow {
                            session_id: session_id.to_owned(),
                            action: crate::store::TranscriptHitAction::ToggleThought {
                                message_id: message_id.to_owned(),
                                part_id: part_id.to_owned(),
                            },
                            y: screen_y,
                            height: 1,
                            x,
                            width: end - x,
                        })
                    })
                } else if let Some(rest) = row.key.strip_prefix("tool:") {
                    // SURF-1: a tool detail cell's click surface. The
                    // action keys on the tool (see `tool_cell_hit`'s
                    // lane-KM v2 arm table): `execute` + background
                    // shells open the detail dialog, the subagent row
                    // navigates to the child session, expandable
                    // tools/failures toggle the INLINE disclosure.
                    rest.split_once(':').and_then(|(message_id, part_id)| {
                        tool_cell_hit(store, session_id, message_id, part_id, row, screen_y)
                    })
                } else if let Some(rest) = row.key.strip_prefix("part:") {
                    // TOOL parts carry the tool click surface (SURF-1);
                    // TEXT/REASONING parts arm their markdown LINK runs
                    // (lane KM, auditA #9 — ui/link.tsx:27's onMouseUp
                    // → openUrl).
                    let part = rest.split_once(':').and_then(|(message_id, part_id)| {
                        let messages = store.messages(session_id);
                        let part = messages
                            .iter()
                            .find(|message| message.id() == message_id)
                            .and_then(|message| {
                                crate::components::message::resolve_part(message.parts(), part_id)
                            });
                        part.map(|part| {
                            matches!(part, crate::components::message::model::Part::Tool { .. })
                        })
                    });
                    match part {
                        Some(true) => rest.split_once(':').and_then(|(message_id, part_id)| {
                            tool_cell_hit(store, session_id, message_id, part_id, row, screen_y)
                        }),
                        _ => None,
                    }
                } else if row.key == "revert:banner" {
                    // The banner's dismiss surface: the raised block's
                    // full width (`RevertMessage`'s onMouseUp,
                    // index.tsx:2093-2113).
                    Some(crate::store::TranscriptHitRow {
                        session_id: session_id.to_owned(),
                        action: crate::store::TranscriptHitAction::DismissRevert {
                            session_id: session_id.to_owned(),
                        },
                        y: screen_y,
                        height: row.height,
                        x: transcript.inset,
                        width: column.saturating_sub(transcript.inset * 2),
                    })
                } else if let Some(message_id) = row.key.strip_prefix("message:") {
                    // The user-message block (`UserMessage`'s inner
                    // content box, session/index.tsx:2219-2250):
                    // full-width rows of the raised box.
                    let content_width = column - transcript.inset * 2;
                    Some(crate::store::TranscriptHitRow {
                        session_id: session_id.to_owned(),
                        action: crate::store::TranscriptHitAction::MessageActions {
                            message_id: message_id.to_owned(),
                        },
                        y: screen_y,
                        height: row.height,
                        x: transcript.inset + 1,
                        width: content_width.saturating_sub(1),
                    })
                } else {
                    None
                };
                let mut hits: Vec<crate::store::TranscriptHitRow> = primary.into_iter().collect();
                // Lane KM: the text/reasoning rows' LINK runs (every
                // `part:` row whose primary arm stayed empty — the
                // tool surfaces never render markdown links).
                if row.key.starts_with("part:") && hits.is_empty() {
                    hits.extend(link_hit_rows(session_id, row, screen_y));
                }
                hits
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
    // The transcript region: rows `[scroll_offset, scroll_offset +
    // transcript_height)` of the column, pasted under the strip.
    // Lane M4 (retired): the region renders through the opentui-solid
    // tree — the only renderer since the retirement lane deleted the
    // hand-rolled region walk (the hit rows, metrics and image
    // placements stay painter-agnostic, computed above).
    let region_size = (
        u16::try_from(column).unwrap_or(u16::MAX),
        u16::try_from(transcript_height).unwrap_or(u16::MAX),
    );
    let region = crate::components::transcript_solid::paint_transcript_region(
        &transcript,
        styles,
        region_size,
        scroll_offset,
    );
    let mut frame = paint_placed_with_fills(&[], &[], size, fill);
    paste(&mut frame, &region, 0, content_top);
    // SURF-1: the jump slot's loading state ("Loading session
    // history…", index.tsx:1425-1427) — right-aligned over the slot
    // row while the transcript fetch is in flight and no rows landed
    // yet (v2's `firstJump()` hold suppresses the jump indicator).
    if store.history_loading(session_id) && transcript.rows.is_empty() {
        let slot_y = content_top + transcript_height.saturating_sub(1);
        let label = "Loading session history…";
        let label_width = crate::wrap::display_width(label);
        let x = column.saturating_sub(inset + label_width);
        let loading = crate::render::paint_placed(
            &[crate::layout::Placed {
                x,
                y: 0,
                width: label_width,
                lines: vec![crate::layout::PlacedLine::plain(
                    label,
                    ratatui::style::Style::new()
                        .fg(styles.text_muted.fg.unwrap_or_default())
                        .bg(styles.background),
                )],
                tag: Some("session-loading-history".into()),
            }],
            (u16::try_from(column).unwrap_or(u16::MAX), 1),
            ratatui::style::Style::new()
                .fg(crate::components::tool_rows::DEFAULT_FOREGROUND)
                .bg(styles.background),
        );
        paste(&mut frame, &loading, 0, slot_y);
    }
    if let Some(strip) = strip {
        paste(&mut frame, &strip, 0, 0);
    }
    if let Some(dock) =
        (!queued.is_empty()).then(|| paint_queued_dock(&queued, column - inset, styles))
    {
        paste(&mut frame, &dock, inset, height - prompt_height - dock_height);
    }
    paste(&mut frame, &mount, mount_x, height - prompt_height);
    // The sidebar pane mounts under the strip across the frame's full
    // remaining height (session-frame.tsx:325-331 — `flexShrink 0`,
    // width 42): docked at the right edge beside the reflowed
    // transcript column, or — when the manual toggle forced it on at
    // a narrow width — over a scrim covering the frame
    // (session-frame.tsx:385-397, the rgba(0,0,0,70) backdrop our
    // opaque text grid renders as black).
    if sidebar_shown {
        let pane_height = height - content_top;
        let view = live_sidebar_view(store, session_id, height);
        // Lane M5 dual-path — the flag selects the opentui-solid pane
        // tree (`sidebar_solid`) or the hand-rolled placed-runs walk
        // (the `OC_SIDEBAR_OPENTUI=0` fallback, one release); BOTH
        // consume the same `render_sidebar` geometry. The content
        // slot's wheel offset (auditA #1) rides the same walk.
        let scroll = store.sidebar_scroll().offset;
        let pane = crate::components::sidebar_solid::sidebar_pane_frame(
            &view,
            styles,
            pane_height,
            scroll,
        );
        let origin_x = if sidebar_docked {
            column
        } else {
            width.saturating_sub(crate::components::session_tabs::SESSION_SIDEBAR_WIDTH)
        };
        // The wheel geometry: the content slot's height + the rows it
        // can show (the same bound the render clips against), plus
        // the pane's absolute columns — `mouse_event`'s Scroll* arm
        // gates on them.
        store.set_sidebar_scroll_geometry(
            view.sections.content_height(),
            crate::components::sidebar::sidebar_content_viewport(&view, styles, pane_height),
            Some((origin_x, crate::components::session_tabs::SESSION_SIDEBAR_WIDTH)),
        );
        // The onboarding card's pointer targets at ABSOLUTE cells: the
        // same `render_sidebar` geometry the pane painted, translated
        // by the paste origin (docked at the transcript column, or the
        // right edge under the overlay scrim). The extra walk only
        // runs while the card is up; a hidden card clears the rects
        // (stale hits after a dismiss would zombie-click).
        let hits = view.onboarding.then(|| {
            crate::components::sidebar::render_sidebar_scrolled(&view, styles, pane_height, scroll)
                .onboarding_hits
        });
        store.set_sidebar_onboarding_hits(hits.flatten().map(|mut hits| {
            let shift = |rect: &mut crate::store::HitRect| {
                rect.x += origin_x;
                rect.y += content_top;
            };
            shift(&mut hits.dismiss);
            shift(&mut hits.connect);
            hits
        }));
        // The MCP section's targets ride the same walk (the header's
        // collapse toggle + the per-server rows) — translated to the
        // pane's paste origin like the onboarding rects above.
        let mut mcp_hits =
            crate::components::sidebar::render_sidebar_scrolled(&view, styles, pane_height, scroll)
                .mcp_hits;
        if let Some(header) = &mut mcp_hits.header {
            header.x += origin_x;
            header.y += content_top;
        }
        for (_, rect) in &mut mcp_hits.rows {
            rect.x += origin_x;
            rect.y += content_top;
        }
        store.set_sidebar_mcp_hits(mcp_hits);
        if sidebar_docked {
            paste(&mut frame, &pane, column, content_top);
        } else if sidebar_overlay {
            // The translucent scrim (session-frame.tsx:385-397): v2
            // composites `rgba(0,0,0,70)` OVER the route content, so
            // typing under the overlay keeps its echo (the registered
            // "no echo under the scrim" seam, ROADMAP 2026-10-12).
            // The text grid has no alpha channel: blend each covered
            // cell's background toward black at 70/255 in place —
            // glyphs (the composer's echo) survive underneath.
            apply_scrim(&mut frame, 0, content_top, width, pane_height);
            paste(&mut frame, &pane, origin_x, content_top);
        }
    } else {
        store.set_sidebar_onboarding_hits(None);
        store.set_sidebar_mcp_hits(crate::store::SidebarMcpHits::default());
        // The pane unmounted — v2's scrollbox goes with it (the
        // offset resets, no x extent gates the wheel).
        store.set_sidebar_scroll_geometry(0, 0, None);
    }
    frame
}

/// The manual-open scrim (session-frame.tsx:385-397): the whole frame
/// area under the strip dims under the floating sidebar — v2 blends/// rgba(0,0,0,70) over the content; the text grid has no alpha, so
/// the scrim paints opaque black (the dominant visual: a dark
/// backdrop with the pane floating right).
/// The row's markdown LINK runs (lane KM, auditA #9): every placed
/// run carrying a `link` URL arms one tight rect — the run's own
/// glyph extent on its line (`ui/link.tsx`'s `<text>` leaf; the
/// release fires `openUrl(href)`).
fn link_hit_rows(
    session_id: &str,
    row: &crate::components::message::list::MeasuredRow,
    screen_y: usize,
) -> Vec<crate::store::TranscriptHitRow> {
    let mut hits = Vec::new();
    for item in &row.placed {
        for (line_index, line) in item.lines.iter().enumerate() {
            let mut run_x = item.x;
            for run in &line.runs {
                let width = crate::wrap::display_width(&run.text);
                if let Some(url) = run.link.clone().filter(|_| !run.text.trim().is_empty()) {
                    hits.push(crate::store::TranscriptHitRow {
                        session_id: session_id.to_owned(),
                        action: crate::store::TranscriptHitAction::OpenLink { url },
                        y: screen_y + item.y + line_index,
                        height: 1,
                        x: run_x,
                        width,
                    });
                }
                run_x += width;
            }
        }
    }
    hits
}

/// One tool cell's hit row (the `tool:`/`part:` arm's shared body):
/// * `execute` + background shells: `openDetails` on click AND on
///   error-click → the detail dialog (`OpenToolDialog`, index.tsx:
///   3207-3218);
/// * the `Subagent`/Task row: its `onClick` navigates to the child session
///   (`metadata.sessionID ?? metadata.sessionId`, index.tsx:3083-3112);
/// * `write`/`edit`/`patch` `BlockTool` cards: NO `onClick`
///   (index.tsx:2989-3016) — never arm;
/// * `read`/`glob`/`grep`/`webfetch`/`websearch` `InlineTool`s carry
///   no `onClick` (index.tsx:2991-3081): arm ONLY while failed (the
///   error reveal — `InlineTool`'s failed branch, index.tsx:
///   2656-2668);
/// * the generic tool: `expandable() = input.length > 0`
///   (index.tsx:2548) — arm when expandable or failed;
/// * the shell card: `expandable() = shellID || overflow`
///   (index.tsx:2927) —
///   the port approximates always-expandable (registered).
fn tool_cell_hit(
    store: &Store,
    session_id: &str,
    message_id: &str,
    part_id: &str,
    row: &crate::components::message::list::MeasuredRow,
    screen_y: usize,
) -> Option<crate::store::TranscriptHitRow> {
    let messages = store.messages(session_id);
    let part = messages
        .iter()
        .find(|message| message.id() == message_id)
        .and_then(|message| crate::components::message::resolve_part(message.parts(), part_id))?;
    let crate::components::message::model::Part::Tool { name, state, .. } = part else {
        return None;
    };
    let canonical = crate::components::message::model::canonical_tool_name(name).to_owned();
    let failed = state.error.is_some() || matches!(state.status, ToolStatus::Error);
    let background_shell = canonical == "shell" && state.metadata.contains_key("shellID");
    let action = if canonical == "execute" || background_shell {
        // openDetails (click AND error-click).
        crate::store::TranscriptHitAction::OpenToolDialog {
            message_id: message_id.to_owned(),
            part_id: part_id.to_owned(),
        }
    } else if canonical == "subagent" {
        // The child-session navigation (metadata sessionID/sessionId).
        let child =
            state.metadata.get("sessionID").or_else(|| state.metadata.get("sessionId"))?.clone();
        crate::store::TranscriptHitAction::NavigateToSession { session_id: child }
    } else if matches!(canonical.as_str(), "write" | "edit" | "patch") {
        // BlockTool cards without onClick — never clickable (even
        // failed: the card renders the error text inline, no toggle).
        return None;
    } else if matches!(canonical.as_str(), "read" | "glob" | "grep" | "webfetch" | "websearch") {
        if !failed {
            return None;
        }
        // The failed InlineTool's error reveal.
        crate::store::TranscriptHitAction::ToggleToolDetail {
            message_id: message_id.to_owned(),
            part_id: part_id.to_owned(),
        }
    } else {
        // The generic tool: expandable (input present) or failed.
        if state.input.is_empty() && !failed {
            return None;
        }
        crate::store::TranscriptHitAction::ToggleToolDetail {
            message_id: message_id.to_owned(),
            part_id: part_id.to_owned(),
        }
    };
    let heads = row.placed.iter().filter(|item| item.y == row.y);
    let x = heads.clone().map(|item| item.x).min().unwrap_or_default();
    let end = heads
        .map(|item| {
            item.x
                + item
                    .lines
                    .first()
                    .map(|line| {
                        line.runs
                            .iter()
                            .map(|run| crate::wrap::display_width(&run.text))
                            .sum::<usize>()
                    })
                    .unwrap_or_default()
        })
        .max()
        .unwrap_or_default();
    (end > x).then(|| crate::store::TranscriptHitRow {
        session_id: session_id.to_owned(),
        action,
        y: screen_y,
        height: row.height,
        x,
        width: end - x,
    })
}

/// The manual-open scrim's alpha blend (session-frame.tsx:385-397):
/// every covered cell's background blends toward black at
/// `70/255` — `rgba(0,0,0,70)` composited over the route content
/// (glyphs keep their fg; only the veil's bg dimming applies, the
/// opentui respectAlpha semantics for a bg-only overlay box).
fn apply_scrim(frame: &mut Frame, x: usize, y: usize, width: usize, height: usize) {
    const SCRIM_ALPHA: f64 = 70.0 / 255.0;
    // The blend factors a bounded 0..=255 channel by a 0..1 alpha.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let dim = |channel: u8| (f64::from(channel) * (1.0 - SCRIM_ALPHA)).round() as u8;
    for row in y..y.saturating_add(height).min(frame.height) {
        for col in x..x.saturating_add(width).min(frame.width) {
            if let Some(cell) = frame.cells.get_mut(row).and_then(|cells| cells.get_mut(col)) {
                if let crate::render::CellColor::Rgb(r, g, b) = cell.bg {
                    cell.bg = crate::render::CellColor::Rgb(dim(r), dim(g), dim(b));
                }
            }
        }
    }
}

/// The live sidebar's content (`routes/session/sidebar.tsx`): the
/// session's title (the tab list's title — the strip's "Untitled
/// session" fallback), the plugin sections (`sidebar.content` — the
/// context/MCP widgets + one per plugin through the slot bridge), the
/// getting-started card (height ≥ 22 and not dismissed, footer.tsx:18-23)
/// and the footer location line `{dir}:{branch}` (the same source the
/// composer footer's status reads — the location worker's vcs lookup).
fn live_sidebar_view(store: &Store, session_id: &str, height: usize) -> SidebarView {
    // Session-title-first (the tab list's title resolution —
    // session-tabs.tsx:754), the strip's "Untitled session" fallback.
    let title = store
        .tabs()
        .tabs
        .iter()
        .find(|tab| tab.session_id == session_id)
        .and_then(|tab| store.display_title(tab))
        .unwrap_or_else(|| "Untitled session".to_owned());
    // The context widget (`contextUsage`, util/session.ts:46-73): the
    // LAST assistant with a token payload after the newest completed
    // compaction; percent divides by the model catalog's context
    // limit. The mm2 seam (docs/migration-m1-tabstrip.md): the ref's
    // live "Context / N tokens" meter — NOW wired on our side; the
    // rig's A/B mismatch class for it retires with this lane.
    // `cost` (the ref's `data.session.cost()` — a per-session server
    // total over the family) has no store source yet: the row waits
    // for the session-info cost field (seam noted in surf1.log).
    let messages = store.messages(session_id);
    let compaction_index = messages
        .iter()
        .rposition(|message| {
            matches!(
                message,
                crate::components::message::model::Message::Compaction {
                    failed: false,
                    running: false,
                    ..
                }
            )
        })
        .map_or(0, |index| index + 1);
    let usage = messages[compaction_index..].iter().rev().find_map(|message| match message {
        crate::components::message::model::Message::Assistant {
            usage_total: Some(total),
            model,
            ..
        } if *total > 0 => Some((*total, model.clone())),
        _ => None,
    });
    let context = match usage {
        Some((tokens, model)) => SidebarContextWidget {
            tokens: Some(tokens),
            percent: {
                let lists = store.server_lists();
                lists
                    .models
                    .rows()
                    .and_then(|models| {
                        models
                            .iter()
                            .find(|entry| model == format!("{}/{}", entry.provider_id, entry.id))
                    })
                    .and_then(|entry| entry.context_limit.filter(|limit| *limit > 0))
                    .map(|limit| {
                        // `Math.round(tokens / limit * 100)` — display
                        // rounding, not accounting (bounded + clamped:
                        // a used-context meter stays in 0..=100; the
                        // allows cover the display-only f64 round-trip).
                        #[allow(
                            clippy::cast_possible_truncation,
                            clippy::cast_sign_loss,
                            clippy::cast_precision_loss
                        )]
                        fn percent(tokens: u64, limit: i64) -> u8 {
                            let ratio = (tokens.min(u64::from(u32::MAX)) as f64
                                / f64::from(limit.clamp(1, 100_000_000) as u32))
                                * 100.0;
                            ratio.round().clamp(0.0, 100.0) as u8
                        }
                        percent(tokens, limit)
                    })
            },
            cost: None,
        },
        None => SidebarContextWidget::default(),
    };
    SidebarView {
        title,
        title_pending_since: store.title_pending_since(session_id),
        clock_ms: store.clock(),
        sections: SidebarSections {
            context,
            mcp: SidebarMcpWidget { servers: store.mcp_servers(), open: store.sidebar_mcp_open() },
            plugins: store.plugin_sidebar_sections(),
        },
        onboarding: height >= 22
            && !store.sidebar_onboarding_dismissed()
            && !store.server_lists().connected,
        location: (!store.footer_location().is_empty()).then(|| store.footer_location().clone()),
    }
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
    terminal_width: usize,
    height: usize,
) -> (Frame, usize, usize, Option<(usize, usize)>) {
    let composer = store.composer(session_id);
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
    // NEW-S1a: v2's live caret is the TERMINAL cursor (never a
    // cell-painted block — the corpus never caught one painted); the
    // route hands the hardware-cursor position to the app instead
    // (`Store::set_caret_cell`).
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
            // #4: the session's `{directory}:{branch}` label when
            // idle (v2 hides it while a turn runs — the interrupt
            // status owns the left slot then).
            location: if store.session_running(session_id) {
                String::new()
            } else {
                store.footer_location()
            },
            agent_shortcut: "shift+tab".into(),
            command_shortcut: "ctrl+p".into(),
            // #4's usage arm: the context-usage segment (PromptFooter's
            // first Match) — present after a completed turn.
            usage: composer_usage(store, session_id),
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
            location_hovered: store.location_hovered(),
        },
        metadata: composer_metadata(store, session_id),
        terminal_width,
        top: prompt_height - mount_height,
        rail,
        caret: None,
    };
    // #33 — the location label's hit rect (the footer row is the
    // prompt mount's LAST row: pasted at `height - prompt_height`,
    // its footer row lands on `height - 1` — the lane/if PTY proof
    // clicked the painted label and caught the rect one row high).
    // The label's painted extent comes from the same math the row
    // paints with. Empty label ⇒ no hit.
    let status_row = height.saturating_sub(1);
    let location_hit = crate::components::composer::render::footer_location_extent(&view, width)
        .map(|(x, label_width)| crate::store::FooterLocationHit {
            directory: view
                .footer
                .location
                .rsplit_once(':')
                .map_or_else(|| view.footer.location.clone(), |(dir, _)| dir.to_owned()),
            row: status_row,
            x,
            width: label_width,
        });
    store.set_footer_location_hit(location_hit);
    let frame = crate::components::composer_solid::render_prompt_frame_dispatch(
        &view,
        width,
        prompt_height,
        theme,
    );
    // NEW-S1a: the caret's screen cell — the mount pastes at
    // (inset, height - prompt_height); the helper's y already includes
    // the composer's `top` inside the mount frame.
    let caret_screen = crate::components::composer::render::prompt_caret_screen_cell(
        &composer.draft,
        width,
        prompt_height - mount_height,
        height,
        composer.caret(),
    )
    .map(|(x, cell_y)| (x, height - prompt_height + cell_y))
    .filter(|(_, row_y)| *row_y < height);
    (frame, prompt_height, mount_height, caret_screen)
}

/// The composer's metadata row inputs (#5, `PromptMetadataRow`): the
/// staged agent + model selection — the agent's display name, the
/// model's display NAME (catalog `name`, `parsed().model`), the
/// provider's display name, and the selected variant.
pub fn composer_metadata(
    store: &Store,
    session_id: &str,
) -> crate::components::composer::render::MetadataLine {
    let selection = store.selection_for(session_id);
    // `promptDisplay().agentLabel = Locale.titlecase(agent.id)` — the
    // CURRENT agent (v2's `local.agent.current()` always resolves;
    // build by default), never an empty label once the list loads.
    let agent_id = selection.agent.or_else(|| {
        store
            .server_lists()
            .agents
            .rows()
            .and_then(|rows| {
                rows.iter()
                    .find(|agent| !agent.hidden)
                    .map(|agent| agent.id.clone())
                    .or_else(|| rows.first().map(|agent| agent.id.clone()))
            })
            .or_else(|| Some("build".to_owned()))
    });
    let agent = agent_id.map(|agent| statusline_agent_label(store, &agent)).unwrap_or_default();
    let model_ref = store.effective_model(session_id);
    let lists = store.server_lists();
    let (model, provider) = match &model_ref {
        Some(reference) => {
            let model = lists
                .models
                .rows()
                .and_then(|rows| {
                    rows.iter()
                        .find(|row| {
                            row.provider_id == reference.provider_id && row.id == reference.model_id
                        })
                        .map(|row| row.name.clone())
                })
                .unwrap_or_else(|| format!("{} (unavailable)", reference.model_id));
            let provider = lists
                .providers
                .iter()
                .find(|row| row.id == reference.provider_id)
                .map_or_else(|| reference.provider_id.clone(), |row| row.name.clone());
            (model, provider)
        }
        None => (String::new(), String::new()),
    };
    crate::components::composer::render::MetadataLine {
        agent,
        // `auto` rides `local.permission.mode === "autoaccept"` —
        // `--auto`'s arm (the rig's config stays "prompt").
        auto: store.permission_autoaccept(),
        model,
        provider,
        // The staged variant (`local.model.variant.current()`); the
        // selection's model ref carries it when pinned.
        variant: model_ref
            .as_ref()
            .and_then(|reference| reference.variant.clone())
            .unwrap_or_default(),
    }
}

/// The footer's context-usage segment (#4's usage arm,
/// `formatContextUsage`): the LAST assistant's
/// `input+output+reasoning+cache` tokens vs the model's context
/// limit — `18 (0%)`; `None` until a turn completed.
fn composer_usage(store: &Store, session_id: &str) -> String {
    let messages = store.messages(session_id);
    let Some(crate::components::message::model::Message::Assistant { model, usage_total, .. }) =
        messages.iter().rev().find(|message| {
            matches!(message, crate::components::message::model::Message::Assistant { .. })
        })
    else {
        return String::new();
    };
    let Some(total) = usage_total else { return String::new() };
    if *total == 0 {
        return String::new();
    }
    let percent = store
        .server_lists()
        .models
        .rows()
        .and_then(|rows| {
            rows.iter()
                .find(|row| format!("{}/{}", row.provider_id, row.id) == *model)
                .and_then(|row| row.context_limit)
        })
        .filter(|limit| *limit > 0)
        .map(|limit| {
            // Integer floor percent — `18 (0%)`: sub-1% reads 0, the
            // ref's Math.round only differs at a .5 boundary.
            total.saturating_mul(100) / u64::try_from(limit).unwrap_or(u64::MAX)
        });
    #[allow(clippy::cast_precision_loss)]
    let number = if *total >= 1_000_000 {
        format!("{:.1}M", *total as f64 / 1_000_000.0)
    } else if *total >= 1_000 {
        format!("{:.1}K", *total as f64 / 1_000.0)
    } else {
        total.to_string()
    };
    match percent {
        Some(percent) => format!("{number} ({percent}%)"),
        None => number,
    }
}

/// The assistant-footer model display-name table (#13): the server
/// model catalog's `name` keyed by `providerID/modelID`.
fn model_display_names(store: &Store) -> Vec<(String, String)> {
    store
        .server_lists()
        .models
        .rows()
        .map(|rows| {
            rows.iter()
                .map(|row| (format!("{}/{}", row.provider_id, row.id), row.name.clone()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
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

    /// Lane/chrome #13: the streamed-turn event sequence must render
    /// the assistant footer row under the reply, with the tok/s
    /// suffix (` · Nms · X.X tok/s`; values per-run, format pinned).
    #[test]
    fn session_frame_renders_the_assistant_footer_row() {
        use crate::data::Ingest;
        let store = Store::new(Runtime::new());
        store.open_tab("s1", Some("Bug hunt".to_owned()));
        let mut started = crate::data::BusEvent::new(
            "session.step.started",
            serde_json::json!({
                "sessionID": "s1",
                "assistantMessageID": "msg_a1",
                "agent": "build",
                "model": { "providerID": "parityfake", "id": "fake-model" },
            }),
        );
        started.created = Some(101);
        store.ingest_all([Ingest::Event(started)]);
        store.ingest_all([Ingest::Event(crate::data::BusEvent::new(
            "session.text.started",
            serde_json::json!({ "sessionID": "s1", "assistantMessageID": "msg_a1" }),
        ))]);
        store.ingest_all([Ingest::Event(crate::data::BusEvent::new(
            "session.text.ended",
            serde_json::json!({ "sessionID": "s1", "assistantMessageID": "msg_a1", "ordinal": 0, "text": "Parity fake says hello." }),
        ))]);
        let mut step_ended = crate::data::BusEvent::new(
            "session.step.ended",
            serde_json::json!({
                "sessionID": "s1",
                "assistantMessageID": "msg_a1",
                "finish": "stop",
                "tokens": { "input": 11, "output": 5, "reasoning": 2 }
            }),
        );
        step_ended.created = Some(259);
        store.ingest_all([Ingest::Event(step_ended)]);
        let theme = crate::theme::ThemeState::v2_dark();
        let frame = render_session(&store, "s1", &theme.styles(), (80, 24));
        let grid = frame.text_grid().join("\n");
        assert!(grid.contains("Parity fake says hello."), "{grid}");
        assert!(grid.contains("Build"), "assistant footer row: {grid}");
        assert!(grid.contains(" · 158ms"), "duration: {grid}");
        assert!(grid.contains(" tok/s"), "tok/s suffix: {grid}");
        let stats =
            grid.lines().find(|line| line.contains("tok/s")).unwrap_or_default().trim().to_owned();
        assert!(stats.starts_with("Build · "), "row shape: {stats}");
        assert!(stats.contains("ms · "), "row shape: {stats}");
    }

    /// Lane/chrome #14: the reasoning group header carries the
    /// duration suffix (`+ Thought · 8ms`) — `session.reasoning.started`
    /// carries `time.created` (the missing reducer this lane adds) and
    /// `reasoning.ended` completes it.
    #[test]
    fn session_frame_renders_the_thought_duration_suffix() {
        use crate::data::Ingest;
        let store = Store::new(Runtime::new());
        store.open_tab("s1", Some("Bug hunt".to_owned()));
        let mut started = crate::data::BusEvent::new(
            "session.step.started",
            serde_json::json!({
                "sessionID": "s1",
                "assistantMessageID": "msg_a1",
                "agent": "build",
                "model": { "providerID": "parityfake", "id": "fake-model" },
            }),
        );
        started.created = Some(100);
        store.ingest_all([Ingest::Event(started)]);
        let mut reasoning_started = crate::data::BusEvent::new(
            "session.reasoning.started",
            serde_json::json!({ "sessionID": "s1", "assistantMessageID": "msg_a1", "ordinal": 0 }),
        );
        reasoning_started.created = Some(100);
        store.ingest_all([Ingest::Event(reasoning_started)]);
        store.ingest_all([Ingest::Event(crate::data::BusEvent::new(
            "session.reasoning.delta",
            serde_json::json!({ "sessionID": "s1", "assistantMessageID": "msg_a1", "ordinal": 0, "delta": "Let me think." }),
        ))]);
        let mut reasoning_ended = crate::data::BusEvent::new(
            "session.reasoning.ended",
            serde_json::json!({ "sessionID": "s1", "assistantMessageID": "msg_a1", "ordinal": 0, "text": "Let me think." }),
        );
        reasoning_ended.created = Some(108);
        store.ingest_all([Ingest::Event(reasoning_ended)]);
        let mut text_ended = crate::data::BusEvent::new(
            "session.text.ended",
            serde_json::json!({ "sessionID": "s1", "assistantMessageID": "msg_a1", "ordinal": 0, "text": "Parity fake says hello." }),
        );
        text_ended.created = Some(120);
        store.ingest_all([Ingest::Event(text_ended)]);
        let mut step_ended = crate::data::BusEvent::new(
            "session.step.ended",
            serde_json::json!({ "sessionID": "s1", "assistantMessageID": "msg_a1", "finish": "stop" }),
        );
        step_ended.created = Some(121);
        store.ingest_all([Ingest::Event(step_ended)]);
        let theme = crate::theme::ThemeState::v2_dark();
        let frame = render_session(&store, "s1", &theme.styles(), (80, 24));
        let grid = frame.text_grid().join("\n");
        assert!(
            grid.contains("Thought · 8ms"),
            "the collapsed header with its duration suffix: {grid}"
        );
    }

    #[test]
    fn session_frame_docks_the_composer_under_the_transcript() {
        let store = Store::new(Runtime::new());
        store.open_tab("s1", Some("Bug hunt".to_owned()));
        let theme = crate::theme::ThemeState::v2_dark();
        let frame = render_session(&store, "s1", &theme.styles(), (40, 12));
        let grid = frame.text_grid().join("");
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

    /// Lane/title #12 — the auto-title CONSUMPTION seam. The forked
    /// title task publishes `session.renamed` while the submit flow's
    /// tab-open may still be in flight (drain.rs `fork_title` races
    /// the prompt response), so the tab label resolves the live
    /// session registry FIRST — v2 `data?.session.get(tab.sessionID)
    /// ?.title ?? tab.title ?? "Untitled session"`
    /// (component/session-tabs.tsx:754) — and a title that landed
    /// BEFORE the tab existed still displays.
    #[test]
    fn tab_title_lands_when_renamed_beat_the_tab_open() {
        use crate::data::Ingest;
        let store = Store::new(Runtime::new());
        // The rename arrives while NO tab exists (the race the rig
        // pinned: renamed at seq 13, navigate open_tab after
        // execution.succeeded).
        store.ingest_all([Ingest::Event(crate::data::BusEvent::new(
            "session.renamed",
            serde_json::json!({ "sessionID": "s1", "title": "Parity fake says hello." }),
        ))]);
        store.open_tab("s1", None);
        let theme = crate::theme::ThemeState::v2_dark();
        let frame = render_session(&store, "s1", &theme.styles(), (80, 24));
        let strip = &frame.text_grid()[0];
        assert!(
            strip.starts_with("   Parity fake says hello."),
            "registry title leads the strip: {strip}"
        );
        assert!(!strip.contains("Untitled session"), "placeholder replaced: {strip}");
    }

    /// #12 placeholder semantics: an untitled session holds
    /// `Untitled session` until a title lands, then the SAME open tab
    /// re-renders it (the renamed event on the live stream).
    #[test]
    fn strip_placeholder_holds_until_titled_then_refreshes() {
        use crate::data::Ingest;
        let store = Store::new(Runtime::new());
        store.open_tab("s1", None);
        let theme = crate::theme::ThemeState::v2_dark();
        let strip = render_session(&store, "s1", &theme.styles(), (80, 24)).text_grid()[0].clone();
        assert!(strip.contains("Untitled session"), "placeholder until titled: {strip}");
        store.ingest_all([Ingest::Event(crate::data::BusEvent::new(
            "session.renamed",
            serde_json::json!({ "sessionID": "s1", "title": "Fix the flux capacitor" }),
        ))]);
        let strip = render_session(&store, "s1", &theme.styles(), (80, 24)).text_grid()[0].clone();
        assert!(strip.contains("Fix the flux capacitor"), "renamed refresh: {strip}");
        assert!(!strip.contains("Untitled session"), "placeholder gone: {strip}");
    }

    /// #12 downstream: a long generated title truncates to the strip
    /// width and the ` + ` affordance keeps its column at the right
    /// margin (the strip's layout math sizes by the title, never the
    /// reverse — `restingTitleWidth`, session-tabs.tsx:751).
    #[test]
    fn long_registry_title_truncates_and_keeps_the_plus_affordance() {
        let store = Store::new(Runtime::new());
        let long_title =
            "A very long generated session title that cannot fit the strip width at all";
        store.open_tab("s1", Some(long_title.to_owned()));
        let theme = crate::theme::ThemeState::v2_dark();
        for width in [40usize, 80, 120] {
            let frame =
                render_session(&store, "s1", &theme.styles(), (u16::try_from(width).unwrap(), 24));
            let grid = frame.text_grid();
            let strip = &grid[0];
            assert_eq!(strip.chars().count(), width, "strip spans the width: {strip}");
            assert!(strip.contains(" + "), "affordance kept at {width}: {strip}");
            assert!(
                strip.contains("A very long generated"),
                "truncated title visible at {width}: {strip}"
            );
            assert!(!strip.contains(long_title), "title truncated at {width}: {strip}");
            // Nothing bleeds onto the row below the strip.
            assert!(!grid[1].contains("generated"), "one-row strip at {width}: {}", grid[1]);
        }
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
        let joined = frame.text_grid().join("");
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
                    tokens: None,
                    usage_total: None,
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

    // -- the live sidebar mount (session-frame.tsx) -----------------------

    /// Auto at 160: the pane docks at the right edge (42 cols), the
    /// transcript + composer reflow to 118, the strip keeps the full
    /// width, and the title + onboarding card render.
    #[test]
    fn wide_session_docks_the_sidebar_and_shrinks_the_transcript() {
        let store = Store::new(Runtime::new());
        store.open_tab("s1", Some("Bug hunt".to_owned()));
        let theme = crate::theme::ThemeState::v2_dark();
        let frame = render_session(&store, "s1", &theme.styles(), (160, 30));
        let grid = frame.text_grid();
        // The pane starts at column 118, row 1 (under the strip): the
        // title 2 cols in on its row-1 (paddingTop 1 + paddingLeft 2).
        let pane = |row: usize| grid[row].chars().skip(118).collect::<String>();
        assert!(pane(2).starts_with("  Bug hunt"), "the title at pane (2, 1): {:?}", pane(2));
        assert!(pane(2).chars().take(2).all(|c| c == ' '), "cols 118-119 blank (paddingLeft 2)");
        // The onboarding card shows at height 30 (>= 22).
        assert!(grid.iter().any(|row| row.contains("Getting started")), "the onboarding card");
        // The composer's left border stays inside the reflowed column
        // (the box's `┃` rows); the sidebar region keeps its own
        // background — no composer bytes leak into the pane.
        assert!(
            grid.iter().any(|row| {
                row.starts_with("┃") || row.starts_with(" ┃") || row.starts_with("  ┃")
            }),
            "the composer mounts at the reflowed column"
        );
        let card_row = grid
            .iter()
            .position(|row| row.contains("Getting started"))
            .expect("the onboarding card");
        assert!(
            grid[card_row].chars().nth(118).is_some_and(|c| c == ' '),
            "no composer bytes in the pane"
        );
    }

    /// Auto at 80 (below the breakpoint): NO pane — the transcript and
    /// composer span the full width (the pre-sidebar shape, byte-for-
    /// byte the left column of the wide frame).
    #[test]
    fn narrow_session_renders_without_the_sidebar() {
        let store = Store::new(Runtime::new());
        store.open_tab("s1", Some("Bug hunt".to_owned()));
        let theme = crate::theme::ThemeState::v2_dark();
        let frame = render_session(&store, "s1", &theme.styles(), (80, 24));
        let grid = frame.text_grid();
        assert!(
            !grid.iter().any(|row| row.contains("Getting started")),
            "no sidebar content at 80 cols"
        );
    }

    /// The manual toggle at a narrow width floats the pane over a
    /// black scrim (session-frame.tsx:385-397) — the pane still
    /// renders its title at the right edge, the transcript underneath
    /// is covered.
    #[test]
    fn manual_toggle_at_narrow_width_overlays_the_pane() {
        let store = Store::new(Runtime::new());
        store.open_tab("s1", Some("Bug hunt".to_owned()));
        store.set_terminal_size((80, 24));
        store.toggle_sidebar();
        assert!(store.sidebar_shown("s1", 80));
        let theme = crate::theme::ThemeState::v2_dark();
        let frame = render_session(&store, "s1", &theme.styles(), (80, 24));
        let grid = frame.text_grid();
        let title: String = grid[2].chars().skip(40).collect();
        assert!(title.contains("Bug hunt"), "the pane floats at the right edge: {title}");
        // The scrim covers the transcript column under the strip.
        assert!(grid[1].chars().take(38).all(|c| c == ' '), "the scrim covers the frame");
    }

    /// The plugin slot bridge's sections render in the LIVE pane (the
    /// `sidebar.content` seam): the store's plugin sections paint
    /// under the title.
    #[test]
    fn plugin_sidebar_sections_render_in_the_live_pane() {
        let store = Store::new(Runtime::new());
        store.open_tab("s1", Some("Bug hunt".to_owned()));
        store.set_plugin_sidebar_section(&crate::components::sidebar::SidebarSection {
            plugin_id: "tuislots".into(),
            title: "Fixture".into(),
            rows: vec!["preset: zhipuai".into(), "sessions: 7".into()],
        });
        let theme = crate::theme::ThemeState::v2_dark();
        let frame = render_session(&store, "s1", &theme.styles(), (160, 30));
        let grid = frame.text_grid();
        let pane: Vec<String> = grid.iter().map(|row| row.chars().skip(118).collect()).collect();
        assert!(pane[3].starts_with("  Fixture"), "the section title: {:?}", pane[3]);
        assert!(pane[4].starts_with("  preset: zhipuai"), "row 1: {:?}", pane[4]);
        assert!(pane[5].starts_with("  sessions: 7"), "row 2: {:?}", pane[5]);
    }

    /// A subagent session never mounts the pane (session-frame.tsx:108).
    #[test]
    fn subagent_session_never_shows_the_sidebar() {
        let store = Store::new(Runtime::new());
        store.open_tab("sub", None);
        store.ingest_all([crate::data::Ingest::Event(crate::data::BusEvent::new(
            "session.created",
            serde_json::json!({ "sessionID": "sub", "parentID": "parent" }),
        ))]);
        let theme = crate::theme::ThemeState::v2_dark();
        let frame = render_session(&store, "sub", &theme.styles(), (160, 30));
        assert!(
            !frame.text_grid().iter().any(|row| row.contains("Getting started")),
            "no pane for subagent sessions"
        );
    }

    /// The connected gate (auditB #4 — GATE-MISMATCH vs
    /// footer.tsx:18-23's `!hasConnectedProvider(integrations)`): the
    /// card hides once an integration is connected (our
    /// `ServerLists.connected` fold), and the hit rects clear with it.
    #[test]
    fn connected_provider_hides_the_onboarding_card() {
        let store = Store::new(Runtime::new());
        store.open_tab("s1", Some("Bug hunt".to_owned()));
        let theme = crate::theme::ThemeState::v2_dark();
        let before = render_session(&store, "s1", &theme.styles(), (160, 30));
        assert!(
            before.text_grid().iter().any(|row| row.contains("Getting started")),
            "the card shows while disconnected"
        );
        assert!(store.sidebar_onboarding_hits().is_some(), "the targets recorded");
        store.set_integration_connected(true);
        let after = render_session(&store, "s1", &theme.styles(), (160, 30));
        assert!(
            !after.text_grid().iter().any(|row| row.contains("Getting started")),
            "a connected provider gates the card off"
        );
        assert!(store.sidebar_onboarding_hits().is_none(), "no targets without the card");
    }

    /// The dismissal gate (`!onboarding.dismissed`, footer.tsx:22):
    /// the boot-restored flag hides the card the same way.
    #[test]
    fn restored_dismissal_hides_the_onboarding_card() {
        let store = Store::new(Runtime::new());
        store.open_tab("s1", Some("Bug hunt".to_owned()));
        store.restore_sidebar_onboarding(true);
        let theme = crate::theme::ThemeState::v2_dark();
        let frame = render_session(&store, "s1", &theme.styles(), (160, 30));
        assert!(
            !frame.text_grid().iter().any(|row| row.contains("Getting started")),
            "the persisted dismissal keeps the card hidden"
        );
    }

    /// #44/#45 — the picker panel REPLACES the prompt mount when live
    /// (open or a child route): the panel's border column paints at
    /// the frame's col 2 (its own geometry carries the inset), the
    /// `╹` prompt transition never renders, and the strip keeps the
    /// family ROOT's title on a child route (no new tab).
    #[test]
    fn picker_live_replaces_the_prompt_and_roots_the_strip() {
        let store = Store::new(Runtime::new());
        store.open_tab("root", Some("Root flow".to_owned()));
        store.navigate(crate::store::Route::Session { session_id: "root".to_owned() });
        store.ingest_all([crate::data::Ingest::Event(crate::data::BusEvent::new(
            "session.created",
            serde_json::json!({
                "sessionID": "ses_child", "parentID": "root",
                "agent": "general", "title": "Check the fixture"
            }),
        ))]);
        store.ingest_all([crate::data::Ingest::Event(crate::data::BusEvent::new(
            "session.execution.succeeded",
            serde_json::json!({ "sessionID": "ses_child" }),
        ))]);
        let mut picker = store.picker();
        picker.open = true;
        picker.filter_active = false;
        store.set_picker(picker);
        let theme = crate::theme::ThemeState::v2_dark();
        let frame = render_session(&store, "root", &theme.styles(), (80, 24));
        let grid = frame.text_grid();
        assert!(
            grid.iter().any(|row| row.contains("Subagents  Shell  Terminals")),
            "the panel's tabs row: {grid:?}"
        );
        assert!(
            grid.iter().any(|row| row.contains("General: Check the fixture")),
            "the entry row: {grid:?}"
        );
        assert!(
            !grid.iter().any(|row| row.contains('╹')),
            "the prompt's transition row must not render under the panel: {grid:?}"
        );
        // Navigate into the child: the strip keeps the ROOT's pill.
        store.picker_navigate();
        assert!(
            matches!(store.route(), crate::store::Route::Session { ref session_id } if session_id == "ses_child")
        );
        assert!(store.picker_live("ses_child"), "the child always mounts the panel");
        let child_frame = render_session(&store, "ses_child", &theme.styles(), (80, 24));
        let strip = child_frame.text_grid()[0].clone();
        assert!(strip.contains("Root flow"), "the root's pill stays active: {strip}");
        assert!(!strip.contains("Check the fixture"), "no tab for the child: {strip}");
    }
}
