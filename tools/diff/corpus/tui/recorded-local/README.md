# recorded-local — 本地真实语料（git-ignored）

> **日常录制启用法（三行）**
> 1. `export TUI_RECORD_DIR=~/.local/share/opencode/tui-recordings`（想录时才设；不设=完全旁路，零开销）
> 2. 用 opencode-rust（TUI）正常工作——会话事件流/键序/fetch/尺寸史自动落 `<dir>/rec-*.raw.json`（默认脱敏）
> 3. `python3 tools/diff/tui_record_compile.py <dir>` 编译进本目录；之后跑通道（`tools/diff/tui_fullscreen.py`）即自动纳入重放
>
> 铁律不变：录制内容是真实用户数据，**永不入库、永不上传**（下方第 1-4 条）。

**隐私铁律（用户裁定 2026-11-15，最高优先级）**：

1. 本目录存放从真实 TUI 会话编译而来的差分场景
   （`tui-record-compile` 编译 `TUI_RECORD_DIR` 录制的 `.raw.json`
   原始件）。**真实语料只本地跑，数据不上传。**
2. 本目录整体 git-ignored（根 `.gitignore` 的
   `tools/diff/corpus/tui/recorded-local/` 条目）；任何 commit 不得
   含录制内容——原始件（`.raw.json`）与编译件都不例外。
   防线：`tools/guards/recorded_local.py`（`git ls-files` 断言）+
   编排器 `tools/diff/tui_fullscreen.py` 的启动检查。
3. 报告只允许**聚合计数**（case 数、verdict 分布），不得引用录制
   内容原文。
4. 消息内容默认脱敏（录制器与编译器双层，同形占位符：保长度/
   换行/unicode 类别）；`TUI_RECORD_REDACT=0` / `--no-redact` 仅为
   本地排障，产物仍不得离开本机。

编排器行为：本目录存在即自动纳入通道 8（case id 前缀 `rec-`，
报告单列 recorded-local 小节，只聚合计数）；目录缺席 = 零影响。
**门语义（控制者裁定 2026-11-15）**：`rec-*` 的未裁决分歧 =
advisory——不参与 gate exit（日常录制常驻未裁决项，committed 绿
才是门绿）；`rec-*` 的 infra 失败（stub_gap/网格缺损等）仍 =
exit 1（那是重放机制坏了，与裁定无关）。
