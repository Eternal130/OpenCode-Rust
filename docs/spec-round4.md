# SPEC-R4 — Round-4: verbatim v4 integration + P1 closure (zero-loss verify + stress re-sweep)

**Lane:** SPEC-R4 (worktree `.worktrees/lane-spec4`, branch `lane/spec4`). **Date:** 2026-10-17.
**Inputs:** `docs/scan-spec.md` v3, `docs/spec-round{1,2,3}.md` (full gap registers), the ref
tree `/opt/opencode-ref/packages/` (re-listed this round, not remembered), ours `crates/oc-*`.
**Product-adjacent edit:** `docs/scan-spec.md` ONLY (integration round). This file is the round
report.

## Mission A — corrected integration (what v4 contains)

The integration was performed by THIS agent with clause text carried VERBATIM (spot-audited:
27/27 distinctive anchors from R2 §c / R3 §5 found byte-identical in v4, modulo line wraps).
After the orchestrator's three consecutive lossy integrations (R3's existence proof for I11),
v4 restores everything v3 dropped and lands R3's register:

| Change | Clause(s) | Provenance |
|---|---|---|
| Images restored (v3 had 0 "image" hits) | **A9** | R1 S1-9 · R2 §c-A9 · R3 U1 |
| Code Mode + plugin-browser restored; **D9 number-reuse resolved** — model/variant KEEPS D9, codemode gets D10 | **D10** | R2 §c-D9 · R3 U2 |
| Determinism restored (v3 had 0 "determin" hits) | **C21** | R1 S2-13 · R2 §c-C17 · R3 U11 |
| SDK/client restored (R3 left fold decision to R4 → CLAUSE, not fold) | **D11** | R2 §c-D10 |
| Remote/container matrix restored (v3 reused B8 for width) | **B9** | R2 §c-B8 · R2 S3-1/2 |
| Storage/IO + idle-CPU restored (deep metrics stay with P2) | **E10, E11** | R2 §c |
| Search/filter — **R2-S2-4, dropped by v3 AND missed by R3's register; caught by THIS round's zero-loss check** | **E12 (new)** | R2 S2-4 |
| New U-families | **C17** terminal/PTY, **C18** CLI/service/run (incl. U5 run-mode fold), **C19** integrations, **C20** tool family | R3 U3–U7 |
| Desktop + app frontends | D6 amendment | R3 U10 |
| Enumeration-universe pin | **I10b** (35 packages + OOC list + accounting entries) | R3 U12 |
| I8 canary tiering (per-round random + per-axis first-evidence + re-canary on instrument change; 5 concrete designs bound) | I8 | R3 §3/§5 |
| R2 §c rewrites re-applied verbatim | A3, A6, B6, B7, C3, C8, C9, C11, D6, E8 | R2 S5-1 |
| R3 §2-driven rewrites (0 adequate / 5 nominal / 3 weak in v3) | A8, B8, C13, C14, C15, C16, D9, F6, F7 | R3 §2 |
| 8 fold-amendments as NAMED cells (not prose) | C1/C2, C15, A8, B7(x3), C6/A5, C16/C5 | R3 §1 folds |
| Other amendments | I2 (≥3 ecosystems), I3 (all timed behavior), I5 (in-repo instruments bound), F1/F2 (coverage % + I10b quantification) | R2 (a)/S4-6/M-4 |
| Traceability appendix added | 102 gap IDs → clause/amend/OOC | R2 M-4 |

C14 explicitly carries BOTH of R2's proposed clauses as named sub-families
(a: instruction discovery w/ system-prompt-diff artifact; b: commands/agents/prompts w/
frontmatter validation + agent-mode binding) — v3 had merged them into one line.

## Mission B — P1 closure

### 1. Zero-loss check (re-derived from round docs, mechanical)

Gap IDs re-extracted from the three round documents by script and diffed against the v4
appendix (ID-normalized):

| Register | Re-derived count | Traced in appendix | Missing | Extra |
|---|---|---|---|---|
| R1 (S1-1…S5-11) | 52 | 52 | 0 | 0 |
| R2 (S1-1…M-4) | 30 | 30 | 0 | 0 |
| R3 (U1…U12) | 12 | 12 | 0 | 0 |
| R3 folds | 8 | 8 (FOLD-1…8, minted IDs) | 0 | 0 |
| **Total** | **102** | **102** | **0** | **0** |

Every ID maps to: an owning clause (C), an amendment/named cell (A), or an out-of-scope
justification (OOC under I10b: U8 stats, U9 identity/enterprise). R3 §2's 9 stress rows are
cured by tagged rewrites; R3 §3's 5 canary designs are bound under I8.

**One NEW loss found and fixed during this check:** R2-S2-4 (search/filter quality) was
dropped by v3 and escaped R3's register entirely (R3 swept feature-space families;
search/filter is a quality-dim, R3's P1 lens couldn't see it). Restored as E12. This is the
fourth and final instance of the integration-loss class — and the reason the appendix ledger
now exists: loss is detectable by table-diff, no longer by heroic re-reading.

### 2. Stress re-sweep (bottom-up family enumeration vs v4, ONCE)

Re-listed all 35 packages; re-enumerated the six exhaustive src trees + spot-verified the
29 family-granular ones (file paths listed this round, not remembered). Family→clause
mapping of everything seen, including items no round doc had characterized:

- `core/src` tail (reference/repository/ripgrep/rpc/session/shell/skill/snapshot/state/tool/
  tool-output/v1/variant/vcs/websearch/wellknown/workspace/worktree) → C13/C20 (ripgrep,
  tool-output, vcs, websearch), C1/C17 (session, shell), D4 (v1, snapshot, state), D9
  (variant), D5 (reference, rpc id-chains), E12 (ripgrep search behavior), C8 (workspace,
  worktree). Owned.
- `tui/src/routes/session/*`: anchors/anchor-view/grouping/message-navigation/dialog-fork →
  FOLD-1 cells in C1/C2; dialog-execute/timeline/message → C16; location-missing → C12;
  thinking-syntax → FOLD-3 in A8; composer tabs → FOLD-2 in C15 + C17. Owned.
- `tui/src/component/dialog-*` (23 dialogs re-listed): each maps — image-preview→A9,
  integration→C19, mcp→C9, model/variant→D9, pair→C10, session-list→C2, shell-output→C17,
  skill→C13, theme-list→A7, update→C11, workspace-file-changes→FOLD-6, workspaces/
  worktree-name→C8, rest→C16 action-asserts. Owned.
- Previously uncharacterized: `tui/src/devtools` (dev-only route → F1's route matrix),
  `console` (empty scaffold), `posts` (static docs site), `script` (dev script) → added as
  standing accounting/OOC entries in I10b (no parity surface; this IS the U12 mechanism
  firing). `function/{api,github}.ts` → D9 provider-shape/C19; `gui-extensions/*` → D6 +
  C16 (review) + D11 (sdk). Owned or dispositioned.
- **Unowned product families remaining: ZERO** (was 12 at R3).

Adequacy self-stress of the NEW/REWORDED clauses (cheapest-legal-evidence test): C17–C20,
A9, C21, D10, D11, B9 are filesystem-anchored enums with per-cell dispositions (cheapest
legal pass = actually executing the matrix) — ADEQUATE. E12 has corpus+parity assert —
ADEQUATE. **E10/E11 are NOMINAL (2)**: metric families named, thresholds/cadence deferred to
the P2 deep pass (their owning track, per R3 §5). R3's closure criterion was "no new unowned
families and ≤2 nominals" → **0 unowned / 2 nominals: criterion MET.**

### 3. P1 verdict: **CLOSED at R4**

- Discovery: family enumeration is exhaustive (35-package universe pinned in I10b; re-swept
  this round with zero residual product families).
- Integration: verbatim, mechanically zero-loss (102/102 traced; appendix makes regression
  table-diffable).
- The only residual nominals (E10/E11) are owned by the next track by design, not by P1
  neglect.
- P1 reopens ONLY if: a field round finds a product family absent from the I10b universe, or
  the appendix diff goes non-zero at any future integration.

## Directives for the next focus — P2 (quality-dimensions round, SPEC-R5)

1. **E-track metric ratification.** Write concrete metric + threshold + percentile + capture
   artifact for E1–E4 using ref-supplied constants already pinned in the spec (StartupLoading
   500ms/3s gates; reconnect backoff 1s/5s; E1 stall-burst = >2-frame deficit; E3 RSS slope
   MB/h + span count ≥4h; E4 p95 first-paint @10k msgs). Confirm or replace each threshold
   with a measured ref baseline under I10 pin.
2. **Close the two P1-residual nominals:** E10 (DB size, log growth, state-file churn, write
   amplification — name metric, cadence, artifact) and E11 (wakeups/min at rest — name
   sampling method + threshold). These are the only NOMINAL clauses in v4; P2's exit
   criterion is zero.
3. **E12 first evidence pass:** search/filter result-parity corpus (ascii, case variants,
   combining chars, CJK, fuzzy) — E12 was never scanned by any round (dropped since R2).
4. **F6 pair-corpus generation:** materialize the risk-ranked standing axis-pair corpus
   (seed pairs are in F6) incl. no-prior-finding pairs; designate interaction-of-the-round.
5. **F7 trace-scope implementation:** the {events, files, sockets, env reads, network calls}
   × name+count+args tracer is named but not built — prototype it on one surface and run the
   I8 extra-write/extra-call canary against it.
6. **Process:** ≤45-min chunks; carry gap IDs for anything new (R5-*); integration of R5's
   diff MUST follow this round's protocol — verbatim clause text + appendix row per gap ID,
   then table-diff the appendix (the zero-loss check is now a script, not a re-read).

## Round log (this lane)
- v4 integrated verbatim; 7 dropped clauses restored (A9, C21, D10, D11, B9, E10, E11) + 1
  new loss caught (E12); U-families C17–C20 landed; I10b + I8-tiering; appendix 102/102.
- Stress re-sweep: 0 unowned families / 2 nominals (E10/E11, P2-owned). **P1 CLOSED.**
- Next: SPEC-R5 = P2 quality-dimensions round with the directives above.
