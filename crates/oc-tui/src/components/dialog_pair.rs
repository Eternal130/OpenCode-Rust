//! `DialogPair` (`/pair` — ref `component/dialog-pair.tsx`): a CUSTOM
//! dialog, not a `DialogSelect`. The ref component renders a
//! word-wrapped intro (`wrapMode="word"`, :46-48), the `This device` /
//! `Links` URL blocks (:49-68), the loopback hint (:70-72), the
//! loading / error states (:100-127) — free-flowing text rows that the
//! select pipeline cannot model (an option row is ONE line, so the
//! 65/76-char copy would have to truncate; word wrap is the point).
//!
//! This module is the content model: the row walk (already
//! word-wrapped to the content width) + the per-row style. The tree
//! lives in [`crate::components::dialog_solid`] (`Family::Pair`), the
//! geometry in the ref's root box (`paddingLeft/right 2,
//! paddingBottom 1, gap 1`, :90).
//!
//! REGISTERED divergences (bounded, see the task-7 report):
//! REGISTERED divergences — BOTH now CLOSED (2026-10-26):
//! * QR (f15-a): the QR block (ref :75-84, `renderUnicodeCompact(
//!   links[0], { border: 1 })`) renders via the vendored `oc-uqr`
//!   crate (third_party/oc-uqr) — a faithful Rust port of the
//!   ref-pinned uqr 0.1.3, matrix-identical by golden test (same
//!   encoder closes the CLI `pair` gap, oc-cli misc.rs).
//! * ≥96 split (f15-b): the ref gates on the TERMINAL width
//!   (`createMemo(() => dimensions().width >= 96)`, dialog-pair.tsx:39)
//!   and seats the text column at a FIXED 29 cells (`width={horizontal
//!   ? 29 : "100%"}`, :45) beside the QR column. The earlier port
//!   registered "content width ≥ 96 never fires at Large" — it gated
//!   on the wrong width; the gate below is the terminal width.

use crate::store::PairState;

/// The horizontal-split gate (`dialog-pair.tsx:39`: `dimensions()
/// .width >= 96` — the TERMINAL width, not the dialog's).
pub const SPLIT_TERMINAL_WIDTH: usize = 96;

/// The split's fixed text-column width (`dialog-pair.tsx:45`:
/// `width={horizontal ? 29 : "100%"}`).
pub const SPLIT_COLUMN_WIDTH: usize = 29;

/// The split gate (dialog-pair.tsx:39): at ≥96 TERMINAL columns the
/// content lays out as a horizontal row (text column | QR column).
#[must_use]
pub fn is_horizontal(terminal_width: usize) -> bool {
    terminal_width >= SPLIT_TERMINAL_WIDTH
}

/// The width the text walk wraps to (dialog-pair.tsx:44-45): 29 in
/// the horizontal split (the fixed left column), the box's inner
/// width otherwise (`"100%"`).
#[must_use]
pub fn walk_width(terminal_width: usize, content_width: usize) -> usize {
    if is_horizontal(terminal_width) {
        SPLIT_COLUMN_WIDTH
    } else {
        content_width
    }
}

/// The loopback hint (`dialog-pair.tsx:70-72`) — 76 chars, word-wrap
/// territory at every dialog width below xlarge.
const LOOPBACK_HINT: &str =
    "Run `opencode service set hostname 0.0.0.0` to access the service remotely.";

/// One row of the pair dialog's content (the ref's content walk,
/// flattened to pre-wrapped lines + the flex `gap`s between the
/// content's child boxes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairRow {
    /// A `gap 1` row between the content's child boxes (renders
    /// blank).
    Gap,
    /// One text row, already wrapped to the content width.
    Text { text: String, style: PairStyle },
    /// One row of the QR block (ref :81-83): a line of uqr's
    /// `renderUnicodeCompact(links[0], { border: 1 })` half-block
    /// output, painted `theme.text.base`. Pre-rendered (never
    /// wrapped — a Large dialog's content is 84 cols, the QR for
    /// any sane pairing URL is ≤ 43).
    Qr { text: String },
}

/// A pair row's paint (the ref's `fg` ladder: `text.muted`,
/// `text.base`, `text.feedback.error.base` + BOLD).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairStyle {
    /// `theme.text.muted` — the intro, the labels, the hints.
    Muted,
    /// `theme.text.base` — the links themselves.
    Base,
    /// `text.feedback.error.base` + BOLD — the failed-load header.
    ErrorBold,
}

/// The content walk (`dialog-pair.tsx:40-87`): the state's rows at
/// `content_width` columns (the box's inner width — chrome width
/// minus the root's paddingLeft/right 2).
#[must_use]
pub fn rows(state: &PairState, content_width: usize) -> Vec<PairRow> {
    let width = content_width.max(1);
    let muted_lines = |text: &str| -> Vec<PairRow> {
        crate::wrap::wrap_label(text, width)
            .into_iter()
            .map(|line| PairRow::Text { text: line, style: PairStyle::Muted })
            .collect()
    };
    match state {
        // The resource's pending state (:102).
        PairState::Loading => vec![PairRow::Text {
            text: "Loading server information…".to_owned(),
            style: PairStyle::Muted,
        }],
        // The catch arm (:119-127): the bold error header, the
        // wrapped error body, the retry hint.
        PairState::Failed(error) => {
            let mut out = vec![PairRow::Text {
                text: "Could not load server information".to_owned(),
                style: PairStyle::ErrorBold,
            }];
            out.extend(muted_lines(error));
            out.push(PairRow::Text {
                text: "Close and reopen Pair to try again.".to_owned(),
                style: PairStyle::Muted,
            });
            out
        }
        // The loaded content (:44-74): the wrapped intro (:46-48),
        // the localhost block (:49-58), the links block (:59-68),
        // the loopback hint (:69-73). The URL rows WORD-WRAP like
        // every other text: the ref renders them through `<Link>`
        // (ui/link.tsx:33-39), whose `<text>` sets NO `wrapMode`, and
        // the TextRenderable default is `"word"`
        // (@opentui/core chunk-node-wp7ct2m6.js:2833,2847 — the
        // `_defaultOptions` / `options.wrapMode ??
        // _defaultOptions.wrapMode` read). A hard truncation below
        // some width is NOT ref behavior: an over-width URL wraps
        // onto continuation rows (an unbroken token hard-breaks, as
        // `wrap_label` does).
        PairState::Loaded { links, localhost, minutes, loopback } => {
            let mut out = muted_lines(&format!(
                "Open a link to connect. Links work once and expire in {minutes} minutes."
            ));
            let base_lines = |url: &str| -> Vec<PairRow> {
                crate::wrap::wrap_label(url, width)
                    .into_iter()
                    .map(|line| PairRow::Text { text: line, style: PairStyle::Base })
                    .collect()
            };
            if let Some(url) = localhost {
                out.push(PairRow::Gap);
                out.push(PairRow::Text { text: "This device".to_owned(), style: PairStyle::Muted });
                out.extend(base_lines(url));
            }
            out.push(PairRow::Gap);
            out.push(PairRow::Text { text: "Links".to_owned(), style: PairStyle::Muted });
            for url in links {
                out.extend(base_lines(url));
            }
            if *loopback {
                out.push(PairRow::Gap);
                out.extend(muted_lines(LOOPBACK_HINT));
            }
            // The QR block (:75-84): the content box's `gap={2}`
            // (:44) seats it two blank rows below the text column;
            // `Show when={value.links[0]}` gates the QR text itself.
            // Column layout (the port's only reachable layout) — the
            // QR box is `width="100%"`, `alignItems="center"`.
            out.push(PairRow::Gap);
            out.push(PairRow::Gap);
            if let Some(first) = links.first() {
                for line in oc_uqr::render_unicode_compact(first, 1).split('\n') {
                    out.push(PairRow::Qr { text: line.to_owned() });
                }
            }
            out
        }
    }
}

/// The height<36 scrollbox budget (`dialog-pair.tsx:104-112`):
/// `max(8, height - height/4 - 6)` content rows under an invisible
/// scrollbar. The port clips the window to the budget (the ref's
/// scrollbox is pointer-scrolled only).
#[must_use]
pub fn scroll_budget(terminal_height: usize) -> usize {
    if terminal_height >= 36 {
        usize::MAX
    } else {
        (terminal_height.saturating_sub(terminal_height / 4)).saturating_sub(6).max(8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str = "http://localhost:4096/auth/connect/pair-code-123";

    fn loaded() -> PairState {
        PairState::Loaded {
            links: vec!["http://127.0.0.1:4096/auth/connect/pair-code-123".to_owned()],
            localhost: Some(URL.to_owned()),
            minutes: 10,
            loopback: true,
        }
    }

    fn texts(rows: &[PairRow]) -> Vec<String> {
        rows.iter()
            .filter_map(|row| match row {
                PairRow::Gap | PairRow::Qr { .. } => None,
                PairRow::Text { text, .. } => Some(text.clone()),
            })
            .collect()
    }

    fn qr_rows(rows: &[PairRow]) -> Vec<String> {
        rows.iter()
            .filter_map(|row| match row {
                PairRow::Qr { text } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn loading_row() {
        assert_eq!(
            rows(&PairState::Loading, 84),
            vec![PairRow::Text {
                text: "Loading server information…".to_owned(),
                style: PairStyle::Muted,
            }]
        );
    }

    #[test]
    fn failed_state_rows() {
        let rows = rows(&PairState::Failed("connection refused".to_owned()), 84);
        assert_eq!(
            rows[0],
            PairRow::Text {
                text: "Could not load server information".to_owned(),
                style: PairStyle::ErrorBold,
            }
        );
        let texts = texts(&rows);
        assert!(texts.contains(&"connection refused".to_owned()));
        assert!(texts.contains(&"Close and reopen Pair to try again.".to_owned()));
    }

    /// The pty proof's exact fixture: at a Large dialog's content
    /// width (88 − 4) the 65-char intro is ONE row — the full
    /// "expire in 10 minutes" substring renders (F15 C1: word wrap,
    /// never the option-row truncation).
    #[test]
    fn intro_and_links_at_large_content_width() {
        let rows = rows(&loaded(), 84);
        let texts = texts(&rows);
        assert!(
            texts.iter().any(|line| line.contains("expire in 10 minutes")),
            "the full expiry copy on one row: {texts:?}"
        );
        assert!(texts.contains(&"This device".to_owned()));
        assert!(texts.contains(&URL.to_owned()), "the localhost link row");
        assert!(texts.contains(&"Links".to_owned()));
        assert!(
            texts.iter().any(|line| line.contains("opencode service set hostname 0.0.0.0")),
            "the loopback hint"
        );
        // The styled ladder: the intro is muted, the link rows base.
        assert!(matches!(rows[0], PairRow::Text { style: PairStyle::Muted, .. }));
        assert!(rows.iter().any(|row| matches!(
            row,
            PairRow::Text { text, style: PairStyle::Base } if text == URL
        )));
    }

    /// Word wrap at narrow widths: every row fits the width and the
    /// expiry copy stays CONTIGUOUS on one row for every width that
    /// can hold its tail (the [35..49] band breaks the line before
    /// `expire`; ≥ 65 holds the whole intro). Below 35 or inside
    /// 50-64 the greedy break lands inside the substring — same as
    /// the tsx's `wrapMode="word"`.
    #[test]
    fn word_wrap_keeps_the_expiry_copy_contiguous() {
        for width in [35, 40, 49, 65, 84] {
            let rows = rows(&loaded(), width);
            for row in &rows {
                if let PairRow::Text { text, .. } = row {
                    assert!(
                        crate::wrap::display_width(text) <= width,
                        "row fits {width}: {text:?}"
                    );
                }
            }
            let joined = texts(&rows);
            assert!(
                joined.iter().any(|line| line.contains("expire in 10 minutes")),
                "width {width} keeps the expiry copy on one row: {joined:?}"
            );
        }
    }

    /// The height<36 scrollbox budget (:107): max(8, h − h/4 − 6);
    /// at ≥36 the content is unclipped.
    #[test]
    fn scrollbox_budget() {
        assert_eq!(scroll_budget(30), 17);
        assert_eq!(scroll_budget(20), 9);
        assert_eq!(scroll_budget(17), 8);
        assert_eq!(scroll_budget(36), usize::MAX);
    }

    #[test]
    fn split_gate_is_terminal_width() {
        assert!(!is_horizontal(95), "below the threshold: stacked");
        assert!(is_horizontal(96), "the ref's exact >= 96 gate");
        assert!(is_horizontal(160));
        // The walk width: 29 (the fixed text column, :45) in the
        // split; the caller's content width otherwise.
        assert_eq!(walk_width(96, 84), 29);
        assert_eq!(walk_width(160, 84), 29);
        assert_eq!(walk_width(95, 84), 84);
        assert_eq!(walk_width(80, 60), 60);
    }

    /// In the split the text walk wraps to the 29-cell column
    /// (dialog-pair.tsx:45): the intro breaks across rows and every
    /// row fits 29 cells; below the gate the same intro is one row
    /// at the full content width.
    #[test]
    fn split_wraps_the_text_column_to_29() {
        let stacked = rows(&loaded(), 84);
        assert!(
            texts(&stacked).iter().any(|line| line.contains("expire in 10 minutes")),
            "stacked keeps the whole intro on one row"
        );
        let split = rows(&loaded(), walk_width(96, 84));
        assert!(
            !texts(&split).iter().any(|line| line.contains("expire in 10 minutes")),
            "the 29-cell column cannot hold the 65-char intro on one row"
        );
        for row in &split {
            if let PairRow::Text { text, .. } = row {
                assert!(crate::wrap::display_width(text) <= 29, "row fits 29: {text:?}");
            }
        }
        // The blocks survive the narrow column: the labels and the
        // (truncated) link rows are still present.
        let joined = texts(&split).join("\n");
        assert!(joined.contains("This device"));
        assert!(joined.contains("Links"));
        assert!(joined.contains("auth/connect") || joined.contains("localhost"));
    }

    /// The QR block (ref dialog-pair.tsx:75-84): two `gap 2` rows
    /// below the text column, then the compact unicode QR of the
    /// FIRST link (`renderUnicodeCompact(links[0], { border: 1 })`)
    /// — matrix-identical to the ref-pinned uqr (third_party/oc-uqr
    /// goldens). At Large's 84-col content the 31-wide QR fits whole.
    #[test]
    fn qr_block_matches_uqr_output() {
        let rows = rows(&loaded(), 84);
        let qr = qr_rows(&rows);
        assert!(!qr.is_empty(), "the QR rows render");
        // The exact expected block: uqr renderUnicodeCompact of the
        // first link (127.0.0.1 URL), border 1.
        assert_eq!(
            qr.join("\n"),
            oc_uqr::render_unicode_compact("http://127.0.0.1:4096/auth/connect/pair-code-123", 1)
        );
        // The `gap 2` seat: exactly two Gap rows between the last
        // text row and the first QR row.
        let tail = &rows[rows.len() - qr.len() - 2..rows.len() - qr.len()];
        assert!(matches!(tail[0], PairRow::Gap) && matches!(tail[1], PairRow::Gap));
    }

    /// No links → the `Show when={links[0]}` gate stays closed: the
    /// two gap rows still render (the content box's `gap={2}` is
    /// unconditional, dialog-pair.tsx:44) but no QR rows.
    #[test]
    fn qr_gated_on_first_link() {
        let rows = rows(
            &PairState::Loaded { links: vec![], localhost: None, minutes: 10, loopback: false },
            84,
        );
        assert!(qr_rows(&rows).is_empty());
        assert!(matches!(rows[rows.len() - 1], PairRow::Gap));
        assert!(matches!(rows[rows.len() - 2], PairRow::Gap));
    }

    /// F15 residual: NO hard truncation of the URLs at narrow
    /// content widths. The ref renders links through `<Link>`
    /// (ui/link.tsx:33-39) with NO `wrapMode`, and the TextRenderable
    /// default is `"word"` (@opentui/core chunk-node-wp7ct2m6.js
    /// :2833,2847) — an over-width URL WRAPS (an unbroken token
    /// hard-breaks), and the full URL is recoverable by joining the
    /// row fragments. Truncating below any width column count is NOT
    /// ref behavior.
    #[test]
    fn narrow_width_urls_wrap_never_truncate() {
        for width in [20, 33, 43, 46, 53, 54, 60] {
            let rows = rows(&loaded(), width);
            for url in [URL, "http://127.0.0.1:4096/auth/connect/pair-code-123"] {
                // The Base rows between "This device"/"Links" labels
                // and the next block reassemble to the full URL.
                let joined: String = rows
                    .iter()
                    .filter_map(|row| match row {
                        PairRow::Text { text, style: PairStyle::Base } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("");
                assert!(
                    joined.contains(url),
                    "width {width}: the full URL survives (got {joined:?})"
                );
                assert!(
                    rows.iter().any(|row| matches!(
                        row,
                        PairRow::Text { text, style: PairStyle::Base } if text == url
                    )) || rows
                        .iter()
                        .filter(|row| matches!(
                            row,
                            PairRow::Text { text, style: PairStyle::Base } if !text.is_empty()
                        ))
                        .count()
                        >= 2,
                    "width {width}: an over-width URL occupies continuation rows"
                );
            }
            // Every row still fits the width (QR excluded: it is
            // pre-rendered at its intrinsic 31 cols, ≥ every test
            // width here).
            for row in &rows {
                if let PairRow::Text { text, .. } = row {
                    assert!(
                        crate::wrap::display_width(text) <= width,
                        "width {width}: row fits: {text:?}"
                    );
                }
            }
        }
    }

    /// The specific old-bug shape: at 53/54 columns the URL (47
    /// chars) used to be truncated to 53 cols by `take_width`; ref
    /// wraps it instead (the port's URL is 47 cols so widths ≥ 47
    /// hold it on one row).
    #[test]
    fn url_holds_whole_at_its_own_width() {
        let rows = rows(&loaded(), URL.len());
        assert!(
            rows.iter().any(|row| matches!(
                row,
                PairRow::Text { text, style: PairStyle::Base } if text == URL
            )),
            "one whole-URL row at width {}",
            URL.len()
        );
    }
}
