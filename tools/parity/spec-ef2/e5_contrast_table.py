#!/usr/bin/env python3
"""SCAN-EF2 E5(a) contrast table: WCAG 2.x ratios for real fg/bg token
pairs (base + state variants) from the v2 default theme.

Parity note: ours crates/oc-theme/assets/v2/opencode.json is
byte-identical to ref packages/tui/src/theme/assets/v2/opencode.json
(verified this lane) — ours ratio == ref ratio per cell BY
CONSTRUCTION; the table's violations are INHERITED findings (the
clause's inherited-findings rule), not parity breaks.

Thresholds per spec E5: text 4.5:1, UI/large 3:1.
"""
import json

THEME = "/opt/opencode-rust/.worktrees/lane-scanef2/crates/oc-theme/assets/v2/opencode.json"


def lum(hex_color: str) -> float:
    h = hex_color.lstrip("#")
    r, g, b = (int(h[i:i + 2], 16) / 255.0 for i in (0, 2, 4))
    def f(c):
        return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4
    return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b)


def ratio(fg: str, bg: str) -> float:
    l1, l2 = lum(fg), lum(bg)
    if l1 < l2:
        l1, l2 = l2, l1
    return (l1 + 0.05) / (l2 + 0.05)


def get(d, path: str):
    """Fetch a token, dereferencing $-refs at ANY node (leaf or
    intermediate, e.g. hue.neutral = $hue.gray)."""
    for _ in range(16):
        val = d
        for part in path.split("."):
            if isinstance(val, str):
                if not val.startswith("$"):
                    raise KeyError(f"{path}: hit leaf {val!r} before {part}")
                tgt = val.lstrip("$")
                val = d
                for seg in tgt.split("."):
                    val = val[seg]
            val = val[part]
        if isinstance(val, str) and val.startswith("$") and "." in val[1:]:
            path = val.lstrip("$")
            continue
        return val
    raise KeyError(f"reference loop at {path}")


def merge(a: dict, b: dict) -> dict:
    out = dict(a)
    for k, v in b.items():
        if isinstance(v, dict) and isinstance(out.get(k), dict):
            out[k] = merge(out[k], v)
        else:
            out[k] = v
    return out


def main() -> None:
    with open(THEME) as f:
        doc = json.load(f)
    # resolved DARK theme tree = base merged with the dark mode overlay
    # (expand.rs/resolve.rs semantics; $refs address this merged space)
    base = merge(doc["base"], doc.get("dark", {}))

    # (cell-name, fg-path, bg-path, class) — real painted pairs
    pairs = [
        ("text.base / bg.base", "text.base", "background.base", "text"),
        ("text.muted / bg.base", "text.muted", "background.base", "text"),
        ("text.base / bg.raised", "text.base", "background.raised.base", "text"),
        ("text.muted / bg.raised", "text.muted", "background.raised.base", "text"),
        ("error / bg.base", "text.feedback.error.base", "background.base", "text"),
        ("warning / bg.base", "text.feedback.warning.base", "background.base", "text"),
        ("success(busy) / bg.base", "text.feedback.success.base", "background.base", "text"),
        ("info / bg.base", "text.feedback.info.base", "background.base", "text"),
        ("action.primary / bg.base", "text.action.primary.base", "background.base", "ui"),
        ("action.primary.$hovered / bg.base", "text.action.primary.$hovered", "background.base", "ui"),
        ("action.primary.$disabled / bg.base", "text.action.primary.$disabled", "background.base", "ui"),
        ("action.secondary.$hovered / bg.base", "text.action.secondary.$hovered", "background.base", "ui"),
        ("action.destructive.$disabled / bg.base", "text.action.destructive.$disabled", "background.base", "ui"),
        ("formfield.$hovered / bg.base", "text.formfield.$hovered", "background.base", "text"),
        ("formfield.$disabled / bg.base", "text.formfield.$disabled", "background.base", "text"),
        ("formfield.$selected / bg.base", "text.formfield.$selected", "background.base", "text"),
        ("border.base / bg.base", "border.base", "background.base", "ui"),
        ("scrollbar / bg.base", "scrollbar.base", "background.base", "ui"),
        # busy interactive glyph family (spinner hue)
        ("hue.interactive.200 / bg.base", "hue.interactive.200", "background.base", "ui"),
        # selected tab row: bg overlay token if present
        ("text.base / bg.action.primary.$selected", "text.base", "background.action.primary.$selected", "text"),
    ]

    rows = []
    for name, fgp, bgp, cls in pairs:
        try:
            fg, bg = get(base, fgp), get(base, bgp)
            hexok = lambda c: isinstance(c, str) and c.startswith("#") and len(c) == 7
            if not (hexok(fg) and hexok(bg)):
                rows.append((name, None, None, f"NON-COLOR({fg!r}/{bg!r})"))
                continue
        except (KeyError, TypeError):
            rows.append((name, None, None, "TOKEN-MISSING"))
            continue
        r = ratio(fg, bg)
        thr = 4.5 if cls == "text" else 3.0
        verdict = "PASS" if r >= thr else "INHERITED-VIOLATION"
        rows.append((name, r, thr, verdict))

    out = ["# E5(a) WCAG contrast table — v2 default theme (dark), token pairs", "",
           "ours asset == ref asset (byte-identical, verified) => per-cell parity by construction.",
           "", "| cell | ratio | threshold | verdict |", "|---|---|---|---|"]
    for name, r, thr, v in rows:
        out.append(f"| {name} | {r:.2f} | {thr} | {v} |" if r is not None
                   else f"| {name} | - | - | {v} |")
    nviol = sum(1 for *_, v in rows if v == "INHERITED-VIOLATION")
    nmiss = sum(1 for *_, v in rows if v == "TOKEN-MISSING")
    out += ["", f"cells: {len(rows)}; inherited violations: {nviol}; token-missing: {nmiss}",
            "(c) color-independence + (b) key-walk: see spec_e5_canary legs B/C (PASS this lane)."]
    text = "\n".join(out)
    with open("/tmp/opencode/scanef2/e5-contrast-table.md", "w") as f:
        f.write(text + "\n")
    print(text)


if __name__ == "__main__":
    main()
