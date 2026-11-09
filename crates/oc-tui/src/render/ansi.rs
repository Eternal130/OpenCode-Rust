//! Canonical ANSI serialization of a [`Frame`]: identical bytes to the
//! golden exporter and `parity::tui_frame::render_ansi` (cross-checked by
//! tests). One line per row; a single full-state SGR
//! `ESC[0;attrs;fg;bgm` on every style change; `ESC[0m` at the end of a
//! non-default-styled line.

use super::{Cell, CellColor, Frame, CONTINUATION};
use std::fmt::Write as _; // format_push_string: write! into the existing String

/// Serialize a frame to the canonical golden byte format.
#[must_use]
pub fn serialize_canonical(frame: &Frame) -> String {
    let mut out = String::new();
    for line in &frame.cells {
        let mut current = (0u16, CellColor::Default, CellColor::Default);
        let mut serialized = String::new();
        for cell in line {
            if cell.ch == CONTINUATION {
                // Wide-cluster continuation cells emit nothing (the
                // exporter walks cells; empty symbols vanish).
                continue;
            }
            let style = (cell.attrs, cell.fg, cell.bg);
            if style != current {
                serialized.push_str(&sgr(cell));
                current = style;
            }
            serialized.push(cell.ch);
        }
        if current != (0, CellColor::Default, CellColor::Default) {
            serialized.push_str("\x1b[0m");
        }
        out.push_str(&serialized);
        out.push('\n');
    }
    out
}

fn sgr(cell: &Cell) -> String {
    let mut params: Vec<String> = vec!["0".into()];
    if cell.attrs & (1 << 0) != 0 {
        params.push("1".into());
    }
    if cell.attrs & (1 << 1) != 0 {
        params.push("2".into());
    }
    if cell.attrs & (1 << 2) != 0 {
        params.push("3".into());
    }
    if cell.attrs & (1 << 3) != 0 {
        params.push("4".into());
    }
    if cell.attrs & (1 << 4) != 0 {
        params.push("5".into());
    }
    if cell.attrs & (1 << 5) != 0 {
        params.push("7".into());
    }
    if cell.attrs & (1 << 6) != 0 {
        params.push("8".into());
    }
    if cell.attrs & (1 << 7) != 0 {
        params.push("9".into());
    }
    params.extend(color_params(cell.fg, 30));
    params.extend(color_params(cell.bg, 40));
    format!("\x1b[{}m", params.join(";"))
}

fn color_params(color: CellColor, base: u8) -> Vec<String> {
    match color {
        CellColor::Default => vec![(base + 9).to_string()],
        CellColor::Indexed(index) => vec![(base + 8).to_string(), "5".into(), index.to_string()],
        CellColor::Rgb(r, g, b) => {
            vec![(base + 8).to_string(), "2".into(), r.to_string(), g.to_string(), b.to_string()]
        }
    }
}

// -- SGR emission helpers (lane B3b, relocated by D4.2) --------------------
//
// These were the GridEmitter's serialization helpers; the mini
// scrollback transcript (mini/lifecycle.rs serialize_atoms_row) and
// the terminal pane's attr mask still consume them, so they survive
// the GridEmitter's retirement HERE (D4.1c deviation 1's standing
// exception — the committed mini transcript bytes depend on them).
// Byte shapes are opentui-core ansi.rs's (emitColor /
// TextAttributes.apply_attributes), unchanged.

/// ECMA-48 attribute bits — the exact packing of opentui-core
/// `ansi.rs` `text_attributes` (bit 0 = param 1 … bit 7 = param 9).
pub mod text_attributes {
    pub const BOLD: u16 = 1 << 0;
    pub const DIM: u16 = 1 << 1;
    pub const ITALIC: u16 = 1 << 2;
    pub const UNDERLINE: u16 = 1 << 3;
    pub const BLINK: u16 = 1 << 4;
    pub const INVERSE: u16 = 1 << 5;
    pub const HIDDEN: u16 = 1 << 6;
    pub const STRIKETHROUGH: u16 = 1 << 7;
}

/// SGR foreground — opentui-core `ansi.rs` `emit_color` (:114-144)
/// specialized to the oc-tui color vocabulary.
pub fn emit_fg(buf: &mut String, color: CellColor) {
    match color {
        CellColor::Default => buf.push_str("\x1b[39m"),
        CellColor::Indexed(index) => write!(buf, "\x1b[38;5;{index}m").unwrap_or(()),
        CellColor::Rgb(r, g, b) => write!(buf, "\x1b[38;2;{r};{g};{b}m").unwrap_or(()),
    }
}

/// SGR background — as [`emit_fg`], on the 4x plane.
pub fn emit_bg(buf: &mut String, color: CellColor) {
    match color {
        CellColor::Default => buf.push_str("\x1b[49m"),
        CellColor::Indexed(index) => write!(buf, "\x1b[48;5;{index}m").unwrap_or(()),
        CellColor::Rgb(r, g, b) => write!(buf, "\x1b[48;2;{r};{g};{b}m").unwrap_or(()),
    }
}

/// The attribute SGR burst — opentui-core `ansi.rs`
/// `TextAttributes::apply_attributes` (:76-103): fixed order, one SGR
/// per set bit.
pub fn apply_attributes(buf: &mut String, attrs: u16) {
    if attrs & text_attributes::BOLD != 0 {
        buf.push_str("\x1b[1m");
    }
    if attrs & text_attributes::DIM != 0 {
        buf.push_str("\x1b[2m");
    }
    if attrs & text_attributes::ITALIC != 0 {
        buf.push_str("\x1b[3m");
    }
    if attrs & text_attributes::UNDERLINE != 0 {
        buf.push_str("\x1b[4m");
    }
    if attrs & text_attributes::BLINK != 0 {
        buf.push_str("\x1b[5m");
    }
    if attrs & text_attributes::INVERSE != 0 {
        buf.push_str("\x1b[7m");
    }
    if attrs & text_attributes::HIDDEN != 0 {
        buf.push_str("\x1b[8m");
    }
    if attrs & text_attributes::STRIKETHROUGH != 0 {
        buf.push_str("\x1b[9m");
    }
}
