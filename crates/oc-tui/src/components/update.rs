//! The update-notification surfaces (auditB #10 — v2's
//! `context/update-notification.tsx` + `routes/home.tsx:125-175` +
//! `component/dialog-update.tsx`), lane TAIL.
//!
//! **Ownership (lane D3-6).** Surface model, already on the shared
//! tree: the home banner's z slot is painted by
//! [`crate::components::home_solid`] (it consumes [`UpdateNotice`]
//! and [`crate::components::masked_text`]) and `DialogUpdate` by the
//! dialog family ([`crate::components::dialog`] /
//! [`crate::components::command_palette`]), so nothing here mounts.
//! The one [`Frame`] producer is [`error_component_frame`] — the
//! render-crash fallback, deliberately NOT a tree surface (a broken
//! renderer must still paint its last screen). It is a reasoned
//! retention for D4, not a GridEmitter blocker.
//!
//! **K-class licensed exception (lane D4.2, ADR-0019)**:
//! [`error_component_frame`] is the ONE licensed `Frame` producer
//! after the emission flip — the render-crash fallback (update.rs
//! → app.rs's `error_mode` arms) paints a bare grid because a
//! broken render path must not depend on the tree/composite seam it
//! crashed in. It keeps the `Frame` shape on purpose; see ADR-0019
//! for the final registry entry.
//!
//! **Sources** (v2's layered `UpdateSource`): the SSE
//! `installation.update-available`/`installation.updated` events (the
//! server-hosting process's updater, `autoupdate.rs`'s
//! `InstallationNotice` bus) and the CLI checker's NOTICE FILE
//! (`<state>/update-notice.log` — the TUI-mode channel oc-cli's
//! detached checker appends to; polled at boot + on config ticks).
//!
//! **Surfaces**: the home banner (one centered row between the logo
//! and the prompt — `/update to install v{…}` or `/exit restart to
//! use v{…}`, click → the dialog; the remote-server variant reads
//! "remote server update available" and click dismisses) and
//! `DialogUpdate` (`/update`): checking / current / available /
//! installing / installed / failed rows with Skip + Update/Restart
//! actions. Dismissed versions persist (`update-notifications`, the
//! storage key v2's `useStorage().store` owns).
//!
//! The INSTALL arm shells out to the CLI's own `upgrade` command
//! (stdio appended to the notice file — completion lands through the
//! same poll); no in-TUI network code exists on this side.

use crate::render::{Cell, CellColor, Frame};

/// One banner notice (v2's `Notice`: type + version + source).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateNotice {
    pub kind: UpdateNoticeKind,
    pub version: String,
    /// `client` (the CLI checker / the notice file) or `server`
    /// (the SSE event; `remote` = a remotely-connected server).
    pub remote: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateNoticeKind {
    Available,
    Installed,
}

/// The dialog's live state (`UpdateState` + the check outcome).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum UpdateDialogState {
    /// Nothing known yet — a fresh dialog starts a check.
    #[default]
    Current,
    Checking,
    Available(String),
    Installing(String),
    Installed(String),
    Failed(String),
    /// No updater is wired (the default build's seam).
    Unavailable(String),
}

impl UpdateNotice {
    /// TUI-TD27 — the banner's hide gate input (`type === "installed"`).
    #[must_use]
    pub const fn is_installed(&self) -> bool {
        matches!(self.kind, UpdateNoticeKind::Installed)
    }

    /// The banner's `/…` prefix (home.tsx:160-163).
    #[must_use]
    pub fn prefix(&self) -> &'static str {
        match self.kind {
            UpdateNoticeKind::Installed => "/exit",
            UpdateNoticeKind::Available => "/update",
        }
    }

    /// The banner's body text after the prefix.
    #[must_use]
    pub fn body(&self) -> String {
        if self.remote {
            return "remote server update available".to_owned();
        }
        match self.kind {
            UpdateNoticeKind::Installed => format!(" restart to use v{}", self.version),
            UpdateNoticeKind::Available => format!(" to install v{}", self.version),
        }
    }

    /// Parse one notice-file line (oc-cli autoupdate's TUI channel):
    /// `… update available: v{new} (current v{old}); …` /
    /// `… update installed (v{new}), restart to apply`.
    #[must_use]
    pub fn parse_notice_line(line: &str) -> Option<Self> {
        if let Some(at) = line.find("update available: v") {
            let rest = &line[at + "update available: v".len()..];
            let version = rest.split(' ').next()?.split(';').next()?.to_owned();
            return Some(Self { kind: UpdateNoticeKind::Available, version, remote: false });
        }
        if let Some(at) = line.find("update installed (v") {
            let rest = &line[at + "update installed (v".len()..];
            let version = rest.split(')').next()?.to_owned();
            return Some(Self { kind: UpdateNoticeKind::Installed, version, remote: false });
        }
        None
    }

    /// The last parseable notice in the file (the newest signal wins).
    #[must_use]
    pub fn from_notice_file(text: &str) -> Option<Self> {
        text.lines().rev().find_map(Self::parse_notice_line)
    }
}

/// The notice-file's path (oc-cli `autoupdate::notice_file`'s
/// contract: `<state>/update-notice.log`).
#[must_use]
pub fn notice_file_path() -> std::path::PathBuf {
    oc_util::global_roots::resolve().state.join("update-notice.log")
}

/// The dialog's body rows for one state (dialog-update.tsx's
/// Switch): the title line + the detail line.
#[must_use]
pub fn dialog_rows(state: &UpdateDialogState) -> (String, String) {
    match state {
        UpdateDialogState::Checking => ("Checking for updates…".to_owned(), " ".to_owned()),
        UpdateDialogState::Current => ("opencode is up to date".to_owned(), " ".to_owned()),
        UpdateDialogState::Available(version) => {
            ("Update available".to_owned(), format!("v{version} will be installed on confirm"))
        }
        UpdateDialogState::Installing(version) => {
            ("Installing update…".to_owned(), format!("v{version} is being installed"))
        }
        UpdateDialogState::Installed(version) => {
            ("Update installed".to_owned(), format!("restart to use v{version}"))
        }
        UpdateDialogState::Failed(message) => ("Update failed".to_owned(), message.clone()),
        UpdateDialogState::Unavailable(message) => {
            ("Updates unavailable".to_owned(), message.clone())
        }
    }
}

/// The dialog's confirm action label (`None` = Skip only).
#[must_use]
pub fn dialog_confirm(state: &UpdateDialogState) -> Option<&'static str> {
    match state {
        UpdateDialogState::Available(_) => Some("Update"),
        UpdateDialogState::Installed(_) => Some("Restart"),
        _ => None,
    }
}

/// G-287② — the dialog's INITIAL focus (`createEffect(()
/// => setActive(Math.max(0, buttons().length - 1))`,
/// dialog-update.tsx:65): the cursor rests on the CONFIRM button when
/// one exists (Update/Restart), else Skip — the port's row model
/// puts the state row first, so the confirm is the LAST row.
#[must_use]
pub fn dialog_initial_selection(state: &UpdateDialogState) -> usize {
    match dialog_confirm(state) {
        Some(_) => 2, // [state row, Skip, confirm]
        None => 1,    // [state row, Skip]
    }
}

/// The crash-fallback screen (auditB #9 — v2's `ErrorComponent`):
/// the frame the run loop paints when the render path itself failed
/// (a caught panic — the terminal stays up, the user reads the error
/// and exits). No store access: a broken store is the crash cause.
/// G-287⑤ — the ref's structure (error-component.tsx:27-283): the
/// "OpenCode crashed" headline, the error panel, the horizontal
/// action row (c Copy report / r Restart / q Quit — the port's crash
/// loop only wires q/escape to exit; the labels still document the
/// ref's model), the stack section, and the version footer.
#[must_use]
pub fn error_component_frame(size: (u16, u16), message: &str) -> Frame {
    error_component_frame_with(size, message, None, "")
}

/// G-287⑤ — [`error_component_frame`] with the stack + version the
/// ref renders (`{stack || "No stack trace available."}` and
/// `OpenCode {version}`, error-component.tsx:169-172/277-279).
#[must_use]
pub fn error_component_frame_with(
    size: (u16, u16),
    message: &str,
    stack: Option<&str>,
    version: &str,
) -> Frame {
    let (width, height) = (usize::from(size.0), usize::from(size.1));
    let mut cells: Vec<Vec<Cell>> = vec![vec![Cell::default(); width.max(1)]; height.max(1)];
    let mut put = |row: usize, text: &str| {
        if let Some(line) = cells.get_mut(row) {
            for (index, ch) in text.chars().enumerate().take(width.saturating_sub(3).max(1)) {
                if let Some(cell) = line.get_mut(2 + index) {
                    cell.ch = ch;
                    cell.fg = CellColor::Default;
                }
            }
        }
    };
    put(1, "OpenCode crashed");
    // The subtext row renders when the screen is tall enough
    // (`showSubtext = height >= 18`).
    if height >= 18 {
        put(2, "An unexpected error stopped the session.");
    }
    let detail = crate::wrap::take_width(message, width.saturating_sub(4).max(8));
    put(4, &detail);
    // The actions row (the ref's horizontal buttons, focused Copy
    // report first): the key letters mirror the ref's labels.
    put(6, "c Copy report   r Restart   q Quit");
    // The stack section: the first lines that fit above the footer
    // (the ref scrollboxes the trace; the crash frame is static — a
    // bounded deviation, the panic path paints once).
    let stack_text = stack.filter(|text| !text.is_empty()).unwrap_or("No stack trace available.");
    put(8, "Stack trace");
    for (index, line) in stack_text.lines().take(height.saturating_sub(12)).enumerate() {
        put(9 + index, &crate::wrap::take_width(line, width.saturating_sub(4).max(8)));
    }
    if !version.is_empty() && height >= 20 {
        put(
            height.saturating_sub(3),
            &crate::wrap::take_width(
                &format!("Copy the report and open a GitHub issue to help us fix this. OpenCode {version}"),
                width.saturating_sub(4).max(8),
            ),
        );
    }
    put(height.saturating_sub(2), "Press escape to exit.");
    Frame { cells, width, height, clusters: None }
}

/// G-287⑤ — `buildIssueURL` (error-component.tsx:285-322): the
/// pre-filled bug-report issue URL (title/version + the description
/// carrying the error + stack). The binary search trims the stack
/// until the ENCODED URL fits 6000 chars — the port measures
/// percent-encoded lengths (the reserved set escaped).
#[must_use]
pub fn build_issue_report(message: &str, stack: &str, version: &str) -> String {
    const MAX_URL_LENGTH: usize = 6000;
    const MARKER: &str = "\n… (truncated)";
    let encode = |text: &str| {
        // Minimal form-encoding: the URL-unsafe reserved set + the
        // query delimiters.
        let mut out = String::new();
        for byte in text.bytes() {
            match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    out.push(byte as char);
                }
                b' ' => out.push_str("%20"),
                _ => out.push_str(&format!("%{byte:02X}")),
            }
        }
        out
    };
    let prefix = format!(
        "https://github.com/anomalyco/opencode/issues/new?template=bug-report.yml&title={}&opencode-version={}",
        encode(&format!("TUI crash: {message}")),
        encode(version)
    );
    let body_of = |stack_text: &str| {
        format!(
            "The OpenCode TUI crashed with an unexpected error.\n\n**Error:** {message}\n\n**Stack trace:**\n```\n{stack_text}\n```"
        )
    };
    let url_len = |stack_text: &str| prefix.len() + "&description=".len() + encode(&body_of(stack_text)).len();
    let mut stack_text = stack.to_owned();
    if url_len(&stack_text) > MAX_URL_LENGTH {
        // Largest char-safe stack prefix whose encoded URL (with the
        // marker) still fits (the ref's binary search,
        // error-component.tsx:312-320).
        let cut_points: Vec<usize> = std::iter::once(0)
            .chain(stack.char_indices().map(|(at, _)| at))
            .collect();
        let mut lo = 0usize;
        let mut hi = cut_points.len() - 1;
        while lo < hi {
            let mid = (lo + hi + 1) / 2;
            let cut = format!("{}{MARKER}", &stack[..cut_points[mid]]);
            if url_len(&cut) <= MAX_URL_LENGTH {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        stack_text = format!("{}{MARKER}", &stack[..cut_points[lo]]);
    }
    format!("{prefix}&description={}", encode(&body_of(&stack_text)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The notice-file parser: the CLI checker's two line shapes +
    /// garbage.
    #[test]
    fn notice_file_lines_parse() {
        let available = "opencode-rust: update available: v2.0.25.beta.1 (current v2.0.24.beta.1); run 'opencode-rust upgrade'";
        assert_eq!(
            UpdateNotice::parse_notice_line(available),
            Some(UpdateNotice {
                kind: UpdateNoticeKind::Available,
                version: "2.0.25.beta.1".to_owned(),
                remote: false,
            })
        );
        let installed = "opencode-rust: update installed (v2.0.25.beta.1), restart to apply";
        assert_eq!(
            UpdateNotice::parse_notice_line(installed),
            Some(UpdateNotice {
                kind: UpdateNoticeKind::Installed,
                version: "2.0.25.beta.1".to_owned(),
                remote: false,
            })
        );
        assert_eq!(UpdateNotice::parse_notice_line("unrelated log line"), None);
        // The file reader takes the LAST parseable line.
        let file =
            "noise\nupdate available: v2.0.25.beta.1 (current v2.0.24.beta.1); x\nmore noise\n";
        assert_eq!(
            UpdateNotice::from_notice_file(file).map(|n| n.version),
            Some("2.0.25.beta.1".to_owned())
        );
    }

    /// The banner's copy (home.tsx:153-170): the prefix + body
    /// variants.
    #[test]
    fn banner_copy_matches_the_reference() {
        let available = UpdateNotice {
            kind: UpdateNoticeKind::Available,
            version: "2.0.25.beta.1".to_owned(),
            remote: false,
        };
        assert_eq!(available.prefix(), "/update");
        assert_eq!(available.body(), " to install v2.0.25.beta.1");
        let installed = UpdateNotice {
            kind: UpdateNoticeKind::Installed,
            version: "2.0.25.beta.1".to_owned(),
            remote: false,
        };
        assert_eq!(installed.prefix(), "/exit");
        assert_eq!(installed.body(), " restart to use v2.0.25.beta.1");
        let remote =
            UpdateNotice { kind: UpdateNoticeKind::Available, version: "9".into(), remote: true };
        assert_eq!(remote.body(), "remote server update available");
    }

    /// The dialog's confirm labels + rows per state.
    #[test]
    fn dialog_states_carry_actions() {
        assert_eq!(dialog_confirm(&UpdateDialogState::Available("1".into())), Some("Update"));
        assert_eq!(dialog_confirm(&UpdateDialogState::Installed("1".into())), Some("Restart"));
        assert_eq!(dialog_confirm(&UpdateDialogState::Current), None);
        let (title, _) = dialog_rows(&UpdateDialogState::Checking);
        assert_eq!(title, "Checking for updates…");
        let (title, detail) = dialog_rows(&UpdateDialogState::Failed("dns broke".into()));
        assert_eq!(title, "Update failed");
        assert_eq!(detail, "dns broke");
    }

    /// The crash fallback paints its rows at any size (tiny widths
    /// clamp, the detail truncates).
    #[test]
    fn error_component_paints_rows() {
        let frame = error_component_frame((40, 8), "index outside of buffer");
        let grid = frame.text_grid();
        assert!(grid[1].contains("OpenCode crashed"), "{grid:?}");
        assert!(grid[4].contains("index outside"), "{grid:?}");
        assert!(grid[6].contains("Press escape"), "{grid:?}");
        let tiny = error_component_frame((10, 6), "way too long detail message to fit");
        assert_eq!(tiny.text_grid().len(), 6);
    }

    /// G-287⑤ — the crash screen's ref structure: the action row, the
    /// stack section, the version footer.
    #[test]
    fn crash_screen_carries_actions_stack_and_version() {
        let frame = error_component_frame_with(
            (80, 24),
            "index outside of buffer",
            Some("at app::frame (app.rs:100)\n   at render (render.rs:9)"),
            "2.0.24",
        );
        let grid = frame.text_grid();
        assert!(grid[1].contains("OpenCode crashed"), "{grid:?}");
        assert!(grid[2].contains("unexpected error"), "subtext at >=18 rows");
        assert!(grid[6].contains("c Copy report"), "the action row: {grid:?}");
        assert!(grid[6].contains("r Restart"));
        assert!(grid[6].contains("q Quit"));
        assert!(grid[8].contains("Stack trace"));
        assert!(grid[9].contains("at app::frame"), "the stack rows");
        assert!(
            grid[21].contains("GitHub issue") && grid[21].contains("2.0.24"),
            "the version footer: {grid:?}"
        );
        // A missing stack shows the ref's placeholder.
        let frame = error_component_frame_with((80, 24), "boom", None, "");
        assert!(frame.text_grid()[9].contains("No stack trace available."));
    }

    /// G-287⑤ — `buildIssueURL`: the report carries the error + stack
    /// and the huge-stack trim keeps the URL under 6000 chars.
    #[test]
    fn issue_report_trims_huge_stacks() {
        let url = build_issue_report("boom", "at frame (x.rs:1)", "2.0.24");
        assert!(url.starts_with("https://github.com/anomalyco/opencode/issues/new?template=bug-report.yml"));
        assert!(url.contains("TUI%20crash%3A%20boom") || url.contains("TUI%20crash"));
        assert!(url.contains("at%20frame%20"), "the stack rides the body");
        assert!(url.len() <= 6000);
        let huge = "x".repeat(20_000);
        let url = build_issue_report("boom", &huge, "2.0.24");
        assert!(url.len() <= 6000, "the trim holds: {}", url.len());
        assert!(url.contains("%E2%80%A6%20(truncated)"), "the truncation marker: …");
    }
}
