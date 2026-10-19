#!/usr/bin/env python3
''''exec python3 -- "$0" "$@" # '''
r"""Guard `config_consumers` (§9.1): schema has a key, code has no read point.

WHAT IT CHECKS
  Every deserializable field of the `opencode.json` schema structs in
  `crates/oc-schema/src/config/**` must have at least one plausible
  consumer in production Rust code. A violation ("dead key") is a schema
  field with zero references outside the schema definitions themselves.
  The baseline absorbs today's dead keys — the ledger tracks those
  individually; this guard only fails on NEW dead keys.

HEURISTIC (pragmatic, regex-based — no rustc)
  * Schema side: every `pub <field>: <ty>` line inside a `struct` block
    in `crates/oc-schema/src/config/*.rs`. The config key is the field
    name, or the `#[serde(rename = "..")]` value when present. Fields
    carrying a bare `#[serde(skip)]` / `skip_deserializing` are not
    config keys and are ignored (`skip_serializing_if` alone does NOT
    disqualify — those fields still deserialize).
  * Consumer side: a field/key counts as consumed when the token
    `.<field>` (struct access) OR the quoted key `"<key>"` (map lookup /
    serde alias) appears in any non-test `.rs` file under `crates/`
    EXCLUDING `crates/oc-schema/src/config/**` (the definitions).
    Common-word field names (`model`, `agent`, `shell`, …) therefore
    almost never read as dead — the heuristic is deliberately
    conservative: it can miss dead keys, it must not invent any.

KNOWN LIMITATIONS (honest scoping)
  * The struct parser is line-regex based. Tuple structs, macros, or
    nested-in-impl structs would be mis-parsed; none exist in the config
    tree today, and the guard reports the parsed section inventory in
    `meta` so drift is visible. If a section ever becomes unparseable
    it shows up as a diff in field counts, not as silent skipping.
  * A reference inside a `#[cfg(test)]` module of a production file
    still counts as a consumer (file-level test exclusion only). This
    can only under-report violations.
  * Consumption means "referenced", not "semantically honored" — a key
    whose read point exists but is wired to a no-op is invisible here.

UPDATE BASELINE (`--update-baseline`) only when the key set legitimately
changed (schema port grew); justify in the commit message (README § Updating).
"""

import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import guardlib  # noqa: E402

CONFIG_DIR = guardlib.ROOT / "crates" / "oc-schema" / "src" / "config"

STRUCT_RE = re.compile(r"struct\s+(\w+)[^{]*\{(.*?)\n\}", re.S)
FIELD_RE = re.compile(r"((?:#\[[^\]]*\]\s*)*)pub\s+(\w+)\s*:")
SKIP_RE = re.compile(r"\bskip\b(?![\w_])")
RENAME_RE = re.compile(r"rename\s*=\s*\"([^\"]+)\"")


def collect() -> tuple[list[dict], dict]:
    if not CONFIG_DIR.is_dir():
        raise guardlib.GuardError(f"config schema dir missing: {CONFIG_DIR}")

    keys: list[tuple[str, str, str, str]] = []  # (file, struct, field, key)
    sections: dict[str, dict] = {}
    for path in sorted(CONFIG_DIR.glob("*.rs")):
        rel = path.relative_to(guardlib.ROOT).as_posix()
        text = guardlib.read_text(path)
        n_structs = n_fields = 0
        for match in STRUCT_RE.finditer(text):
            struct, body = match.group(1), match.group(2)
            n_structs += 1
            for field_match in FIELD_RE.finditer(body):
                attrs, field = field_match.group(1), field_match.group(2)
                n_fields += 1
                if SKIP_RE.search(attrs):
                    continue  # never deserialized — not a config key
                rename = RENAME_RE.search(attrs)
                key = rename.group(1) if rename else field
                keys.append((rel, struct, field, key))
        sections[rel] = {"structs": n_structs, "fields": n_fields}
    if not keys:
        raise guardlib.GuardError("no config schema fields parsed — parser broken?")

    # Consumer corpus: production .rs under crates/, minus the definitions
    # themselves and minus test files.
    corpus: list[tuple[str, str]] = []
    for rel, path in guardlib.rust_files(guardlib.ROOT / "crates"):
        if rel.startswith("crates/oc-schema/src/config/") or guardlib.is_test_path(rel):
            continue
        corpus.append((rel, guardlib.read_text(path)))

    violations: list[dict] = []
    for rel, struct, field, key in keys:
        pattern = re.compile(
            r"\.\s*" + re.escape(field) + r"\b|\"" + re.escape(key) + "\""
        )
        if not any(pattern.search(text) for _, text in corpus):
            violations.append({
                "id": f"config:{rel}::{struct}.{field}",
                "detail": f"config key {key!r} has no read point outside the schema definitions",
            })

    meta = {
        "schema_sections": sections,
        "keys_parsed": len(keys),
        "consumer_files": len(corpus),
    }
    return violations, meta


if __name__ == "__main__":
    sys.exit(guardlib.run_guard("config_consumers", collect, __doc__))
