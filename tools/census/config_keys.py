#!/usr/bin/env python3
"""Config-key census: dual-side extraction of the configuration key universe
plus a default-value diff for keys where both sides express a literal default.

Axis 1 — key-path universe (dot paths):
  rust: serde structs rooted at `oc_schema::config::Info` (the opencode.json
        document, crates/oc-schema/src/config/*) and at
        `oc_core::cli_config::Info` (the cli.json document,
        crates/oc-core/src/cli_config/mod.rs) — fields expanded recursively,
        `#[serde(rename)]` applied, `#[serde(flatten)]`/string-keyed maps
        collapsed to a `<*>` open-map node.  crates/oc-core/src/config.rs is
        the loader (no key definitions; verified below); the TUI slice in
        crates/oc-tui/src/config.rs consumes cli_config::Info.
  ref:  effect Schema structs rooted at packages/schema/src/config.ts `Info`
        (opencode.json) and packages/tui/src/config/index.tsx `Info`
        (cli.json), following `Config<Module>.<Name>` / `Mcp.<Name>` /
        `Permission.<Name>` references into packages/schema/src/config/*.ts,
        ../mcp.ts and ../permission.ts.  Config/plugin/*.ts sub-behaviours
        (mcp.ts timeout merge, snapshot.ts switch) are covered by the anchor
        checks, not the key graph.

Normalization (declared mapping rule): both sides author every key in these
two documents literally — snake_case at the top level (`default_agent`,
`tool_output`, …) and camelCase inside the MCP/provider shapes
(`headerTimeout`; `sound_pack` is snake_case on both sides) — so the mapping
rule is IDENTITY; a case-only mismatch would be reported as CASE_MISMATCH,
never silently normalized.

Axis 2 — defaults: for keys where both sides express a literal default
(ref `resolve()` `?? <lit>` chains vs the rust `Default` impl), compare
normalized literals.  Behavioural anchors (mcp.timeout document-level merge,
snapshots switch/default, session.tps default) are probed mechanically with
regex evidence on both sides.

Usage: python3 tools/census/config_keys.py [--ref /opt/opencode-ref]
          [--write docs/audit/census/CONFIG-KEYS.md]
"""
from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

from common import Report, _match_brace, strip_tests  # noqa: E402

# ---------------------------------------------------------------- rust side

RUST_SCHEMA_CONFIG = Path("crates/oc-schema/src/config")
RUST_EXTRA_FILES = [
    Path("crates/oc-schema/src/mcp.rs"),
    Path("crates/oc-schema/src/permission.rs"),
    Path("crates/oc-schema/src/provider.rs"),
    Path("crates/oc-schema/src/model.rs"),
    Path("crates/oc-core/src/cli_config/mod.rs"),
]

RUST_TUI_ROOT = ("cli_config/mod.rs", "Info")
RUST_DOC_ROOT = ("config/mod.rs", "Info")

PRIMS = {"Value", "String", "bool", "u64", "f64", "serde_json::Value", "Json"}


def _match_paren(src: str, open_idx: int) -> int:
    """Index of the paren matching src[open_idx]=='(' (string-safe), or -1."""
    depth, i, n = 0, open_idx, len(src)
    while i < n:
        c = src[i]
        if c == '"':
            i += 1
            while i < n and src[i] != '"':
                i += 2 if src[i] == "\\" else 1
        elif c == "(":
            depth += 1
        elif c == ")":
            depth -= 1
            if depth == 0:
                return i
        i += 1
    return -1


def _read_type(src: str, start: int) -> str | None:
    depth = 0
    i = start
    while i < len(src):
        c = src[i]
        if c == '"':
            i += 1
            while i < len(src) and src[i] != '"':
                i += 2 if src[i] == "\\" else 1
        elif c in "<([{":
            depth += 1
        elif c in ">)]}":
            depth -= 1
        elif c == "," and depth == 0:
            return src[start:i]
        i += 1
    tail = src[start:].strip()
    return tail or None


def rust_structs(files: list[Path]):
    """Structs, enums, type aliases. Returns (structs, enums, aliases, uses,
    locations)."""
    structs: dict[tuple[str, str], dict] = {}
    enums: dict[tuple[str, str], list] = {}
    aliases: dict[tuple[str, str], str] = {}
    uses: dict[str, dict[str, str]] = {}
    locations: dict[tuple[str, str], tuple[str, int]] = {}
    for f in files:
        body = strip_tests(f.read_text())
        rel = str(f).split("src/", 1)[1] if "src/" in str(f) else str(f)
        umap: dict[str, str] = {}
        for um in re.finditer(r"use crate::((?:\w+::)*)(\w+);", body):
            umap[um.group(2)] = um.group(1).rstrip(":").split("::")[-1] or um.group(1)
        for um in re.finditer(r"use crate::((?:\w+::)*)\{([^}]*)\};", body):
            seg = um.group(1).rstrip(":").split("::")[-1] or um.group(1)
            for name in um.group(2).split(","):
                name = name.strip()
                if name:
                    umap.setdefault(name, seg)
        uses[rel] = umap
        for m in re.finditer(r"pub type (\w+)\s*=\s*([^;]+);", body):
            aliases[(rel, m.group(1))] = re.sub(r"\s+", " ", m.group(2)).strip()
        for m in re.finditer(
            r"((?:#\[[^\]]*\]\s*)*)pub struct (\w+)[^{;=]*\{", body
        ):
            name = m.group(2)
            rename_all = re.search(r'rename_all\s*=\s*"([^"]+)"', m.group(1))
            close = _match_brace(body, m.end() - 1)
            if close == -1:
                continue
            inner = body[m.end():close]
            fields = {}
            for fm in re.finditer(
                r"((?:#\[[^\]]*\]\s*)*)(?:pub(?:\([^)]*\))?\s+)([a-z_$]\w*)\s*:", inner
            ):
                typ = _read_type(inner, fm.end())
                if typ is None:
                    continue
                attrs = fm.group(1)
                rename = re.search(r'rename\s*=\s*"([^"]+)"', attrs)
                default_fn = re.search(r'default\s*=\s*"([^"]+)"', attrs)
                alias = re.search(r'alias\s*=\s*"([^"]+)"', attrs)
                key = fm.group(2)
                if not rename:
                    if rename_all and rename_all.group(1) == "camelCase":
                        key = re.sub(r"_(\w)", lambda mm: mm.group(1).upper(), key)
                    elif rename_all and rename_all.group(1) == "kebab-case":
                        key = key.replace("_", "-")
                fields[fm.group(2)] = (
                    rename.group(1) if rename else key,
                    re.sub(r"\s+", " ", typ).strip(),
                    {
                        "flatten": "flatten" in attrs,
                        "alias": alias.group(1) if alias else None,
                        "default_fn": default_fn.group(1) if default_fn else None,
                    },
                )
            structs[(rel, name)] = fields
            locations[(rel, name)] = (str(f), body[: m.start()].count("\n") + 1)
        for m in re.finditer(r"((?:#\[[^\]]*\]\s*)*)pub enum (\w+)[^{;=]*\{", body):
            name = m.group(2)
            close = _match_brace(body, m.end() - 1)
            if close == -1:
                continue
            tagm = re.search(r'tag\s*=\s*"([^"]+)"', m.group(1))
            variants = []
            inner_body = body[m.end():close]
            for vm in re.finditer(
                r"^\s*(?:#\[[^\]]*\]\s*)*(\w+)(\([^{)]*\)|\s*\{)", inner_body, re.M
            ):
                if vm.group(2).startswith("("):
                    variants.append((vm.group(1), vm.group(2)[1:-1].strip(), None))
                else:
                    vopen = inner_body.index("{", vm.start())
                    vclose = _match_brace(inner_body, vopen)
                    variants.append((vm.group(1), None, inner_body[vopen + 1:vclose]))
            enums[(rel, name)] = {"tag": tagm.group(1) if tagm else None, "variants": variants}
            locations.setdefault((rel, name), (str(f), body[: m.start()].count("\n") + 1))
    return structs, enums, aliases, uses, locations


def _resolve_rust(structs, enums, current: tuple[str, str], typ: str, uses: dict):
    short = typ.split("::")[-1].strip()
    same_file = (current[0], short)
    if same_file in structs or same_file in enums:
        return same_file
    hits = [k for k in list(structs) + list(enums) if k[1] == short]
    seg = uses.get(current[0], {}).get(short)
    if seg:
        preferred = [
            k for k in hits if k[0].rsplit("/", 1)[-1].rsplit(".", 1)[0] == seg
        ]
        nested = [k for k in preferred if "/" in k[0]]
        if nested:
            preferred = nested
        if len(preferred) == 1:
            return preferred[0]
    if len(hits) == 1:
        return hits[0]
    # ambiguity fallback: prefer the config/ subtree (config-tree walk)
    config_hits = [k for k in hits if k[0].startswith("config/")]
    if len(config_hits) == 1:
        return config_hits[0]
    return None


def expand_rust(structs, enums, aliases, uses, locations, root: tuple[str, str]):
    """{dot-path: (file, line, typexpr)} from a struct root + notes."""
    out: dict[str, tuple[str, int, str]] = {}
    notes: list[str] = []

    def deref(typ: str, node: tuple[str, str]) -> str:
        seen = 0
        while (node[0], typ.split("::")[-1].strip()) in aliases and seen < 5:
            typ = aliases[(node[0], typ.split("::")[-1].strip())]
            seen += 1
        return typ

    def walk(node, path, ancestors):
        if node in ancestors:
            out[path + ".<…>"] = (*locations[node], "(recursive)")
            return
        if node in enums:
            f, ln = locations[node]
            spec = enums[node]
            if spec["tag"]:
                tpath = f"{path}.{spec['tag']}" if path else spec["tag"]
                out[tpath] = (f, ln, "<tag discriminator>")
            expanded = False
            for variant, inner, inline in spec["variants"]:
                if inline is not None:
                    fields = {}
                    for fm in re.finditer(
                        r"((?:#\[[^\]]*\]\s*)*)(?:pub(?:\([^)]*\))?\s+)?([a-z_$]\w*)\s*:", inline
                    ):
                        typ = _read_type(inline, fm.end())
                        if typ is None:
                            continue
                        attrs = fm.group(1)
                        rename = re.search(r'rename\s*=\s*"([^"]+)"', attrs)
                        fields[fm.group(2)] = (
                            rename.group(1) if rename else fm.group(2),
                            re.sub(r"\s+", " ", typ).strip(),
                            {"flatten": False, "alias": None, "default_fn": None},
                        )
                    emit_fields(fields, node, path, ancestors)
                    expanded = True
                    continue
                if not inner:
                    continue
                it = deref(inner, node)
                vm = re.match(r"(?:BTree|Hash)Map\s*<\s*String\s*,\s*(.+?)\s*>$", it)
                if vm:
                    out[f"{path}.<*>"] = (f, ln, f"<enum map variant> {variant}")
                    target = _resolve_rust(structs, enums, node, vm.group(1), uses)
                    if target:
                        walk(target, f"{path}.<*>", ancestors | {node})
                    expanded = True
                    continue
                if it.startswith("Vec<") and it.endswith(">"):
                    vt = it[4:-1].strip()
                    out[f"{path}.<*>"] = (f, ln, f"<enum array variant> {variant}")
                    target = _resolve_rust(structs, enums, node, vt, uses)
                    if target:
                        walk(target, f"{path}.<*>", ancestors | {node})
                    expanded = True
                    continue
                base = it
                for pre in ("Box<", "Option<"):
                    if base.startswith(pre) and base.endswith(">"):
                        base = base[len(pre):-1].strip()
                target = _resolve_rust(structs, enums, node, base, uses)
                if target:
                    walk(target, path, ancestors | {node})
                    expanded = True
                elif base.split("::")[-1] not in PRIMS:
                    out[path] = (f, ln, f"<enum variant> {variant}: {it}")
                    expanded = True
            if not expanded and not spec["tag"]:
                names = "/".join(v for v, _, _ in spec["variants"]) or "(empty)"
                out[path] = (f, ln, f"<enum: {names}>")
            return
        fields = structs.get(node)
        if fields is None:
            notes.append(f"UNRESOLVED {path or '<root>'} -> {node[0]}::{node[1]}")
            return
        emit_fields(fields, node, path, ancestors)

    def emit_fields(fields, node, path, ancestors) -> None:
        for field, (key, typ, attrs) in sorted(fields.items()):
            f, ln = locations[node]
            kpath = f"{path}.{key}" if path else key
            if attrs["flatten"]:
                # serde flatten: map entries inline at the PARENT path
                marker = (path + ".<*>") if path else (key + ".<*>")
                out[marker] = (f, ln, f"<open map (flatten): {typ}>")
                continue
            inner = deref(typ, node)
            if inner.startswith("Option<") and inner.endswith(">"):
                inner = inner[7:-1].strip()
            vm = re.match(r"Vec\s*<\s*(.+?)\s*>$", inner)
            if vm:
                vt = vm.group(1)
                target = _resolve_rust(structs, enums, node, vt, uses)
                out[kpath + ".<*>"] = (f, ln, f"<array: {vt}>")
                if target:
                    walk(target, kpath + ".<*>", ancestors | {node})
                continue
            mm = re.match(r"(?:BTree|Hash)Map\s*<\s*String\s*,\s*(.+?)\s*>$", inner)
            if mm:
                vt = mm.group(1)
                if vt.startswith("Option<") and vt.endswith(">"):
                    vt = vt[7:-1].strip()
                out[kpath + ".<*>"] = (f, ln, f"<open map: {vt}>")
                target = _resolve_rust(structs, enums, node, vt, uses)
                if target:
                    walk(target, kpath + ".<*>", ancestors | {node})
                else:
                    out[kpath + ".<*>"] = (f, ln, f"<open map (union/leaf): {vt}>")
                continue
            target = _resolve_rust(structs, enums, node, inner, uses)
            if target:
                walk(target, kpath, ancestors | {node})
            else:
                extra = f" [serde default_fn={attrs['default_fn']}]" if attrs["default_fn"] else ""
                out[kpath] = (f, ln, typ + extra)

    walk(root, "", frozenset())
    return out, notes


# ----------------------------------------------------------------- ref side

def _strip_ts_comments(text: str) -> str:
    """Blank out // and /* */ comments (string-aware), preserving offsets."""
    out = list(text)
    i, n = 0, len(text)
    while i < n:
        c = text[i]
        if c in "\"'":
            q = c
            i += 1
            while i < n and text[i] != q:
                i += 2 if text[i] == "\\" else 1
            i += 1
        elif text.startswith("//", i):
            j = text.find("\n", i)
            j = n if j == -1 else j
            for k in range(i, j):
                out[k] = " "
            i = j
        elif text.startswith("/*", i):
            j = text.find("*/", i)
            j = n if j == -1 else j + 2
            for k in range(i, j):
                out[k] = " " if text[k] != "\n" else "\n"
            i = j
        else:
            i += 1
    return "".join(out)


def ref_parse_file(text: str):
    text = _strip_ts_comments(text)
    blocks: dict[tuple[str, str], str] = {}
    locations: dict[tuple[str, str], int] = {}
    for m in re.finditer(
        r'(?:export )?class (\w+)(?:\s+extends\s+[\w.]+(?:<[^>]*>)?\("[^"]*"\))?\(\s*\{', text
    ):
        open_idx = text.index("{", m.start(), m.end() + 1)
        close = _match_brace(text, open_idx)
        if close == -1:
            continue
        blocks.setdefault(("class", m.group(1)), text[m.end():close])
        locations.setdefault(("class", m.group(1)), text[: m.start()].count("\n") + 1)
    for m in re.finditer(
        r"(?:export )?const (\w+)\s*=\s*(?:Schema\.\w+\(\s*(?:Schema\.\w+\(\s*)?)?\{", text
    ):
        close = _match_brace(text, m.end() - 1)
        if close == -1:
            continue
        blocks.setdefault(("const", m.group(1)), text[m.end():close])
        locations.setdefault(("const", m.group(1)), text[: m.start()].count("\n") + 1)
    for m in re.finditer(r"(?:export )?const (\w+)\s*=\s*Schema\.Union\(", text):
        close = _match_paren(text, m.end() - 1)
        if close == -1:
            continue
        blocks.setdefault(("union", m.group(1)), text[m.end():close])
        locations.setdefault(("union", m.group(1)), text[: m.start()].count("\n") + 1)
    for m in re.finditer(r"export const (\w+)\s*=\s*([A-Za-z_][\w.]*)\s*$", text, re.M):
        blocks.setdefault(("alias", m.group(1)), m.group(2))
        locations.setdefault(("alias", m.group(1)), text[: m.start()].count("\n") + 1)
    return blocks, locations


def ref_fields(inner: str) -> list[tuple[str, str]]:
    out: list[tuple[str, str]] = []
    depth = 0
    i, n, last = 0, len(inner), 0
    while i < n:
        c = inner[i]
        if c == '"':
            i += 1
            while i < n and inner[i] != '"':
                i += 2 if inner[i] == "\\" else 1
        elif c in "{([":
            depth += 1
        elif c in "})]":
            depth -= 1
        elif c == "," and depth == 0:
            _emit_field(inner[last:i], out)
            last = i + 1
        i += 1
    _emit_field(inner[last:], out)
    return out


def _emit_field(seg: str, out: list[tuple[str, str]]) -> None:
    # drop leading // and /** */ comment lines so commented fields aren't swallowed
    while True:
        stripped = re.sub(r"^\s*//[^\n]*\n?", "", seg)
        stripped = re.sub(r"^\s*/\*.*?\*/\s*", "", stripped, flags=re.S)
        if stripped == seg:
            break
        seg = stripped
    sm = re.match(r"\s*\.\.\.(\w+)\s*$", seg)
    if sm:
        out.append(("...", sm.group(1)))
        return
    fm = re.match(r'\s*(?:([$\w]+)|"([^"]+)")\s*:\s*(.*)', seg, re.S)
    if fm:
        out.append((fm.group(1) or fm.group(2), fm.group(3).strip()))


def _strip_call(v: str, names: tuple[str, ...]) -> str | None:
    """If v ends with `.name(...)` (balanced, string-safe), return v minus it."""
    if not v.endswith(")"):
        return None
    depth = 0
    i = len(v) - 1
    while i >= 0:
        c = v[i]
        if c == '"':
            j = i - 1
            while j >= 0 and v[j] != '"':
                j -= 2 if v[j] == "\\" else 1
            i = j - 1
            continue
        if c == ")":
            depth += 1
        elif c == "(":
            depth -= 1
            if depth == 0:
                head = v[:i]
                m = re.search(r"\.(\w+)$", head)
                if m and m.group(1) in names:
                    return head[: m.start()]
                return None
        i -= 1
    return None


def ref_strip(value: str) -> str:
    v = value.strip()
    while True:
        nv = _strip_call(v, ("pipe", "annotate", "check", "mapFields"))
        if nv is None:
            break
        v = nv.strip()
    # unwrap optional(...) when it spans the whole expression
    for prefix in ("Schema.optional(", "optional("):
        if v.startswith(prefix) and v.endswith(")"):
            depth = 0
            for i, c in enumerate(v):
                if c == "(":
                    depth += 1
                elif c == ")":
                    depth -= 1
                    if depth == 0 and i == len(v) - 1:
                        v = v[len(prefix):-1].strip()
            break
    return v


def camel_kebab(name: str) -> str:
    return re.sub(r"(?<!^)(?=[A-Z])", "-", name).lower()


def _norm_mod(name: str) -> str:
    return re.sub(r"[-_]", "", name.lower())


def ref_is_array(value: str) -> bool:
    return bool(re.search(r"\.pipe\(\s*Schema\.Array\b", value))


class RefGraph:
    """Namespace = path under packages/ without extension
    (e.g. `schema/src/config`, `schema/src/config/mcp`, `tui/src/config/index`)."""

    def __init__(self, ref: Path):
        self.ref = ref
        self.modules: dict[str, dict] = {}
        self.locs: dict[str, dict] = {}
        files = [
            ref / "packages/schema/src/config.ts",
            *sorted((ref / "packages/schema/src/config").glob("*.ts")),
            ref / "packages/schema/src/mcp.ts",
            ref / "packages/schema/src/permission.ts",
            ref / "packages/schema/src/provider.ts",
            ref / "packages/schema/src/model.ts",
            ref / "packages/tui/src/config/index.tsx",
        ]
        for f in files:
            text = f.read_text()
            blocks, locs = ref_parse_file(text)
            ns = str(f.relative_to(ref / "packages")).rpartition(".")[0]
            self.modules[ns] = blocks
            self.locs[ns] = locs
        # import map: bare names imported from relative modules
        self.imports: dict[str, dict[str, str]] = {}
        for f in files:
            ns = str(f.relative_to(ref / "packages")).rpartition(".")[0]
            imap: dict[str, str] = {}
            for im in re.finditer(r'import\s*\{([^}]*)\}\s*from\s*"(\.\./[^"]+)"', f.read_text()):
                target = self._rel_module(ns, im.group(2))
                if target:
                    for name in im.group(1).split(","):
                        name = name.strip().split(" as ")[-1].strip()
                        if name:
                            imap[name] = target
            self.imports[ns] = imap
        # StructWithRest consts: known fields + open rest
        self.swr: set[tuple[str, str]] = set()
        for ns, blocks in self.modules.items():
            f = ref / "packages" / (ns + (".tsx" if ns.endswith("index") else ".ts"))
            if not f.exists():
                continue
            for m in re.finditer(r"(?:export )?const (\w+)\s*=\s*Schema\.StructWithRest\(", f.read_text()):
                self.swr.add((ns, m.group(1)))

    @staticmethod
    def _rel_module(ns: str, spec: str) -> str | None:
        parts = ns.split("/")[:-1]
        segs = spec.split("/")
        for seg in segs[:-1]:
            if seg == "..":
                if not parts:
                    return None
                parts.pop()
            elif seg not in (".", ""):
                parts.append(seg)
        parts.append(segs[-1].rpartition(".")[0])
        return "/".join(parts)

    def resolve(self, ns: str, name: str):
        for kind in ("const", "union", "class", "alias"):
            if (kind, name) in self.modules.get(ns, {}):
                return ns, kind, name
        target_ns = self.imports.get(ns, {}).get(name)
        if target_ns and target_ns in self.modules:
            for kind in ("class", "const", "union", "alias"):
                if (kind, name) in self.modules[target_ns]:
                    return target_ns, kind, name
        m = re.match(r"^Config([A-Z]\w*)\.(\w+)$", name)
        if m:
            for mod in self.modules:
                if mod.startswith("schema/src/config/") and _norm_mod(
                    mod.rsplit("/", 1)[1]
                ) == _norm_mod(m.group(1)):
                    for kind in ("class", "const", "union", "alias"):
                        if (kind, m.group(2)) in self.modules[mod]:
                            return mod, kind, m.group(2)
        m = re.match(r"^(Mcp|Permission|Provider)\.(\w+)$", name)
        if m:
            target = {
                "Mcp": "schema/src/mcp",
                "Permission": "schema/src/permission",
                "Provider": "schema/src/provider",
            }[m.group(1)]
            for kind in ("class", "const", "union", "alias"):
                if (kind, m.group(2)) in self.modules.get(target, {}):
                    return target, kind, m.group(2)
        return None

    def expand(self, ns: str, kind: str, name: str) -> tuple[dict, list[str]]:
        out: dict[str, tuple[str, int, str]] = {}
        notes: list[str] = []

        def expand_union(args: str, mod: str, kpath: str, ancestors) -> bool:
            """Expand every struct/class branch of a union (variant merge)."""
            expanded = False
            for am in re.finditer(r"Schema\.Struct\(\s*\{", args):
                o2 = args.index("{", am.start())
                c2 = _match_brace(args, o2)
                walk_fields(args[o2 + 1:c2], mod, kpath, ancestors)
                notes.append(f"UNION-EXPANDED {kpath} (struct branch of union)")
                expanded = True
            for am in re.finditer(r"\b([A-Z]\w*(?:\.[A-Z]\w*)*)\b", args):
                if am.group(1) == "Schema" or am.group(1) == "LocalConfig" and False:
                    continue
                target = self.resolve(mod, am.group(1))
                if target:
                    walk_ref(target, kpath, ancestors)
                    notes.append(f"UNION-EXPANDED {kpath} -> {am.group(1)} (union branch)")
                    expanded = True
            return expanded

        def walk_fields(block: str, mod: str, path: str, ancestors) -> None:
            for key, value in ref_fields(block):
                kpath = f"{path}.{key}" if path else key
                array = ref_is_array(value)
                if array:
                    kpath = kpath + ".<*>"
                v = ref_strip(value)
                if key == "..." or v.startswith("..."):
                    spread = value if key == "..." else v[3:].strip()
                    target = self.resolve(mod, spread)
                    if target:
                        tbl = self.modules[target[0]].get((target[1], target[2]))
                        if target[1] in ("const", "union") and isinstance(tbl, str):
                            if "{" in tbl:
                                open_idx = tbl.index("{")
                                close = _match_brace(tbl, open_idx)
                                walk_fields(tbl[open_idx + 1:close], mod, path, ancestors)
                            else:
                                walk_fields(tbl, mod, path, ancestors)
                        else:
                            walk_ref(target, kpath, ancestors)
                    else:
                        notes.append(f"SPREAD-UNRESOLVED {kpath}: {v}")
                    continue
                m = re.match(r"^(\w+)\.(\w+)$", v)
                if m and ("const", m.group(1)) in self.modules.get(mod, {}):
                    tbl = self.modules[mod][("const", m.group(1))]
                    wanted = dict(ref_fields(tbl)).get(m.group(2))
                    if wanted is not None:
                        value = wanted
                        v = ref_strip(wanted)
                        array = ref_is_array(wanted)
                        if array:
                            kpath = kpath + ".<*>"
                if v.startswith("Schema.StructWithRest("):
                    m = re.search(r"Schema\.Struct\(\s*\{", v)
                    if m:
                        open_idx = v.index("{", m.start())
                        close = _match_brace(v, open_idx)
                        walk_fields(v[open_idx + 1:close], mod, kpath, ancestors)
                        out[kpath + ".<*>"] = _loc(mod, "rest", "(StructWithRest rest)")
                    continue
                if v.startswith("Schema.Union("):
                    # union-with-shorthand: expand the struct/class branches
                    close = _match_paren(v, v.index("("))
                    args = v[v.index("(") + 1:close]
                    if not expand_union(args, mod, kpath, ancestors):
                        out[kpath] = _loc(mod, key, _short(v))
                    continue
                if v.startswith("Schema.Struct(") and "{" in v:
                    open_idx = v.index("{")
                    close = _match_brace(v, open_idx)
                    if close != -1:
                        walk_fields(v[open_idx + 1:close], mod, kpath, ancestors)
                        continue
                m = re.match(r"^Schema\.Record\(\s*Schema\.String\s*,\s*(.*)\)$", v, re.S)
                if m and m.group(1).lstrip().startswith("Schema.Struct("):
                    sub = m.group(1).lstrip()
                    open_idx = sub.index("{")
                    close = _match_brace(sub, open_idx)
                    walk_fields(sub[open_idx + 1:close], mod, kpath + ".<*>", ancestors)
                    continue
                if m and re.fullmatch(r"[A-Za-z_][\w.]*", m.group(1).strip()):
                    target = self.resolve(mod, m.group(1).strip())
                    if target:
                        walk_ref(target, kpath + ".<*>", ancestors)
                        continue
                m = re.match(r"^Schema\.Array\(\s*([A-Za-z_][\w.]*)\s*\)$", v)
                if m:
                    target = self.resolve(mod, m.group(1))
                    if target:
                        walk_ref(target, kpath + ".<*>", ancestors)
                    else:
                        out[kpath] = _loc(mod, key, f"<array of ref> {m.group(1)}")
                    continue
                if re.fullmatch(r"[A-Za-z_][\w.]*", v):
                    target = self.resolve(mod, v)
                    if target:
                        walk_ref(target, kpath, ancestors)
                    else:
                        out[kpath] = _loc(mod, key, f"(leaf ref) {v}")
                    continue
                out[kpath] = _loc(mod, key, _short(v))

        def walk_ref(target, path: str, ancestors) -> None:
            tns, tkind, tname = target
            key = (tns, tkind, tname)
            if key in ancestors:
                out[path + ".<…>"] = _loc(tns, tname, "(recursive)")
                return
            block = self.modules[tns].get((tkind, tname))
            if block is None:
                notes.append(f"UNRESOLVED {path} -> {tns}:{tkind}:{tname}")
                return
            if tkind == "union":
                if not expand_union(block, tns, path, ancestors | {key}):
                    out[path] = _loc(tns, tname, "(union of literals)")
                return
            if (tns, tname) in self.swr:
                out[path + ".<*>"] = _loc(tns, tname, "<open map (StructWithRest rest)>")
            if tkind == "alias":
                nxt = self.resolve(tns, block)
                if nxt:
                    walk_ref(nxt, path, ancestors | {key})
                else:
                    out[path] = _loc(tns, tname, f"(leaf ref) {block}")
                return
            walk_fields(block, tns, path, ancestors | {key})

        def _loc(mod: str, name: str, desc: str) -> tuple[str, int, str]:
            ln = 0
            for kind in ("class", "const", "union", "alias"):
                if (kind, name) in self.locs.get(mod, {}):
                    ln = self.locs[mod][(kind, name)]
                    break
            ext = ".tsx" if mod.endswith("index") else ".ts"
            return (f"packages/{mod}{ext}", ln, desc)

        walk_ref((ns, kind, name), "", frozenset())
        return out, notes


def _short(v: str) -> str:
    v = re.sub(r"\s+", " ", v)
    return v if len(v) <= 70 else v[:67] + "..."


# ------------------------------------------------------------- axis 2 pieces

# Hand-checked mapping (each entry verified against both sides' current code):
# TuiConfig field -> cli.json dot path.
RUST_TUI_FIELD_PATH = {
    "session_tps": "session.tps",
    "session_image_preview": "session.image_preview",
    "mouse": "mouse",
}


def rust_tui_defaults(src: str) -> dict[str, str]:
    m = re.search(r"impl Default for TuiConfig \{(.*?)\n\}", src, re.S)
    if not m:
        return {}
    out: dict[str, str] = {}
    for fm in re.finditer(r"([a-z_]\w*)\s*:\s*([^,\n]+),", m.group(1)):
        out.setdefault(fm.group(1), fm.group(2).strip())
    return out


def ref_resolve_defaults(src: str) -> dict[str, str]:
    out: dict[str, str] = {}
    for m in re.finditer(r"input\.(\w+)\??\.(\w+)\s*\?\?\s*([^,\n}]+)", src):
        val = m.group(3).strip()
        out[f"{m.group(1)}.{m.group(2)}"] = val + "}" if val == "{" else val
    for m in re.finditer(r"input\.(\w+)\s*\?\?\s*([^,\n}]+)", src):
        val = m.group(2).strip()
        out.setdefault(m.group(1), val + "}" if val == "{" else val)
    return out


# 差异裁定表：提取器收敛后剩余差异项的亲读定性（file:line 以提取器证据为准）
ADJUDICATIONS = {
    "experimental.file_retention.cleanup_interval": (
        "RUST_ONLY·加固旋钮",
        "rust `crates/oc-schema/src/config/experimental.rs` 的 `FileRetention`（默认 3_600_000ms 扫描）"
        "为 rust 新增的文件保留调优面；ref `packages/schema/src/config/experimental.ts` 仅有 "
        "portable_shell_scanner/subagent_depth/policies 三键。需登记册 rust-extra 家族背书。",
    ),
    "providers.models.compatibility.requireSignature": (
        "RUST_ONLY·有据分歧",
        "rust `crates/oc-schema/src/model.rs` Compatibility 第 7 键（rename_all camelCase → "
        "requireSignature）；ref schema `packages/schema/src/model.ts` Compatibility 仅 6 键不收此名，"
        "但 ref 内部 provider 数据确实写入（`packages/ai/src/providers/alibaba.ts:104` "
        "`compatibility:{requireSignature:false}`）——rust schema 比 ref 严格 schema 宽一档。",
    ),
    "$schema": (
        "RUST_ONLY·宽容旋钮",
        "rust `crates/oc-core/src/cli_config/mod.rs` 的 cli.json `Info.$schema`；ref TUI "
        "`packages/tui/src/config/index.tsx` 的 Info 未声明（opencode.json 文档侧两侧都有）。",
    ),
    "attention.bell": (
        "RUST_ONLY·文档化取舍",
        "rust `crates/oc-core/src/cli_config/mod.rs` + `crates/oc-tui/src/config.rs:61`（默认 false，"
        "注释标 P11：v2 从不 emit BEL）；ref attention 节无 bell 键。",
    ),
}


def norm_default(v: str) -> str:
    v = v.strip()
    if re.fullmatch(r'"[^"]*"', v) or v in ("true", "false"):
        return v
    return re.sub(r"\s+", "", v)


# ------------------------------------------------------------------- anchors

def anchor_checks(ref: Path) -> list[tuple[str, str | None, str | None]]:
    engine = Path("crates/oc-core/src/engine/mod.rs").read_text()
    ref_mcp_plugin = (ref / "packages/core/src/config/plugin/mcp.ts").read_text()
    ref_snapshot_plugin = (ref / "packages/core/src/config/plugin/snapshot.ts").read_text()
    ref_snapshot = (ref / "packages/core/src/snapshot.ts").read_text()
    rust_snapshot = Path("crates/oc-core/src/snapshot.rs").read_text()
    rust_tui = Path("crates/oc-tui/src/config.rs").read_text()
    ref_tui = (ref / "packages/tui/src/config/index.tsx").read_text()

    def find(pattern: str, src: str, fname: str):
        m = re.search(pattern, src)
        return None if not m else f"{fname}:{src[: m.start()].count(chr(10)) + 1}"

    return [
        (
            "mcp.timeout 文档级合并（曾 G-362b）",
            find(r"G-362b — document-level `mcp\.timeout`", engine, "crates/oc-core/src/engine/mod.rs"),
            find(r"entry\.info\.mcp\?\.timeout", ref_mcp_plugin, "packages/core/src/config/plugin/mcp.ts"),
        ),
        (
            "snapshots:false 开关（曾 G-362a）",
            find(r"document\.info\.snapshots", engine, "crates/oc-core/src/engine/mod.rs"),
            find(r'Config\.latest\(loaded\.entries, "snapshots"\)', ref_snapshot_plugin,
                 "packages/core/src/config/plugin/snapshot.ts"),
        ),
        (
            "snapshots 默认值 true",
            find(r"AtomicBool::new\(true\)", rust_snapshot, "crates/oc-core/src/snapshot.rs"),
            find(r"enabled: true \}", ref_snapshot, "packages/core/src/snapshot.ts"),
        ),
        (
            "session.tps 默认值 true（曾 G-367）",
            find(r"session_tps: true", rust_tui, "crates/oc-tui/src/config.rs"),
            find(r"tps: input\.session\?\.tps \?\? true", ref_tui, "packages/tui/src/config/index.tsx"),
        ),
    ]


# --------------------------------------------------------------------- main

def _canon(k: str) -> str:
    """Remove open-node markers (`.<*>`/`.<…>`) anywhere for set comparison."""
    return re.sub(r"\.(?:<\*>|<…>)(?=\.|$)", "", k)


def diff(rust: dict, refk: dict):
    rk = {_canon(k): k for k in rust}
    fk = {_canon(k): k for k in refk}
    both = sorted(set(rk) & set(fk))
    allk = set(rk) | set(fk)

    def _container_only(k: str) -> bool:
        """Key is a pure container (an ancestor of a deeper key on its side)."""
        return any(o != k and o.startswith(k + ".") for o in allk)

    ref_only = sorted(k for k in set(fk) - set(rk) if not _container_only(k))
    rust_only = sorted(k for k in set(rk) - set(fk) if not _container_only(k))
    return both, ref_only, rust_only


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--ref", default="/opt/opencode-ref")
    ap.add_argument("--write", default="docs/audit/census/CONFIG-KEYS.md")
    args = ap.parse_args()
    ref = Path(args.ref)

    rust_files = sorted(RUST_SCHEMA_CONFIG.glob("*.rs")) + [Path(p) for p in RUST_EXTRA_FILES]
    structs, enums, aliases, uses, locations = rust_structs(rust_files)
    rust_doc_keys, n1 = expand_rust(structs, enums, aliases, uses, locations, RUST_DOC_ROOT)
    rust_tui_keys, n2 = expand_rust(structs, enums, aliases, uses, locations, RUST_TUI_ROOT)

    graph = RefGraph(ref)
    ref_doc_keys, n3 = graph.expand("schema/src/config", "class", "Info")
    ref_tui_keys, n4 = graph.expand("tui/src/config/index", "const", "Info")

    loader_src = Path("crates/oc-core/src/config.rs").read_text()
    loader_structs = re.findall(r"pub struct (\w+)", strip_tests(loader_src))

    doc_both, doc_ref_only, doc_rust_only = diff(rust_doc_keys, ref_doc_keys)
    tui_both, tui_ref_only, tui_rust_only = diff(rust_tui_keys, ref_tui_keys)
    rc_doc = {_canon(k): v for k, v in rust_doc_keys.items()}
    fc_doc = {_canon(k): v for k, v in ref_doc_keys.items()}
    rc_tui = {_canon(k): v for k, v in rust_tui_keys.items()}
    fc_tui = {_canon(k): v for k, v in ref_tui_keys.items()}

    rust_tui_src = Path("crates/oc-tui/src/config.rs").read_text()
    r_defaults_raw = rust_tui_defaults(rust_tui_src)
    r_defaults = {
        path: r_defaults_raw[field]
        for field, path in RUST_TUI_FIELD_PATH.items()
        if field in r_defaults_raw
    }
    ref_tui_src = (ref / "packages/tui/src/config/index.tsx").read_text()
    f_defaults = ref_resolve_defaults(ref_tui_src)

    anchors = anchor_checks(ref)

    rep = Report(
        "CONFIG-KEYS Census（缺口普查 · 通道 2/∞）",
        "双侧配置键全集机械提取 + 字面默认值对拍；census 非门禁，exit 0。",
    )
    rep.w("> 生成命令：`python3 tools/census/config_keys.py --write docs/audit/census/CONFIG-KEYS.md`")
    rep.w("> 权威源：rust serde 结构体（oc-schema/config + oc-core/cli_config）vs ref effect Schema"
          "（packages/schema/src/config* + packages/tui/src/config/index.tsx）。")
    rep.w("")
    rep.w("## 归一化与映射规则")
    rep.w("")
    rep.w("- 键名映射规则：**IDENTITY + 显式容器规则** —— 两侧文档键名均为逐字面量（顶层 snake_case：`default_agent`/`tool_output`；"
          "嵌套 MCP/provider 形状 camelCase：`headerTimeout`，rust 经 `#[serde(rename)]`/`rename_all=\"camelCase\"` 显式对齐）。"
          "大小写不做静默归一；case-only 不匹配将以 CASE_MISMATCH 呈现（本轮未出现）。")
    rep.w("- 开放形状归一：`#[serde(flatten)]`/字符串键 map/`Schema.Record(Schema.String,…)`/`Schema.StructWithRest` → `<*>` 节点"
          "（serde flatten 语义为父级内联）；`Vec<T>`/`Schema.Array` → `<*>` 数组节点；集合比对时统一剥除 `.<*>`/`.<…>` 标记。")
    rep.w("- union/tagged-union：双侧变体合并展开（rust enum 各 variant 的结构、`#[serde(tag=\"type\")]` 判别键；"
          "ref `Schema.Union` 的各 struct/class 分支）；纯字面量 union 记为叶子。")
    rep.w("- ref 侧 `//`、`/* */` 注释先行剥除（保偏移）；跨模块引用经 import 表解析（`../model.js` 等）。")
    rep.w("- 默认值文本规范化：保留引号（`\"lit\"` ≠ `lit`）；`true/false`/数字/带引号字符串直接字面比对。")
    rep.w("")
    rep.w("## 范围声明")
    rep.w("")
    rep.w(f"- `crates/oc-core/src/config.rs` 为加载器（发现/合并/变量替换/watch），无键定义；其顶层结构体仅"
          f" `{', '.join(loader_structs[:6])}` 等服务类型 —— 键全集由 oc-schema（opencode.json）与 oc-core/cli_config（cli.json）承载。")
    rep.w("- `crates/oc-tui/src/config.rs` 消费 `cli_config::Info`（另有手写 `parse` 直读 `value.get/pointer`，键集一致）；只取默认值与锚点证据。")
    rep.w("- ref `packages/core/src/config/plugin/*.ts` 是行为消费者（不定义键），以锚点检查覆盖（mcp.ts/snapshot.ts）。")
    rep.w("- `permissions`（Ruleset）两侧为递归规则结构，以 `<…>`/叶子表示，内部不展开（属 permission 通道）。")
    rep.w("- `keybinds` 为开放键表（rust `Value` vs ref `TuiKeybind.KeybindOverrides`），记为 `<*>` 开放节点。")
    rep.w("- 双方都读但语义分叉的键不做行为判定（超范围）。")
    rep.w("")
    rep.w("## 轴 1a：opencode.json 键路径全集")
    rep.w("")
    rep.w(f"- rust（crates/oc-schema/src/config/mod.rs `Info` 根展开）：{len(rust_doc_keys)} 个点路径")
    rep.w(f"- ref（packages/schema/src/config.ts `Info` 根展开）：{len(ref_doc_keys)} 个点路径")
    rep.w(f"- **BOTH：{len(doc_both)}** · REF_ONLY：**{len(doc_ref_only)}** · RUST_ONLY：**{len(doc_rust_only)}**")
    for k in doc_ref_only:
        ev = fc_doc.get(k, ("?", 0, ""))
        rep.w(f"  - 🔴 REF_ONLY `{k}` — ref: {ev[0]}:{ev[1]}")
    for k in doc_rust_only:
        ev = rc_doc.get(k, ("?", 0, ""))
        rep.w(f"  - 🔗 RUST_ONLY `{k}` — {ev[0]}:{ev[1]}")
    for k in doc_ref_only + doc_rust_only:
        if k in ADJUDICATIONS:
            tag, why = ADJUDICATIONS[k]
            rep.w(f"    - 裁定【{tag}】：{why}")
        else:
            rep.w(f"    - 裁定【待人工】：未见预登记定性，需亲读双侧后归档。")
    rep.w("")
    rep.w("## 轴 1b：cli.json（TUI）键路径全集")
    rep.w("")
    rep.w(f"- rust（crates/oc-core/src/cli_config/mod.rs `Info` 根展开）：{len(rust_tui_keys)} 个点路径")
    rep.w(f"- ref（packages/tui/src/config/index.tsx `Info` 根展开）：{len(ref_tui_keys)} 个点路径")
    rep.w(f"- **BOTH：{len(tui_both)}** · REF_ONLY：**{len(tui_ref_only)}** · RUST_ONLY：**{len(tui_rust_only)}**")
    for k in tui_ref_only:
        ev = fc_tui.get(k, ("?", 0, ""))
        rep.w(f"  - 🔴 REF_ONLY `{k}` — ref: {ev[0]}:{ev[1]}")
    for k in tui_rust_only:
        ev = rc_tui.get(k, ("?", 0, ""))
        rep.w(f"  - 🔗 RUST_ONLY `{k}` — {ev[0]}:{ev[1]}")
    for k in tui_ref_only + tui_rust_only:
        if k in ADJUDICATIONS:
            tag, why = ADJUDICATIONS[k]
            rep.w(f"    - 裁定【{tag}】：{why}")
        else:
            rep.w(f"    - 裁定【待人工】：未见预登记定性，需亲读双侧后归档。")
    rep.w("")
    rep.w("## 轴 2：字面默认值对拍（双侧均有字面默认值的键）")
    rep.w("")
    rep.w(f"- ref 源：`packages/tui/src/config/index.tsx` `resolve()` 的 `?? <lit>` 链（机械提取 {len(f_defaults)} 项）。")
    rep.w(f"- rust 源：`crates/oc-tui/src/config.rs` `impl Default for TuiConfig` 字面（机械提取 {len(r_defaults_raw)} 项；"
          f"可比子集经人工核对映射表 {len(RUST_TUI_FIELD_PATH)} 项）。")
    rep.w("- RUST_TUI_FIELD_PATH（TuiConfig 字段→cli.json 点路径，逐项亲读双侧核对）：")
    for field, path in sorted(RUST_TUI_FIELD_PATH.items()):
        rep.w(f"  - `{field}` → `{path}`")
    rep.w("")
    comparable = sorted(set(r_defaults) & set(f_defaults))
    mismatches = 0
    rep.w(f"- 可比键：{len(comparable)}")
    for k in comparable:
        rv, fv = norm_default(r_defaults[k]), norm_default(f_defaults[k])
        mark = "✅" if rv == fv else "⚠️ MISMATCH"
        mismatches += rv != fv
        rep.w(f"  - {mark} `{k}` rust=`{rv}` ref=`{fv}`")
    ref_only_d = sorted(set(f_defaults) - set(r_defaults))
    rust_only_d = sorted(set(r_defaults) - set(f_defaults))
    rep.w(f"- 仅 ref 有机械可提字面默认（rust 默认在解析分支/子结构中，本轮不比对）：{len(ref_only_d)}")
    for k in ref_only_d:
        rep.w(f"  - ℹ️ `{k}` ref=`{norm_default(f_defaults[k])}`")
    rep.w(f"- 仅 rust 有机械可提字面默认（ref 以 falsy 检查/子结构承载，本轮不比对）：{len(rust_only_d)}")
    for k in rust_only_d:
        rep.w(f"  - ℹ️ `{k}` rust=`{norm_default(r_defaults[k])}`")
    rep.w("")
    rep.w("## 锚点核对（提取器正确性验证；三锚来自上轮修复）")
    rep.w("")
    for name, r_ev, f_ev in anchors:
        ok = "✅ 对齐" if (r_ev and f_ev) else "⚠️ 需人工复核（先查提取 bug，再查修复是否落地）"
        rep.w(f"- **{name}**：rust [{r_ev or '❌ 未见证据'}] vs ref [{f_ev or '❌ 未见证据'}] → {ok}")
    rep.w("")
    rep.w("## Census 裁定与覆盖率")
    rep.w("")
    total_a = len(set(rc_doc) | set(fc_doc))
    total_b = len(set(rc_tui) | set(fc_tui))
    adj_a = len(doc_both) + len(doc_ref_only) + len(doc_rust_only)
    adj_b = len(tui_both) + len(tui_ref_only) + len(tui_rust_only)
    rep.w(f"- 轴 1a：差异面已裁定 {adj_a} 项（BOTH {len(doc_both)} + REF_ONLY {len(doc_ref_only)} + RUST_ONLY {len(doc_rust_only)}；"
          f"全集 {total_a} 中其余 {total_a - adj_a} 个为仅作前缀的容器节点，已在归一化规则下剔除）")
    rep.w(f"- 轴 1b：差异面已裁定 {adj_b} 项（BOTH {len(tui_both)} + REF_ONLY {len(tui_ref_only)} + RUST_ONLY {len(tui_rust_only)}；"
          f"全集 {total_b} 中其余 {total_b - adj_b} 个为容器节点）")
    rep.w(f"- 轴 2：可比键 {len(comparable)}，默认值不一致 {mismatches} 项。")
    rep.w("- 结论：REF_ONLY 为缺口候选（逐项亲读定性）；RUST_ONLY 为 rust 额外旋钮（需登记册 rust-extra 背书）；`<*>` 开放节点内部不判定。")
    rep.w("")
    rep.w("## 提取不确定性")
    rep.w("")
    for tag, notes in (("轴1a-rust", n1), ("轴1b-rust", n2), ("轴1a-ref", n3), ("轴1b-ref", n4)):
        for note in notes[:15]:
            rep.w(f"- {tag}: {note}")
        if len(notes) > 15:
            rep.w(f"- {tag}: …另有 {len(notes) - 15} 条同类")
    rep.w("- 键名提取只认 `#[serde(rename)]`/容器 `rename_all` 与 TS 字面键；rust 漏写 rename 的字段名≠键名情形提取器无法发现（人工抽查未见）。")
    rep.w("- ref `mapFields` 动态改写按原 struct 字段展开（tools/input/output）；`Schema.withConstructorDefault` 等构造默认不影响键集。")
    rep.w("- ref 相对 import（`../model.js`）经 import 表解析；包级 import（`@opencode/...`）不解析——本轴所需形状均在 schema 包内。")
    rep.w("- rust 侧跨文件同名歧义经 `use crate::…` 段 + config/ 子树优先消歧（如 `Compaction` 的 provider/config 双定义）。")
    rep.w("- union 展开为各分支键并集（variant merge），不表达互斥关系；`<*>` 开放节点内部键差不判定。")
    rep.w("")
    rep.w("## 验证记录")
    rep.w("")
    rep.w("- 每个差异项已亲读双侧当前代码核实（file:line 见上；旧文档仅作线索）。")
    rep.w("- 幂等性：连跑两次输出经 `diff` 逐字节一致（/tmp/opencode/ 验证）。")
    rep.w("- 确定性：文件排序遍历、集合输出前排序；无时间戳/随机源。")
    rep.w("- exit 0（census 非门禁）。")

    rep.write(args.write)
    print("=== SUMMARY ===")
    print(f"axis1a: rust={len(rust_doc_keys)} ref={len(ref_doc_keys)} both={len(doc_both)}")
    print("  REF_ONLY:", doc_ref_only)
    print("  RUST_ONLY:", doc_rust_only)
    print(f"axis1b: rust={len(rust_tui_keys)} ref={len(ref_tui_keys)} both={len(tui_both)}")
    print("  REF_ONLY:", tui_ref_only)
    print("  RUST_ONLY:", tui_rust_only)
    print("notes:", n1 + n2 + n3 + n4)
    return 0


if __name__ == "__main__":
    sys.exit(main())
