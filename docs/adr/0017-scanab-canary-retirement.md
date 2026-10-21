# ADR-0017: Retire the scanab A1 leg-swap canary

- Date: 2026-10-21
- Status: accepted (retirement)
- Ledger: SPEC-SCANAB-A1 (accepted); flake class tracked by TUI-E1-FLAKE

## Context

`a1_canary_leg_swap_verdict` was a spec-corpus canary built to observe the
"first exit chord sometimes ignored" behavior during canary bring-up
(observation window: 0/0 swaps on slow machines across every CI run). The
hypothesis it guarded (input-reader zombie SA-F13) was fixed and pinned by
dedicated tests; the spec-corpus canary family it belongs to was retired.

## Decision

Retire the canary formally: it stays `#[ignore]` opt-in (already applied
2026-10-19) and is not a release gate. The observation-window flake class it
belonged to is tracked by TUI-E1-FLAKE (34k paste retry-pass flake).

## Consequences

- No CI time spent on a 0-observation canary.
- If the leg-swap behavior ever needs guarding again, write a dedicated PTY
  test against the fixed seam (SA-F13 regression tests exist), not a
  timing-window canary.
