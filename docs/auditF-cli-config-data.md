# Lane audit-F — CLI + CONFIG + DATA layers: v2 reference vs the Rust port

**Scope**: exhaustive inventory of (1) the CLI command/flag surface (`/opt/opencode-ref/packages/cli` vs `crates/oc-cli`), (2) the config schemas (`packages/schema/src/config*`, `packages/cli/src/config`, `packages/tui/src/config` vs `crates/oc-schema/src/config`, `crates/oc-core/src/config`, `crates/oc-tui/src/config.rs`), and (3) the persisted data layer (drizzle SQL schema, state files, logs, update cache, v2→rust migration). Read-only audit; nothing but this file was modified.

**Method**: structural walk of both trees, flag-by-flag and key-by-key diff, with handler-level verification of output formats and exit codes for every command. File:line refs are to the reference unless prefixed `rust:`.

**Headline counts**

| Layer | Ref inventory | Rust parity | Missing | Divergent semantics |
|---|---|---|---|---|
| CLI commands | 49 surfaces (root TUI + 19 top-level + 29 subcommands) | 49/49 | **0** | — |
| CLI flags/args | ~85 flags/args (incl. 6 global) | all present | **0** | **10** (2 high-impact) |
| Core `opencode.json` | 28 root keys + ~90 nested fields | 28/28 root, nested ~1:1 (+5 rust-extra fields) | **0** | 0 material |
| TUI `cli.json` | 20 sections / ~70 leaf keys | 7 sections read (2 partially) | **13 sections, ~40 leaf keys silently ignored** | sidebar persistence location |
| Data layer | 19 SQL tables, 48 migrations, ~12 state/log/cache artifacts | 19/19 tables, 48/48 migrations | **3 state files** (prompt history family) | 2 divergent (pinned, logs); state-dir migration gap |

---

## 1. CLI layer

### 1.1 Command inventory (full parity on names/aliases)

Reference tree from `packages/cli/src/commands/commands.ts`; rust tree from `crates/oc-cli/src/cli.rs` (its own header claims 1:1 and holds). Dispatch map `packages/cli/src/index.ts:25-79` ↔ `rust:crates/oc-cli/src/main.rs:56-194`.

| Command (ref file:line) | Flags/args | Rust | Verdict |
|---|---|---|---|
| root TUI (`commands.ts:38-58`) | `directory`, `-c/--continue`, `-s/--session`, `--prompt` + globals `--print-logs --standalone --server --auto --yolo --dangerously-skip-permissions` | `cli.rs:27-64` | **DIVERGENT** — `--prompt` and `--auto`/`--yolo`/`--dangerously-skip-permissions` parsed but **never forwarded** (see 1.2 #1/#2) |
| `upgrade`/`update` (`commands.ts:60-74`) | `target`, `-m/--method` | `cli.rs:89-97` → `misc.rs` `upgrade` | PORTED (method set reduced to curl — see 1.2 #10) |
| `uninstall` (`commands.ts:75-98`) | `-c --keep-config -d --keep-data --dry-run -f --force` | `cli.rs:98-112` → `misc.rs:75` | PORTED — removal plan identical (`updater.rs:617-625` = `uninstall.ts:24-27`) |
| `acp` (`commands.ts:99`) | — | `cli.rs:114` → `misc.rs` `acp` | PORTED |
| `api` (`commands.ts:100-116`) | `request` (1..2), `-d/--data`, `-H/--header` (≤100), `--param k=v` | `cli.rs:116-129` | PORTED, minor superset (multi `--param`, no 100-header cap) |
| `debug agents/config/paths` (`commands.ts:117-143`) | `paths <name>` enum of 10 | `cli.rs:285-296` | PORTED; `name` unvalidated (free string) |
| `auth list/login/logout/export/import/switch` (`commands.ts:144-221`) | `--format default\|json`, `target`, `--method`, `--answer`, `credential`, `file` | `cli.rs:298-341` → `handlers/auth.rs:147-158` | PORTED; `--format` value unvalidated |
| `mcp list/add/auth/logout` (`commands.ts:222-260`) | `name`, `command…`, `--url --header --env --global` | `cli.rs:343-380` → `handlers/mcp.rs` | PORTED + rust-only `--force` (JSONC rewrite, documented) |
| `plugin list/add/check/update/remove` (`commands.ts:261-304`) | `--builtin`, `package`, `target` | `cli.rs:382-416` → `handlers/plugin.rs` | PORTED + rust-only `--force` on add/remove |
| `models` (`commands.ts:305-308`) | — | `cli.rs:150-151` → `misc.rs:174-205` | PORTED (locale-aware vs byte sort — cosmetic) |
| `stats` (`commands.ts:309-339`) | `--days --year --all --project --models --tools --cost --full --limit --json` (range-checked) | `cli.rs:152-184` | PORTED; range validation dropped (`--limit 0`, `--year 99999` accepted) |
| `mini` (`commands.ts:340-375`) | `-c -s --fork --replay/--no-replay --replay-limit -m --agent --prompt --demo(hidden)` | `cli.rs:185-217` | PORTED incl. hidden `--demo`; **config fallback for replay missing** (1.2 #5) |
| `run` (`commands.ts:376-417`) | `message…`, `-c -s --fork -m --agent --format default\|json -f/--file --title --thinking --auto --yolo --dangerously-skip-permissions` | `cli.rs:218-250` → `run/` | PORTED; `--format` unvalidated; exit codes 1/130 ported (`noninteractive.rs:514-517,590-597`) |
| `session list/delete/export/import` (`commands.ts:418-470`) | `-n/--max-count`, `--format table\|json`, `sessionID`, `--sanitize`, `file`, `--directory` | `cli.rs:418-450` → `handlers/session.rs` | PORTED; no `less` pager, UTC timestamps (1.2 #8) |
| `service start/restart/status/stop/get/set/unset` (`commands.ts:471-512`) | `key`, `value`, `env-value`, `name` | `cli.rs:452-486` → `service_cmds.rs` | PORTED |
| `reload` (`commands.ts:513-518`) | — | `cli.rs:261-262` | PORTED |
| `pair` (`commands.ts:519-536`) | `--url` (validated http/https) | `cli.rs:263-268` | PORTED; QR block deferred (registered), `--url` validation lighter |
| `serve` (`commands.ts:537-550`) | `--hostname --port --cors --service --stdio` | `cli.rs:269-282` | PORTED; `--hostname` IP-only (1.2 #7) |

Hidden entrypoints: SSH askpass via `OPENCODE_SSH_ASKPASS_PORT` (`index.ts:20-23` ↔ `rust:main.rs:20-22`) — PORTED.

### 1.2 Divergent flag semantics / output / exit codes

1. **Root `--prompt` is silently dropped (HIGH)**. Ref passes `args.prompt` into the TUI (`handlers/default.ts:115`); rust `handlers/misc.rs:15-19` only receives `directory/continue/session` and `BootArgs` has no prompt field (`rust:oc-tui/src/app.rs:2088-2096`). `opencode --prompt "fix the build"` starts an idle TUI — the user's prompt vanishes.
2. **Root permission flags not forwarded to the TUI (HIGH)**. Ref: `args.auto = input.auto || input.yolo || input.dangerouslySkipPermissions` (`handlers/default.ts:116`). Rust computes `effective_auto()` (`rust:cli.rs:80-85`) but `dispatch` only uses it for `run` (`rust:main.rs:171`); the root TUI path ignores it. `opencode --auto` boots a TUI that still prompts for every permission.
3. **Global-flag scoping is a superset (LOW)**. Ref scopes `--standalone/--server` to specific commands (`ServerParams` spreads) and permission flags to root+run; rust declares all six as clap globals (`rust:cli.rs:33-45,67-78`), so they're accepted (and ignored) on commands where v2 errors.
4. **`--format` enum validation dropped (LOW-MED)**. Ref `Flag.choice` for `run --format` (`commands.ts:404`), `session list --format` (`commands.ts:431`), `auth list --format` (`commands.ts:151`); rust takes free strings (`rust:cli.rs:239,303,426`) and silently falls back to the default rendering for unknown values (e.g. `--format yaml` prints the human format instead of erroring).
5. **`mini` ignores the `mini.replay`/`mini.replay_limit` config fallback (MED)**. Ref: `input.replay ?? resolved.mini?.replay ?? true` and `input.replayLimit ?? resolved.mini?.replay_limit` (`handlers/mini.ts:37-38`). Rust cites the formula in a comment (`rust:main.rs:129-132`) but `MiniInput` is built only from flags — the `cli.json` `mini` section is never consulted.
6. **`debug paths <name>` accepts any string (LOW)**. Ref `Argument.choice` of 10 names (`commands.ts:125-140`); rust `Option<String>` (`rust:cli.rs:292-295`) — typos silently produce empty/whole output instead of a usage error.
7. **`serve --hostname` only parses IP literals (MED)**. Rust `parse_hostname = IpAddr::from_str` (`rust:oc-server/src/boot.rs:249-252`) and silently falls back to 127.0.0.1 on failure (`rust:handlers/serve.rs:66-72`), so `--hostname localhost` (valid for v2's listener, `server/src/process.ts:139-148`) is ignored without a word. Same silent fallback applies to `service.json`'s `hostname`.
8. **`session list` output deltas (LOW-MED)**. (a) Ref pipes through `less -R -S` when stdout is a TTY and `--max-count` absent (`session/list.ts:63-96`); rust always prints. (b) Ref formats `time.updated` with `toLocaleString()` (local tz, `session/list.ts:99-107`); rust prints fixed UTC `YYYY-MM-DD HH:MM:SS` (`rust:handlers/session.rs:83-95`). (c) Ref exit code 1 on error matches; interrupt-exit 130 in `session export` (`session/export.ts:66`) has no rust counterpart.
9. **`stats` constraint checks dropped (LOW)**. `days ≥ 0`, `1970 ≤ year ≤ 9999`, `limit ≥ 1` enforced by schema in ref (`commands.ts:313-336`); rust `u32`s accept `--limit 0` etc. Also the GitHub-style painted calendar is a registered simplification (`rust:handlers/stats.rs:285`).
10. **`upgrade --method` reduced (LOW-MED)**. Ref methods `curl npm pnpm bun yarn vp brew` (`services/updater.ts:14`); rust implements curl-only, installing into `~/.opencode/bin/opencode-rust` (`rust:services/updater.rs:70-84`) — npm/brew users get a divergent install path. Plus rust-only extras: `--version-rust`, `mcp add --force`, `plugin add/remove --force` (all documented as rust additions), and the binary name is `opencode-rust` vs ref's `OPENCODE_CLI_NAME ?? "opencode"` (`commands.ts:38`).

Exit-code parity elsewhere verified: `run` 1/130, `api` 1, `auth *` 1, `plugin check/update` 1-on-error, `session delete` 1, `acp` failure→1 — all mirrored in the rust handlers.

---

## 2. Config layer

### 2.1 Core `opencode.json`/`opencode.jsonc` schema — full parity

Ref root schema `packages/schema/src/config.ts:25-110` vs `rust:crates/oc-schema/src/config/mod.rs:54-137`: all 28 root keys present and identically named (`$schema, shell, model, default_agent, update, share, enterprise, username, permissions, agents, snapshots, watcher, formatter, lsp, media, tool_output, mcp, compaction, skills, commands, instructions, references, websearch, plugins, worktree, warming, providers, experimental`).

Nested schema field diff (ref `packages/schema/src/config/*.ts` vs `rust:crates/oc-schema/src/config/*.rs`):

| Section | Ref fields | Rust | Verdict |
|---|---|---|---|
| agent | model request system description mode hidden color steps disabled permissions | identical | PORTED |
| command | template description agent model subagent subtask | identical | PORTED |
| compaction | tokens auto keep buffer | identical | PORTED |
| experimental | portable_shell_scanner subagent_depth policies | identical | PORTED |
| formatter | disabled command environment extensions | identical | PORTED |
| lsp | disabled command extensions env initialization | identical | PORTED |
| mcp | timeout servers | identical | PORTED |
| media | auto_resize max_width max_height image | + `max_base64_bytes` | PORTED (rust superset) |
| model | providerID model variant | `provider_id` (renamed) | PORTED |
| plugin | string \| {package options} | identical (`Plugin::Spec/Entry`) | PORTED |
| policy | action resource effect | identical | PORTED |
| provider | settings/headers/body per method, tiers, variants, cost, disabled, limit, canonical, env, package, models | + `timeout header_timeout chunk_timeout transport compaction` | PORTED (rust superset, 5 extra fields) |
| reference | repository branch description hidden / path … | identical | PORTED |
| tool-output | max_lines max_bytes | identical | PORTED |
| warming | prompt interval duration | identical | PORTED |
| watcher | ignore | identical | PORTED |
| websearch | provider | identical | PORTED |
| worktree | directory | identical | PORTED |

Merge/normalize: rust `crates/oc-core/src/config/normalize.rs` (3,442 lines incl. v1 key migration) ports ref `normalize.ts` (808 lines) — verified legacy aliases all present: `snapshot`→`snapshots` (`normalize.rs:247` = `normalize.ts:70-88`), `experimental.mcp_timeout`→`mcp.timeout.{catalog,execution}` (`normalize.rs:812-816` = `normalize.ts:251-261`), `compaction.preserve_recent_tokens`→`keep`, unsupported `tail_turns`/`prune` (`normalize.rs:950-951` = `normalize.ts:332-333`). Discovery walk-up + global roots (`~/.config/opencode`, `~/.claude`, `~/.agents`) ported in `rust:oc-core/src/config/discovery.rs`; `OPENCODE_CONFIG_DIR`/XDG env honored (`rust:oc-util/src/global_roots.rs:98-99`). **No ref key is silently ignored at this layer.**

### 2.2 TUI/CLI `cli.json` — the big gap (~40 keys silently ignored)

Ref schema = TUI `Config.Info` (`packages/tui/src/config/index.tsx:69-249`) re-exported as the CLI config schema (`packages/cli/src/config/schema.ts`), loaded from `<config>/cli.json` with flock, JSONC-comment-preserving updates (`packages/cli/src/config/config.ts:34-105`), a v1→v2 migration (`config/migrate.ts`: `terminal.copy_on_select`→`terminal.copy`, legacy keybind targets), and an `OPENCODE_CLI_CONFIG_CONTENT` env overlay (`config.ts:34-36, 120`).

Rust reads `cli.json` in `rust:crates/oc-tui/src/config.rs:161-211` — a **subset**:

| cli.json section (ref keys) | Read by rust? | Notes |
|---|---|---|
| `theme{name,mode}` | ✅ `config.rs:180` | |
| `keybinds` | ✅ `config.rs:192` | full v2 override table incl. `terminal.suspend` special-case |
| `plugins` | ✅ `plugin_boot.rs:147` | |
| `leader{timeout}` | ✅ `config.rs:181` | |
| `attention{notifications,sound,volume,sound_pack,sounds}` | ✅ `config.rs:44-70` | v2 defaults replicated |
| `session{image_preview,sidebar}` | ✅ partial (2 of 10) | ignored: `thinking scrollbar grouping verbosity tps markdown new_location permissions` |
| `tabs{mode,enabled,scope}` | ✅ partial (3 of 5) | ignored: `layout indicators` (vertical tabs unmounted — see audit-B) |
| `mouse` | ❌ | **rust always enables mouse capture** (`rust:oc-tui/src/lib.rs` `EnableMouseCapture`); `mouse:false` users lose native terminal selection |
| `cursor{style,blinking}` | ❌ | |
| `scroll{speed,acceleration}` | ❌ | |
| `animations` | ❌ | |
| `diffs{source,wrap,tree,single,view}` | ❌ | diff viewer defaults hardcoded |
| `terminal{title,copy}` | ❌ | copy-on-select etc. (also flagged by audit-B) |
| `prompt{editor,paste,image_preview}` | ❌ | |
| `mini{thinking,tools,shell_output,turn_summary,footer,splash,work_spinner,mono,replay,replay_limit}` | ❌ (11 keys) | incl. the `replay`/`replay_limit` the `mini` command should fall back to (§1.2 #5) |
| `debug{devtools,timing,turn_tokens}` | ❌ | |
| `experimental` (bool record) | ❌ | |
| `$schema` | ❌ (never written/validated) | |

Missing mechanisms:
- **No write path**: ref `Config.update` persists settings toggles back to `cli.json` with flock + JSONC preservation (`config.ts:75-105`); rust never writes the file (only mtime-based `reload`, `config.rs:206-210`) — the session-sidebar toggle is diverted to the channel-storage `sidebar` key instead (`rust:oc-tui/src/storage.rs:163-176`), so a settings change made in the rust TUI is invisible to v2 and vice versa.
- **No v1 cli.json migration** (ref `migrate.ts`) and **no `OPENCODE_CLI_CONFIG_CONTENT`** support (used by the desktop/mini hosts to inject config).

### 2.3 Defaults divergences found

- `mini.replay` default true both sides (flag level) — but config-level fallback missing (above).
- Everything else spot-checked matches (attention defaults 0.4/`opencode.default`, tabs auto+cwd+HERDR_ENV gate, sidebar auto — `config.rs` tests mirror `config/index.tsx:298-333`).

---

## 3. Data layer

### 3.1 Database — full parity by construction

- **19/19 tables identical**: `account, account_state, control_account, credential, event, event_sequence, instruction_blob, instruction_entry, instruction_state, kv, permission, project, project_directory, session, session_inbox, session_message, session_pending, workspace, worktree` — rust `BOOTSTRAP_SQL` is a verbatim transliteration of ref `schema.gen.ts` (`rust:oc-core/src/database/bootstrap.rs:9-13`, diff of table lists is empty).
- **48/48 migrations** in `migration.gen.ts` order incl. `20260805200742_import_legacy_credentials` (reads `data/auth.json`) — `rust:oc-core/src/database/migrations.rs:110,470,642`. Fresh DBs take the one-transaction bootstrap snapshot, matching ref `migration.ts:22-52`.
- **DB filename scheme parity**: channel-suffixed `opencode[-<channel>].db`, `OPENCODE_DB` (incl. `:memory:`), `OPENCODE_DISABLE_CHANNEL_DB` — `packages/cli/src/database-path.ts` ↔ `rust:oc-core/src/database/path.rs:48-88` (rust improvement: empty `OPENCODE_DB` ignored).

### 3.2 State files / logs / cache

| Artifact (ref) | Ref location | Rust | Verdict |
|---|---|---|---|
| Service registration | `<state>/service[-<channel>].json` (`cli/src/services/service-config.ts:30-43,97-99`) | same names + sha1 legacy fallback (`rust:services/service_config.rs:34-95,118-120`) | PORTED |
| ChannelStorage tabs | `<state>/<channel>/tui/tabs.json` (`tui/src/context/storage.tsx:47-57`, `session-tabs.tsx:72`) | `<state>/<channel>/tui/tabs.json` (`rust:storage.rs:8,251`) | PORTED |
| ChannelStorage session-list | same dir (`dialog-session-list.tsx:42`) | same (`rust:storage.rs:164`) | PORTED |
| ChannelStorage `view` (sidebar MCP open state) | same dir (`feature-plugins/sidebar/mcp.tsx:6`) | **absent** — rust writes its own `sidebar` key with different semantics (session-sidebar mode) (`rust:storage.rs:163-176`) | **DIVERGENT** |
| ChannelStorage `getting-started` (onboarding dismissal) | same dir (`feature-plugins/sidebar/footer.tsx:11`) | in store, but **not in `MIGRATED_KEYS`** (`rust:storage.rs:189`) | PARTIAL |
| Pinned sessions | `<state>/session.json` `{pinned:[…]}` (`tui/src/context/local.tsx:504-523`) | stored inside `tabs.json` instead (`rust:store.rs:345,2392-2395`) | **DIVERGENT** — v2's file ignored, rust's shape invisible to v2 |
| Model preference | `<state>/model.json` (`context/local.tsx:154`) | `<state>/model.json` (`rust:store.rs:2767,2868`) | PORTED |
| Prompt history | `<state>/prompt-history.jsonl` (`prompt/history.tsx:93`) | **absent** (no persistence; grep across crates empty) | **MISSING** |
| Prompt frecency | `<state>/frecency.jsonl` (`prompt/frecency.tsx:42`) | **absent** | **MISSING** |
| Prompt stash | `<state>/prompt-stash.jsonl` (`prompt/stash.tsx:40`) | **absent** (drafts in-memory only, `rust:store.rs:623-628`) | **MISSING** |
| Legacy credentials | `<data>/auth.json` (imported by migration `20260805200742`) | copied by v2 migration (`rust:v2_migration.rs:331-348`) | PORTED |
| Logs | `<data>/log/opencode.log` / `opencode-<channel>.log` with periodic trim (`util/src/observability/logging.ts:59-61`) | `data/log/server.log` (server, `rust:oc-server/src/errors.rs:607-616`) + `<state>/tui.log` (TUI stderr, `rust:oc-tui/src/stderr_redirect.rs:131`) | **DIVERGENT** — filenames and dirs differ; v2 docs/scripts pointing at `opencode.log` find nothing; no trim |
| Update cache | `<cache>/bin`, `<cache>/npm` staging (`services/updater.ts:316-323,401-408`) | copied by migration (`rust:v2_migration.rs:143-144`); runtime updates use `~/.opencode/bin` instead | PARTIAL/DIVERGENT |
| Rust-only | — | `<state>/update-notice.log` (`rust:autoupdate.rs:46`) | rust-extra |

### 3.3 v2→rust migration gap (state root not copied)

`rust:oc-core/src/v2_migration.rs:110-150` copies: databases (`data/*.db` via sqlite backup + wal/shm), `auth.json`, flat config files (incl. `cli.json` — good), and `cache/npm` + `cache/bin`. It does **not** touch the state root, so a v2 user switching to the rust port silently loses: open tabs, session-list prefs, `getting-started` dismissal, pinned sessions (`session.json`), model preference (`model.json`), and the service registration (`service.json` → rust re-registers, but custom hostname/port/env/password settings are dropped). Given the port otherwise targets drop-in compatibility (same DB file scheme), this is the largest single migration hole.

---

## 4. Top-10 worst findings (severity-ranked by user impact)

1. **Root `--prompt` silently dropped** — `opencode --prompt "…"` starts an idle TUI; ref sends it as the first message. `rust:main.rs:60-67`, `rust:oc-tui/src/app.rs:2088-2096` vs `handlers/default.ts:115`.
2. **Root `--auto`/`--yolo`/`--dangerously-skip-permissions` not forwarded to the TUI** — permission auto-approve works only for `run`. `rust:main.rs:56-67` vs `handlers/default.ts:116`.
3. **~40 `cli.json` keys silently ignored** (13 of 20 sections unread: `mouse, cursor, scroll, animations, diffs, terminal, prompt, mini, debug, experimental` + 8 `session.*` + `tabs.{layout,indicators}`) — worst concrete case: `mouse:false` is overridden because rust unconditionally captures the mouse. `rust:oc-tui/src/config.rs:161-211` vs `tui/src/config/index.tsx:69-249`.
4. **v2→rust migration skips the state root** — tabs, pinned sessions, model preference, service settings all lost on first boot. `rust:v2_migration.rs:137-146`.
5. **`mini` ignores `mini.replay`/`mini.replay_limit` config fallback** — documented formula not implemented; only CLI flags work. `rust:main.rs:110-136` vs `handlers/mini.ts:37-38`.
6. **`cli.json` is never written** — no `Config.update` port (flock + JSONC-preserving writes), no v1 config migration, no `OPENCODE_CLI_CONFIG_CONTENT`; the sidebar toggle is diverted to a rust-private storage key that v2 can't see. `rust:oc-tui/src/config.rs:206-210`, `rust:storage.rs:163-176` vs `cli/src/config/config.ts:75-105`, `migrate.ts`.
7. **Prompt history / frecency / stash never persisted** — `<state>/prompt-history.jsonl`, `frecency.jsonl`, `prompt-stash.jsonl` have no rust counterpart; input history and stashed drafts evaporate on exit. `tui/src/prompt/{history,frecency,stash}.tsx` vs (absent).
8. **Pinned sessions stored in the wrong file** — rust keeps `pinned` inside `tabs.json`; v2's `<state>/session.json` is ignored and rust's pins are invisible to v2. `rust:store.rs:345` vs `tui/src/context/local.tsx:504-523`.
9. **`serve --hostname localhost` (and any hostname) silently falls back to 127.0.0.1** — IP-literal-only parser, no error; same for `service.json` hostname. `rust:oc-server/src/boot.rs:249-252`, `rust:handlers/serve.rs:66-72` vs `server/src/process.ts:139-148`.
10. **Log layout divergence** — v2 writes/rotates `data/log/opencode[-<channel>].log`; rust writes `data/log/server.log` and `<state>/tui.log` with no trim. Debugging guides and log collectors aimed at v2 paths come up empty. `rust:oc-server/src/errors.rs:607-616`, `rust:oc-tui/src/stderr_redirect.rs:131` vs `util/src/observability/logging.ts:59-77`.

Honorable mentions: `--format`/`debug paths`/`stats` enum validation dropped (bad values silently degrade instead of erroring), `session list` UTC timestamps + no `less` pager, `upgrade --method` reduced to curl with a different install prefix, binary name `opencode-rust` vs `opencode` in help output, `pair` without QR codes, api `--param`/global-flag surfaces accepted where v2 would reject.
