# opencode-rust 移植路线图（ROADMAP）

> 2026-10-11 起。范围裁定（用户指令）：**除桌面端（packages/desktop Tauri 壳）外，v2.0.24 的一切都要迁移，包括 Web UI（packages/app）**。
> 规范源不变：opencode v2.0.24（commit e7a34f09bf，/opt/opencode-ref 只读）。
> 本文档是唯一的差距台账：每项有编号、来源、状态与目标批次；完成即勾销并注明验收证据。

## 范围解释（登记防歧义）

- **IN**：`packages/app`（Web UI，由 `opencode serve` 一并托管，PWA）、`packages/cli` 全部命令（含 `mini`、`acp`）、`session-ui` 等被 app/cli 共享的包、TUI 全部能力（C2 动画类解冻）、插件体系全部语义。
- **OUT**：`packages/desktop`（Tauri 壳）；`packages/web`（Astro 官网/文档站——营销站非产品功能，如需一并迁移另行指令）；v2.0.24 之后的上游新特性（钉版纪律不变）。
- `packages/gui-extensions`（SSH/WSL 远程 CLI 管理）：属桌面配套，随桌面端排除；若其中含独立于桌面的能力再个案评估。

## 状态图例

`pending` 未开工 ｜ `in-flight` 当前批次 ｜ `blocked` 等外部依赖 ｜ `done` 已完成并验收

---

## 一、当前批次 beta.3（已发布）+ beta.4（已落地待发）

| 编号 | 项 | 状态 |
|---|---|---|
| B3-01 | DB 行映射 NULL 容忍 + 500 必落服务端日志 | done（beta.3） |
| B3-02 | TUI 终端交互六连修 + 应答型 PTY 测试类 | done（beta.3） |
| B3-03 | rust 独立默认服务端口 | done（beta.3） |
| B3-04 | 新验证类补盲（应答 PTY / 真实库副本测试） | done（beta.3） |
| B4-H1 | provider/model 大小写解析（`own` 键 vs `Own/glm-5.3` 引用；根因 resolver.rs 大小写敏感+错误映射丢模型 ID） | done（beta.4） |
| B4-H2 | 回合失败浮出：未上屏错误补 v2 形态 error part + 服务端恒定一行日志；顺带修无限重试（per-attempt 重建 RetryPolicy）与连接期错误静默两个潜伏 bug | done（beta.4） |
| B4-H3 | 升级后旧守护自动换血：`/api/info` 响应头 `x-opencode-build`（<tag>@<hash>，golden 不可见），build 不匹配→Mismatch::Replace | **done（beta.5）** |

## 一点五、beta.4 功能落地

| 编号 | 项 | 状态 |
|---|---|---|
| R-01a | tree-sitter spike：oc-syntax crate + rust/python/ts 三语言 + markdown 代码块接线（+3.1MiB，18 语言外推 18.6MiB<20 阈值，性能 19× 余量） | done（beta.4） |
| R-01b | 15 语言铺开 + diff/tool-row 接线；体积护栏触发（72.7MiB>67.6 上限）→ 裁掉 swift/c-sharp/kotlin 三重语法，16 filetypes 约 64MiB；再加回路径=cargo feature 门控 | done（beta.5） |
| R-02 | `opencode mini` 分屏前端（滚动回显+4 行活动底栏+权限/表单+demo 模式，PTY 集成测试） | done（beta.4） |
| R-03 | `acp` 编辑器协议（官方 crate vendor，13 方法+权限/表单桥接，二进制级集成测试） | done（beta.4） |

## 二、功能缺失（全量消项——用户指令：全部解决）

| 编号 | 项 | v2 来源 | 优先级 | 目标批次 |
|---|---|---|---|---|
| R-01 | tree-sitter 语法高亮（diff/代码着色） | core 高亮管线 | P1 | beta.4；**方案已裁定（oracle）**：新叶 crate `oc-syntax`，原生精选 18 语言编译内置（bash/pwsh 已有；+rust/python/ts/js/go/c/cpp/c#/java/json/yaml/toml/html/css/diff），.scm 查询按 v2 的逐语言来源选择钉版 vendor（修复 v2 指向 nvim master 的不可复现性）；未覆盖语言回退纯文本（=现状）；禁运行时拉取（断网独立）；边界 API `highlight(block, filetype)`，markdown.rs/diff_viewer.rs/tool-row 三处接线；先行 spike（3 语言+体积/性能测量）后铺全量 |
| R-02 | `opencode mini` 第二前端 | cli/handlers/mini.ts | P1 | **done（beta.4）** |
| R-03 | `acp` 编辑器协议（Zed/Neovim 等） | cli/handlers/acp.ts | P1 | beta.4；**已定栈（librarian）**：官方 `agent-client-protocol =3.3.0`（Apache-2.0，Zed 自家客户端同款）+ `stdio` feature + v1 builder API + `unstable_session_fork`（v2 参照含 fork，parity 必带）；钉精确版本、升级需回归（1.11.0 收紧了接收严格性）；能力宣告精确对齐 `acp/service.ts:106-130`；桥接层映射到现有 oc-client SSE/权限应答地基 |
| R-04 | **Web UI**：`serve` 托管 `packages/app`（PWA）+ 前端功能等价 | packages/app + serve 静态托管 | P1（大体量） | beta.5 起、跨多批 |
| R-05 | worktree UI | TUI/worktree | P2 | **done（beta.6）** |
| R-06 | 声音 / OS 通知 | TUI C2 类 | P2 | **done（beta.6）**：v2 触发矩阵+OSC 9/99/777+5 资产 vendor；播放镜像为系统播放器（登记分歧：v2 为进程内 miniaudio） |
| R-07 | 图像支持（粘贴/显示 kitty/sixel） | TUI/app | P2 | **done（beta.6）**：路径粘贴→附件 chip→data-URI→引擎（anthropic/openai 图像块已有 golden）；显示 kitty APC 分块/sixel 透传/占位回退，`session.image_preview` 默认关 |
| R-08 | 半块字形、子像素 RGBA 渲染 | TUI C2 类 | P3 | **done（beta.7）**：blocks 图像回退（v2 `resolveImageRenderProtocol`→"blocks"：PNG 解码→2×2 象限量化→Unicode 块字形+真彩，逆向 @opentui/core 0.5.14 二进制探针固化算法与已测分歧）；mini 退出 splash 半块 `_`/`^`/`~` 渲染（splash.ts cells/draw）；「子像素」结论：v2/opentui 无该术语机制，可观测面=blocks 子单元格 RGBA，`drawSuperSampleBuffer` 系列原生未用（wire 不可观测，仅文档化） |
| R-09 | PTY 守护跨重启交接 | core pty | P3 | **done（beta.7）**：`oc-core/persistent_pty.rs` 移植 v2 交接生命周期（`daemon.ts:403-428` 阶梯：过期→注册发现→协议→实例→锁存活→票据/状态），`handoff()` 真票据+`state.json` 终端快照，`adopt()` 重挂到继承目录（inode 保锁迁移 `daemon.ts:435-444`）并恢复重放窗口（子进程随旧服务器死亡= v2 守护死亡语义，标记 exited）；`oc-server/boot.rs` 消费 `OPENCODE_PTY_HANDOFF`（`server-process.ts:46-57`）；守护进程边界本体（socket/子进程保活）仍属 Phase 9 |
| R-10 | rmcp 0.5 elicitation | 协议库升级 | blocked（上游） | 跟踪 |
| R-11 | 平台矩阵扩展（aarch64/musl/macOS/Windows） | 非功能差距，单独决策 | P3（待用户定时间） | — |

## 三、行为分歧（消项或给出不可消的理由）

| 编号 | 现状 | v2 行为 | 处置 |
|---|---|---|---|
| D-01 | ~~JSONC 配置写回：拒绝+`--force`~~ 已修复：注释保留写回 | 保留注释重写 | **已修**（beta.4）：`oc-core/src/config/jsonc_edit.rs` 移植 jsonc-parser 3.3.1 setProperty/format（拼接+行级重缩进，字节级对齐已用 37 例 golden 语料钉死）；`--force` 保留为 rust-extra 覆盖 |
| D-02 | 非空草稿时 leader 和弦不触发 | 和弦挂起输入框后触发 | **done（beta.5）** |
| D-03 | 插件 hook 可变对象 copy-in/copy-out | 引用可变（进程内） | **spike 评估**：bun 宿主内对象代间同步或等价语义；无法等价则永久登记并给出隔离性收益说明 |
| D-04 | 请求 schema 校验仅 2 路由 | 全路由 400 形状 | **修**：按 openapi 契约生成全路由校验电池，beta.5 |
| D-05 | /help clamp；模型 release-date 精细序简化；input_paste preventDefault 未移植 | 各自精确行为 | **done（beta.6）** |
| D-06 | TUI 本地状态 local→stable 目录不自动迁移 | 出厂 stable | **修**（一次性迁移逻辑，参照 v2 的 v1→v2 套路），beta.4 |
| D-07 | G11-F1 首连快照/流竞态理论窗口 | v2 存在同类（行为对齐） | **closed by proof（beta.7）**：确定性仿真套件 `oc-tui/tests/g11f1_proof.rs`（真实 Store+BroadcastTap 对）跑遍对抗序：`first_connect_snapshot_then_stream_overlap_is_exact` / `reconnect_resync_resets_display_and_ended_heals` / `complete_message_snapshot_never_appends` / `last_event_id_resume_mid_snapshot_is_idempotent` / `dedup_ring_boundary_exact_coverage` / `snapshot_queued_mid_stream_serializes_in_channel_order`。安全性由构造保证：delta 为 ephemeral（`registry.rs SESSION_EPHEMERAL`，`message_updater.rs` 文本仅在 `text.ended` 物化）⇒ 任何 fetch 不含在途增量文本，快照/流文本重叠不存在；+ store 完成守卫/4096 id 环/tap 独立环三重兜底。"重放先于快照"序经可达性论证排除（单通道 resync-先于流 + 游标=最后应用 id） |

## 四、验证体系补强（本轮盲区的制度化）

1. **应答型 PTY 测试类**：终端模拟器对 OSC 11/DA 给真实应答——B3-04 落地后成为 TUI 门禁一环。
2. **真实语料库**：以脱敏后的真实 v2 库快照为只读 fixture 跑引擎/DB 集成（NULL 分布、大库性能）。
3. **终端矩阵冒烟**：tmux / kitty / Windows Terminal 差异面纳入 TUI_SMOKE 可选电池。
4. **长时浸泡**：serve+TUI 数小时级 soak（内存/句柄/事件序）。
5. **双端差分测试规范**（2026-10-11 用户指令升格，**TUI parity 唯一裁决标准**）：

   > **同目录、同操作、双端并跑、逐步全量一致。**

   - **双端并跑**：v2.0.24 参照 TUI（钉版源 `/opt/opencode-ref` 重建）与 rust TUI 各占一个 PTY，同尺寸（80x24、120x30 起）。
   - **同目录**：同一项目 cwd 与种子文件（git 仓+文件名完全一致）；各自独立 home/data 目录（DB 不可共享）；同一 stub provider、同模型名、两端配置等价——页脚/提示等 chrome 也须可比。
   - **同操作**：同一份 JSON 操作脚本（文本/按键/SGR 鼠标/leader 和弦/等待），按相同节奏喂两端。
   - **逐步全量一致**：每步后 cell 级逐格比对（字符+样式摘要）。**chrome 区域（标签条/页脚/对话框/输入框/侧边栏）逐字节一致**；每步记录 identical / 首差格子 / 双端行快照。
   - **结构对比模式**：模型输出区域文本两次运行天然可差，不要求逐字一致——归一化为标记后比对**结构几何**：行数-内容长度关系、节头与内容块距离（如思考展开后 Thought 头行↔内容块间距）、边距与缩进关系。两种模式结果分开报告。
   - **归一化最小化**：仅允许登记过理由的归一化（模型文本归一、两端不可避免的进程级差异）；禁止以归一化掩盖分歧，新增归一化必须在 NORMALIZATION 文档登记。
   - **裁决权**：自建 golden 降级为防回归手段；TUI parity 以双端零 diff 为准。mismatch 三分类：确定性 bug（必修）/ 参照端时序噪声（重跑分离）/ 已登记合法分歧（须论证）。
   - **入口**：`OC_TUI_DIFF=1` 门控（参照构建缺失时跳过，同 OC_TUI_SMOKE 模式）；语料含 boot、编辑+提交（stub 固定回复→转录比对）、标签生命周期、/help、palette、主题切换、思考展开（结构模式）、标签/+/✕ 悬停（SGR）、resize。

### 实测发现台账（dogfood 波1，beta.9，22 项）

- **波 A 已合并覆盖**：F1-F5 编辑键簇、F18 escape 中断、F21 滚动、F20 队列（oc-tui-core）
- **dogfood1 泳道已修**（lane/dogfood1，PTY `pty_restore.rs` 五场景 + 单测钉死）：F19 恢复会话转录冻结（根因：`fetch_messages` 吃服务端 desc 序未反转——v2 data.ts:1633-1637 `order:"desc"`+`toReversed()`；修复后时间序+Wave A 粘底生效）、F12 /sessions 打开空白（run-loop loop-fetcher 按开页取 `message.list`，store `MessagesSnapshot` 合并保住在途行）、F6 多行用户消息（`wrap_lines` 硬换行）、F7 跨会话串染（渲染纯函数等价测试钉死）、F8/F10/F22 模型链（显式→durable(session.model)→config.model→recent→models[0]，submit 前 commitModel 先行，session.new/clear 继承）、F16 leader>e（Wave A 已激活，PTY 钉死）、F11 错误面板漏键（带标题 error toast 模态直至关闭；无标题跳过类不拦截）、F9 home 标签条（app.tsx:576-579）、F13 /sessions 打开重取、F14 palette 按 value 去重；F15（窄宽换行）登记未修——需选项行高模型重构，非快赢
- **方法论教训**：审计将 F16 标记为 done 但实测死亡——「实现存在」≠「链路接通」。此后所有交互面修复必须带 PTY/实测级验证，审计状态仅供参考起点。

### 验证方法论盲区登记（持续追加）

- golden 帧证明的是「渲染保真」，不证明「屏幕组成模型」——标签条模型错误（全历史 vs 打开标签）与启动路由错误（恢复会话 vs home）在帧级逐字节自洽下存活至 beta.7 用户实测才暴露。对策：交互模型类（路由/标签/列表取数）必须有参照源码级对照（file:line），不能只靠 golden 自证。

## 四点五、开发纪律（结构性强制，2026-10-11 起）

本机与线上服务（newapi、49374）共享 16 核/22G。历史事故：未限流 cargo 构建把 CPU 打满一次；root 直跑 cargo 弄脏属主三次。纪律已从「写进泳道提示词」升级为**结构性强制**：

- **`tools/dev-gate.sh` 是一切本地构建/测试命令的唯一入口**：强制 eternal130 身份执行、装入 systemd 瞬态 scope（`CPUQuota=400%` 四核硬顶 + `MemoryMax=10G` + `TasksMax=512`）、1 分钟负载 ≥6 拒绝启动。已实测 `cpu.max=400000 100000` 生效——任何泳道漏写 `-j4`、或并行泳道叠加，物理上都不可能再打满机器。
- CI（GitHub Actions）豁免（不在本机跑）。
- 其余冻结纪律继续有效：绝不触碰 49374 与线上目录；测试一律 `127.0.0.1:0`；/tmp/opencode 顶层用 PID 唯一名；无裸 PID 杀进程；开发测试只写沙箱，不写用户真实数据目录；DNS 探针（/tmp/opencode/dns-probe.*）不属于构建产物，勿清理。
- 之后所有泳道规格必须写明「本地命令一律经 `tools/dev-gate.sh`」。
- **泳道写入隔离（2026-10-11 二次事故后新增）**：多泳道并行时各自使用独立 git worktree（`git worktree add`），整合时串行合回 main——共享单一检出目录已造成一次泳道文件被误删。共享 main 检出的泳道必须：早提交勤提交（WIP 小步提交，后续 squash）、只碰自己范围的文件、最终提交前 `git status` 复核。

### 架构决策（2026-10-11，用户裁定 B）：交互契约层 `oc-tui-core`

审计确认缺口集中于 opentui 的交互半（键位分层引擎/鼠标管线/文本编辑原语/which-key/滚动接线），渲染半已被 golden 逐字节证明。裁定：**不做全库移植**，把交互契约按 v2 语义移植为独立 crate `oc-tui-core`（键位引擎补全至 opentui 分层语义、crossterm 鼠标捕获+命中+悬停、选区/undo/词移动编辑原语、滚动标准件），现有屏幕迁移其上；波 A 落于该 crate，波 B/C 消费。框架级 parity 测试（分层解析序、鼠标命中、焦点遍历）替代逐表面 PTY。

## 五、批次节奏

- **beta.3**（当前）：B3-01..04 → CI → 发布 → 真机验收。
- **beta.4**：R-01/02/03 + D-01/02/06（高感知日常项）。
- **beta.5**：R-04 Web UI（先架构侦察+spike：托管方式、构建链、鉴权流、与 API 的实时通道；预期跨批次）+ D-04。
- **beta.6**：R-05/06/07 + D-05。
- **beta.7+**：R-08/09、D-03 结论落地、R-11 按用户排期。
- 验收全部通过后发正式 **v2.0.24**（发布即终态，台账清零或带已具名豁免）。

### R-04 Web UI 架构决策（2026-10-11 侦察后裁定）

事实基础：Rust 侧托管契约已全量就绪（`oc-server/src/assets.rs` 的 serveUI 移植：SPA 回退、`_assets|icons` 404、immutable/no-cache 拆分、CSP+内联脚本哈希、nosniff、HEAD；`auth.rs` Basic+`auth_token`+同名端口 cookie；`pairing.rs` 配对；SSE 事件流）——**唯一缺口是 `state.rs` 的资产 map 为空**。v2.0.24 不发布独立 app 产物：app（Solid+Vite8+PWA，`packages/app`，无任何 `@opencode/desktop` 依赖）构建产物在 v2 内以 brotli+base64 归档**编译期嵌入 CLI**；dist 不入库、必须源码构建。

裁定（镜像 v2 语义、适配我们的发布管线）：
1. **构建**：release workflow 新增 `web-assets` job——拉取钉版参照源码（anomalyco/opencode @ e7a34f09bf tarball）→ `bun install` → `VITE_OPENCODE_SERVER_MODE=origin bun run build`（packages/app）→ 逐文件 brotli(q6) → 归档产物 job 间传递。不在开发机构建（无本地构建纪律），不依赖任何已安装二进制。
2. **嵌入**：Rust 侧新增 `oc-server/build.rs` 读归档（`include_bytes!` 等价），编译期写入资产 map；`OPENCODE_APP_ASSETS` 环境可指向磁盘归档（对应 v2 的 `OPENCODE_LOCAL` dist 回退），运行时惰性解压。
3. **一致性纪律**：归档携带 provenance 清单（ref commit、app 构建输入哈希）；`_assets` 内容哈希与 SW 完整性校验天然拒绝混版部署。
4. **验证**：浏览器端到端冒烟（serve → 登录屏 → Basic/配对链接登录 → 会话列表/发送/PTY WS）；套件内用微型 fixture 资产 map 钉版 headers/CSP/回退行为（不依赖真实归档）。
5. 体积预期：含 ui 包字体/图标/音频，dist brotli 后估计数 MB—十余 MB 级，随二进制分发（与 v2 嵌入 bun 单文件的做法对等）。

## 六、变更记录

- 2026-10-11：初版。范围扩大（Web UI 入、桌面端维持排除）；功能缺失全量消项令；beta.3 批次定义。

- 2026-10-11（beta.5 批）：D-01/02/06、B4-H3、R-01b(裁剪)、热点二号（整数参数/消息端点 panic 容器/目录容错 404）落地；已知小项：m1 双胞胎测试共享 artifact 路径的并发竞态（登记待修）。

- 2026-10-11（方法升格）：**双端差分测试规范入册**（第四节 5）——「同目录、同操作、双端并跑、逐步全量一致 + 结构对比模式」成为 TUI parity 唯一裁决标准；自建 golden 降级为防回归手段。触发：✕/+ 悬停高亮「凭理解实现」与 v2 实际语义不符——凡视觉/交互面，以与钉版 v2 实时对照的零 diff 为准，不再凭解读实现。

- 2026-10-11（差分首跑）：rig 落地（tools/parity/tui-diff，`OC_TUI_DIFF=1` 门控）并入 main；首跑 656 步：648 不匹配 / 8 一致 / **0 纯文本差、全部确定性**（零时序类）。25 项分类清单见 `tools/parity/tui-diff/REPORT.md`：主罪 S0=SGR 256 索引降级缺失（TERM=xterm-256color 下 v2 发 idx 色、我们恒发真彩）+ 主题值漂移（#080808 vs #0A0A0A）——污染每一格样式；其余=S1 模型链（boot 选择无视 config 默认/页脚左槽/输入框模型行/占位符池）、S2 模型对话框五项、S3 转录 chrome（自动标题/统计行/思考时长后缀/翻页滚动量/leader+l 空操作）、S4/S5 几何（对话框统一矮一行族/60 列页脚溢出/wordmark 居中差一）。正面项：标签 ✕ 悬停生命周期、转录/思考几何、resize 重排已达 parity。修复波序：A(S0 颜色层，独占 golden 全量重生成) → B/C/D 并行（boot chrome+模型链 / 转录+滚动 / 对话框几何族）；语料二期（活会话 tab 和弦/多会话/宽屏侧边栏对照）并行扩展中。
- 2026-10-11（侧边栏）：v2 会话侧边栏移植至实况路由（lane/sidebar 并入）——Auto 默认（宽>120 才显示）、42 列 dock、子代理抑制、`<leader>b` 切换+配置持久化、窄宽浮层、插件 setSection 端到端、PTY+goldens 钉死。

- 2026-10-11（波 A 落地）：S0 颜色层修复（lane/s0-color 并入）——RGB→xterm-256 降级实现在绘制缝（`render/color_mode.rs`：能力矩阵照抄 v2 raster 反汇编+实证补全、144 色 sweep 全对、最近平方距离+最低索引平手规则）；**主题值漂移证明为幻影**（量化遮蔽）；golden 零变动（出口降级与 v2 同构）。样式分歧从每格皆错塌缩至 11 对、各归档；新登记：NEW-S1a 表单域样式、NEW-S2a 对话框背板、NEW-S3a 转录前景 idx15/idx255；follow-up：mini 滚回区用独立 oklab 量化器（mini/theme.ts:95-110），主 TUI 降级不适用于 mini——待单独对齐。修复波 B（模型链+模型对话框 #3#7-#11）/C（屏幕 chrome #4#5#6#12#13#14#20+NEW-S1a/S3a）并行中；D（对话框几何族+居中）/E（滚动语义+leader+l 浏览器）排队待合并位。
