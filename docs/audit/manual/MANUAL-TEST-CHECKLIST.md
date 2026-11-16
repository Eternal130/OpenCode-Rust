# Manual Test Checklist — opencode-rust 2.0.24 移植验收（人工测试阶段）

**阶段定位**（用户路线图 2026-11-16）：差分通道全一致（2026-11-16 达成：committed 90/90 + frames 15/15 + rec- 9/9 全 EXACT、CAUSES 0）之后，人工测试通过即宣告 2.0.24 移植完成，随后跟随上游。

**为什么需要人工测试**：差分只证 rust==ref 2.0.24（parity）；双侧同错不可见（correctness 盲区）。本清单覆盖"差分管不到"的真实使用面——你是执行者，日常用 2.2.7 的真实习惯就是对照基线。

**执行方式**：日常使用 opencode-rust 替代日常驱动，按下面清单逐项过。每项三态：✅ 通过 / ❌ 异常（记录复现步骤+截图）/ ⚠️ 存疑。建议开启录制（`export TUI_RECORD_DIR=$HOME/.opencode/recordings`——异常自动留证，通道可重放归因）。

---

## A. 启动与配置（冷启动面）

- [ ] A1 冷启动到主屏（首帧无闪帧/无残留块）
- [ ] A2 `opencode`（无参）与 `opencode "prompt"`（带初始 prompt）两条路径
- [ ] A3 auth：API key / OAuth 流程后模型可列举可选用
- [ ] A4 config 变更热重载（改主题/模型后不重启即生效）
- [ ] A5 主题切换（含 256 色降级终端）

## B. 核心对话环（最高频面）

- [ ] B1 提问→流式输出→完成（无丢字/无重复段/无乱序）
- [ ] B2 工具调用：请求→授权→执行→结果回填（含多轮连续工具）
- [ ] B3 中断（ESC）：流式中断后可继续提问；tab spinner 正确静止化
- [ ] B4 排队/steer（active turn 中 `<leader>return` queue）：乐观行出现在转录尾部、delivered 后清除
- [ ] B5 重试/失败：provider 报错后 toast 可操作、草稿恢复、无幽灵行
- [ ] B6 长会话（>30 轮）滚动/性能无劣化

## C. 会话与标签（多会话面）

- [ ] C1 新建/切换/关闭会话；tab 条状态（busy/idle/unread 指示）
- [ ] C2 重连到 running 会话（boot 快照：spinner/interrupt 门应亮——G-426④/boot snapshot 修复面）
- [ ] C3 session 恢复（重启 opencode-rust 后历史会话可继续）
- [ ] C4 重连/网络闪断：reconnecting 覆盖层出现又消失、事件不丢

## D. Composer（输入面）

- [ ] D1 粘贴大段文本/多行/含 ANSI 转义
- [ ] D2 `@` 文件引用与 skills 弹层、过滤、选中
- [ ] D3 leader 和弦（`<leader>` 系列操作）
- [ ] D4 历史回溯（↑/↓）与草稿保留语义
- [ ] D5 图片/附件拖入或路径引用

## E. TUI 表面（对话框与覆盖层）

- [ ] E1 命令面板：搜索、分组、执行命令
- [ ] E2 /plugins 管理屏：列表、失败行、enter 查看错误（G-425 修复面）
- [ ] E3 config 对话框（含 animations 开关——G-430 生产接线面：关掉后 spinner 应转 ⋯）
- [ ] E4 toast 动作（长标题折行 + `› label` affordance——G-424/G-424b 修复面）
- [ ] E5 console 面板 / which-key / 快捷键提示层
- [ ] E6 update 检查对话框

## F. PTY 与嵌入式终端

- [ ] F1 ctrl+b shell 输出查看器、ctrl+d kill
- [ ] F2 嵌入终端跑真实命令（cargo/bun），输出滚动/搜索
- [ ] F3 shell 行首行折行与长命令显示

## G. 插件（真载荷）

- [ ] G1 omo-slim 加载（侧栏/命令）；superpowers skills 可用
- [ ] G2 插件失败态：坏插件目录 → chip+toast+管理屏失败行（G-425 面）
- [ ] G3 插件命令经 palette 执行

## H. 环境与鲁棒

- [ ] H1 resize（拖拽窗口/分屏变化）内容不错位
- [ ] H2 超宽终端（≥200 列）与超窄（80 列）
- [ ] H3 鼠标：滚轮翻页、点击行/hover
- [ ] H4 两个并发实例（不同项目目录）

---

## 已知边界（测到=预期内，不是新 bug）

- 无标题 toast 长消息不折行（无证据面，登记在册）
- command-palette@320x80 ref 分组序竞态簇（登记在册）
- 重连不重灌 running 快照（reconnect 重灌，登记在册）
- `session_list_all_projects()` 恒 false 占位（登记在册）
- outbox 竞态（POST 超时+echo 先至的窄窗口，登记在册）

## 汇报格式

每项一行三态 + 异常附复现步骤（有录制则附 rec 文件名）。全部 ✅（或异常均已归因修复）→ **2.0.24 移植宣告完成** → 进入上游跟随阶段。
