# TUI side-by-side differential — corpus report

Comparison bar: COMPLETE cell-grid identity (char + style digest)
after every step, v2.0.24 TUI vs Rust TUI, same project dir, same
config/stub, identical input bytes. Structural mode re-examines
mismatches under the alphanumeric glyph mask (geometry+style must
still match); chrome is never masked.

## Corpus results

| script | size | run | steps | identical | text-only | mismatch | boot r/o | error |
|---|---|---|---|---|---|---|---|---|
| boot-home | 80x24 | 0 | 3 | 0 | 0 | 3 | true/true |  |
| boot-home | 80x24 | 1 | 3 | 0 | 0 | 3 | true/true |  |
| boot-home | 120x30 | 0 | 3 | 0 | 0 | 3 | true/true |  |
| boot-home | 120x30 | 1 | 3 | 0 | 0 | 3 | true/true |  |
| composer-clear | 80x24 | 0 | 10 | 0 | 0 | 10 | true/true |  |
| composer-clear | 80x24 | 1 | 10 | 0 | 0 | 10 | true/true |  |
| composer-clear | 120x30 | 0 | 10 | 0 | 0 | 10 | true/true |  |
| composer-clear | 120x30 | 1 | 10 | 0 | 0 | 10 | true/true |  |
| help-dialog | 80x24 | 0 | 8 | 2 | 0 | 6 | true/true |  |
| help-dialog | 80x24 | 1 | 8 | 2 | 0 | 6 | true/true |  |
| help-dialog | 120x30 | 0 | 8 | 2 | 0 | 6 | true/true |  |
| help-dialog | 120x30 | 1 | 8 | 2 | 0 | 6 | true/true |  |
| long-reply-scroll | 80x24 | 0 | 16 | 0 | 0 | 16 | true/true |  |
| long-reply-scroll | 80x24 | 1 | 16 | 0 | 0 | 16 | true/true |  |
| long-reply-scroll | 120x30 | 0 | 16 | 0 | 0 | 16 | true/true |  |
| long-reply-scroll | 120x30 | 1 | 16 | 0 | 0 | 16 | true/true |  |
| mouse-hover-tabstrip | 80x24 | 0 | 15 | 0 | 0 | 15 | true/true |  |
| mouse-hover-tabstrip | 80x24 | 1 | 15 | 0 | 0 | 15 | true/true |  |
| mouse-hover-tabstrip | 120x30 | 0 | 15 | 0 | 0 | 15 | true/true |  |
| mouse-hover-tabstrip | 120x30 | 1 | 15 | 0 | 0 | 15 | true/true |  |
| palette | 80x24 | 0 | 6 | 0 | 0 | 6 | true/true |  |
| palette | 80x24 | 1 | 6 | 0 | 0 | 6 | true/true |  |
| palette | 120x30 | 0 | 6 | 0 | 0 | 6 | true/true |  |
| palette | 120x30 | 1 | 6 | 0 | 0 | 6 | true/true |  |
| rename-dialog | 80x24 | 0 | 15 | 0 | 0 | 15 | true/true |  |
| rename-dialog | 80x24 | 1 | 15 | 0 | 0 | 15 | true/true |  |
| rename-dialog | 120x30 | 0 | 15 | 0 | 0 | 15 | true/true |  |
| rename-dialog | 120x30 | 1 | 15 | 0 | 0 | 15 | true/true |  |
| resize | 80x24 | 0 | 6 | 0 | 0 | 6 | true/true |  |
| resize | 80x24 | 1 | 6 | 0 | 0 | 6 | true/true |  |
| resize | 120x30 | 0 | 6 | 0 | 0 | 6 | true/true |  |
| resize | 120x30 | 1 | 6 | 0 | 0 | 6 | true/true |  |
| session-list-dialog | 80x24 | 0 | 14 | 0 | 0 | 14 | true/true |  |
| session-list-dialog | 80x24 | 1 | 14 | 0 | 0 | 14 | true/true |  |
| session-list-dialog | 120x30 | 0 | 14 | 0 | 0 | 14 | true/true |  |
| session-list-dialog | 120x30 | 1 | 14 | 0 | 0 | 14 | true/true |  |
| tabs-keyboard | 80x24 | 0 | 8 | 0 | 0 | 8 | true/true |  |
| tabs-keyboard | 80x24 | 1 | 8 | 0 | 0 | 8 | true/true |  |
| tabs-keyboard | 120x30 | 0 | 8 | 0 | 0 | 8 | true/true |  |
| tabs-keyboard | 120x30 | 1 | 8 | 0 | 0 | 8 | true/true |  |
| themes-nord | 80x24 | 0 | 8 | 0 | 0 | 8 | true/true |  |
| themes-nord | 80x24 | 1 | 8 | 0 | 0 | 8 | true/true |  |
| themes-nord | 120x30 | 0 | 8 | 0 | 0 | 8 | true/true |  |
| themes-nord | 120x30 | 1 | 8 | 0 | 0 | 8 | true/true |  |
| thought-expand | 80x24 | 0 | 14 | 0 | 0 | 14 | true/true |  |
| thought-expand | 80x24 | 1 | 14 | 0 | 0 | 14 | true/true |  |
| thought-expand | 120x30 | 0 | 14 | 0 | 0 | 14 | true/true |  |
| thought-expand | 120x30 | 1 | 14 | 0 | 0 | 14 | true/true |  |
| thought-render | 80x24 | 0 | 11 | 0 | 0 | 11 | true/true |  |
| thought-render | 80x24 | 1 | 11 | 0 | 0 | 11 | true/true |  |
| thought-render | 120x30 | 0 | 11 | 0 | 0 | 11 | true/true |  |
| thought-render | 120x30 | 1 | 11 | 0 | 0 | 11 | true/true |  |
| type-edit-submit | 80x24 | 0 | 13 | 0 | 0 | 13 | true/true |  |
| type-edit-submit | 80x24 | 1 | 13 | 0 | 0 | 13 | true/true |  |
| type-edit-submit | 120x30 | 0 | 13 | 0 | 0 | 13 | true/true |  |
| type-edit-submit | 120x30 | 1 | 13 | 0 | 0 | 13 | true/true |  |

## Determinism split (across runs)

Mismatches present in EVERY run (deterministic — fix-wave backlog):
["boot-home@80x24#0", "boot-home@80x24#1", "boot-home@80x24#2", "boot-home@120x30#0", "boot-home@120x30#1", "boot-home@120x30#2", "composer-clear@80x24#0", "composer-clear@80x24#1", "composer-clear@80x24#2", "composer-clear@80x24#3", "composer-clear@80x24#4", "composer-clear@80x24#5", "composer-clear@80x24#6", "composer-clear@80x24#7", "composer-clear@80x24#8", "composer-clear@80x24#9", "composer-clear@120x30#0", "composer-clear@120x30#1", "composer-clear@120x30#2", "composer-clear@120x30#3", "composer-clear@120x30#4", "composer-clear@120x30#5", "composer-clear@120x30#6", "composer-clear@120x30#7", "composer-clear@120x30#8", "composer-clear@120x30#9", "help-dialog@80x24#0", "help-dialog@80x24#1", "help-dialog@80x24#2", "help-dialog@80x24#3", "help-dialog@80x24#4", "help-dialog@80x24#5", "help-dialog@120x30#0", "help-dialog@120x30#1", "help-dialog@120x30#2", "help-dialog@120x30#3", "help-dialog@120x30#4", "help-dialog@120x30#5", "long-reply-scroll@80x24#0", "long-reply-scroll@80x24#1", "long-reply-scroll@80x24#2", "long-reply-scroll@80x24#3", "long-reply-scroll@80x24#4", "long-reply-scroll@80x24#5", "long-reply-scroll@80x24#6", "long-reply-scroll@80x24#7", "long-reply-scroll@80x24#8", "long-reply-scroll@80x24#9", "long-reply-scroll@80x24#10", "long-reply-scroll@80x24#11", "long-reply-scroll@80x24#12", "long-reply-scroll@80x24#13", "long-reply-scroll@80x24#14", "long-reply-scroll@80x24#15", "long-reply-scroll@120x30#0", "long-reply-scroll@120x30#1", "long-reply-scroll@120x30#2", "long-reply-scroll@120x30#3", "long-reply-scroll@120x30#4", "long-reply-scroll@120x30#5", "long-reply-scroll@120x30#6", "long-reply-scroll@120x30#7", "long-reply-scroll@120x30#8", "long-reply-scroll@120x30#9", "long-reply-scroll@120x30#10", "long-reply-scroll@120x30#11", "long-reply-scroll@120x30#12", "long-reply-scroll@120x30#13", "long-reply-scroll@120x30#14", "long-reply-scroll@120x30#15", "mouse-hover-tabstrip@80x24#0", "mouse-hover-tabstrip@80x24#1", "mouse-hover-tabstrip@80x24#2", "mouse-hover-tabstrip@80x24#3", "mouse-hover-tabstrip@80x24#4", "mouse-hover-tabstrip@80x24#5", "mouse-hover-tabstrip@80x24#6", "mouse-hover-tabstrip@80x24#7", "mouse-hover-tabstrip@80x24#8", "mouse-hover-tabstrip@80x24#9", "mouse-hover-tabstrip@80x24#10", "mouse-hover-tabstrip@80x24#11", "mouse-hover-tabstrip@80x24#12", "mouse-hover-tabstrip@80x24#13", "mouse-hover-tabstrip@80x24#14", "mouse-hover-tabstrip@120x30#0", "mouse-hover-tabstrip@120x30#1", "mouse-hover-tabstrip@120x30#2", "mouse-hover-tabstrip@120x30#3", "mouse-hover-tabstrip@120x30#4", "mouse-hover-tabstrip@120x30#5", "mouse-hover-tabstrip@120x30#6", "mouse-hover-tabstrip@120x30#7", "mouse-hover-tabstrip@120x30#8", "mouse-hover-tabstrip@120x30#9", "mouse-hover-tabstrip@120x30#10", "mouse-hover-tabstrip@120x30#11", "mouse-hover-tabstrip@120x30#12", "mouse-hover-tabstrip@120x30#13", "mouse-hover-tabstrip@120x30#14", "palette@80x24#0", "palette@80x24#1", "palette@80x24#2", "palette@80x24#3", "palette@80x24#4", "palette@80x24#5", "palette@120x30#0", "palette@120x30#1", "palette@120x30#2", "palette@120x30#3", "palette@120x30#4", "palette@120x30#5", "rename-dialog@80x24#0", "rename-dialog@80x24#1", "rename-dialog@80x24#2", "rename-dialog@80x24#3", "rename-dialog@80x24#4", "rename-dialog@80x24#5", "rename-dialog@80x24#6", "rename-dialog@80x24#7", "rename-dialog@80x24#8", "rename-dialog@80x24#9", "rename-dialog@80x24#10", "rename-dialog@80x24#11", "rename-dialog@80x24#12", "rename-dialog@80x24#13", "rename-dialog@80x24#14", "rename-dialog@120x30#0", "rename-dialog@120x30#1", "rename-dialog@120x30#2", "rename-dialog@120x30#3", "rename-dialog@120x30#4", "rename-dialog@120x30#5", "rename-dialog@120x30#6", "rename-dialog@120x30#7", "rename-dialog@120x30#8", "rename-dialog@120x30#9", "rename-dialog@120x30#10", "rename-dialog@120x30#11", "rename-dialog@120x30#12", "rename-dialog@120x30#13", "rename-dialog@120x30#14", "resize@80x24#0", "resize@80x24#1", "resize@80x24#2", "resize@80x24#3", "resize@80x24#4", "resize@80x24#5", "resize@120x30#0", "resize@120x30#1", "resize@120x30#2", "resize@120x30#3", "resize@120x30#4", "resize@120x30#5", "session-list-dialog@80x24#0", "session-list-dialog@80x24#1", "session-list-dialog@80x24#2", "session-list-dialog@80x24#3", "session-list-dialog@80x24#4", "session-list-dialog@80x24#5", "session-list-dialog@80x24#6", "session-list-dialog@80x24#7", "session-list-dialog@80x24#8", "session-list-dialog@80x24#9", "session-list-dialog@80x24#10", "session-list-dialog@80x24#11", "session-list-dialog@80x24#12", "session-list-dialog@80x24#13", "session-list-dialog@120x30#0", "session-list-dialog@120x30#1", "session-list-dialog@120x30#2", "session-list-dialog@120x30#3", "session-list-dialog@120x30#4", "session-list-dialog@120x30#5", "session-list-dialog@120x30#6", "session-list-dialog@120x30#7", "session-list-dialog@120x30#8", "session-list-dialog@120x30#9", "session-list-dialog@120x30#10", "session-list-dialog@120x30#11", "session-list-dialog@120x30#12", "session-list-dialog@120x30#13", "tabs-keyboard@80x24#0", "tabs-keyboard@80x24#1", "tabs-keyboard@80x24#2", "tabs-keyboard@80x24#3", "tabs-keyboard@80x24#4", "tabs-keyboard@80x24#5", "tabs-keyboard@80x24#6", "tabs-keyboard@80x24#7", "tabs-keyboard@120x30#0", "tabs-keyboard@120x30#1", "tabs-keyboard@120x30#2", "tabs-keyboard@120x30#3", "tabs-keyboard@120x30#4", "tabs-keyboard@120x30#5", "tabs-keyboard@120x30#6", "tabs-keyboard@120x30#7", "themes-nord@80x24#0", "themes-nord@80x24#1", "themes-nord@80x24#2", "themes-nord@80x24#3", "themes-nord@80x24#4", "themes-nord@80x24#5", "themes-nord@80x24#6", "themes-nord@80x24#7", "themes-nord@120x30#0", "themes-nord@120x30#1", "themes-nord@120x30#2", "themes-nord@120x30#3", "themes-nord@120x30#4", "themes-nord@120x30#5", "themes-nord@120x30#6", "themes-nord@120x30#7", "thought-expand@80x24#0", "thought-expand@80x24#1", "thought-expand@80x24#2", "thought-expand@80x24#3", "thought-expand@80x24#4", "thought-expand@80x24#5", "thought-expand@80x24#6", "thought-expand@80x24#7", "thought-expand@80x24#8", "thought-expand@80x24#9", "thought-expand@80x24#10", "thought-expand@80x24#11", "thought-expand@80x24#12", "thought-expand@80x24#13", "thought-expand@120x30#0", "thought-expand@120x30#1", "thought-expand@120x30#2", "thought-expand@120x30#3", "thought-expand@120x30#4", "thought-expand@120x30#5", "thought-expand@120x30#6", "thought-expand@120x30#7", "thought-expand@120x30#8", "thought-expand@120x30#9", "thought-expand@120x30#10", "thought-expand@120x30#11", "thought-expand@120x30#12", "thought-expand@120x30#13", "thought-render@80x24#0", "thought-render@80x24#1", "thought-render@80x24#2", "thought-render@80x24#3", "thought-render@80x24#4", "thought-render@80x24#5", "thought-render@80x24#6", "thought-render@80x24#7", "thought-render@80x24#8", "thought-render@80x24#9", "thought-render@80x24#10", "thought-render@120x30#0", "thought-render@120x30#1", "thought-render@120x30#2", "thought-render@120x30#3", "thought-render@120x30#4", "thought-render@120x30#5", "thought-render@120x30#6", "thought-render@120x30#7", "thought-render@120x30#8", "thought-render@120x30#9", "thought-render@120x30#10", "type-edit-submit@80x24#0", "type-edit-submit@80x24#1", "type-edit-submit@80x24#2", "type-edit-submit@80x24#3", "type-edit-submit@80x24#4", "type-edit-submit@80x24#5", "type-edit-submit@80x24#6", "type-edit-submit@80x24#7", "type-edit-submit@80x24#8", "type-edit-submit@80x24#9", "type-edit-submit@80x24#10", "type-edit-submit@80x24#11", "type-edit-submit@80x24#12", "type-edit-submit@120x30#0", "type-edit-submit@120x30#1", "type-edit-submit@120x30#2", "type-edit-submit@120x30#3", "type-edit-submit@120x30#4", "type-edit-submit@120x30#5", "type-edit-submit@120x30#6", "type-edit-submit@120x30#7", "type-edit-submit@120x30#8", "type-edit-submit@120x30#9", "type-edit-submit@120x30#10", "type-edit-submit@120x30#11", "type-edit-submit@120x30#12"]


## Mismatch detail

### boot-home @ 80x24 run 0 step 0 — MISMATCH (boot)

op: wait
settled: ref=true our=true | mismatches: 1920 (char 84 / style 1920), text-only 1713
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

### boot-home @ 80x24 run 0 step 1 — MISMATCH (dismiss-first-run)

op: keys[esc]
settled: ref=true our=true | mismatches: 1920 (char 84 / style 1920), text-only 1713
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

### boot-home @ 80x24 run 0 step 2 — MISMATCH (exit)

op: keys[leader,q]
settled: ref=true our=true | mismatches: 1920 (char 84 / style 1920), text-only 1713
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

### boot-home @ 80x24 run 1 step 0 — MISMATCH (boot)

op: wait
settled: ref=true our=true | mismatches: 1920 (char 84 / style 1920), text-only 1713
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

### boot-home @ 80x24 run 1 step 1 — MISMATCH (dismiss-first-run)

op: keys[esc]
settled: ref=true our=true | mismatches: 1920 (char 84 / style 1920), text-only 1713
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

### boot-home @ 80x24 run 1 step 2 — MISMATCH (exit)

op: keys[leader,q]
settled: ref=true our=true | mismatches: 1920 (char 84 / style 1920), text-only 1713
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

### boot-home @ 120x30 run 0 step 0 — MISMATCH (boot)

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

### boot-home @ 120x30 run 0 step 1 — MISMATCH (dismiss-first-run)

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

### boot-home @ 120x30 run 0 step 2 — MISMATCH (exit)

op: keys[leader,q]
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

### boot-home @ 120x30 run 1 step 0 — MISMATCH (boot)

op: wait
settled: ref=true our=true | mismatches: 3600 (char 102 / style 3600), text-only 3380
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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

### boot-home @ 120x30 run 1 step 1 — MISMATCH (dismiss-first-run)

op: keys[esc]
settled: ref=true our=true | mismatches: 3600 (char 102 / style 3600), text-only 3380
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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

### boot-home @ 120x30 run 1 step 2 — MISMATCH (exit)

op: keys[leader,q]
settled: ref=true our=true | mismatches: 3600 (char 102 / style 3600), text-only 3380
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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

### composer-clear @ 80x24 run 0 step 0 — MISMATCH (boot)

op: wait
settled: ref=true our=true | mismatches: 1920 (char 84 / style 1920), text-only 1713
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

### composer-clear @ 80x24 run 0 step 1 — MISMATCH (dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 1920 (char 84 / style 1920), text-only 1713
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

### composer-clear @ 80x24 run 0 step 2 — MISMATCH (type)

op: text("draft text")
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

### composer-clear @ 80x24 run 0 step 3 — MISMATCH (caret-left)

op: keys[left,left,left]
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1755
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

### composer-clear @ 80x24 run 0 step 4 — MISMATCH (caret-right)

op: keys[right]
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1755
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

### composer-clear @ 80x24 run 0 step 5 — MISMATCH (caret-home)

op: keys[home]
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

### composer-clear @ 80x24 run 0 step 6 — MISMATCH (more-text)

op: text("abc")
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1755
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

### composer-clear @ 80x24 run 0 step 7 — MISMATCH (clear)

op: keys[ctrl+c]
settled: ref=true our=true | mismatches: 1920 (char 84 / style 1920), text-only 1713
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

### composer-clear @ 80x24 run 0 step 8 — MISMATCH (cleared)

op: wait
settled: ref=true our=true | mismatches: 1920 (char 84 / style 1920), text-only 1713
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

### composer-clear @ 80x24 run 0 step 9 — MISMATCH (exit)

op: keys[leader,q]
settled: ref=true our=true | mismatches: 1920 (char 84 / style 1920), text-only 1713
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

### composer-clear @ 80x24 run 1 step 0 — MISMATCH (boot)

op: wait
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1713
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

### composer-clear @ 80x24 run 1 step 1 — MISMATCH (dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1713
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

### composer-clear @ 80x24 run 1 step 2 — MISMATCH (type)

op: text("draft text")
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1755
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

### composer-clear @ 80x24 run 1 step 3 — MISMATCH (caret-left)

op: keys[left,left,left]
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

### composer-clear @ 80x24 run 1 step 4 — MISMATCH (caret-right)

op: keys[right]
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1755
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

### composer-clear @ 80x24 run 1 step 5 — MISMATCH (caret-home)

op: keys[home]
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1755
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

### composer-clear @ 80x24 run 1 step 6 — MISMATCH (more-text)

op: text("abc")
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

### composer-clear @ 80x24 run 1 step 7 — MISMATCH (clear)

op: keys[ctrl+c]
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1713
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

### composer-clear @ 80x24 run 1 step 8 — MISMATCH (cleared)

op: wait
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1713
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

### composer-clear @ 80x24 run 1 step 9 — MISMATCH (exit)

op: keys[leader,q]
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1713
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

### composer-clear @ 120x30 run 0 step 0 — MISMATCH (boot)

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

### composer-clear @ 120x30 run 0 step 1 — MISMATCH (dismiss)

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

### composer-clear @ 120x30 run 0 step 2 — MISMATCH (type)

op: text("draft text")
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

### composer-clear @ 120x30 run 0 step 3 — MISMATCH (caret-left)

op: keys[left,left,left]
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

### composer-clear @ 120x30 run 0 step 4 — MISMATCH (caret-right)

op: keys[right]
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

### composer-clear @ 120x30 run 0 step 5 — MISMATCH (caret-home)

op: keys[home]
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

### composer-clear @ 120x30 run 0 step 6 — MISMATCH (more-text)

op: text("abc")
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

### composer-clear @ 120x30 run 0 step 7 — MISMATCH (clear)

op: keys[ctrl+c]
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

### composer-clear @ 120x30 run 0 step 8 — MISMATCH (cleared)

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

### composer-clear @ 120x30 run 0 step 9 — MISMATCH (exit)

op: keys[leader,q]
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

### composer-clear @ 120x30 run 1 step 0 — MISMATCH (boot)

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

### composer-clear @ 120x30 run 1 step 1 — MISMATCH (dismiss)

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

### composer-clear @ 120x30 run 1 step 2 — MISMATCH (type)

op: text("draft text")
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

### composer-clear @ 120x30 run 1 step 3 — MISMATCH (caret-left)

op: keys[left,left,left]
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

### composer-clear @ 120x30 run 1 step 4 — MISMATCH (caret-right)

op: keys[right]
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

### composer-clear @ 120x30 run 1 step 5 — MISMATCH (caret-home)

op: keys[home]
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

### composer-clear @ 120x30 run 1 step 6 — MISMATCH (more-text)

op: text("abc")
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

### composer-clear @ 120x30 run 1 step 7 — MISMATCH (clear)

op: keys[ctrl+c]
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

### composer-clear @ 120x30 run 1 step 8 — MISMATCH (cleared)

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

### composer-clear @ 120x30 run 1 step 9 — MISMATCH (exit)

op: keys[leader,q]
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

### help-dialog @ 80x24 run 0 step 0 — MISMATCH (boot)

op: wait
settled: ref=true our=true | mismatches: 1920 (char 85 / style 1920), text-only 1713
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

### help-dialog @ 80x24 run 0 step 1 — MISMATCH (dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 1920 (char 85 / style 1920), text-only 1713
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

### help-dialog @ 80x24 run 0 step 2 — MISMATCH (slash)

op: text("/help")
settled: ref=true our=true | mismatches: 1920 (char 65 / style 1920), text-only 1754
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

### help-dialog @ 80x24 run 0 step 3 — MISMATCH (submit)

op: keys[enter]
settled: ref=true our=true | mismatches: 1920 (char 187 / style 1920), text-only 1599
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 7 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=6
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]

### help-dialog @ 80x24 run 0 step 4 — MISMATCH (help-open)

op: wait
settled: ref=true our=true | mismatches: 1920 (char 187 / style 1920), text-only 1599
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 7 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=6
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]

### help-dialog @ 80x24 run 0 step 5 — MISMATCH (help-close)

op: keys[esc]
settled: ref=true our=true | mismatches: 1920 (char 85 / style 1920), text-only 1713
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

### help-dialog @ 80x24 run 1 step 0 — MISMATCH (boot)

op: wait
settled: ref=true our=true | mismatches: 1920 (char 65 / style 1920), text-only 1713
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

### help-dialog @ 80x24 run 1 step 1 — MISMATCH (dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 1920 (char 65 / style 1920), text-only 1713
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

### help-dialog @ 80x24 run 1 step 2 — MISMATCH (slash)

op: text("/help")
settled: ref=true our=true | mismatches: 1920 (char 65 / style 1920), text-only 1755
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

### help-dialog @ 80x24 run 1 step 3 — MISMATCH (submit)

op: keys[enter]
settled: ref=true our=true | mismatches: 1920 (char 187 / style 1920), text-only 1599
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 7 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=6
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]

### help-dialog @ 80x24 run 1 step 4 — MISMATCH (help-open)

op: wait
settled: ref=true our=true | mismatches: 1920 (char 187 / style 1920), text-only 1599
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 7 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=6
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]

### help-dialog @ 80x24 run 1 step 5 — MISMATCH (help-close)

op: keys[esc]
settled: ref=true our=true | mismatches: 1920 (char 65 / style 1920), text-only 1713
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

### help-dialog @ 120x30 run 0 step 0 — MISMATCH (boot)

op: wait
settled: ref=true our=true | mismatches: 3600 (char 103 / style 3600), text-only 3380
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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

### help-dialog @ 120x30 run 0 step 1 — MISMATCH (dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 3600 (char 103 / style 3600), text-only 3380
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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

### help-dialog @ 120x30 run 0 step 2 — MISMATCH (slash)

op: text("/help")
settled: ref=true our=true | mismatches: 3600 (char 65 / style 3600), text-only 3434
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

### help-dialog @ 120x30 run 0 step 3 — MISMATCH (submit)

op: keys[enter]
settled: ref=true our=true | mismatches: 3600 (char 233 / style 3600), text-only 3215
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 8 (masked-text runs/margins differ)
structure: ref text-rows=8 our text-rows=6
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]

### help-dialog @ 120x30 run 0 step 4 — MISMATCH (help-open)

op: wait
settled: ref=true our=true | mismatches: 3600 (char 233 / style 3600), text-only 3215
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 8 (masked-text runs/margins differ)
structure: ref text-rows=8 our text-rows=6
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]

### help-dialog @ 120x30 run 0 step 5 — MISMATCH (help-close)

op: keys[esc]
settled: ref=true our=true | mismatches: 3600 (char 103 / style 3600), text-only 3380
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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

### help-dialog @ 120x30 run 1 step 0 — MISMATCH (boot)

op: wait
settled: ref=true our=true | mismatches: 3600 (char 65 / style 3600), text-only 3393
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

### help-dialog @ 120x30 run 1 step 1 — MISMATCH (dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 3600 (char 65 / style 3600), text-only 3393
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

### help-dialog @ 120x30 run 1 step 2 — MISMATCH (slash)

op: text("/help")
settled: ref=true our=true | mismatches: 3600 (char 65 / style 3600), text-only 3434
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

### help-dialog @ 120x30 run 1 step 3 — MISMATCH (submit)

op: keys[enter]
settled: ref=true our=true | mismatches: 3600 (char 233 / style 3600), text-only 3215
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 8 (masked-text runs/margins differ)
structure: ref text-rows=8 our text-rows=6
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]

### help-dialog @ 120x30 run 1 step 4 — MISMATCH (help-open)

op: wait
settled: ref=true our=true | mismatches: 3600 (char 233 / style 3600), text-only 3215
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 8 (masked-text runs/margins differ)
structure: ref text-rows=8 our text-rows=6
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]

### help-dialog @ 120x30 run 1 step 5 — MISMATCH (help-close)

op: keys[esc]
settled: ref=true our=true | mismatches: 3600 (char 65 / style 3600), text-only 3393
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

### long-reply-scroll @ 80x24 run 0 step 0 — MISMATCH (boot)

op: wait
settled: ref=true our=true | mismatches: 1920 (char 84 / style 1920), text-only 1713
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

### long-reply-scroll @ 80x24 run 0 step 1 — MISMATCH (dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 1920 (char 84 / style 1920), text-only 1713
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

### long-reply-scroll @ 80x24 run 0 step 2 — MISMATCH (models-cmd)

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

### long-reply-scroll @ 80x24 run 0 step 3 — MISMATCH (models-open)

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

### long-reply-scroll @ 80x24 run 0 step 4 — MISMATCH (models-filter)

op: text("fake")
settled: ref=true our=true | mismatches: 1920 (char 273 / style 1920), text-only 1607
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 11 (masked-text runs/margins differ)
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

### long-reply-scroll @ 80x24 run 0 step 5 — MISMATCH (models-select)

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

### long-reply-scroll @ 80x24 run 0 step 6 — MISMATCH (variant-dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 1920 (char 84 / style 1920), text-only 1713
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

### long-reply-scroll @ 80x24 run 0 step 7 — MISMATCH (type)

op: text("Give me the long fixture")
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

### long-reply-scroll @ 80x24 run 0 step 8 — MISMATCH (submit)

op: keys[enter]
settled: ref=true our=true | mismatches: 1920 (char 1214 / style 1920), text-only 970
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Long reply line 1 of the pari +`
our row 0: `   Untitled session              +`
ref row 1: `     Long reply line 20 of the parity scroll fixture.`
our row 1: `     Long reply line 10 of the parity scroll fixture. Long reply line 11 of`
ref row 2: `     Long reply line 21 of the parity scroll fixture.`
our row 2: `     the parity scroll fixture. Long reply line 12 of the parity scroll`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=17 our text-rows=18
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'L'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'o'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'n'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'g'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) ' '→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### long-reply-scroll @ 80x24 run 0 step 9 — MISMATCH (settled)

op: wait
settled: ref=true our=true | mismatches: 1920 (char 1214 / style 1920), text-only 970
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Long reply line 1 of the pari +`
our row 0: `   Untitled session              +`
ref row 1: `     Long reply line 20 of the parity scroll fixture.`
our row 1: `     Long reply line 10 of the parity scroll fixture. Long reply line 11 of`
ref row 2: `     Long reply line 21 of the parity scroll fixture.`
our row 2: `     the parity scroll fixture. Long reply line 12 of the parity scroll`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=17 our text-rows=18
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'L'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'o'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'n'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'g'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) ' '→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### long-reply-scroll @ 80x24 run 0 step 10 — MISMATCH (pageup)

op: keys[pageup]
settled: ref=true our=true | mismatches: 1920 (char 1212 / style 1920), text-only 927
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Long reply line 1 of the pari +`
our row 0: `   Untitled session              +`
ref row 1: `     Long reply line 13 of the parity scroll fixture.`
our row 1: `     + Thought`
ref row 2: `     Long reply line 14 of the parity scroll fixture.`
our row 2: ``
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=19 our text-rows=18
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'L'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'o'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'n'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'g'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) ' '→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### long-reply-scroll @ 80x24 run 0 step 11 — MISMATCH (pageup-again)

op: keys[pageup]
settled: ref=true our=true | mismatches: 1920 (char 1079 / style 1920), text-only 916
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Long reply line 1 of the pari +`
our row 0: `   Untitled session              +`
ref row 1: `     Long reply line 6 of the parity scroll fixture.`
our row 1: ``
ref row 2: `     Long reply line 7 of the parity scroll fixture.`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=19 our text-rows=14
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'L'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'o'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'n'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'g'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) ' '→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### long-reply-scroll @ 80x24 run 0 step 12 — MISMATCH (pagedown)

op: keys[pagedown]
settled: ref=true our=true | mismatches: 1920 (char 1280 / style 1920), text-only 936
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Long reply line 1 of the pari +`
our row 0: `   Untitled session              +`
ref row 1: `     Long reply line 14 of the parity scroll fixture.`
our row 1: `     parity scroll fixture. Long reply line 3 of the parity scroll fixture.`
ref row 2: `     Long reply line 15 of the parity scroll fixture.`
our row 2: `     Long reply line 4 of the parity scroll fixture. Long reply line 5 of the`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=19 our text-rows=19
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'L'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'o'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'n'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'g'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) ' '→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### long-reply-scroll @ 80x24 run 0 step 13 — MISMATCH (home)

op: keys[home]
settled: ref=true our=true | mismatches: 1920 (char 741 / style 1920), text-only 1240
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Long reply line 1 of the pari +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=14 our text-rows=14
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'L'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'o'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'n'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'g'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) ' '→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### long-reply-scroll @ 80x24 run 0 step 14 — MISMATCH (end)

op: keys[end]
settled: ref=true our=true | mismatches: 1920 (char 1214 / style 1920), text-only 971
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Long reply line 1 of the pari +`
our row 0: `   Untitled session              +`
ref row 1: `     Long reply line 20 of the parity scroll fixture.`
our row 1: `     Long reply line 10 of the parity scroll fixture. Long reply line 11 of`
ref row 2: `     Long reply line 21 of the parity scroll fixture.`
our row 2: `     the parity scroll fixture. Long reply line 12 of the parity scroll`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=17 our text-rows=18
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'L'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'o'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'n'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'g'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) ' '→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### long-reply-scroll @ 80x24 run 0 step 15 — MISMATCH (exit)

op: keys[leader,q]
settled: ref=true our=true | mismatches: 1920 (char 1214 / style 1920), text-only 970
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Long reply line 1 of the pari +`
our row 0: `   Untitled session              +`
ref row 1: `     Long reply line 20 of the parity scroll fixture.`
our row 1: `     Long reply line 10 of the parity scroll fixture. Long reply line 11 of`
ref row 2: `     Long reply line 21 of the parity scroll fixture.`
our row 2: `     the parity scroll fixture. Long reply line 12 of the parity scroll`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=17 our text-rows=18
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'L'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'o'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'n'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'g'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) ' '→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### long-reply-scroll @ 80x24 run 1 step 0 — MISMATCH (boot)

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

### long-reply-scroll @ 80x24 run 1 step 1 — MISMATCH (dismiss)

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

### long-reply-scroll @ 80x24 run 1 step 2 — MISMATCH (models-cmd)

op: text("/models")
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1754
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

### long-reply-scroll @ 80x24 run 1 step 3 — MISMATCH (models-open)

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

### long-reply-scroll @ 80x24 run 1 step 4 — MISMATCH (models-filter)

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

### long-reply-scroll @ 80x24 run 1 step 5 — MISMATCH (models-select)

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

### long-reply-scroll @ 80x24 run 1 step 6 — MISMATCH (variant-dismiss)

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

### long-reply-scroll @ 80x24 run 1 step 7 — MISMATCH (type)

op: text("Give me the long fixture")
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1755
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

### long-reply-scroll @ 80x24 run 1 step 8 — MISMATCH (submit)

op: keys[enter]
settled: ref=true our=true | mismatches: 1920 (char 1214 / style 1920), text-only 971
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Long reply line 1 of the pari +`
our row 0: `   Untitled session              +`
ref row 1: `     Long reply line 20 of the parity scroll fixture.`
our row 1: `     Long reply line 10 of the parity scroll fixture. Long reply line 11 of`
ref row 2: `     Long reply line 21 of the parity scroll fixture.`
our row 2: `     the parity scroll fixture. Long reply line 12 of the parity scroll`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=17 our text-rows=18
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'L'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'o'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'n'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'g'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) ' '→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### long-reply-scroll @ 80x24 run 1 step 9 — MISMATCH (settled)

op: wait
settled: ref=true our=true | mismatches: 1920 (char 1214 / style 1920), text-only 971
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Long reply line 1 of the pari +`
our row 0: `   Untitled session              +`
ref row 1: `     Long reply line 20 of the parity scroll fixture.`
our row 1: `     Long reply line 10 of the parity scroll fixture. Long reply line 11 of`
ref row 2: `     Long reply line 21 of the parity scroll fixture.`
our row 2: `     the parity scroll fixture. Long reply line 12 of the parity scroll`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=17 our text-rows=18
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'L'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'o'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'n'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'g'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) ' '→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### long-reply-scroll @ 80x24 run 1 step 10 — MISMATCH (pageup)

op: keys[pageup]
settled: ref=true our=true | mismatches: 1920 (char 1212 / style 1920), text-only 928
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Long reply line 1 of the pari +`
our row 0: `   Untitled session              +`
ref row 1: `     Long reply line 13 of the parity scroll fixture.`
our row 1: `     + Thought`
ref row 2: `     Long reply line 14 of the parity scroll fixture.`
our row 2: ``
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=19 our text-rows=18
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'L'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'o'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'n'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'g'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) ' '→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### long-reply-scroll @ 80x24 run 1 step 11 — MISMATCH (pageup-again)

op: keys[pageup]
settled: ref=true our=true | mismatches: 1920 (char 1079 / style 1920), text-only 917
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Long reply line 1 of the pari +`
our row 0: `   Untitled session              +`
ref row 1: `     Long reply line 6 of the parity scroll fixture.`
our row 1: ``
ref row 2: `     Long reply line 7 of the parity scroll fixture.`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=19 our text-rows=14
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'L'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'o'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'n'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'g'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) ' '→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### long-reply-scroll @ 80x24 run 1 step 12 — MISMATCH (pagedown)

op: keys[pagedown]
settled: ref=true our=true | mismatches: 1920 (char 1280 / style 1920), text-only 936
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Long reply line 1 of the pari +`
our row 0: `   Untitled session              +`
ref row 1: `     Long reply line 14 of the parity scroll fixture.`
our row 1: `     parity scroll fixture. Long reply line 3 of the parity scroll fixture.`
ref row 2: `     Long reply line 15 of the parity scroll fixture.`
our row 2: `     Long reply line 4 of the parity scroll fixture. Long reply line 5 of the`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=19 our text-rows=19
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'L'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'o'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'n'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'g'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) ' '→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### long-reply-scroll @ 80x24 run 1 step 13 — MISMATCH (home)

op: keys[home]
settled: ref=true our=true | mismatches: 1920 (char 741 / style 1920), text-only 1239
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Long reply line 1 of the pari +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=14 our text-rows=14
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'L'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'o'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'n'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'g'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) ' '→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### long-reply-scroll @ 80x24 run 1 step 14 — MISMATCH (end)

op: keys[end]
settled: ref=true our=true | mismatches: 1920 (char 1214 / style 1920), text-only 970
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Long reply line 1 of the pari +`
our row 0: `   Untitled session              +`
ref row 1: `     Long reply line 20 of the parity scroll fixture.`
our row 1: `     Long reply line 10 of the parity scroll fixture. Long reply line 11 of`
ref row 2: `     Long reply line 21 of the parity scroll fixture.`
our row 2: `     the parity scroll fixture. Long reply line 12 of the parity scroll`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=17 our text-rows=18
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'L'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'o'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'n'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'g'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) ' '→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### long-reply-scroll @ 80x24 run 1 step 15 — MISMATCH (exit)

op: keys[leader,q]
settled: ref=true our=true | mismatches: 1920 (char 1214 / style 1920), text-only 971
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Long reply line 1 of the pari +`
our row 0: `   Untitled session              +`
ref row 1: `     Long reply line 20 of the parity scroll fixture.`
our row 1: `     Long reply line 10 of the parity scroll fixture. Long reply line 11 of`
ref row 2: `     Long reply line 21 of the parity scroll fixture.`
our row 2: `     the parity scroll fixture. Long reply line 12 of the parity scroll`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=17 our text-rows=18
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'L'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'o'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'n'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'g'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) ' '→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### long-reply-scroll @ 120x30 run 0 step 0 — MISMATCH (boot)

op: wait
settled: ref=true our=true | mismatches: 3600 (char 102 / style 3600), text-only 3380
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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

### long-reply-scroll @ 120x30 run 0 step 1 — MISMATCH (dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 3600 (char 102 / style 3600), text-only 3380
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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

### long-reply-scroll @ 120x30 run 0 step 2 — MISMATCH (models-cmd)

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

### long-reply-scroll @ 120x30 run 0 step 3 — MISMATCH (models-open)

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

### long-reply-scroll @ 120x30 run 0 step 4 — MISMATCH (models-filter)

op: text("fake")
settled: ref=true our=true | mismatches: 3600 (char 362 / style 3600), text-only 3206
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

### long-reply-scroll @ 120x30 run 0 step 5 — MISMATCH (models-select)

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

### long-reply-scroll @ 120x30 run 0 step 6 — MISMATCH (variant-dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 3600 (char 102 / style 3600), text-only 3380
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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

### long-reply-scroll @ 120x30 run 0 step 7 — MISMATCH (type)

op: text("Give me the long fixture")
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

### long-reply-scroll @ 120x30 run 0 step 8 — MISMATCH (submit)

op: keys[enter]
settled: ref=true our=true | mismatches: 3600 (char 1901 / style 3600), text-only 1792
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Long reply line 1 of the pari +`
our row 0: `   Untitled session              +`
ref row 1: `     Long reply line 14 of the parity scroll fixture.`
our row 1: ``
ref row 2: `     Long reply line 15 of the parity scroll fixture.`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=23 our text-rows=18
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'L'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'o'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'n'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'g'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) ' '→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### long-reply-scroll @ 120x30 run 0 step 9 — MISMATCH (settled)

op: wait
settled: ref=true our=true | mismatches: 3600 (char 1901 / style 3600), text-only 1792
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Long reply line 1 of the pari +`
our row 0: `   Untitled session              +`
ref row 1: `     Long reply line 14 of the parity scroll fixture.`
our row 1: ``
ref row 2: `     Long reply line 15 of the parity scroll fixture.`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=23 our text-rows=18
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'L'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'o'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'n'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'g'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) ' '→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### long-reply-scroll @ 120x30 run 0 step 10 — MISMATCH (pageup)

op: keys[pageup]
settled: ref=true our=true | mismatches: 3600 (char 1912 / style 3600), text-only 1843
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Long reply line 1 of the pari +`
our row 0: `   Untitled session              +`
ref row 1: `     Long reply line 4 of the parity scroll fixture.`
our row 1: ``
ref row 2: `     Long reply line 5 of the parity scroll fixture.`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=25 our text-rows=18
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'L'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'o'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'n'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'g'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) ' '→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### long-reply-scroll @ 120x30 run 0 step 11 — MISMATCH (pageup-again)

op: keys[pageup]
settled: ref=true our=true | mismatches: 3600 (char 1572 / style 3600), text-only 2168
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Long reply line 1 of the pari +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=20 our text-rows=18
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'L'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'o'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'n'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'g'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) ' '→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### long-reply-scroll @ 120x30 run 0 step 12 — MISMATCH (pagedown)

op: keys[pagedown]
settled: ref=true our=true | mismatches: 3600 (char 1913 / style 3600), text-only 1842
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Long reply line 1 of the pari +`
our row 0: `   Untitled session              +`
ref row 1: `     Long reply line 5 of the parity scroll fixture.`
our row 1: ``
ref row 2: `     Long reply line 6 of the parity scroll fixture.`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=25 our text-rows=18
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'L'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'o'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'n'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'g'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) ' '→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### long-reply-scroll @ 120x30 run 0 step 13 — MISMATCH (home)

op: keys[home]
settled: ref=true our=true | mismatches: 3600 (char 1572 / style 3600), text-only 2169
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Long reply line 1 of the pari +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=20 our text-rows=18
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'L'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'o'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'n'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'g'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) ' '→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### long-reply-scroll @ 120x30 run 0 step 14 — MISMATCH (end)

op: keys[end]
settled: ref=true our=true | mismatches: 3600 (char 1901 / style 3600), text-only 1793
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Long reply line 1 of the pari +`
our row 0: `   Untitled session              +`
ref row 1: `     Long reply line 14 of the parity scroll fixture.`
our row 1: ``
ref row 2: `     Long reply line 15 of the parity scroll fixture.`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=23 our text-rows=18
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'L'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'o'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'n'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'g'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) ' '→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### long-reply-scroll @ 120x30 run 0 step 15 — MISMATCH (exit)

op: keys[leader,q]
settled: ref=true our=true | mismatches: 3600 (char 1901 / style 3600), text-only 1792
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Long reply line 1 of the pari +`
our row 0: `   Untitled session              +`
ref row 1: `     Long reply line 14 of the parity scroll fixture.`
our row 1: ``
ref row 2: `     Long reply line 15 of the parity scroll fixture.`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=23 our text-rows=18
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'L'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'o'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'n'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'g'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) ' '→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### long-reply-scroll @ 120x30 run 1 step 0 — MISMATCH (boot)

op: wait
settled: ref=true our=true | mismatches: 3600 (char 102 / style 3600), text-only 3380
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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

### long-reply-scroll @ 120x30 run 1 step 1 — MISMATCH (dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 3600 (char 102 / style 3600), text-only 3380
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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

### long-reply-scroll @ 120x30 run 1 step 2 — MISMATCH (models-cmd)

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

### long-reply-scroll @ 120x30 run 1 step 3 — MISMATCH (models-open)

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

### long-reply-scroll @ 120x30 run 1 step 4 — MISMATCH (models-filter)

op: text("fake")
settled: ref=true our=true | mismatches: 3600 (char 362 / style 3600), text-only 3206
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

### long-reply-scroll @ 120x30 run 1 step 5 — MISMATCH (models-select)

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

### long-reply-scroll @ 120x30 run 1 step 6 — MISMATCH (variant-dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 3600 (char 102 / style 3600), text-only 3380
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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

### long-reply-scroll @ 120x30 run 1 step 7 — MISMATCH (type)

op: text("Give me the long fixture")
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

### long-reply-scroll @ 120x30 run 1 step 8 — MISMATCH (submit)

op: keys[enter]
settled: ref=true our=true | mismatches: 3600 (char 1901 / style 3600), text-only 1793
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Long reply line 1 of the pari +`
our row 0: `   Untitled session              +`
ref row 1: `     Long reply line 14 of the parity scroll fixture.`
our row 1: ``
ref row 2: `     Long reply line 15 of the parity scroll fixture.`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=23 our text-rows=18
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'L'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'o'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'n'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'g'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) ' '→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### long-reply-scroll @ 120x30 run 1 step 9 — MISMATCH (settled)

op: wait
settled: ref=true our=true | mismatches: 3600 (char 1901 / style 3600), text-only 1793
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Long reply line 1 of the pari +`
our row 0: `   Untitled session              +`
ref row 1: `     Long reply line 14 of the parity scroll fixture.`
our row 1: ``
ref row 2: `     Long reply line 15 of the parity scroll fixture.`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=23 our text-rows=18
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'L'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'o'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'n'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'g'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) ' '→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### long-reply-scroll @ 120x30 run 1 step 10 — MISMATCH (pageup)

op: keys[pageup]
settled: ref=true our=true | mismatches: 3600 (char 1912 / style 3600), text-only 1844
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Long reply line 1 of the pari +`
our row 0: `   Untitled session              +`
ref row 1: `     Long reply line 4 of the parity scroll fixture.`
our row 1: ``
ref row 2: `     Long reply line 5 of the parity scroll fixture.`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=25 our text-rows=18
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'L'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'o'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'n'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'g'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) ' '→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### long-reply-scroll @ 120x30 run 1 step 11 — MISMATCH (pageup-again)

op: keys[pageup]
settled: ref=true our=true | mismatches: 3600 (char 1572 / style 3600), text-only 2169
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Long reply line 1 of the pari +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=20 our text-rows=18
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'L'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'o'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'n'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'g'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) ' '→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### long-reply-scroll @ 120x30 run 1 step 12 — MISMATCH (pagedown)

op: keys[pagedown]
settled: ref=true our=true | mismatches: 3600 (char 1913 / style 3600), text-only 1842
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Long reply line 1 of the pari +`
our row 0: `   Untitled session              +`
ref row 1: `     Long reply line 5 of the parity scroll fixture.`
our row 1: ``
ref row 2: `     Long reply line 6 of the parity scroll fixture.`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=25 our text-rows=18
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'L'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'o'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'n'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'g'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) ' '→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### long-reply-scroll @ 120x30 run 1 step 13 — MISMATCH (home)

op: keys[home]
settled: ref=true our=true | mismatches: 3600 (char 1572 / style 3600), text-only 2168
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Long reply line 1 of the pari +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=20 our text-rows=18
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'L'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'o'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'n'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'g'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) ' '→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### long-reply-scroll @ 120x30 run 1 step 14 — MISMATCH (end)

op: keys[end]
settled: ref=true our=true | mismatches: 3600 (char 1901 / style 3600), text-only 1792
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Long reply line 1 of the pari +`
our row 0: `   Untitled session              +`
ref row 1: `     Long reply line 14 of the parity scroll fixture.`
our row 1: ``
ref row 2: `     Long reply line 15 of the parity scroll fixture.`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=23 our text-rows=18
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'L'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'o'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'n'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'g'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) ' '→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### long-reply-scroll @ 120x30 run 1 step 15 — MISMATCH (exit)

op: keys[leader,q]
settled: ref=true our=true | mismatches: 3600 (char 1901 / style 3600), text-only 1793
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Long reply line 1 of the pari +`
our row 0: `   Untitled session              +`
ref row 1: `     Long reply line 14 of the parity scroll fixture.`
our row 1: ``
ref row 2: `     Long reply line 15 of the parity scroll fixture.`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=23 our text-rows=18
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'L'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'o'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'n'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'g'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) ' '→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### mouse-hover-tabstrip @ 80x24 run 0 step 0 — MISMATCH (boot)

op: wait
settled: ref=true our=true | mismatches: 1920 (char 84 / style 1920), text-only 1713
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
settled: ref=true our=true | mismatches: 1920 (char 84 / style 1920), text-only 1713
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
settled: ref=true our=true | mismatches: 1920 (char 84 / style 1920), text-only 1713
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

### mouse-hover-tabstrip @ 80x24 run 0 step 10 — MISMATCH (hover-tab)

op: mouse(motion @4,1)
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

### mouse-hover-tabstrip @ 80x24 run 0 step 11 — MISMATCH (hover-plus)

op: mouse(motion @76,1)
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

### mouse-hover-tabstrip @ 80x24 run 0 step 12 — MISMATCH (hover-close)

op: mouse(motion @70,1)
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

### mouse-hover-tabstrip @ 80x24 run 0 step 13 — MISMATCH (hover-off)

op: mouse(motion @40,12)
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

### mouse-hover-tabstrip @ 80x24 run 0 step 14 — MISMATCH (exit)

op: keys[leader,q]
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
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1754
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
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1755
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

### mouse-hover-tabstrip @ 80x24 run 1 step 9 — MISMATCH (reply-settled)

op: wait
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

### mouse-hover-tabstrip @ 80x24 run 1 step 10 — MISMATCH (hover-tab)

op: mouse(motion @4,1)
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

### mouse-hover-tabstrip @ 80x24 run 1 step 11 — MISMATCH (hover-plus)

op: mouse(motion @76,1)
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

### mouse-hover-tabstrip @ 80x24 run 1 step 12 — MISMATCH (hover-close)

op: mouse(motion @70,1)
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

### mouse-hover-tabstrip @ 80x24 run 1 step 13 — MISMATCH (hover-off)

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

### mouse-hover-tabstrip @ 80x24 run 1 step 14 — MISMATCH (exit)

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
settled: ref=true our=true | mismatches: 3600 (char 84 / style 3600), text-only 3393
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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
settled: ref=true our=true | mismatches: 3600 (char 84 / style 3600), text-only 3393
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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
settled: ref=true our=true | mismatches: 3600 (char 84 / style 3600), text-only 3393
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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

### mouse-hover-tabstrip @ 120x30 run 0 step 8 — MISMATCH (submit)

op: keys[enter]
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

### mouse-hover-tabstrip @ 120x30 run 0 step 9 — MISMATCH (reply-settled)

op: wait
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

### mouse-hover-tabstrip @ 120x30 run 0 step 10 — MISMATCH (hover-tab)

op: mouse(motion @4,1)
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

### mouse-hover-tabstrip @ 120x30 run 0 step 11 — MISMATCH (hover-plus)

op: mouse(motion @76,1)
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

### mouse-hover-tabstrip @ 120x30 run 0 step 12 — MISMATCH (hover-close)

op: mouse(motion @70,1)
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

### mouse-hover-tabstrip @ 120x30 run 0 step 13 — MISMATCH (hover-off)

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

### mouse-hover-tabstrip @ 120x30 run 0 step 14 — MISMATCH (exit)

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
settled: ref=true our=true | mismatches: 3600 (char 84 / style 3600), text-only 3393
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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
settled: ref=true our=true | mismatches: 3600 (char 84 / style 3600), text-only 3393
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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
settled: ref=true our=true | mismatches: 3600 (char 84 / style 3600), text-only 3393
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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

### mouse-hover-tabstrip @ 120x30 run 1 step 10 — MISMATCH (hover-tab)

op: mouse(motion @4,1)
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

### mouse-hover-tabstrip @ 120x30 run 1 step 11 — MISMATCH (hover-plus)

op: mouse(motion @76,1)
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

### mouse-hover-tabstrip @ 120x30 run 1 step 12 — MISMATCH (hover-close)

op: mouse(motion @70,1)
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

### mouse-hover-tabstrip @ 120x30 run 1 step 13 — MISMATCH (hover-off)

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

### mouse-hover-tabstrip @ 120x30 run 1 step 14 — MISMATCH (exit)

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

### palette @ 80x24 run 0 step 0 — MISMATCH (boot)

op: wait
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1713
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

### palette @ 80x24 run 0 step 1 — MISMATCH (dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1713
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

### palette @ 80x24 run 0 step 2 — MISMATCH (palette-open)

op: keys[ctrl+p]
settled: ref=true our=true | mismatches: 1920 (char 71 / style 1920), text-only 1800
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 13 (masked-text runs/margins differ)
structure: ref text-rows=10 our text-rows=9
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]

### palette @ 80x24 run 0 step 3 — MISMATCH (palette-down)

op: keys[down,down]
settled: ref=true our=true | mismatches: 1920 (char 71 / style 1920), text-only 1792
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 13 (masked-text runs/margins differ)
structure: ref text-rows=10 our text-rows=9
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]

### palette @ 80x24 run 0 step 4 — MISMATCH (palette-close)

op: keys[esc]
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1713
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

### palette @ 80x24 run 0 step 5 — MISMATCH (exit)

op: keys[leader,q]
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1713
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

### palette @ 80x24 run 1 step 0 — MISMATCH (boot)

op: wait
settled: ref=true our=true | mismatches: 1920 (char 84 / style 1920), text-only 1713
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

### palette @ 80x24 run 1 step 1 — MISMATCH (dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 1920 (char 84 / style 1920), text-only 1713
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

### palette @ 80x24 run 1 step 2 — MISMATCH (palette-open)

op: keys[ctrl+p]
settled: ref=true our=true | mismatches: 1920 (char 71 / style 1920), text-only 1800
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 13 (masked-text runs/margins differ)
structure: ref text-rows=10 our text-rows=9
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]

### palette @ 80x24 run 1 step 3 — MISMATCH (palette-down)

op: keys[down,down]
settled: ref=true our=true | mismatches: 1920 (char 71 / style 1920), text-only 1792
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 13 (masked-text runs/margins differ)
structure: ref text-rows=10 our text-rows=9
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]

### palette @ 80x24 run 1 step 4 — MISMATCH (palette-close)

op: keys[esc]
settled: ref=true our=true | mismatches: 1920 (char 84 / style 1920), text-only 1713
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

### palette @ 80x24 run 1 step 5 — MISMATCH (exit)

op: keys[leader,q]
settled: ref=true our=true | mismatches: 1920 (char 84 / style 1920), text-only 1713
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

### palette @ 120x30 run 0 step 0 — MISMATCH (boot)

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

### palette @ 120x30 run 0 step 1 — MISMATCH (dismiss)

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

### palette @ 120x30 run 0 step 2 — MISMATCH (palette-open)

op: keys[ctrl+p]
settled: ref=true our=true | mismatches: 3600 (char 222 / style 3600), text-only 3346
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 8 (masked-text runs/margins differ)
structure: ref text-rows=11 our text-rows=11
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]

### palette @ 120x30 run 0 step 3 — MISMATCH (palette-down)

op: keys[down,down]
settled: ref=true our=true | mismatches: 3600 (char 222 / style 3600), text-only 3356
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 8 (masked-text runs/margins differ)
structure: ref text-rows=11 our text-rows=11
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]

### palette @ 120x30 run 0 step 4 — MISMATCH (palette-close)

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

### palette @ 120x30 run 0 step 5 — MISMATCH (exit)

op: keys[leader,q]
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

### palette @ 120x30 run 1 step 0 — MISMATCH (boot)

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

### palette @ 120x30 run 1 step 1 — MISMATCH (dismiss)

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

### palette @ 120x30 run 1 step 2 — MISMATCH (palette-open)

op: keys[ctrl+p]
settled: ref=true our=true | mismatches: 3600 (char 222 / style 3600), text-only 3346
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 8 (masked-text runs/margins differ)
structure: ref text-rows=11 our text-rows=11
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]

### palette @ 120x30 run 1 step 3 — MISMATCH (palette-down)

op: keys[down,down]
settled: ref=true our=true | mismatches: 3600 (char 222 / style 3600), text-only 3356
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 8 (masked-text runs/margins differ)
structure: ref text-rows=11 our text-rows=11
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]

### palette @ 120x30 run 1 step 4 — MISMATCH (palette-close)

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

### palette @ 120x30 run 1 step 5 — MISMATCH (exit)

op: keys[leader,q]
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

### rename-dialog @ 80x24 run 0 step 0 — MISMATCH (boot)

op: wait
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1713
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

### rename-dialog @ 80x24 run 0 step 1 — MISMATCH (dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1713
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

### rename-dialog @ 80x24 run 0 step 2 — MISMATCH (models-cmd)

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

### rename-dialog @ 80x24 run 0 step 3 — MISMATCH (models-open)

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

### rename-dialog @ 80x24 run 0 step 4 — MISMATCH (models-filter)

op: text("fake")
settled: ref=true our=true | mismatches: 1920 (char 273 / style 1920), text-only 1607
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 11 (masked-text runs/margins differ)
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

### rename-dialog @ 80x24 run 0 step 5 — MISMATCH (models-select)

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

### rename-dialog @ 80x24 run 0 step 6 — MISMATCH (variant-dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1713
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

### rename-dialog @ 80x24 run 0 step 7 — MISMATCH (type)

op: text("Rename me")
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1755
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

### rename-dialog @ 80x24 run 0 step 8 — MISMATCH (submit)

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

### rename-dialog @ 80x24 run 0 step 9 — MISMATCH (reply-settled)

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

### rename-dialog @ 80x24 run 0 step 10 — MISMATCH (rename-open)

op: keys[ctrl+r]
settled: ref=true our=true | mismatches: 1920 (char 304 / style 1920), text-only 1597
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0C0C0C]
ref row 0: `   Parity fake says hello.       +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=9 our text-rows=6
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0C0C0C]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0C0C0C]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0C0C0C]
  (3,0) 'P'→'U' ref[fg=idx241 bg=idx232 [bold]] our[fg=#EEEEEE bg=#0C0C0C [bold]]
  (4,0) 'a'→'n' ref[fg=idx241 bg=idx232 [bold]] our[fg=#EEEEEE bg=#0C0C0C [bold]]
  (5,0) 'r'→'t' ref[fg=idx241 bg=idx232 [bold]] our[fg=#EEEEEE bg=#0C0C0C [bold]]
  (6,0) 'i'→'i' ref[fg=idx241 bg=idx232 [bold]] our[fg=#EEEEEE bg=#0C0C0C [bold]]
  (7,0) 't'→'t' ref[fg=idx241 bg=idx232 [bold]] our[fg=#EEEEEE bg=#0C0C0C [bold]]

### rename-dialog @ 80x24 run 0 step 11 — MISMATCH (rename-type)

op: text("renamed tab")
settled: ref=true our=true | mismatches: 1920 (char 313 / style 1920), text-only 1585
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0C0C0C]
ref row 0: `   Parity fake says hello.       +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=9 our text-rows=6
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0C0C0C]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0C0C0C]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0C0C0C]
  (3,0) 'P'→'U' ref[fg=idx241 bg=idx232 [bold]] our[fg=#EEEEEE bg=#0C0C0C [bold]]
  (4,0) 'a'→'n' ref[fg=idx241 bg=idx232 [bold]] our[fg=#EEEEEE bg=#0C0C0C [bold]]
  (5,0) 'r'→'t' ref[fg=idx241 bg=idx232 [bold]] our[fg=#EEEEEE bg=#0C0C0C [bold]]
  (6,0) 'i'→'i' ref[fg=idx241 bg=idx232 [bold]] our[fg=#EEEEEE bg=#0C0C0C [bold]]
  (7,0) 't'→'t' ref[fg=idx241 bg=idx232 [bold]] our[fg=#EEEEEE bg=#0C0C0C [bold]]

### rename-dialog @ 80x24 run 0 step 12 — MISMATCH (rename-submit)

op: keys[enter]
settled: ref=true our=true | mismatches: 1920 (char 287 / style 1920), text-only 1622
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.rename +`
our row 0: `   renamed tab                   +`
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
  (3,0) 'P'→'r' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'e' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'a' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'m' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### rename-dialog @ 80x24 run 0 step 13 — MISMATCH (renamed)

op: wait
settled: ref=true our=true | mismatches: 1920 (char 287 / style 1920), text-only 1623
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.rename +`
our row 0: `   renamed tab                   +`
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
  (3,0) 'P'→'r' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'e' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'a' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'m' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### rename-dialog @ 80x24 run 0 step 14 — MISMATCH (exit)

op: keys[leader,q]
settled: ref=true our=true | mismatches: 1920 (char 287 / style 1920), text-only 1623
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.rename +`
our row 0: `   renamed tab                   +`
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
  (3,0) 'P'→'r' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'e' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'a' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'m' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### rename-dialog @ 80x24 run 1 step 0 — MISMATCH (boot)

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

### rename-dialog @ 80x24 run 1 step 1 — MISMATCH (dismiss)

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

### rename-dialog @ 80x24 run 1 step 2 — MISMATCH (models-cmd)

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

### rename-dialog @ 80x24 run 1 step 3 — MISMATCH (models-open)

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

### rename-dialog @ 80x24 run 1 step 4 — MISMATCH (models-filter)

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

### rename-dialog @ 80x24 run 1 step 5 — MISMATCH (models-select)

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

### rename-dialog @ 80x24 run 1 step 6 — MISMATCH (variant-dismiss)

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

### rename-dialog @ 80x24 run 1 step 7 — MISMATCH (type)

op: text("Rename me")
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

### rename-dialog @ 80x24 run 1 step 8 — MISMATCH (submit)

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

### rename-dialog @ 80x24 run 1 step 9 — MISMATCH (reply-settled)

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

### rename-dialog @ 80x24 run 1 step 10 — MISMATCH (rename-open)

op: keys[ctrl+r]
settled: ref=true our=true | mismatches: 1920 (char 304 / style 1920), text-only 1597
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0C0C0C]
ref row 0: `   Parity fake says hello.       +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=9 our text-rows=6
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0C0C0C]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0C0C0C]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0C0C0C]
  (3,0) 'P'→'U' ref[fg=idx241 bg=idx232 [bold]] our[fg=#EEEEEE bg=#0C0C0C [bold]]
  (4,0) 'a'→'n' ref[fg=idx241 bg=idx232 [bold]] our[fg=#EEEEEE bg=#0C0C0C [bold]]
  (5,0) 'r'→'t' ref[fg=idx241 bg=idx232 [bold]] our[fg=#EEEEEE bg=#0C0C0C [bold]]
  (6,0) 'i'→'i' ref[fg=idx241 bg=idx232 [bold]] our[fg=#EEEEEE bg=#0C0C0C [bold]]
  (7,0) 't'→'t' ref[fg=idx241 bg=idx232 [bold]] our[fg=#EEEEEE bg=#0C0C0C [bold]]

### rename-dialog @ 80x24 run 1 step 11 — MISMATCH (rename-type)

op: text("renamed tab")
settled: ref=true our=true | mismatches: 1920 (char 313 / style 1920), text-only 1585
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0C0C0C]
ref row 0: `   Parity fake says hello.       +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=9 our text-rows=6
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0C0C0C]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0C0C0C]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0C0C0C]
  (3,0) 'P'→'U' ref[fg=idx241 bg=idx232 [bold]] our[fg=#EEEEEE bg=#0C0C0C [bold]]
  (4,0) 'a'→'n' ref[fg=idx241 bg=idx232 [bold]] our[fg=#EEEEEE bg=#0C0C0C [bold]]
  (5,0) 'r'→'t' ref[fg=idx241 bg=idx232 [bold]] our[fg=#EEEEEE bg=#0C0C0C [bold]]
  (6,0) 'i'→'i' ref[fg=idx241 bg=idx232 [bold]] our[fg=#EEEEEE bg=#0C0C0C [bold]]
  (7,0) 't'→'t' ref[fg=idx241 bg=idx232 [bold]] our[fg=#EEEEEE bg=#0C0C0C [bold]]

### rename-dialog @ 80x24 run 1 step 12 — MISMATCH (rename-submit)

op: keys[enter]
settled: ref=true our=true | mismatches: 1920 (char 287 / style 1920), text-only 1622
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.rename +`
our row 0: `   renamed tab                   +`
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
  (3,0) 'P'→'r' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'e' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'a' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'m' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### rename-dialog @ 80x24 run 1 step 13 — MISMATCH (renamed)

op: wait
settled: ref=true our=true | mismatches: 1920 (char 287 / style 1920), text-only 1622
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.rename +`
our row 0: `   renamed tab                   +`
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
  (3,0) 'P'→'r' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'e' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'a' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'m' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### rename-dialog @ 80x24 run 1 step 14 — MISMATCH (exit)

op: keys[leader,q]
settled: ref=true our=true | mismatches: 1920 (char 287 / style 1920), text-only 1623
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.rename +`
our row 0: `   renamed tab                   +`
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
  (3,0) 'P'→'r' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'e' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'a' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'m' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### rename-dialog @ 120x30 run 0 step 0 — MISMATCH (boot)

op: wait
settled: ref=true our=true | mismatches: 3600 (char 102 / style 3600), text-only 3380
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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

### rename-dialog @ 120x30 run 0 step 1 — MISMATCH (dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 3600 (char 102 / style 3600), text-only 3380
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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

### rename-dialog @ 120x30 run 0 step 2 — MISMATCH (models-cmd)

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

### rename-dialog @ 120x30 run 0 step 3 — MISMATCH (models-open)

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

### rename-dialog @ 120x30 run 0 step 4 — MISMATCH (models-filter)

op: text("fake")
settled: ref=true our=true | mismatches: 3600 (char 362 / style 3600), text-only 3206
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

### rename-dialog @ 120x30 run 0 step 5 — MISMATCH (models-select)

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

### rename-dialog @ 120x30 run 0 step 6 — MISMATCH (variant-dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 3600 (char 102 / style 3600), text-only 3380
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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

### rename-dialog @ 120x30 run 0 step 7 — MISMATCH (type)

op: text("Rename me")
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

### rename-dialog @ 120x30 run 0 step 8 — MISMATCH (submit)

op: keys[enter]
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

### rename-dialog @ 120x30 run 0 step 9 — MISMATCH (reply-settled)

op: wait
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

### rename-dialog @ 120x30 run 0 step 10 — MISMATCH (rename-open)

op: keys[ctrl+r]
settled: ref=true our=true | mismatches: 3600 (char 421 / style 3600), text-only 3128
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0C0C0C]
ref row 0: `   Parity fake says hello.       +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=9 our text-rows=6
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0C0C0C]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0C0C0C]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0C0C0C]
  (3,0) 'P'→'U' ref[fg=idx241 bg=idx232 [bold]] our[fg=#EEEEEE bg=#0C0C0C [bold]]
  (4,0) 'a'→'n' ref[fg=idx241 bg=idx232 [bold]] our[fg=#EEEEEE bg=#0C0C0C [bold]]
  (5,0) 'r'→'t' ref[fg=idx241 bg=idx232 [bold]] our[fg=#EEEEEE bg=#0C0C0C [bold]]
  (6,0) 'i'→'i' ref[fg=idx241 bg=idx232 [bold]] our[fg=#EEEEEE bg=#0C0C0C [bold]]
  (7,0) 't'→'t' ref[fg=idx241 bg=idx232 [bold]] our[fg=#EEEEEE bg=#0C0C0C [bold]]

### rename-dialog @ 120x30 run 0 step 11 — MISMATCH (rename-type)

op: text("renamed tab")
settled: ref=true our=true | mismatches: 3600 (char 429 / style 3600), text-only 3117
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0C0C0C]
ref row 0: `   Parity fake says hello.       +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=9 our text-rows=6
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0C0C0C]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0C0C0C]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0C0C0C]
  (3,0) 'P'→'U' ref[fg=idx241 bg=idx232 [bold]] our[fg=#EEEEEE bg=#0C0C0C [bold]]
  (4,0) 'a'→'n' ref[fg=idx241 bg=idx232 [bold]] our[fg=#EEEEEE bg=#0C0C0C [bold]]
  (5,0) 'r'→'t' ref[fg=idx241 bg=idx232 [bold]] our[fg=#EEEEEE bg=#0C0C0C [bold]]
  (6,0) 'i'→'i' ref[fg=idx241 bg=idx232 [bold]] our[fg=#EEEEEE bg=#0C0C0C [bold]]
  (7,0) 't'→'t' ref[fg=idx241 bg=idx232 [bold]] our[fg=#EEEEEE bg=#0C0C0C [bold]]

### rename-dialog @ 120x30 run 0 step 12 — MISMATCH (rename-submit)

op: keys[enter]
settled: ref=true our=true | mismatches: 3600 (char 367 / style 3600), text-only 3222
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.rename +`
our row 0: `   renamed tab                   +`
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
  (3,0) 'P'→'r' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'e' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'a' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'m' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### rename-dialog @ 120x30 run 0 step 13 — MISMATCH (renamed)

op: wait
settled: ref=true our=true | mismatches: 3600 (char 367 / style 3600), text-only 3222
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.rename +`
our row 0: `   renamed tab                   +`
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
  (3,0) 'P'→'r' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'e' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'a' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'m' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### rename-dialog @ 120x30 run 0 step 14 — MISMATCH (exit)

op: keys[leader,q]
settled: ref=true our=true | mismatches: 3600 (char 367 / style 3600), text-only 3223
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.rename +`
our row 0: `   renamed tab                   +`
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
  (3,0) 'P'→'r' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'e' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'a' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'m' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### rename-dialog @ 120x30 run 1 step 0 — MISMATCH (boot)

op: wait
settled: ref=true our=true | mismatches: 3600 (char 84 / style 3600), text-only 3393
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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

### rename-dialog @ 120x30 run 1 step 1 — MISMATCH (dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 3600 (char 84 / style 3600), text-only 3393
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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

### rename-dialog @ 120x30 run 1 step 2 — MISMATCH (models-cmd)

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

### rename-dialog @ 120x30 run 1 step 3 — MISMATCH (models-open)

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

### rename-dialog @ 120x30 run 1 step 4 — MISMATCH (models-filter)

op: text("fake")
settled: ref=true our=true | mismatches: 3600 (char 362 / style 3600), text-only 3206
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

### rename-dialog @ 120x30 run 1 step 5 — MISMATCH (models-select)

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

### rename-dialog @ 120x30 run 1 step 6 — MISMATCH (variant-dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 3600 (char 84 / style 3600), text-only 3393
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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

### rename-dialog @ 120x30 run 1 step 7 — MISMATCH (type)

op: text("Rename me")
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

### rename-dialog @ 120x30 run 1 step 8 — MISMATCH (submit)

op: keys[enter]
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

### rename-dialog @ 120x30 run 1 step 9 — MISMATCH (reply-settled)

op: wait
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

### rename-dialog @ 120x30 run 1 step 10 — MISMATCH (rename-open)

op: keys[ctrl+r]
settled: ref=true our=true | mismatches: 3600 (char 421 / style 3600), text-only 3127
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0C0C0C]
ref row 0: `   Parity fake says hello.       +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=9 our text-rows=6
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0C0C0C]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0C0C0C]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0C0C0C]
  (3,0) 'P'→'U' ref[fg=idx241 bg=idx232 [bold]] our[fg=#EEEEEE bg=#0C0C0C [bold]]
  (4,0) 'a'→'n' ref[fg=idx241 bg=idx232 [bold]] our[fg=#EEEEEE bg=#0C0C0C [bold]]
  (5,0) 'r'→'t' ref[fg=idx241 bg=idx232 [bold]] our[fg=#EEEEEE bg=#0C0C0C [bold]]
  (6,0) 'i'→'i' ref[fg=idx241 bg=idx232 [bold]] our[fg=#EEEEEE bg=#0C0C0C [bold]]
  (7,0) 't'→'t' ref[fg=idx241 bg=idx232 [bold]] our[fg=#EEEEEE bg=#0C0C0C [bold]]

### rename-dialog @ 120x30 run 1 step 11 — MISMATCH (rename-type)

op: text("renamed tab")
settled: ref=true our=true | mismatches: 3600 (char 429 / style 3600), text-only 3118
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0C0C0C]
ref row 0: `   Parity fake says hello.       +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=9 our text-rows=6
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0C0C0C]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0C0C0C]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0C0C0C]
  (3,0) 'P'→'U' ref[fg=idx241 bg=idx232 [bold]] our[fg=#EEEEEE bg=#0C0C0C [bold]]
  (4,0) 'a'→'n' ref[fg=idx241 bg=idx232 [bold]] our[fg=#EEEEEE bg=#0C0C0C [bold]]
  (5,0) 'r'→'t' ref[fg=idx241 bg=idx232 [bold]] our[fg=#EEEEEE bg=#0C0C0C [bold]]
  (6,0) 'i'→'i' ref[fg=idx241 bg=idx232 [bold]] our[fg=#EEEEEE bg=#0C0C0C [bold]]
  (7,0) 't'→'t' ref[fg=idx241 bg=idx232 [bold]] our[fg=#EEEEEE bg=#0C0C0C [bold]]

### rename-dialog @ 120x30 run 1 step 12 — MISMATCH (rename-submit)

op: keys[enter]
settled: ref=true our=true | mismatches: 3600 (char 367 / style 3600), text-only 3223
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.rename +`
our row 0: `   renamed tab                   +`
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
  (3,0) 'P'→'r' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'e' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'a' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'m' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### rename-dialog @ 120x30 run 1 step 13 — MISMATCH (renamed)

op: wait
settled: ref=true our=true | mismatches: 3600 (char 367 / style 3600), text-only 3222
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.rename +`
our row 0: `   renamed tab                   +`
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
  (3,0) 'P'→'r' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'e' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'a' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'m' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### rename-dialog @ 120x30 run 1 step 14 — MISMATCH (exit)

op: keys[leader,q]
settled: ref=true our=true | mismatches: 3600 (char 367 / style 3600), text-only 3223
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.rename +`
our row 0: `   renamed tab                   +`
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
  (3,0) 'P'→'r' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'e' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'a' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'m' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### resize @ 80x24 run 0 step 0 — MISMATCH (boot)

op: wait
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1713
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

### resize @ 80x24 run 0 step 1 — MISMATCH (dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1713
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

### resize @ 80x24 run 0 step 2 — MISMATCH (grow)

op: resize(100x28)
settled: ref=true our=true | mismatches: 2800 (char 64 / style 2800), text-only 2593
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 17 (masked-text runs/margins differ)
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

### resize @ 80x24 run 0 step 3 — MISMATCH (shrink)

op: resize(60x20)
settled: ref=true our=true | mismatches: 1200 (char 345 / style 1200), text-only 772
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 11 (masked-text runs/margins differ)
structure: ref text-rows=3 our text-rows=3
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]

### resize @ 80x24 run 0 step 4 — MISMATCH (restore)

op: resize(80x24)
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1713
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

### resize @ 80x24 run 0 step 5 — MISMATCH (exit)

op: keys[leader,q]
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1713
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

### resize @ 80x24 run 1 step 0 — MISMATCH (boot)

op: wait
settled: ref=true our=true | mismatches: 1920 (char 84 / style 1920), text-only 1713
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

### resize @ 80x24 run 1 step 1 — MISMATCH (dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 1920 (char 84 / style 1920), text-only 1713
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

### resize @ 80x24 run 1 step 2 — MISMATCH (grow)

op: resize(100x28)
settled: ref=true our=true | mismatches: 2800 (char 84 / style 2800), text-only 2593
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

### resize @ 80x24 run 1 step 3 — MISMATCH (shrink)

op: resize(60x20)
settled: ref=true our=true | mismatches: 1200 (char 338 / style 1200), text-only 782
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 11 (masked-text runs/margins differ)
structure: ref text-rows=3 our text-rows=3
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]

### resize @ 80x24 run 1 step 4 — MISMATCH (restore)

op: resize(80x24)
settled: ref=true our=true | mismatches: 1920 (char 84 / style 1920), text-only 1713
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

### resize @ 80x24 run 1 step 5 — MISMATCH (exit)

op: keys[leader,q]
settled: ref=true our=true | mismatches: 1920 (char 84 / style 1920), text-only 1713
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

### resize @ 120x30 run 0 step 0 — MISMATCH (boot)

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

### resize @ 120x30 run 0 step 1 — MISMATCH (dismiss)

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

### resize @ 120x30 run 0 step 2 — MISMATCH (grow)

op: resize(100x28)
settled: ref=true our=true | mismatches: 2800 (char 64 / style 2800), text-only 2593
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 17 (masked-text runs/margins differ)
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

### resize @ 120x30 run 0 step 3 — MISMATCH (shrink)

op: resize(60x20)
settled: ref=true our=true | mismatches: 1200 (char 345 / style 1200), text-only 772
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 11 (masked-text runs/margins differ)
structure: ref text-rows=3 our text-rows=3
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]

### resize @ 120x30 run 0 step 4 — MISMATCH (restore)

op: resize(80x24)
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1713
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

### resize @ 120x30 run 0 step 5 — MISMATCH (exit)

op: keys[leader,q]
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1713
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

### resize @ 120x30 run 1 step 0 — MISMATCH (boot)

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

### resize @ 120x30 run 1 step 1 — MISMATCH (dismiss)

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

### resize @ 120x30 run 1 step 2 — MISMATCH (grow)

op: resize(100x28)
settled: ref=true our=true | mismatches: 2800 (char 521 / style 2800), text-only 0
first diff: (0, 0) ref=' ' our='o' styles ref[fg=idx15 bg=idx232] our[fg=default bg=default]
ref row 0: ``
our row 0: `opencode tui panicked: panicked at /home/eternal130/.cargo/registry/src/index.crates.io-1949cf8c6b5b`
ref row 1: ``
our row 1: `557f/ratatui-0.29.0/src/buffer/buffer.rs:253:13:`
ref row 2: ``
our row 2: `                                                index outside of buffer: the area is Rect { x: 0, y:`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=4 our text-rows=4
cell samples (x,y): ref→our
  (0,0) ' '→'o' ref[fg=idx15 bg=idx232] our[fg=default bg=default]
  (1,0) ' '→'p' ref[fg=idx15 bg=idx232] our[fg=default bg=default]
  (2,0) ' '→'e' ref[fg=idx15 bg=idx232] our[fg=default bg=default]
  (3,0) ' '→'n' ref[fg=idx15 bg=idx232] our[fg=default bg=default]
  (4,0) ' '→'c' ref[fg=idx15 bg=idx232] our[fg=default bg=default]
  (5,0) ' '→'o' ref[fg=idx15 bg=idx232] our[fg=default bg=default]
  (6,0) ' '→'d' ref[fg=idx15 bg=idx232] our[fg=default bg=default]
  (7,0) ' '→'e' ref[fg=idx15 bg=idx232] our[fg=default bg=default]

### resize @ 120x30 run 1 step 3 — MISMATCH (shrink)

op: resize(60x20)
settled: ref=true our=true | mismatches: 1200 (char 254 / style 1200), text-only 0
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=default bg=default]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 12 (masked-text runs/margins differ)
structure: ref text-rows=3 our text-rows=0
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=default bg=default]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=default bg=default]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=default bg=default]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=default bg=default]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=default bg=default]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=default bg=default]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=default bg=default]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=default bg=default]

### resize @ 120x30 run 1 step 4 — MISMATCH (restore)

op: resize(80x24)
settled: ref=true our=true | mismatches: 1920 (char 298 / style 1920), text-only 0
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=default bg=default]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 13 (masked-text runs/margins differ)
structure: ref text-rows=4 our text-rows=0
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=default bg=default]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=default bg=default]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=default bg=default]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=default bg=default]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=default bg=default]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=default bg=default]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=default bg=default]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=default bg=default]

### resize @ 120x30 run 1 step 5 — MISMATCH (exit)

op: keys[leader,q]
settled: ref=true our=true | mismatches: 1920 (char 298 / style 1920), text-only 0
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=default bg=default]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 13 (masked-text runs/margins differ)
structure: ref text-rows=4 our text-rows=0
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=default bg=default]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=default bg=default]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=default bg=default]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=default bg=default]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=default bg=default]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=default bg=default]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=default bg=default]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx232] our[fg=default bg=default]

### session-list-dialog @ 80x24 run 0 step 0 — MISMATCH (boot)

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

### session-list-dialog @ 80x24 run 0 step 1 — MISMATCH (dismiss)

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

### session-list-dialog @ 80x24 run 0 step 2 — MISMATCH (models-cmd)

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

### session-list-dialog @ 80x24 run 0 step 3 — MISMATCH (models-open)

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

### session-list-dialog @ 80x24 run 0 step 4 — MISMATCH (models-filter)

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

### session-list-dialog @ 80x24 run 0 step 5 — MISMATCH (models-select)

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

### session-list-dialog @ 80x24 run 0 step 6 — MISMATCH (variant-dismiss)

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

### session-list-dialog @ 80x24 run 0 step 7 — MISMATCH (type)

op: text("Create a session")
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

### session-list-dialog @ 80x24 run 0 step 8 — MISMATCH (submit)

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

### session-list-dialog @ 80x24 run 0 step 9 — MISMATCH (reply-settled)

op: wait
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

### session-list-dialog @ 80x24 run 0 step 10 — MISMATCH (open-list)

op: keys[leader,l]
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

### session-list-dialog @ 80x24 run 0 step 11 — MISMATCH (browse)

op: keys[down]
settled: ref=true our=true | mismatches: 1920 (char 251 / style 1920), text-only 1655
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.       +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=4
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### session-list-dialog @ 80x24 run 0 step 12 — MISMATCH (choose)

op: keys[enter]
settled: ref=true our=true | mismatches: 1920 (char 251 / style 1920), text-only 1655
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.       +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=4
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### session-list-dialog @ 80x24 run 0 step 13 — MISMATCH (exit)

op: keys[leader,q]
settled: ref=true our=true | mismatches: 1920 (char 251 / style 1920), text-only 1654
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.       +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=4
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### session-list-dialog @ 80x24 run 1 step 0 — MISMATCH (boot)

op: wait
settled: ref=true our=true | mismatches: 1920 (char 84 / style 1920), text-only 1713
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

### session-list-dialog @ 80x24 run 1 step 1 — MISMATCH (dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 1920 (char 84 / style 1920), text-only 1713
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

### session-list-dialog @ 80x24 run 1 step 2 — MISMATCH (models-cmd)

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

### session-list-dialog @ 80x24 run 1 step 3 — MISMATCH (models-open)

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

### session-list-dialog @ 80x24 run 1 step 4 — MISMATCH (models-filter)

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

### session-list-dialog @ 80x24 run 1 step 5 — MISMATCH (models-select)

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

### session-list-dialog @ 80x24 run 1 step 6 — MISMATCH (variant-dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 1920 (char 84 / style 1920), text-only 1713
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

### session-list-dialog @ 80x24 run 1 step 7 — MISMATCH (type)

op: text("Create a session")
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

### session-list-dialog @ 80x24 run 1 step 8 — MISMATCH (submit)

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

### session-list-dialog @ 80x24 run 1 step 9 — MISMATCH (reply-settled)

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

### session-list-dialog @ 80x24 run 1 step 10 — MISMATCH (open-list)

op: keys[leader,l]
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

### session-list-dialog @ 80x24 run 1 step 11 — MISMATCH (browse)

op: keys[down]
settled: ref=true our=true | mismatches: 1920 (char 251 / style 1920), text-only 1655
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.       +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=4
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### session-list-dialog @ 80x24 run 1 step 12 — MISMATCH (choose)

op: keys[enter]
settled: ref=true our=true | mismatches: 1920 (char 251 / style 1920), text-only 1655
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.       +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=4
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### session-list-dialog @ 80x24 run 1 step 13 — MISMATCH (exit)

op: keys[leader,q]
settled: ref=true our=true | mismatches: 1920 (char 251 / style 1920), text-only 1654
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.       +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=4
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### session-list-dialog @ 120x30 run 0 step 0 — MISMATCH (boot)

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

### session-list-dialog @ 120x30 run 0 step 1 — MISMATCH (dismiss)

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

### session-list-dialog @ 120x30 run 0 step 2 — MISMATCH (models-cmd)

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

### session-list-dialog @ 120x30 run 0 step 3 — MISMATCH (models-open)

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

### session-list-dialog @ 120x30 run 0 step 4 — MISMATCH (models-filter)

op: text("fake")
settled: ref=true our=true | mismatches: 3600 (char 359 / style 3600), text-only 3206
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

### session-list-dialog @ 120x30 run 0 step 5 — MISMATCH (models-select)

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

### session-list-dialog @ 120x30 run 0 step 6 — MISMATCH (variant-dismiss)

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

### session-list-dialog @ 120x30 run 0 step 7 — MISMATCH (type)

op: text("Create a session")
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

### session-list-dialog @ 120x30 run 0 step 8 — MISMATCH (submit)

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

### session-list-dialog @ 120x30 run 0 step 9 — MISMATCH (reply-settled)

op: wait
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

### session-list-dialog @ 120x30 run 0 step 10 — MISMATCH (open-list)

op: keys[leader,l]
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

### session-list-dialog @ 120x30 run 0 step 11 — MISMATCH (browse)

op: keys[down]
settled: ref=true our=true | mismatches: 3600 (char 291 / style 3600), text-only 3294
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.       +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=4
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### session-list-dialog @ 120x30 run 0 step 12 — MISMATCH (choose)

op: keys[enter]
settled: ref=true our=true | mismatches: 3600 (char 291 / style 3600), text-only 3294
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.       +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=4
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### session-list-dialog @ 120x30 run 0 step 13 — MISMATCH (exit)

op: keys[leader,q]
settled: ref=true our=true | mismatches: 3600 (char 291 / style 3600), text-only 3294
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.       +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=4
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### session-list-dialog @ 120x30 run 1 step 0 — MISMATCH (boot)

op: wait
settled: ref=true our=true | mismatches: 3600 (char 84 / style 3600), text-only 3393
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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

### session-list-dialog @ 120x30 run 1 step 1 — MISMATCH (dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 3600 (char 84 / style 3600), text-only 3393
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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

### session-list-dialog @ 120x30 run 1 step 2 — MISMATCH (models-cmd)

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

### session-list-dialog @ 120x30 run 1 step 3 — MISMATCH (models-open)

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

### session-list-dialog @ 120x30 run 1 step 4 — MISMATCH (models-filter)

op: text("fake")
settled: ref=true our=true | mismatches: 3600 (char 359 / style 3600), text-only 3206
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

### session-list-dialog @ 120x30 run 1 step 5 — MISMATCH (models-select)

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

### session-list-dialog @ 120x30 run 1 step 6 — MISMATCH (variant-dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 3600 (char 84 / style 3600), text-only 3393
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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

### session-list-dialog @ 120x30 run 1 step 7 — MISMATCH (type)

op: text("Create a session")
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

### session-list-dialog @ 120x30 run 1 step 8 — MISMATCH (submit)

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

### session-list-dialog @ 120x30 run 1 step 9 — MISMATCH (reply-settled)

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

### session-list-dialog @ 120x30 run 1 step 10 — MISMATCH (open-list)

op: keys[leader,l]
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

### session-list-dialog @ 120x30 run 1 step 11 — MISMATCH (browse)

op: keys[down]
settled: ref=true our=true | mismatches: 3600 (char 291 / style 3600), text-only 3295
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.       +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=4
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### session-list-dialog @ 120x30 run 1 step 12 — MISMATCH (choose)

op: keys[enter]
settled: ref=true our=true | mismatches: 3600 (char 291 / style 3600), text-only 3294
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.       +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=4
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### session-list-dialog @ 120x30 run 1 step 13 — MISMATCH (exit)

op: keys[leader,q]
settled: ref=true our=true | mismatches: 3600 (char 291 / style 3600), text-only 3295
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello.       +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=4
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### tabs-keyboard @ 80x24 run 0 step 0 — MISMATCH (boot)

op: wait
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1713
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

### tabs-keyboard @ 80x24 run 0 step 1 — MISMATCH (dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1713
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

### tabs-keyboard @ 80x24 run 0 step 2 — MISMATCH (tab-new-1)

op: keys[leader,n]
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1713
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

### tabs-keyboard @ 80x24 run 0 step 3 — MISMATCH (tab-new-2)

op: keys[leader,n]
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1713
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

### tabs-keyboard @ 80x24 run 0 step 4 — MISMATCH (tab-next)

op: keys[alt+down]
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1713
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

### tabs-keyboard @ 80x24 run 0 step 5 — MISMATCH (tab-prev)

op: keys[alt+up]
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1713
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

### tabs-keyboard @ 80x24 run 0 step 6 — MISMATCH (tab-close)

op: keys[leader,w]
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1713
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

### tabs-keyboard @ 80x24 run 0 step 7 — MISMATCH (exit)

op: keys[leader,q]
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1713
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

### tabs-keyboard @ 80x24 run 1 step 0 — MISMATCH (boot)

op: wait
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1713
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

### tabs-keyboard @ 80x24 run 1 step 1 — MISMATCH (dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1713
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

### tabs-keyboard @ 80x24 run 1 step 2 — MISMATCH (tab-new-1)

op: keys[leader,n]
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1713
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

### tabs-keyboard @ 80x24 run 1 step 3 — MISMATCH (tab-new-2)

op: keys[leader,n]
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1713
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

### tabs-keyboard @ 80x24 run 1 step 4 — MISMATCH (tab-next)

op: keys[alt+down]
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1713
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

### tabs-keyboard @ 80x24 run 1 step 5 — MISMATCH (tab-prev)

op: keys[alt+up]
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1713
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

### tabs-keyboard @ 80x24 run 1 step 6 — MISMATCH (tab-close)

op: keys[leader,w]
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1713
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

### tabs-keyboard @ 80x24 run 1 step 7 — MISMATCH (exit)

op: keys[leader,q]
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1713
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

### tabs-keyboard @ 120x30 run 0 step 0 — MISMATCH (boot)

op: wait
settled: ref=true our=true | mismatches: 3600 (char 102 / style 3600), text-only 3380
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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

### tabs-keyboard @ 120x30 run 0 step 1 — MISMATCH (dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 3600 (char 102 / style 3600), text-only 3380
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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

### tabs-keyboard @ 120x30 run 0 step 2 — MISMATCH (tab-new-1)

op: keys[leader,n]
settled: ref=true our=true | mismatches: 3600 (char 102 / style 3600), text-only 3380
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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

### tabs-keyboard @ 120x30 run 0 step 3 — MISMATCH (tab-new-2)

op: keys[leader,n]
settled: ref=true our=true | mismatches: 3600 (char 102 / style 3600), text-only 3380
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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

### tabs-keyboard @ 120x30 run 0 step 4 — MISMATCH (tab-next)

op: keys[alt+down]
settled: ref=true our=true | mismatches: 3600 (char 102 / style 3600), text-only 3380
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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

### tabs-keyboard @ 120x30 run 0 step 5 — MISMATCH (tab-prev)

op: keys[alt+up]
settled: ref=true our=true | mismatches: 3600 (char 102 / style 3600), text-only 3380
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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

### tabs-keyboard @ 120x30 run 0 step 6 — MISMATCH (tab-close)

op: keys[leader,w]
settled: ref=true our=true | mismatches: 3600 (char 102 / style 3600), text-only 3380
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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

### tabs-keyboard @ 120x30 run 0 step 7 — MISMATCH (exit)

op: keys[leader,q]
settled: ref=true our=true | mismatches: 3600 (char 102 / style 3600), text-only 3380
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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

### tabs-keyboard @ 120x30 run 1 step 0 — MISMATCH (boot)

op: wait
settled: ref=true our=true | mismatches: 3600 (char 84 / style 3600), text-only 3393
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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

### tabs-keyboard @ 120x30 run 1 step 1 — MISMATCH (dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 3600 (char 84 / style 3600), text-only 3393
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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

### tabs-keyboard @ 120x30 run 1 step 2 — MISMATCH (tab-new-1)

op: keys[leader,n]
settled: ref=true our=true | mismatches: 3600 (char 84 / style 3600), text-only 3393
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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

### tabs-keyboard @ 120x30 run 1 step 3 — MISMATCH (tab-new-2)

op: keys[leader,n]
settled: ref=true our=true | mismatches: 3600 (char 84 / style 3600), text-only 3393
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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

### tabs-keyboard @ 120x30 run 1 step 4 — MISMATCH (tab-next)

op: keys[alt+down]
settled: ref=true our=true | mismatches: 3600 (char 84 / style 3600), text-only 3393
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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

### tabs-keyboard @ 120x30 run 1 step 5 — MISMATCH (tab-prev)

op: keys[alt+up]
settled: ref=true our=true | mismatches: 3600 (char 84 / style 3600), text-only 3393
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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

### tabs-keyboard @ 120x30 run 1 step 6 — MISMATCH (tab-close)

op: keys[leader,w]
settled: ref=true our=true | mismatches: 3600 (char 84 / style 3600), text-only 3393
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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

### tabs-keyboard @ 120x30 run 1 step 7 — MISMATCH (exit)

op: keys[leader,q]
settled: ref=true our=true | mismatches: 3600 (char 84 / style 3600), text-only 3393
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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

### themes-nord @ 80x24 run 0 step 0 — MISMATCH (boot)

op: wait
settled: ref=true our=true | mismatches: 1920 (char 85 / style 1920), text-only 1713
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

### themes-nord @ 80x24 run 0 step 1 — MISMATCH (dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 1920 (char 85 / style 1920), text-only 1713
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

### themes-nord @ 80x24 run 0 step 2 — MISMATCH (slash)

op: text("/themes")
settled: ref=true our=true | mismatches: 1920 (char 65 / style 1920), text-only 1755
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

### themes-nord @ 80x24 run 0 step 3 — MISMATCH (submit)

op: keys[enter]
settled: ref=true our=true | mismatches: 1920 (char 63 / style 1920), text-only 1837
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 11 (masked-text runs/margins differ)
structure: ref text-rows=10 our text-rows=10
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]

### themes-nord @ 80x24 run 0 step 4 — MISMATCH (filter)

op: text("nord")
settled: ref=true our=true | mismatches: 1920 (char 75 / style 1920), text-only 1791
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx233] our[fg=#FFFFFF bg=#040404]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 11 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=7
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx233] our[fg=#FFFFFF bg=#040404]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx233] our[fg=#FFFFFF bg=#040404]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx233] our[fg=#FFFFFF bg=#040404]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx233] our[fg=#FFFFFF bg=#040404]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx233] our[fg=#FFFFFF bg=#040404]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx233] our[fg=#FFFFFF bg=#040404]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx233] our[fg=#FFFFFF bg=#040404]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx233] our[fg=#FFFFFF bg=#040404]

### themes-nord @ 80x24 run 0 step 5 — MISMATCH (apply)

op: keys[enter]
settled: ref=true our=true | mismatches: 1920 (char 85 / style 1920), text-only 1637
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 13 (masked-text runs/margins differ)
structure: ref text-rows=4 our text-rows=3
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]

### themes-nord @ 80x24 run 0 step 6 — MISMATCH (restyled)

op: wait
settled: ref=true our=true | mismatches: 1920 (char 85 / style 1920), text-only 1637
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 13 (masked-text runs/margins differ)
structure: ref text-rows=4 our text-rows=3
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]

### themes-nord @ 80x24 run 0 step 7 — MISMATCH (exit)

op: keys[leader,q]
settled: ref=true our=true | mismatches: 1920 (char 85 / style 1920), text-only 1637
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 13 (masked-text runs/margins differ)
structure: ref text-rows=4 our text-rows=3
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]

### themes-nord @ 80x24 run 1 step 0 — MISMATCH (boot)

op: wait
settled: ref=true our=true | mismatches: 1920 (char 103 / style 1920), text-only 1700
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

### themes-nord @ 80x24 run 1 step 1 — MISMATCH (dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 1920 (char 103 / style 1920), text-only 1700
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

### themes-nord @ 80x24 run 1 step 2 — MISMATCH (slash)

op: text("/themes")
settled: ref=true our=true | mismatches: 1920 (char 65 / style 1920), text-only 1754
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

### themes-nord @ 80x24 run 1 step 3 — MISMATCH (submit)

op: keys[enter]
settled: ref=true our=true | mismatches: 1920 (char 63 / style 1920), text-only 1837
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 11 (masked-text runs/margins differ)
structure: ref text-rows=10 our text-rows=10
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]

### themes-nord @ 80x24 run 1 step 4 — MISMATCH (filter)

op: text("nord")
settled: ref=true our=true | mismatches: 1920 (char 75 / style 1920), text-only 1791
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx233] our[fg=#FFFFFF bg=#040404]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 11 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=7
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx233] our[fg=#FFFFFF bg=#040404]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx233] our[fg=#FFFFFF bg=#040404]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx233] our[fg=#FFFFFF bg=#040404]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx233] our[fg=#FFFFFF bg=#040404]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx233] our[fg=#FFFFFF bg=#040404]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx233] our[fg=#FFFFFF bg=#040404]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx233] our[fg=#FFFFFF bg=#040404]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx233] our[fg=#FFFFFF bg=#040404]

### themes-nord @ 80x24 run 1 step 5 — MISMATCH (apply)

op: keys[enter]
settled: ref=true our=true | mismatches: 1920 (char 103 / style 1920), text-only 1624
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 13 (masked-text runs/margins differ)
structure: ref text-rows=4 our text-rows=3
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]

### themes-nord @ 80x24 run 1 step 6 — MISMATCH (restyled)

op: wait
settled: ref=true our=true | mismatches: 1920 (char 103 / style 1920), text-only 1624
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 13 (masked-text runs/margins differ)
structure: ref text-rows=4 our text-rows=3
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]

### themes-nord @ 80x24 run 1 step 7 — MISMATCH (exit)

op: keys[leader,q]
settled: ref=true our=true | mismatches: 1920 (char 103 / style 1920), text-only 1624
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 13 (masked-text runs/margins differ)
structure: ref text-rows=4 our text-rows=3
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]

### themes-nord @ 120x30 run 0 step 0 — MISMATCH (boot)

op: wait
settled: ref=true our=true | mismatches: 3600 (char 85 / style 3600), text-only 3393
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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

### themes-nord @ 120x30 run 0 step 1 — MISMATCH (dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 3600 (char 85 / style 3600), text-only 3393
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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

### themes-nord @ 120x30 run 0 step 2 — MISMATCH (slash)

op: text("/themes")
settled: ref=true our=true | mismatches: 3600 (char 65 / style 3600), text-only 3435
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

### themes-nord @ 120x30 run 0 step 3 — MISMATCH (submit)

op: keys[enter]
settled: ref=true our=true | mismatches: 3600 (char 119 / style 3600), text-only 3472
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 8 (masked-text runs/margins differ)
structure: ref text-rows=12 our text-rows=12
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]

### themes-nord @ 120x30 run 0 step 4 — MISMATCH (filter)

op: text("nord")
settled: ref=true our=true | mismatches: 3600 (char 117 / style 3600), text-only 3414
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx233] our[fg=#FFFFFF bg=#040404]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 8 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=7
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx233] our[fg=#FFFFFF bg=#040404]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx233] our[fg=#FFFFFF bg=#040404]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx233] our[fg=#FFFFFF bg=#040404]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx233] our[fg=#FFFFFF bg=#040404]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx233] our[fg=#FFFFFF bg=#040404]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx233] our[fg=#FFFFFF bg=#040404]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx233] our[fg=#FFFFFF bg=#040404]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx233] our[fg=#FFFFFF bg=#040404]

### themes-nord @ 120x30 run 0 step 5 — MISMATCH (apply)

op: keys[enter]
settled: ref=true our=true | mismatches: 3600 (char 85 / style 3600), text-only 3317
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
structure: ref text-rows=4 our text-rows=3
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]

### themes-nord @ 120x30 run 0 step 6 — MISMATCH (restyled)

op: wait
settled: ref=true our=true | mismatches: 3600 (char 85 / style 3600), text-only 3317
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
structure: ref text-rows=4 our text-rows=3
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]

### themes-nord @ 120x30 run 0 step 7 — MISMATCH (exit)

op: keys[leader,q]
settled: ref=true our=true | mismatches: 3600 (char 85 / style 3600), text-only 3317
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
structure: ref text-rows=4 our text-rows=3
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]

### themes-nord @ 120x30 run 1 step 0 — MISMATCH (boot)

op: wait
settled: ref=true our=true | mismatches: 3600 (char 85 / style 3600), text-only 3393
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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

### themes-nord @ 120x30 run 1 step 1 — MISMATCH (dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 3600 (char 85 / style 3600), text-only 3393
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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

### themes-nord @ 120x30 run 1 step 2 — MISMATCH (slash)

op: text("/themes")
settled: ref=true our=true | mismatches: 3600 (char 65 / style 3600), text-only 3435
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

### themes-nord @ 120x30 run 1 step 3 — MISMATCH (submit)

op: keys[enter]
settled: ref=true our=true | mismatches: 3600 (char 119 / style 3600), text-only 3472
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 8 (masked-text runs/margins differ)
structure: ref text-rows=12 our text-rows=12
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]

### themes-nord @ 120x30 run 1 step 4 — MISMATCH (filter)

op: text("nord")
settled: ref=true our=true | mismatches: 3600 (char 117 / style 3600), text-only 3414
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx233] our[fg=#FFFFFF bg=#040404]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 8 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=7
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx233] our[fg=#FFFFFF bg=#040404]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx233] our[fg=#FFFFFF bg=#040404]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx233] our[fg=#FFFFFF bg=#040404]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx233] our[fg=#FFFFFF bg=#040404]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx233] our[fg=#FFFFFF bg=#040404]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx233] our[fg=#FFFFFF bg=#040404]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx233] our[fg=#FFFFFF bg=#040404]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx233] our[fg=#FFFFFF bg=#040404]

### themes-nord @ 120x30 run 1 step 5 — MISMATCH (apply)

op: keys[enter]
settled: ref=true our=true | mismatches: 3600 (char 85 / style 3600), text-only 3317
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
structure: ref text-rows=4 our text-rows=3
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]

### themes-nord @ 120x30 run 1 step 6 — MISMATCH (restyled)

op: wait
settled: ref=true our=true | mismatches: 3600 (char 85 / style 3600), text-only 3317
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
structure: ref text-rows=4 our text-rows=3
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]

### themes-nord @ 120x30 run 1 step 7 — MISMATCH (exit)

op: keys[leader,q]
settled: ref=true our=true | mismatches: 3600 (char 85 / style 3600), text-only 3317
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
structure: ref text-rows=4 our text-rows=3
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (3,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (4,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (5,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (6,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]
  (7,0) ' '→' ' ref[fg=idx15 bg=idx237] our[fg=#FFFFFF bg=#2E3440]

### thought-expand @ 80x24 run 0 step 0 — MISMATCH (boot)

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

### thought-expand @ 80x24 run 0 step 1 — MISMATCH (dismiss)

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

### thought-expand @ 80x24 run 0 step 2 — MISMATCH (models-cmd)

op: text("/models")
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1754
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

### thought-expand @ 80x24 run 0 step 3 — MISMATCH (models-open)

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

### thought-expand @ 80x24 run 0 step 4 — MISMATCH (models-filter)

op: text("fake")
settled: ref=true our=true | mismatches: 1920 (char 273 / style 1920), text-only 1607
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 11 (masked-text runs/margins differ)
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

### thought-expand @ 80x24 run 0 step 5 — MISMATCH (models-select)

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

### thought-expand @ 80x24 run 0 step 6 — MISMATCH (variant-dismiss)

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

### thought-expand @ 80x24 run 0 step 7 — MISMATCH (type)

op: text("Think carefully")
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1755
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

### thought-expand @ 80x24 run 0 step 8 — MISMATCH (submit)

op: keys[enter]
settled: ref=true our=true | mismatches: 1920 (char 286 / style 1920), text-only 1613
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello after  +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=5
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### thought-expand @ 80x24 run 0 step 9 — MISMATCH (settled)

op: wait
settled: ref=true our=true | mismatches: 1920 (char 286 / style 1920), text-only 1612
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello after  +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=5
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### thought-expand @ 80x24 run 0 step 10 — MISMATCH (expand-press)

op: mouse(press @4,8)
settled: ref=true our=true | mismatches: 1920 (char 286 / style 1920), text-only 1613
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello after  +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=5
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### thought-expand @ 80x24 run 0 step 11 — MISMATCH (expand-release)

op: mouse(release @4,8)
settled: ref=true our=true | mismatches: 1920 (char 286 / style 1920), text-only 1612
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello after  +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=5
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### thought-expand @ 80x24 run 0 step 12 — MISMATCH (expanded)

op: wait
settled: ref=true our=true | mismatches: 1920 (char 286 / style 1920), text-only 1613
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello after  +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=5
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### thought-expand @ 80x24 run 0 step 13 — MISMATCH (exit)

op: keys[leader,q]
settled: ref=true our=true | mismatches: 1920 (char 286 / style 1920), text-only 1612
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello after  +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=5
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### thought-expand @ 80x24 run 1 step 0 — MISMATCH (boot)

op: wait
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1713
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

### thought-expand @ 80x24 run 1 step 1 — MISMATCH (dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1713
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

### thought-expand @ 80x24 run 1 step 2 — MISMATCH (models-cmd)

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

### thought-expand @ 80x24 run 1 step 3 — MISMATCH (models-open)

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

### thought-expand @ 80x24 run 1 step 4 — MISMATCH (models-filter)

op: text("fake")
settled: ref=true our=true | mismatches: 1920 (char 273 / style 1920), text-only 1607
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx0] our[fg=#FFFFFF bg=#040404]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 11 (masked-text runs/margins differ)
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

### thought-expand @ 80x24 run 1 step 5 — MISMATCH (models-select)

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

### thought-expand @ 80x24 run 1 step 6 — MISMATCH (variant-dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1713
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

### thought-expand @ 80x24 run 1 step 7 — MISMATCH (type)

op: text("Think carefully")
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1755
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

### thought-expand @ 80x24 run 1 step 8 — MISMATCH (submit)

op: keys[enter]
settled: ref=true our=true | mismatches: 1920 (char 286 / style 1920), text-only 1613
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello after  +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=5
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### thought-expand @ 80x24 run 1 step 9 — MISMATCH (settled)

op: wait
settled: ref=true our=true | mismatches: 1920 (char 286 / style 1920), text-only 1612
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello after  +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=5
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### thought-expand @ 80x24 run 1 step 10 — MISMATCH (expand-press)

op: mouse(press @4,8)
settled: ref=true our=true | mismatches: 1920 (char 286 / style 1920), text-only 1613
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello after  +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=5
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### thought-expand @ 80x24 run 1 step 11 — MISMATCH (expand-release)

op: mouse(release @4,8)
settled: ref=true our=true | mismatches: 1920 (char 286 / style 1920), text-only 1612
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello after  +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=5
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### thought-expand @ 80x24 run 1 step 12 — MISMATCH (expanded)

op: wait
settled: ref=true our=true | mismatches: 1920 (char 286 / style 1920), text-only 1612
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello after  +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=5
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### thought-expand @ 80x24 run 1 step 13 — MISMATCH (exit)

op: keys[leader,q]
settled: ref=true our=true | mismatches: 1920 (char 286 / style 1920), text-only 1613
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello after  +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=5
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### thought-expand @ 120x30 run 0 step 0 — MISMATCH (boot)

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

### thought-expand @ 120x30 run 0 step 1 — MISMATCH (dismiss)

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

### thought-expand @ 120x30 run 0 step 2 — MISMATCH (models-cmd)

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

### thought-expand @ 120x30 run 0 step 3 — MISMATCH (models-open)

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

### thought-expand @ 120x30 run 0 step 4 — MISMATCH (models-filter)

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

### thought-expand @ 120x30 run 0 step 5 — MISMATCH (models-select)

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

### thought-expand @ 120x30 run 0 step 6 — MISMATCH (variant-dismiss)

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

### thought-expand @ 120x30 run 0 step 7 — MISMATCH (type)

op: text("Think carefully")
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

### thought-expand @ 120x30 run 0 step 8 — MISMATCH (submit)

op: keys[enter]
settled: ref=true our=true | mismatches: 3600 (char 366 / style 3600), text-only 3212
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello after  +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=5
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### thought-expand @ 120x30 run 0 step 9 — MISMATCH (settled)

op: wait
settled: ref=true our=true | mismatches: 3600 (char 366 / style 3600), text-only 3213
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello after  +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=5
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### thought-expand @ 120x30 run 0 step 10 — MISMATCH (expand-press)

op: mouse(press @4,8)
settled: ref=true our=true | mismatches: 3600 (char 366 / style 3600), text-only 3212
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello after  +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=5
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### thought-expand @ 120x30 run 0 step 11 — MISMATCH (expand-release)

op: mouse(release @4,8)
settled: ref=true our=true | mismatches: 3600 (char 366 / style 3600), text-only 3213
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello after  +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=5
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### thought-expand @ 120x30 run 0 step 12 — MISMATCH (expanded)

op: wait
settled: ref=true our=true | mismatches: 3600 (char 366 / style 3600), text-only 3212
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello after  +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=5
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### thought-expand @ 120x30 run 0 step 13 — MISMATCH (exit)

op: keys[leader,q]
settled: ref=true our=true | mismatches: 3600 (char 366 / style 3600), text-only 3213
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello after  +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=5
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### thought-expand @ 120x30 run 1 step 0 — MISMATCH (boot)

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

### thought-expand @ 120x30 run 1 step 1 — MISMATCH (dismiss)

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

### thought-expand @ 120x30 run 1 step 2 — MISMATCH (models-cmd)

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

### thought-expand @ 120x30 run 1 step 3 — MISMATCH (models-open)

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

### thought-expand @ 120x30 run 1 step 4 — MISMATCH (models-filter)

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

### thought-expand @ 120x30 run 1 step 5 — MISMATCH (models-select)

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

### thought-expand @ 120x30 run 1 step 6 — MISMATCH (variant-dismiss)

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

### thought-expand @ 120x30 run 1 step 7 — MISMATCH (type)

op: text("Think carefully")
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

### thought-expand @ 120x30 run 1 step 8 — MISMATCH (submit)

op: keys[enter]
settled: ref=true our=true | mismatches: 3600 (char 367 / style 3600), text-only 3212
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello after  +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=5
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### thought-expand @ 120x30 run 1 step 9 — MISMATCH (settled)

op: wait
settled: ref=true our=true | mismatches: 3600 (char 367 / style 3600), text-only 3211
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello after  +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=5
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### thought-expand @ 120x30 run 1 step 10 — MISMATCH (expand-press)

op: mouse(press @4,8)
settled: ref=true our=true | mismatches: 3600 (char 367 / style 3600), text-only 3212
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello after  +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=5
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### thought-expand @ 120x30 run 1 step 11 — MISMATCH (expand-release)

op: mouse(release @4,8)
settled: ref=true our=true | mismatches: 3600 (char 367 / style 3600), text-only 3211
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello after  +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=5
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### thought-expand @ 120x30 run 1 step 12 — MISMATCH (expanded)

op: wait
settled: ref=true our=true | mismatches: 3600 (char 367 / style 3600), text-only 3212
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello after  +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=5
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### thought-expand @ 120x30 run 1 step 13 — MISMATCH (exit)

op: keys[leader,q]
settled: ref=true our=true | mismatches: 3600 (char 367 / style 3600), text-only 3211
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello after  +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=5
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### thought-render @ 80x24 run 0 step 0 — MISMATCH (boot)

op: wait
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1713
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

### thought-render @ 80x24 run 0 step 1 — MISMATCH (dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1713
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

### thought-render @ 80x24 run 0 step 2 — MISMATCH (models-cmd)

op: text("/models")
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1754
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

### thought-render @ 80x24 run 0 step 3 — MISMATCH (models-open)

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

### thought-render @ 80x24 run 0 step 4 — MISMATCH (models-filter)

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

### thought-render @ 80x24 run 0 step 5 — MISMATCH (models-select)

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

### thought-render @ 80x24 run 0 step 6 — MISMATCH (variant-dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1713
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

### thought-render @ 80x24 run 0 step 7 — MISMATCH (type)

op: text("Think carefully")
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1755
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

### thought-render @ 80x24 run 0 step 8 — MISMATCH (submit)

op: keys[enter]
settled: ref=true our=true | mismatches: 1920 (char 286 / style 1920), text-only 1613
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello after  +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=5
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### thought-render @ 80x24 run 0 step 9 — MISMATCH (settled)

op: wait
settled: ref=true our=true | mismatches: 1920 (char 286 / style 1920), text-only 1613
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello after  +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=5
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### thought-render @ 80x24 run 0 step 10 — MISMATCH (exit)

op: keys[leader,q]
settled: ref=true our=true | mismatches: 1920 (char 286 / style 1920), text-only 1612
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello after  +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=5
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### thought-render @ 80x24 run 1 step 0 — MISMATCH (boot)

op: wait
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1713
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

### thought-render @ 80x24 run 1 step 1 — MISMATCH (dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1713
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

### thought-render @ 80x24 run 1 step 2 — MISMATCH (models-cmd)

op: text("/models")
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1754
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

### thought-render @ 80x24 run 1 step 3 — MISMATCH (models-open)

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

### thought-render @ 80x24 run 1 step 4 — MISMATCH (models-filter)

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

### thought-render @ 80x24 run 1 step 5 — MISMATCH (models-select)

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

### thought-render @ 80x24 run 1 step 6 — MISMATCH (variant-dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1713
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

### thought-render @ 80x24 run 1 step 7 — MISMATCH (type)

op: text("Think carefully")
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1755
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

### thought-render @ 80x24 run 1 step 8 — MISMATCH (submit)

op: keys[enter]
settled: ref=true our=true | mismatches: 1920 (char 287 / style 1920), text-only 1612
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello after  +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=5
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### thought-render @ 80x24 run 1 step 9 — MISMATCH (settled)

op: wait
settled: ref=true our=true | mismatches: 1920 (char 287 / style 1920), text-only 1611
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello after  +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=5
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### thought-render @ 80x24 run 1 step 10 — MISMATCH (exit)

op: keys[leader,q]
settled: ref=true our=true | mismatches: 1920 (char 287 / style 1920), text-only 1612
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello after  +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=5
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### thought-render @ 120x30 run 0 step 0 — MISMATCH (boot)

op: wait
settled: ref=true our=true | mismatches: 3600 (char 84 / style 3600), text-only 3393
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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

### thought-render @ 120x30 run 0 step 1 — MISMATCH (dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 3600 (char 84 / style 3600), text-only 3393
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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

### thought-render @ 120x30 run 0 step 2 — MISMATCH (models-cmd)

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

### thought-render @ 120x30 run 0 step 3 — MISMATCH (models-open)

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

### thought-render @ 120x30 run 0 step 4 — MISMATCH (models-filter)

op: text("fake")
settled: ref=true our=true | mismatches: 3600 (char 362 / style 3600), text-only 3206
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

### thought-render @ 120x30 run 0 step 5 — MISMATCH (models-select)

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

### thought-render @ 120x30 run 0 step 6 — MISMATCH (variant-dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 3600 (char 84 / style 3600), text-only 3393
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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

### thought-render @ 120x30 run 0 step 7 — MISMATCH (type)

op: text("Think carefully")
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

### thought-render @ 120x30 run 0 step 8 — MISMATCH (submit)

op: keys[enter]
settled: ref=true our=true | mismatches: 3600 (char 366 / style 3600), text-only 3213
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello after  +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=5
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### thought-render @ 120x30 run 0 step 9 — MISMATCH (settled)

op: wait
settled: ref=true our=true | mismatches: 3600 (char 366 / style 3600), text-only 3212
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello after  +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=5
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### thought-render @ 120x30 run 0 step 10 — MISMATCH (exit)

op: keys[leader,q]
settled: ref=true our=true | mismatches: 3600 (char 366 / style 3600), text-only 3213
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello after  +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=5
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### thought-render @ 120x30 run 1 step 0 — MISMATCH (boot)

op: wait
settled: ref=true our=true | mismatches: 3600 (char 102 / style 3600), text-only 3380
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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

### thought-render @ 120x30 run 1 step 1 — MISMATCH (dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 3600 (char 102 / style 3600), text-only 3380
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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

### thought-render @ 120x30 run 1 step 2 — MISMATCH (models-cmd)

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

### thought-render @ 120x30 run 1 step 3 — MISMATCH (models-open)

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

### thought-render @ 120x30 run 1 step 4 — MISMATCH (models-filter)

op: text("fake")
settled: ref=true our=true | mismatches: 3600 (char 362 / style 3600), text-only 3206
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

### thought-render @ 120x30 run 1 step 5 — MISMATCH (models-select)

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

### thought-render @ 120x30 run 1 step 6 — MISMATCH (variant-dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 3600 (char 102 / style 3600), text-only 3380
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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

### thought-render @ 120x30 run 1 step 7 — MISMATCH (type)

op: text("Think carefully")
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

### thought-render @ 120x30 run 1 step 8 — MISMATCH (submit)

op: keys[enter]
settled: ref=true our=true | mismatches: 3600 (char 366 / style 3600), text-only 3212
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello after  +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=5
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### thought-render @ 120x30 run 1 step 9 — MISMATCH (settled)

op: wait
settled: ref=true our=true | mismatches: 3600 (char 366 / style 3600), text-only 3213
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello after  +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=5
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### thought-render @ 120x30 run 1 step 10 — MISMATCH (exit)

op: keys[leader,q]
settled: ref=true our=true | mismatches: 3600 (char 366 / style 3600), text-only 3213
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
ref row 0: `   Parity fake says hello after  +`
our row 0: `   Untitled session              +`
ref row 1: ``
our row 1: ``
ref row 2: `  ┃`
our row 2: `  ┃`
structure: first run-digest divergence at row 0 (masked-text runs/margins differ)
structure: ref text-rows=7 our text-rows=5
cell samples (x,y): ref→our
  (0,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (1,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (2,0) ' '→' ' ref[fg=idx15 bg=idx234] our[fg=#FFFFFF bg=#1E1E1E]
  (3,0) 'P'→'U' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (4,0) 'a'→'n' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (5,0) 'r'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (6,0) 'i'→'i' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]
  (7,0) 't'→'t' ref[fg=idx255 bg=idx234 [bold]] our[fg=#EEEEEE bg=#1E1E1E [bold]]

### type-edit-submit @ 80x24 run 0 step 0 — MISMATCH (boot)

op: wait
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1713
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

### type-edit-submit @ 80x24 run 0 step 1 — MISMATCH (dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1713
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

### type-edit-submit @ 80x24 run 0 step 2 — MISMATCH (models-cmd)

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

### type-edit-submit @ 80x24 run 0 step 3 — MISMATCH (models-open)

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

### type-edit-submit @ 80x24 run 0 step 4 — MISMATCH (models-filter)

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

### type-edit-submit @ 80x24 run 0 step 5 — MISMATCH (models-select)

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

### type-edit-submit @ 80x24 run 0 step 6 — MISMATCH (variant-dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1713
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

### type-edit-submit @ 80x24 run 0 step 7 — MISMATCH (type)

op: text("Say hello world")
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1755
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

### type-edit-submit @ 80x24 run 0 step 8 — MISMATCH (edit)

op: keys[backspace,backspace,backspace,backspace,backspace]
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1755
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

### type-edit-submit @ 80x24 run 0 step 9 — MISMATCH (retype)

op: text("hello.")
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

### type-edit-submit @ 80x24 run 0 step 10 — MISMATCH (submit)

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

### type-edit-submit @ 80x24 run 0 step 11 — MISMATCH (settled-reply)

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

### type-edit-submit @ 80x24 run 0 step 12 — MISMATCH (exit)

op: keys[leader,q]
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

### type-edit-submit @ 80x24 run 1 step 0 — MISMATCH (boot)

op: wait
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1713
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

### type-edit-submit @ 80x24 run 1 step 1 — MISMATCH (dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1713
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

### type-edit-submit @ 80x24 run 1 step 2 — MISMATCH (models-cmd)

op: text("/models")
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1754
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

### type-edit-submit @ 80x24 run 1 step 3 — MISMATCH (models-open)

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

### type-edit-submit @ 80x24 run 1 step 4 — MISMATCH (models-filter)

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

### type-edit-submit @ 80x24 run 1 step 5 — MISMATCH (models-select)

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

### type-edit-submit @ 80x24 run 1 step 6 — MISMATCH (variant-dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1713
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

### type-edit-submit @ 80x24 run 1 step 7 — MISMATCH (type)

op: text("Say hello world")
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

### type-edit-submit @ 80x24 run 1 step 8 — MISMATCH (edit)

op: keys[backspace,backspace,backspace,backspace,backspace]
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1755
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

### type-edit-submit @ 80x24 run 1 step 9 — MISMATCH (retype)

op: text("hello.")
settled: ref=true our=true | mismatches: 1920 (char 64 / style 1920), text-only 1755
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

### type-edit-submit @ 80x24 run 1 step 10 — MISMATCH (submit)

op: keys[enter]
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

### type-edit-submit @ 80x24 run 1 step 11 — MISMATCH (settled-reply)

op: wait
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

### type-edit-submit @ 80x24 run 1 step 12 — MISMATCH (exit)

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

### type-edit-submit @ 120x30 run 0 step 0 — MISMATCH (boot)

op: wait
settled: ref=true our=true | mismatches: 3600 (char 102 / style 3600), text-only 3380
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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

### type-edit-submit @ 120x30 run 0 step 1 — MISMATCH (dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 3600 (char 102 / style 3600), text-only 3380
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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

### type-edit-submit @ 120x30 run 0 step 2 — MISMATCH (models-cmd)

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

### type-edit-submit @ 120x30 run 0 step 3 — MISMATCH (models-open)

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

### type-edit-submit @ 120x30 run 0 step 4 — MISMATCH (models-filter)

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

### type-edit-submit @ 120x30 run 0 step 5 — MISMATCH (models-select)

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

### type-edit-submit @ 120x30 run 0 step 6 — MISMATCH (variant-dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 3600 (char 102 / style 3600), text-only 3380
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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

### type-edit-submit @ 120x30 run 0 step 7 — MISMATCH (type)

op: text("Say hello world")
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

### type-edit-submit @ 120x30 run 0 step 8 — MISMATCH (edit)

op: keys[backspace,backspace,backspace,backspace,backspace]
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

### type-edit-submit @ 120x30 run 0 step 9 — MISMATCH (retype)

op: text("hello.")
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

### type-edit-submit @ 120x30 run 0 step 10 — MISMATCH (submit)

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

### type-edit-submit @ 120x30 run 0 step 11 — MISMATCH (settled-reply)

op: wait
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

### type-edit-submit @ 120x30 run 0 step 12 — MISMATCH (exit)

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

### type-edit-submit @ 120x30 run 1 step 0 — MISMATCH (boot)

op: wait
settled: ref=true our=true | mismatches: 3600 (char 102 / style 3600), text-only 3380
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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

### type-edit-submit @ 120x30 run 1 step 1 — MISMATCH (dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 3600 (char 102 / style 3600), text-only 3380
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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

### type-edit-submit @ 120x30 run 1 step 2 — MISMATCH (models-cmd)

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

### type-edit-submit @ 120x30 run 1 step 3 — MISMATCH (models-open)

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

### type-edit-submit @ 120x30 run 1 step 4 — MISMATCH (models-filter)

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

### type-edit-submit @ 120x30 run 1 step 5 — MISMATCH (models-select)

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

### type-edit-submit @ 120x30 run 1 step 6 — MISMATCH (variant-dismiss)

op: keys[esc]
settled: ref=true our=true | mismatches: 3600 (char 102 / style 3600), text-only 3380
first diff: (0, 0) ref=' ' our=' ' styles ref[fg=idx15 bg=idx232] our[fg=#FFFFFF bg=#0A0A0A]
ref row 0: ``
our row 0: ``
ref row 1: ``
our row 1: ``
ref row 2: ``
our row 2: ``
structure: first run-digest divergence at row 16 (masked-text runs/margins differ)
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

### type-edit-submit @ 120x30 run 1 step 7 — MISMATCH (type)

op: text("Say hello world")
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

### type-edit-submit @ 120x30 run 1 step 8 — MISMATCH (edit)

op: keys[backspace,backspace,backspace,backspace,backspace]
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

### type-edit-submit @ 120x30 run 1 step 9 — MISMATCH (retype)

op: text("hello.")
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

### type-edit-submit @ 120x30 run 1 step 10 — MISMATCH (submit)

op: keys[enter]
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

### type-edit-submit @ 120x30 run 1 step 11 — MISMATCH (settled-reply)

op: wait
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

### type-edit-submit @ 120x30 run 1 step 12 — MISMATCH (exit)

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

