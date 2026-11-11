"""Shared helpers for census scripts (tools/census/*).

Every census script follows the http_surface.py pattern:
  - dual-side extraction from authoritative sources (self-report first,
    robust static extraction second, runtime cross-check third)
  - normalization rules stated in the report
  - markdown report with per-item file:line evidence
  - coverage number = adjudicated/total; gap candidates listed, exit 0

Import this module (scripts run from repo root: python3 tools/census/<x>.py).
"""
from __future__ import annotations

import re
from pathlib import Path

VERBS = ("get", "post", "put", "delete", "patch")


# ------------------------------------------------------- rust source utils
def _skip_noncode(src: str, start: int) -> int:
    """Advance past string literals, char literals and comments from start."""
    i, n = start, len(src)
    while i < n:
        c = src[i]
        if c == '"':
            i += 1
            while i < n and src[i] != '"':
                i += 2 if src[i] == "\\" else 1
            i += 1
        elif c == "'":
            i += 1
            while i < n and src[i] != "'":
                i += 2 if src[i] == "\\" else 1
            i += 1
        elif src.startswith("//", i):
            nl = src.find("\n", i)
            i = n if nl == -1 else nl
        elif src.startswith("/*", i):
            end = src.find("*/", i)
            i = n if end == -1 else end + 2
        else:
            return i
    return n


def _match_brace(src: str, open_idx: int) -> int:
    """Index of the brace matching src[open_idx]=='{' (literal-safe), or -1."""
    depth, i, n = 0, open_idx, len(src)
    while i < n:
        i = _skip_noncode(src, i)
        if i >= n:
            break
        c = src[i]
        if c == "{":
            depth += 1
        elif c == "}":
            depth -= 1
            if depth == 0:
                return i
        i += 1
    return -1


def strip_tests(src: str) -> str:
    """Blank out `#[cfg(test)]` items and `#[test]`/`#[tokio::test]` fns."""
    out = list(src)
    for pat in (r"#\[cfg\(test\)\]", r"#\[tokio::test[^\]]*\]", r"#\[test\]"):
        for m in re.finditer(pat, src):
            head = src[m.end():]
            head_ns = _skip_noncode(head, 0)
            brace = head.find("{", head_ns) if head_ns < len(head) else -1
            stop = len(head)
            for delim in (";", "#["):
                d = head.find(delim, head_ns)
                if d != -1:
                    stop = min(stop, d)
            if brace != -1 and brace < stop:
                close = _match_brace(head, brace)
                if close != -1:
                    for j in range(m.start(), min(m.end() + close + 1, len(out))):
                        out[j] = " "
                    continue
            nl = head.find("\n")
            if nl != -1:
                for j in range(m.start(), min(m.end() + nl, len(out))):
                    out[j] = " "
    return "".join(out)


def rust_path_literals(src_dir: Path, pattern: str, *, skip_gen: bool = True) -> list[tuple[str, str, int]]:
    """All regex matches in production rust sources: (match, file, line)."""
    hits = []
    for f in sorted(src_dir.rglob("*.rs")):
        if skip_gen and f.name.endswith(".gen.rs"):
            continue
        body = strip_tests(f.read_text())
        for m in re.finditer(pattern, body, re.MULTILINE):
            line = body[: m.start()].count("\n") + 1
            hits.append((m.group(0), str(f), line))
    return hits


# ------------------------------------------------------- path canonicalization
def canon_ref(path: str) -> str:
    """openapi `{param}` -> `{}`; keep literal `*` suffix."""
    return re.sub(r"\{[^}]+\}", "{}", path)


def canon_axum(path: str) -> str:
    """axum `:param` / `*wild` -> canonical; `*name` tail -> `*`."""
    p = re.sub(r":\w+", "{}", path)
    return re.sub(r"\*\w+", "*", p)


# ------------------------------------------------------- report scaffold
class Report:
    def __init__(self, title: str, subtitle: str = ""):
        self.lines: list[str] = [f"# {title}", ""]
        if subtitle:
            self.lines += [f"> {subtitle}", ""]

    def w(self, text: str = "") -> None:
        self.lines.append(text)

    def write(self, path: str | None) -> str:
        text = "\n".join(self.lines) + "\n"
        if path:
            out = Path(path)
            out.parent.mkdir(parents=True, exist_ok=True)
            out.write_text(text)
            print(f"wrote {out}")
        print(text)
        return text
