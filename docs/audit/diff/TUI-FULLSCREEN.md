# TUI-FULLSCREEN — TUI 整屏差分（通道 8）

同一 `tui-diff-scenario/1` 场景（tools/diff/corpus/tui/SCHEMA.md）喂双侧 TUI 实现，
静止帧导出为 `tui-diff-grid/1` 稠密网格后比对。T5 判门：exact / structural
（G-368 RGB ≤8 容差，仍是分歧）/ diverge。CAUSES 已知因定性属 T6，
adjudication 属 T9；当前一切非 exact 均未裁决 → exit 2。

## 总量: 6 个 case（主档 + 镜像 120x30 档）— exact 0 / structural 0 / diverge 6

| case | verdict | diff cells | first_diff | ref size | rust size | ref gaps | rust gaps | ref paths | rust paths |
|---|---|---|---|---|---|---|---|---|---|
| home-empty | diverge | 42 | r15c6 ch ref=' ' rust='B' | 80x24 | 80x24 | 0 | 0 | 20 | 11 |
| home-empty@120x30 | diverge | 42 | r18c26 ch ref=' ' rust='B' | 120x30 | 120x30 | 0 | 0 | 20 | 11 |
| session-markdown | diverge | 612 | r0c0 fg ref=[255, 255, 255] rust=[238, 238, 238] | 80x24 | 80x24 | 0 | 0 | 27 | 14 |
| session-markdown@120x30 | diverge | 892 | r0c0 fg ref=[255, 255, 255] rust=[238, 238, 238] | 120x30 | 120x30 | 0 | 0 | 27 | 14 |
| session-text | diverge | 610 | r0c0 fg ref=[255, 255, 255] rust=[238, 238, 238] | 80x24 | 80x24 | 0 | 0 | 27 | 14 |
| session-text@120x30 | diverge | 890 | r0c0 fg ref=[255, 255, 255] rust=[238, 238, 238] | 120x30 | 120x30 | 0 | 0 | 27 | 14 |

### first_diff 明细（未裁决分歧基线，裁决属 T9）

- `home-empty` [diverge] row=15 col=6 field=ch ref=' ' rust='B' cells_diff_count=42
- `home-empty@120x30` [diverge] row=18 col=26 field=ch ref=' ' rust='B' cells_diff_count=42
- `session-markdown` [diverge] row=0 col=0 field=fg ref=[255, 255, 255] rust=[238, 238, 238] cells_diff_count=612
- `session-markdown@120x30` [diverge] row=0 col=0 field=fg ref=[255, 255, 255] rust=[238, 238, 238] cells_diff_count=892
- `session-text` [diverge] row=0 col=0 field=fg ref=[255, 255, 255] rust=[238, 238, 238] cells_diff_count=610
- `session-text@120x30` [diverge] row=0 col=0 field=fg ref=[255, 255, 255] rust=[238, 238, 238] cells_diff_count=890

request_census 不参与比对（双侧 boot 请求模式已知不对称，仅供参考；
stub_gap 规则见 SCHEMA.md，检测到即 infra 红，编排器已按此判门）。

