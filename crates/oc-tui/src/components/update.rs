//! The update-notification surfaces (auditB #10 — v2's
//! `context/update-notification.tsx` + `routes/home.tsx:125-175` +
//! `component/dialog-update.tsx`), lane TAIL.
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

/// The crash-fallback screen (auditB #9 — v2's `ErrorComponent`):
/// the frame the run loop paints when the render path itself failed
/// (a caught panic — the terminal stays up, the user reads the error
/// and exits). No store access: a broken store is the crash cause.
#[must_use]
pub fn error_component_frame(size: (u16, u16), message: &str) -> Frame {
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
    put(1, "Something went wrong");
    let detail = crate::wrap::take_width(message, width.saturating_sub(4).max(8));
    put(3, &detail);
    put(height.saturating_sub(2), "Press escape to exit.");
    Frame { cells, width, height }
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
        assert!(grid[1].contains("Something went wrong"), "{grid:?}");
        assert!(grid[3].contains("index outside"), "{grid:?}");
        assert!(grid[6].contains("Press escape"), "{grid:?}");
        let tiny = error_component_frame((10, 6), "way too long detail message to fit");
        assert_eq!(tiny.text_grid().len(), 6);
    }
}
