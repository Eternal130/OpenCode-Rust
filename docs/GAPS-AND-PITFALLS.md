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
| G-14 | Web UI 功能一致性 E2E 验证 | 开放 | 前端 = 内嵌 ref 构建（app_archive.rs:44），不移植；一致性等价于服务端契约一致性。已对表：141 路由（含错误 _tag）、SSE 帧字节级、WS+ticket、auth/CORS/location。残留 4 类风险：① handler 长尾仅签名级验证（高频端点才做了 body 级）；② G-12 七类事件无 schema golden 对表，UI 按 shape 解析会**静默坏**；③ 时序/边界（分页 cursor、Last-Event-ID 断线重连、竞态）只能真跑；④ 资产钉定漂移（ref 前端演进需重建嵌入）。闭环 = 六场景浏览器冒烟（auth、会话/prompt 流式、文件树/mentions、PTY 面板、权限弹窗、事件重连）挂 **runtime-gates 周 cron（GitHub Actions runner，不占本机；首次稳定化本地错峰跑一轮再去 flaky 后再上 cron）**；G-12 类型化后的 golden 对表为纯 cargo 测试，进**常规 PR CI**（秒级确定性，勿等周 cron）；两项绿后方可宣称"功能一致已验证" |
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
| — | pre-2.0.24 旧库升级路径 | **电池已落地（2026-10-24）**：合成 stepwise k=1..48 全绿 + 4 个 canary 边界（78976eb，常驻 CI；详见 MIGRATION.md §3.1）；真实历史语料门仍是 OC_P10；**首次启动前建议备份数据目录** |
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
| RB-07 | 图像协议探测为 env 启发式 | TUI | 低 | **部分解锁（2026-10-26 lane R0）**：原登记的解锁条件（"渲染栈迁移引入异步输入泵"）经核实为陈旧——输入线程 + mpsc + 800ms boot 探测窗自 M-lane 起即已存在（app.rs boot capability probe，原为 OSC 11 + kitty 键盘查询）。R0 已在该探测窗内加入同步 DA1（`CSI c`）往返：应答 `\x1b[?{attrs}c` 中含属性 `4` = 支持 sixel（vt240 惯例，如 `\x1b[?62;4;6;8;15;22c`）；应答无 4 = 无 sixel；无应答回退 env 启发式（KITTY_WINDOW_ID/ghostty 仍为 kitty 权威快路径）——见 `oc-tui/src/image.rs::resolve_protocol` 与 `theme.rs::TerminalProbe::da1_sixel`（解析器单测 + PTY 注入测试 tests/da1_probe.rs）。**仍登记**：PNG→sixel 转码未实现（仅原始 sixel 字节直通，image.rs:15-18）——这才是 RB-07 剩余的真实成本；分析见 .slim/deepwork/render-migration-assessment/oracle.md |
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
| RC-04 | shell 取消 Drop-guard 为 fire-and-forget | 工具 | 低 | ref 用 Effect.onInterrupt 保证 remove+cancel 完成序；port `ShellCancelGuard::drop` 走 `tokio::spawn`（自述幂等），进程随即退出时取消可能丢失，靠 Recovery::Shell 兜底。处置：补 kill-race 测试 |
| RC-05 | 冲突/哨兵经 `\0` 字符串穿透事务错误通道 | Core | 低（结构债） | inbox 生命周期冲突（`"\0session-inbox-conflict\1"`）、`ALREADY_PROJECTED`、instruction 哨兵均以字符串 marker 经 `DbError::Pragma` 隧道回传；任何 strip 遗漏/前缀碰撞会把可恢复冲突变成不可诊断 DB 错误。处置：收敛为类型化 error 变体 |
| RC-06 | oc-ai 无 per-provider 定制挂载点 | AI | 低（结构债） | ref ~60 个 per-provider 实体文件；port 折叠进 `openai_compatible` 包装 + `cache_policy.rs` 特判（如 alibaba Qwen 缓存规则）。行为当前等价，但新增 provider 专属定制（openrouter 选项、groq 参数类）缺少挂载点，易退化为 if/else 蔓延。处置：预留 ProviderCustomization seam |
| RC-07 | anthropic_messages.rs 单文件 5420 行 | AI | 低（卫生） | vs ref 1673 行，膨胀主因测试内联（jeq 断言等）。处置：`#[cfg(test)]` 模块拆至 tests.rs |
| RC-08 | routes.gen.rs 头注释与实际不符 | Server | 信息 | 注释自称 axum 0.8 `{param}` 语法，实际 axum 0.7 `:param`（生成器注释，DO-NOT-EDIT 文件——修生成器模板即可）；路由匹配为线性扫描 141 条 O(n)，当前规模可忽略 |
| RC-09 | session slug 词表与 ref 不同 | Core | 信息（外观） | `slug_create` 自带 30×30 词表 vs ref TS 词表 → 生成 id 风格不同。id 为不透明字符串、无协议影响；跨版本 golden/对拍时需知晓。处置：parity 测试显式钉住 |
| RC-10 | stats 聚合与 session move 目的地校验上移出 core | Core | 信息（分层） | `session.moved` 投影在 core，但 move.ts 的 FS 目的地三分类错误在 oc-server handler；stats.ts 聚合在 server/cli。非缺失；"core 层独立测试 move/stats"这一 v2 性质在 port 不成立，审查这两功能需看 oc-server |
| RC-11 | shell 补全双侧缺失 | CLI | 信息 | ref 无 completion 命令，port 亦未引入 clap_complete。上游缺口，补齐成本极低（rust-extra 机会项） |
| RC-12 | `api --header` 无 100 条上限 / `stats --year` 等无范围校验 | CLI | 信息 | ref 有 Schema 范围校验（year 1970-9999、header ≤100）；port parse 期只拒类型错。RB-10 超集家族同类，行为面无实际风险 |

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
| RD-21 | 文档性建议（不修码） | Server/Schema | 信息 | ① RB-05 的 31 处 operationId 改名中 `server.experimental.persistentPty.*` 命名空间（含 terminal 三条复用）未在 README 逐条列举，生态工具按 opID 映射会踩——建议 tools/openapi-gen/README.md 附完整 31 条清单；② oc-schema 模块重组（ref ~85 文件→rust 34）无 ref 文件→rust 模块映射表（Llm/Rpc/ModelsDev 散落 oc-core），建议 lib.rs 或 docs 补映射降低对表成本 |

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
| 写隔离 | 每 lane 独立 worktree；串行合并；删除清单核对 | 写隔离制度（2026-10-11） |
| 合并 | 受影响 crate 构建+测试后推送；落地证明（git log -1） | 合并门（2026-10-18） |
| oracle | 版本断言+指纹+always-refresh | `tools/diff`（opentui-rust） |
| 磁盘 | 增量抑制（仓库级 .cargo/config.toml incremental=false，2026-10-25 起强制）/周期 clean；清理门 mtime≥30 分钟视为在用 | 本册 2.1#6 |
| 验证 | 交互面必须 PTY/field 级；组合模型必须源码级对比；金帧=回归套件 | 方法论登记（ROADMAP §4） |
