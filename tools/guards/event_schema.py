#!/usr/bin/env python3
''''exec python3 -- "$0" "$@" # '''
r"""Guard `event_schema` (§9.1): published event type without schema/round-trip test.

WHAT IT CHECKS
  Every event type known to the event bus registry must be referenced by
  at least one test (schema validation or round-trip). A violation is a
  published event type that no test file mentions — new events can be
  added without any serialization contract being pinned.

HEURISTIC
  * Event-type universe (production side):
      - dotted lowercase type literals in
        `crates/oc-core/src/bus/registry.rs` (the SESSION_V1/V2/EPHEMERAL
        lists, the DEFINITIONS order table, and const definitions), plus
      - string literals passed to `EventDefinition::ephemeral("..")` /
        `EventDefinition::durable("..", ..)` across non-test `crates/`
        files (ad-hoc publishers: announce, pty, shell, forms, …).
    An event type is a string matching `[a-z][a-z0-9_-]*(\.[a-z0-9_-]+)+`
    — at least one dot, lowercase segments.
  * Test coverage: the quoted literal `"<type>"` appears in any test
    file (`**/tests/**`, `tests.rs`, `*_tests.rs`) under `crates/`.
  * Dynamic publishers that compute the type at runtime (e.g.
    `EventDefinition::ephemeral(type_)` with a variable) cannot be
    resolved statically — the registry lists are the canonical surface,
    so dynamic-only types are out of scope (documented limitation).

KNOWN LIMITATIONS
  * Coverage means "the type string appears in a test" — the guard does
    not verify the test actually round-trips a payload. It prevents the
    "new event, zero contract" regression class.
  * cfg(test) modules inside production files do NOT count as tests
    here (file-level classification); that can only over-report, and
    the baseline absorbs any such entries.

UPDATE BASELINE (`--update-baseline`) when an event gains tests (shrink)
or a legitimately new unpinned event is accepted into debt (grow — with
a ledger entry; README § Updating).
"""

import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import guardlib  # noqa: E402

REGISTRY_RS = guardlib.ROOT / "crates" / "oc-core" / "src" / "bus" / "registry.rs"
TYPE_LITERAL_RE = re.compile(r"\"((?:[a-z][a-z0-9_-]*\.)+[a-z0-9_-]+)\"")
TYPE_SHAPE_RE = re.compile(r"[a-z][a-z0-9_-]*(?:\.[a-z0-9_-]+)+")
CONSTRUCTOR_RE = re.compile(r"EventDefinition::(?:ephemeral|durable)\(\s*\"([^\"]+)\"")


def collect() -> tuple[list[dict], dict]:
    if not REGISTRY_RS.is_file():
        raise guardlib.GuardError(f"event registry missing: {REGISTRY_RS}")

    types: set[str] = set()
    for literal in TYPE_LITERAL_RE.findall(guardlib.read_text(REGISTRY_RS)):
        if TYPE_SHAPE_RE.fullmatch(literal):
            types.add(literal)

    publisher_files = 0
    for rel, path in guardlib.rust_files(guardlib.ROOT / "crates"):
        if guardlib.is_test_path(rel):
            continue
        text = guardlib.read_text(path)
        found = CONSTRUCTOR_RE.findall(text)
        if found:
            publisher_files += 1
        for literal in found:
            if TYPE_SHAPE_RE.fullmatch(literal):
                types.add(literal)

    if not types:
        raise guardlib.GuardError("no event types parsed — parser broken?")

    test_corpus: list[tuple[str, str]] = []
    for rel, path in guardlib.rust_files(guardlib.ROOT / "crates"):
        if guardlib.is_test_path(rel):
            test_corpus.append((rel, guardlib.read_text(path)))

    violations: list[dict] = []
    covered: list[str] = []
    for event_type in sorted(types):
        needle = f"\"{event_type}\""
        if any(needle in text for _, text in test_corpus):
            covered.append(event_type)
        else:
            violations.append({
                "id": f"event:{event_type}",
                "detail": "event type published via the bus registry / EventDefinition "
                          "constructors but referenced by no schema/round-trip test",
            })

    meta = {
        "event_types": len(types),
        "test_files": len(test_corpus),
        "publisher_files_with_literals": publisher_files,
        "covered": len(covered),
    }
    return violations, meta


if __name__ == "__main__":
    sys.exit(guardlib.run_guard("event_schema", collect, __doc__))
