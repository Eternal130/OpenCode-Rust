# 沙箱覆盖台账（Coverage Ledger）

> 依据：开发规范 2.2#13（2026-11-18 用户裁定二）——能测的都测，不能有任何已知的遗漏。
> 状态词汇：**covered**=场景在册（列场景名）；**GAP**=已知遗漏（入排期，两波内不补须波计划显式说明）；**untestable**=不可测+理由；**audit**=覆盖与否待盘点。
> 通道 8 的 30 场景×3 宽度档 + frames 15 + B2 真插件 + 通道 1-7 单元 parity 构成基线，本台账以通道 9 真实链路族为主视角盘点增量面。

## A. 输入事件层（PTY 真字节注入）

| 面 | 状态 | 场景/备注 |
|---|---|---|
| 字母/数字/符号键 | covered | 各场景 write_both |
| 修饰键/chords（ctrl+x m 等） | covered | S3/S6 palette 键序 |
| ESC 链（中断/退出对话框） | covered | P3 中断流 |
| Enter/多行 | covered | tl 工具环 |
| 鼠标左键点击（行/tab/对话框） | covered | 单元级 mouse_* 族（app 级） |
| **鼠标滚轮（SGR 64/65）** | covered | pty_w11_mouse（W11 新增——此前为 GAP，真机暴露） |
| 鼠标拖选+高亮+copy-on-select | covered | pty_w11_mouse（W11） |
| **拖选范围语义（按字符 vs 含空白）** | GAP | 真机报告⑧残余——待 ref oracle 裁定后钉 |
| 鼠标中/右键 | GAP | 无场景 |
| 双击（词选） | GAP | 无场景 |
| 悬停 hover（tooltip/status） | GAP | 无场景 |
| 粘贴（普通+括号粘贴模式+大文本 8k） | 部分 | e1_cells_paste_8k（单元级）；PTY 真字节粘贴缺 |
| resize（SIGWINCH 尺寸切换） | GAP | 无场景（宽档只覆盖各自 boot 尺寸，无运行中切换） |
| focus/blur 事件 | audit | — |

## B. 会话流（真实链路）

| 面 | 状态 | 场景/备注 |
|---|---|---|
| boot/连接/settle | covered | t9_1/S1 |
| 慢启动 boot 窗 | covered | t9_8 |
| 发消息→回复+标题 | covered | S2/t9_3 |
| 工具环（bash/edit/read） | covered | tl_tool_loop |
| 工具族其余（write/glob/grep/task） | GAP | tl 只覆盖 bash+edit——工具行渲染形状族 |
| 权限对话框（允许/拒绝/始终） | GAP | 预授权绕过——对话框流零场景 |
| 中断（流中 ESC） | covered | P3 |
| 慢 TTFT/慢 TPS | covered | P1/P2 |
| 长会话多轮 | covered | P4 state C |
| 会话切换/多标签 | 部分 | S4 重启恢复；运行中切换 tab 缺 |
| compaction（压缩标记渲染） | GAP | 无场景 |
| 错误显示（provider 错误/`· interrupted`） | covered | G-438 P3 |
| 断连恢复（连接丢而复得） | covered | vt_temporal loss-recovery（单元级）；真实链路缺 | audit |

## C. UI 组件面

| 面 | 状态 | 场景/备注 |
|---|---|---|
| composer @ 引用+截断 | covered | G-448 单测——PTY 双端场景缺 | audit |
| composer / 命令 | audit | — |
| palette 模型/命令 | covered | S3/S6 |
| **插件 sections（omo-slim）** | GAP | 真机报告⑥''——触发态未建（空会话双端都不渲染） |
| 侧栏各宽度档 | covered | S5(160)/P4b(120)；80/320 档缺 | 部分 |
| tabs busy 呼吸灯 | covered | busy_pulse_frames |
| tabs 滚动/关闭/恢复 | 部分 | S4 恢复；关闭/滚动缺 |
| 对话框族（confirm/input/select） | audit | 单元级部分；链路级缺 |
| toast（错误/提示/退场） | 部分 | copy-on-select toast（W11） |
| 错误/权限 UI | GAP | 随 B 权限对话框 |

## D. 配置/环境

| 面 | 状态 | 场景/备注 |
|---|---|---|
| 配置热载→palette | covered | S6/G-437 |
| 插件装载/热载 | 部分 | T9-7 装载；热载缺 |
| 模型目录（自定义 provider） | covered | S3/G-433 |
| agent/command 定义变更 | GAP | 无场景 |
| AGENTS.md 变更 | GAP | 无场景 |
| 主题切换 | GAP | 无场景 |

## E. 持久化/迁移

| 面 | 状态 | 场景/备注 |
|---|---|---|
| 会话存储/重启恢复 | covered | S4/t9_4 |
| **v2 数据迁移（凭据/会话）** | GAP | **G-445 真机根因正是迁移面**——沙箱零场景；concern②（全新安装跳过 legacy 凭据导入）也在此 |
| 多 channel 数据隔离 | audit | — |

## F. 终端环境档

| 面 | 状态 | 场景/备注 |
|---|---|---|
| 宽度档 120/160 | covered | 通道 8 三档+S5/P4b |
| 运行中 resize | GAP | 同 A-resize |
| 颜色能力降档（256/16） | audit | — |
| kitty 键盘协议 | 部分 | key_event 解析在；端到端场景 audit |

## 维护规程

- 每波收口：控制者核对台账——新covered/新GAP/关闭的GAP；GAP 超两波未补=波计划显式说明。
- 新可测面来源：用户真机报告（第一优先）、代码审读、上游 diff、复盘。
- 本台账初始版为控制者草案（2026-11-18），audit 项由 W13 盘点定稿。
