//! Port of `cli/src/run/ui.ts` — the run command's rendering channel.
//! `println`/`empty`/`error` all write to **stderr** (v2 semantics); only
//! assistant text and JSON events go to stdout.
//!
//! Faithful quirk: `empty()`'s `blank` latch is never reset in v2, so at
//! most ONE blank line is ever printed per process.

use std::io::Write as _;

use crate::util;

pub mod style {
    pub const TEXT_DIM: &str = "\x1b[90m";
    pub const TEXT_NORMAL: &str = "\x1b[0m";
    pub const TEXT_WARNING_BOLD: &str = "\x1b[93m\x1b[1m";
    pub const TEXT_DANGER_BOLD: &str = "\x1b[91m\x1b[1m";
}

/// `UI.println`: space-joined message + EOL on stderr.
pub fn println(parts: &[&str]) {
    let mut stderr = std::io::stderr().lock();
    let _ = writeln!(stderr, "{}", parts.join(" "));
}

thread_local! {
    static BLANK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// `UI.empty`: one blank line, collapsing runs (`blank` module state).
pub fn empty() {
    if BLANK.with(std::cell::Cell::get) {
        return;
    }
    println(&[style::TEXT_NORMAL]);
    BLANK.with(|blank| blank.set(true));
}

/// `UI.error`: strips a leading `Error: ` before re-printing in danger bold.
pub fn error(message: &str) {
    let message = util::error_message(message);
    println(&[
        format!("{}Error: {}", style::TEXT_DANGER_BOLD, style::TEXT_NORMAL).as_str(),
        message.as_str(),
    ]);
}
