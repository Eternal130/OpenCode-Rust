#!/usr/bin/env python3
''''exec python3 -- "$0" "$@" # '''
r"""Guard `route_validation` (§9.1): route table vs request-validation coverage.

WHAT IT CHECKS
  1. Every operation in the generated route table
     (`crates/oc-server/src/routes.gen.rs`, read-only generated file) must
     be mounted in a handler module AND its handler function must invoke a
     request validator (payload/query schema check) before doing work.
  2. Reverse direction (mount drift): every handler mount must correspond
     to an entry of the generated table — a route mounted in
     `crates/oc-server/src/handlers/` but absent from routes.gen.rs is a
     surface living outside the OpenAPI contract (or a stale mount after
     regeneration).

HEURISTIC (pragmatic — this is the documented approximation)
  * Routes: `Route { method: Method::X, path: "..", operation_id: ".." }`
    entries parsed from routes.gen.rs (141 today).
  * Mounts: `.route("<path>", <method>(<fn>))` registrations parsed from
    the non-test files in `crates/oc-server/src/handlers/` (axum-style
    `:param` templates on both sides — they match 1:1).
  * Handler body: the `async fn <fn>` block in the mounting file, located
    by paren/brace matching (not regex-on-body).
  * "Validation" = EITHER of:
      a) the handler body contains a validator invocation: a
         `schema_check::<fn>(..)` call (the port's byte-pinned Effect-
         schema error surface) or any call whose callee contains
         `validat`; OR
      b) the operation is listed in `ENFORCED_OPS` of the HTTP-H0
         generated-table middleware seam
         (`crates/oc-server/src/request_validation.rs`, wired as
         `schema_error_middleware` around the handler table in
         `handlers/mod.rs::mount`). The list is parsed from the seam the
         same way mounts are parsed from handlers — mechanical, no
         per-route allowlist. v2 validates EVERY route through schema
         middleware; the seam's list is the incremental rollout valve
         (HTTP-H0), so the baseline shrinks as the slice grows.

KNOWN LIMITATIONS
  * Deserialization via `Json<T>` extractors is treated as NOT validation
    (axum rejection ≠ Effect SchemaError bytes). That makes the guard
    strict; the baseline absorbs the difference today.
  * Closures or indirect mounts (`get(|| async ..)`) map to no named fn
    and count as unvalidated.
  * A validator invoked but ignored (`let _ =`) still counts as coverage.
  * The seam covers validate-first routes only; endpoints carrying v2's
    sessionLocationMiddleware (404-before-payload-400) deliberately stay
    unvalidated until their post-resolution seam lands (ledger HTTP-H0).

UPDATE BASELINE (`--update-baseline`) when a handler gains a validator
(shrink) or the OpenAPI surface is regenerated and grew (grow — justify
with the regeneration commit; README § Updating). Shrink-by-seam updates
quote the HTTP-H0 slice commit.
"""

import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import guardlib  # noqa: E402

ROUTES_GEN = guardlib.ROOT / "crates" / "oc-server" / "src" / "routes.gen.rs"
HANDLERS_DIR = guardlib.ROOT / "crates" / "oc-server" / "src" / "handlers"
SEAM = guardlib.ROOT / "crates" / "oc-server" / "src" / "request_validation.rs"
SEAM_WIRING = HANDLERS_DIR / "mod.rs"

ROUTE_RE = re.compile(
    r"Route\s*\{\s*method:\s*Method::(\w+)\s*,\s*path:\s*\"([^\"]+)\"\s*,"
    r"\s*operation_id:\s*\"([^\"]+)\"",
    re.S,
)
MOUNT_RE = re.compile(
    r"\.route\(\s*\"([^\"]+)\"\s*,\s*(?:\w+::)*(\w+)\s*\(\s*([\w]+)\s*\)",
    re.S,
)
FN_RE = re.compile(r"(?:pub\s+)?async\s+fn\s+(\w+)\s*\(")
VALIDATOR_RE = re.compile(r"schema_check::\s*\w+\s*\(|\b\w*validat\w*\s*\(")
SEAM_OPS_RE = re.compile(
    r"const ENFORCED_OPS:\s*&\[\&str\]\s*=\s*&\[(.*?)\]", re.S
)
SEAM_OP_ITEM_RE = re.compile(r"\"([^\"]+)\"")
SEAM_WIRING_RE = re.compile(r"request_validation::schema_error_middleware")


def middleware_enforced_ops() -> set[str]:
    """Operation ids enforced by the HTTP-H0 middleware seam.

    Raises when the seam declares ops but lost its wiring — a dead seam
    must fail the guard, not silently shrink the baseline.
    """
    if not SEAM.is_file():
        return set()
    text = guardlib.read_text(SEAM)
    match = SEAM_OPS_RE.search(text)
    if not match:
        raise guardlib.GuardError(f"seam {SEAM} has no ENFORCED_OPS const")
    ops = set(SEAM_OP_ITEM_RE.findall(match.group(1)))
    if ops and SEAM_WIRING.is_file():
        wiring = guardlib.read_text(SEAM_WIRING)
        cfg_test_at = wiring.find("#[cfg(test)]")
        searchable = wiring if cfg_test_at == -1 else wiring[:cfg_test_at]
        if not SEAM_WIRING_RE.search(searchable):
            raise guardlib.GuardError(
                f"seam declares {len(ops)} enforced ops but {SEAM_WIRING.name} "
                "does not layer request_validation::schema_error_middleware "
                "(production call site missing)"
            )
    return ops


def _fn_body(text: str, name: str) -> str | None:
    """Body of the first `async fn <name>` — paren-then-brace matched."""
    for match in FN_RE.finditer(text):
        if match.group(1) != name:
            continue
        i = match.end() - 1  # at '('
        depth = 0
        while i < len(text):
            if text[i] == "(":
                depth += 1
            elif text[i] == ")":
                depth -= 1
                if depth == 0:
                    break
            i += 1
        if depth != 0:
            return None
        while i < len(text) and text[i] != "{":
            if text[i] == ";":
                return None  # declaration without a body
            i += 1
        if i >= len(text):
            return None
        start = i
        depth = 0
        while i < len(text):
            if text[i] == "{":
                depth += 1
            elif text[i] == "}":
                depth -= 1
                if depth == 0:
                    return text[start : i + 1]
            i += 1
        return None
    return None


def collect() -> tuple[list[dict], dict]:
    if not ROUTES_GEN.is_file():
        raise guardlib.GuardError(f"route table missing: {ROUTES_GEN}")
    if not HANDLERS_DIR.is_dir():
        raise guardlib.GuardError(f"handlers dir missing: {HANDLERS_DIR}")

    gen_text = guardlib.read_text(ROUTES_GEN)
    routes = [
        (m.group(1).lower(), m.group(2), m.group(3))
        for m in ROUTE_RE.finditer(gen_text)
    ]
    if not routes:
        raise guardlib.GuardError("no routes parsed from routes.gen.rs — parser broken?")

    mounts: dict[str, list[tuple[str, str, str]]] = {}  # path -> [(method, fn, file)]
    bodies: dict[tuple[str, str], str] = {}             # (file, fn) -> body
    for path in sorted(HANDLERS_DIR.glob("*.rs")):
        rel = path.relative_to(guardlib.ROOT).as_posix()
        if guardlib.is_test_path(rel):
            continue
        text = guardlib.read_text(path)
        # Mounts/fns after a `#[cfg(test)]` marker are test fixtures
        # (e.g. mod.rs's `/api/boom` panic-envelope test), not surface.
        cfg_test_at = text.find("#[cfg(test)]")
        for m in MOUNT_RE.finditer(text):
            if cfg_test_at != -1 and m.start() >= cfg_test_at:
                continue
            mounts.setdefault(m.group(1), []).append((m.group(2).lower(), m.group(3), rel))
        seen: set[str] = set()
        for fm in FN_RE.finditer(text):
            if fm.group(1) in seen:
                continue
            seen.add(fm.group(1))
            body = _fn_body(text, fm.group(1))
            if body is not None:
                bodies[(rel, fm.group(1))] = body

    violations: list[dict] = []
    validated: list[str] = []
    seam_ops = middleware_enforced_ops()
    known_ops = {(method, route_path, op) for method, route_path, op in routes}
    unknown_seam = sorted(op for op in seam_ops if op not in {o for _, _, o in known_ops})
    if unknown_seam:
        raise guardlib.GuardError(
            f"seam ENFORCED_OPS names operations absent from routes.gen.rs: {unknown_seam}"
        )
    route_keys = {(method, route_path) for method, route_path, _ in routes}
    route_paths = {route_path for _, route_path, _ in routes}
    for method, route_path, op in routes:
        if op in seam_ops:
            validated.append(op)
            continue
        candidates = [c for c in mounts.get(route_path, []) if c[0] == method]
        if not candidates:
            if mounts.get(route_path):
                violations.append({
                    "id": f"route:{method.upper()} {route_path}",
                    "detail": f"op={op} mounted for a different method; no {method.upper()} handler mount found",
                })
            else:
                violations.append({
                    "id": f"route:{method.upper()} {route_path}",
                    "detail": f"op={op} has no handler mount in crates/oc-server/src/handlers/",
                })
            continue
        _, fn, rel = candidates[0]
        body = bodies.get((rel, fn))
        if body is not None and VALIDATOR_RE.search(body):
            validated.append(op)
        else:
            handler = f"{rel}:{fn}" if body is not None else f"{rel}:{fn} (body not extracted)"
            violations.append({
                "id": f"route:{method.upper()} {route_path}",
                "detail": f"op={op} handler {handler} invokes no validator (schema_check::/validat*)",
            })

    # Mount drift: handler mounts that live outside the generated table.
    drift = 0
    for route_path, entries in sorted(mounts.items()):
        for method, fn, rel in sorted(entries):
            if (method, route_path) in route_keys:
                continue
            drift += 1
            if route_path in route_paths:
                detail = f"mounted {method.upper()} but the table only has other methods"
            else:
                detail = "mount absent from the generated route table (surface drift)"
            violations.append({
                "id": f"route-mount:{method.upper()} {route_path}",
                "detail": f"{rel}:{fn}: {detail}",
            })

    meta = {
        "routes": len(routes),
        "mount_paths": len(mounts),
        "mount_drift": drift,
        "validated_ops": len(validated),
        "validated": sorted(validated),
        "seam_enforced_ops": len(seam_ops),
    }
    return violations, meta


if __name__ == "__main__":
    sys.exit(guardlib.run_guard("route_validation", collect, __doc__))
