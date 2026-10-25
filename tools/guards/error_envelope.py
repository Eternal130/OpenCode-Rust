#!/usr/bin/env python3
''''exec python3 -- "$0" "$@" # '''
r"""Guard `error_envelope` (RA-09): OpenAPI error responses vs the oc
error-envelope registry.

WHAT IT CHECKS
  For every (operation, status, `_tag`) the pinned OpenAPI spec
  (`tools/openapi-gen/openapi.json`) declares in an operation's non-2xx
  JSON responses, the oc registry (`crates/oc-server/src/errors.rs`)
  must be able to produce an envelope with exactly that `_tag` and that
  HTTP status. The registry is parsed statically — a constructor counts
  when it builds either
    * `tagged("<Tag>", StatusCode::<CONST>, ..)`  (the registry helper), or
    * `ApiError::new(StatusCode::<CONST>, json!({ "_tag": "<Tag>", .. }))`.

  Registry constructors richer than the spec (e.g. the readiness `code`
  envelopes, which have no `_tag`) are fine — the check is one-directional:
  the SPEC side may never drift outside the registry.

SUPPLEMENT (spec-invisible envelopes)
  The OpenAPI spec cannot express middleware-produced failures.
  `LocationNotFoundError` 404 is emitted by the request-location seam
  (`handlers/common.rs::resolve_location`) on every location-carrying
  route; three representative sites are pinned by hand below. Both
  directions hold for the supplement: the tag/status must exist in the
  registry (checked), and the entries must keep resolving (hand-listed
  here, not generated).

WHY A GUARD (not a generated test)
  Zero cargo dependency: this runs with the other §9.1 stdlib-only
  guards in CI's `guards` job before any Rust build, so an OpenAPI
  regeneration that introduces a new error tag fails fast instead of
  surfacing as a runtime parity gap. (The alternative — a generated
  `tests/error_envelope.gen.rs` — needs a compiler to say anything.)

EXIT CODES
  0 all spec (op, status, tag) triples + supplement are producible
  1 at least one triple has no registry constructor (message lists them)
  2 guard error (malformed spec / unreadable registry)
"""

import json
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import guardlib  # noqa: E402

SPEC = guardlib.ROOT / "tools" / "openapi-gen" / "openapi.json"
REGISTRY = guardlib.ROOT / "crates" / "oc-server" / "src" / "errors.rs"

# Hand-maintained supplement: (site, tag, status) the spec cannot see.
SUPPLEMENT = [
    ("middleware: location query resolution (any location-carrying route)", "LocationNotFoundError", 404),
    ("middleware: x-opencode-directory header resolution", "LocationNotFoundError", 404),
    ("middleware: boot-location fallback resolution", "LocationNotFoundError", 404),
]

# axum StatusCode const name -> HTTP status code (the subset the registry
# uses; an unknown const is a guard error, not a silent pass).
STATUS_CODES = {
    "BAD_REQUEST": 400,
    "UNAUTHORIZED": 401,
    "FORBIDDEN": 403,
    "NOT_FOUND": 404,
    "CONFLICT": 409,
    "PAYLOAD_TOO_LARGE": 413,
    "UNPROCESSABLE_ENTITY": 422,
    "TOO_MANY_REQUESTS": 429,
    "INTERNAL_SERVER_ERROR": 500,
    "NOT_IMPLEMENTED": 501,
    "BAD_GATEWAY": 502,
    "SERVICE_UNAVAILABLE": 503,
}

TAGGED_RE = re.compile(r'tagged\(\s*"([A-Za-z0-9_.]+)"\s*,\s*StatusCode::([A-Z_]+)')
INLINE_RE = re.compile(
    r'ApiError::new\(\s*StatusCode::([A-Z_]+)[^;]{0,400}?"_tag"\s*:\s*"([A-Za-z0-9_.]+)"',
    re.DOTALL,
)


def registry_pairs():
    """(tag -> {status codes}) producible by the oc registry."""
    text = REGISTRY.read_text(encoding="utf-8")
    # Strip the test module: test literals are not constructors.
    text = text.split("#[cfg(test)]", 1)[0]
    pairs = {}
    for tag, const in TAGGED_RE.findall(text):
        pairs.setdefault(tag, set()).add(STATUS_CODES[const])
    for const, tag in INLINE_RE.findall(text):
        pairs.setdefault(tag, set()).add(STATUS_CODES[const])
    return pairs


def resolve(schema, schemas, depth=0):
    if not isinstance(schema, dict) or depth > 8:
        return None
    if "$ref" in schema:
        return resolve(schemas.get(schema["$ref"].split("/")[-1], {}), schemas, depth + 1)
    return schema


def spec_triples():
    """[(op_id, status, tag)] declared by the pinned spec."""
    spec = json.loads(SPEC.read_text(encoding="utf-8"))
    schemas = spec.get("components", {}).get("schemas", {})
    triples = set()
    for path, item in spec.get("paths", {}).items():
        for method, op in item.items():
            if method.startswith(("x-", "parameters")) or not isinstance(op, dict):
                continue
            op_id = op.get("operationId") or f"{method.upper()} {path}"
            for status, response in (op.get("responses") or {}).items():
                for media in (response.get("content") or {}).values():
                    schema = resolve(media.get("schema", {}), schemas)
                    if not schema:
                        continue
                    candidates = [schema] + schema.get("anyOf", []) + schema.get("oneOf", [])
                    for candidate in candidates:
                        resolved = resolve(candidate, schemas)
                        tag = (resolved or {}).get("properties", {}).get("_tag", {})
                        value = tag.get("const") if isinstance(tag, dict) else None
                        if isinstance(tag, dict) and value is None and len(tag.get("enum", [])) == 1:
                            value = tag["enum"][0]
                        if isinstance(value, str):
                            triples.add((op_id, int(status), value))
    return triples


def main():
    try:
        producible = registry_pairs()
    except (OSError, KeyError) as error:
        print(f"error: cannot parse registry {REGISTRY}: {error}")
        return 2
    try:
        triples = spec_triples()
    except (OSError, ValueError, KeyError) as error:
        print(f"error: cannot parse spec {SPEC}: {error}")
        return 2
    if not triples:
        print("error: spec yielded zero error triples — parsing drift?")
        return 2

    missing = []
    for op_id, status, tag in sorted(triples):
        if status not in producible.get(tag, set()):
            missing.append(f"{op_id} -> {status} {tag}")
    for site, tag, status in SUPPLEMENT:
        if status not in producible.get(tag, set()):
            missing.append(f"supplement ({site}) -> {status} {tag}")

    print(f"spec error triples: {len(triples)} (+{len(SUPPLEMENT)} supplement entries)")
    print(f"registry tags: {len(producible)}")
    if missing:
        print("\nNEW spec error responses the registry cannot produce:")
        for line in missing:
            print(f"  {line}")
        print("\nfix: add a constructor in crates/oc-server/src/errors.rs (or regenerate")
        print("the spec if the surface shrank).")
        print(f"SUMMARY error_envelope total={len(triples) + len(SUPPLEMENT)} baseline=0 allowlist=0 new={len(missing)} shrunk=0")
        return 1
    print("SUMMARY error_envelope total=%d baseline=0 allowlist=0 new=0 shrunk=0" % (len(triples) + len(SUPPLEMENT)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
