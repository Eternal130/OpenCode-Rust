# TUI side-by-side differential — corpus report

Comparison bar: COMPLETE cell-grid identity (char + style digest)
after every step, v2.0.24 TUI vs Rust TUI, same project dir, same
config/stub, identical input bytes. Structural mode re-examines
mismatches under the alphanumeric glyph mask (geometry+style must
still match); chrome is never masked.

## Corpus results

| script | size | run | steps | identical | text-only | mismatch | boot r/o | error |
|---|---|---|---|---|---|---|---|---|
| mouse-hover-tabstrip | 80x24 | 0 | 17 | 0 | 0 | 17 | true/true |  |
| mouse-hover-tabstrip | 80x24 | 1 | 17 | 0 | 0 | 17 | true/true |  |
| mouse-hover-tabstrip | 120x30 | 0 | 17 | 0 | 0 | 17 | true/true |  |
| mouse-hover-tabstrip | 120x30 | 1 | 17 | 0 | 0 | 17 | true/true |  |

## Determinism split (across runs)

Mismatches present in EVERY run (deterministic — fix-wave backlog):
["mouse-hover-tabstrip@80x24#0", "mouse-hover-tabstrip@80x24#1", "mouse-hover-tabstrip@80x24#2", "mouse-hover-tabstrip@80x24#3", "mouse-hover-tabstrip@80x24#4", "mouse-hover-tabstrip@80x24#5", "mouse-hover-tabstrip@80x24#6", "mouse-hover-tabstrip@80x24#7", "mouse-hover-tabstrip@80x24#8", "mouse-hover-tabstrip@80x24#9", "mouse-hover-tabstrip@80x24#10", "mouse-hover-tabstrip@80x24#11", "mouse-hover-tabstrip@80x24#12", "mouse-hover-tabstrip@80x24#13", "mouse-hover-tabstrip@80x24#14", "mouse-hover-tabstrip@80x24#15", "mouse-hover-tabstrip@80x24#16", "mouse-hover-tabstrip@120x30#0", "mouse-hover-tabstrip@120x30#1", "mouse-hover-tabstrip@120x30#2", "mouse-hover-tabstrip@120x30#3", "mouse-hover-tabstrip@120x30#4", "mouse-hover-tabstrip@120x30#5", "mouse-hover-tabstrip@120x30#6", "mouse-hover-tabstrip@120x30#7", "mouse-hover-tabstrip@120x30#8", "mouse-hover-tabstrip@120x30#9", "mouse-hover-tabstrip@120x30#10", "mouse-hover-tabstrip@120x30#11", "mouse-hover-tabstrip@120x30#12", "mouse-hover-tabstrip@120x30#13", "mouse-hover-tabstrip@120x30#14", "mouse-hover-tabstrip@120x30#15", "mouse-hover-tabstrip@120x30#16"]


## Mismatch detail

### mouse-hover-tabstrip @ 80x24 run 0 step 0 — MISMATCH (boot)

op: wait
settled: ref=true our=true | mismatches: 1920 (char 102 / style 1920), text-only 1700
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 13 (masked-text runs/margins differ)
structure: ref text-rows=4 our text-rows=3
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]

### mouse-hover-tabstrip @ 80x24 run 0 step 1 — MISMATCH (dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 1920 (char 102 / style 1920), text-only 1700
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 13 (masked-text runs/margins differ)
structure: ref text-rows=4 our text-rows=3
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]

### mouse-hover-tabstrip @ 80x24 run 0 step 2 — MISMATCH (models-cmd)

op: text("/models")
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1755
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 15 (masked-text runs/margins differ)
structure: ref text-rows=5 our text-rows=4
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]

### mouse-hover-tabstrip @ 80x24 run 0 step 3 — MISMATCH (models-open)

op: keys[enter]
settled: ref=true our=true | mismatches: 1920 (char 156 / style 1920), text-only 1727
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 11 (masked-text runs/margins differ)
structure: ref text-rows=11 our text-rows=10
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]

### mouse-hover-tabstrip @ 80x24 run 0 step 4 — MISMATCH (models-filter)

op: text("fake")
settled: ref=true our=true | mismatches: 1920 (char 261 / style 1920), text-only 1633
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 9 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=10
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]

### mouse-hover-tabstrip @ 80x24 run 0 step 5 — MISMATCH (models-select)

op: keys[enter]
settled: ref=true our=true | mismatches: 1920 (char 251 / style 1920), text-only 1535
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 7 (masked-text runs/margins differ)
structure: ref text-rows=9 our text-rows=3
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#0A0A0A]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#0A0A0A]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#0A0A0A]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#0A0A0A]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#0A0A0A]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#0A0A0A]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#0A0A0A]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#0A0A0A]

### mouse-hover-tabstrip @ 80x24 run 0 step 6 — MISMATCH (variant-dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 1920 (char 102 / style 1920), text-only 1700
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 13 (masked-text runs/margins differ)
structure: ref text-rows=4 our text-rows=3
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]

### mouse-hover-tabstrip @ 80x24 run 0 step 7 — MISMATCH (type)

op: text("Hover test")
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1754
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 15 (masked-text runs/margins differ)
structure: ref text-rows=4 our text-rows=3
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]

### mouse-hover-tabstrip @ 80x24 run 0 step 8 — MISMATCH (submit)

op: keys[enter]
settled: ref=true our=true | mismatches: 1920 (char 278 / style 1920), text-only 1632
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.       +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=6 our text-rows=4
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### mouse-hover-tabstrip @ 80x24 run 0 step 9 — MISMATCH (reply-settled)

op: wait
settled: ref=true our=true | mismatches: 1920 (char 278 / style 1920), text-only 1632
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.       +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=6 our text-rows=4
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### mouse-hover-tabstrip @ 80x24 run 0 step 10 — MISMATCH (hover-tab-title)

op: mouse(motion @4,0)
settled: ref=true our=true | mismatches: 1920 (char 278 / style 1920), text-only 1632
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.    ✕  +`
our row 0: `   Untitled session           ✕  +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=6 our text-rows=4
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### mouse-hover-tabstrip @ 80x24 run 0 step 11 — MISMATCH (hover-tab-mid)

op: mouse(motion @12,0)
settled: ref=true our=true | mismatches: 1920 (char 278 / style 1920), text-only 1632
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.    ✕  +`
our row 0: `   Untitled session           ✕  +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=6 our text-rows=4
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### mouse-hover-tabstrip @ 80x24 run 0 step 12 — MISMATCH (hover-plus-ref-col)

op: mouse(motion @30,0)
settled: ref=true our=true | mismatches: 1920 (char 278 / style 1920), text-only 1631
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.    ✕  +`
our row 0: `   Untitled session           ✕  +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=6 our text-rows=4
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### mouse-hover-tabstrip @ 80x24 run 0 step 13 — MISMATCH (hover-plus-our-col)

op: mouse(motion @22,0)
settled: ref=true our=true | mismatches: 1920 (char 278 / style 1920), text-only 1632
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.    ✕  +`
our row 0: `   Untitled session           ✕  +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=6 our text-rows=4
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### mouse-hover-tabstrip @ 80x24 run 0 step 14 — MISMATCH (hover-close-col)

op: mouse(motion @17,0)
settled: ref=true our=true | mismatches: 1920 (char 278 / style 1920), text-only 1633
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.    ✕  +`
our row 0: `   Untitled session           ✕  +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=6 our text-rows=4
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### mouse-hover-tabstrip @ 80x24 run 0 step 15 — MISMATCH (hover-off)

op: mouse(motion @40,12)
settled: ref=true our=true | mismatches: 1920 (char 278 / style 1920), text-only 1633
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.       +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=6 our text-rows=4
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### mouse-hover-tabstrip @ 80x24 run 0 step 16 — MISMATCH (exit)

op: keys[leader,q]
settled: ref=true our=true | mismatches: 1920 (char 278 / style 1920), text-only 1632
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.       +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=6 our text-rows=4
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### mouse-hover-tabstrip @ 80x24 run 1 step 0 — MISMATCH (boot)

op: wait
settled: ref=true our=true | mismatches: 1920 (char 102 / style 1920), text-only 1700
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 13 (masked-text runs/margins differ)
structure: ref text-rows=4 our text-rows=3
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]

### mouse-hover-tabstrip @ 80x24 run 1 step 1 — MISMATCH (dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 1920 (char 102 / style 1920), text-only 1700
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 13 (masked-text runs/margins differ)
structure: ref text-rows=4 our text-rows=3
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]

### mouse-hover-tabstrip @ 80x24 run 1 step 2 — MISMATCH (models-cmd)

op: text("/models")
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1755
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 15 (masked-text runs/margins differ)
structure: ref text-rows=5 our text-rows=4
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]

### mouse-hover-tabstrip @ 80x24 run 1 step 3 — MISMATCH (models-open)

op: keys[enter]
settled: ref=true our=true | mismatches: 1920 (char 156 / style 1920), text-only 1727
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 11 (masked-text runs/margins differ)
structure: ref text-rows=11 our text-rows=10
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]

### mouse-hover-tabstrip @ 80x24 run 1 step 4 — MISMATCH (models-filter)

op: text("fake")
settled: ref=true our=true | mismatches: 1920 (char 261 / style 1920), text-only 1633
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 9 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=10
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]

### mouse-hover-tabstrip @ 80x24 run 1 step 5 — MISMATCH (models-select)

op: keys[enter]
settled: ref=true our=true | mismatches: 1920 (char 251 / style 1920), text-only 1535
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 7 (masked-text runs/margins differ)
structure: ref text-rows=9 our text-rows=3
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#0A0A0A]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#0A0A0A]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#0A0A0A]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#0A0A0A]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#0A0A0A]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#0A0A0A]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#0A0A0A]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#0A0A0A]

### mouse-hover-tabstrip @ 80x24 run 1 step 6 — MISMATCH (variant-dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 1920 (char 102 / style 1920), text-only 1700
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 13 (masked-text runs/margins differ)
structure: ref text-rows=4 our text-rows=3
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]

### mouse-hover-tabstrip @ 80x24 run 1 step 7 — MISMATCH (type)

op: text("Hover test")
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1754
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 15 (masked-text runs/margins differ)
structure: ref text-rows=4 our text-rows=3
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]

### mouse-hover-tabstrip @ 80x24 run 1 step 8 — MISMATCH (submit)

op: keys[enter]
settled: ref=true our=true | mismatches: 1920 (char 278 / style 1920), text-only 1632
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.       +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=6 our text-rows=4
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### mouse-hover-tabstrip @ 80x24 run 1 step 9 — MISMATCH (reply-settled)

op: wait
settled: ref=true our=true | mismatches: 1920 (char 278 / style 1920), text-only 1632
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.       +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=6 our text-rows=4
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### mouse-hover-tabstrip @ 80x24 run 1 step 10 — MISMATCH (hover-tab-title)

op: mouse(motion @4,0)
settled: ref=true our=true | mismatches: 1920 (char 278 / style 1920), text-only 1632
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.    ✕  +`
our row 0: `   Untitled session           ✕  +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=6 our text-rows=4
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### mouse-hover-tabstrip @ 80x24 run 1 step 11 — MISMATCH (hover-tab-mid)

op: mouse(motion @12,0)
settled: ref=true our=true | mismatches: 1920 (char 278 / style 1920), text-only 1632
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.    ✕  +`
our row 0: `   Untitled session           ✕  +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=6 our text-rows=4
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### mouse-hover-tabstrip @ 80x24 run 1 step 12 — MISMATCH (hover-plus-ref-col)

op: mouse(motion @30,0)
settled: ref=true our=true | mismatches: 1920 (char 278 / style 1920), text-only 1631
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.    ✕  +`
our row 0: `   Untitled session           ✕  +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=6 our text-rows=4
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### mouse-hover-tabstrip @ 80x24 run 1 step 13 — MISMATCH (hover-plus-our-col)

op: mouse(motion @22,0)
settled: ref=true our=true | mismatches: 1920 (char 278 / style 1920), text-only 1633
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.    ✕  +`
our row 0: `   Untitled session           ✕  +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=6 our text-rows=4
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### mouse-hover-tabstrip @ 80x24 run 1 step 14 — MISMATCH (hover-close-col)

op: mouse(motion @17,0)
settled: ref=true our=true | mismatches: 1920 (char 278 / style 1920), text-only 1633
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.    ✕  +`
our row 0: `   Untitled session           ✕  +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=6 our text-rows=4
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### mouse-hover-tabstrip @ 80x24 run 1 step 15 — MISMATCH (hover-off)

op: mouse(motion @40,12)
settled: ref=true our=true | mismatches: 1920 (char 278 / style 1920), text-only 1633
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.       +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=6 our text-rows=4
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### mouse-hover-tabstrip @ 80x24 run 1 step 16 — MISMATCH (exit)

op: keys[leader,q]
settled: ref=true our=true | mismatches: 1920 (char 278 / style 1920), text-only 1632
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.       +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=6 our text-rows=4
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### mouse-hover-tabstrip @ 120x30 run 0 step 0 — MISMATCH (boot)

op: wait
settled: ref=true our=true | mismatches: 3600 (char 64 / style 3600), text-only 3393
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 18 (masked-text runs/margins differ)
structure: ref text-rows=4 our text-rows=3
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]

### mouse-hover-tabstrip @ 120x30 run 0 step 1 — MISMATCH (dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 3600 (char 64 / style 3600), text-only 3393
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 18 (masked-text runs/margins differ)
structure: ref text-rows=4 our text-rows=3
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]

### mouse-hover-tabstrip @ 120x30 run 0 step 2 — MISMATCH (models-cmd)

op: text("/models")
settled: ref=true our=true | mismatches: 3600 (char 64 / style 3600), text-only 3434
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 18 (masked-text runs/margins differ)
structure: ref text-rows=5 our text-rows=4
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]

### mouse-hover-tabstrip @ 120x30 run 0 step 3 — MISMATCH (models-open)

op: keys[enter]
settled: ref=true our=true | mismatches: 3600 (char 283 / style 3600), text-only 3279
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 8 (masked-text runs/margins differ)
structure: ref text-rows=13 our text-rows=11
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]

### mouse-hover-tabstrip @ 120x30 run 0 step 4 — MISMATCH (models-filter)

op: text("fake")
settled: ref=true our=true | mismatches: 3600 (char 363 / style 3600), text-only 3206
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 8 (masked-text runs/margins differ)
structure: ref text-rows=8 our text-rows=12
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]

### mouse-hover-tabstrip @ 120x30 run 0 step 5 — MISMATCH (models-select)

op: keys[enter]
settled: ref=true our=true | mismatches: 3600 (char 196 / style 3600), text-only 3213
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 9 (masked-text runs/margins differ)
structure: ref text-rows=9 our text-rows=3
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#0A0A0A]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#0A0A0A]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#0A0A0A]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#0A0A0A]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#0A0A0A]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#0A0A0A]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#0A0A0A]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#0A0A0A]

### mouse-hover-tabstrip @ 120x30 run 0 step 6 — MISMATCH (variant-dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 3600 (char 64 / style 3600), text-only 3393
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 18 (masked-text runs/margins differ)
structure: ref text-rows=4 our text-rows=3
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]

### mouse-hover-tabstrip @ 120x30 run 0 step 7 — MISMATCH (type)

op: text("Hover test")
settled: ref=true our=true | mismatches: 3600 (char 64 / style 3600), text-only 3435
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 18 (masked-text runs/margins differ)
structure: ref text-rows=4 our text-rows=3
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]

### mouse-hover-tabstrip @ 120x30 run 0 step 8 — MISMATCH (submit)

op: keys[enter]
settled: ref=true our=true | mismatches: 3600 (char 358 / style 3600), text-only 3233
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.       +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=6 our text-rows=4
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### mouse-hover-tabstrip @ 120x30 run 0 step 9 — MISMATCH (reply-settled)

op: wait
settled: ref=true our=true | mismatches: 3600 (char 358 / style 3600), text-only 3233
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.       +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=6 our text-rows=4
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### mouse-hover-tabstrip @ 120x30 run 0 step 10 — MISMATCH (hover-tab-title)

op: mouse(motion @4,0)
settled: ref=true our=true | mismatches: 3600 (char 358 / style 3600), text-only 3233
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.    ✕  +`
our row 0: `   Untitled session           ✕  +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=6 our text-rows=4
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### mouse-hover-tabstrip @ 120x30 run 0 step 11 — MISMATCH (hover-tab-mid)

op: mouse(motion @12,0)
settled: ref=true our=true | mismatches: 3600 (char 358 / style 3600), text-only 3233
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.    ✕  +`
our row 0: `   Untitled session           ✕  +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=6 our text-rows=4
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### mouse-hover-tabstrip @ 120x30 run 0 step 12 — MISMATCH (hover-plus-ref-col)

op: mouse(motion @30,0)
settled: ref=true our=true | mismatches: 3600 (char 358 / style 3600), text-only 3232
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.    ✕  +`
our row 0: `   Untitled session           ✕  +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=6 our text-rows=4
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### mouse-hover-tabstrip @ 120x30 run 0 step 13 — MISMATCH (hover-plus-our-col)

op: mouse(motion @22,0)
settled: ref=true our=true | mismatches: 3600 (char 358 / style 3600), text-only 3233
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.    ✕  +`
our row 0: `   Untitled session           ✕  +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=6 our text-rows=4
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### mouse-hover-tabstrip @ 120x30 run 0 step 14 — MISMATCH (hover-close-col)

op: mouse(motion @17,0)
settled: ref=true our=true | mismatches: 3600 (char 358 / style 3600), text-only 3232
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.    ✕  +`
our row 0: `   Untitled session           ✕  +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=6 our text-rows=4
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### mouse-hover-tabstrip @ 120x30 run 0 step 15 — MISMATCH (hover-off)

op: mouse(motion @40,12)
settled: ref=true our=true | mismatches: 3600 (char 358 / style 3600), text-only 3232
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.       +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=6 our text-rows=4
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### mouse-hover-tabstrip @ 120x30 run 0 step 16 — MISMATCH (exit)

op: keys[leader,q]
settled: ref=true our=true | mismatches: 3600 (char 358 / style 3600), text-only 3233
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.       +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=6 our text-rows=4
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### mouse-hover-tabstrip @ 120x30 run 1 step 0 — MISMATCH (boot)

op: wait
settled: ref=true our=true | mismatches: 3600 (char 64 / style 3600), text-only 3393
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 18 (masked-text runs/margins differ)
structure: ref text-rows=4 our text-rows=3
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]

### mouse-hover-tabstrip @ 120x30 run 1 step 1 — MISMATCH (dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 3600 (char 64 / style 3600), text-only 3393
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 18 (masked-text runs/margins differ)
structure: ref text-rows=4 our text-rows=3
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]

### mouse-hover-tabstrip @ 120x30 run 1 step 2 — MISMATCH (models-cmd)

op: text("/models")
settled: ref=true our=true | mismatches: 3600 (char 64 / style 3600), text-only 3435
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 18 (masked-text runs/margins differ)
structure: ref text-rows=5 our text-rows=4
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]

### mouse-hover-tabstrip @ 120x30 run 1 step 3 — MISMATCH (models-open)

op: keys[enter]
settled: ref=true our=true | mismatches: 3600 (char 283 / style 3600), text-only 3279
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 8 (masked-text runs/margins differ)
structure: ref text-rows=13 our text-rows=11
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]

### mouse-hover-tabstrip @ 120x30 run 1 step 4 — MISMATCH (models-filter)

op: text("fake")
settled: ref=true our=true | mismatches: 3600 (char 363 / style 3600), text-only 3206
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 8 (masked-text runs/margins differ)
structure: ref text-rows=8 our text-rows=12
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]

### mouse-hover-tabstrip @ 120x30 run 1 step 5 — MISMATCH (models-select)

op: keys[enter]
settled: ref=true our=true | mismatches: 3600 (char 196 / style 3600), text-only 3213
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 9 (masked-text runs/margins differ)
structure: ref text-rows=9 our text-rows=3
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#0A0A0A]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#0A0A0A]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#0A0A0A]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#0A0A0A]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#0A0A0A]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#0A0A0A]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#0A0A0A]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#0A0A0A]

### mouse-hover-tabstrip @ 120x30 run 1 step 6 — MISMATCH (variant-dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 3600 (char 64 / style 3600), text-only 3393
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 18 (masked-text runs/margins differ)
structure: ref text-rows=4 our text-rows=3
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]

### mouse-hover-tabstrip @ 120x30 run 1 step 7 — MISMATCH (type)

op: text("Hover test")
settled: ref=true our=true | mismatches: 3600 (char 64 / style 3600), text-only 3434
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 18 (masked-text runs/margins differ)
structure: ref text-rows=4 our text-rows=3
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]

### mouse-hover-tabstrip @ 120x30 run 1 step 8 — MISMATCH (submit)

op: keys[enter]
settled: ref=true our=true | mismatches: 3600 (char 358 / style 3600), text-only 3233
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.       +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=6 our text-rows=4
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### mouse-hover-tabstrip @ 120x30 run 1 step 9 — MISMATCH (reply-settled)

op: wait
settled: ref=true our=true | mismatches: 3600 (char 358 / style 3600), text-only 3233
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.       +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=6 our text-rows=4
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### mouse-hover-tabstrip @ 120x30 run 1 step 10 — MISMATCH (hover-tab-title)

op: mouse(motion @4,0)
settled: ref=true our=true | mismatches: 3600 (char 358 / style 3600), text-only 3232
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.    ✕  +`
our row 0: `   Untitled session           ✕  +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=6 our text-rows=4
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### mouse-hover-tabstrip @ 120x30 run 1 step 11 — MISMATCH (hover-tab-mid)

op: mouse(motion @12,0)
settled: ref=true our=true | mismatches: 3600 (char 358 / style 3600), text-only 3232
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.    ✕  +`
our row 0: `   Untitled session           ✕  +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=6 our text-rows=4
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### mouse-hover-tabstrip @ 120x30 run 1 step 12 — MISMATCH (hover-plus-ref-col)

op: mouse(motion @30,0)
settled: ref=true our=true | mismatches: 3600 (char 358 / style 3600), text-only 3231
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.    ✕  +`
our row 0: `   Untitled session           ✕  +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=6 our text-rows=4
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### mouse-hover-tabstrip @ 120x30 run 1 step 13 — MISMATCH (hover-plus-our-col)

op: mouse(motion @22,0)
settled: ref=true our=true | mismatches: 3600 (char 358 / style 3600), text-only 3232
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.    ✕  +`
our row 0: `   Untitled session           ✕  +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=6 our text-rows=4
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### mouse-hover-tabstrip @ 120x30 run 1 step 14 — MISMATCH (hover-close-col)

op: mouse(motion @17,0)
settled: ref=true our=true | mismatches: 3600 (char 358 / style 3600), text-only 3232
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.    ✕  +`
our row 0: `   Untitled session           ✕  +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=6 our text-rows=4
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### mouse-hover-tabstrip @ 120x30 run 1 step 15 — MISMATCH (hover-off)

op: mouse(motion @40,12)
settled: ref=true our=true | mismatches: 3600 (char 358 / style 3600), text-only 3233
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.       +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=6 our text-rows=4
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### mouse-hover-tabstrip @ 120x30 run 1 step 16 — MISMATCH (exit)

op: keys[leader,q]
settled: ref=true our=true | mismatches: 3600 (char 358 / style 3600), text-only 3232
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.       +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=6 our text-rows=4
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

