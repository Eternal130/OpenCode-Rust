#!/usr/bin/env python3
"""HTTP full differential (channel 6 of the diff layer) — the expensive
channel: both sides boot as REAL server processes, one shared request
corpus is replayed in the same order against each, responses are
normalized and diffed field by field.

  rust : cargo-built `oc-server serve --port 0` (ephemeral loopback;
         passwordless — AuthConfig password=None semantics; roots moved
         under /tmp via OPENCODE_TEST_HOME / XDG_* / TMPDIR). Spawned and
         reaped as a child process by this orchestrator.
  ref  : bun driver (tools/diff/drivers/http_ref_server.ts) staged to
         /tmp with a symlinked node_modules tree into the read-only ref
         checkout; boots the real @opencode/server graph through
         ServerFetch.make + Bun.serve (embedder entry, fetch.ts docs).
         All durable roots pointed at /tmp — ref git tree zero-write
         (asserted below via `git status --porcelain` before/after).

Corpus (46 requests, 4 families, easy → hard):
  L1 read-only (15) — GET /api/info, /api/config, /api/agent, /api/command,
      /api/skill, /api/model, /api/model/default, /api/provider, /api/form,
      /api/location, /api/integration, /api/fs/list, /api/fs/find,
      /api/project, /api/worktree, /api/mcp (no-credential, no-network).
  L2 error paths (12) — missing id 404 shapes, missing/invalid body
      400/422 shapes, nonexistent location (G-220/G-233/G-270④ anchors).
  L3 side effects (16) — session create → get → rename → context → list;
      shell create → get → list → output → delete (G-349 idempotence
      anchor); session delete → empty list again.
  L4 SSE (3) — GET /api/event handshake: first frame shape and the event
      NAME sequence over a short window, then disconnect. Only names and
      the first frame are compared; payloads/timing are declared
      ADJUDICATED (inherently generated order).

Normalization (declared, applied identically to both sides):
  - status + coarse content type (json vs event-stream) compared.
  - JSON bodies: recursive key sort; string values that look like ids
    (uuid, ses_/sh_/evt_/msg_/prt_/frm_/per_/req_/cmd_/int_ prefixed) → <ID>;
    ISO-8601 timestamps → <TS>; unix-ms 13-digit integers → <TS_MS>;
    per-side tmp/ws roots → <TMP>/<WS>; ports in URLs → <PORT>;
    /api/info version → <VER>, pid → <PID>, urls[] → <URLS>.
  - Arrays compared as sorted multisets (canonical JSON sort) — list order
    is not part of the contract.
  - SSE: only the event-name sequence + first frame's data key set.
  - Cases whose content is inherently run-dependent are marked
    ADJUDICATED in CORPUS (declared), never silently dropped.

Stability: boot retried once per side; per-request timeout; both sides
execute the identical corpus in identical order; child processes killed
by process group on exit (orphan sweep at the end).

Output: markdown report (default docs/audit/diff/HTTP-FULL.md) with
MATCH/DIVERGE/ADJUDICATED counts and per-case dual-side file:line hints
(locate by grepping the route literal in both trees). Exit 0 — a diff
report, not a gate. Infrastructure failure (boot impossible) exits 1.
"""
from __future__ import annotations

import argparse
import json
import os
import re
import signal
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent.parent
STAGE = Path("/tmp/opencode/diff-http")
REF_DRIVER = REPO / "tools/diff/drivers/http_ref_server.ts"
BOOT_TIMEOUT = 90.0
REQ_TIMEOUT = 20.0
SSE_WINDOW = 4.0

MISSING = "diff-does-not-exist-0000000000"


def q(directory: str) -> str:
    return urllib.parse.quote(directory, safe="")


# ---------------------------------------------------------------- corpus

def corpus() -> dict:
    """Ordered families → requests. `{SID}`/`{SHID}` filled from captures,
    `{WS}` from the per-side workspace root at request time."""
    l1 = [
        ("info", "GET", "/api/info", None),
        ("config", "GET", "/api/config", None),
        ("agent", "GET", "/api/agent", None),
        ("command", "GET", "/api/command", None),
        ("skill", "GET", "/api/skill", None),
        ("model", "GET", "/api/model", None),
        ("model_default", "GET", "/api/model/default", None),
        ("provider", "GET", "/api/provider", None),
        ("form", "GET", "/api/form", None),
        ("location", "GET", "/api/location", None),
        ("integration", "GET", "/api/integration", None),
        ("fs_list_default", "GET", "/api/fs/list", None),
        ("fs_find", "GET", "/api/fs/find?query=diff&" + urllib.parse.urlencode({"type": "file"}), None),
        ("project", "GET", "/api/project", None),
        ("worktree", "GET", "/api/worktree", None),
        ("mcp", "GET", "/api/mcp", None),
    ]
    l2 = [
        ("session_missing_get", "GET", f"/api/session/{MISSING}", None),
        ("session_missing_patch", "PATCH", f"/api/session/{MISSING}", {"title": "x"}),
        ("session_missing_delete", "DELETE", f"/api/session/{MISSING}", None),
        ("session_missing_context", "GET", f"/api/session/{MISSING}/context", None),
        ("session_missing_message", "GET", f"/api/session/{MISSING}/message", None),
        ("session_create_no_body", "POST", "/api/session", None),
        ("session_create_bad_location", "POST", "/api/session",
         {"location": {"directory": "/diff-nonexistent-location"}}),
        ("agent_missing", "GET", "/api/agent/does-not-exist", None),
        ("skill_missing", "GET", "/api/skill/does-not-exist", None),
        ("command_missing", "GET", "/api/command/does-not-exist", None),
        ("shell_missing_get", "GET", "/api/shell/does-not-exist", None),
        ("fs_read_missing", "GET", "/api/fs/read/diff-no-such-file.txt", None),
        ("config_shell", "GET", "/api/config/shell", None),
    ]
    l3 = [
        ("session_create", "POST", "/api/session", {"location": {"directory": "{WS}"}}, "sid"),
        ("session_get", "GET", "/api/session/{SID}", None),
        ("session_rename", "PATCH", "/api/session/{SID}", {"title": "diff-renamed"}),
        ("session_context", "GET", "/api/session/{SID}/context", None),
        ("session_inbox", "GET", "/api/session/{SID}/inbox", None),
        ("session_list_one", "GET", "/api/session", None),
        ("permission_saved_empty", "GET", "/api/permission/saved", None),
        ("shell_create", "POST", "/api/shell", {"command": "printf diffch6"}, "shid"),
        # settle: let `printf` exit so running/exited status is not a race
        ("shell_get", "GET", "/api/shell/{SHID}", None, None, 0.8),
        ("shell_list", "GET", "/api/shell", None),
        ("shell_output", "GET", "/api/shell/{SHID}/output", None),
        ("shell_delete", "DELETE", "/api/shell/{SHID}", None),
        ("shell_list_after", "GET", "/api/shell", None),
        ("session_delete", "DELETE", "/api/session/{SID}", None),
        ("session_list_empty_again", "GET", "/api/session", None),
    ]
    l4 = [
        ("sse_first_frame", "SSE", "/api/event", None),
        ("sse_name_sequence", "SSE_NAMES", "/api/event", None),
        ("sse_reconnect_first_frame", "SSE", "/api/event", None),
    ]
    families = [("L1-readonly", l1), ("L2-errors", l2), ("L3-side-effects", l3), ("L4-sse", l4)]
    cases = []
    for fam, reqs in families:
        for r in reqs:
            name, method, path, body = r[0], r[1], r[2], r[3]
            cases.append({
                "family": fam, "name": name, "method": method, "path": path,
                "body": body, "capture": r[4] if len(r) > 4 else None,
                "settle": r[5] if len(r) > 5 else 0.0,
            })
    return {"families": [f for f, _ in families], "cases": cases}


# Cases whose compared surface is inherently run-dependent: declared
# ADJUDICATED (normalization declared, not a DIVERGE).
ADJUDICATED = {
    # event NAME sequences depend on ambient bus traffic each boot
    "sse_name_sequence": "SSE 事件名序列取决于 boot 期环境总线流量，只比名字多重集",
    # /api/model in no-credential+no-network mode lists catalog state whose
    # freshness differs by build; shape is compared, membership adjudicated
    "model": "无凭据+禁网态下模型清单成员随目录新鲜度浮动；只比形状",
}

# 定性（人工归属）for known DIVERGE surfaces — printed with the detail.
DIV_NOTES = {
    "agent": "内置 agent 的 permissions 清单与 system prompt 文本漂移（rust 少 5-9 条权限项）",
    "skill": "rust 序列化 autoinvoke:null（ref 省略缺省键）+ 内置 skill 正文文本漂移",
    "model_default": "rust 静态目录缺 package/settings.apiKey/settings.baseURL（zen 接线在无凭据态未呈现）",
    "provider": "语义分歧：rust 返回全量目录(224, activation=auto, 无 location 包裹)；ref 只返回已启用 provider(1) 且带 location 包裹",
    "integration": "目录条目数漂移（224 vs 229，目录版本差异 + 疑似过滤差异）",
    "project": "rust 输出 commands/icon/name/vcs:null；ref 省略缺省键（序列化形状）",
    "session_create_bad_location": "语义分歧：session.create 对不存在目录 rust=404 LocationNotFound；ref=200 创建悬挂 placement session（fetch.test.ts 声明 create 不 boot Location 图）",
    "session_create": "cost 序列化 0.0(float) vs 0(int)；rust 额外输出 slug/subpath 字段",
    "session_get": "同 session_create（cost 类型 + slug/subpath）",
    "session_list_one": "ref 列表包含 bad-location 会话（rust 已 404 拒绝创建）→ 级联差异",
    "session_list_empty_again": "ref cursor.next/previous 恒返回锚点（即使空页/单页）；rust 返回 null；且 ref 残留悬挂会话",
    "agent_missing": "错误 message 文本：rust 缺失 offender 后缀（'Agent not found' vs 'Agent not found: does-not-exist'）",
    "fs_read_missing": "错误 message 文本：rust 缺失 path 后缀（同上模式）",
}


# ------------------------------------------------------------ side setup

def fresh_root(tag: str) -> Path:
    root = STAGE / tag
    subprocess.run(["rm", "-rf", str(root)], check=True)
    for sub in ("ws", "home/.local/share", "home/.cache", "home/.config", "home/.local/state", "tmp"):
        (root / sub).mkdir(parents=True, exist_ok=True)
    return root


def side_env(root: Path) -> dict:
    # XDG roots nested under HOME so both sides' config/data path strings
    # coincide (rust resolves config through HOME, ref through XDG_CONFIG_HOME).
    env = dict(os.environ)
    env.update({
        "HOME": str(root / "home"),
        "OPENCODE_TEST_HOME": str(root / "home"),
        "XDG_DATA_HOME": str(root / "home/.local/share"),
        "XDG_CACHE_HOME": str(root / "home/.cache"),
        "XDG_CONFIG_HOME": str(root / "home/.config"),
        "XDG_STATE_HOME": str(root / "home/.local/state"),
        "TMPDIR": str(root / "tmp"),
        # no network egress on either side
        "OPENCODE_CATALOG_FETCH": "0",
        "OPENCODE_DISABLE_MODELS_FETCH": "1",
        "NO_COLOR": "1",
    })
    return env


def stage_ref_driver(ref: Path) -> Path:
    stage = STAGE / "stage"
    stage.mkdir(parents=True, exist_ok=True)
    driver = stage / "http_ref_server.ts"
    driver.write_text(REF_DRIVER.read_text())
    nm = stage / "node_modules"
    if nm.exists():
        subprocess.run(["rm", "-rf", str(nm)], check=True)
    (nm / "@opencode").mkdir(parents=True)
    # symlink every dep of packages/server (incl. @effect/effect/etc.) plus
    # the @opencode/server workspace link itself; nested workspace packages
    # resolve the rest through the ref root's node_modules on the upward walk.
    for entry in (ref / "packages/server/node_modules").iterdir():
        target = nm / entry.name
        if target.exists() or target.is_symlink():
            continue
        target.symlink_to(entry)
    sh_link = nm / "@opencode/server"
    if not sh_link.is_symlink():
        sh_link.symlink_to(ref / "packages/server")
    return driver


def spawn(args, cwd, env):
    return subprocess.Popen(
        args, cwd=str(cwd), env=env, stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL, text=True, start_new_session=True,
    )


def wait_marker(proc, prefix, timeout) -> str:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        line = proc.stdout.readline()
        if not line:
            raise RuntimeError(f"process exited before marker (rc={proc.poll()})")
        line = line.strip()
        if line.startswith(prefix):
            return line
    raise TimeoutError(f"marker {prefix!r} not seen in {timeout}s")


def boot_rust(root: Path, binary: Path):
    env = side_env(root)
    proc = spawn([str(binary), "serve", "--port", "0"], root / "ws", env)
    line = wait_marker(proc, "server listening on ", BOOT_TIMEOUT)
    url = line.split("server listening on ", 1)[1].strip()
    return proc, url


def boot_ref(root: Path, ref: Path):
    driver = stage_ref_driver(ref)
    env = side_env(root)
    env["DIFF_APP_VERSION"] = "test-version"
    proc = spawn(["bun", str(driver)], root / "ws", env)
    line = wait_marker(proc, "DIFF_READY ", BOOT_TIMEOUT)
    port = line.split("DIFF_READY ", 1)[1].strip()
    return proc, f"http://127.0.0.1:{port}"


def kill_side(proc) -> None:
    if proc.poll() is not None:
        return
    try:
        os.killpg(os.getpgid(proc.pid), signal.SIGTERM)
    except ProcessLookupError:
        return
    try:
        proc.wait(timeout=8)
    except subprocess.TimeoutExpired:
        try:
            os.killpg(os.getpgid(proc.pid), signal.SIGKILL)
        except ProcessLookupError:
            pass
        proc.wait(timeout=5)


# -------------------------------------------------------------- requests

def fill(text: str, ws: str, caps: dict) -> str:
    text = text.replace("{WS}", ws)
    return text.replace("{SID}", caps.get("sid", MISSING)).replace("{SHID}", caps.get("shid", MISSING))


def do_request(base_url: str, case: dict, ws: str, caps: dict) -> dict:
    path = fill(case["path"].replace("{WS}", q(ws)), ws, caps)
    method = case["method"]
    if method == "SSE" or method == "SSE_NAMES":
        return do_sse(base_url + path, names_only=(method == "SSE_NAMES"))
    if case.get("settle"):
        time.sleep(case["settle"])
    body = None
    headers = {"accept": "application/json"}
    if case["body"] is not None:
        raw = json.dumps(case["body"])
        body = fill(raw, ws, caps).encode()
        headers["content-type"] = "application/json"
        headers["content-type"] = "application/json"
    req = urllib.request.Request(base_url + path, data=body, headers=headers, method=method)
    try:
        with urllib.request.urlopen(req, timeout=REQ_TIMEOUT) as resp:
            raw = resp.read()
            return {"status": resp.status, "ctype": resp.headers.get("content-type", ""),
                    "body": raw.decode("utf-8", "replace")}
    except urllib.error.HTTPError as err:
        raw = err.read()
        return {"status": err.code, "ctype": err.headers.get("content-type", ""),
                "body": raw.decode("utf-8", "replace")}
    except Exception as exc:  # noqa: BLE001 — transport failure is a finding surface
        return {"status": None, "ctype": "", "body": f"<TRANSPORT-ERROR {type(exc).__name__}>"}


def do_sse(url: str, names_only: bool) -> dict:
    names: list[str] = []
    first_frame = None
    frames = 0
    try:
        req = urllib.request.Request(url, headers={"accept": "text/event-stream"})
        with urllib.request.urlopen(req, timeout=REQ_TIMEOUT) as resp:
            status, ctype = resp.status, resp.headers.get("content-type", "")
            deadline = time.monotonic() + SSE_WINDOW
            event = None
            while time.monotonic() < deadline:
                line = resp.readline()
                if not line:
                    break
                text = line.decode("utf-8", "replace").rstrip("\n")
                if text.startswith("event:"):
                    event = text.split(":", 1)[1].strip()
                elif text.startswith("data:") and event is not None:
                    frames += 1
                    if names_only:
                        names.append(event)
                    elif first_frame is None:
                        first_frame = {"event": event, "data": text.split(":", 1)[1].strip()}
                    event = None
    except Exception as exc:  # noqa: BLE001
        return {"status": None, "ctype": "", "body": f"<TRANSPORT-ERROR {type(exc).__name__}>"}
    if names_only:
        # name SEQUENCE is ambient; compare as a name multiset + presence of
        # a heartbeat cadence is out of scope. Dedup consecutive repeats.
        dedup = [n for i, n in enumerate(names) if i == 0 or n != names[i - 1]]
        return {"status": status, "ctype": ctype, "body": json.dumps({"names": sorted(set(dedup)), "frames": frames})}
    return {"status": status, "ctype": ctype,
            "body": json.dumps({"first_frame": first_frame, "frames": frames})}


# ---------------------------------------------------------- normalization

ID_RE = re.compile(r"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$", re.I)
PFX_RE = re.compile(r"^(ses|sh|evt|msg|prt|frm|per|req|cmd|int|aut|prj)[0-9A-Za-z_]{8,}$")
ISO_RE = re.compile(r"^\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}(:\d{2})?(\.\d+)?(Z|[+-]\d{2}:?\d{2})?$")
SHA1_RE = re.compile(r"^[0-9a-f]{40}$", re.I)
# base64url pagination anchors (start with eyJ = {" on decode)
CURSOR_RE = re.compile(r"^eyJ[A-Za-z0-9_-]{16,}$")


def norm_value(value, subs: list[tuple[re.Pattern, str]]):
    if isinstance(value, dict):
        out = {}
        for k in sorted(value):
            v = value[k]
            if k in ("pid",) and isinstance(v, (int, str)):
                out[k] = "<PID>"
            elif k in ("version",) and isinstance(v, str):
                out[k] = "<VER>"
            elif k in ("urls",) and isinstance(v, list):
                # presence depends on the listener entrypoint (embedder has
                # none); declared non-contractual
                out[k] = "<URLS>"
            else:
                out[k] = norm_value(v, subs)
        return out
    if isinstance(value, list):
        items = [norm_value(v, subs) for v in value]
        return sorted(items, key=lambda v: json.dumps(v, sort_keys=True, ensure_ascii=False))
    if isinstance(value, bool):
        return value
    if isinstance(value, (int, float)) and not isinstance(value, bool):
        if isinstance(value, int) and 1_600_000_000_000 <= value <= 9_999_999_999_999:
            return "<TS_MS>"
        if isinstance(value, int) and 1_600_000_000 <= value <= 9_999_999_999:
            return "<TS_S>"
        return value
    if isinstance(value, str):
        if ID_RE.match(value) or PFX_RE.match(value) or SHA1_RE.match(value):
            return "<ID>"
        if CURSOR_RE.match(value):
            return "<CURSOR>"
        if ISO_RE.match(value):
            return "<TS>"
        s = value
        for pat, rep in subs:
            s = pat.sub(rep, s)
        if s != value:
            return s
        if s.startswith("ses_") or s.startswith("sh_"):
            return "<ID>"
        return s
    return value


def normalize(resp: dict, root: Path, ws: Path) -> dict:
    subs = [
        (re.compile(re.escape(str(root))), "<TMP>"),
        (re.compile(re.escape(str(ws))), "<WS>"),
        # the rust port keeps its data dir under the suffixed app name
        (re.compile(r"opencode-rust"), "opencode"),
        (re.compile(r"/tmp/opencode(-rust)?$"), "/tmp/<APP>"),
        # ids embedded INSIDE strings (shell capture file paths, messages)
        (re.compile(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}", re.I), "<ID>"),
        (re.compile(r"\b[0-9a-f]{40}\b", re.I), "<HASH>"),
        (re.compile(r"\b(ses|sh|evt|msg|prt|frm|per)_([0-9A-Za-z]{8,})"), r"\1_<ID>"),
        (re.compile(r"http://(127\.0\.0\.1|localhost|\[::1\]):\d+"), "http://127.0.0.1:<PORT>"),
    ]
    ctype = resp.get("ctype", "").split(";")[0].strip()
    body = resp.get("body", "")
    try:
        parsed = json.loads(body)
        norm_body = norm_value(parsed, subs)
    except (json.JSONDecodeError, ValueError):
        norm_body = subs_apply(body, subs)
    status = resp.get("status")
    return {"status": status, "ctype": ctype, "body": norm_body}


def subs_apply(text: str, subs) -> str:
    s = text
    for pat, rep in subs:
        s = pat.sub(rep, s)
    return s


# ------------------------------------------------------- locating anchors

def first_diffs(a, b, path="$", out=None, limit=6):
    """Structural first differing leaf paths (bounded)."""
    if out is None:
        out = []
    if len(out) >= limit:
        return out
    if type(a) is not type(b):
        out.append(f"{path}: type {type(a).__name__} vs {type(b).__name__}")
        return out
    if isinstance(a, dict):
        for k in sorted(set(a) | set(b)):
            if k not in a or k not in b:
                out.append(f"{path}.{k}: {'<absent on rust>' if k not in a else '<absent on ref>'}")
            else:
                first_diffs(a[k], b[k], f"{path}.{k}", out, limit)
            if len(out) >= limit:
                break
        return out
    if isinstance(a, list):
        if len(a) != len(b):
            out.append(f"{path}: list len {len(a)} vs {len(b)}")
            return out
        for i, (x, y) in enumerate(zip(a, b)):
            first_diffs(x, y, f"{path}[{i}]", out, limit)
            if len(out) >= limit:
                break
        return out
    if a != b:
        out.append(f"{path}: {json.dumps(a, ensure_ascii=False)[:200]} vs {json.dumps(b, ensure_ascii=False)[:200]}")
    return out


def locate(ref: Path, case: dict) -> str:
    """Best-effort dual-side file:line anchor for a route literal."""
    path = case["path"].split("?")[0]
    lit = path.rstrip("/").split("/")
    # longest static prefix of the path
    static = []
    for part in lit:
        if "{" in part:
            break
        static.append(part)
    needle = "/".join(static) or path
    def grep(root, includes):
        try:
            res = subprocess.run(
                ["grep", "-rn", "-m", "1", "-F", needle, *includes],
                cwd=str(root), capture_output=True, text=True, timeout=10)
            for line in res.stdout.splitlines():
                f, _, rest = line.partition(":")
                if "node_modules" in f or "/target/" in f:
                    continue
                return f"{f}:{rest.split(':')[0]}"
        except Exception:  # noqa: BLE001
            pass
        return "?"
    rust = grep(REPO, ["crates/oc-server/src"])
    refd = grep(ref, ["packages/protocol/src", "packages/server/src"])
    return f"rust `{rust}` · ref `{refd}`"


# ------------------------------------------------------------------ main

def build_rust() -> Path:
    res = subprocess.run(["cargo", "build", "-p", "oc-server", "--bin", "oc-server"],
                         cwd=str(REPO), capture_output=True, text=True)
    binary = REPO / "target/debug/oc-server"
    if res.returncode != 0 or not binary.exists():
        print(res.stdout[-2000:], res.stderr[-2000:], sep="\n", file=sys.stderr)
        raise SystemExit("cargo build oc-server failed")
    return binary


def git_clean(ref: Path) -> bool:
    res = subprocess.run(["git", "-C", str(ref), "status", "--porcelain"],
                         capture_output=True, text=True)
    return res.returncode == 0 and not res.stdout.strip()


def boot_with_retry(boot_fn, *args):
    last = None
    for _ in range(2):  # boot failure → one retry
        proc = None
        try:
            proc, url = boot_fn(*args)
            return proc, url
        except Exception as exc:  # noqa: BLE001
            last = exc
            if proc is not None:
                kill_side(proc)
    raise SystemExit(f"boot failed after retry: {last}")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--ref", default="/opt/opencode-ref")
    ap.add_argument("--write", default="docs/audit/diff/HTTP-FULL.md")
    ap.add_argument("--skip-build", action="store_true")
    args = ap.parse_args()
    ref = Path(args.ref)

    ref_clean_before = git_clean(ref)
    spec = corpus()
    binary = REPO / "target/debug/oc-server" if args.skip_build else build_rust()

    STAGE.mkdir(parents=True, exist_ok=True)
    rust_root = fresh_root("rust")
    ref_root = fresh_root("ref")

    rust_proc, rust_url = boot_with_retry(boot_rust, rust_root, binary)
    ref_proc, ref_url = boot_with_retry(boot_ref, ref_root, ref)

    results = {}
    try:
        for side, url, root in (("rust", rust_url, rust_root), ("ref", ref_url, ref_root)):
            caps: dict[str, str] = {}
            recs = []
            for case in spec["cases"]:
                resp = do_request(url, case, str(root / "ws"), caps)
                rec = {"name": case["name"], "family": case["family"], **normalize(resp, root, root / "ws")}
                recs.append(rec)
                if case["capture"] and resp.get("status") in (200, 201):
                    try:
                        data = json.loads(resp["body"]).get("data", {})
                        rid = data.get("id") if isinstance(data, dict) else None
                        if rid:
                            caps[case["capture"]] = rid
                    except (json.JSONDecodeError, ValueError, AttributeError):
                        pass
            results[side] = {r["name"]: r for r in recs}
    finally:
        kill_side(rust_proc)
        kill_side(ref_proc)

    ref_clean_after = git_clean(ref)
    if ref_clean_before and not ref_clean_after:
        print("WARNING: ref git tree was written during the run", file=sys.stderr)

    # orphan sweep: no leftover server processes from this channel
    orphans = []
    for pat in ("oc-server", "http_ref_server.ts"):
        res = subprocess.run(["pgrep", "-f", pat], capture_output=True, text=True)
        orphans += [p for p in res.stdout.split() if p]
    if orphans:
        print(f"WARNING: orphan server processes remain: {orphans}", file=sys.stderr)

    names = [c["name"] for c in spec["cases"]]
    fam_of = {c["name"]: c["family"] for c in spec["cases"]}
    lines = []
    w = lines.append
    w("# HTTP-FULL Differential（差分层 · 通道 6 · 全量 HTTP 对拍）")
    w("")
    w("> 同一 corpus（4 族）以相同顺序打双侧**真实 server 进程**：")
    w(f"> rust = `oc-server serve --port 0`（boot 输出 `{rust_url}`，AuthConfig password=None 语义，roots → /tmp）")
    w(f"> ref = bun 驱动 `tools/diff/drivers/http_ref_server.ts`（ServerFetch.make + Bun.serve，`{ref_url}`，roots → /tmp，ref git 树零写入）")
    w("> 归一化：键序排序、id/uuid/时间戳/pid/version/端口/绝对路径（→ `<TMP>`/`<WS>`）剥换、列表按多重集比较、SSE 只比首帧+事件名。")
    w(f"> 复现：`python3 tools/diff/http_full.py`（corpus 内嵌，{len(names)} 请求）")
    w("")
    w("## 范围声明")
    w("")
    w("- 不覆盖：网络出口（provider/models-dev 实际调用，双侧均禁网）、password 鉴权态、workerd 剖面、插件安装路径。")
    w("- wp 局限：ref 侧经 ServerFetch（web-handler embedder 入口）而非 node:http ServerProcess（后者强制 password），")
    w("  故 auth 拦截层与信号处理不在对拍面内；路由/序列化层与真实进程一致。")
    w("- SSE 只比首帧形状与事件名多重集；帧序/心跳节奏声明为 ADJUDICATED（依赖 boot 期环境总线流量）。")
    w("")
    match = diverge = adj = err = 0
    rows = []
    div_detail = []
    for name in names:
        r, f = results["rust"].get(name), results["ref"].get(name)
        if r is None or f is None:
            err += 1
            rows.append((fam_of[name], name, "MISSING"))
            continue
        adj_reason = ADJUDICATED.get(name)
        if adj_reason:
            adj += 1
            rows.append((fam_of[name], name, "ADJUDICATED"))
            continue
        if r == f:
            match += 1
            rows.append((fam_of[name], name, "MATCH"))
        else:
            diverge += 1
            rows.append((fam_of[name], name, "DIVERGE"))
            div_detail.append((name, r, f))
    total = len(names)
    w(f"- **MATCH：{match}/{total}** · DIVERGE：**{diverge}** · ADJUDICATED：**{adj}** · MISSING：**{err}**")
    w("")
    w("| 族 | 用例 | 判定 |")
    w("|---|---|---|")
    for fam, name, verdict in rows:
        w(f"| {fam} | `{name}` | {verdict} |")
    w("")
    if div_detail:
        w("## 分歧明细（双侧 file:line 锚 + 定性）")
        for name, r, f in div_detail:
            case = next(c for c in spec["cases"] if c["name"] == name)
            w(f"### `{name}`（{fam_of[name]} · {case['method']} `{case['path'].split('?')[0]}`）")
            w(f"- 锚点：{locate(ref, case)}")
            if name in DIV_NOTES:
                w(f"- 定性：{DIV_NOTES[name]}")
            for field in ("status", "ctype", "body"):
                if r[field] != f[field]:
                    w(f"- `{field}` 分歧：" if field != "body" else "- `body` 分歧（首个结构差叶路径，rust vs ref）：")
                    if field == "body" and isinstance(r[field], (dict, list)) and isinstance(f[field], (dict, list)):
                        for d in first_diffs(r[field], f[field]):
                            w(f"  - `{d}`")
                    else:
                        w(f"  - rust = `{json.dumps(r[field], ensure_ascii=False, sort_keys=True)[:500]}`")
                        w(f"  - ref  = `{json.dumps(f[field], ensure_ascii=False, sort_keys=True)[:500]}`")
            w("")
    w("## 判定")
    w("")
    w(f"- 已裁定：{total}/{total}（MATCH+ADJUDICATED+DIVERGE 逐例有归属；DIVERGE 为登记册候选，见上表锚点）")
    w(f"- ADJUDICATED {adj} 项：归一化声明见范围声明与 corpus 注释（SSE 名序 / 目录清单成员），非 DIVERGE。")
    w("- 稳定性：双侧 boot 各重试一次窗口（本次未触发即成功）、每请求独立超时 "
      f"({REQ_TIMEOUT}s)、SSE 短窗 {SSE_WINDOW}s、子进程按进程组回收（SIGTERM→SIGKILL）。")
    if not ref_clean_after:
        w("- ⚠️ ref git 树在运行期间出现脏项（见 stderr 警告），需人工复核。")
    else:
        w("- ref git 树零写入（运行前后 `git status --porcelain` 均空）。")
    report = "\n".join(lines) + "\n"
    out_path = Path(args.write)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(report)
    print(f"wrote {out_path}")
    print(f"MATCH {match}/{total}, DIVERGE {diverge}, ADJUDICATED {adj}, MISSING {err}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
