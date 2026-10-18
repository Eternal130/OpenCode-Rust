#!/usr/bin/env python3
r"""Guard `command_keymap` (§9.1): registered TUI command with a dead/no-op handler.

WHAT IT CHECKS
  Every command registered in the TUI keymap table
  (`crates/oc-tui-core/src/keymap/table.rs`, the `defs!` entries of
  `DEFAULT_TABLE`) must be referenced by at least one production handler —
  a quoted `"command.name"` literal in oc-tui / oc-tui-core / oc-cli
  production code outside the table itself. A violation is a registered
  command with no handler reference: registered-and-dead (palette row
  without behavior, deprecated binding, or wiring that was never ported).

HEURISTIC
  * Registrations: table lines `"bind_name" => "command.name", "defaults",
    "description", "layer"` (the `defs!` macro body).
  * Handler reference: the exact quoted literal `"command.name"`
    appearing in a non-test `.rs` file under `crates/oc-tui/src`,
    `crates/oc-tui-core/src`, or `crates/oc-cli/src`, EXCLUDING:
      - `crates/oc-tui-core/src/keymap/table.rs` (the registration
        surface itself),
      - `crates/oc-tui/src/config.rs` (the keybind-override parser — it
        mentions every command name as a config key, which is not a
        handler).
  * The dispatcher in `app.rs` compares command strings
    (`command == "app.exit"`, `Outcome::Command { .. } if command == ..`)
    — those comparisons are the handler references this guard counts.

KNOWN LIMITATIONS
  * A command whose handler exists but is a no-op body still passes
    (static reference ≠ implemented behavior).
  * Dynamic command construction (`format!("{prefix}.exit")`) is
    invisible; handler references must be string literals.
  * cfg(test) modules inside production files count as handler
    references (file-level test exclusion only) — under-reporting only.

UPDATE BASELINE (`--update-baseline`) when commands gain handlers
(shrink) or the v2 keymap backlog adds palette-only rows deliberately
(grow — with a ledger TUI entry; README § Updating).
"""
from __future__ import annotations

import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import guardlib  # noqa: E402

TABLE_RS = guardlib.ROOT / "crates" / "oc-tui-core" / "src" / "keymap" / "table.rs"
COMMAND_RE = re.compile(r"^\s*\"\w+\" => \"([\w.]+)\",", re.M)
CORPUS_ROOTS = ("crates/oc-tui/src", "crates/oc-tui-core/src", "crates/oc-cli/src")
CORPUS_EXCLUDES = {
    "crates/oc-tui-core/src/keymap/table.rs",
    "crates/oc-tui/src/config.rs",
}


def collect() -> tuple[list[dict], dict]:
    if not TABLE_RS.is_file():
        raise guardlib.GuardError(f"keymap table missing: {TABLE_RS}")
    commands = COMMAND_RE.findall(guardlib.read_text(TABLE_RS))
    if not commands:
        raise guardlib.GuardError("no commands parsed from the keymap table — parser broken?")

    corpus: list[tuple[str, str]] = []
    for root in CORPUS_ROOTS:
        base = guardlib.ROOT / root
        if not base.is_dir():
            raise guardlib.GuardError(f"corpus root missing: {base}")
        for path in sorted(base.rglob("*.rs")):
            rel = path.relative_to(guardlib.ROOT).as_posix()
            if rel in CORPUS_EXCLUDES or guardlib.is_test_path(rel):
                continue
            corpus.append((rel, guardlib.read_text(path)))

    violations: list[dict] = []
    handled: list[str] = []
    for command in commands:
        needle = f"\"{command}\""
        if any(needle in text for _, text in corpus):
            handled.append(command)
        else:
            violations.append({
                "id": f"cmd:{command}",
                "detail": "command registered in DEFAULT_TABLE but its string literal has no "
                          "handler reference outside the table/keybind-config parser",
            })

    meta = {
        "commands": len(commands),
        "handler_referenced": len(handled),
        "corpus_files": len(corpus),
    }
    return violations, meta


if __name__ == "__main__":
    sys.exit(guardlib.run_guard("command_keymap", collect, __doc__))
