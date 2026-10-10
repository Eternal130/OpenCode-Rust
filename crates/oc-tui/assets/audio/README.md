# Attention sound assets

The five built-in TUI attention sounds (the `opencode.default` sound
pack), vendored byte-for-byte from `@opencode/ui@2.0.24`
(`packages/ui/src/assets/audio/`), which is MIT-licensed
(https://github.com/anomalyco/opencode, packages/ui). Provenance:

| File (this dir)            | Attention event  | Source (same filename)                    | MD5                              |
| -------------------------- | ---------------- | ----------------------------------------- | -------------------------------- |
| `bip-bop-01.mp3`           | `default`        | `@opencode/ui/audio/bip-bop-01.mp3`       | `d1e504422de45a3c588c4865cfe165b0` |
| `bip-bop-03.mp3`           | `question`       | `@opencode/ui/audio/bip-bop-03.mp3`       | `bcf0fda84cc85ee883efb54909e578f2` |
| `staplebops-06.mp3`        | `permission`     | `@opencode/ui/audio/staplebops-06.mp3`    | `b86325d70eeaa6779e9ec063b1ecfe73` |
| `nope-03.mp3`              | `error`          | `@opencode/ui/audio/nope-03.mp3`          | `b977e16facba3d646a268400cd43dc28` |
| `yup-01.mp3`               | `subagent_done`  | `@opencode/ui/audio/yup-01.mp3`           | `a83abd2a69fd5b78b95ea1dc0b6d238a` |

v2 wires these in `packages/tui/src/attention-sounds.{bun,node}.ts`
(`BUILTIN_SOUNDS` in `packages/tui/src/attention.ts`; `done` reuses the
`default` sound — no separate asset exists upstream either).

The files are embedded with `include_bytes!` (≈22 KiB total) and
extracted lazily to the opencode cache dir at first play
(`crates/oc-tui/src/sound.rs`).
