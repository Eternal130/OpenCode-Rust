//! Canonical ANSI serialization of a [`Frame`]: identical bytes to the
//! golden exporter and `parity::tui_frame::render_ansi` (cross-checked by
//! tests). One line per row; a single full-state SGR
//! `ESC[0;attrs;fg;bgm` on every style change; `ESC[0m` at the end of a
//! non-default-styled line.

use super::{Cell, CellColor, Frame, CONTINUATION};

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
