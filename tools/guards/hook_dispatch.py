#!/usr/bin/env python3
''''exec python3 -- "$0" "$@" # '''
r"""Guard `hook_dispatch` (§9.1): registered hook kind, no production dispatch.

WHAT IT CHECKS
  Every hook kind declared in the `oc_hook_points!` macro table
  (`crates/oc-core/src/hooks.rs`) must have its generated
  `dispatch_<kind>(` function called from at least one PRODUCTION file
  (i.e. outside `hooks.rs` itself and outside test files). A violation is
  a registered-but-never-dispatched hook kind: plugins can subscribe to
  an event the engine never fires.

HEURISTIC
  * Hook-kind table: lines of the macro invocation matching
    `<kind>, register_<kind>, dispatch_<kind>, has_<kind>,`.
  * Dispatch sites: `\bdispatch_<kind>\s*\(` searched across every
    non-test `.rs` file under `crates/` except `crates/oc-core/src/hooks.rs`
    (the generator/tests live there).
  * `crates/oc-plugin-host` registers plugins and dispatches generically
    through `Bridge::dispatch_hook` / the supervisor's `dispatch_pump`
    (engine-driven RPC), which is why per-kind call sites are the oc-core
    surface this guard measures. Adding a host-side generic dispatcher is
    not a per-kind dispatch and does not count.

KNOWN LIMITATIONS
  * File-level test exclusion only: a dispatch inside a `#[cfg(test)]`
    module of a production file counts as production. Under-reporting
    only — it cannot produce a false NEW violation.
  * A dispatch site that exists but sits behind a dead flag is invisible.

UPDATE BASELINE (`--update-baseline`) only when a hook kind is ported
(making it live) or deliberately parked with a ledger entry; justify in
the commit message (README § Updating).
"""

import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import guardlib  # noqa: E402

HOOKS_RS = guardlib.ROOT / "crates" / "oc-core" / "src" / "hooks.rs"
MACRO_RE = re.compile(r"oc_hook_points!\s*\{(.*?)\n\}", re.S)
KIND_RE = re.compile(r"^\s*(\w+),\s*register_\w+,\s*dispatch_\w+,\s*has_\w+,", re.M)


def collect() -> tuple[list[dict], dict]:
    if not HOOKS_RS.is_file():
        raise guardlib.GuardError(f"hook registry missing: {HOOKS_RS}")
    macro = MACRO_RE.search(guardlib.read_text(HOOKS_RS))
    if macro is None:
        raise guardlib.GuardError("oc_hook_points! invocation not found in hooks.rs")
    kinds = KIND_RE.findall(macro.group(1))
    if not kinds:
        raise guardlib.GuardError("no hook kinds parsed — parser broken?")

    corpus: list[tuple[str, str]] = []
    for rel, path in guardlib.rust_files(guardlib.ROOT / "crates"):
        if rel == "crates/oc-core/src/hooks.rs" or guardlib.is_test_path(rel):
            continue
        corpus.append((rel, guardlib.read_text(path)))

    violations: list[dict] = []
    live: list[str] = []
    for kind in kinds:
        pattern = re.compile(r"\bdispatch_" + re.escape(kind) + r"\s*\(")
        callers = [rel for rel, text in corpus if pattern.search(text)]
        if callers:
            live.append(kind)
        else:
            violations.append({
                "id": f"hook:{kind}",
                "detail": "hook kind registered in oc_hook_points! but dispatch_<kind>() "
                          "has no production call site (test-only callers don't count)",
            })

    meta = {"hook_kinds": len(kinds), "live": len(live)}
    return violations, meta


if __name__ == "__main__":
    sys.exit(guardlib.run_guard("hook_dispatch", collect, __doc__))
