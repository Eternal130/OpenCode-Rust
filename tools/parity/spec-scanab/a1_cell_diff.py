#!/usr/bin/env python3
"""SCAN-AB lane (spec v9) — A1 cell-diff analysis.

Loads rescan-format `.styles.txt` captures (ours: /tmp/opencode/scanab/a1,
ref: /tmp/opencode/rescan-{live,time} — I9: all real-run PTY captures),
decodes the vt_harness style digests, expands indexed colors through the
xterm-256 palette, matches element cells across sides via text anchors,
and emits a token-exact per-cell verdict table (markdown).

Digest packing (vt_harness::style_digest):
  style = fg_packed << 30 | bg_packed << 4 | mods
  packed: Default=0, Idx=(1<<8)|i, Rgb=(2<<24)|r<<16|g<<8|b
"""
from __future__ import annotations

import sys
from pathlib import Path

XTERM_256 = []


def _build_palette() -> None:
    levels = [0, 95, 135, 175, 215, 255]
    for i in range(16):
        XTERM_256.append(None)  # 16 system colors: terminal-defined
    for i in range(216):
        r, rem = divmod(i, 36)
        g, b = divmod(rem, 6)
        XTERM_256.append((levels[r], levels[g], levels[b]))
    for i in range(24):
        v = 8 + i * 10
        XTERM_256.append((v, v, v))


_build_palette()


def decode(style: int) -> dict:
    fg = style >> 30
    bg = (style >> 4) & ((1 << 26) - 1)
    mods = style & 0xF
    def col(p):
        kind = (p >> 24) & 0x3
        if kind == 2:
            rgb = (p & 0xFFFFFF)
            return {"kind": "rgb", "rgb": (rgb >> 16 & 255, rgb >> 8 & 255, rgb & 255)}
        if p == 0:
            return {"kind": "default"}
        if (p >> 8) == 1:
            i = p & 0xFF
            return {"kind": "idx", "idx": i, "rgb": XTERM_256[i] if i < 256 else None}
        return {"kind": "?", "raw": hex(p)}
    return {
        "fg": col(fg),
        "bg": col(bg),
        "bold": bool(mods & 1),
        "italic": bool(mods & 2),
        "underline": bool(mods & 4),
        "inverse": bool(mods & 8),
    }


def fmt(c: dict) -> str:
    if c["kind"] == "rgb":
        return "#%02x%02x%02x" % c["rgb"]
    if c["kind"] == "idx":
        return "idx%d%s" % (c["idx"], "→#%02x%02x%02x" % c["rgb"] if c["rgb"] else "?")
    return c["kind"]


def load(styles: Path):
    cells = {}
    for line in styles.read_text().splitlines():
        coord, rest = line.split(":", 1)
        x, y = (int(v) for v in coord.split(","))
        h, ch = rest.split(" ", 1)
        cells[(x, y)] = (int(h, 16), ch)
    return cells


def rows_of(text: Path):
    return text.read_text().splitlines()


def find_row(rows, needle, start=0):
    for i in range(start, len(rows)):
        if needle in rows[i]:
            return i
    return None


def cell_of_text(rows, needle, col_offset=0):
    """(x, y) of the first cell of `needle` on its row."""
    y = find_row(rows, needle)
    if y is None:
        return None
    return (rows[y].index(needle) + col_offset, y)


def verdict(ours, ref):
    o, r = decode(ours), decode(ref)
    ofg, rfg = o["fg"].get("rgb"), r["fg"].get("rgb")
    same_rgb = ofg is not None and rfg is not None and ofg == rfg
    notes = []
    if o["fg"]["kind"] != r["fg"]["kind"]:
        notes.append("emit-kind %s vs %s" % (o["fg"]["kind"], r["fg"]["kind"]))
    for bit in ("bold", "italic", "underline", "inverse"):
        if o[bit] != r[bit]:
            notes.append(bit)
    if same_rgb and not notes:
        return "MATCH", notes
    if same_rgb:
        return "RGB-MATCH/emit-diff", notes
    return "DIVERGE", notes


def compare(label, ours_cells, ours_xy, ref_cells, ref_xy, out):
    if ours_xy is None or ref_xy is None:
        out.append("| %s | anchor-miss | | | |" % label)
        return
    a = ours_cells.get(ours_xy)
    b = ref_cells.get(ref_xy)
    if a is None or b is None:
        out.append("| %s | cell-miss ours=%s ref=%s | | | |" % (label, a is not None, b is not None))
        return
    v, notes = verdict(a[0], b[0])
    da, db = decode(a[0]), decode(b[0])
    out.append(
        "| %s | %s | `%s`%s+%s | `%s`%s+%s | %s |"
        % (
            label, v,
            a[1], fmt(da["fg"]), "B" if da["bold"] else "",
            b[1], fmt(db["fg"]), "B" if db["bold"] else "",
            "; ".join(notes) or "—",
        )
    )


def main() -> int:
    ours_dir = Path("/tmp/opencode/scanab/a1")
    ref_sess = Path("/tmp/opencode/rescan-live/session/captures")
    ref_time = Path("/tmp/opencode/rescan-time/temporal/captures")
    out = ["## A1 token-exact cell diff (ours @ lane-scanab vs ref 2.0.25 binary, both real-run PTY captures, 160x36)", ""]

    # --- spinner family (the rescan-F3 class) ---
    out.append("### Spinner cells (tab-strip busy + transcript Thinking row)")
    out.append("| cell | verdict | ours | ref | notes |")
    out.append("|---|---|---|---|---|")
    ours_cells = load(ours_dir / "ours-busy-phase-02.styles.txt")
    ref_cells = load(ref_time / "ref-busy-00.styles.txt")
    compare("tab-strip busy glyph (1,0)", ours_cells, (1, 0), ref_cells, (1, 0), out)
    # transcript thinking-row glyph: ours at (5,6) per capture; ref: find ⠙ row cell
    ref_busy_txt = rows_of(ref_time / "ref-busy-00.txt")
    ref_xy = None
    for (x, y), (s, ch) in ref_cells.items():
        if ch in "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏" and y > 2:
            ref_xy = (x, y)
            break
    compare("transcript thinking glyph", ours_cells, (5, 6), ref_cells, ref_xy, out)

    # --- settled session surface: sidebar / footer / tab strip ---
    out.append("")
    out.append("### Settled session surface (anchors matched by text)")
    out.append("| cell | verdict | ours | ref | notes |")
    out.append("|---|---|---|---|---|")
    ours_txt = rows_of(ours_dir / "ours-session-base.txt")
    ours_cells = load(ours_dir / "ours-session-base.styles.txt")
    # pick the LAST ref-session sample (most settled)
    ref_samples = sorted(ref_sess.glob("ref-session-*.txt"))
    ref_txt = rows_of(ref_samples[-1])
    ref_cells = load(ref_samples[-1].with_suffix("").with_suffix(".styles.txt"))
    for anchor, off in [("Context", 0), ("MCP", 0)]:
        oy, ry = find_row(ours_txt, anchor), find_row(ref_txt, anchor)
        if oy is None or ry is None:
            out.append("| sidebar '%s' | anchor-miss | | | |" % anchor)
            continue
        for k in range(min(4, len(anchor))):
            compare(
                "sidebar '%s'[%d]" % (anchor, k),
                ours_cells, (ours_txt[oy].index(anchor) + k, oy),
                ref_cells, (ref_txt[ry].index(anchor) + k, ry),
                out,
            )
    for token_word in ["Connected", "% used", "tokens"]:
        oy, ry = find_row(ours_txt, token_word), find_row(ref_txt, token_word)
        if oy is None or ry is None:
            out.append("| sidebar '%s' | anchor-miss (ours=%s ref=%s) | | | |" % (token_word, oy, ry))
            continue
        ox = ours_txt[oy].index(token_word)
        rx = ref_txt[ry].index(token_word)
        compare("sidebar '%s'[0]" % token_word, ours_cells, (ox, oy), ref_cells, (rx, ry), out)
    # tab strip row 0 first non-space cell
    def first_nonspace(rows):
        r = rows[0]
        for i, ch in enumerate(r):
            if ch != " ":
                return i
        return None
    compare(
        "tab strip first cell", ours_cells, (first_nonspace(ours_txt), 0),
        ref_cells, (first_nonspace(ref_txt), 0), out,
    )

    print("\n".join(out))
    Path("/tmp/opencode/scanab/a1/a1-diff-table.md").write_text("\n".join(out) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
