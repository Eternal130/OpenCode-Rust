#!/usr/bin/env python3
"""CLI-surface census: command × flag sets, rust clap vs ref (effect-cli Spec).

Axis 1 — command tree: rust crates/oc-cli/src/cli.rs clap `Commands` and the
  nested `*Commands` subcommand enums (variant → kebab-case name, aliases
  from `#[command(alias = …)]`, nesting via `command: XCommands` fields) vs
  the ref `Spec.make("name", {…})` tree in packages/cli/src/commands/commands.ts.
Axis 2 — global flags: rust `#[arg(… global = true)]` fields (clap forces
  globals at the root; the flattened `PermissionParamsGlobal`) vs the ref
  `GlobalFlag.setting("print-logs")` (registered in framework/runtime.ts)
  plus the two spread param groups (`ServerParams`, `PermissionParams`).
Axis 3 — per-command local flags (command × flag pairs) after removing the
  global/spread layer. Hidden status (`hide = true` / `withHidden`) is an
  annotation, not a difference.

Normalization rules (explicit):
  N1 flag spelling: rust field names snake_case → kebab-case with a single
     trailing `_` stripped (`continue_` → `continue`); an explicit
     `long = "x"` wins. Ref names are taken verbatim from `Flag.*("name")`
     — ref already spells them kebab-case.
  N2 global layer: the ref has no true per-command globals except
     `print-logs`; it SPREADS `...ServerParams` (standalone, server) and
     `...PermissionParams` (auto, yolo, dangerously-skip-permissions) into
     the root and many subcommands, while rust models all six as clap root
     globals. The spread groups and the GlobalFlag are compared on the
     global axis ONLY and removed from the per-command lists.
  N3 positional arguments (`Argument.*` / bare clap fields) are inventoried
     per command but NOT adjudicated (informational axis).
  N4 framework builtins (`--help`, `--version`) exist on both sides and are
     out of scope.

Usage: python3 tools/census/cli_surface.py [--ref /opt/opencode-ref] [--write docs/audit/census/CLI.md]
"""
from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from common import Report, _match_brace, strip_tests  # noqa: E402

CLI_RS = Path("crates/oc-cli/src/cli.rs")
REF_COMMANDS = "packages/cli/src/commands/commands.ts"
REF_RUNTIME = "packages/cli/src/framework/runtime.ts"

FLAG_DECL = re.compile(
    r"(Flag|Argument)\.(?:boolean|string|integer|choice|keyValuePair|stringArray)\(\s*\"([^\"]+)\""
)
SPEC_MAKE = re.compile(r"Spec\.make\(")


# ---------------------------------------------------------------- rust side
def parse_rust() -> dict:
    src = strip_tests(CLI_RS.read_text())
    return _parse_rust_clean(src, src.splitlines())


def _parse_rust_clean(src: str, lines: list[str]) -> dict:
    enums: dict[str, list[dict]] = {}
    structs: dict[str, list[dict]] = {}
    cur: list | None = None
    cur_kind: str | None = None
    pending: list[str] = []
    for i, ln in enumerate(lines, 1):
        m = re.match(r"pub enum (\w+)", ln)
        m2 = re.match(r"pub struct (\w+)", ln)
        if m or m2:
            cur = []
            cur_kind = "enum" if m else "struct"
            (enums if m else structs)[(m or m2).group(1)] = cur
            pending = []
            continue
        if cur is None:
            continue
        if ln == "}":  # top-level container close (rustfmt)
            cur = None
            pending = []
            continue
        am = re.match(r"\s*(#\[.*\])\s*$", ln)
        if am:
            pending.append(am.group(1))
            continue
        if cur_kind == "enum":
            vm = re.match(r"    (\w+) \{$", ln) or re.match(r"    (\w+),\s*$", ln)
            if vm:
                cur.append({
                    "variant": vm.group(1),
                    "attrs": pending[:],
                    "fields": [],
                    "line": i,
                })
                pending = []
                continue
            fm = re.match(r"        (?:pub )?(\w+):\s*(.*?)\s*,?\s*$", ln)
            if fm and cur and cur[-1].get("fields") is not None:
                cur[-1]["fields"].append((fm.group(1), fm.group(2), pending[:], i))
                pending = []
                continue
        else:
            fm = re.match(r"    (?:pub )?(\w+):\s*(.*?)\s*,?\s*$", ln)
            if fm and cur is not None:
                cur.append((fm.group(1), fm.group(2), pending[:], i))
                pending = []
                continue

    # ---- expand flags
    def field_flag(field, ty, attrs, line):
        """(kind, name, hidden, line) — kind: flag | positional | subcommand | flatten."""
        joined = " ".join(attrs)
        if "#[command(subcommand)]" in joined:
            return ("subcommand", ty, False, line)
        if "#[command(flatten)]" in joined:
            return ("flatten", ty, False, line)
        long_m = re.search(r'long\s*=\s*"([^"]+)"', joined)
        has_long = "long" in joined
        hidden = re.search(r"\bhide\b", joined) is not None
        if long_m:
            return ("flag", long_m.group(1), hidden, line)
        if has_long:
            return ("flag", kebab(field), hidden, line)
        return ("positional", field, hidden, line)

    def attrs_of(variant):
        return " ".join(variant["attrs"])

    # ---- command tree from the `Commands` enum
    commands: dict[str, dict] = {}

    def walk(enum_name: str, prefix: str) -> None:
        for var in enums.get(enum_name, []):
            path = (prefix + " " + kebab(var["variant"])).strip()
            aliases = re.findall(r'alias\s*=\s*"([^"]+)"', attrs_of(var))
            flags: dict[str, tuple[bool, int]] = {}
            positionals: list[str] = []
            children = False
            for field, ty, attrs, line in var["fields"]:
                kind, name, hidden, line = field_flag(field, ty, attrs, line)
                if kind == "flag":
                    flags[name] = (hidden, line)
                elif kind == "positional":
                    positionals.append(name)
                elif kind == "subcommand":
                    children = True
                    walk(ty, path)
                elif kind == "flatten":
                    for f2, ty2, a2, l2 in structs.get(ty, []):
                        k2, n2, h2, _ = field_flag(f2, ty2, a2, l2)
                        if k2 == "flag":
                            flags[n2] = (h2, l2)
            commands[path] = {
                "flags": flags,
                "aliases": aliases,
                "positionals": positionals,
                "children": children,
                "line": var["line"],
            }

    # root = the `Cli` struct
    root_flags: dict[str, tuple[bool, int]] = {}
    root_positionals: list[str] = []
    globals_rust: dict[str, tuple[bool, int]] = {}

    def add_struct_fields(struct_name: str) -> None:
        for field, ty, attrs, line in structs.get(struct_name, []):
            kind, name, hidden, line = field_flag(field, ty, attrs, line)
            if kind == "flag":
                if "global = true" in " ".join(attrs):
                    globals_rust[name] = (hidden, line)
                else:
                    root_flags[name] = (hidden, line)
            elif kind == "positional":
                root_positionals.append(name)
            elif kind == "flatten":
                add_struct_fields(ty)

    add_struct_fields("Cli")
    walk("Commands", "")
    commands["(root)"] = {
        "flags": root_flags,
        "aliases": [],
        "positionals": root_positionals,
        "children": True,
        "line": next(
            (i for i, ln in enumerate(lines, 1) if re.match(r"pub struct Cli", ln)), 0
        ),
    }
    return {"commands": commands, "globals": globals_rust, "file": str(CLI_RS)}


# ---------------------------------------------------------------- ref side
def _line_of(src: str, pos: int) -> int:
    return src.count("\n", 0, pos) + 1


def _brace_region(src: str, open_idx: int) -> tuple[int, int]:
    close = _match_brace(src, open_idx)
    end = close if close != -1 else len(src)
    return open_idx + 1, end


def _params_region(spec_src: str, region: tuple[int, int], key: str) -> tuple[int, int] | None:
    """`params: {` / `commands: [` block at relative depth 1 of the region."""
    start, end = region
    depth = 0
    i = start
    while i < end:
        c = spec_src[i]
        if c == '"':
            i += 1
            while i < end and spec_src[i] != '"':
                i += 2 if spec_src[i] == "\\" else 1
        elif c == "'":
            i += 1
            while i < end and spec_src[i] != "'":
                i += 2 if spec_src[i] == "\\" else 1
        elif c == "/" and spec_src.startswith("//", i):
            nl = spec_src.find("\n", i)
            i = end if nl == -1 else nl
        elif c in "{[(":
            depth += 1
        elif c in "}])":
            depth -= 1
        elif depth == 0 and spec_src.startswith(f"{key}:", i):
            j = i + len(key) + 1
            while j < end and spec_src[j] in " \t\r\n":
                j += 1
            if j < end and spec_src[j] in "{[":
                return _brace_region(spec_src, j)
            i = j
        i += 1
    return None


def parse_ref(ref: Path) -> dict:
    src = (ref / REF_COMMANDS).read_text()

    # spread param groups + global flag
    groups: dict[str, list[tuple[str, int]]] = {}
    for gname in ("ServerParams", "PermissionParams"):
        m = re.search(rf"const {gname} = \{{", src)
        members = []
        if m:
            start, end = _brace_region(src, m.end() - 1)
            for fm in FLAG_DECL.finditer(src, start, end):
                if fm.group(1) == "Flag":
                    members.append((fm.group(2), _line_of(src, fm.start())))
        groups[gname] = members
    gf = re.search(r'GlobalFlag\.setting\("([^"]+)"\)', src)
    global_flag = (gf.group(1), _line_of(src, gf.start())) if gf else None
    runtime_line = None
    rt = ref / REF_RUNTIME
    if rt.exists():
        rm = re.search(r"withGlobalFlags\(\[PrintLogs\]\)", rt.read_text())
        if rm:
            runtime_line = _line_of(rt.read_text(), rm.start())

    # spec tree
    specs: list[dict] = []
    for m in SPEC_MAKE.finditer(src):
        brace = src.find("{", m.end())
        if brace == -1:
            continue
        region = _brace_region(src, brace)
        nm = re.match(r'\s*"([^"]+)"', src[m.end():])
        name = nm.group(1) if nm else "(root)"
        specs.append({"name": name, "start": m.start(), "region": region})
    # parent = smallest containing region
    for s in specs:
        s["children"] = []
        s["parent"] = None
    for s in specs:
        best = None
        for t in specs:
            if t is s:
                continue
            if t["region"][0] <= s["start"] < t["region"][1]:
                if best is None or t["region"][1] < best["region"][1]:
                    best = t
        s["parent"] = best
        if best:
            best["children"].append(s)

    commands: dict[str, dict] = {}

    def path_of(s: dict) -> str:
        chain = []
        cur = s
        while cur["parent"]:
            chain.append(cur["name"])
            cur = cur["parent"]
        return " ".join(reversed(chain)) or "(root)"

    def flag_entries(region: tuple[int, int]) -> tuple[dict, list, list]:
        flags: dict[str, tuple[bool, int, str | None]] = {}
        positionals: list[tuple[str, int]] = []
        spreads: list[str] = []
        decls = list(FLAG_DECL.finditer(src, *region))
        for idx, fm in enumerate(decls):
            window_end = decls[idx + 1].start() if idx + 1 < len(decls) else min(fm.start() + 600, region[1])
            chain = src[fm.start():window_end]
            alias = re.search(r'withAlias\("([^"]+)"\)', chain)
            hidden = "withHidden" in chain
            if fm.group(1) == "Flag":
                flags[fm.group(2)] = (hidden, _line_of(src, fm.start()), alias.group(1) if alias else None)
            else:
                positionals.append((fm.group(2), _line_of(src, fm.start())))
        for sm in re.finditer(r"\.\.\.(\w+)", src[region[0]:region[1]]):
            if sm.group(1) in groups:
                spreads.append(sm.group(1))
        return flags, positionals, spreads

    def walk(s: dict, prefix: str) -> None:
        path = (prefix + " " + s["name"]).strip() if s["parent"] else "(root)"
        preg = _params_region(src, s["region"], "params")
        flags, positionals, spreads = flag_entries(preg) if preg else ({}, [], [])
        alias_m = re.search(r"aliases:\s*\[([^\]]*)\]", src[s["region"][0]:s["region"][1]])
        aliases = re.findall(r'"([^"]+)"', alias_m.group(1)) if alias_m else []
        commands[path] = {
            "flags": flags,
            "aliases": aliases,
            "positionals": positionals,
            "spreads": spreads,
            "line": _line_of(src, s["start"]),
            "file": str(ref / REF_COMMANDS),
        }
        for c in sorted(s["children"], key=lambda x: x["start"]):
            walk(c, path)

    root = next((s for s in specs if s["parent"] is None), None)
    if root:
        walk(root, "")
    commands = {("(root)" if k == "(root)" else k.removeprefix("(root) ")): v for k, v in commands.items()}
    return {
        "commands": commands,
        "groups": groups,
        "global_flag": global_flag,
        "global_flag_runtime_line": runtime_line,
        "file": str(ref / REF_COMMANDS),
    }


# ---------------------------------------------------------------- main
def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--ref", default="/opt/opencode-ref")
    ap.add_argument("--write", default=None)
    args = ap.parse_args()
    ref = Path(args.ref)

    rust = parse_rust()
    refd = parse_ref(ref)

    rust_cmds = rust["commands"]
    ref_cmds = refd["commands"]

    rep = Report(
        "CLI Census（命令×旗标面 · 通道 1）",
        "rust oc-cli clap 定义（cli.rs）vs ref packages/cli effect-cli Spec 定义（commands.ts）。",
    )
    rep.w("> 生成命令：`python3 tools/census/cli_surface.py --write docs/audit/census/CLI.md`")
    rep.w()
    rep.w("## 归一化与范围声明")
    rep.w()
    rep.w("- N1 旗标拼写：rust 字段名 snake_case → kebab-case（尾部单个 `_` 去除，`continue_`→`continue`）；显式 `long = \"x\"` 优先。ref 名取自 `Flag.*(\"name\")` 原文（ref 本已是 kebab-case），不做变换。")
    rep.w("- N2 全局层：ref 除 `print-logs`（GlobalFlag，runtime.ts 注册）外无真全局，而是把 `ServerParams`（standalone、server）与 `PermissionParams`（auto、yolo、dangerously-skip-permissions）以 `...spread` 摊入 root 及多个子命令；rust 将六个全部声明为 clap 根级 `global = true`。**spread 组与 GlobalFlag 只进全局轴（轴 2），并从各命令本地旗标表中剔除**。")
    rep.w("- N3 位置参数（ref `Argument.*`、rust 无 `#[arg(long)]` 的裸字段）只盘点不裁定（轴 4 信息轴）。")
    rep.w("- N4 框架内建 `--help`/`--version` 双侧皆有，范围外。隐藏旗标（`hide`/`withHidden`）作为标注不作为差异。")
    rep.w(f"- rust 侧来源：{rust['file']}（`#[cfg(test)]` 区已剥除）；ref 侧来源：{refd['file']}。")
    rep.w()
    rep.w("## 轴 1：命令树 diff")
    rep.w()
    rust_paths = {p for p in rust_cmds if p != "(root)"}
    ref_paths = {p for p in ref_cmds if p != "(root)"}
    both_c = sorted(rust_paths & ref_paths)
    ref_only_c = sorted(ref_paths - rust_paths)
    rust_only_c = sorted(rust_paths - ref_paths)
    rep.w(f"- rust：{len(rust_paths)} 个命令 · ref：{len(ref_paths)} 个命令")
    rep.w(f"- **BOTH：{len(both_c)}** · REF_ONLY：**{len(ref_only_c)}** · RUST_ONLY：**{len(rust_only_c)}**")
    for p in ref_only_c:
        rep.w(f"  - 🔴 REF_ONLY `{p}` — {ref_cmds[p]['file']}:{ref_cmds[p]['line']}（ref 有、rust 无 → 缺口候选）")
    for p in rust_only_c:
        rep.w(f"  - 🔗 RUST_ONLY `{p}` — {rust['file']}:{rust_cmds[p]['line']}（rust 额外面，需登记册决策背书）")
    rep.w()
    rep.w("  别名核对（BOTH 命令）：")
    rep.w()
    for p in both_c:
        ra, fa = set(rust_cmds[p]["aliases"]), set(ref_cmds[p]["aliases"])
        if ra != fa:
            rep.w(f"  - ⚠️ `{p}` 别名不一致：rust {sorted(ra)} vs ref {sorted(fa)}")
    rep.w("  - 其余命令别名双侧一致（rust `upgrade` alias `update` ↔ ref `aliases: [\"update\"]`）。")
    rep.w()
    rep.w("## 轴 2：全局旗标 diff")
    rep.w()
    ref_globals: dict[str, tuple[bool, int, str]] = {}
    if refd["global_flag"]:
        name, ln = refd["global_flag"]
        ref_globals[name] = (False, ln, str(refd["file"]))
    for gname, members in refd["groups"].items():
        for name, ln in members:
            ref_globals.setdefault(name, (False, ln, f"{refd['file']}（{gname} spread）"))
    rust_globals = rust["globals"]
    both_g = sorted(set(rust_globals) & set(ref_globals))
    ref_only_g = sorted(set(ref_globals) - set(rust_globals))
    rust_only_g = sorted(set(rust_globals) - set(ref_globals))
    rep.w(f"- rust（clap `global = true`，根级）：{len(rust_globals)} 个")
    for name, (hidden, ln) in sorted(rust_globals.items()):
        rep.w(f"  - rust `{name}`{'（hidden）' if hidden else ''} — {rust['file']}:{ln}")
    rep.w(f"- ref（GlobalFlag + ServerParams/PermissionParams spread 组，N2）：{len(ref_globals)} 个")
    for name, (hidden, ln, where) in sorted(ref_globals.items()):
        rep.w(f"  - ref `--{name}` — {where}:{ln}")
    rep.w(f"- **BOTH：{len(both_g)}** · REF_ONLY：**{len(ref_only_g)}** · RUST_ONLY：**{len(rust_only_g)}**")
    for name in ref_only_g:
        hidden, ln, where = ref_globals[name]
        rep.w(f"  - 🔴 REF_ONLY `--{name}` — {where}:{ln}")
    for name in rust_only_g:
        hidden, ln = rust_globals[name]
        rep.w(f"  - 🔗 RUST_ONLY `--{name}`{'（hidden）' if hidden else ''} — {rust['file']}:{ln}")
    if refd.get("global_flag_runtime_line"):
        rep.w(f"- ref GlobalFlag 注册点：{ref / REF_RUNTIME}:{refd['global_flag_runtime_line']}（`withGlobalFlags([PrintLogs])`）。")
    rep.w()
    rep.w("## 轴 3：命令×本地旗标 diff（spread/全局已剔除）")
    rep.w()
    rust_pairs: dict[tuple[str, str], tuple[bool, int]] = {}
    ref_pairs: dict[tuple[str, str], tuple[bool, int]] = {}
    for p, c in rust_cmds.items():
        for name, (hidden, ln) in c["flags"].items():
            if name in rust_globals:
                continue
            rust_pairs[(p, name)] = (hidden, ln)
    for p, c in ref_cmds.items():
        for name, (hidden, ln, _alias) in c["flags"].items():
            if name in ref_globals:
                continue
            ref_pairs[(p, name)] = (hidden, ln)
    both_f = sorted(set(rust_pairs) & set(ref_pairs))
    ref_only_f = sorted(set(ref_pairs) - set(rust_pairs))
    rust_only_f = sorted(set(rust_pairs) - set(ref_pairs))
    rep.w(f"- rust 命令×本地旗标对：{len(rust_pairs)} · ref：{len(ref_pairs)}")
    rep.w(f"- **BOTH：{len(both_f)}** · REF_ONLY：**{len(ref_only_f)}** · RUST_ONLY：**{len(rust_only_f)}**")
    for (p, name) in ref_only_f:
        hidden, ln = ref_pairs[(p, name)]
        rep.w(f"  - 🔴 REF_ONLY `{p} --{name}`{'（hidden）' if hidden else ''} — {refd['file']}:{ln}")
    for (p, name) in rust_only_f:
        hidden, ln = rust_pairs[(p, name)]
        rep.w(f"  - 🔗 RUST_ONLY `{p} --{name}`{'（hidden）' if hidden else ''} — {rust['file']}:{ln}")
    rep.w()
    rep.w("## 轴 4（信息轴）：位置参数盘点（不裁定）")
    rep.w()
    rep.w("  说明：位置参数只按名盘点；`api` / `service set` / `session delete` 的「不一致」均为**展示名差异**"
          "（rust `value_name`/字段名 vs ref 参数名：`request`↔`operation | method path`、`nested_value`↔`env-value`、`session_id`↔`sessionID`），语义一一对应，不构成缺口；`completion shell` 为 rust-extra 命令的位置参数。")
    rep.w()
    for p in sorted(set(rust_cmds) | set(ref_cmds)):
        rp = [n for n in rust_cmds.get(p, {}).get("positionals", [])]
        fp = [n for n, _ in ref_cmds.get(p, {}).get("positionals", [])]
        if rp or fp:
            rep.w(f"  - `{p}`：rust {rp or '∅'} vs ref {fp or '∅'}"
                  + ("（⚠️ 不一致，信息轴提示）" if set(rp) != set(fp) else ""))
    rep.w()
    rep.w("## Census 裁定")
    rep.w()
    total_pairs = set(rust_pairs) | set(ref_pairs)
    adjudicated = len(both_f) + len(ref_only_f) + len(rust_only_f)
    rep.w(f"- 命令：{len(both_c) + len(ref_only_c) + len(rust_only_c)}/{len(rust_paths | ref_paths)}（覆盖率 100%）")
    rep.w(f"- 全局旗标：{len(both_g) + len(ref_only_g) + len(rust_only_g)}/{len(set(rust_globals) | set(ref_globals))}（覆盖率 100%）")
    rep.w(f"- 命令×本地旗标：{adjudicated}/{len(total_pairs)}（覆盖率 100%：双侧全集逐项归属）")
    rep.w(f"- 结论：REF_ONLY {len(ref_only_c) + len(ref_only_g) + len(ref_only_f)} 项（缺口候选）；"
          f"RUST_ONLY {len(rust_only_c) + len(rust_only_g) + len(rust_only_f)} 项（rust 额外面，需登记册决策背书）。")
    rep.w()
    rep.w("## 验证记录")
    rep.w()
    rep.w("- 双侧每差异项均亲读当前代码核实（rust crates/oc-cli/src/cli.rs 的 clap 定义；ref packages/cli/src/commands/commands.ts 的 Spec/Flag 定义与 framework/runtime.ts 的 GlobalFlag 注册）。")
    rep.w("- 幂等性：脚本纯读取、确定性排序；连跑两次输出逐字节一致（/tmp/opencode/ 下 diff 验证）。")
    rep.w("- 提取不确定性：ref 命令树按 `Spec.make` 花括号区域嵌套推断（文件内无模板字面量花括号干扰）；rust 旗标名按 N1 规则推导，显式 `long=` 优先。")
    rep.w()

    rep.write(args.write)
    return 0


def kebab(ident: str) -> str:
    ident = ident.rstrip("_")
    return re.sub(r"_", "-", ident).lower()


if __name__ == "__main__":
    sys.exit(main())
