#!/usr/bin/env python3
"""TUI full-screen differential (channel 8 of the diff layer).

Corpus: `tui-diff-scenario/1` fixtures
(tools/diff/corpus/tui/scenarios/*.json, normative semantics in
tools/diff/corpus/tui/SCHEMA.md).  Each scenario is replayed through
BOTH TUI implementations and the resting frame compared as a
normalized dense cell grid (`tui-diff-grid/1`):

  ref  : bun  tools/diff/drivers/tui_fullscreen_ref.ts
         (staged to /tmp/opencode/diff-tuifull; boots the reference
          v2.0.24 TUI headless through its own createAppFixture at
          $DIFF_REF_ROOT, read-only; the solid transform preload is
          passed as an ABSOLUTE ref-tree path, so — unlike channel 7 —
          NO node_modules symlink is staged; bun 1.3.10, the tui-dump
          invocation convention).
  rust : cargo nextest -p oc-tui --test tui_fullscreen_diff_driver
         (env-gated: TUI_DIFF_MANIFEST/TUI_DIFF_OUT, driven through
          tools/dev-gate.sh; replays the scenario through the
          production ingest + render chain, no PTY).

Size tiers (SCHEMA 尺寸档位): every scenario renders at three tiers —
the primary tier (the file's own width/height, un-suffixed case id)
plus the mirrored 120x30 and 320x80 tiers (case id `<scenario-id>@WxH`)
— same fixture, only the grid size changes.  A scenario `tiers` array
overrides the mirrored set (declared subset only, justification
required in `note` per SCHEMA).

Manifest ("tui-diff-manifest/1", consumed by BOTH drivers, cases
sorted by id): {"schema","cases":[{id,scenario,width,height}]}.
Scenario paths in the manifest are absolute paths into the STAGE copy
(the rust driver runs nextest crate-local and cannot resolve a
ref-cwd-relative path; the ref driver's path.resolve handles absolute
paths identically).

Infra semantics (this task, T4): a run is an infra failure — exit 1 —
when either driver exits nonzero, when any case's `stub_gaps` is
non-empty (SCHEMA stub_gap rule: a requested-but-unstubbed endpoint is
an infra failure, never a render diff), or when a scenario is missing
its grid on either side.  `request_census` is NOT compared: the two
sides' boot request patterns are known to be asymmetric; the census is
recorded in the report for reference only.

Subprocess discipline: each driver is spawned as an ordinary child
(terminate/wait on our own child only); NO process-group signalling —
the http_full.py killpg pattern is path-specific and explicitly NOT
reusable here.  Both drivers are expected to exit on their own.

Out of scope (later tasks on this channel): divergence adjudication
(T9).  The CAUSES table (T6) annotates KNOWN-cause clusters only —
matchers are exact-shape (scenario-id prefix + divergence form); an
unknown divergence never matches and stays unadjudicated => exit 2.

Output: markdown report (default docs/audit/diff/TUI-FULLSCREEN.md).
Exit codes (gate): 0 — report produced and every COMMITTED case
adjudicated (exact, or non-exact covered by a CAUSES matcher);
recorded-local (`rec-*`) unadjudicated divergences are ADVISORY (the
2026-11-15 controller ruling: daily recordings routinely carry open
adjudications — committed green is gate green; their infra failures
still exit 1 — a broken replay mechanism is never advisory); 1 —
infra failure (driver crash/timeout, stub gap, missing/malformed
grid, size or cells-shape mismatch vs manifest — for every case,
rec- included); 2 — at least one COMMITTED case diverges and is not
covered by a known CAUSES annotation (T6 table; unadjudicated
divergences are T9's input).
Deterministic: sorted cases, canonical JSON staging.
"""
from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent.parent
REF_DEFAULT = Path("/opt/opencode-ref")
STAGE = Path("/tmp/opencode/diff-tuifull")
LOG_DIR = Path("/tmp/opencode/logs")

CORPUS = REPO / "tools/diff/corpus/tui/scenarios"
# diff-ext C2 — the LOCAL real-session corpus (git-ignored, privacy
# iron law: recorded data never enters a commit; reports aggregate
# counts only). Directory absent = zero effect on the channel.
RECORDED = REPO / "tools/diff/corpus/tui/recorded-local"
REF_DRIVER = REPO / "tools/diff/drivers/tui_fullscreen_ref.ts"
MIRRORED_TIERS = [(120, 30), (320, 80)]

REF_PRELOAD = "packages/tui/node_modules/@opentui/solid/scripts/preload.js"

# B2 — the real-plugin payload registry (user ruling 2026-11-14: the
# diff pin is ref 2.0.24 and the payload is the user's REAL plugins;
# boot success/failure is decided by the fixture run, not static
# analysis of the minified bundles). Scenario `plugins` fields carry
# the symbolic ids; the orchestrator stages each payload ONCE per run
# into STAGE/plugins/node_modules/<package> (a real node_modules
# layout: the rust guest's @opentui shims land in the parent dir and
# bare imports resolve through the node walk) and the manifest carries
# the staged ABSOLUTE package dirs (both drivers read them from there
# — the read-only cache entities are never loaded in place).
PLUGIN_PAYLOADS = {
    "omo-slim": {
        # newest timestamped stamp under the @latest install key
        "source": "npm/oh-my-opencode-slim@latest",
        "package": "oh-my-opencode-slim",
        # dist+src+manifest (the flagship-test recipe); deps copied
        # from the install's hoisted node_modules
        "copy_dirs": ["dist", "src"],
        "copy_files": ["package.json", "oh-my-opencode-slim.schema.json"],
        "deps": ["jsonc-parser", "zod", "solid-js", "lru-cache", "semver",
                 "@opencode-ai/plugin", "@opencode-ai/sdk"],
        # B2-fix: 2.0.24's local-dir loader resolves ONLY the
        # `<dir>/tui.*` extension-probe convention (host.ts entry
        # (["tui"]) — package.json exports subpaths are never
        # consulted for absolute specifiers, probed under bun 1.4.2:
        # `Bun.resolveSync("<pkg>/tui")` throws ERR_MODULE_NOT_FOUND
        # even though exports["./tui"] exists). The production path
        # (packages.prepare -> package-name resolution through
        # node_modules) is pinned away by the fixture's
        # `prepare: async () => ({ directory: "" })` (ref tree
        # read-only). The staging therefore adds a 2-line re-export
        # shim at the staged copy's root expressing the package's OWN
        # declared TUI entry in the one shape 2.0.24 understands —
        # the payload itself and the read-only cache entity are
        # untouched, and the rust side (exports-aware) is unaffected.
        "tui_shim": True,
    },
    "superpowers": {
        "source": "npm/git-superpowers-*",
        "package": "superpowers",
        # the whole tree (main = .opencode/plugins/superpowers.js walks
        # up to ../skills); no external deps
        "copy_dirs": [],
        "copy_files": [],
        "deps": [],
    },
    # B2-fix: the deliberate-failure payload for plugin-fallback — the
    # failure-state rendering path on BOTH sides (committed fixture,
    # no cache entity). ref: `<dir>/tui` probes tui.js -> import
    # throws -> reconcile failure state. rust: package.json main ->
    # tui.js -> guest load throws -> PluginState::Failed (contained).
    "broken": {
        "source": "repo/tools/diff/corpus/tui/payloads/broken-plugin",
        "package": "broken-plugin",
        "copy_dirs": [],
        "copy_files": [],
        "deps": [],
    },
}
# env overrides (payload roots), e.g. OC_DIFF_PLUGIN_OMO_SLIM=/path/to/pkg
PLUGIN_ENV = {"omo-slim": "OC_DIFF_PLUGIN_OMO_SLIM",
              "superpowers": "OC_DIFF_PLUGIN_SUPERPOWERS"}
NPM_CACHE = Path(os.environ.get("OC_DIFF_NPM_CACHE", "/root/.cache/opencode/npm"))

# B2-fix (controller-confirmed root cause): the ref plugin-boot failure
# pinned in the first B2 round was bun 1.3.10's ResolveMessage NOT
# being an `instanceof Error` (it IS one under bun >= 1.4.2) — ref
# host.ts:17-43's swallow guard depends on it. Plugin scenarios'
# ref leg therefore runs under TUI_DIFF_BUN with version >= this floor;
# non-plugin scenarios keep the environment bun (baseline no-drift).
BUN_FLOOR = (1, 4, 2)

# driver timeouts (seconds): ref boots bun quickly; rust includes a
# cold `cargo nextest` compile of the oc-tui tree
REF_TIMEOUT = 600
RUST_TIMEOUT = 2400


# ---------------------------------------------------------------------------
# manifest + staging
# ---------------------------------------------------------------------------

def _copytree(source: Path, destination: Path) -> None:
    shutil.copytree(source, destination, dirs_exist_ok=True)


def _payload_source(payload: dict) -> Path | None:
    """Resolve one payload's read-only source entity (env override
    first; repo:// fixtures for committed payloads; newest npm stamp
    otherwise)."""
    override = os.environ.get(PLUGIN_ENV.get(str(payload), "") or "", "")
    if override:
        path = Path(override)
        return path if path.is_dir() else None
    source = payload["source"]
    if source.startswith("repo/"):
        path = REPO / source[len("repo/"):]
        return path if path.is_dir() else None
    kind, _, key = source.partition("/")
    if kind != "npm" or not NPM_CACHE.is_dir():
        return None
    # glob keys (git-superpowers-*) resolve to their install bases first
    import glob as _glob
    bases = sorted(Path(p) for p in _glob.glob(str(NPM_CACHE / key)) if Path(p).is_dir())
    stamps: list[Path] = []
    for base in reversed(bases):  # newest base, then newest stamp
        stamps.extend(p for p in base.iterdir() if p.is_dir())
    for stamp in reversed(stamps):
        candidate = stamp / "node_modules" / payload["package"]
        if candidate.is_dir():
            return candidate
    return None


def stage_plugins(stage: Path, ids: list[str]) -> tuple[dict[str, str], dict[str, str], list[str]]:
    """Stage every referenced payload into TWO STAGE roots (B2-fix):
    `plugins-ref/node_modules` — the REF leg's tree: identical payload
    copies + deps PLUS an @opentui symlink into the ref tree's own
    host-provided renderer stack (the ref process really runs the
    plugin's Solid sidebar under its own @opentui 0.5.14).
    `plugins-rust/node_modules` — the RUST leg's tree: same payload
    copies + deps, NO @opentui — the rust guest's ensureOpentuiShims
    writes its bounded bridge shims there (the vt_plugin_sidebar
    recipe; a real @opentui in the guest breaks at render() with "No
    renderer found" — there is no renderer context in the guest).
    Both world-writable (gate user + guest shim writes).
    Returns ({id: ref dir}, {id: rust dir}, [unavailable ids])."""
    staged_ref: dict[str, str] = {}
    staged_rust: dict[str, str] = {}
    unavailable: list[str] = []
    for pid in sorted(set(ids)):
        payload = PLUGIN_PAYLOADS[pid]
        source = _payload_source(payload)
        if source is None:
            unavailable.append(pid)
            continue
        for root_name in ("plugins-ref", "plugins-rust"):
            root = stage / root_name / "node_modules"
            package = root / payload["package"]
            for name in payload["copy_dirs"]:
                _copytree(source / name, package / name)
            for name in payload["copy_files"]:
                shutil.copyfile(source / name, package / name)
            if not payload["copy_dirs"] and not payload["copy_files"]:
                _copytree(source, package)  # whole-tree payload
            if payload.get("tui_shim"):
                (package / "tui.js").write_text(
                    "// harness staging shim (diff-ext B2-fix): re-export the "
                    "package's declared TUI entry (exports[\"./tui\"] -> "
                    "dist/tui2.js) in the one shape the 2.0.24 local-dir "
                    "loader resolves (<dir>/tui.* extension probe). Not part "
                    "of the payload; the read-only cache entity is "
                    "untouched.\nexport { default } from \"./dist/tui2.js\";\n"
                    "export * from \"./dist/tui2.js\";\n")
            for dep in payload["deps"]:
                dep_source = source.parent / dep
                if dep_source.is_dir():
                    _copytree(dep_source, root / dep)
            # the ref tree's host-provided @opentui renderer stack (the
            # plugin's optionalDependencies pin 0.5.11; the ref harness's
            # own resolved stack is the closest "host-provided"
            # instance) — REF ROOT ONLY (see docstring), a symlink into
            # the read-only ref tree, never loaded in place.
            if root_name == "plugins-ref" and pid == "omo-slim":
                ref_opentui = Path(os.environ.get("DIFF_REF_ROOT", str(REF_DEFAULT))) \
                    / "packages/tui/node_modules/@opentui"
                if ref_opentui.is_dir() and not (root / "@opentui").exists():
                    os.symlink(ref_opentui, root / "@opentui")
            # world-writable (gate user + guest shim writes)
            for dirpath, _dirnames, filenames in os.walk(root):
                os.chmod(dirpath, 0o1777)
                for name in filenames:
                    os.chmod(os.path.join(dirpath, name), 0o666)
            if root_name == "plugins-ref":
                staged_ref[pid] = str(package)
            else:
                staged_rust[pid] = str(package)
    return staged_ref, staged_rust, unavailable


def build_manifest(stage: Path) -> tuple[list[dict], list[str]]:
    """Stage every scenario fixture into STAGE and emit the sorted
    `tui-diff-manifest/1` case list (primary tier + mirrored tiers, or
    the scenario's declared `tiers` subset when present). Plugin
    scenarios (`plugins` field) additionally stage their payloads; a
    payload whose cache entity is absent marks the case
    plugins_unavailable on BOTH sides (a consistent skip, SCHEMA B2 —
    the gate drops it, the report lists it)."""
    if stage.exists():
        shutil.rmtree(stage)
    scenarios_dir = stage / "scenarios"
    scenarios_dir.mkdir(parents=True)
    # the rust leg runs through tools/dev-gate.sh as a different user
    # (eternal130, the gate's identity rule) — stage world-writable
    # with the sticky bit (1777, like /tmp)
    os.chmod(stage, 0o1777)
    os.chmod(scenarios_dir, 0o1777)

    plugin_ids: list[str] = []
    raw_cases: list[dict] = []

    # diff-ext C2 — CI/infra defense FIRST: nothing under recorded-local
    # may be tracked or staged (a `git add -f` would leak real session
    # data; the guard tools/guards/recorded_local.py is the static twin).
    tracked = subprocess.run(
        ["git", "-c", "core.quotepath=false", "ls-files", str(RECORDED.relative_to(REPO))],
        cwd=REPO, capture_output=True, text=True, check=False)
    leaked = [p for p in tracked.stdout.split()
              if p != "tools/diff/corpus/tui/recorded-local/README.md"]
    if leaked:
        raise SystemExit(
            f"infra failure: recorded-local content is git-tracked: {leaked} "
            "(privacy iron law: recorded data never enters a commit)")

    corpus_dirs = [CORPUS]
    if RECORDED.is_dir():
        corpus_dirs.append(RECORDED)
    for corpus_dir in corpus_dirs:
        for fname in sorted(os.listdir(corpus_dir)):
            if not fname.endswith(".json"):
                continue
            doc = json.loads((corpus_dir / fname).read_text())
            if doc.get("schema") != "tui-diff-scenario/1":
                raise SystemExit(f"bad scenario schema: {fname}")
            staged = scenarios_dir / fname
            shutil.copyfile(corpus_dir / fname, staged)
            primary = (doc["width"], doc["height"])
            # A2: the frame-sequence tick count (present on anim scenarios
            # only; load_side validates frames length against it)
            anim_ticks = (doc.get("anim") or {}).get("ticks")
            # B2: the plugin payload ids (resolved to staged dirs after the
            # corpus sweep — staging happens once per referenced payload)
            plugins = doc.get("plugins") or []
            plugin_ids.extend(plugins)
            common = {**({"anim_ticks": anim_ticks} if anim_ticks is not None else {})}
            tiers: list[tuple[int, int]]
            if "tiers" in doc:
                tiers = [tuple(t) for t in doc["tiers"]]  # type: ignore[misc]
                if not tiers:
                    raise SystemExit(f"{fname}: empty tiers override")
            else:
                tiers = [primary, *(t for t in MIRRORED_TIERS if t != primary)]
            for w, h in tiers:
                tid = doc["id"] if (w, h) == primary else f"{doc['id']}@{w}x{h}"
                raw_cases.append({"id": tid, "scenario": str(staged),
                                  "width": w, "height": h,
                                  **({"plugins": plugins} if plugins else {}), **common})
    if not raw_cases:
        raise SystemExit("empty corpus: no tui-diff-scenario/1 fixtures found "
                         f"under {CORPUS}")

    # B2 plugin staging: resolve payloads -> staged dirs / unavailable.
    # `plugins` (the manifest's ref-leg field) carries the REF-root
    # staged dirs (real @opentui); `plugins_rust` carries the RUST-root
    # twins (host-shim tree) — the drivers each read their own field.
    staged_ref, staged_rust, unavailable_plugins = stage_plugins(stage, plugin_ids)
    skipped: list[str] = []
    cases: list[dict] = []
    for case in raw_cases:
        pids = case.get("plugins") or []
        if pids:
            if any(pid in unavailable_plugins for pid in pids):
                skipped.append(case["id"])
                continue
            # G-424b fixture-boundary mirror: both legs pass the SAME
            # canonical path string (STAGE/plugins/…) — a symlink the
            # orchestrator repoints per leg (ref -> plugins-ref
            # before run_ref, rust -> plugins-rust before run_rust).
            # The staged trees must stay separate (the ref root
            # carries the host @opentui symlink; the rust root gets
            # the guest's bridge shims), but the toast/dialog
            # surfaces embed the config target STRING — unequal roots
            # ("plugins-ref" vs "plugins-rust") leak a one-char
            # harness asymmetry into the compared grids that no port
            # can close.
            case["plugins"] = [staged_ref[pid].replace("plugins-ref", "plugins") for pid in pids]
            case["plugins_rust"] = [staged_rust[pid].replace("plugins-rust", "plugins") for pid in pids]
        cases.append(case)
    cases.sort(key=lambda c: c["id"])
    manifest = {"schema": "tui-diff-manifest/1", "cases": cases}
    (stage / "manifest.json").write_text(json.dumps(manifest, indent=1))
    return cases, skipped


# ---------------------------------------------------------------------------
# drivers
# ---------------------------------------------------------------------------

def _logged_run(cmd: list[str], log_name: str, timeout: int | None = None,
                side: str = "", **kw) -> subprocess.CompletedProcess:
    LOG_DIR.mkdir(parents=True, exist_ok=True)
    try:
        res = subprocess.run(cmd, capture_output=True, text=True,
                             timeout=timeout, **kw)
    except subprocess.TimeoutExpired as exc:
        out, err = exc.stdout or "", exc.stderr or ""
        if isinstance(out, bytes):
            out = out.decode(errors="replace")
        if isinstance(err, bytes):
            err = err.decode(errors="replace")
        with open(LOG_DIR / log_name, "a") as log:
            log.write(f"$ {' '.join(cmd)}\nTIMEOUT after {timeout}s\n{out}{err}\n")
        print(out[-4000:], err[-4000:], sep="\n", file=sys.stderr)
        raise SystemExit(
            f"infra failure: {side or 'driver'} timed out after {timeout}s")
    with open(LOG_DIR / log_name, "a") as log:
        log.write(f"$ {' '.join(cmd)}\nrc={res.returncode}\n{res.stdout}{res.stderr}\n")
    return res


def _bun_version(bun: str) -> tuple[int, ...] | None:
    """Parse `<bun> --version` (e.g. '1.4.2'); None when unrunnable."""
    try:
        res = subprocess.run([bun, "--version"], capture_output=True, text=True,
                             timeout=30)
    except (OSError, subprocess.TimeoutExpired):
        return None
    if res.returncode != 0:
        return None
    parts = res.stdout.strip().split(".")
    try:
        return tuple(int(p.split("-")[0]) for p in parts[:3])
    except ValueError:
        return None


def run_ref(stage: Path, ref: Path, manifest: Path, out: Path,
            bun: str = "bun") -> None:
    """Bun-side reference driver: staged copy under STAGE, DIFF_REF_ROOT
    env, absolute-path solid preload (no node_modules symlink needed).
    B2-fix: plugin-case legs run under `bun` = TUI_DIFF_BUN (>= 1.4.2,
    the ResolveMessage-extends-Error fix the ref host.ts swallow guard
    depends on); non-plugin legs keep the environment bun.
    W5 hermeticity: XDG_CONFIG_HOME pins BOTH legs to one EMPTY config
    dir — the flagship payload reads the runner's personal user config
    (`~/.config/opencode/oh-my-opencode-slim.json`, tui2.js
    getDefaultOpenCodeConfigDir) for its preset row, so an unpinned
    environment leaks the DRIVER USER's private config into one leg
    only (the ref bun runs as the invoking user; the rust guest under
    the gate user) — an environment accident, not scenario data."""
    staged = stage / "tui_fullscreen_ref_driver.ts"
    if not staged.exists():
        shutil.copyfile(REF_DRIVER, staged)
    preload = ref / REF_PRELOAD
    env = {**os.environ,
           "DIFF_REF_ROOT": str(ref),
           "XDG_CONFIG_HOME": str(hermetic_config_home(stage))}
    res = _logged_run(
        [bun, "--preload", str(preload), str(staged),
         "--manifest", str(manifest), "--out", str(out)],
        "t4-ref-driver.log", cwd=stage, env=env,
        timeout=REF_TIMEOUT, side=f"ref driver ({bun})",
    )
    if res.returncode != 0:
        print(res.stdout[-4000:], res.stderr[-4000:], sep="\n", file=sys.stderr)
        raise SystemExit(f"ref driver failed: rc={res.returncode}")


def hermetic_config_home(stage: Path) -> Path:
    """The shared EMPTY XDG config dir (both legs' hermetic pin)."""
    xdg = stage / "hermetic-xdg"
    xdg.mkdir(parents=True, exist_ok=True)
    os.chmod(xdg, 0o1777)
    return xdg


def run_rust(manifest: Path, out: Path) -> None:
    """Rust-side driver via the dev gate (single command string, env
    prefixes folded into the same string per dev-gate convention).
    W5: TUI_DIFF_XDG pins the guest's XDG_CONFIG_HOME to the same
    EMPTY dir the ref leg runs under (the payload reads the runner's
    personal user config for its preset row — see run_ref)."""
    cmd = (
        f"TUI_DIFF_MANIFEST={manifest} TUI_DIFF_OUT={out} "
        f"TUI_DIFF_XDG={hermetic_config_home(manifest.parent)} "
        f"cargo nextest run -p oc-tui --test tui_fullscreen_diff_driver"
    )
    res = _logged_run(["tools/dev-gate.sh", cmd], "t4-rust-driver.log", cwd=REPO,
                      timeout=RUST_TIMEOUT, side="rust driver")
    if res.returncode != 0:
        print(res.stdout[-4000:], res.stderr[-4000:], sep="\n", file=sys.stderr)
        raise SystemExit(f"rust driver failed: rc={res.returncode}")


# ---------------------------------------------------------------------------
# result loading + infra semantics
# ---------------------------------------------------------------------------

def load_side(out: Path, cases: list[dict], side: str) -> tuple[dict[str, dict], list[str]]:
    """Load one side's grids; enforce the infra rules (stub_gaps,
    missing grid, cells/frames shape) with SystemExit(1). B2: grids
    carrying plugins_unavailable are collected (consistency ruled
    later) -> (grids, plugins_unavailable case ids)."""
    grids: dict[str, dict] = {}
    infra: list[str] = []
    plugins_unavailable: list[str] = []
    for case in cases:
        path = out / f"{case['id']}.json"
        if not path.exists():
            infra.append(f"{case['id']}: missing {side} grid")
            continue
        grid = json.loads(path.read_text())
        if grid.get("schema") != "tui-diff-grid/1":
            infra.append(f"{case['id']}: bad grid schema on {side}")
        if grid.get("error"):
            infra.append(f"{case['id']}: {side} driver error: {grid['error']}")
        stub_gaps = grid.get("stub_gaps") or []
        if stub_gaps:
            infra.append(f"{case['id']}: {side} stub_gaps {stub_gaps}")
        if grid.get("width") != case["width"] or grid.get("height") != case["height"]:
            infra.append(
                f"{case['id']}: {side} grid size {grid.get('width')}x{grid.get('height')} "
                f"!= manifest {case['width']}x{case['height']}")
        # T5-review follow-up: the cells array must actually be a dense
        # width x height grid — declared size == real size, else infra
        cells = grid.get("cells")
        if _bad_dense_grid(cells, grid.get("width"), grid.get("height")):
            real = (f"{len(cells)}x?" if isinstance(cells, list) else "non-list")
            infra.append(
                f"{case['id']}: {side} cells shape {real} "
                f"!= declared {grid.get('width')}x{grid.get('height')}")
        # A2: an anim scenario (manifest anim_ticks present) MUST emit
        # frames on BOTH sides — a missing/short frames array on either
        # side is an infra failure (never a silent exact: the review
        # found compare_frames' zip() over a missing rust side would
        # produce an empty per_tick list that all()-folded to "exact").
        frames = grid.get("frames")
        ticks = case.get("anim_ticks")
        if ticks is not None:
            if frames is None:
                infra.append(f"{case['id']}: {side} anim scenario (ticks={ticks}) "
                             f"emitted no frames")
            elif not isinstance(frames, list) or len(frames) != ticks + 1:
                infra.append(f"{case['id']}: {side} frames length "
                             f"{len(frames) if isinstance(frames, list) else 'non-list'} "
                             f"!= ticks+1 ({ticks + 1})")
            else:
                for tick, fgrid in enumerate(frames):
                    if _bad_dense_grid(fgrid, grid.get("width"), grid.get("height")):
                        infra.append(f"{case['id']}: {side} frame {tick} not a dense "
                                     f"{grid.get('width')}x{grid.get('height')} grid")
                        break
        elif frames is not None:
            infra.append(f"{case['id']}: {side} emitted frames but scenario has no anim")
        # B2 plugin-skip semantics (SCHEMA): a plugins_unavailable grid
        # (bun/guest runtime or staged payload missing on that side) is
        # NOT an infra failure by itself — the consistency ruling is
        # the orchestrator's (both sides marked ⇒ consistent skip; one
        # side ⇒ infra). The grid still carries a valid dense blank so
        # shape checks hold.
        if grid.get("plugins_unavailable"):
            plugins_unavailable.append(case["id"])
        grids[case["id"]] = grid
    if infra:
        for line in infra:
            print(f"infra failure: {line}", file=sys.stderr)
        raise SystemExit(1)
    if plugins_unavailable:
        return grids, plugins_unavailable
    return grids, []


def reconcile_plugin_skips(ref_grids: dict[str, dict], rust_grids: dict[str, dict],
                           cases: list[dict]) -> tuple[list[dict], list[str]]:
    """B2 skip consistency: a plugins_unavailable marker must appear on
    BOTH sides — both ⇒ the case is skipped (dropped from the gate,
    listed in the report); exactly one side ⇒ infra failure (exit 1).
    Returns the surviving case list + the skipped case ids."""
    skipped: list[str] = []
    surviving: list[dict] = []
    for case in cases:
        cid = case["id"]
        ref_mark = bool(ref_grids.get(cid, {}).get("plugins_unavailable"))
        rust_mark = bool(rust_grids.get(cid, {}).get("plugins_unavailable"))
        if ref_mark and rust_mark:
            skipped.append(cid)
        elif ref_mark or rust_mark:
            side = "ref" if ref_mark else "rust"
            raise SystemExit(
                f"infra failure: {cid}: plugins_unavailable on the {side} side only "
                f"(SCHEMA B2: bun/payload availability must be consistent)")
        else:
            surviving.append(case)
    return surviving, skipped


def _bad_dense_grid(cells, width, height) -> bool:
    return (not isinstance(cells, list) or len(cells) != height or any(
        not isinstance(row, list) or len(row) != width for row in cells))


def check_bun_gate(bun: str | None, version_fn=_bun_version) -> None:
    """The TUI_DIFF_BUN gate (B2-fix): plugin-case ref legs need bun
    >= BUN_FLOOR. Raises SystemExit (the infra exit-1 path) when unset,
    unrunnable, or too old. `version_fn` is injectable for the
    selftest's negative cases (C3 final-review minor B2)."""
    if not bun:
        raise SystemExit(
            "infra failure: plugin scenarios present but TUI_DIFF_BUN is unset — "
            "the ref plugin boot requires bun >= 1.4.2 (ResolveMessage extends "
            "Error; the 2.0.24 host.ts swallow guard depends on it). Point "
            "TUI_DIFF_BUN at a bun >= 1.4.2 executable — the pinned repo-local "
            f"default is {REPO / 'tools' / 'bin' / 'bun-1.4.2'} "
            "(missing? fetch the bun v1.4.2 linux-x64 zip from bun-sh/bun "
            "tag bun-v1.4.2 and extract `bun` there, chmod +x).")
    version = version_fn(bun)
    if version is None or version < BUN_FLOOR:
        raise SystemExit(
            f"infra failure: TUI_DIFF_BUN={bun} is {version or 'unrunnable'}; "
            f"plugin scenarios need bun >= {'.'.join(map(str, BUN_FLOOR))} "
            "(ResolveMessage instanceof Error — the ref host.ts:17-43 swallow "
            "guard's dependency, controller-confirmed root cause of the B2 "
            "first-round whole-screen pseudo-diffs).")


def _point_plugins_link(stage: Path, root_name: str) -> None:
    """Repoint STAGE/plugins at one staged root (the G-424b canonical
    path: both legs' config targets read the same string while each
    resolves into its own tree)."""
    link = stage / "plugins"
    if link.is_symlink() or link.exists():
        if link.is_symlink() or not link.is_dir():
            link.unlink()
        else:
            raise SystemExit(f"{link}: exists and is not a symlink")
    os.symlink(root_name, link)


def run(stage: Path, ref: Path, cases: list[dict]) -> tuple[dict[str, dict], dict[str, dict]]:
    """Drive both sides over the staged manifest (cases from the
    already-run build_manifest) -> (ref grids, rust grids). B2-fix:
    the ref leg splits — non-plugin cases run under the environment
    bun (baseline no-drift), plugin cases under TUI_DIFF_BUN (version
    gate: >= 1.4.2, else a pointed infra error)."""
    manifest = stage / "manifest.json"
    ref_out = stage / "ref-out"
    rust_out = stage / "rust-out"
    for out in (ref_out, rust_out):
        out.mkdir(exist_ok=True)
        os.chmod(out, 0o1777)

    plugin_cases = [c for c in cases if c.get("plugins")]
    plain_cases = [c for c in cases if not c.get("plugins")]
    if plugin_cases:
        check_bun_gate(os.environ.get("TUI_DIFF_BUN", "").strip() or None)
        bun = os.environ["TUI_DIFF_BUN"].strip()
        plugin_manifest = stage / "manifest-plugins.json"
        plugin_manifest.write_text(json.dumps(
            {"schema": "tui-diff-manifest/1", "cases": plugin_cases}, indent=1))
        _point_plugins_link(stage, "plugins-ref")
        run_ref(stage, ref, plugin_manifest, ref_out, bun=bun)
    if plain_cases:
        plain_manifest = stage / "manifest-plain.json"
        plain_manifest.write_text(json.dumps(
            {"schema": "tui-diff-manifest/1", "cases": plain_cases}, indent=1))
        run_ref(stage, ref, plain_manifest, ref_out)
    _point_plugins_link(stage, "plugins-rust")
    run_rust(manifest, rust_out)
    ref_grids, ref_unavail = load_side(ref_out, cases, "ref")
    rust_grids, rust_unavail = load_side(rust_out, cases, "rust")
    del ref_unavail, rust_unavail  # consistency ruled in reconcile_plugin_skips
    return ref_grids, rust_grids


# ---------------------------------------------------------------------------
# normalization + compare + gate (T5)
# ---------------------------------------------------------------------------

# keys stripped by normalize(): any time-ish field a driver might emit
# alongside the resting frame (none in the current drivers, but the hook
# exists so future instrumentation cannot perturb the diff)
TIME_FIELD_PREFIXES = ("captured_at", "driver_time", "timestamp", "elapsed")

# G-368 structural tolerance: RGB channels may differ by at most this much
RGB_TOLERANCE = 8


def normalize(grid: dict) -> dict:
    """Return a normalized copy of a `tui-diff-grid/1` payload: time
    fields stripped (hook — see TIME_FIELD_PREFIXES), cells made dense
    with canonical defaults for missing/None subfields."""
    out = {k: v for k, v in grid.items()
           if not k.startswith(TIME_FIELD_PREFIXES)}
    cells = []
    for row in grid.get("cells") or []:
        crow = []
        for cell in row:
            cell = cell or {}
            crow.append({
                "ch": cell.get("ch") or " ",
                "attrs": sorted(set(cell.get("attrs") or [])),
                "fg": list(cell.get("fg") or [-1, -1, -1]),
                "bg": list(cell.get("bg") or [-1, -1, -1]),
            })
        cells.append(crow)
    out["cells"] = cells
    return out


def _shape(cells: list) -> str:
    """Compact rows x cols-of-first-row shape string for cells_shape diffs."""
    if not isinstance(cells, list):
        return "non-list"
    cols = {len(r) for r in cells if isinstance(r, list)}
    return f"{len(cells)}x{','.join(str(c) for c in sorted(cols)) or '?'}"


def _first_cell_diff(ref: dict, rust: dict) -> dict | None:
    """First differing field between two normalized cells, in a fixed
    field order (ch, attrs, fg, bg); None when equal."""
    for field in ("ch", "attrs", "fg", "bg"):
        if ref[field] != rust[field]:
            return {"field": field, "ref": ref[field], "rust": rust[field]}
    return None


def _within_tolerance(a: list, b: list) -> bool:
    """RGB closeness: every channel differs by <= RGB_TOLERANCE.
    Sentinels (negative values) never match a concrete RGB."""
    if len(a) != len(b):
        return False
    for x, y in zip(a, b):
        if x < 0 or y < 0:
            return False
        if abs(x - y) > RGB_TOLERANCE:
            return False
    return True


def compare(ref_grid: dict, rust_grid: dict) -> dict:
    """Compare two normalized grids -> verdict + first_diff + count.

    verdict:
      exact      — every cell identical
      structural — char/attrs identical everywhere, only RGB differs
                   and every differing channel is within RGB_TOLERANCE
                   (G-368; STILL a divergence, never an exemption)
      diverge    — anything else (char, attrs, sentinel-vs-RGB,
                   out-of-tolerance RGB, size mismatch)

    first_diff: {row, col, field, ref, rust} of the first differing
    cell in row-major order (None for exact).

    Defensive only: size or cells-shape mismatches diverge loudly
    rather than being silently truncated by zip() — unreachable in
    practice because load_side() already rejects any grid whose
    declared size or cells shape is inconsistent (infra, exit 1).
    """
    result: dict = {"verdict": "exact", "first_diff": None, "cells_diff_count": 0}
    rw, rh = ref_grid.get("width"), ref_grid.get("height")
    uw, uh = rust_grid.get("width"), rust_grid.get("height")
    if (rw, rh) != (uw, uh):
        return {
            # defensive, unreachable after load validation (load_side
            # red-flags size mismatches vs manifest on both sides)
            "verdict": "diverge",
            "first_diff": {"row": None, "col": None, "field": "size",
                           "ref": f"{rw}x{rh}", "rust": f"{uw}x{uh}"},
            "cells_diff_count": rw * rh if rw and rh else 0,
        }
    ref_cells = ref_grid.get("cells") or []
    rust_cells = rust_grid.get("cells") or []
    if (len(ref_cells) != len(rust_cells)
            or any(len(a) != len(b) for a, b in zip(ref_cells, rust_cells))):
        # defensive, unreachable after load validation (load_side
        # requires a dense width x height cells array)
        return {
            "verdict": "diverge",
            "first_diff": {"row": None, "col": None, "field": "cells_shape",
                           "ref": _shape(ref_cells), "rust": _shape(rust_cells)},
            "cells_diff_count": max(len(ref_cells), len(rust_cells)),
        }
    only_rgb = True
    diff_count = 0
    first: dict | None = None
    for r, (rrow, urow) in enumerate(zip(ref_cells, rust_cells)):
        for c, (rc, uc) in enumerate(zip(rrow, urow)):
            fd = _first_cell_diff(rc, uc)
            if fd is None:
                continue
            diff_count += 1
            if first is None:
                first = {"row": r, "col": c, **fd}
            if fd["field"] in ("ch", "attrs"):
                only_rgb = False
            elif fd["field"] in ("fg", "bg"):
                if not _within_tolerance(fd["ref"], fd["rust"]):
                    only_rgb = False
    if diff_count == 0:
        return result
    return {
        "verdict": "structural" if only_rgb else "diverge",
        "first_diff": first,
        "cells_diff_count": diff_count,
    }


# ---------------------------------------------------------------------------
# A2 frame-sequence comparison (anim dimension)
# ---------------------------------------------------------------------------

def compare_frames(ref_grid: dict, rust_grid: dict) -> dict:
    """Sequence-compare the `frames` arrays (both validated by
    load_side: K+1 dense grids) -> per-tick verdicts + first divergent
    tick localization {tick,row,col,field,ref,rust}.

    verdict (overall): exact — every tick exact; otherwise the worst
    per-tick form (diverge > structural), mirroring compare()'s
    semantics applied per tick.
    """
    ref_frames = ref_grid.get("frames") or []
    rust_frames = rust_grid.get("frames") or []
    # Empty-sequence defense (review #1): an empty frames array on
    # either side must NEVER fold to "exact" (all() over [] is True).
    # load_side already red-flags this as infra, but compare_frames
    # stays defensive on its own — empty ≠ exact, it is a length
    # divergence.
    if not ref_frames or not rust_frames or len(ref_frames) != len(rust_frames):
        # (load_side already enforces K+1 on both sides — this is the
        # same defensive branch for length asymmetry: zip() must never
        # silently truncate a longer side into an "exact" prefix)
        return {
            "dimension": "frames",
            "verdict": "diverge",
            "per_tick": [],
            "first_tick_diff": {"tick": None, "row": None, "col": None,
                                "field": "frames_length",
                                "ref": len(ref_frames), "rust": len(rust_frames)},
            "cells_diff_count": max(len(ref_frames), len(rust_frames), 1),
        }
    per_tick = []
    first: dict | None = None
    for tick, (rf, uf) in enumerate(zip(ref_frames, rust_frames)):
        wrapped_r = {"width": ref_grid.get("width"), "height": ref_grid.get("height"), "cells": rf}
        wrapped_u = {"width": rust_grid.get("width"), "height": rust_grid.get("height"), "cells": uf}
        res = compare(normalize(wrapped_r), normalize(wrapped_u))
        per_tick.append(res["verdict"])
        if res["verdict"] != "exact" and first is None:
            fd = res["first_diff"] or {}
            first = {"tick": tick, **fd}
    verdict_all = "exact" if all(v == "exact" for v in per_tick) else (
        "diverge" if "diverge" in per_tick else "structural")
    return {
        "dimension": "frames",
        "verdict": verdict_all,
        "per_tick": per_tick,
        "first_tick_diff": first,
        "cells_diff_count": sum(1 for v in per_tick if v != "exact"),
    }


# ---------------------------------------------------------------------------
# CAUSES adjudication table (T6) + gate
# ---------------------------------------------------------------------------

# Known-cause annotations (dual file:line), channel-7 lambda style:
# (label, matcher(divergence_record)->bool, (定性文, ref file:line, rust file:line)).
# A divergence_record is {"id","dimension","verdict","first_diff"/
# "first_tick_diff","cells_diff_count","per_tick"?}. Matchers are written
# against EXACT shapes (scenario-id prefix + precise divergence form);
# anything broader would swallow unknown divergences and fake a green
# gate.  Unknown divergences stay unadjudicated (T9's job).
CAUSES = [
    (
        "G-368 量化色（oklab 最近邻量化差）",
        lambda d: d.get("dimension", "resting") == "resting"
        and d.get("verdict") == "structural",
        (
            "仅 RGB 差且每通道 ≤RGB_TOLERANCE(8)、char/attrs 全等：双侧滚动区主题"
            "各经一次 oklab 感知最近邻量化（同一算法、不同浮点环境），量化格点"
            "选择可相邻一格——结构性色彩差，非语义差",
            "ref packages/tui/src/mini/theme.ts:81-105（oklab/nearestIndexed），"
            "经 mini/theme.ts:117-125 V1 序列化恢复后量化",
            "rust crates/oc-tui/src/mini/theme.rs:114-192（oklab 量化 "
            "nearest_indexed/scrollback_color，1:1 移植）",
        ),
    ),
    (
        "G-128 midframe 簇（spring/tween 未移植——中间拍属性位置差，末拍收敛）",
        lambda d: d.get("dimension") == "frames"
        and _midframe_shape(d) == "converging",
        (
            "anim 场景的帧序列差：首拍（静息帧）与末拍逐格一致、仅中间拍出现"
            "ch/attrs 属性差——ref 弹簧/补间（ui/animation.ts advanceSpring）"
            "过渡的中间帧位置 vs rust 已到位（无过渡引擎，终态直渲染）的"
            "已立项过渡差；末拍收敛 ⇒ 登记不拦；末拍也差的不是 midframe 族，"
            "不命中本簇（拦，T9）",
            "ref packages/tui/src/ui/animation.ts:1-60（advanceSpring，未移植）",
            "rust crates/oc-tui/src/ 全树无 spring/tween（G-128 登记面）；"
            "rust 渲染为 (store, clock) 纯函数（A1，store.rs epoch/clock 接线）",
        ),
    ),
    (
        "G-49/G-368 节奏族（中间拍量化色差，末拍收敛）",
        lambda d: d.get("dimension") == "frames"
        and _midframe_shape(d) == "rhythm",
        (
            "anim 场景的帧序列差：首拍与末拍一致、仅中间拍 structural（RGB ≤8、"
            "char/attrs 全等）——过渡帧的 oklab 量化格点相邻差（G-368 同因）"
            "或已登记的 repaint 节奏差（G-49，帧率文档化决策）在中间拍的"
            "采样差；末拍收敛 ⇒ 登记不拦",
            "ref packages/tui/src/mini/theme.ts:81-105（量化）+ 组件 rAF 帧节奏",
            "rust crates/oc-tui/src/app.rs:10138-10166（100ms 泵粒度，SCHEMA "
            "ANIM_TICK_MS 对齐）",
        ),
    ),
    (
        "G-423 多 text part 内联拼接渲染差（anim-stream-multi-part）",
        lambda d: str(d.get("id", "")).startswith("anim-stream-multi-part")
        and d.get("dimension") in ("frames", "resting", None),
        (
            "anim-stream-multi-part 场景：同一 assistant 消息的两个 text part "
            "（ordinal 0/1）交错流式 delta——ref 将两 part 的文本内联拼接到同一行"
            "（'Alpha Beta part'），且双 text.ended 后仅显示 part 1 终文；"
            "rust 按分段段落渲染（两个独立块，终态两段完整）。A2 帧序列维度"
            "新发现的渲染语义差，登记（G-423），不拦",
            "ref packages/tui/src/routes/session/message-parts.tsx:198-275"
            "（InlineToolRow/多 part 段落装配）",
            "rust crates/oc-tui/src/components/message/rows.rs:120-135"
            "（subagent/多 part 行装配，分段段落）",
        ),
    ),
    (
        "ref 命令面板分组序竞态（groupBy 先到先得，跨运行可翻转）",
        lambda d: str(d.get("id", "")).startswith("command-palette")
        and d.get("dimension", "resting") == "resting"
        and (d.get("first_diff") or {}).get("field") == "ch",
        (
            "command-palette 场景的 ch 字段差（面板分组顺序差）：ref 的分组序"
            "由 `groupBy(category).entries()` 的首现顺序决定（dialog-select."
            "tsx:214-226），而命令选项来自多个异步注册源（app 内建/会话命令/"
            "/api/command/skills）——注册完成序跨运行可翻转（A2 复跑取证："
            "5 次单跑 1 次 Prompt↔Session 翻转；rust 侧确定性注册序恒 "
            "Session 组在前）。ref 树只读不可修，登记为通道 8 已知 ref 侧"
            "竞态，非移植语义差",
            "ref packages/tui/src/ui/dialog-select.tsx:214-226（groupBy/"
            "entries 首现序）+ 多源异步注册（routes/session/index.tsx "
            "commands 装配）",
            "rust crates/oc-tui/src/components/command_palette.rs（确定性"
            "分组装配，无竞态）",
        ),
    ),
    (
        # RETIRED (gb17 wave 4, 2026-11-16): the G-424b title-wrap
        # residual is FIXED — the toast title now shrinks and
        # word-wraps inside the maxWidth-60 panel (toast_wrap: the
        # global break set PLUS '/', evidenced by the ref grids'
        # "…/node_modules/" line ending), the geometry clamp includes
        # the rails, panel rows grow with the wrapped lines, the
        # dialog (z-3000 in the ref) covers the toast's overlapped
        # rows and its scrim dims the rail glyphs (ToastRailCover),
        # and the /plugins dialog row carries the failed gutter `x`
        # with the ref's footer-prop/hint pairs. plugin-fallback all
        # three tiers exact in resting — 0 hits, cluster removed
        # (the G-424 retirement precedent). A NEW toast-shape
        # divergence requires a NEW CAUSES entry with its own probe
        # evidence.
        "G-424b toast 长标题折行未移植（rust 单行截断 vs ref 标题 shrink+wrap）— RETIRED",
        lambda d: False,
        (
            "（已关闭）plugin-fallback 三档 exact（gb17 W4，2026-11-16）",
            "ref packages/tui/src/ui/toast.tsx:46-99（maxWidth 整盒钳制 + 标题默认 wrap）",
            "rust crates/oc-tui/src/components/toast.rs toast_layout/toast_wrap"
            " + toast_solid.rs ToastRailCover",
        ),
    ),
    # RETIRED (gb17 tool-glyph, 2026-11-16): the A2 animation-gate
    # artifact cluster ("A2 ref 动画门控夹具伪影") is REMOVED — the
    # animations gate is now PORTED: the store carries
    # `config.animations ?? true` (spinner.tsx:29-31's gate; default
    # true — the production face), the diff driver mirrors the ref
    # fixture's resolved `animations:false` (test/fixture/app.ts:37),
    # and every running-row indicator (tool rows, the shell block's
    # running command, the reasoning "Thinking" header, the compaction
    # divider) renders the static `⋯` fallback under the OFF gate.
    # The evidenced shape (rust clock-driven ⠋ vs the fixture's ⋯ on
    # anim-tool-progress's running-tool row) is exact in BOTH
    # dimensions — resting 3/3 + frames 3/3, 0 hits (the G-424
    # retirement precedent). A NEW artifact shape (caret blink, knight
    # gradient, spinner rotation once a stable ref clock exists)
    # requires a NEW CAUSES entry with its own probe evidence.
    (
        # B2 CAUSES version-skew 簇 (user ruling 2026-11-14): residual
        # 2.0.24-vs-2.2.x PLUGIN BEHAVIOR diffs — local shapes where the
        # payload's newer-host API assumptions degrade on the 2.0.24
        # face (evidenced shape: omo-slim's tui entry, once loaded,
        # degrades the ref's "/" slash-palette path — probed 2026-11-15;
        # the plugin never reaches its ctx.client/… API face otherwise).
        # The FIRST-round whole-screen mechanism (bun 1.3.10's
        # ResolveMessage not being an Error subclass → host.ts rethrow
        # → deterministic "Plugin failed" state) was controller-
        # confirmed and is now GATED AWAY (TUI_DIFF_BUN >= 1.4.2);
        # under the gate both payloads boot on the ref side, so this
        # cluster only matches plugin-* cases whose REF grid still
        # shows the failure notice — i.e. a REAL un-bootable payload,
        # never the deliberate plugin-fallback fixture.
        "B2 version-skew 簇（退版本加载伪差：插件 2.2.x API 面在 ref 2.0.24 降级/不可 boot）",
        lambda d: d.get("ref_plugin_failed") is True
        and str(d.get("id", "")).startswith("plugin-")
        and not str(d.get("id", "")).startswith("plugin-fallback"),
        (
            "plugin-* 场景且 ref 侧网格呈现失败态（'Plugin failed: <path>' 公告）：在 "
            "TUI_DIFF_BUN≥1.4.2 门控下（首轮整屏伪差根因= bun 1.3.10 ResolveMessage "
            "非 Error 子类、host.ts:17-43 吞错守卫重抛——控制者实验实锤，已门控隔离）"
            "仍失败的载荷=其 2.2.x 宿主 API 假设在 2.0.24 面上不可满足——退版本加载"
            "伪差而非移植 bug，rust 侧 boot 出的渲染整体登记不拦；判定文须引插件"
            "调用点与 2.0.24 API 面缺失符号（首轮实证附加形状：omo-slim tui 入口"
            "加载后 ref 的 '/' 斜杠面板路径被插件降级破坏）。当前语料 0 命中"
            "（双插件在门控下均可 boot）——簇保留接未来真不可 boot 载荷",
            "ref packages/plugin/src/host.ts:17-43（吞错守卫；bun≥1.4.2 依赖）+ "
            "packages/tui/src/plugin/context.tsx:478-484（失败态公告）",
            "rust crates/oc-plugin-host/src/resolve.rs:249-289（包感知 resolve_entry）"
            "+ supervisor.rs:2359（load.tui 伴生装载）——rust 侧正常 boot",
        ),
    ),
    (
        # RETIRED (gb17 wave 4, 2026-11-16): the loading-model
        # asymmetry is closed at the HARNESS boundary (the controller
        # ruling's retirement condition — driver-side mirror of the
        # ref fixture's loading face): the rust diff driver boots the
        # host with `tui_only: true` (oc-plugin-host's
        # PluginHostConfig), so each staged package loads ONLY its
        # tui entry through the TUI-companion op — exactly the ref
        # TUI fixture's face (context.tsx:670 `Host.resolve(target)
        # .tui`; the server entry runs in the ref's SERVER process,
        # which the fixture has none of). Both sides now boot the
        # SAME entry set: plugin-boot-*/plugin-fallback/plugin-
        # panel-open all exact at 80/120; the 320x80 tier's residual
        # is the bounded sidebar shim (the payload's own render under
        # the bridge walk drops the blank spacer + remote-hydrated
        # `Preset zhipuai ▸` row) — the G-28 shape face the original
        # entry predicted ("320×80 档 … G-28 形状面共现"), now
        # CAUGHT by G-28. 0 hits on this cluster, removed. Production
        # loading is UNCHANGED (default tui_only=false: the unified
        # process still loads server+tui).
        "B2 插件注册投影（rust 统一进程 server-entry 装载 vs ref TUI 仅 tui 入口面）— RETIRED",
        lambda d: False,
        (
            "（已关闭）driver 装配镜像 ref 夹具装载面（tui_only），残余 sidebar 形状归 G-28",
            "ref packages/tui/src/plugin/context.tsx:670（仅 tui 入口装载）",
            "rust crates/oc-plugin-host/src/lib.rs PluginHostConfig::tui_only"
            " + supervisor.rs load_entry（tui_only 分支）+ oc-tui diff driver 装配",
        ),
    ),
    (
        # RETIRED (gap-burndown-16 W2, 2026-11-15): G-425 fixed — the
        # /plugins dialog now lists the boot-failure rows (store
        # plugin_boot_failures ← surface_boot_failures, footer
        # "failed, local" + "enter view error" hint + error-details
        # submit). The plugin-fallback residual is NOT this cluster's
        # shape anymore — it moved to the narrowed G-424b toast
        # title-wrap cluster above. Original prefix matcher removed
        # per B2 review Important (plugin-fallback* swallowed every
        # divergence in the scenario).
        "G-425 插件失败态管理屏不列失败入口（已修复——零命中保留注释）",
        lambda d: False,
        (
            "（已关闭）plugin-fallback 场景的残余归 G-424b toast 长标题折行簇",
            "ref packages/tui/src/plugin/context.tsx:460-484 + "
            "feature-plugins/system/plugins.tsx:54-76",
            "rust crates/oc-tui/src/plugin_boot.rs surface_boot_failures + "
            "components/command_palette.rs DialogKind::Plugins（本轮修复）",
        ),
    ),
    (
        # RETIRED (gb17 wave 5, 2026-11-16): the wide-tier sidebar
        # bridge-walk residual is FIXED and the channel is ALL-EXACT
        # for the first time — committed 90/90 + frames 15/15 + rec-
        # 9/9, CAUSES hits 0. Two-part fix:
        # (1) the bridge walk models the ref box's VERTICAL metrics
        #     (host/host.js treeLines: a column box's border +
        #     paddingTop/Bottom rows and per-child marginTop model as
        #     blank lines; flattenRenderTree keeps every line as a
        #     row — the old blank-dropping filter shifted the whole
        #     section up, [blank,blank,header,agents…] collapsed to
        #     [header,agents…]);
        # (2) harness hermeticity: both legs pin XDG_CONFIG_HOME to
        #     one EMPTY stage dir — the flagship payload reads the
        #     DRIVER USER's personal ~/.config/opencode/
        #     oh-my-opencode-slim.json (tui2.js getDefaultOpenCode-
        #     ConfigDir) for its preset row, an environment accident
        #     that leaked into whichever leg ran as that user.
        # The cluster's full history: the original plugin-* prefix
        # matcher (the bounded TSX-slot face) was narrowed to this
        # single case at gb17-W4 review; with the case exact the
        # family is CLOSED — a NEW plugin-shape divergence is
        # unadjudicated and GATES RED (the conservative-correct
        # policy the W4 narrowing set).
        "G-28 插件 TSX 有界（bounded shim）——已修复关簇（gb17 W5 桥接 walk 空行模型 + 夹具密闭化）",
        lambda d: False,
        (
            "（已关闭）plugin-panel-open@320x80 exact；通道首次全一致（90/90+15/15+rec- 9/9，CAUSES 0）",
            "ref packages/tui/src/component/panel-host.tsx:35-55（Slot path="
            "\"session.panel\"，TSX 活树 + reactive props）",
            "rust crates/oc-plugin-host/src/host/host.js treeLines/"
            "flattenRenderTree（盒垂向度量→空行模型）+ tools/diff/"
            "tui_fullscreen.py hermetic_config_home（双侧 XDG 密闭）",
        ),
    ),
    (
        "G-332 markdown 有界行形状",
        lambda d: d.get("dimension", "resting") == "resting"
        and str(d.get("id", "")).startswith("session-markdown-")
        and (d.get("first_diff") or {}).get("field") == "ch",
        (
            "session-markdown-* 场景且 ch 字段差（行形状分歧）：ref 经 markdown "
            "渲染器 memo 重建整块，rust 经分派表逐行缓存渲染——折行/代码块"
            "边界行的有界形状差",
            "ref packages/tui/src/plugin/markdown.ts:7-18（createMarkdownRenderer "
            "memo，renderNode 变更整块重建）",
            "rust crates/oc-tui/src/plugin_markdown.rs:82-147（分派表 install_table/"
            "cached_rows/spawn_render 逐行缓存）",
        ),
    ),
    (
        "G-239/G-331 panel 有界（PanelFingerprint）",
        lambda d: d.get("dimension", "resting") == "resting"
        and str(d.get("id", "")).startswith("panel-"),
        (
            "panel-* 场景：PanelInput 的 focus()/close()/toggleFullscreen() "
            "闭包无法过线，rust 以 PanelFingerprint 快照近似 reactive props——"
            "已立项的有界偏差",
            "ref packages/tui/src/component/panel-host.tsx:36-55（Slot input 的 "
            "reactive width/presentation/focused + 闭包 props）",
            "rust crates/oc-tui/src/plugin_boot.rs:100（PanelFingerprint）、"
            ":425-440（session.panel props 有界化）、:767-780（fingerprint 快照）",
        ),
    ),
    # G-426 residual matcher RETIRED (gb17 wave 3 re-root-cause,
    # 2026-11-16): the old entry pinned rec-20261114-205531-bb7f1e74's
    # mirrored tiers as "opentui-core marked@17 lexical double defect
    # (empty-list interruption + setext underline)". The controller
    # falsified that attribution (upstream marked@17 fixes landed at
    # 12658bc + rev bump ⇒ zero rust output change with confirmed
    # recompiles). The REAL root cause was the missing top-level
    # markdown block/margin model (ref index.bun.js
    # buildTopLevelRenderBlocks/shouldAddTopLevelMargin): rust rendered
    # the top level through the scrollback coalescing path, gluing
    # e.g. an empty-list-item block onto the preceding paragraph and
    # dropping the one-row inter-block margins (plus the empty-item
    # marker row and the exploration group's every-tool-completed
    # rule). Fixed in opentui-core a9160cc
    # (build_markdown_blocks_top_level) + oc-tui (markdown.rs
    # top-level switch, empty-item marker row, list.rs
    # tools_all_complete); all nine rec- tiers render exact, so the
    # matcher has no shape left to match. See GAPS G-426 for the
    # falsification note.
]


def _ref_cells_text(grid: dict) -> str:
    """Row-joined plain text of a grid's cells (evidence extraction —
    the ref 'Plugin failed' failure notice is screen text)."""
    chunks = []
    for row in grid.get("cells") or []:
        chunks.append("".join(str(c.get("ch") or " ") if isinstance(c, dict) else " "
                              for c in row))
    return "\n".join(chunks)


def _plugin_failed_on_ref(ref_grid: dict) -> bool:
    """B2 version-skew evidence predicate: the ref 2.0.24 harness
    could not boot the plugin AT ALL (the deterministic failure state:
    the 'Plugin failed: <path>' notice + the 'N plugins failed' footer
    pill + the Plugins screen's failed rows)."""
    return "Plugin failed" in _ref_cells_text(ref_grid)


# NOTE (gb17, 2026-11-16): `_first_glyph_diff` was removed together with
# the retired A2 glyph cluster (no remaining callers).


def _midframe_shape(d: dict) -> str | None:
    """Classify a frames-dimension record: 'converging' (tick 0 exact,
    last tick exact, ≥1 non-exact middle tick with a ch/attrs diff),
    'rhythm' (same but every non-exact middle tick is structural), else
    None (NOT a midframe family — e.g. the last tick also diverges:
    those block, per the A2 brief)."""
    per_tick = d.get("per_tick") or []
    if len(per_tick) < 3:
        return None
    if per_tick[0] != "exact" or per_tick[-1] != "exact":
        return None
    mid = per_tick[1:-1]
    if all(v == "exact" for v in mid):
        return None
    if any(v == "diverge" for v in mid):
        return "converging"
    return "rhythm"


def adjudicate(divergence_record: dict) -> str | None:
    """First matching CAUSES label for a divergence record, else None
    (unadjudicated — a T9 candidate, never silently green)."""
    for label, match, _ in CAUSES:
        if match(divergence_record):
            return label
    return None


def verdict(results: list[dict], causes: list = ()) -> int:
    """Gate: 0 when every case is adjudicated (exact, or non-exact but
    covered by a CAUSES matcher); 2 when any case diverges without a
    known cause.  `results` records carry the case id (the CAUSES
    matchers key on scenario-id prefixes).  Infra failures never reach
    here (they exit 1 earlier).

    Controller ruling (2026-11-15, C3): gate scope is the COMMITTED
    corpus only — recorded-local cases (id prefix `rec-`) are ADVISORY:
    their unadjudicated divergences stay in the report's recorded-local
    section (C-line root-cause input) but never redden the release
    gate (daily recordings routinely carry open adjudications;
    committed green is gate green).  A rec- INFRA failure still exits
    1 — that is the replay mechanism itself being broken, and it is
    enforced earlier by load_side/driver plumbing regardless of id.
    """
    for r in results:
        if r.get("verdict") == "exact":
            continue
        if any(match(r) for _, match, _ in causes):
            continue
        if str(r.get("id", "")).split("@")[0].startswith("rec-"):
            continue  # advisory (controller ruling above)
        return 2
    return 0


# ---------------------------------------------------------------------------
# report
# ---------------------------------------------------------------------------

def _rec_with_flag(cid: str, res: dict, ref: dict[str, dict]) -> dict:
    """A divergence record with the B2 version-skew evidence flag
    attached for plugin-* cases (ref-side 'Plugin failed' screen)."""
    rec = {"id": cid, **res}
    if cid.startswith("plugin-"):
        rec["ref_plugin_failed"] = _plugin_failed_on_ref(ref[cid])
    return rec


def _adj_report(cid: str, res: dict, ref: dict[str, dict]) -> str | None:
    return adjudicate(_rec_with_flag(cid, res, ref))


def write_report(path: Path, cases: list[dict], ref: dict[str, dict], rust: dict[str, dict],
                 results: dict[str, dict], frame_results: dict[str, dict],
                 skipped: list[str] | None = None) -> None:
    lines = []
    # --- ① header: replay line + both driver surfaces (file:line) ---
    lines.append("# TUI-FULLSCREEN — TUI 整屏差分（通道 8）")
    lines.append("")
    lines.append("同一 `tui-diff-scenario/1` 场景（tools/diff/corpus/tui/SCHEMA.md）喂双侧 TUI 实现，")
    lines.append("静止帧导出为 `tui-diff-grid/1` 稠密网格后归一化比对。判门：exact /")
    lines.append("structural（G-368 RGB ≤8 容差，仍是分歧）/ diverge；CAUSES 已知因定性（本表），")
    lines.append("adjudication 属 T9——committed 语料未命中 CAUSES 的一切非 exact 均未裁决 → exit 2")
    lines.append("（recorded-local `rec-*` 未裁决=advisory，见其小节；控制者裁定 2026-11-15）。")
    lines.append("")
    lines.append("> 复现：python3 tools/diff/tui_fullscreen.py --write docs/audit/diff/TUI-FULLSCREEN.md")
    lines.append("")
    lines.append("- rust 侧: `cargo nextest -p oc-tui --test tui_fullscreen_diff_driver`（env 门控")
    lines.append("  TUI_DIFF_MANIFEST/TUI_DIFF_OUT，dev-gate 身份）。驱动面:")
    lines.append("  crates/oc-tui/tests/tui_fullscreen_diff_driver.rs:1")
    lines.append("- ref 侧: `bun --preload @opentui/solid/scripts/preload.js")
    lines.append("  tools/diff/drivers/tui_fullscreen_ref.ts`（staged 到 STAGE，DIFF_REF_ROOT")
    lines.append("  指向 ref 树，只读）。驱动面: tools/diff/drivers/tui_fullscreen_ref.ts:1")
    lines.append("- 编排器: tools/diff/tui_fullscreen.py:1（manifest/staging/infra 判门/比对/本报告）")
    lines.append("")
    # --- ② normalization declaration ---
    lines.append("归一化声明: time* 钩子字段剥离（captured_at/driver_time/timestamp/elapsed）；")
    lines.append("cells 稠密化，None/缺省子字段补规范默认（ch=' '、attrs 排序去重、fg/bg=[-1,-1,-1]）；")
    lines.append("比较字段序 ch→attrs→fg→bg；RGB 每通道 |Δ|≤8 为 structural（G-368，仍计分歧）；")
    lines.append("哨兵负值 vs 具体 RGB 一律 diverge；request_census 不参与比对（双侧 boot 请求模式")
    lines.append("已知不对称，信息性记录）；stub_gap 检测到即 infra 红（SCHEMA 规则）。")
    lines.append("")
    # --- ③ counts table (COMMITTED corpus scope — the gate's scope;
    # recorded-local rec-* cases are counted in their own section) ---
    committed = {cid: res for cid, res in results.items()
                 if not cid.split("@")[0].startswith("rec-")}
    n_exact = sum(1 for r in committed.values() if r["verdict"] == "exact")
    n_struct = sum(1 for r in committed.values() if r["verdict"] == "structural")
    n_div = sum(1 for r in committed.values() if r["verdict"] == "diverge")
    causes_hit: dict[str, list[str]] = {}
    n_unadj = 0
    for cid, res in committed.items():
        if res["verdict"] == "exact":
            continue
        label = _adj_report(cid, res, ref)
        if label is None:
            n_unadj += 1
        else:
            causes_hit.setdefault(label, []).append(cid)
    n_caused = sum(len(v) for v in causes_hit.values())
    lines.append("## 计数（gate 口径：committed 语料）")
    lines.append("")
    lines.append("| 总量 | EXACT | STRUCTURAL | CAUSES 命中 | 未裁决 |")
    lines.append("|---|---|---|---|---|")
    lines.append(f"| {len(committed)} | {n_exact} | {n_struct} | {n_caused} | {n_unadj} |")
    lines.append("")
    if skipped:
        lines.append(f"B2 plugins_unavailable 一致跳过 {len(skipped)} case（双侧标记一致，"
                     "SCHEMA B2 skip 语义；payload/bun 环境缺失，非差分结论）："
                     + ", ".join(f"`{c}`" for c in sorted(skipped)) + "")
    lines.append(f"（verdict 分布：exact {n_exact} / structural {n_struct} / diverge {n_div}；")
    lines.append("committed 未裁决>0 ⇒ exit 2；rec-* recorded-local 见其小节（advisory，不入门））")
    lines.append("")
    # --- ④ per-case divergence detail ---
    lines.append("## 分歧明细")
    lines.append("")
    # diff-ext C2 — recorded-local cases are reported as AGGREGATES ONLY
    # (privacy iron law: no recorded content — not even diff cell values
    # — is quoted in reports); their per-case rows live in the
    # recorded-local section below.
    table_cases = [c for c in cases if not c["id"].split("@")[0].startswith("rec-")]
    rec_cases = [c for c in cases if c["id"].split("@")[0].startswith("rec-")]
    lines.append("| case | verdict | diff cells | first_diff | ref size | rust size | ref gaps | rust gaps | ref paths | rust paths |")
    lines.append("|---|---|---|---|---|---|---|---|---|---|")
    for case in table_cases:
        cid = case["id"]
        r, u, res = ref[cid], rust[cid], results[cid]
        fd = res["first_diff"]
        fd_s = "—" if fd is None else (
            f"r{fd['row']}c{fd['col']} {fd['field']} ref={fd['ref']!r} rust={fd['rust']!r}"
            if fd.get("row") is not None else f"{fd['field']} {fd['ref']} vs {fd['rust']}")
        lines.append(
            f"| {cid} | {res['verdict']} | {res['cells_diff_count']} | {fd_s} "
            f"| {r.get('width')}x{r.get('height')} | {u.get('width')}x{u.get('height')} "
            f"| {len(r.get('stub_gaps') or [])} | {len(u.get('stub_gaps') or [])} "
            f"| {len(r.get('request_census') or {})} | {len(u.get('request_census') or {})} |"
        )
    lines.append("")
    if n_struct or n_div:
        lines.append("### first_diff 明细")
        lines.append("")
        for case in table_cases:
            cid = case["id"]
            res = results[cid]
            if res["verdict"] == "exact":
                continue
            fd = res["first_diff"]
            if fd is None:
                continue
            cause = _adj_report(cid, res, ref)
            cause_s = f" — CAUSES: {cause}" if cause else " — 未裁决（T9 候选）"
            if fd.get("row") is None:
                lines.append(f"- `{cid}` [{res['verdict']}] {fd['field']}: "
                             f"ref={fd['ref']} rust={fd['rust']} "
                             f"cells_diff_count={res['cells_diff_count']}{cause_s}")
            else:
                lines.append(
                    f"- `{cid}` [{res['verdict']}] row={fd['row']} col={fd['col']} "
                    f"field={fd['field']} ref={fd['ref']!r} rust={fd['rust']!r} "
                    f"cells_diff_count={res['cells_diff_count']}{cause_s}")
        lines.append("")
    # --- ⑤ CAUSES adjudication ---
    lines.append("## CAUSES 定性")
    lines.append("")
    for label, _match, (desc, ref_loc, rust_loc) in CAUSES:
        hits = causes_hit.get(label, [])
        lines.append(f"- **{label}** — 命中 {len(hits)} case"
                     + (f"（{', '.join(f'`{c}`' for c in hits)}）" if hits else ""))
        lines.append(f"  - {desc}")
        lines.append(f"  - {ref_loc}")
        lines.append(f"  - {rust_loc}")
    lines.append("")
    lines.append("未命中任何簇的分歧保持未裁决进入 T9；CAUSES 不为绿灯放宽 matcher。")
    lines.append("")
    # --- ⑥ frames 维度（A2 帧序列对拍独立小节） ---
    anim_cases = [c for c in cases if c["id"] in frame_results]
    lines.append("## frames 维度（A2 帧序列对拍）")
    lines.append("")
    lines.append("anim 场景（`anim.ticks` 声明，SCHEMA A2 扩展）在静息排空后逐拍对拍（"
                 "ANIM_TICK_MS=100ms，k=0..K 共 K+1 帧）：逐拍 EXACT + 首异拍定位。"
                 "rust 侧 store.advance_clock 纯函数帧（A1）；ref 侧事件驱动逐拍"
                 "（tick_events 注入 + 双 idle/render 泵，无虚拟时钟——ManualClock 整 app "
                 "注入原生不稳，SCHEMA comparability 声明）。"
                 "G-128 midframe / G-49·G-368 节奏簇仅在「首拍与末拍一致、仅中间拍差」时"
                 "豁免（登记不拦）；末拍也差的不是 midframe 族——未命中即未裁决。")
    lines.append("")
    n_f_total = len(anim_cases)
    n_f_exact = sum(1 for r in frame_results.values() if r["verdict"] == "exact")
    f_causes_hit: dict[str, list[str]] = {}
    n_f_unadj = 0
    for cid, res in frame_results.items():
        if res["verdict"] == "exact":
            continue
        label = _adj_report(cid, res, ref)
        if label is None:
            n_f_unadj += 1
        else:
            f_causes_hit.setdefault(label, []).append(cid)
    n_f_caused = sum(len(v) for v in f_causes_hit.values())
    lines.append("| anim 总量 | EXACT | CAUSES 命中 | 未裁决 |")
    lines.append("|---|---|---|---|")
    lines.append(f"| {n_f_total} | {n_f_exact} | {n_f_caused} | {n_f_unadj} |")
    lines.append("")
    if n_f_total != len(frame_results) or any(
            r["verdict"] != "exact" for r in frame_results.values()):
        lines.append("| case | verdict | 非exact拍 | per-tick | first 异拍 |")
        lines.append("|---|---|---|---|---|")
        for case in anim_cases:
            cid = case["id"]
            res = frame_results[cid]
            ticks = case.get("anim_ticks")
            fd = res["first_tick_diff"]
            fd_s = "—" if fd is None else (
                f"tick={fd.get('tick')} r{fd.get('row')}c{fd.get('col')} "
                f"{fd.get('field')} ref={fd.get('ref')!r} rust={fd.get('rust')!r}")
            per = "".join("." if v == "exact" else ("R" if v == "structural" else "X")
                          for v in res["per_tick"])
            lines.append(
                f"| {cid} | {res['verdict']} | {res['cells_diff_count']}/{len(res['per_tick'])} "
                f"| `{per}` (K={ticks}) | {fd_s} |")
        lines.append("")
        for cid, res in frame_results.items():
            if res["verdict"] == "exact":
                continue
            cause = _adj_report(cid, res, ref)
            cause_s = f" — CAUSES: {cause}" if cause else " — 未裁决（T9 候选）"
            lines.append(f"- `{cid}` {cause_s}")
        lines.append("")
    # --- ⑥b recorded-local（真实语料，聚合计数 only — diff-ext C2） ---
    if rec_cases or skipped and any(str(c).split("@")[0].startswith("rec-") for c in skipped):
        lines.append("## recorded-local（真实语料，聚合计数 — advisory）")
        lines.append("")
        lines.append("本地真实会话语料（`tools/diff/corpus/tui/recorded-local/`，git-ignored；")
        lines.append("隐私铁律：报告只列聚合计数，不引用录制内容原文，分歧明细表亦不展开）。")
        lines.append("**门语义（控制者裁定 2026-11-15）：本节未裁决=advisory——不参与 gate exit")
        lines.append("（日常录制常驻未裁决项，committed 绿才是门绿）；rec-* 的 infra 失败仍=exit 1")
        lines.append("（重放机制坏了）；未裁决项为 C 线后续 root-cause 输入（G-426/G-427 等）。**")
        lines.append("")
        rec_ids = {c["id"] for c in rec_cases}
        rec_frame_ids = {c for c in frame_results if c.split("@")[0].startswith("rec-")}
        def _agg(ids: set[str], res_map: dict[str, dict]) -> tuple[int, int, int, int, int]:
            total = len(ids)
            exact = sum(1 for i in ids if (res_map.get(i) or {}).get("verdict") == "exact")
            struct = sum(1 for i in ids if (res_map.get(i) or {}).get("verdict") == "structural")
            div = sum(1 for i in ids if (res_map.get(i) or {}).get("verdict") == "diverge")
            unadj = sum(1 for i in ids
                        if (res_map.get(i) or {}).get("verdict") not in (None, "exact")
                        and _adj_report(i, res_map[i], ref) is None)
            return total, exact, struct, div, unadj
        rt, re_, rs, rd, ru = _agg(rec_ids, results)
        lines.append("| 维度 | 总量 | EXACT | STRUCTURAL | DIVERGE | 未裁决 |")
        lines.append("|---|---|---|---|---|---|")
        lines.append(f"| resting | {rt} | {re_} | {rs} | {rd} | {ru} |")
        if rec_frame_ids:
            ft, fe, fs, fd, fu = _agg(rec_frame_ids, frame_results)
            lines.append(f"| frames | {ft} | {fe} | {fs} | {fd} | {fu} |")
        rec_skipped = [c for c in (skipped or []) if str(c).split("@")[0].startswith("rec-")]
        if rec_skipped:
            lines.append("")
            lines.append(f"plugins_unavailable 一致跳过：{len(rec_skipped)} case（聚合计数）。")
        lines.append("")
    # --- ⑦ out-of-scope declaration ---
    lines.append("## 范围外声明")
    lines.append("")
    lines.append("- /opt/opentui-rust 127 场景 = 框架层（OpenTUI 渲染器）边界，本通道只测 oc-tui 应用层，")
    lines.append("  不在其上做差分裁决")
    lines.append("- 动画中间帧 G-128/G-49/G-368：A2 起有了 frames 维度的可断言面（anim 场景"
                 "逐拍对拍，上节）；纯静息场景仍只比静止帧，frames 簇豁免收窄为"
                 "「首末拍一致、仅中间拍差」的 midframe 形状")
    lines.append("- ref 侧不可虚拟时钟化的动画面：toast 解散（真实全局 setTimeout，"
                 "ui/toast.tsx:142）与 caret blink（原生 cursor 样式，captureSpans 不可见）"
                 "——帧序列不覆盖（SCHEMA comparability 声明）")
    lines.append("- 活体交互序列（键位/焦点/resize 时序）= v2 扩展，静止帧差分不覆盖")
    lines.append("- 双侧 request_census 不对称 = 信息性（boot 请求模式已知不同），不参与判门")
    lines.append("")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines) + "\n")
    print(f"wrote {path}")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--ref", default=str(REF_DEFAULT))
    ap.add_argument("--write", default="docs/audit/diff/TUI-FULLSCREEN.md")
    ap.add_argument("--manifest-only", action="store_true")
    args = ap.parse_args()
    ref = Path(args.ref)

    cases, staged_out = build_manifest(STAGE)
    skipped = list(staged_out)
    manifest = STAGE / "manifest.json"
    print(f"manifest: {len(cases)} cases -> {manifest}"
          + (f" ({len(skipped)} plugins_unavailable)" if skipped else ""))
    if args.manifest_only:
        return 0

    ref_grids, rust_grids = run(STAGE, ref, cases)
    # B2 plugin-skip reconciliation (mutates nothing: surviving cases
    # drive the compare/gate; skipped ids are reported)
    cases, skipped_runtime = reconcile_plugin_skips(ref_grids, rust_grids, cases)
    skipped = sorted(set(skipped) | set(skipped_runtime))
    results = {c["id"]: compare(normalize(ref_grids[c["id"]]),
                                normalize(rust_grids[c["id"]]))
               for c in cases}
    # A2 frames dimension: sequence-compare every anim case's frames
    # (both sides validated K+1 dense grids by load_side)
    frame_results = {c["id"]: compare_frames(ref_grids[c["id"]], rust_grids[c["id"]])
                     for c in cases if "frames" in (ref_grids[c["id"]] or {})}
    write_report(Path(args.write), cases, ref_grids, rust_grids, results, frame_results,
                 skipped)
    recs = [{"id": cid, "dimension": "resting",
             **({"ref_plugin_failed": _plugin_failed_on_ref(ref_grids[cid])}
                if cid.startswith("plugin-") else {}),
             **res} for cid, res in results.items()]
    recs += [{"id": cid,
              **({"ref_plugin_failed": _plugin_failed_on_ref(ref_grids[cid])}
                 if cid.startswith("plugin-") else {}),
              **res} for cid, res in frame_results.items()]
    code = verdict(recs, CAUSES)
    n_caused = sum(1 for r in recs if r["verdict"] != "exact"
                   and adjudicate(r) is not None)
    for cid, res in results.items():
        fd = res["first_diff"]
        fd_s = "" if fd is None else f" first_diff={fd}"
        cause = adjudicate(_rec_with_flag(cid, {"dimension": "resting", **res}, ref_grids)) if res["verdict"] != "exact" else None
        cause_s = f" cause={cause}" if cause else ""
        print(f"{cid}: {res['verdict']} cells_diff={res['cells_diff_count']}"
              f"{fd_s}{cause_s}")
    for cid, res in frame_results.items():
        fd = res["first_tick_diff"]
        fd_s = "" if fd is None else f" first_tick_diff={fd}"
        cause = adjudicate(_rec_with_flag(cid, {**res}, ref_grids)) if res["verdict"] != "exact" else None
        cause_s = f" cause={cause}" if cause else ""
        per = "".join("." if v == "exact" else ("R" if v == "structural" else "X")
                      for v in res["per_tick"])
        print(f"[frames] {cid}: {res['verdict']} nonexact={res['cells_diff_count']} "
              f"per_tick={per}{fd_s}{cause_s}")
    print(f"gate exit {code} "
          f"(0=committed corpus all adjudicated exact or CAUSES-covered; "
          f"2=unadjudicated committed divergence; rec-* recorded-local "
          f"unadjudicated=advisory per the 2026-11-15 controller ruling; "
          f"infra failures exit 1 before this point); "
          f"CAUSES hits: {n_caused}")
    return code


if __name__ == "__main__":
    raise SystemExit(main())
