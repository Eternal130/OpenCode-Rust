# E12 live-ref recheck — session-list search semantics (F1–F6)

**Date:** 2026-10-23 ｜ **Lane:** `lane/e12` (Task 4, SPEC-R6 last E12 dispositions)
**Ref:** live v2.0.24 TUI @ `e7a34f09bf` (bun 1.4.2, sandbox copy of `/opt/opencode-ref`,
never mutated) ｜ **Ours:** `opencode-rust` debug build (oc-cli/oc-server, worktree `lane/e12` @ main `48ba71a`)

## Method (one-shot semantic confirmation, NOT a differential loop)

* Both TUIs booted **only inside a PTY owned by the driver**
  (`e12_pty_driver.py`, python `pty.fork` + `pyte` screen model, 120×40,
  TERM=xterm-256color). The driver answers the TUI's terminal-capability
  probes (DA1 `CSI c`, kitty `CSI ? u`, CPR `CSI 6 n`) the way a real
  terminal does — without the replies the ref TUI's key pipeline stays
  idle and every keystroke falls into the prompt editor.
* Both stacks fully isolated: own `HOME`/`XDG_*` roots, own project dir
  (`opencode.json` + git-init + solid-js `tsconfig.json` seeded, per the
  retired differential rig's invariant), own server on `127.0.0.1`
  (ref :41234 / ours :41256), `OPENCODE_DISABLE_AUTOUPDATE=1`,
  `OPENCODE_CHANNEL=stable`, `LANG=C.UTF-8`.
* Seeding via each stack's **own** HTTP API (`POST /api/session`, exact
  corpus title bytes — codepoint audit in `e12_seed.py` output below;
  `ses_child` created with `parentID=ses_alpha`). 9 sessions, incl. one
  child, NFC `café`/`naïve`, CJK, and space-padded/doubled-space title.
* Drive: boot → settle → `esc` (first-run modal) → `ctrl+x` `l`
  (`session.list` = `<leader>l`, leader default `ctrl+x`) → type query →
  settle (>150 ms debounce + round trip) → capture rendered rows.
  **One fresh TUI boot per query** (13 boots per side) — a same-boot
  loop was observed to poison later queries with the previous query's
  empty result (ref `searchResults.latest ?? []` while loading), so
  per-boot isolation is the recorded protocol.
* Corpus: `tools/parity/spec-canaries/e12_search_corpus.json` (12 queries).
  Raw snapshots: `ref/` and `our/` (`-dialog-baseline.txt` + `q00..q11.txt`
  + `-corpus.json`). Server-side probes: `server-api-probes.txt`.
* `our/our-corpus-part2-raw.json` = raw capture of the our-side re-run
  (dialog baseline + queries 9–12), kept for provenance of those four rows.
* Per-query numbering note: `q00..q11` on both sides = corpus queries 1..12
  in order (`qNN` ↔ corpus entry `NN+1`); an earlier our-side misfiling of
  queries 9–12 as `q00..q03` was fixed by re-extracting from `our-corpus.json`
  — no query/offset mismatch remains.

## Seeded sessions (title codepoints verified at create time)

| id | title | note |
|---|---|---|
| ses_alpha | `Fix login redirect loop` | parent |
| ses_child | `Fix login redirect loop` | **child of ses_alpha** (same title — leak = 2 rows) |
| ses_beta | `refactor: database migrations` | |
| ses_gamma | `CJK 支付宝接入` | |
| ses_delta | `café menu parser` | NFC (U+00E9) |
| ses_epsilon | `CAFE overhaul` | ASCII lookalike |
| ses_zeta | `Zalando API client` | |
| ses_eta | `·Wide··spaces··title·` | leading/trailing + doubled internal spaces |
| ses_theta | `naïve UTF-8 benches` | NFC (U+00EF) |

## Result table — LIVE ref vs ours (rendered session-list rows)

| # | query (class) | live ref v2.0.24 | ours | verdict |
|---|---|---|---|---|
| — | *(empty dialog baseline)* | 8 rows = all parents, **child absent** | 8 rows, child absent | **parity** |
| 1 | `login` (ascii) | `ses_alpha` ×1 (**no child row**) | `ses_alpha` | **F1: parity — no child leak on either side** |
| 2 | `· FIX Login ·` (case+ws) | `ses_alpha` | `ses_alpha` | parity |
| 3 | `MIGRATIONS` (case-variant) | `ses_beta` | `ses_beta` | parity (ASCII case-insensitive both) |
| 4 | `café` NFC (U+00E9) | `ses_delta` | `ses_delta` | **F2: parity — é≠e, no cross-matching** |
| 5 | `cafe`+U+0301 NFD | none → “No sessions found” | none → “No sessions found” | **F3: parity — NFD query ≠ NFC title, no normalization on either side** |
| 6 | `cafe` (ascii prefix) | `ses_epsilon` (CAFE overhaul) **only** | `ses_epsilon` only | **F2: parity — `cafe` does NOT match `café…`; byte-exact contains** |
| 7 | `支付宝` (CJK) | `ses_gamma` | `ses_gamma` | parity |
| 8 | `支付` (CJK substring) | `ses_gamma` | `ses_gamma` | parity |
| 9 | `loginredirect` (fuzzy-concat) | none | none | **F4: parity — ref is NOT fuzzy; substring only. Ours pins substring ⇒ decision: substring, ours already correct** |
| 10 | `zln` (fuzzy-abbrev) | none | none | **F4: parity** |
| 11 | `wide spaces` (ws-collapse) | none → “No sessions found” | none | **F5: parity — no whitespace collapsing; byte-exact** |
| 12 | `··` (whitespace-only) | **full 8-parent list** | full 8-parent list | **F6: parity — trimmed-empty query = full set (children still excluded in rendered rows)** |

**12/12 queries parity; zero divergences.** Server-side `GET /api/session?search=…`
probes (`server-api-probes.txt`) agree on every probe, both stacks.

## Findings dispositions (what the live run settles)

* **F1 (child leak):** NOT a live-ref divergence — the ref never renders
  child rows (server path pins `parentID: null`, `options()` re-filters
  `!parentID`). Our first-pass F1 was an artifact of testing the raw
  predicate in isolation rather than the mounted dialog. Ours (options
  built from `parent_id.is_none()` — `dialog_open.rs:504-511`) = parity.
* **F2 (accent equivalence):** ref has NO accent folding: `cafe`↔`café`
  never cross-match; `café`(NFC query) matches the NFC title only.
  Ours pins the same byte-exact semantics ⇒ **semantics approved as
  pinned; no change**.
* **F3 (NFD):** neither side normalizes; NFD query matches nothing
  against NFC titles. **Approved as pinned.**
* **F4 (fuzzy vs substring):** live ref session list is **substring,
  never fuzzy** (`zln`, `loginredirect` → no rows). Our port already
  mounts `.substring_filter()` for `session.list`
  (`command_palette.rs:704-721`, `dialog_select.rs:273-286`). **Decision
  closed: substring; no code change.**
* **F5 (whitespace collapse):** no collapsing on either side; doubled
  spaces defeat a single-space query. **Approved as pinned.**
* **F6 (whitespace-only query):** trimmed-empty = the **full list** on
  both sides (NOT an empty result). Note vs the corpus cell: the
  *predicate* keeps children for an empty needle, but the *rendered
  dialog* never shows them (`options()` filters `!parentID`) — the
  rendered surface is what E12 clauses over, and both sides agree.
  Our `filter_indices` returns the full option set for a
  whitespace-only needle — parity.

## Source-vs-live nuance recorded (no action)

v2.0.24's session list is two-path: an immediate local filter
(`filter().trim().toLowerCase().includes` — the predicate our port pinned)
and, after a 150 ms debounce, a server search
(`GET /api/session?search=…&parentID=null&limit=50&order=desc` → SQLite
`LIKE %q%`, ASCII-case-insensitive, byte-exact otherwise —
`store.ts:109`). For every corpus query the two paths agree on the match
set, and the live settled rows agree with our pinned trim+case-fold+
byte-exact contains. Case-folding proven live: `MIGRATIONS`→beta,
`cafe`→epsilon on both sides.

## Reproduction

```bash
# ref sandbox (bun 1.4.2): copy /opt/opencode-ref → /tmp/opencode/e12ref-$$,
# bun install --frozen-lockfile, serve on 127.0.0.1 (isolated XDG home),
# seed via e12_seed.py, then:
python3 docs/e12-live-ref-recheck/e12_pty_driver.py perquery ref \
  --cwd <project-dir> --argv-json <bun argv> --env-json <isolated env> \
  --out <out-dir>
# ours: opencode-rust serve + seed, same driver, argv = [opencode-rust, --server, URL]
```
