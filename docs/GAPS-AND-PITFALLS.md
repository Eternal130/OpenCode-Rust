# 缺口与踩坑登记册（GAPS & PITFALLS）

> 建立：2026-10-23 ｜ 来源：ROADMAP 变更日志、FINDINGS.md、PORT_REPORT.md、docs/host-issues.md、移植进度审计（2026-10-23）
> 用途：§1 是**当前仍然开放**的缺口（ID 与 ROADMAP.md 台账一致，此处为汇总视图）；§2 是开发全程踩过的坑，每条按「事故 → 根因 → 现行规则」沉淀；§3 是现行防复发制度速查。
> 原则：坑的价值在于结构化修复——本册只收录已转化为规则或已明确登记的教训，不收纯流水账。

---

## 1. 当前缺口（截至 2026-10-24，v2.0.24 gap burn-down 收尾之后）

> **2026-10-24 burn-down 收尾**：计划 docs/superpowers/plans/2026-10-23-v2-gap-burndown.md 的全部 lane 落地后，§1 所列功能缺口**除具名豁免外全部闭环**（台账：.superpowers/sdd/2026-10-23-v2-gap-burndown/progress.md）。发布就绪判定 = 台账清零或仅余具名豁免：R-04 前端范围外裁定、R1–R5 hook 注册豁免、(b)-类随机 coin、**F15 残留五项已全部于 2026-10-26 关闭（QR 编码器 vendor oc-uqr + URL word-wrap + ≥96 终端宽分屏 + Enter 关闭〔登记过期证实已移植〕+ h<36 滚动对齐；见 §1.2 F15 行）**、**console variants/模型级 package/native-protocol 已于 2026-10-26 关闭（deepwork cv-boot：模型级 variants per-id overlay 合并 opencode.ts:282-293 + package 阶梯 :272 + native-protocol 绑定 package→family_of/resolve_specifier、CORE_KEYS/凭据剥离、52 门内置表对账快照；残留具名豁免=非 native `aisdk:` 包桥接，需 plugin-host LanguageModelV3-over-RPC 基建〔ref 走进程内 runSDK hook，aisdk.ts:373-414〕，port 保留精确 UnsupportedProtocol 报错——同报错亦覆盖**任意非内置 npm 包**的 ref loadPackage npm 安装通道〔provider.ts:112-133〕，ref 对不可解析包同样干净报错，port 报错为正确对等物）、OPENCODE_FAST_BOOT/skipInitialLoading 已于 2026-10-26 关闭（CONFIG-E3：Boolean(env) 语义 + pill 永不挂载，app.tsx:351/1389-1391 平价）。

> **2026-10-24 G 系列 burn-down（本日第二轮，deepwork g-series）**：G-01～G-17 全部处置完毕——除 G-09（维持登记：双侧一致的 LSP 非回归）与 G-14（六场景浏览器冒烟本地 3×全绿 + runtime-gates 周 cron 已挂，**完成待首周 cron 实跑观察后方可宣称"功能一致已验证"**）外全部闭环；G-11 收敛至无损集（clipboard/paste 评估/diff 逻辑），sound/syntax/terminal_pane 阻塞上游并附解锁条件（docs/FRAMEWORK-CONVERGENCE.md）；G-06 复核修正登记册"本体已备"的错误声明。三道 oracle gate 均 GO-WITH-FOLLOWUPS、followup 全部消化；全 workspace 4190/4190 绿。G 系列已同步 ROADMAP §2 台账（2026-10-24）。原有具名豁免（R-04、R1–R5、(b)-coin、F15 残留、console variants 延后、FAST_BOOT）维持不变。

### 1.1 功能缺口

| ID | 缺口 | 状态 | 说明 |
|---|---|---|---|
| R-10 | rmcp 0.5 elicitation 能力 | **已闭环（2026-10-24）** | 重裁定为真缺口（v2.0.24 已接线 elicitation，packages/core/src/mcp/index.ts:231+）：rmcp 0.5.0→3.7.0（60728e3）+ 客户端 elicitation 按 v2.0.24 接线（fd0f591） |
| R-11 | 平台矩阵（aarch64/musl/macOS/Windows） | **已完成（2026-10-24）** | 6 目标发布矩阵（linux x64/arm64 gnu + musl×2、darwin-arm64、windows-x64 实验性）+ 每周 runtime-gates cron；9cf480d |
| R-04 | Web UI 前端本体 | 范围外（裁定，2026-10-24 复核维持） | 静态托管 + 发布资产内嵌已完成；前端不移植（直接内嵌 ref 构建产物，oc-server/src/app_archive.rs:44）——当日曾短暂反转为移植目标，经一致性分析后同日撤回，缺口收窄为 G-14 E2E 一致性验证 |
| — | PTY 守护进程边界（Phase 9） | **已实现（2026-10-24）** | 设计 docs/superpowers/specs/2026-10-23-pty-daemon-design.md（DESIGN-GO）；D1 oc-pty 守护二进制（d343a12）+ D2 客户端 transport/frontend（f98233f）+ D3 11 个进程级测试（f06245f，旗舰=子进程在 server 被杀后存活）；oc-pty 随 release 产物分发，OC_PTY_DAEMON=0 留一个 release 窗口 |
| — | 企业集成 RS256/JWKS 签名校验 | **已实现（2026-10-24）** | ring 实现（无新加密依赖），v2 chatgpt.ts:439-459 平价，完整失败矩阵（伪造签名/错 iss/aud/过期/缺 claim/nonce 不匹配/未知 kid+冷却轮换/alg 混淆）；合并 6722828 |
| — | Skill URL 源下载通道 | **已实现（2026-10-24）** | v2 discovery.ts 移植：index.json 协议、sha1 哈希缓存、版本门控原子刷新、document-order 碰撞语义；合并 c1a7001 |
| — | console capability/cost/limit 合并 | **已实现（2026-10-24）** | v2 opencode.ts:245-315：cost 整体替换、limit/caps 逐字段覆盖、baseURL 优先级（含 provider 级删除继承），含 listing surfaces + default fallback；合并 14d8b76 |
| — | oc-client sse 测试时序敏感 | **已闭环（2026-10-24）** | 确定性 readiness-sync 重写（onActivity 同步替代固定睡眠），20/20 + oc-client 57/57；合并 31a6561 |
| G-01 | 内建 agent `explore` 未播种 | **已闭环（2026-10-24）** | explore 播种（Subagent 可见，PROMPT_EXPLORE + description 逐字节 ref 移植，权限按 ref 顺序 deny-all→allow read/grep/glob/webfetch/websearch/shell→`*.env` ask 组→subagent deny→external_directory ask）；合并 a1666c9 |
| G-02 | 内建 agent `summary` 未播种 | **已闭环（2026-10-24）** | summary 播种（隐藏 Primary + PROMPT_SUMMARY + deny-all）；合并 a1666c9 |
| G-03 | TUI 宿主剪贴板读取 | **已闭环（2026-10-24）** | oc-tui 剪贴板收敛到 opentui-core 引擎（OSC52 写字节平价测试）；`ClipboardSource` 可插拔读臂（host 工具层 pbpaste/xclip/xsel/wl-paste + OSC52 查询窗，preferredTypes image/png+text/plain），ctrl+v 图像→attachment chip（filename "clipboard"）、文本→既有粘贴流、无 host 臂不回归 bracketed paste；合并 5edbd8f |
| G-04 | TUI 插件 UI 槽位不全 | **已闭环（2026-10-24）** | 挂载路径 2→8：+home.footer(.status)/prompt.footer(.status/.file)/sidebar.footer（行级面 + PromptFooterInput props 透传，oc-plugin-host render_slot_claim_with_props）；具名豁免：session.panel（交互式 PanelInput API，无 shipping plugin 使用）、session.composer.top（需新布局面）；合并 82e8689 |
| G-05 | 配置/插件热重载 | **已闭环（2026-10-24）** | oc-core WatchedConfig 接线 server boot（100ms debounce + deep-equal 短路）→ config.updated bus/SSE（ref config.ts:276 平价）+ location graph 失效刷新；插件源 fs-watch + supervisor resolve diff 驱动动态世代装载/卸载（PluginHost::add_entry/remove_entry，坏插件含隔离 dyn_entries 实测）；已知限制：动态 entry 无 boot 期 ctx.options（文档化）；合并 c1da274 |
| G-06 | v1→v2 交互式迁移引导 UI | **已闭环（2026-10-24）** | 复核发现登记册"本体已备"不实（v1_migrate.rs 仅主题、storage.rs 仅 channel 拷贝）——实际迁移变换/路由/客户端已在 oc-core database/v1_migration.rs（1951 行）；补齐 boot 期 runner（KV 门控、失败不倒服务）+ 4 场景电池 + TUI 轮询 overlay（migration-overlay.tsx 移植，运行中右上 spinner+计数，错误 toast 10s）；合并 de161be |
| G-07 | TUI devtools 栏 + storybook | **已闭环（2026-10-24，裁剪具名）** | registry + 底部 devtools 栏（server/theme/tools/ui 面板，debug.devtools 配置门控，禁用时 golden 帧字节不变有守卫测试）+ Experiments palette 接线 + state dump；裁剪（代码内具名）：无工具清单（仅最近调用）、无 /api/info 版本模型、storybook 不做（fixture 路由，开发态）；合并 83aa520 |
| G-08 | 语法高亮缺 4 门尾部语言 | **已闭环（2026-10-24）** | lua(=0.5.0)/scala(=0.24.0，pin 依据注释)/make(=1.1.1) 上游 crate + vim 第三方 vendor（third_party/tree-sitter-vim @v0.8.1），.scm 按 ref pin 版本 vendored，目录 20 门；顺带修 engine 分块解析偏移 bug（非零 offset 曾返回空串截断解析）；合并 f0c1791 |
| G-09 | LSP 运行时 | 登记（非回归） | 两侧一致：ref 亦仅配置 schema + TODO（core write.ts:44、edit.ts:107），无客户端运行时；未来补齐时需双侧对表，"完成"口径不得宣称 LSP 可用 |
| G-10 | fork 编排迁回 oc-core | **已闭环（2026-10-24）** | boundary + instruction 快照迁入 oc-core session/fork.rs（单读事务；与 SessionStore::boundary 的 seq/存在性探测语义分歧文档化），handler 仅剩鉴权/发布/回读、零 SQL；核心级单测（boundary 四腿 + 快照升序）；合并 7ab1270 |
| G-11 | oc-tui 自研框架件收敛至 opentui-rust | **已处置（2026-10-24；2026-10-25 上游事实修正）** | 已收敛：clipboard（G-03 同 lane）、paste（评估后**有意不采**）、diff 解析逻辑（parse_patch 单源化，golden 字节不变）。修正（2026-10-25）：①sound 已收敛（opentui rev 18e2904 上游落地真音频 → 本仓收敛 lane 合并 3f436ba：device-audio feature + OpentuiSoundPlayer 适配器经 SoundHalf seam，inproc 引擎与 third_party/miniaudio-sys 删除，非 Linux spawn 回退保留）；②语法目录 opentui（TS）同样只有 ~5 门（ref 全目录是其 TUI 自带 wasm 运行时，非 opentui core）——20 门目录是本 port 自身优势，收敛仍为回归，维持阻塞；③terminal_pane **非 opentui 功能**，无上游对应体，**永久本地保留**。渲染耦合项修正（2026-10-26 lane R0）：渲染栈 strangler 迁移（M1–M6 + M-theme + M-engine，docs/migration-m1-tabstrip.md）**已完成且 hybrid 架构（opentui raster → cell-copy → ratatui Frame blitter → 本地 ANSI 序列化）已被宣告为目标架构**（docs/FRAMEWORK-CONVERGENCE.md "Rendering-stack migration"），"渲染耦合项统一卡在渲染栈迁移、未排期"的旧表述作废——syntax 仍按自身双重前置条件（语言目录 + 兼容 span 输出）维持阻塞，line_number 改为 pending R3 字节平价 spike（失败则与 terminal_pane 同列永久本地保留），terminal_pane 维持永久本地保留；git rev pin 规则维持；合并 0806272 |
| G-12 | oc-schema 缺 7 类事件类型化模型 | **已闭环（2026-10-24）** | pty/persistent-pty/filesystem/reference 四模块（ref 字段名/casing/可空性），9 个发射点全部类型化，golden 对表测试进常规 cargo CI + persistent-pty 与 terminal_wire_json 无漂移测试；pty 事件 info exitCode 对齐 ref casing（无内部消费方）；合并 304ae9e |
| G-13 | CLI 枚举校验偏松 | **已闭环（2026-10-24）** | 五处 clap ValueEnum（upgrade --method 七选项+rust-extra local-file/github 保留、run/auth list --format、session list --format、debug paths）；--force 裁定：保留为文档化 rust-extra（默认路径即 v2 平价的 comment-preserving 写）；合并 f0c1791 |
| G-14 | Web UI 功能一致性 E2E 验证 | 开放 | 前端 = 内嵌 ref 构建（app_archive.rs:44），不移植；一致性等价于服务端契约一致性。已对表：141 路由（含错误 _tag）、SSE 帧字节级、WS+ticket、auth/CORS/location。残留 4 类风险：① handler 长尾仅签名级验证（高频端点才做了 body 级）；② G-12 七类事件无 schema golden 对表，UI 按 shape 解析会**静默坏**；③ 时序/边界（分页 cursor、Last-Event-ID 断线重连、竞态）只能真跑；④ 资产钉定漂移（ref 前端演进需重建嵌入）。闭环 = 六场景浏览器冒烟（auth、会话/prompt 流式、文件树/mentions、PTY 面板、权限弹窗、事件重连）挂 **runtime-gates 周 cron（GitHub Actions runner，不占本机；首次稳定化本地错峰跑一轮再去 flaky 后再上 cron）**；G-12 类型化后的 golden 对表为纯 cargo 测试，进**常规 PR CI**（秒级确定性，勿等周 cron）；两项绿后方可宣称"功能一致已验证"。**2026-10-27 复核：首周 schedule cron 已于 2026-10-26T08:44Z 实跑（run 40294359979）且 `G-14 browser smoke` job EXECUTED=success（同 run §12.3 runtime gates、p10 battery 亦绿；30m soak/release smoke 为 skipped 属调度设计）——"待首周 cron 实跑观察"条件已满足** |
| G-14 进展 | 浏览器冒烟 chunk 1 落地（2026-10-24，lane P3-E2E） | 进行中 | harness + 场景 1-2 绿：`tools/parity/tests/browser_e2e.rs`（chromiumoxide 0.9 dev-dep，headless `--no-sandbox` 新 profile；OC_BROWSER_E2E=1 运行时 opt-in；资产解析顺序 OC_BROWSER_APP_ASSETS → /tmp/opencode/ref/packages/app/dist → app-archive.json → 生成的 minimal contract page；本地实测 real-spa-dist 与 minimal-contract 两模式均绿）。S1（auth）：匿名 /api/info 401 + WWW-Authenticate Basic（harness 侧 minih——页内 fetch 带 sec-fetch-mode: cors 不发 challenge，auth.rs challenge_request 语义）+ Basic/auth_token 两路 200 + 真实 SPA shell 渲染。S2（流式）：页内 EventSource(auth_token) 收到 `session.text.delta` 全生命周期帧（v2 无 `message.part`，schema session-event.ts:405），页内 POST /api/session/:id/prompt（注意：**prompt 端点**，/message 是 GET-only 列表），最终 assistant 文本匹配 ScriptedFakeLlm。剩余 chunk：场景 3 文件树/mentions、4 PTY、5 权限弹窗、6 Last-Event-ID 重连 |
| G-14 进展 2 | chunk 2 落地（2026-10-24，lane P3-E2E）：场景 3+6 绿 | 进行中 | S3：`RustSpawn::project_cwd` 把服务端 cwd 钉在种子工作区（默认镜像 ref/packages/cli）；页内断言 `/api/fs/list`（src/+README 可见）、`/api/fs/read/src/main.rs` 内容匹配、`/api/fs/find?query=`（SPA mention/context 自动补全走 `fsFind`，mock-api.ts:206）命中种子文件。S6（契约事实，两端源码一致）：①`/api/event` 为 **live-only**——帧是 `data:`-only 无 SSE `id:` 字段（sse.rs::frame 镜像 event-feed.ts::frame），浏览器 EventSource 永远不会带 Last-Event-ID，重连不重放断线窗口（页内实测：断线期间事件不回放、重连后 live 事件正常）；客户端靠 snapshot 重同步。②真正的 cursor 恢复面是 `GET /api/experimental/session/:id/log?after=<seq>`（ref session.log，handlers/session.ts:611）：页内 fetch 流读取实测 after=seq 重放严格大于 cursor（minSeq 11 > cursor 10）、无重复 id、`log.synced` 水位前进（10→19）、断线窗口事件全覆盖；`session.text.delta` 为 live-only 非持久（持久日志只有 text.started/ended 生命周期对）。harness 事实：nextest 每测试独立进程 → 场景串行化用跨进程 flock（目标目录 .scenario-lock），否则第二个并发 Chrome 在 gate 任务限额内 pthread_create EAGAIN 挂掉。剩余 chunk 3：S4 PTY 面板、S5 权限弹窗 + 稳定化 + runtime-gates cron 挂载 |
| G-14 进展 3 | chunk 3 落地（2026-10-24，lane P3-E2E）：六场景全绿，cron 已挂 | **完成（待 cron 首跑观察）** | S4（全部腿页内）：POST /api/pty 建 sh → live 流 `pty.created`（typed info：id/status running/pid 数值/无 exitCode）；`POST /api/pty/:id/connect-token`（需 `x-opencode-ticket: 1` 头）出 ticket；ticket-exempt WS `GET /api/pty/:id/connect?ticket=` 附着——stdin=text 帧、stdout=text 帧、单条二进制 metaframe `0x00||{"cursor":N}`；`exit` → `pty.exited{exitCode:0}` + WS close 1000；PUT 更新出 `pty.updated`。S5：build agent 默认 `*/*` allow（oc-schema/agent Info::default）→ 用**会话级 ruleset**（POST /api/session 带 `permissions:[{action:"shell",resource:"*",effect:"ask"}]`，SessionRuleResolver 会话规则覆盖 agent 规则）让 shell 工具调用触发 `permission.asked`（typed：per_ id/sessionID/action/resources）；页内 `POST /api/session/:sid/permission/:rid/reply {"decision":"once"}`（204）→ `permission.replied` + execution.succeeded + transcript 带 shell 输出标记。稳定化：全六场景连跑 3×全绿（12.8s/12.0s/12.0s，每场景 1.5-9s 串行）；minimal-contract 回退模式亦 6/6（9.7s）。cron：runtime-gates.yml 新增 `browser-e2e` job（默认 ON + schedule 臂；镜像 release.yml web-assets 暂存真 SPA dist：同 PARITY_REF tarball + 定钉 bun 构建；dist 暂存失败 → battery 降级 minimal-contract、summary 记 SKIPPED-GAP；无浏览器 → 显式 skip-gap；gates-summary 增行含 `success-minimal-contract` verdict）。剩余：观察首周 cron 实跑 |

### 1.2 行为分歧（待裁定 / 登记未修）

| ID | 分歧 | 处置状态 |
|---|---|---|
| F15 | TUI 窄宽度换行 | **已全部解决（2026-10-24 主体；2026-10-26 f15 残留五项全清零）**：主体——live ref 证实为**截断而非换行**（dialog-select.tsx:885-919 两段式截断已 1:1 移植）；DialogPair 重移植为自定义换行对话框（c2f59d7）。残留五项 2026-10-26 处置（f15-a/f15-b lane + 合并后续修，oracle gate 见 .slim/deepwork/f15-burndown/oracle-gate.md）：①QR 编码器——vendor `third_party/oc-uqr`（ref 钉版 uqr 0.1.3 忠实 Rust 移植，golden 由 ref 自带 node_modules 生成、字节等同），TUI DialogPair 分屏右侧列（dialog-pair.tsx:39/44/45/75-84）+ CLI pair 输出（pair.ts:24-42 border 2 缩进）；②<54 列 URL——ref `<Link>` 默认 word wrap（ui/link.tsx:33-39 + opentui TextRenderable wrapMode:"word"），port 硬裁剪已改 word-wrap、全 URL 可拼接恢复；③≥96 列横排——门槛=**终端**宽（:39）非内容宽（原登记根因），文本列固定 29 格（:45），QR 列 flexGrow 右置（合并后集成修 e2e380d：QR 置于列侧否则 h<36 滚动预算截掉）；④Enter 关闭——证实登记过期，help/alert/confirm/export 四处早已移植（dialog.rs:189/455/479/1142）；⑤h<36 滚动——删除 port 自创 min-5 下限（SE-F6 主张无 ref 出处），对齐 `min(rows, floor(h/2)-6)`（dialog-select.tsx:241）；pair 预算 max(8, h−h/4−6) 与 :104-112 逐字一致。PTY 证明：100×30 分屏 QR 可见 + 46 列窄宽 URL 换行 |
| E12-F1 | 会话列表搜索：子会话泄漏 | **已裁定非分歧（2026-10-23 live ref 复核）**：live v2.0.24 从不渲染子会话行（服务端 `parentID:null` + `options()` 再过滤）；首轮 F1 系裸谓词孤立测试的伪影。证据 docs/e12-live-ref-recheck/（12/12 查询 parity） |
| E12-F6 | 空白查询返回空 | **已裁定：裁剪后空查询 = 全量列表**（live ref 与本port一致返回全部父会话行）；谓词层保留子会话但渲染层永不含子行。与我们 `filter_indices` 语义一致，无需改码 |
| E12-F4 | fuzzy picker vs 专用 substring 过滤 | **已裁定：substring（live ref 无 fuzzy：`zln`/`loginredirect` 均无结果）**；本port `session.list` 已挂 `.substring_filter()`——语义正确，闭环 |
| E12-F2 | 重音等价 | **已裁定：无重音折叠**（`cafe`↔`café` 永不互配；`café` NFC 查询只配 NFC 标题）——live ref 与本port byte-exact 一致，维持 pinned 语义 |
| E12-F3 | NFD | **已裁定：双方均不归一化**（NFD 查询对 NFC 标题零匹配），维持 pinned 语义 |
| E12-F5 | 空白折叠 | **已裁定：无空白折叠**（双空格标题不配单空格查询，双方一致），维持 pinned 语义 |
| E2 | boot-stretch 门行为断言 | **已闭环（2026-10-24）**：temporal gate 落地 + REAL 接线修复（StartupLoading pill 此前不可达——ready 改为在本地 boot settle 解析、不受 server handshake 门控）；reconnect 覆盖层 5s grace 0 锚点修复；4d16916 |
| R1–R5 | 插件 hook 永久注册分歧（aisdk 对象 / 返回后突变 / 交错序 / 非 JSON stash / retry 失败通道） | 永久登记——换取崩溃隔离，依据 FINDINGS.md §5 |
| — | pre-2.0.24 旧库升级路径 | **电池已落地（2026-10-24）**：合成 stepwise k=1..48 全绿 + 4 个 canary 边界（78976eb，常驻 CI；详见 MIGRATION.md §3.1）；**真实历史语料门已接 cron（2026-10-27，6797966）**：runtime-gates 新增 `p10-real-history` job（周 cron + 默认 ON）——维护者经 `OC_CI_V2_HIST_DB_B64` secret 供给真实 pre-2.0.24 db（gzip+base64），job 做 provenance 校验（migration journal 须 1..47 行，48/48 v2.0.24 写入形状**拒收**为错标）后跑 `p10_db_import OC_P10=1 OC_GATE_STRICT=1`；secret 未设时诚实 SKIPPED-GAP verdict（不绿不红、入 standing gaps）；语料获取本身仍是人工（用户数据不可伪造）。**首次启动前建议备份数据目录** |
| G-15 | fork 子行 `parent_id` 写实值 | **已修（2026-10-24）** | 子行改写 `parent_id = NULL`、`fork_session_id = <parent>`（ref projector.ts:151 平价）；失真 "CD-2" 注释重写为 ref 核验事实；消费方复核（subagent 父链行走、drain 顶层判定、jobs 子匹配）均向正确方向收敛；合并 a1666c9 |
| G-16 | fork entry 插入吞错 | **已修（2026-10-24）** | instruction_entry_initialize 返回 Result 并传播（orDie 语义：投影事务整体回滚→fork 500，总线 Err 回滚有测试）；批量 10 行 + 快照读 `ORDER BY key ASC`（随 G-10 移入 core 时保留）；合并 a1666c9/7ab1270 |
| G-17 | prompt `delivery` 校验偏松 | **已修（2026-10-24）** | parse_delivery 共享解析：absent→steer、"steer"/"queue" 原样、其余字符串/非字符串→400 InvalidRequestError/ParseError（prompt + synthetic 两路，均 E2E 覆盖）；合并 a1666c9 |

### 1.3 验证盲区（已知未自动化）

- **浏览器冒烟的口径说明（2026-10-24）**：`browser_e2e` 六场景在无 `OC_BROWSER_E2E=1` 时以早退 `Ok(())` 通过——nextest 计为 pass 而非 skip，报告"21 skipped"时勿把浏览器腿重复计为绿覆盖；真实绿凭据 = 带 env 的实跑（本地 3×全绿）+ 周 cron 的 gates-summary EXECUTED 行（首周观察 pending）。CI 首跑同时验证 runner 上 parity 测试编译是否落在 30 分钟预算内。
- 最严格的验证门是 **opt-in/manual，不在 CI**：OpenAPI 启动差分与 DB 互操作电池需 `OC_P10=1`，跨二进制/TUI 冒烟同理——声明的"empty diff"依赖手动执行，存在跳过风险。**P10 盲区已自动化（2026-10-24）**：runtime-gates workflow 挂每周 cron（self-staging jobs；schedule arms 已修复 cron input 强转 no-op 问题）。注意：workflow 在已知红项（GATE-DB-STABLE/NULLROWS/TUI-RENAME）关闭前保持红——按设计保持诚实。
- 旗舰插件（oh-my-opencode-slim）验收测试仅在有安装缓存的机器真跑，CI 自跳过（PLUG-D07 之后的回归正是它抓到的）。**已纳入每周 schedule（2026-10-24 起）**：omo staging 为 self-staging job，随 runtime-gates 周 cron 真跑。

### 1.4 独立审查新发现（2026-10-25，四域 oracle 审查；详见 docs/PORT_REVIEW_2026-10-25.md）

> 来源：core / server / TUI / AI+插件+CLI 四条并行独立审查泳道，全部以代码核实、不采信自述。以下为 PORT_REPORT §3 与本册此前**未登记或登记不充分**的条目。

| ID | 发现 | 域 | 性质/状态 | 说明 |
|---|---|---|---|---|
| RA-01 | 请求 schema 校验仅覆盖 session.create/list 2/141 路由 | **已闭环（2026-10-25）** | 原"2/141"登记系滞后（HTTP-H0 后实际 28 body + 69/71 query 已由 middleware 全局执法）；本轮补齐剩余缝：post-location seam 18 body + 2 query op（404 解析后经 `validate_at_handler` 出 v2 形状 400）、path-param 模式校验（59 行 `^ses_/^msg_/^frm_/^per/^pty/^sh_`，Params→Query→Payload 序）、退役 `schema_check.rs`（362 行）；prompt/synthetic delivery 非法值改为 schema 层 `kind=Payload`（ref openapi 枚举约束核实）；oc-server 281/281 |
| RA-02 | 包管理器升级路径降级为报错 | **已闭环（2026-10-25）** | npm/pnpm/bun/yarn/vp/brew 真实驱动（argv 逐字对齐 ref updater.ts，5 分钟超时、conciseDetail 输出压缩、OPENCODE_PACKAGE 可覆写包名）；PATH-shim 假二进制测试 8 项；oc-cli 211/211；合并 f26363f |
| RA-03 | oc-theme discovery / system-theme 检测未移植 | **已闭环（2026-10-25）** | discovery（多目录扫描 + 后序覆盖 + 谐波合并）、config_directories（祖先 `.opencode` 遍历）、Rec.601 luma 明暗分类移植 oc-theme；oc-tui 去重委托；分歧文档化（坏 JSON 跳过不中止）；oc-theme/oc-tui 1264/1264；合并 83bfd1c |
| RA-04 | `event-logger` / `file-retention` 无对位模块 | **已闭环（2026-10-25）** | 核实：日志基座（trim_log 50/25MiB）已在 errors.rs；本轮补 wiring——tool_output/shell retention 任务于 engine boot 挂起（此前 spawn_cleanup_task 零调用点 = 真实磁盘无界增长）、shell 侧清扫新实现（7d、sh_*.out）、5 事件 log 监听移植 boot；台账 TOOL-C11/TOOL-RETENTION 转 fixed；oc-core/oc-server 1344/1344；合并 03946f6 |
| RA-05 | OS 通知通道协议分歧 | **登记为具名分歧（维持 OSC 路线）** | ref 走宿主 renderer 回调；rust 走 OSC 9/777/99（`os_notify.rs`）。裸 linux console/部分复用器静默丢失通知——接受为终端宿主下的合理代价（ref 回调依赖 GUI renderer，TUI 语境无对等面）；不修，仅登记 |
| RA-06 | openai-responses WebSocket 已实现，报告滞后 | **已修订（2026-10-25）** | PORT_REPORT §3 "WebSocket 通道延后" 表述已删改为"已实现（ws-transport 默认 feature）" |
| RA-07 | oc-syntax 语言查询覆盖面未对表 | **已闭环（2026-10-25）** | 对表结论：ref 37 门 vs rust 20 门；本轮 17/17 全补（8 门 ref pin 精确上游 crate、hcl 走 nvim-treesitter、markdown 降级无 injection、vue/kotlin third_party vendor）；默认构建体积未动（全部新语法门控在 `langs-extra`/`lang-*`/`all-languages` cargo features，default-on 实测 122.3MiB 超限故全量门控）；**发布矩阵裁定：六目标 release 产物默认不启用 `langs-extra`/`all-languages`（默认 20 门与 v2.0.24 主流核心等价、体积达标），需要 37 门平价的下游以 `--features oc-syntax/all-languages` 自建**；oc-syntax 35/35（default 与 all-languages）；合并 c15486f |
| RA-08 | openapi 由运行时生成改为钉死快照伺服 | **已闭环（2026-10-25，登记补全）** | 快照滞后风险的制度化处置：ci.yml guards job 新增 `openapi-gen --check`；`tools/openapi-gen/README.md` 落地基线升级清单（7 步：换 spec→regen+fmt→--check→guards baseline→CAPPED_NODES 审计→p10 goldens→消息重探针） |
| RA-09 | 错误信封手工注册表无对账 | **已闭环（2026-10-25）** | errors.ts 实为 30 类且 registry 全数已在；3 个 wire-only 内联信封（WorktreeError/VcsInitNotSupported/InstructionEntryTooLarge）收编 registry 单源；Rpc/RpcInternal 补 `data?` 字段；新增 `tools/guards/error_envelope.py` 从钉版 openapi 生成 (op,status,tag) 431+3 条对账（LocationNotFoundError spec 不可见补 3 条），进 guards CI；合并 32de6a0 |
| RA-10 | worktree 策略枚举对应关系未验证 | **已闭环（2026-10-25）** | 对表结论：ref 无闭合枚举（StrategyID 开放串 + 唯一内建 git + 插件扩展点），git 策略端到端行为等价；修掉两处分歧：未知 stored strategy remove 现按 ref 出 `Worktree strategy unavailable`（原先盲目 git remove）、source 行校验跳过补齐（ref 无条件校验）；插件策略注册点登记为 R1–R5 家族具名豁免；合并 32de6a0 |

> 同次审查核实：PORT_REPORT §3 既有登记项（插件宿主 R1–R5、单向迁移、更新通道换 feed+sha256、throw→skip、~~leader 和弦~~、P10-N1=RA-01、LSP 双侧共同缺失 G-09）均属实；141↔141 路由程序化对账全等；CLI 命令面逐 flag 对应。行动项优先级见 PORT_REVIEW_2026-10-25.md §4。
> **2026-10-27 修正**：其中 "leader 和弦" 一项的属实裁定已被六域复审推翻——app.rs D-02（prompt/index.tsx:193 挂起语义 + 测试）早已落地，属登记过期而非真实分歧，PORT_REPORT §3 已同日销账（见 §1.6 头注）。

### 1.5 五域全量对比审查新发现（2026-10-25 第二轮，CLI/Server/Core/AI+插件/TUI 五条并行 oracle 泳道；详见 docs/CODE_REVIEW-2026-10-25.md）

> 本轮对全部功能域逐项对比 ref 与 port 源码。与 §1.4 撞项的 RA-01（请求校验）、RA-02（包管理器升级）经当前代码复核**确已闭环**（request_validation.rs:1509-1531 "rollout COMPLETE"、updater.rs:19-20 真实 argv 驱动），不重复登记。leader 和弦（PORT_REPORT §3）、OS 通知 OSC 路线（RA-05）、非 Linux 声音 spawn 回退（G-11 修正①）、`--force` rust-extra（G-13）等既有登记项亦不重复。

| ID | 发现 | 域 | 严重度 | 说明 |
|---|---|---|---|---|
| ~~RB-01~~ | CodeMode 缺结构性资源上限 | AI+插件 | 中 | **已闭环（2026-10-25，lane rb-f1）**：host.js 边界层移植 ref 限额——`checkValueLimits` 递归巡检每次 host↔guest 穿越（工具入参 guest→host 准入期、工具结果 host→guest、程序返回值），MAX_STRING_LENGTH=1<<24 / MAX_ARRAY_LENGTH=10M，错误消息与 ref 逐字平价（`Invalid string length`/`Invalid array length`，RangeError）；MAX_PENDING_PROMISES=10k 在 hostRequest 并发计数（超限拒绝消息 = ref promises.ts:45-49 逐字）；测试 oc-plugin-host/tests/codemode_limits.rs（超长串/超长数组/边界放行/万级 promise 洪泛/正常用法不回归）；codemode.rs 与 host.js 的"引擎内生故不移植"陈旧注释改写；合并 73f2af8 |
| ~~RB-02~~ | root `--session` 无存在性探测 | CLI/TUI | 中 | **已闭环（2026-10-25，lane rb-f2）**：ref 语义定性（default.ts:44-114）——不存在**不报错**，id 作为 newSessionID 传给首个 prompt 创建的会话（HOME boot）；port 平价：CLI 层 `boot_session_ids` 经 `client.session().get` 探测，存在→`BootArgs.session` 路由 tab，不存在→`BootArgs.new_session_id`（HttpPromptSink take-once，首个 home 提交的 POST /api/session body id），非 SessionNotFoundError 查询错 fatal；fake-HTTP-server 4 腿单测 + oc-tui 2 测；合并 47f66d5 |
| ~~RB-03~~ | `pair --url` 校验缺失 | CLI | 中 | **已闭环（2026-10-25，lane rb-f2）**：`validate_pair_url()`（misc.rs）——仅 HTTP(S)、拒绝 username/password/query/fragment、href 规范化 + 尾斜杠剥离，错误消息 ref 逐字（"Expected an HTTP(S) server URL without credentials, query parameters, or a fragment"）；任何 service 工作前先校验；6 单测含非法 scheme/凭据/query/hash 各腿；合并 47f66d5 |
| ~~RB-04~~ | PORT_REPORT "Last-Event-ID 重订阅为新增"表述失真 | Server | 低（文档） | **已闭环（2026-10-25，lane rb-docs）**：PORT_REPORT §3 措辞改为"客户端重连协议预留（服务器不回放，v2 与 port 一致 live-only；断线恢复靠 `/api/experimental/session/:id/log?after=` cursor）"。事实依据：oc-server 全目录不消费该 header（grep 仅 oc-client/src/event_ext.rs 发送、oc-tui/src/data.rs 本地 4096 环去重）；服务器（v2 与 port）均不回放——与 G-14 进展2 的页内实测一致 |
| ~~RB-05~~ | 内部 operationId 系统性改名 | Server | 低 | **已闭环（2026-10-25，lane rb-docs，文档化登记）**：PORT_REPORT §3 新增条目 + tools/openapi-gen/README.md 新增 "Internal operationId divergence" 小节（routes.gen.rs 为纯生成文件带 DO-NOT-EDIT 标记，注释按约定落在生成器 README）。要点：port 路由 id 带 `experimental.*` 前缀、`session.message.get/list` vs ref `session.message/messages`、`debug.location.list` vs ref `debug.location` 等；线上契约与 served openapi 不受影响（文档为参照逐字节拷贝），但按 operationId 做钩子/统计/插件映射的生态工具两侧不可互换——应按 path+method 映射 |
| ~~RB-06~~ | 语法 query vendored 版本漂移风险 | TUI | 低 | **已闭环（2026-10-25，lane rb-docs）**：`tools/syntax-query-drift.sh` 落地——对 crates/oc-syntax/queries/ 下每个 vendored .scm，按 pin（crates.io crate 版本 / third_party README 标签）拉上游 raw URL 逐字节 diff（剥离 vendoring 头注释），输出 PASS/DRIFT/SKIP 汇总，DRIFT 非零退出；SKIP=上游无钉版 URL（hcl/markdown 为 nvim master 刻意不钉、clojure 无 v0.1.0 git tag）。首跑全绿：PASS=39 DRIFT=0 SKIP=4。已挂 runtime-gates 周 cron 新增独立 job `syntax-query-drift`（真实门，不入 gates-summary 账本，避免瞬时网络抖动误伤总判定）；默认 PR CI 不依赖网络不受影响 |
| RB-07 | 图像协议探测为 env 启发式 | TUI | 低 | **部分解锁（2026-10-26 lane R0）**：原登记的解锁条件（"渲染栈迁移引入异步输入泵"）经核实为陈旧——输入线程 + mpsc + 800ms boot 探测窗自 M-lane 起即已存在（app.rs boot capability probe，原为 OSC 11 + kitty 键盘查询）。R0 已在该探测窗内加入同步 DA1（`CSI c`）往返：应答 `\x1b[?{attrs}c` 中含属性 `4` = 支持 sixel（vt240 惯例，如 `\x1b[?62;4;6;8;15;22c`）；应答无 4 = 无 sixel；无应答回退 env 启发式（KITTY_WINDOW_ID/ghostty 仍为 kitty 权威快路径）——见 `oc-tui/src/image.rs::resolve_protocol` 与 `theme.rs::TerminalProbe::da1_sixel`（解析器单测 + PTY 注入测试 tests/da1_probe.rs）。**PNG→sixel 转码已闭环（2026-10-27，48f5f09）**：新模块 image::sixel——复用仓内纯 Rust decode_png、box-average 降采样（400px 长边上限）、确定性 median-cut ≤16 色寄存器（不抖动）、标准 sixel 发射（`"1;1;W;H`、`#i;2;r;g;b`、6 行 band、`$`/`!` run coding、透明像素不绘制）；sixel_display_escape 对 PNG 走转码（CUP+DCS 与直通同语义）、drawable() 解码门控；零新依赖；raw sixel 直通与其余协议路径不变；golden 结构断言 + 字节级确定性单测（palette tie-break 故意不冻结全输出）。RB-07 全项闭环 |
| ~~RB-08~~ | CodeMode 嵌套结果取值口径 | AI+插件 | 低 | **已闭环**：`nested_value`（codemode.rs:1133-1154）优先返回声明 output schema 的结构化值（edit/patch/shell/mcp_resource/websearch 五个 seam 已产出 output 字段），content-parts 拼接文本仅作回退；RB burn-down 补齐陈旧模块注释（:34-36 曾仍声称"仅拼接文本"）；合并 73f2af8 |
| ~~RB-09~~ | usage/stats/affinity/history 子域无行级语义 diff | Core | 低（验证盲区） | **已闭环 usage+affinity 两子域（2026-10-25，lane rb-f3）**：tests_usage_affinity.rs 5 个确定性离线测试——计费扇出手算对表（计价 turn 0.0935 入消息树 + session_v2 计数器；跨 turn 累计 + tier 升档 1.0850125）、affinity 优先级（parent > fork > self）、e2e 亲和头粘性 + parent 重键失效（镜像 ref session-runner.test.ts:4707-4741）；stats/history 子域维持登记（ref 无同构文件，语义由 projector 重放对账覆盖）；合并 9bac00e |
| ~~RB-10~~ | 零散超集/理论性偏差 | CLI/AI | 信息 | **已闭环（2026-10-25）**：calculateCost tie-break 改 first-wins（严格大于 fold 替代 `max_by_key` 末位，含重复 tier 单测；合并 9bac00e）；uninstall 补 "Installation method:" 探测行（复用 updater::detect_method，含 unknown 警告腿，ref uninstall.ts:31-52 平价；合并 47f66d5）；`api --param` 可重复维持超集登记（向后兼容）；websearch 硬编码注册表维持登记（上游新增需手动同步，效果层动态装配为远期对齐项） |

> 总判定：五域 0 个 ❌ 级缺失，PORT_REPORT 无虚报完成项；新登记项以 RB-01~RB-03（中）为收口优先级。
> **2026-10-25 RB 系列 burn-down（deepwork rb-burndown）**：RB-01～RB-10 全部处置完毕——RB-01/02/03/04/05/06/08/09/10 闭环（四 lane：rb-f1/f2/f3/docs，串行合并 2b8c8a7→47f66d5），RB-07 登记为具名分歧（env 启发式图像探测，解锁条件=渲染栈异步输入泵；**2026-10-26 R0 修正：解锁条件陈旧，泵早已存在，DA1 探测已落地，仅 PNG→sixel 转码仍登记**）；oracle gate 结论见 .slim/deepwork/rb-burndown/oracle-gate.md。收尾配套：.cargo/config.toml 仓库级 incremental=false 固化（坑 2.1#6 建议落为制度，bdef59d）。

### 1.6 六域全量对比审查新发现（2026-10-27，core / 工具+LSP+MCP+插件 / AI / Server / TUI / CLI+配置 六条并行 oracle 泳道）

> 本轮对全部功能域逐项对比 ref 与 port 源码。与既有条目去重：PNG→sixel 转码（=RB-07 剩余）、半块字形/子像素 RGBA（PORT_REPORT §3 延后项）、OS 通知 OSC（=RA-05）、插件 hook 分歧家族（=R1–R5）、keymap throw→skip、flock 30s 降级（PORT_REPORT §3）、JWKS 无缓存（镜像 v2 行为，不登记为缺口）、v1 keybind 迁移丢注释（已登记）均不重复收录。同轮销账三处过期登记（见 PORT_REPORT §3 修订）："63 主题"口径（实为 34）、"/help 有 clamp"（D-05 已消除）、"leader 和弦非空草稿不触发"（D-02 已实现挂起语义）。

| ID | 发现 | 域 | 严重度 | 说明 |
|---|---|---|---|---|
| ~~RC-01~~ | rmcp 不支持 modern-era url elicitation | MCP | **已闭环（2026-10-27，fbf93f7）**：MRTR elicitation 循环（含 url 形状）已实现；sampling/roots 维持为文档化边界；证据 .slim/deepwork/rd-burndown/rcbatch.md（1464/1464） |
| ~~RC-02~~ | CodeMode 无 inventory `search` | AI+插件 | **已闭环（2026-10-27，fbf93f7）**：codemode search 与 ref 平价补齐；证据 .slim/deepwork/rd-burndown/rcbatch.md（1464/1464） |
| ~~RC-03~~ | 插件宿主 per-dispatch 硬超时截断语义 | 插件 | **已闭环（2026-10-27，fbf93f7）**：按 oracle 裁定落地两类 hook 超时（120s/5s/2s）；host.js bun-build 冒烟通过；证据 .slim/deepwork/rd-burndown/rcbatch.md（1464/1464） |
| RC-04 | shell 取消 Drop-guard 为 fire-and-forget | **已闭环（2026-10-27，d1ca226）** | kill-race 测试落地（drop 后 2s 有界轮询断言 remove/sh_*.out 清删/Cancelled + drop-after-exit 幂等腿）；结论 **无真实 race**：活 runtime 下 remove+cancel 可靠完成且幂等；首轮红系断言写错（`Job.get` 在 cancel 后仍为 Some 属设计内——CONPLETED_LIMIT 内不逐出）；设计事实入注释：`Shell.remove` 不发信号，exit watcher 独占 Child handle 负责收割 |
| RC-05 | 冲突/哨兵经 `\0` 字符串穿透事务错误通道 | **已闭环（2026-10-27，85b9c2e）** | 类型化 `SessionConflict` 枚举（AlreadyProjected/InboxConflict/TitleChanged）经 `DbError::SessionConflict` 隧道替代 `\0` 哨兵串；全部原 strip/prefix-compare 站点（projector.rs:56、inbox.rs:56、title.rs:312）改 pattern-match；真 Pragma（revert 边界/decode 失败/bus INVALID_MARKER 双端已类型化）直通不受影响；顺带删除 runner/context.rs 死哨兵 UNAVAILABLE_SENTINEL（typed 对应物早已存在）；oc-core 1036/0 绿，oc-server 仅余既有 be2a env 污染 + parity_vcs_wire watch 时序 flake（基线复现证伪，日志 .test-logs/*rc05*）；合并后 crates 树与已测 lane tip 字节等同（证据复用免重跑） |
| RC-06 | oc-ai 无 per-provider 定制挂载点 | **已闭环（2026-10-27，a5571dc+ad31491）** | `customization.rs` 落地 `ProviderCustomization` seam（auto_cache_policy / chat_metadata_key 钩子，no-op 默认 + 静态注册表 openrouter/vercel-ai-gateway-messages/alibaba-chat + 运行期 register()）；cache_policy.rs 的 qwen/gateway/alibaba if-else 分支全部迁入注册表条目、openai_compatible 经 seam 咨询；零行为变化（795 旧测全绿）+ "acme" 假想 provider 注册端到端测试作为扩展点示范 |
| RC-07 | anthropic_messages.rs 单文件 5420 行 | **已闭环（2026-10-27，97bf888）** | `#[cfg(test)]` ~2600 行原样移至 anthropic_messages/tests.rs（沿 gemini/copilot/cohere_chat/mistral_chat 惯例）；主文件 5420→2810 行；测试数逐字不变（795→795） |
| RC-08 | routes.gen.rs 头注释与实际不符 | **已闭环（2026-10-27，b351b63）** | 生成器模板 emit_routes.rs 头注释改为 axum 0.7 spellings；经文档化流程 regen，routes.gen.rs diff 仅头注释两行；`openapi-gen --check` 通过 |
| RC-09 | session slug 词表与 ref 不同 | **已闭环（2026-10-27，d1ca226）** | `slug_contract_pins_local_wordlist_style`（session/store.rs）5000 抽钉住可观察契约：双词一连字符、小写 ASCII、边长界（adj 4-6/noun 3-9）、两侧词表 ≥25/30 覆盖、900 组合中 ≥800 distinct——测试名显式记录与 TS 词表的有意分歧 |
| RC-10 | stats 聚合与 session move 目的地校验上移出 core | Core | 信息（分层） | `session.moved` 投影在 core，但 move.ts 的 FS 目的地三分类错误在 oc-server handler；stats.ts 聚合在 server/cli。非缺失；"core 层独立测试 move/stats"这一 v2 性质在 port 不成立，审查这两功能需看 oc-server。**2026-10-27 处置：维持文档化登记（分层差异非缺陷，无码变更）** |
| RC-11 | shell 补全双侧缺失 | **已闭环（2026-10-27，997e6cb，rust-extra）** | `opencode completion <bash|zsh|fish|elvish>` 子命令落地（clap_complete 4，Cargo.toml 已有声明的依赖由死变活，输出到 stdout）；doc 注释标记 rust-extra；per-shell 解析 + 非法 shell 拒绝单测。登记册旧措辞"port 亦未引入 clap_complete"同日修正（依赖曾已声明未使用） |
| RC-12 | `api --header` 无 100 条上限 / `stats --year` 等无范围校验 | **已闭环（2026-10-27，997e6cb）** | `MAX_API_HEADERS=100` 在 try_parse_from_checked/validate_extra 执法（clap 风格错误消息含计数与上限）；`stats --year` value_parser range 1970..=9999；边界单测（100/101、1969/1970/9999/10000）；无 ref 证据的其他限额不发明 |

> 总判定：六域 0 个 ❌ 级新增缺失；RC-01～RC-03（中）为收口优先级，其余为登记性技术债/信息项。PORT_REPORT 三处过期登记已同日销账（"63 主题"、"/help clamp"、leader 和弦）。

### 1.7 七域全量对比审查新发现（2026-10-27 第二轮，core-A 会话 / core-B 基础设施 / Server+Schema / AI providers / TUI 交互 / TUI 渲染 / CLI+插件 七条并行 oracle 泳道，逐文件通读两侧源码）

> **2026-10-27 rd-burndown 收口**：本节 RD 系列经 deepwork rd-burndown 收口——RD-01～RD-07、RD-09～RD-14、RD-16～RD-19 全部闭环（合并序列 9519db5 → 56fbf75 → d03cab1 → b34802b → 81fb0fc → 746ec58 → 9861852 → 3d17358 → aa39789，逐项证据见 .slim/deepwork/rd-burndown/rd1..7.md / rdlow1..2.md）；RD-08（workspace seam）裁定为具名豁免登记（证据 rulings-1.md），RD-15（run TTY 权限交互）维持具名超集登记（本 session 无码变更）。RC-01/02/03 同日经 rcbatch lane 闭环（fbf93f7）。FINDINGS §3 三处过期登记同日销账（见本节表尾"登记册修正"）。

> 与既有条目去重：RB-05（operationId 改名，本轮 141/141 路由程序化对账再证全等）、RA-07（37/20 语言门控）、RA-08（openapi 快照）、RB-07（PNG→sixel）、RC-05/06/09 等复核属实不重复收录。本轮另核销三处过期登记（见表尾"登记册修正"）。

| ID | 发现 | 域 | 严重度 | 说明 |
|---|---|---|---|---|
| ~~RD-01~~ | **`opencode mcp add <name> -- <command>` 本地 stdio MCP 路径完全不可用** | CLI | **已闭环（2026-10-27，9519db5）**：本地 stdio 路径补齐（含 trailing command 构造 `{type:"local"}`）；oc-cli 223/223；证据 .slim/deepwork/rd-burndown/rd1.md |
| ~~RD-02~~ | **`step.started` 时间戳恒 0，assistant 消息 `time.created` 被持久化为 epoch 0** | Core | **已闭环（2026-10-27，56fbf75）**：StepDeps 接入真实时钟，started/time.created 持久化正确；oc-core 1101/1101；证据 .slim/deepwork/rd-burndown/rd2.md |
| ~~RD-03~~ | `session.compaction` / `session.title` hook 的 `result` 短路未实现（**FINDINGS §3 登记"已达成"与现实相反**） | Core/插件 | **已闭环（2026-10-27，d03cab1）**：`result` 短路（fromHook 语义）+ RD-09 compaction retry hook 接线同 lane 落地；oc-core 1105/1105；FINDINGS §3 同日销账；证据 .slim/deepwork/rd-burndown/rd3.md |
| ~~RD-04~~ | MCP OAuth **CIMD**（client_id_metadata_document_supported 预选 CLIENT_METADATA_URL）未移植 | MCP | **已闭环（2026-10-27，b34802b）**：CIMD 发现即免 DCR 注册路径补齐；oc-core 1110/1110；证据 .slim/deepwork/rd-burndown/rd4.md |
| ~~RD-05~~ | TUI `@agent`/`@skill` 补全只插纯文本，`SessionPromptBody.agents/skills` 字段全仓无生成构造点 | TUI | **已闭环（2026-10-27，9861852）**：结构化 @agent/@skill parts 构造并随 POST 携带；oc-tui 1278/1278；证据 .slim/deepwork/rd-burndown/rd5.md |
| ~~RD-06~~ | 粘贴摘要 chip（`[Pasted ~N lines]`）整体缺失 | TUI | **已闭环（2026-10-27，81fb0fc）**：粘贴折叠 chip 及提交还原/展开链路补齐；oc-tui 1317/1317；证据 .slim/deepwork/rd-burndown/rd6.md |
| ~~RD-07~~ | standalone `run` 对 `global` 会话的 MCP elicitation form 永不取消 | CLI | **已闭环（2026-10-27，746ec58）**：global 分支 form 取消 + 全局清扫补齐；oc-cli 228/228；证据 .slim/deepwork/rd-burndown/rd7.md |
| RD-08 | workspace 运行时 seam（create/provision/connect/destroy + EnvironmentDriver）整体缺失 | Core | **具名豁免（裁定 2026-10-27，REGISTER）**：ref v2.0.24 中 provision 生产代码零调用、provider registry 为空、无 HTTP 面——seam 为死代码，登记不移植；证据 .slim/deepwork/rd-burndown/rulings-1.md |
| ~~RD-09~~ | compaction 重试决策不经过 `session.retry` hook | Core | **已闭环（2026-10-27，d03cab1）**：`Prepared.retry` 接线进 compaction retry 决策（与 RD-03 同 lane）；oc-core 1105/1105；证据 .slim/deepwork/rd-burndown/rd3.md |
| ~~RD-10~~ | `run -s/--session` 目标解析吞掉非 404 错误 | CLI | **已闭环（2026-10-27，3d17358）**：仅 SessionNotFoundError→新建语义，其余错误上抛；core+cli+server 1613/1613；证据 .slim/deepwork/rd-burndown/rdlow1.md |
| ~~RD-11~~ | compaction 被打断：立即结算变延迟结算 | Core | **已闭环（2026-10-27，3d17358）**：中断即刻结算 `compaction.interrupted` 补齐；core+cli+server 1613/1613；证据 .slim/deepwork/rd-burndown/rdlow1.md |
| ~~RD-12~~ | `revert.stage` 边界消息缺失：ref 报错，rust 静默成功 | Core | **已闭环（2026-10-27，3d17358）**：边界查不到按 ref 报 MessageNotFoundError（openapi 快照口径为 **404**——本行原"→400"系笔误，2026-10-27 修正）；core+cli+server 1613/1613；证据 .slim/deepwork/rd-burndown/rdlow1.md |
| ~~RD-13~~ | SSE text/reasoning delta 批的 100ms 定时器被"事件到达"近似 | Core | **已闭环（2026-10-27，3d17358）**：每 fragment 定时发布节奏补齐；core+cli+server 1613/1613；证据 .slim/deepwork/rd-burndown/rdlow1.md |
| ~~RD-14~~ | edit 工具结构化 output 少 `files: FileDiff[]` 字段 | 工具 | **已闭环（2026-10-27，3d17358）**：output 补 `files` 字段对齐 ref 形状；core+cli+server 1613/1613；证据 .slim/deepwork/rd-burndown/rdlow1.md |
| RD-15 | `run` TTY 权限交互为未登记超集（改变默认行为） | CLI | **具名超集登记（维持，2026-10-27 复核）**：rd-burndown session 无码变更，维持登记（真 TTY dialoguer once/always/deny，`OPENCODE_NO_TTY_PROMPT` 可退回；`always` 为 ref run 无的路径） |
| ~~RD-16~~ | ctrl+c 清空长草稿丢失历史保留 | TUI | **已闭环（2026-10-27，aa39789）**：清空前 history.append 补齐；oc-tui+oc-util 1415/1415；证据 .slim/deepwork/rd-burndown/rdlow2.md |
| ~~RD-17~~ | command palette 缺 per-setting 搜索行 | TUI | **已闭环（2026-10-27，aa39789）**：`setting:{id}` 搜索行补齐；oc-tui+oc-util 1415/1415；证据 .slim/deepwork/rd-burndown/rdlow2.md |
| ~~RD-18~~ | `mcp logout`/`mcp list` 与 mcp registry 的排序/解析微差 | CLI/MCP | **已闭环（2026-10-27，aa39789）**：logout 经 id/name 解析过滤对齐 + 排序改 oc-util localeCompare collation（非 ASCII/大小写混合名与 ref 序一致）；oc-tui+oc-util 1415/1415；证据 .slim/deepwork/rd-burndown/rdlow2.md |
| ~~RD-19~~ | mini busy spinner 丢弃 glow levels 与 WORK_PACE 缓动；SEED_LAUNCH 启动行动画疑似缺失 | TUI | **已闭环（2026-10-27，aa39789）**：逐帧 alpha glow + pace 缓动 + SEED_LAUNCH 启动行动画补齐；oc-tui+oc-util 1415/1415；证据 .slim/deepwork/rd-burndown/rdlow2.md |
| RD-20 | 零散信息级差异 | 多域 | 信息 | ① 配置解析器超集：rust 用 json5（额外接受单引号/裸键/十六进制）vs ref jsonc-parser（config.rs:535）；② system.txt 模板 rust 常量缺 ref 文件尾 1 换行（755 vs 756 字节）；③ title 乐观并发输局 rust 重试 20×50ms（ref 直接放弃，title.rs:258-305，超集）；④ pre-step 失败合成 step.started+failed 错误 part 上消息流（execution.rs:884-943，ref 只发 execution.failed，超出上游的补救性行为）；⑤ models.dev `released()` 只收严格 YYYY-MM-DD（catalog.rs:374-385），快照中 `2024-11` 等月份精度日期归 0，JS Date.parse 有效——同 provider 混合精度日期时同月内排序可翻转（§3 release-date 简化的未细化边界）；⑥ 孤儿评测录音 fixture：`oc-ai/test-fixtures/recordings/{typesafe,openrouter,opencode-zen}-evaluation/` 无任何 .rs 引用（ref experimental evaluation lane 为死代码，裁定范围外或删除） |
| RD-21 | 文档性建议（不修码） | Server/Schema | **已闭环（2026-10-27，b351b63）** | ① openapi-gen README 附完整 **31 条改名表**（脚本化对拍 ref `/packages/protocol/src/groups/*.ts` 内部端点名 vs routes.gen.rs，按 path+method 键控）；关键事实修正：routes.gen.rs operationId 与钉版 spec **141/141 字节全等**——分歧是 vs ref 内部端点名；`persistentPty.*` 命名空间行 12-21 全列（terminal 三条复用注明，raw `persistentPty.connect` WS 路由**非**改名——旧叙述反了）；② 新增 docs/SCHEMA-MAP.md：98 个 ref TS 文件→rust 模块映射（由模块 `//!` provenance 头 + 符号 grep 交叉导出，不确定项标 `?`；Llm/Rpc/ModelsDev 的 oc-core/oc-ai 归宿有注记行），openapi-gen README 与 oc-schema lib.rs 均已链接 |

#### 登记册修正（2026-10-27，本轮销账三处过期/不实登记）

1. **PORT_REPORT §3「input_paste 的 preventDefault 对象语义未移植」→ 过期**：`composer/mod.rs:343-359,645-651` + 测试 `:1171-1188` 已实现 D-05 双路语义（PasteDisposition::Prevented/NoEvent），应销账。
2. **FINDINGS §3「session.prompt hook 生产路径未接线」「permission.evaluate 生产 dispatch 点未接线」→ 过期**：`prompt_prepare.rs:204-229` 与 `permission.rs:439-457` 实际已接线，应销账——**2026-10-27 rd-burndown 已在 FINDINGS §3 表内销账**。
3. **FINDINGS §3「compaction/title result 短路 none（已达成）」→ 不实**：见 RD-03——**已闭环（2026-10-27，d03cab1），FINDINGS §3 表内已改为闭环状态**。

> 总判定：七域再证 141/141 路由全等、AI provider 层无实质差异；本轮新增 RD-01（高，功能缺失级）+ RD-02..RD-07（中，6 项）+ RD-08..RD-19（低，12 项）+ RD-20/21（信息）。收口优先级：RD-01 → RD-02 → RD-03/RD-05（先裁定）→ RD-04/06/07。

---

## 2. 踩坑记录（事故 → 根因 → 现行规则）

### 2.1 资源与调度

| # | 事故 | 根因 | 现行规则 |
|---|---|---|---|
| 1 | **多子 agent 同时编译 → CPU 饱和 → 共享主机的 newapi 在线服务卡死** | 本机与在线服务（newapi、49374）共享 16 核/22G；并行 lane 各自 cargo build 叠加无上限 | 一切本地构建/测试经 `tools/dev-gate.sh` 单入口：强制 eternal130 身份 + systemd transient scope（CPUQuota=400% 四核硬顶、MemoryMax=10G、TasksMax=512）、1 分钟 load≥6 拒启动；lane 提示词必须声明走 dev-gate |
| 2 | root 直跑 cargo，三次属主污染 | root 产物属主混入用户目录 | dev-gate 强制降权身份；CI 例外（不占本机） |
| 3 | 共享同一检出目录的两个 lane 互相删文件 | 写所有权无隔离 | lane 写隔离=每 lane 独立 git worktree（`.worktrees/`）+ 串行合并回 main；只碰自己 scope 的文件；收尾前重查 `git status` |
| 4 | /tmp/opencode 被整体清空，5 条 lane 未提交工作全失 | API 配额耗尽杀 lane 后，外部清场连证据目录一起清 | lane worktree 移入仓库内 `.worktrees/`；里程碑 WIP 提交强制（小步提交、事后 squash）；/tmp 顶层只用 PID 唯一名 |
| 5 | SCAN-EF 扫描腿（并行编译 + 探针连发 release 二进制）严重拖慢同机生产的 v2 服务，被用户击杀 | 验证负载无节制 | 生产尊重制度：**单 lane 串行**；扫描腿小批量带暂停；重型腿（E2 TTI/soak/电池）需用户明示或错峰；派发前看生产健康信号 |
| 6 | `target/debug` 无界增长至 **476G**（debuginfo 主导 + 增量编译无指纹 GC），磁盘 99% 满 | cargo 增量缓存只增不减、多 profile 叠加 | 2026-10-23 清理 486G（保留 release 3.2G）；建议日常 `CARGO_INCREMENTAL=0` 或周期 `cargo clean`；无 worktree 残留（已核） |
| 8 | **incremental=false 已生效但磁盘仍持续上涨**（2026-10-27 复盘：主 target/debug 153G + 5 个 lane worktree 各自全量 target ≈51G；incremental 目录实为 4KB，非根因） | ① worktree 天然隔离 → 每 lane 独立编译全依赖图，`incremental=false` 管不到；② cargo 从不 GC target 内陈旧产物（历史 lane 残留/多版本 deps）；③ lane 完成后 target 无人回收 | 三策：① **lane 合并后立即删该 worktree 的 target**（保留分支与 worktree 本体亦可先只删 target；删前过清理门=fuser 无打开文件+30 分钟 mtime 规则）；② worktree 构建可改用 `CARGO_TARGET_DIR` 指向主 target 复用依赖产物（注意指纹翻转抖动与 cargo 文件锁串行化，需 dev-gate 协调）；③ 主 target/debug 定期（≥两周或 >100G）`cargo clean -p` 粗清或整清 |
| 7 | API 配额窗口耗尽（92%）在半夜杀 lane | 配额无监控 | 定时唤醒 + 断点复活模式（WIP 落盘后 lane 死亡也可 revive） |

### 2.2 进程与终端安全

| # | 事故 | 根因 | 现行规则 |
|---|---|---|---|
| 1 | **测试盲杀信号误杀系统服务**（生产事故） | 测试按 PID 名/号发信号，撞上复用 PID 的系统进程 | safe_signal 结构性修复：pidfd 系统调用 + boot_id/进程 starttime 世代 sidecar + 外来记录 cmdline 精确校验；全仓禁裸 PID 发信号；组杀=活 leader 钉住+/proc 扫描+逐成员校验；测试只用自有子进程句柄（process_safety guard 常设） |
| 2 | **扫描腿裸启 TUI 二进制锁死用户终端**：raw mode + alt screen 缓住真终端，两个 TUI 同写一屏→持续乱码、ctrl+c 被吃、被迫重启生产 v2 | E2 进程拉起腿未套 PTY harness，直接 `cargo run` 冒屏 | TUI 二进制任何 lane 只准在 vt/PTY harness 内或 stdout/stderr 重定向 + no-tty 模式启动；tmux/终端矩阵探针只碰自建会话；OSC/转义探针只写自有 PTY |
| 3 | **测试进程泄露**：孤儿 rig/电池进程、stale ref-bun 进程（GATE3 一轮杀 12 个）、lane 死后后台矩阵孤儿化（rig 二进制消失、配对进程残留） | 后台长跑无归拢、teardown 不完整 | 长验证一律前台分块（≤45 分钟/块）；rig teardown 卫生项常设；GATE 轮清扫；清理门=mtime<30 分钟视为在用（两次误踩活动目录后立规） |
| 4 | rig 使用陈旧二进制产物，两次被差分器抓获 | lane 换代码未重建 rig 缓存 | lane 收尾强制重建检查（已知陷阱清单） |
| 5 | **孤儿 `sleep 300` 占住 dev-gate 全局锁 10+ 分钟**（2026-10-25，lane 收尾 watchdog 脱离父进程后持 flock 睡眠，全部构建排队静默堵死；宿主后台 shell 任务亦被清理丢失） | lane 派生的 sleep/watchdog 未在收尾时回收；flock 无超时 | lane 收尾自查自派生后台进程（派发模板新增必查项）；构建排队异常时先 `fuser /tmp/opencode/dev-gate.lock` 查持锁者；重试循环勿依赖宿主后台 shell 存活 |

### 2.3 宿主 / 会话生命周期

| # | 事故 | 根因 | 现行规则 |
|---|---|---|---|
| 1 | **60 分钟 location-TTL 收割器多次静默杀后台 lane**（含 9E 后台矩阵中断） | 宿主对后台会话的位置 TTL 驱逐 | 长任务=前台分块 ≤45 分钟 + 追加式日志 + 每 6 轮 WIP 提交（断点可续）；**单次工具调用硬上限 45 分钟**（一次 80 分钟单调用险触 TTL） |
| 2 | 完成后的会话收到排队消息 → 幽灵重启（M6 幸无实害） | 排队投递无终态检查 | 禁止向已完成会话投递；干预一律 kill + 新 lane |
| 3 | H-1：复活子代理的第二份报告永远到不了父级（completed→completed 不是迁移） | 宿主缺陷，非本仓可修 | 子代理**禁止跑后台任务**（全部前台块）；父级 watcher 允许；完整分析见 docs/host-issues.md；候选上报上游 |
| 4 | 小时级后台睡眠被宿主收割（40 分钟级偶成） | 宿主对长睡眠的回收 | 级联短看守（50 分钟自续完成通知）+ 用户消息兜底 |

### 2.4 验证方法论

| # | 事故 | 根因 | 现行规则 |
|---|---|---|---|
| 1 | **陈旧 oracle 缓存掩盖真回归**：bun 全局缓存以 @opentui/core **0.6.1** 顶替钉版 0.5.14，病灶窗口内全部基准标错；"从未真绿"的提交穿门而过（双病灶之一） | oracle 缓存无版本/新鲜度证明 | oracle 三可证：版本硬断言（fail unless 0.5.14）+ 采集指纹（driver+scenario FNV-1a 双语镜像）+ always-refresh 终门；main.rs 自愈陈旧缓存、连续漂移硬错 |
| 2 | **"实现了 ≠ 链路接通"**（两案：F16 leader>e 审计标 done 实战失效；sidebar 遮罩有渲染无关闭键层，模糊键全进不可见输入） | 审计只证实现存在，未证链路端到端 | 交互面修复必须带 PTY/field 级验证；审计状态只是起点提示 |
| 3 | **双端差分 rig 五轮门只抓 fmt/goldens/flake，漏掉全部真实用户可见 bug**（omo-slim 侧栏、子代理切换、marquee、spinner 色、thought gutter 五案）→ 用户裁定退役 | 参照平价看不见用户插件内容；语料 vanilla-stub；时序节奏与色 token 绑定从未步进验证 | 差分测试 2026-10-17 退役；扫描循环接棒（后按裁定于 FIXW2 终止）；golden 降级为回归套件；常设仪器补：真实插件沙箱 + 时序 + token 覆盖 |
| 4 | 金帧字节自洽放过"屏幕组合模型"错误（错 tab 模型、错 boot 路由都活到 beta.7 实战） | 金帧证明渲染保真，不证明组合/路由模型 | 交互模型类（路由/tabs/列表拉取）必须做参照源码级对比（file:line），不许 golden 自证 |
| 5 | **合并静默回滚他 lane 成果**（C lane 自动合并回滚 B 的 availability 折叠、删测试；E2 被回退数个版本无人察觉，M0–M5 带伤建设） | 三/四方自动合并吃掉语义；rig 上差异被误读为"其他 lane 在飞" | 合并裁决必须列表级核对每个合并 lane 的删除清单；证据优先方整文件恢复；每 lane 合并后跑受影响 crate 测试 |
| 6 | **幽灵提交**两案：lane 报"已提交"而分支仍在 base | lane 自述无凭据 | 派发模板要求最终报告贴 `git log -1 <branch>` 落地证明；缺证明=未完成 |
| 7 | **无测试合并**：DERIG 推送未跑测试，main 的 parity 构建断一天 | 合并无门 | 每次 main 合并至少受影响 crate 构建+测试后再推，无例外 |
| 8 | merge 后必须补 bun 内差分冒烟（纯 cargo 门看不见 JS 侧破坏，O2a×O2b 融合后 driver.ts 语法断裂存活） | 纯 Rust 门对 JS 侧盲 | harness 相关合并加 bun 冒烟 |
| 9 | **子代理"默认测试通过"假象 + 过滤指令吞失败详情**：首轮 cargo test 用了过滤/未带 --no-fail-fast，失败被折叠成汇总行，失败原因不可见，被迫第二轮重测定位，拖长 lane 周期（2026-10-27 收口 session 多 lane 观察） | 测试报告只看退出码/末行汇总；过滤参数（--lib、name filter）把红测试藏进被滤掉的集合 | lane 验证纪律：① 首轮测试**不加过滤**并带 `--no-fail-fast`，拿到全部结果；② 报告必须**原样引用 `test result:` 行**（含 passed/failed 计数），禁止只写"green/通过"；③ 有失败时**同一轮**立即用 `--exact <name> -- --nocapture` 重跑单个红测试取回失败输出，不得先交报告再返工；④ dev-gate 输出被截断时读保存的完整输出文件而非重跑 |
| 10 | **测试证据随会话丢失 → 重复测试**：lane 报告只活在子代理会话里，宿主重启/会话回收后 orchestrator 或后续 lane 拿不到原始 test result 行，只能整轮重跑验证（2026-10-27 合并门曾整轮重测 5 crate，数十分钟；且 /tmp/opencode 有被整体清空的前科 2.1#4） | 测试输出没有落盘为持久文件；依赖会话内存或易失 /tmp | **一切测试运行必须 tee 落盘**：`tools/dev-gate.sh 'cargo test …' 2>&1 \| tee .test-logs/<crate>-<yyyymmdd-HHMM>-<lane>.log`（.test-logs/ 进 .gitignore、不随 /tmp 清空；merge gate 同规则）；lane 最终报告**必须给出日志文件路径 + 关键 `test result:` 行**；后续任何"是否已验证"的争议先读日志文件，禁止凭记忆重跑；日志含红项时连同基线证伪证据（基线 commit + 复现输出）一并落同一目录 |
| 11 | **plain `cargo test --workspace` 产生大面积假红**（2026-10-28 rv-burndown 收口：cargo test 直跑 oc-tui 数十测红——PTY/VT 套件 spawn 真终端+stub server，在无限流/无重试下时序全崩；同一棵树 nextest 4450/4450 绿） | 本仓 canonical 测试 harness 是 nextest（.config/nextest.toml：test-threads=4、全局 retry=1、be1/pty 家族 retry=2 + [profile.pty] test-threads=1 序列化）；plain cargo test 绕过全部降载与重试机制 | 全 workspace 验证一律 `cargo nextest run --workspace --locked --no-fail-fast`；PTY 家族另跑 `--profile pty` 序列化（镜像 ci.yml 两 job）；禁以 plain cargo test 的红作为回归证据，除非单测 --exact 定位复现 |

### 2.5 工程卫生

- 发布门 scoreboard：beta.13 起每修必配独立子代理门扫描，循环至全绿才发（GATE1–GATE5）。
- lane 派发模板强制项：English commits、分块调用（≤45 分钟）、worktree 隔离、写明 dev-gate。
- DNS 探针文件（/tmp/opencode/dns-probe.*）不是构建产物，勿清。
- 仓库根曾经堆积垃圾文件（`None`、`refprobe.pid`）——2026-10-23 清除；收尾自查 `git status` 为模板必查项。

---

## 3. 现行制度速查（防复发汇总）

| 类别 | 制度 | 载体 |
|---|---|---|
| 资源 | 单入口构建/测试（配额+身份+负载闸） | `tools/dev-gate.sh` |
| 调度 | 单 lane 串行；重型腿需批准；生产健康前置检查 | 生产尊重制度（2026-10-18） |
| 终端 | TUI 只在 PTY/vt harness 内启动；探针只碰自有会话 | 终端隔离规则（2026-10-18） |
| 信号 | 全仓 safe_signal；禁裸 PID；测试用自有子进程句柄 | `tools/guards/process_safety.py` |
| 生命周期 | 前台分块 ≤45 分钟；WIP 提交；禁投已完成会话；子代理禁后台 | lane 派发模板 |
| 验证报告 | 首轮测试不过滤+--no-fail-fast；报告原样引用 test result 行；红测试同轮 --exact --nocapture 取回原因 | 本册 2.4#9（2026-10-27） |
| 测试证据落盘 | 一切测试 tee 到 .test-logs/<crate>-<时间>-<lane>.log（git-ignored）；报告给路径；争议先读日志禁止凭记忆重跑 | 本册 2.4#10（2026-10-27） |
| 写隔离 | 每 lane 独立 worktree；串行合并；删除清单核对 | 写隔离制度（2026-10-11） |
| 合并 | 受影响 crate 构建+测试后推送；落地证明（git log -1） | 合并门（2026-10-18） |
| oracle | 版本断言+指纹+always-refresh | `tools/diff`（opentui-rust） |
| 磁盘 | 增量抑制（仓库级 .cargo/config.toml incremental=false，2026-10-25 起强制）/周期 clean；清理门 mtime≥30 分钟视为在用 | 本册 2.1#6 |
| 验证 | 交互面必须 PTY/field 级；组合模型必须源码级对比；金帧=回归套件 | 方法论登记（ROADMAP §4） |

---

## 4. 2026-10-27 全量对比审查发现（RV 系列）

> 来源：7 通道逐域逐行对比 /opt/opencode-ref 与本仓（不采信本册及 PORT_REPORT/ROADMAP 的既有结论），汇总见 `PORT_REVIEW.md`。
> 编号 RV-* 以避免与上文 G/F/R 系列冲突。**2026-10-27 冲突复核完毕**：§1「已闭环」与 RV 初版的 3 处冲突中，2 处系审查通道误报（RV-P2 撤销——PTY daemon 为默认后端已接线，frontend.rs `Frontend::new` spawn-if-absent + transport.rs 完整 makeDaemonTransport；RV-P3 撤销——设备流在 integration/flows.rs:366-437 完整实现，初查只搜了 mcp/oauth.rs）；RV-G3 收窄为「模型输入侧」（TUI 显示侧 R-07/R-08/RB-07 已闭环不冲突，但 media.rs 的 max_width/max_base64_bytes 配置字段全仓无消费者、无 autoResize）。RV-B1/RV-P1 经二次代码复核确认成立。

> **2026-10-28 rv-burndown 收口（deepwork .slim/deepwork/rv-burndown/，四道 oracle gate 均 GO-WITH-FOLLOWUPS、followup 全消化）**：本节 RV 系列全部处置完毕——
> **已闭环（实现）**：RV-B1..B9（B1 重试不耗步数/REF llm.ts:191-192、B2 revert 前置提交、B3 skill 正文注入+session.skill.activated+evt_ id 派生、B4 multiple→multiselect、B5 background jobs+合成通知（REF 自身 conflict→500 缺陷忠实保留）、B6 compact steer 默认+await_idle、B7 view idle/viewed 单调守卫+缺 idle 400、B8 jiff 本地时区聚合、B9 spent 清理收窄本会话+中断路径清理）；RV-G1（26 门 formatter 检测/执行/引擎接线+spawn_blocking off-runtime）；RV-G2（oc-core/filesystem watcher + location 活动 re-arm + filesystem.changed 首个发射器）；RV-G3（oc-core::image 输入侧 limits/autoResize 阶梯，image 0.25 仅 png+jpeg）；RV-G4（session/transfer.rs import 重放，端点 409/400/404/500 映射）；RV-P1（session runner 咨询 ResolvedModel.transport，openai/azure/xai WS 通道 + sticky HTTP 回退，xai 独立 24h rotation/continuation 语义，drain 双 close TODO 清零）；RV-P4（裁定为已移植的 WebSearchMcp.call 固定端点路径——stale TODO 撤销，端点可覆写+exa 覆写 bug 修复+7 个 fake-MCP 测试）；RV-P5（oc-client shared_events.rs 引用计数共享 SSE 复用器，REF 无 backoff 消费者驱动重连）；RV-P6（/api/info wildcard→getifaddrs 网卡 IP 展开）；RV-P7（event_manifest.rs 单一公开性事实源+双侧 drift guard，mcp.tools.changed 对齐 REF 私有）；RV-P8（grep/glob/read 结构化 Output 经 RB-08 seam）；RV-P9（shell 描述随 session hook 动态刷新）。§4.4 杂录 6 项修复（pathdiff ""/URL 协议大小写+registry min-max 校验/read 建议 raw 序/patch Set 去重/pinned catalog+session_move、models compact JSON）+ 校验器子集裁定 adequate。
> **具名豁免（oracle 双侧证据裁定，.slim/deepwork/rv-burndown/rulings-4.md）**：RV-G5（REF experimental/ 仅 export、唯一消费方 exports.test.ts——死代码；port 孤儿评测 fixture 已删 5cdb6cb+fc2aabc）、RV-G6（Cloudflare Worker 云端面，无终端表面）、RV-P10（六条 /api/vcs 路由已在；REF 亦仅 git 单后端，无用户可见损失）、RV-P11（per-series 无活缺口：durable-event 已服务 log 端点；snapshot/Usd/TokenUsage 已类型化于 step 事件）、RV-P12（永久：REF 走 npm 动态 import 不可在 Rust 二进制复现，UnsupportedProtocol 为正确对等物）、RV-P13（effect 轨无终端 shipping 插件消费，port 惯性加载+显式消息）、RV-P14（仅扩展性表面；注意：不得引用 P14 作为 transport 默认值覆盖的依据——WS 默认值豁免见下行）。**附带豁免**：openai/xai/azure 的 websocket *默认值*来自 REF 未移植的 provider 插件 lane——config 显式 settings.transport 已端到端生效，默认值缺失登记为具名豁免（wire 语义等价 + HTTP 回退安全降级，oracle-gate-4.md finding 4.2）。
> **验证**：nextest 全 workspace 4450/4450 绿（canonical harness，.test-logs/final-nextest-20261028.log）+ pty 序列化 profile 70/70 绿（.test-logs/final-pty-20261028.log）；此前一次 plain `cargo test --workspace` 的大面积 oc-tui 红系 **harness 误用**（绕过 nextest test-threads/retry/PTY 序列化），教训入 §2.4#11。下表为 2026-10-27 审查时的原始登记（历史快照，现状以上文为准）。

### 4.1 功能级缺失

| ID | 缺口 | REF 位置 | 现状 | 影响 |
|---|---|---|---|---|
| RV-G1 | Formatter 26 个内置工具链检测/执行 | core/src/formatter/builtins.ts (297 行) | oc-core/formatter.rs 为 Noop stub（自注 TODO(formatter-lane)） | edit/write/patch 后自动格式化不生效 |
| RV-G2 | 通用文件监听服务 + location 活动检测 | filesystem/watcher.ts + location-watcher.ts (445 行) | 仅 config/watch.rs（notify）；FileSystem.Event.Changed 无发布者（G-12 已类型化该事件但无发射源） | 依赖文件变更事件的功能静默失效 |
| RV-G3 | 模型输入侧图片 limits/autoResize（photon 缩放） | core/src/image.ts（tool.ts/instance.ts 消费） | oc-schema/config/media.rs 有 max_width/max_base64_bytes 字段但无消费者，无缩放实现（TUI 显示侧已闭环，见 R-07/R-08/RB-07） | 超限图片无法降采样/拒绝 |
| RV-G4 | session import 回放 | session/transfer.ts (336 行) | 空 synthetic（"imported"）占位，内容全丢 | 导入功能名存实亡 |
| RV-G5 | AI experimental/evaluation | packages/ai/src/experimental/ (543 行) | 无 | 实验性评测缺失 |
| RV-G6 | packages/function（云函数验证） | ~460 行 | 无 | 低影响 |

### 4.2 疑似移植 Bug

| ID | 问题 | 位置 | REF 行为 |
|---|---|---|---|
| RV-B1 | 重试/Continue 也 `step += 1`，步数预算被重试侵蚀 | session/runner/drain.rs:741 | REF 仅每完成一步 +1，重试不耗步数 |
| RV-B2 | prompt/compact 前未 commit staged revert | server session handler | REF session.ts:165 准入前先提交 |
| RV-B3 | skill 端点只准入 `"/{skill_id}"` 占位文本，正文未进上下文，未发 session.skill.activated | skill 路径 | REF 注入技能正文并发布事件 |
| RV-B4 | question multiple:true 未映射 multiselect | tool/question.rs:45-72 | question.ts:124 映射 multiselect |
| RV-B5 | background 端点空 204 | server handler | REF 取消阻塞任务并注入合成通知 |
| RV-B6 | compact 默认 delivery=Queue（REF steer）；wait 50ms 轮询（REF awaitIdle） | server handler | — |
| RV-B7 | view 端点缺 idle/viewed 单调守卫 | server handler | 投影器 max() 兜底，影响有限 |
| RV-B8 | stats 按 UTC 聚合（识别 IANA 名但不用） | handlers/session.rs | REF 按本地时区桶化 |
| RV-B9 | compact() 清空全部会话的 spent 记账 | runner/compaction.rs | REF 只清本会话 |

### 4.3 部分/占位实现

| ID | 项 | 现状 |
|---|---|---|
| RV-P1 | model-transport（会话 WS 传输，REF 571 行） | 仅 seam 占位；modelTransport.close/plugins.awaitActivation TODO |
| ~~RV-P2~~ | ~~持久 PTY daemon 接线~~ | **误报已撤销（2026-10-27 复核）**：Frontend::new 默认即 daemon 后端（spawn-if-absent），OC_PTY_DAEMON=0 为逃生舱；transport.rs 完整实现 makeDaemonTransport（daemon.ts:158-431）——§1 的 D1-D3 闭环记录属实 |
| ~~RV-P3~~ | ~~OAuth 设备流~~ | **误报已撤销（2026-10-27 复核）**：integration/flows.rs:366-437 完整实现（request_device_code/verification_uri/轮询）；初查只搜了 mcp/oauth.rs |
| RV-P4 | websearch MCP 托管 provider | TODO(wave-D) |
| RV-P5 | oc-client 无重连/共享订阅/响应式层（REF shared-events.ts） | 仅裸 SseStream |
| RV-P6 | /api/info wildcard 地址不展开（LAN 发现退化） | info.rs:49-56 |
| RV-P7 | SSE 事件公开性为手工 registry + 补丁段（sse.rs:151-162） | REF EventManifest 单一事实源，漂移风险 |
| RV-P8 | grep/glob/read 结构化 Output 未导出（output: None） | codemode/plugin 拿不到结构化结果 |
| RV-P9 | shell 工具描述注册时固定 | REF 由 session hook 动态刷新 |
| RV-P10 | VCS 抽象层/gitconfig-include/worktree 策略层未移植 | 功能大半被 git2+project.rs 吸收 |
| RV-P11 | schema 长尾（snapshot/llm/money/token-usage/durable-event） | 需确认 durable-event 是否整域缺口 |
| RV-P12 | 长尾 provider 适配器（poe/snowflake/digitalocean/xai 等） | 协议族归并，未逐个确认覆盖 |
| RV-P13 | 插件 SDK 单轨（无 effect 轨） | host.js 提供 promise 轨 |
| RV-P14 | state.ts Transformable 插件动态 transform 弱化 | 仅 config 热重载覆盖 |

### 4.5 未闭环条目专项复核（2026-10-27，对 §1–§3 全部开放/豁免/登记项逐一以代码与 CI 证据核对）

| 条目 | 复核结论 | 证据 |
|---|---|---|
| R-04（Web UI 范围外） | ✅ 属实 | app_archive.rs（19KB）存在且接线 boot.rs/lib.rs；范围外裁定有效 |
| G-09（LSP 双侧缺失） | ✅ 属实 | REF write.ts:44 明注 "TODO: Add LSP ... after LSP runtime exists"；port 侧无 LSP 运行时符号 |
| G-14（E2E 待 cron 观察） | ✅ 登记属实，且**观察条件已满足**：首周 schedule run（2026-10-26，40294359979）browser smoke job EXECUTED=success（§1 行已同步更新） | gh run view |
| R1–R5（hook 永久豁免） | ✅ 属实 | FINDINGS.md §5 R1–R4 逐条在册 |
| (b)-类随机 coin | ✅ 属实 | ROADMAP:133/189/218/220 定义为 placeholder 随机 coin 具名豁免（rig 比对已知差异） |
| `aisdk:` 包桥接豁免 | ✅ 属实 | facade.rs:78/433 类型化 UnsupportedProtocol 精确报错 |
| RA-05（OSC 通知分歧） | ✅ 属实 | os_notify.rs 头注释明确 osc9/99/777 矩阵 + env 覆写 |
| RA-07（37/20 语言门控） | ✅ 属实 | oc-syntax Cargo.toml langs-extra(84)/all-languages(105) features 存在，发布默认 20 门 |
| RC-10（move/stats 分层） | ✅ 属实 | move 目的地解析在 oc-server handlers/session.rs:814-817 |
| RD-08（workspace seam 死代码豁免） | ✅ 属实 | REF provision 仅 workspace/driver.ts（注释），零生产调用 |
| RD-15（run TTY 权限超集） | ✅ 属实 | OPENCODE_NO_TTY_PROMPT 在 run/noninteractive.rs:388-395/1375 |
| RD-20①（json5 超集） | ✅ 属实 | config.rs:535 `json5::from_str` |
| RD-20②（system.txt 差 1 字节） | ✅ 精确属实 | Rust SYSTEM_TEMPLATE 755 字节无尾换行 vs REF system.txt 756 字节（尾 `\n`） |
| RD-20③（title 重试超集） | ✅ 属实 | title.rs `for attempt in 0..20` + 50ms sleep |
| RD-20④（pre-step 合成 part） | ✅ 属实 | execution.rs:884+ publish_failed_turn_error + synthetic json |
| RD-20⑤（released 严格日期） | ✅ 属实 | catalog.rs parse_iso_date_ms 强制 10 字节 YYYY-MM-DD（注释自认 JS 更宽） |
| RD-20⑥（孤儿评测 fixture） | ✅ 属实 | oc-ai/test-fixtures/recordings/{typesafe,openrouter,opencode-zen}-evaluation/ 三目录在树 |
| RB-09 残留（stats/history 盲区登记） | ✅ 维持（信息项，无需码变更） | — |
| RB-10 残留（api --param 超集、websearch 硬编码注册表） | ✅ 属实 | websearch_providers.rs 头注释硬编码 5 provider（MCP-hosted 仍 TODO，与 RV-P4 同源） |
| §1.3 验证盲区（browser_e2e 口径/P10 opt-in/旗舰插件 CI 跳过） | ✅ 属实 | runtime-gates.yml 含 browser-e2e(16)/p10-real-history(17)/syntax-query-drift(18)/omo staging(728) 各 job；p10-real-history 2026-10-27 才入树、尚无 run，SKIPPED-GAP 语义待首跑 |
| pre-2.0.24 真实语料 cron | ✅ 属实（等待 secret + 首跑） | 同上 |

**总判定：登记册全部未闭环条目 0 虚报、0 过期（唯一状态推进 = G-14 首周 cron 已绿，已在 §1 行内更新）。**

### 4.4 小项杂录

pathdiff 同路径 `"."` vs `""`；webfetch timeout clamp(1,120) 与 URL 协议大小写；read missing 建议排序；patch 资源去重排序；session_move pinned:true 未移植；models 内容 pretty vs compact；registry 校验器 JSON-Schema 子集；slug 词表本地化（有意）。扩展内联最多待补审：openai_chat.rs (5736)、facade.rs (2143)。

---

## 5. 2026-10-28 九线全量审查 + 四线复核新发现（RW 系列）

> **2026-10-28 rw-burndown 收口（deepwork .slim/deepwork/rw-burndown/，四道 oracle gate：A/B/C=GO(-WITH-FOLLOWUPS)、D=GO-WITH-FOLLOWUPS，followup 全消化）**：本节 RW 系列全部处置完毕——
> **已闭环（实现）**：RW-01（Anthropic 系 per-provider body wrap：customization seam 新增 `wrap_anthropic_body` 钩子，alibaba+zai-coding-plan thinking 免 budgetTokens、meta `native.meta`→`web_search`；moonshot/minimax 经 ref 核实本就走 plain 协议、不设条目并以回归测试钉住，0da2a1f）；RW-02（persistent-pty 屏幕跟踪换 vt100 真终端仿真，CSI/OSC/游标/erase/alt-screen/resize，wire 形状不变，fdf9b13 + rw2fix a94e5a7：UTF-8 char-boundary 修剪 + meta 三处 parity 补丁）；RW-03（MCP `config.protocol` 协商：legacy/auto-discover+fallback/pin，rmcp ClientLifecycleMode，ref client.ts:393-397；NoCompatibleProtocolVersion 带 ref 补救提示，d56f414）；RW-04（OAuth 同 grant 并发刷新共享 in-flight 去重〔unwind-safe RAII〕，ref oauth.ts:29-45，d56f414）；RW-05（fs/find 移植 protected.ts 排除表 + 有界 walk + 子序列模糊评分 + spawn_blocking，971efc1 + rw4fix 0cdc1fa）；RW-06（OTel 骨架：`otel` feature 默认 OFF，默认闭包 0 otel 依赖〔cargo tree 证据〕，OTLP env/资源属性对齐 ref，971efc1）；RW-07/08/09（auth login 自动开浏览器、session list jiff 本地时区 + less -R -S 分页 + export fuzzy 选择器、biome 经 Npm.which 缓存代 + bun/npm 自动安装，c8bfa92）；RW-10/11/12（forms TTL 淘汰+真 date-time 校验、TS 时代 legacy PersistedRevert 解码、LocationActivity bus 自动 touch，634a0ae）；RW-T1（schema serde camelCase renames：projectID/uriTemplate/forceRequired/partID + gate-4 followup ProjectUpdateInput.projectID，be3e69e）；RW-T2（OPENCODE_PTY_BIN 外部二进制校验：存在/常规文件/可执行位，binary.bun.ts 范围注记）；RW-T4（tool_output 截断预算改 UTF-16 码元口径，星面边界测试；85cc8da）。
> **具名豁免（oracle/证据裁定，rw6.md）**：RW-13（cross-spawn-spawner/open——消费者清单证明 std/tokio Command + oc-cli open_url + oc-tui 已覆盖全部真实调用点）；RW-14（oc-ai experimental/ 折入 RV-G5：ref 侧零非测试消费者死代码）。
> **登记偏差（文档化，非缺口）**：oauth 去重键用 config.url（资源服务器）替身〔发现前 token endpoint 未知，ref 用 token endpoint URL〕；rmcp Auto discover 10s 超时回退 legacy〔ref SDK 源不可检、未证实分歧〕；fs.find 100k 上限 vs ref VCS 根 MAX_SAFE_INTEGER、评分系 fuzzysort 贪心近似〔子串优先性质已测〕；formatter npm 缓存代无 GC（磁盘增长，登记 followup）；vt100 resize 截断不回流（ref 无服务端仿真，快照面偏差）。
> **维持登记（看板项）**：RW-T3（reqwest 0.12/0.13 双版本统一窗口）、T5（MCP 404 宽过期判定）、T6（glob 无 brace/extglob）、T7（serde_yaml 归档等依赖升级看板）、T8（BUN_BE_BUN 空操作提示）。
> **验证**：canonical nextest 全 workspace 4528/4528 绿（.test-logs/rw7-workspace-225809-rw7.log）+ 各合并门全绿（逐 lane 日志 .test-logs/*-{rw1..rw7}*.log）；lane 证据 .slim/deepwork/rw-burndown/{rw1..rw7,rw2fix,rw4fix}.md。下表为 2026-10-28 审查时的原始登记（历史快照，现状以上文为准）。

> 来源：9 条并行审查线对 ref 全部对应包逐文件对比（core×3 / ai / tui / cli / client+server+plugin+pty / schema+util / 依赖审计），随后 4 条复核线对全部初判缺口二次验证（全 workspace grep 排除"移到别处实现"误报）。汇总报告：`FULL_PORT_REVIEW.md`（v2 修正版）。
> **复核共推翻 8 项初判**（详见 FULL_PORT_REVIEW §二），其中与登记册相关的核实：rpc/event-manifest/durable-manifest 均有镜像实现（=RV-P7 闭环属实）；TUI /update UI 完整、autoupdate 默认 notify；SSE 编码失败语义与 ref 一致（踢全部订阅者）；glob brace/shell SIGKILL 无分叉；ULID 型 identifier 逐位 wire 兼容；chromiumoxide（dev-dep）/defmt（jiff 未启用 feature）不进发布闭包。
> 与既有条目去重：workerd（=RV-G6 豁免）、effect/solid 客户端变体（=RV-P13 豁免）、插件子进程架构（=R1–R5 家族）、flock 降级（§1.6 已登记）、run TTY 权限超集（=RD-15）、v1 迁移浓缩（pre-2.0.24 升级路径行 + p10 cron 已覆盖）、opentui 双轨（=G-11/FRAMEWORK-CONVERGENCE，复核补正：**solid 路径为默认主渲染**（footer 等 env unset→ON，`=0` 回退手写），OC_TABS_OPENTUI 已退役、tabs lane 单轨化）、storybook（ADR-0013 不移植）。

### 5.1 功能缺口（复核确认，未见于既有台账）

| ID | 缺口 | 域 | 严重度 | 说明（文件:行 为审查时锚点） |
|---|---|---|---|---|
| RW-01 | Anthropic 系 per-provider **body 包装**未移植：alibaba/zai/minimax/moonshot 的 thinking-免-budgetTokens 豁免、meta 的 `native.meta`→`web_search` 重映射 | AI | 高 | facade.rs:288-292 全并入通用 AnthropicMessages family；anthropic_messages.rs:319 无预算 thinking 一律报错（ref alibaba-messages.ts:29-46 先剥离再重挂）。注意 RC-06 闭环的 customization seam 只有 cache_policy/chat_metadata_key 两钩子，**无 body-wrap 表达力**——修法建议在 seam 上加 `wrap_anthropic_body` hook 一次覆盖五 provider。ref 对应 protocols/{alibaba,zai,meta}-messages.ts |
| RW-02 | persistent-pty 屏幕跟踪无终端仿真：on_output 仅处理 `\n`/`\r`/`0x08`，CSI/OSC 当可打印字节写入 rows | Core/PTY | 高 | persistent_pty.rs:1390-1419（注释自认归 Phase 9 daemon），oc-pty daemon.rs 复用同一朴素模型 → `read`/`snapshot`/`rows` 在 vim/less/带色提示符下乱码。workspace 已有 vt100 crate（oc-tui/terminal_pane.rs:139）可复用 |
| RW-03 | MCP `config.protocol` 协商不生效：固定 pin `V_2025_11_25`，配置项 advisory | MCP | 中 | mcp/client.rs:457-462 vs ref client.ts:193,393-397（legacy/auto/pin）。声明旧协议的服务器可能握手失败 |
| RW-04 | MCP OAuth 同 token 并发刷新无共享去重 | MCP | 中 | oauth.rs:840 无状态自由函数；endpoint_lock 仅按 URL 串行连接。rotation 下并发 prepare_token 第二个吃 invalid_grant（ref oauth.ts:30-51 refreshes map 明确警告此场景） |
| RW-05 | `/api/fs/find` 降级为朴素子串 walk：fff 模糊索引、ripgrep 后备层、protected.ts 敏感目录表缺失 | Server/Filesystem | 中 | handlers/fs.rs:285-316（仅跳 .git/node_modules/target）→ home 根会扫 Desktop/.ssh 等。至少应先补 protected 排除 |
| RW-06 | OTel 遥测零对应（连 tracing crate 都未引入）；oc-util observability 未移植 | Util/可观测 | 中 | ref util observability.ts + otlp.ts + 5 个 @opentelemetry 依赖。补齐路径：tracing + tracing-opentelemetry 骨架起步 |
| RW-07 | CLI `auth login` 不自动打开浏览器 | CLI | 低 | auth.rs:471-473 只 println URL；全 workspace 无 OAuth URL 打开路径（oc-tui 的 xdg-open 是开文件夹，另一功能） |
| RW-08 | `session list` 时间戳硬编码 UTC（函数名 `format_local_time` 名不副实）；无 less 分页；`session export` 无交互选择器 | CLI | 低 | session.rs:96-124/91/139-141 vs ref toLocaleString + less -R -S + autocomplete。RV-B8 已为 stats 引入 jiff 本地时区，可复用 |
| RW-09 | biome formatter 解析差异：查 `node_modules/.bin/biome` 且不自动安装 | Core | 低 | formatter.rs:632-638,572 vs ref `npm.which("@biomejs/biome")` 可装 → 有 biome.json 但未装包的项目 **静默不格式化** |
| RW-10 | 表单已结算条目无 TTL 淘汰（RETENTION_MS 常量零使用）；`is_date_time` 恒真 | Core | 低 | forms.rs:15 / :716-719。内存缓慢泄漏 + date-time 校验形同虚设 |
| RW-11 | PersistedRevert 解码变换缺失（revert 列原样透传、聚合恒 None） | Core/迁移 | 低 | v1_migration.rs:1447-1457；v1 迁移主体（1959 行 + API + battery）在。读 TS 版历史 revert 数据会丢 |
| RW-12 | LocationActivity 缺 bus 自动 touch 与清扫期 re-touch 复查 | Core | 低 | location_services.rs:139 TODO 自认；需与 RV-G2 的"location 活动 re-arm"闭环范围交叉核对，若 RV-G2 已覆盖 re-arm 则本条收窄为 bus 订阅 touch |
| RW-13 | oc-util 真缺失：cross-spawn-spawner(556行)、open 完整跨平台版 | Util | 低 | npm/patch/bom/calendar/title-fallback 等其余初判"未移植"经复核均为换机制实现（bun 子进程 / oc-core tool/patch.rs / file_mutation / 注册过的简化），不列缺口 |
| RW-14 | oc-ai experimental/（evaluation、system-one）未移植 | AI | 低 | 与 RV-G5 豁免（REF 侧死代码）合并审视：ref 侧确为死代码，本条可随 RV-G5 一并豁免，登记待裁定 |

### 5.2 陷阱（行为差异/卫生问题，复核确认）

| ID | 陷阱 | 说明 |
|---|---|---|
| RW-T1 | **schema 结构体 rename 卫生**：`PermissionSaved.project_id`、`Mcp.ResourceTemplate.uri_template`、`WorktreeOperationError.force_required` 缺 `#[serde(rename)]`（camelCase） | 当前线上路径全被 handler 手工 json!/生成端 rename/契约测试兜底（wire_fidelity.rs / parity_http_contract.rs / parity_vcs_wire.rs），**无 wire 缺陷**；但直接 serde 序列化这些结构体即产出 snake_case wire。`WorktreeOperationError` 现为零使用死代码。修法：补 rename（半小时级） |
| RW-T2 | `OPENCODE_PTY_BIN` 环境路径**零校验**直接采用 | transport.rs:130-155；比 ref 弱（ref 对外部二进制做 sha256/属主/lstat 校验 binary.bun.ts）；rust 连可执行位都不查（exe-sibling/PATH 候选仅查可执行位） |
| RW-T3 | reqwest 0.12/0.13 双版本共存（同一 rustls 编译两份，非双 TLS 栈） | 0.13 供 rmcp + oc-core MCP 客户端，**进发布闭包**；cmake 经 aws-lc-sys 出现在发布构建链（build-dep）。升级/安全评审需覆盖两套，统一窗口需排期 |
| RW-T4 | tool_output 截断预算按 Unicode 标量，ref 按 UTF-16 码元 | tool_output.rs:233-251 vs tool-output.ts:93；emoji/增补平面下裁剪点不一致（golden 对拍 diff 来源之一） |
| RW-T5 | MCP 会话过期判定偏宽：remote 连接错误链含 404 即重连+重放 | client.rs:533-552（读不到 rmcp 内部 session id 所致）；"方法不存在返回 404"的 modern 服务器会多一次重连 |
| RW-T6 | glob 匹配器不支持 brace/extglob | 权限匹配两侧等价（ref wildcard 本身转义 `{}`），仅未来挪用时的分叉风险 |
| RW-T7 | 依赖卫生：serde_yaml 0.9 上游归档停维；schemars 0.8（0.9 不兼容）、notify 6、dirs 5、dialoguer 0.11 落后主版本；vt100 pin 0.15（unicode-width 冲突） | 建议升级看板；serde_yaml 迁移优先 |
| RW-T8 | `BUN_BE_BUN=1` 对 rust 空操作 | 排查 env 差异时勿被误导 |

### 5.3 与既有裁定的张力（2026-10-28 两项均已处置闭环）

| 项 | 处置结果 |
|---|---|
| environment/workspace 驱动 | **已闭环（2026-10-28，e86390d，前提修正）**：经 ref 代码核实，`mcp/stdio.ts` 本地分支以 `extendEnv: true` 调用 spawner，本地 cross-spawn spawner（cross-spawn-spawner.ts:111）扁平化为 `{...process.env, ...opts.env}`——**ref 本地分支本就继承宿主全部环境，不剥离任何变量**；剥离语义只存在于未移植的 workspace/sandbox 驱动。故 RD-08 豁免维持原范围（workspace seam），本轮"stdio 执行平面缺口"**不成立**。port 侧改动：将隐式继承抽成显式纯函数 `stdio_environment(host, command, config_env)`（宿主 env < BUN_BE_BUN〔仅 opencode 命令〕< 配置 environment），`env_clear().envs(...)` 显式应用 + 5 个单测钉住语义；oc-core 1194/1194 |
| run `attached:false` 无条件硬编码 | **已闭环（2026-10-28，25dfc66）**：新增 `attached(Option<&StandaloneServer>)` 辅助——共享目标（显式 `--server` 与托管后台服务）传 `attached: true`（对齐 ref run.ts:149 `?? true`，全局表单清扫跳过），standalone 独占租约保持 `false`（attached_client_leaves_global_forms_alone 无回归）；ref 无 attached CLI flag（仅内部从 `input.server !== undefined` 派生），故不加 flag 管道；3 个新测试覆盖三分支；oc-cli 238/238。合并门（两 crate 同树）：nextest 1432/1432（.test-logs/merge-gate-rw53-20261028.log） |

---

## 6. 2026-10-28 全域对比审查（九通道：session/engine、AI/provider、server/http、config/project、tools、MCP/plugin/skill、TUI、CLI/client/schema、依赖）新增缺口与复核结论

> 方法：ref packages/{core,server,cli,tui,client,ai,protocol,schema} ↔ rust crates/oc-*，逐文件枚举对比、不采信仓内报告；下表仅收录**经 orchestrator 二次代码复核确认**的条目（证据 = 本仓代码位置）。与 §1/§5 已登记条目去重：workspace seam（RD-08/RW 系列）、Web UI（R-04）、LSP（G-09）等不重复列。

> **2026-10-29 aud-burndown 收口（deepwork .slim/deepwork/aud-burndown/，三道 oracle gate 均 GO-WITH-FOLLOWUPS、followup 全消化）**：本节 AUD 系列全部处置完毕——
> **已闭环（实现）**：AUD-01（MCP guidance 权限过滤，engine 缓存 raw summaries + 指令读取期按 per-session merged ruleset 过滤 execute/逐工具 deny + codemode 标志，d4b85ef）；AUD-02（附件物化管线 oc-core/session/prompt.rs：data-URL/file:// 读取/行切片/目录列表/双 20MiB 上限/图片归一化，oc-server delegation；HTTP 边界 None image limits = ref resizer-unavailable 透传臂，登记 F-1）；AUD-03（reference 域：repos 缓存/刷新/巡检 + 指令块字节平价 + reference.updated 总线事件 F-5，218211a）；AUD-04（compaction trigger 透传：manual 不再伪报 started(reason:"auto")，runner eager manual started 保留，2731334）；AUD-05（HTTP form-location `"global"` 逃生门五路由接线，2731334）；AUD-06（tool-input-repair 9 类修复 + HookRegistry::keep() 健全注册 F-2，218211a）；AUD-07（rg 解析链 env→bundled→PATH→缓存→自动下载 15.1.0 + exit-2 容忍部分结果 + 仅限流中断才 kill 保真实退出码 F-3，019fce2）；AUD-08（read 256KiB 分块 + 已消费区间 NUL 检查，019fce2）；AUD-09（session.prompt hook 移至准入路径：materialize→hook→revert-commit→admit、失败拒绝请求不毒化 inbox 行、PreparedRing 防双发、drain 侧留引擎内 admits 兜底 + 重复 id 先 reconcile 不重发 hook F-09a，13bd9e7）；AUD-10②（cerebras/kilo/nvidia/llmgateway 四家静态头注入 provider 插件字节平价 + facade 合并优先级 user<plugin<model，5d1016b）；AUD-14（AGENTS.md 变更 per-file 三态 diff 增量渲染 + 短者胜；watcher 时序维持登记分歧，0d4d1f2）；AUD-16（snapshot diff context 参数 `context??3` + git-presence 门（files/diff 不查 enabled=ref 本义，F-16a），0d4d1f2）；AUD-18（非法 sessionID 400 无 kind 信封；仅精确 `"global"` 逃生，0209adc）；AUD-19（webfetch Content-Length 预检 + timeout 校验 0<t≤120 不 clamp，0209adc）；AUD-20①（OAuth 回调 HTML 卡片页 mcp/oauth_page.rs，错误腿即时 400 text/html、404 保持纯文本，0209adc）。
> **销账（误报）**：AUD-10① venice——实为路由级协议已移植（openai_chat.rs:18-24,5000 + 6 recordings），§6.2 该行同日改判；AUD-13——与 RA-07 重复（17/17 门控 feature 已落地）。
> **具名豁免/登记（oracle 双侧证据裁定，rulings-p2.md）**：AUD-10③ modal 远端目录（需通用远端目录 seam 先行，models-dev 快照家族共同缺口）；AUD-10④ copilot 重写（零 recordings 无平价 oracle，组合复用已有单测+facade 测试覆盖；解锁=采集 ref recordings）；AUD-11（ref 自身仅构造 StreamableHTTP、无 SseClientTransport，rmcp 3.7 无 SSE 传输——非缺口）；AUD-12（v1 bridge=已废弃面，数据迁移 G-06 已移植；schema lib.rs TODO=文档卫生，多数条目实际已做 G-12/refresh.rs）；AUD-15（Git.sync 唯一 ref 消费方=reference 缓存、已随 AUD-03 内联落地 repository_cache.rs；index.ignored=snapshot is_ignored；Vcs adapter/patch.ts 折入插件 Context 域豁免）；AUD-17（join 5s 有界超时+Option 序列化=有意取舍，维持登记）；AUD-20 其余（OAuth 去重 key 替身已文档化、插件 Context 域 vcs/websearch/worktree/reference/experimental.terminal、KV 门面、凭据明文双侧一致）；F-10a（custom lane Activation::Enabled 硬编码，TODO(activation-gate) 注释登记）。
> **验证**：canonical nextest 全 workspace 4616/4616 绿（.test-logs/merge-gate-p3-20261029.log）+ pty 序列化 profile 4616/4616 绿（.test-logs/final-pty-20261029.log）；逐 lane 证据 .test-logs/*aud*。下表为 2026-10-28 审查时的原始登记（历史快照，现状以上文为准）。

### 6.1 新登记缺口（按优先级）

| ID | 缺口 | 优先级 | 复核证据（rust / ref） |
|---|---|---|---|
| AUD-01 | **MCP instructions 汇总缓存 `codemode` 硬编码 `true`，且未按 `Permission.evaluate("execute")` 过滤工具/server**：权限收紧后 server 指引仍进系统提示（信息泄露）；codemode=false 的 server 提示行错误 | P0（安全） | oc-core/src/engine/mod.rs:1057（`codemode: true` 字面量）vs ref mcp/instructions.ts:63-108（按权限过滤 + 逐工具 codemode 标志）；汇总仅 500ms 轮询刷新（engine/mod.rs:1050-1070） |
| AUD-02 | **服务端 prompt 附件物化管线缺失**：ref 在提交期对 files 做 data-URL 解码 / file-URI 读取 / start-end 行切片 / 目录列表物化 / MAX_ATTACHMENT_BYTES 上限 / 图片归一化；rust 全仓无对应（TUI 侧 composer/attachment.rs 仅客户端编码，不构成服务端校验） | P0（正确性/资源） | ref core/src/session/prompt.ts:54,93-225（`materializeAttachment`、`MAX_ATTACHMENT_BYTES`）；rust oc-core 无 `materialize`/`MAX_ATTACHMENT` 实现（grep 全仓仅命中无关注释） |
| AUD-03 | **reference 域为 TODO 桩**：`core/reference-guidance` 指令块恒空（read 恒 Removed），git reference 仓库缓存/每日刷新/每小时巡检均无 | P1 | oc-core/src/session/runner/context.rs:620-624（`TODO(wave-C)` 自认）vs ref reference.ts:33-128 + reference/instructions.ts:20-95；oc-util/global_roots.rs:124-127 仅预留 `repos` 目录 |
| AUD-04 | **native compaction 手动路径 trigger 硬编码**：手动 `/compact`（native 模型）错误发布 `session.compaction.started(reason:"auto")` 且跳过手动路径约束 | P1 | oc-core/src/session/runner/compaction.rs:496（`self.deliver(Reason::Auto, …)` 无条件）vs ref compaction.ts:324-372（透传 trigger） |
| AUD-05 | **HTTP FormLocation 的 `"global"` 逃生门未接线**：core 侧哨兵已存在（elicitation.rs:166 `GLOBAL_ELICITATION_SESSION_ID`，elicitation 表单确实以 `session_id="global"` 落库），但 handlers/session.rs:126 `parse_session_id` 无特殊分支、`session_location`（common.rs:270-278）直接查 session 表 → 经 HTTP 查询/应答该表单得 404。收窄说明：这是**路由层缺口**而非整域缺失 | P1 | rust common.rs:270-278 vs ref middleware/form-location.ts:27-38（`sessionID === "global"` 直查全局 Form.Service） |
| AUD-06 | **tool-input-repair 内置插件缺失**：字符串化/宽松类型工具参数修复（`'{"limit":"20"}'`→`{limit:20}` 等 9 类）无对应。注意：oc-ai 的 partial_json.rs/sanitize.rs 是流式解析/代理字符，**不是**等价物 | P1 | ref core/src/plugin/tool-input-repair.ts:1-30+；rust 全仓 `repair` 命中仅 partial_json/sanitize/markdown（均无关） |
| AUD-07 | **rg 无自动下载**：ref 依次系统 rg→全局缓存→自动下载 GitHub release 15.1.0；rust 仅 `OPENCODE_RG_PATH`→bundled→PATH，无 rg 环境直接失败。另：rg 退出码 2（权限/config 类错误）rust 一律报错，ref 容忍并返回部分结果 | P1 | oc-core/src/tool/mod.rs:119-139 vs ref ripgrep/binary.ts:13-136；tool/grep.rs:180-186 vs ref ripgrep.ts:148-151 |
| AUD-08 | **read 工具两处行为偏移**：① 整文件 `std::fs::read` 载入（ref 256KiB rope 分块），大文件内存峰值；② 分页路径对整个文件做 NUL 二进制检查（ref 仅检查已消费区间）→ >50KiB 二进制文件分页读整页报 Binary | P1 | oc-core/src/tool/read.rs（read_file=254；NUL 检查为全文级）vs ref read-filesystem.ts:14-402,151-209 |
| AUD-09 | **prompt hook 时机架构性差异**：ref 在提交时（入 inbox 前）触发 `session.prompt`，失败即拒绝请求；rust 在 Ready 边界对已落库行触发，失败毒化/取消该行——API 错误码与时序可观察不同 | P2 | oc-core/src/session/runner/prompt_prepare.rs:1-26,273-305 vs ref core/src/session/prompt.ts:29-52 |
| AUD-10 | **provider 长尾缺口**：① venice/zai-chat 个性化 body 扩展未在 customization.rs 找到条目（alibaba/zai-coding-plan/zai-messages 已确认在，customization.rs:180,333）；② 头部注入类 provider 插件（cerebras `X-Cereras-3rd-Party-Integration` 等 ~16 家）无对应；③ modal.com 远端模型目录缺失（provider_catalog 仅 models-dev 快照）；④ copilot 为组合复用（~700 行）vs ref 独立实现（~4k 行），边缘行为依赖 recordings 验证 | P2 | oc-ai/src/customization.rs:180-337（zai/alibaba 在、venice 无）；oc-core/src/provider_catalog/（无 modal 源）vs ref plugin/provider/*.ts（34 文件）+ modal/models.ts |
| AUD-11 | **MCP 仅 SSE 的旧服务器无传输回退**：只有 StreamableHTTP + stdio；ref 经 SDK 对 legacy SSE 服务器内部降级 | P2 | oc-core/src/mcp/client.rs:1099-1210（仅 connect_remote/connect_local，无 SseClientTransport） |
| AUD-12 | **CLI run v1 兼容 bridge 缺失**（runV1Bridge）；**oc-schema lib.rs:80-83 自认 TODO**：v1 legacy schema、rpc.ts、pty-ticket.ts、models-dev、event-manifest 集中索引未进 oc-schema（事件清单分散 3 模块 + oc-core registry） | P2 | oc-cli/src/run/ 无 v1 对应；oc-schema/src/lib.rs:80-83 |
| AUD-13 | **TUI 语法高亮语言覆盖缺口**：缺 bash/csharp/kotlin/swift/haskell/julia/ocaml/clojure/vue/hcl（未知 filetype 纯文本回退）；无远程 wasm 加载（安全上反而更收敛）。注：与 G-08/G-11 的 20 门目录现状一致，此处登记 ref 全目录差距 | P2 | oc-syntax/lib.rs:1-408 vs ref parsers-config.ts:6-386 |
| AUD-14 | **AGENTS.md 运行时变更语义弱化**：无 per-file watcher 自动重扫（依赖 config watch 通道）；变更时无 unified diff 增量渲染（ref removed/added/changed 三态 + 短者胜 diff），rust 全量重发（token 放大） | P2 | oc-core/src/config/instruction.rs:65-110（纯函数发现）vs ref config/plugin/instruction.ts:31-118 + instruction-discovery.ts:123-149 |
| AUD-15 | **git 同步面缺失**：Git.sync（clone/fetch/checkout/resetHard）与 index.refresh/ignored、Vcs adapter（status/diff/base/initialize）无 core 对应；识别/快照/worktree/branch-list 已由 git2 覆盖。vcs/patch.ts 的大上下文（2^31-1）patch 语义亦无独立对应（file_diff.rs 固定 4 行上下文） | P2 | oc-core/project.rs（仅识别）、oc-server/handlers/project.rs:285-535（worktree/branch）vs ref git.ts:1-758、vcs/patch.ts:1-60 |
| AUD-16 | **snapshot diff 忽略 `context` 参数**（`_context: Option<u32>` 未用）；enabled 门槛未检查 location vcs 类型 | P2 | oc-core/src/snapshot.rs:609 vs ref snapshot.ts:88-233 |
| AUD-17 | **shell 工具 join 有界超时 5s**（ref 可无限等待卡死工具 fiber，rust abort 后按 interrupted 分类）；工具 metadata 的 signal/timeout 为 `Option`+skip_serialize（ref 恒输出，含 false/undefined 档）——字段**已存在**（见 6.2 复核推翻），仅序列化存在性有微小差异 | P3（有意取舍，登记备查） | oc-core/src/session/runner/step.rs:44 vs ref step.ts:87,136-144；tool/shell.rs:107-118 |
| AUD-18 | **非法 sessionID 报 404 而非 400**：ref 对格式非法的 sessionID 先 400 InvalidRequestError；rust `parse_session_id` 回退原值直接查表 404 | P3 | oc-server/handlers/session.rs:126 vs ref location.ts:56-59 |
| AUD-19 | **webfetch 无 content-length 预检**（超声明体积 ref 不下载，rust 流式读到 5MiB 上限才断）；timeout 用 clamp(1,120) 而非 schema 拒绝非法值 | P3 | oc-core/src/tool/webfetch.rs:73+ vs ref tool/http-body.ts:14-17 |
| AUD-20 | **杂项**：OAuth 回调页纯文本（ref HTML 页，UX 降级）；OAuth 刷新去重 key 用 config.url 而非 token endpoint（oauth.rs:913-920 已文档化偏差，多 AS 同源场景理论误共享）；插件 Context 域 vcs/websearch/worktree/reference/experimental.terminal 无 port；KV 无通用门面（非插件调用方直接摸表）；凭据两侧均明文 SQLite（非回归，共同改进项） | P3 | 各通道明细表 |

### 6.2 复核推翻/修正的通道结论（勿再登记）

| 原结论 | 复核结果 |
|---|---|
| "shell metadata 缺 signal/timeout/truncated:false 字段" | **不成立**：`ShellToolOutput`（oc-core/src/tool/shell.rs:107-118）已含 `exit/signal/shell_id/truncated/timeout/status` 全字段且接线（:355-358）；仅 Option 字段 skip-serialize 与 ref 的恒输出存在序列化存在性微差，降级并入 AUD-17 |
| "FormLocation `sessionID==="global"` 逃生门整体未移植" | **收窄**：core 哨兵 `GLOBAL_ELICITATION_SESSION_ID`（mcp/elicitation.rs:166）存在且 elicitation 表单确实落库为 `"global"`；缺的是 HTTP handlers 层放行（→ AUD-05） |
| "hg VCS 缺失" | **修正**：project 身份的 hg 支持**已移植**（oc-core/src/project.rs，hg log -r roots(all()) 语义一致）；缺的仅是 ref plugin/vcs/hg.ts 的 VCS 信息工具面 |
| "venice/zai 未覆盖" | **部分修正**：zai-coding-plan 与 zai-messages body wrap 均在（customization.rs:180,333-337，有测试 ：455）；**venice 未找到**条目，维持 AUD-10① |
| "rg 退出码 2 rust 直接失败" | **维持**（AUD-07 一并登记），但补充：参数构造逐项一致，差异仅在错误容忍与二进制获取 |

### 6.3 复核后确认无需登记（与 ref 持平或有意的文档化取舍）

- 键位引擎/剪贴板/通知/主题/QR（uqr golden 对拍）：逐文件移植，测试钉死。
- openapi.json 与 ref **md5 逐字节相同**（141 ops / 256 schemas 双向零差异），oc-server include_bytes 自host——全移植最强一致性锚点。
- 141 HTTP 路由 1:1；鉴权/SSE/PTY-WS 边界语义（base64 padding 容错、JS f64 整数语义、cookie 端口命名、ticket scope、WS 4404/1000）均有测试。
- 凭据明文存储：两侧一致，非移植回归。
- `ignore@7.0.5`（ref package.json 声明但源码无 import）：rust 未替代属正确。
- reqwest 双版本 / OTel optional / git2 内嵌：见 RW-T3、RW-06 及依赖对比通道结论，已有登记不重复。

---

## 7. 2026-10-29 六域审查缺口专项复核（RE 系列）

> 来源：本日六通道全量对比审查（core / TUI / CLI+client / server+schema / 插件+AI+基础设施 / 范围盘点，汇总见会话报告）提出的候选缺口，经两条并行 oracle 复核线对当前代码（AUD 收口之后）逐项验证并与 §1/§4/§5/§6 全量去重后登记。**复核推翻三项审查通道初判**（见 §7.2），**销账一项**（websearch MCP provider = RV-P4 已闭环，oc-core/tool/websearch_providers.rs:3-94 实现确认；ora 初判"未见对应"过期）。

### 7.1 新登记条目

> **2026-10-30 re-burndown 收口（deepwork .slim/deepwork/re-burndown/）**：本节 RE 系列全部处置完毕——
> **已闭环（实现）**：RE-02（plugins 对话框 per-plugin active 状态 + `plugins.toggle`，enter 切换且对话框保持打开，TUI-local 语义与 ref context.tsx:150-231 平价——ref 不写 config、不走 host API，G-05 的 entry 级 add/remove 非本机制故不接线）；RE-03（frecency 移植完整：`frequency/(1+Δdays)` 衰减 + `×(1+f)` 评分因子 + accept 侧记录 + `<state>/frecency.jsonl` append/1000 条 compaction/绝对路径键，frecency.tsx 全行为）；RE-04（masked-text 原语家族：masked-text 基类 → shimmer-text（1200ms 扫光）+ fade-in-text（200ms feather-8 wipe），接线 home 更新横幅 fade 与 update 对话框 shimmer spinner，钉时钟确定性测试）；RE-05（clap 四处：run --file/auth answer atMost(100) 走 RC-12 validate_extra、max-count range(1..)、cors NonEmptyString，边界测试 100/101、0/1、空串）；RE-06（消费侧类型化：oc-client `bus_event.rs` EventKind 枚举镜像 event_manifest + 大声 envelope 解码 + `rpc_typed.rs` 类型化 call 包装 + oc-tui BusEvent typed dispatch + 双向 drift guard——开发期即抓到 12 个缺失 wire 类型）；RE-09（按 oracle 裁定删除 RpcPort/PluginSupervisor 死抽象，净 -120 行零行为变化，TODO 换指向 oc-server PluginHost 的文档注释，rulings-1.md）；RE-13（/btw 全链路：palette/slash 真实 dispatch（不再 surfaceless 吞掉）+ session.generate 一次性侧答（oc-core tests_generate 证无 transcript 污染）+ 大号 markdown 滚动/copy 对话框 + prompt.footer.status spinner）；RE-14（/stats 页：stats.open 命令 + StatsPoster（大字年度计数、M-W-S 活动热力日历、指标），stats.tsx/stats-data.ts/activity-calendar.ts 三文件移植）。
> **具名豁免（oracle 双侧证据裁定，rulings-1.md）**：RE-08（promise ctx 五域 aisdk.hook/model/provider/integration/plugin.list——ref 侧零 shipping 消费者，全部内建走 Effect host，外部旗舰插件与 V2 不兼容 issue #8485；出现真实消费者时各域均为机械 hostRequest 透传）；RE-10（SDK EmbeddedHost/OwnedFetch——ref 仓内亦零消费者、纯外部嵌入 API，无 Rust 嵌入方；workerd 已由 RV-G6 豁免；触发条件=首个 Rust 嵌入请求，axum router 为天然 seam）。
> **维持**：RE-01 推迟（用户裁定）、RE-07/11/12 登记项无码变更。
> **验证**：逐 lane 合并门全绿（oc-cli 246 / oc-tui 1304→1379 / 四 crate 2971 / 1817 / oc-core+server 1616），日志 .test-logs/*merge-gate-re*。附带修复：lane 提交身份漂移（子代理误把 dev-gate 降权执行身份推广为 git 作者）——仓库本地 [user] 配置钉死 + filter-branch 重写 main 受影响 2 提交 + re03 重置 author。

| ID | 缺口/陷阱 | 域 | 严重度 | 复核证据（rust / ref） |
|---|---|---|---|---|
| RE-01 | **updater 平台分支与方法探测缺失**（**状态：推迟（2026-10-29 用户裁定；windows-x64 维持 R-11 实验性产物口径，自升级/卸载能力补齐延后，待 windows 用户面出现再裁定是否补 win32 分支）**）：`oc-cli/src/services/updater.rs` 为 unix-only（`std::os::unix::fs::PermissionsExt` 无 cfg 门控 :716/:797，windows 目标不可编译该模块）；ref updater.ts 的 win32 家族（`opencode.exe` 命名 :180、Git Bash 包装 `OPENCODE_GIT_BASH_PATH`→`git --exec-path`→bash.exe :184-198、RetainedImage 运行中二进制保留 :322-330）与 `method()` 的五包管理器已装包探测循环（:201-227，vp 需 JSON 精确解析防子串误报）均无对应——rust `detect_method` 仅 curl/brew（:244-261，注释自认包探测有意缺席）。**与 R-11 的矛盾**：windows-x64 实验产物已发布但该二进制无自升级/卸载路径（uninstall shell_configs 亦为 unix shell 清单 :912-940），按推迟裁定接受该缺口。OPENCODE_RELEASE_URL 换源语义已登记（RA-02/PORT_REPORT §3），不重复 | CLI/平台 | **中（推迟）** | updater.rs:716,797,244-261 / updater.ts:180-330 |
| ~~RE-02~~ | **plugins 管理对话框无 enable/disable**：`oc-tui/src/store.rs:98-99` 自述"enable/disable state is not modeled (submits close)"（近似实现，提交即关闭）；ref `feature-plugins/system/plugins.tsx:119` 按注册插件 `active` 状态切换 enable/disable。用户无法从 TUI 停用插件。G-04 只闭环了 UI 槽位挂载，不覆盖本项 | TUI/插件 | 低-中 | store.rs:98-99 / plugins.tsx:119 |
| ~~RE-03~~ | **@file 补全无 frecency 加权**：`oc-tui/src/components/composer/autocomplete.rs:671-676` 评分 = key-sum×2，无 frecency 因子；`oc-tui/src/storage.rs:270` + `oc-cli/src/cli_config.rs:574-609` 只做 frecency.jsonl 的解析/迁移，全仓无打分消费点。ref `component/prompt/autocomplete.tsx:569-570` 乘 `(1 + frecency.getFrecency(path))`，衰减公式 `prompt/frecency.tsx:22-25`（frequency/(1+Δdays)）——高频文件排序与 ref 分歧 | TUI | 低 | autocomplete.rs:671-676 / autocomplete.tsx:569-570 |
| ~~RE-04~~ | **动画原语族残缺**：fade-in-text（home 更新横幅静态绘制 `home_solid.rs:540+` vs ref `routes/home.tsx:17`）、shimmer-text 横向扫光（`mini/spinner.rs` 仅 glow levels 近似 vs ref `component/spinner.tsx:8,45`）、masked-text 遮罩渲染原语（shimmer 家族基类 `component/masked-text.ts`，rust 无对应）无对应。**title-shimmer 已实现**（`sidebar.rs:282-318`、`session_tabs_solid.rs:662/773`）不在列；tab-pulse/marquee/spinner/RW-19 既有登记维持 | TUI | 低（视觉保真） | home_solid.rs:540 / spinner.rs / masked-text.ts |
| ~~RE-05~~ | **clap 层四处校验缺口**（RC-12 同族残留，修法可复用 `try_parse_from_checked`/`validate_extra`）：`run --file` 无 atMost(100)（cli.rs:395 vs commands.ts:410-412）、`auth login --answer` 无 atMost(100)（cli.rs:505-507 vs :167-169）、`session list --max-count` u32 允许 0（cli.rs:616 vs Schema ≥1）、`serve --cors` 接受空串（cli.rs:435 vs NonEmptyString，commands.ts:542-546）。api --header 已由 RC-12 闭环不重复 | CLI | 低 | oc-cli/src/cli.rs 各行 / commands.ts 对应行 |
| ~~RE-06~~ | **oc-client 事件/RPC 消费侧弱类型**：`oc-client/src/event_ext.rs:55` `SharedEvents<serde_json::Value>`；`shared_events.rs:48` 靠 `get("type")` 字符串探测；oc-tui 消费侧 `data.rs:74-76` 同（BusEvent 弱类型）。`rpc.call` 为泛型单方法（`methods.gen.rs:2630-2633`，`RpcInput/RpcOutput` 内芯 `Option<serde_json::Value>`，types.gen.rs:5904-5916），无 per-definition 类型化 input/errors/events 解码（ref `client/src/rpc-runtime.ts:12-50` Schema 驱动）。服务端发射侧已类型化（G-12/RV-P7）、RV-P5 闭环的是共享复用器——缺的是**消费侧镜像**，schema 漂移会在 oc-client/oc-tui 静默坏 | Client/TUI | 低（技术债/健壮性） | event_ext.rs:55, methods.gen.rs:2630 / rpc-runtime.ts:12-50 |
| RE-07 | **serve SIGTERM 优雅退出为未登记超集**：`oc-cli/src/handlers/serve.rs:221-235` SIGTERM 安装 + bounded_drain（:37，gate-6 F2）+ 超限强制退出（:45）；ref serve handler 无任何信号处理（kill 即死）。容器 stop 语义更优，按 RD-15 先例登记为具名超集，防后续对表误判为分歧 | CLI | 信息（超集） | serve.rs:221-235 / serve.ts（17 行无信号处理） |
| RE-08 | **插件 ctx 缺失域清单补全——具名豁免（2026-10-30，rulings-1.md：ref 侧零 shipping 消费者，五域全豁免）**原登记：：ref `packages/plugin/src/promise/adapter.ts` 的 `ctx.aisdk.hook`（:301-304）、`ctx.model`（list/default/transform/reload，:342-347）、`ctx.provider`（:348-353）、`ctx.integration`（OAuth/connect.key/command connect 全族，:354-430）、`ctx.plugin.list`（:446）在 host.js `makeCtx`（:575-860，仅 plugin{id}/app/directory/location/options/agent/mcp/command/skill/tool/session/generate/permission/shell/rpc/storage/theme/event/ui）均无 port。RV-P12 豁免的是 `aisdk:` npm 包桥接（loadPackage），**不覆盖** aisdk.hook 钩子面。随 AUD-20 同性质定性（shipping 插件消费情况未证） | 插件 | P3 | host.js:575-860 / adapter.ts:301-446 |
| ~~RE-09~~ | **oc-core `RpcPort`/PluginSupervisor 为未接线死抽象（双轨卫生债）**：`oc-core/src/hooks.rs:1442` RpcPort trait（"wire to the server router" 未接线）、`:1561` PluginSupervisor host slot TODO、`:1647` "nothing to boot in Wave 0"；唯一实现是 NoopRpcPort（:1937-1957），engine 两处注入（engine/mod.rs:586/:1517）；oc-plugin-host 全 crate 零引用该 seam；`session/runner/drain.rs:236` `plugins.awaitActivation` 同族 TODO。实际 RPC 面由 oc-server 直连 PluginHost 承担（plugins.rs:140-160、handlers/misc.rs:26）。处置二选一：删除/降级为文档，或注释指向已落地的第二轨道 | Core/插件 | P4（清理/收敛） | hooks.rs:1442,1561,1647 / — |
| RE-10 | **packages/sdk 嵌入式宿主形态无对应——具名豁免（2026-10-30，rulings-1.md：待首个 Rust 嵌入消费方）**原登记：：ref `packages/sdk/src/internal/host.ts:35-60` EmbeddedHost.create（createEmbeddedRoutes in-process 路由 + Web handler）+ `fetch.ts:8-41` OwnedFetch（零监听 fetch transport，db 默认 :memory:）——库方式嵌入 opencode 的 API 面；rust 侧 oc-server 仅监听型（boot_and_serve），state.rs 的两处 "embedded" 注释（:42 passwordless、:94 boot 变体）语义均非此物。workerd 云端变体已由 RV-G6 豁免，通用 in-process 形态此前未具名。Rust 生态嵌入消费方待证 | Server/SDK | P3~P4 | state.rs:42,94 / host.ts:35-60, fetch.ts:8-41 |
| RE-11 | **TUI 插件 ctx stub API 面（具名豁免登记）**：host.js:2082-2174 —— client（rejecting proxy "tui-bridge client stub unavailable"）、event.on（:2128）、keys.map（:2129）、keymap layer/dispatch（:2130）、mode.set/push/pop（:2131）、attention.request/notify（:2132-2135，恒 `{ok:false, reason:"attention_disabled"}` 定向降级）、kv.get/set（:2136）、ui.dialog.open/close/alert/confirm（:2144）、plugins.list→[]（:2174）均为空实现；G-04 仅闭环槽位挂载。无 shipping 插件消费证据，按具名豁免登记并锚定行号，防后续审查重复发现 | 插件/TUI | P4（豁免） | host.js:2082-2174 / ref 对应 solid ctx |
| RE-12 | **ACP 依赖线信息项**：vendored `third_party/agent-client-protocol` 3.3.0（zed Rust crate 线）vs ref `@agentclientprotocol/sdk` 1.6.0（npm 线）——同一 ACP 协议的两种语言实现，版本号不可直接比较；协议版本两侧一致 pin V1（`oc-cli/src/acp/service.rs:61`，`tests/acp_integration.rs:566-579` capabilities 平价断言）。上游两线独立演进，升级任一侧需重跑平价测试防漂移 | CLI/ACP | 信息 | service.rs:61, acp_integration.rs:579 / ref package.json:27 |
| ~~RE-13~~ | **`/btw` 侧问插件本体缺失（palette 假命令）**：`session.aside` 已注册进 keymap/palette（keymap/table.rs:617）但 dispatch 归入 "registered-but-surfaceless" 静默吞掉（command_palette.rs:2856-2860，注释自认 no component answers）；ref `feature-plugins/prompt/btw.tsx:23-86` 的完整链路——`/btw` slash、`session.generate`（no-tool 一次性侧答）、Answer 大尺寸 markdown 对话框（可滚动/copy）、`prompt.footer.status` pending spinner——oc-tui 全仓零对应（grep btw/session.generate 零命中）。用户可见：palette 选 "Ask a side question" 无任何反应（比缺失更差的假命令）。G-04 只闭环槽位挂载，不覆盖插件本体 | TUI/插件 | 低-中 | command_palette.rs:2856-2860, keymap/table.rs:617 / btw.tsx:23-86 |
| ~~RE-14~~ | **`/stats` 使用统计页本体缺失**：ref `feature-plugins/system/stats.tsx:176-203` 注册 `stats.open` palette/slash 命令 + `stats` plugin 路由，渲染 StatsPoster（stats.tsx:30-134：年度会话数大字海报 + M-W-S 活动热力日历 + 花费/streak 指标，stats-data.ts 指标聚合）；rust 侧 `plugin_page.rs` 仅是通用插件路由页（:30 的 "stats" 只是测试字符串），无命令注册、无 StatsPoster/活动日历实现。server 端点已就绪（oc-server/src/handlers/session.rs:2049 session.stats），纯 TUI 呈现层缺席。RC-10（分层信息）/RB-09（测试盲区）/RV-B8（时区）均不覆盖本项。**复核补注（2026-10-29）**：ref 10 个内置 feature-plugins 中 7 个本体已原生移植（home/prompt footer、sidebar context/footer/mcp、notifications、diff-viewer），storybook 已裁定（G-07）、plugins.tsx 缺口=RE-02 | TUI | 低-中 | plugin_page.rs:1-36, handlers/session.rs:2049 / stats.tsx:30-203 |

### 7.2 复核推翻/修正的审查通道初判（勿再登记）

| 原审查结论 | 复核结果 |
|---|---|
| "websearch 经 MCP 的 provider 未移植" | **不成立（销账）**：RV-P4 已闭环，`oc-core/src/tool/websearch_providers.rs:3-94` 实现确认（tools/call JSON-RPC + SSE 双腿 + fake-MCP 集成测试）。§4.5 RB-10 行"MCP-hosted 仍 TODO"为历史快照残留表述 |
| "dialog-config（Settings）子集化，只暴露 rust 实际读写项" | **不成立**：`dialog_config.rs:74-77` 自述"full v2.0.24 table"，逐项 title 计数 rust 34 = ref 34（settings 数组逐项对齐）；读写经通用 settings_snapshot_value/set。非子集 |
| "共享 flag 全局化（--yolo 等对未声明子命令生效）为行为面扩大" | **不成立（语义等价）**：rust PermissionParamsGlobal 三 flag `global=true`（cli.rs:70-79）＝ ref 把 PermissionParams 摊进 Root params（commands.ts:26-42）的可达面；clap 全局化是实现手段非行为差异 |
| "title-shimmer 动画缺失" | **过期**：`sidebar.rs:282-318` + `session_tabs_solid.rs:662/773` 已实现（已从 RE-04 剔除） |

> 总判定：本轮六域审查的候选缺口经复核后确认 12 项登记（RE-01 中、RE-02 低中、RE-03~06 低、RE-07/12 信息、RE-08~11 插件/SDK 域 P3~P4），推翻 4 项通道初判；其余候选均与既有条目（RA-02/RA-07/G-11/G-13/RC-12/RV-P4/RV-P5/RV-P12/RW-T3/AUD-13/AUD-20/F-10a 等）去重合并。收口优先级：RE-01（windows 产物无自升级与 R-11 的矛盾需裁定）→ RE-02 → RE-08（AUD-20 豁免清单补全）。
