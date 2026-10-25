# 缺口与踩坑登记册（GAPS & PITFALLS）

> 建立：2026-10-23 ｜ 来源：ROADMAP 变更日志、FINDINGS.md、PORT_REPORT.md、docs/host-issues.md、移植进度审计（2026-10-23）
> 用途：§1 是**当前仍然开放**的缺口（ID 与 ROADMAP.md 台账一致，此处为汇总视图）；§2 是开发全程踩过的坑，每条按「事故 → 根因 → 现行规则」沉淀；§3 是现行防复发制度速查。
> 原则：坑的价值在于结构化修复——本册只收录已转化为规则或已明确登记的教训，不收纯流水账。

---

## 1. 当前缺口（截至 2026-10-24，v2.0.24 gap burn-down 收尾之后）

> **2026-10-24 burn-down 收尾**：计划 docs/superpowers/plans/2026-10-23-v2-gap-burndown.md 的全部 lane 落地后，§1 所列功能缺口**除具名豁免外全部闭环**（台账：.superpowers/sdd/2026-10-23-v2-gap-burndown/progress.md）。发布就绪判定 = 台账清零或仅余具名豁免：R-04 前端范围外裁定、R1–R5 hook 注册豁免、(b)-类随机 coin、F15 残留具名豁免（QR 编码器缺失〔`oc-cli/src/handlers/misc.rs:225` 同源〕、≥96 列横排、<54 列 URL 截断、Enter 关闭 affordance、h<36 滚动裁剪）、**console variants/模型级 package/native-protocol 接入延后**（原 lane 降范围，待 native-protocol-binding lane 重开）、**OPENCODE_FAST_BOOT/skipInitialLoading 无对等**（CONFIG-E3 登记 open，boot pill 可达后更显性）。

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
| G-11 | oc-tui 自研框架件收敛至 opentui-rust | **已处置（2026-10-24；2026-10-25 上游事实修正）** | 已收敛：clipboard（G-03 同 lane）、paste（评估后**有意不采**）、diff 解析逻辑（parse_patch 单源化，golden 字节不变）。修正（2026-10-25）：①sound 上游已补全真音频输出→**解除阻塞，待本仓收敛 lane**（bump rev + 换 opentui_core::audio，inproc 暂留为已测实现）；②语法目录 opentui（TS）同样只有 ~5 门（ref 全目录是其 TUI 自带 wasm 运行时，非 opentui core）——20 门目录是本 port 自身优势，收敛仍为回归，维持阻塞；③terminal_pane **非 opentui 功能**，无上游对应体，**永久本地保留**。渲染耦合项（syntax/line_number/pane 渲染侧）统一卡在 ratatui→opentui 渲染栈迁移决策，已记录未排期（docs/FRAMEWORK-CONVERGENCE.md "Rendering-stack migration"）；git rev pin 规则维持；合并 0806272 |
| G-12 | oc-schema 缺 7 类事件类型化模型 | **已闭环（2026-10-24）** | pty/persistent-pty/filesystem/reference 四模块（ref 字段名/casing/可空性），9 个发射点全部类型化，golden 对表测试进常规 cargo CI + persistent-pty 与 terminal_wire_json 无漂移测试；pty 事件 info exitCode 对齐 ref casing（无内部消费方）；合并 304ae9e |
| G-13 | CLI 枚举校验偏松 | **已闭环（2026-10-24）** | 五处 clap ValueEnum（upgrade --method 七选项+rust-extra local-file/github 保留、run/auth list --format、session list --format、debug paths）；--force 裁定：保留为文档化 rust-extra（默认路径即 v2 平价的 comment-preserving 写）；合并 f0c1791 |
| G-14 | Web UI 功能一致性 E2E 验证 | 开放 | 前端 = 内嵌 ref 构建（app_archive.rs:44），不移植；一致性等价于服务端契约一致性。已对表：141 路由（含错误 _tag）、SSE 帧字节级、WS+ticket、auth/CORS/location。残留 4 类风险：① handler 长尾仅签名级验证（高频端点才做了 body 级）；② G-12 七类事件无 schema golden 对表，UI 按 shape 解析会**静默坏**；③ 时序/边界（分页 cursor、Last-Event-ID 断线重连、竞态）只能真跑；④ 资产钉定漂移（ref 前端演进需重建嵌入）。闭环 = 六场景浏览器冒烟（auth、会话/prompt 流式、文件树/mentions、PTY 面板、权限弹窗、事件重连）挂 **runtime-gates 周 cron（GitHub Actions runner，不占本机；首次稳定化本地错峰跑一轮再去 flaky 后再上 cron）**；G-12 类型化后的 golden 对表为纯 cargo 测试，进**常规 PR CI**（秒级确定性，勿等周 cron）；两项绿后方可宣称"功能一致已验证" |
| G-14 进展 | 浏览器冒烟 chunk 1 落地（2026-10-24，lane P3-E2E） | 进行中 | harness + 场景 1-2 绿：`tools/parity/tests/browser_e2e.rs`（chromiumoxide 0.9 dev-dep，headless `--no-sandbox` 新 profile；OC_BROWSER_E2E=1 运行时 opt-in；资产解析顺序 OC_BROWSER_APP_ASSETS → /tmp/opencode/ref/packages/app/dist → app-archive.json → 生成的 minimal contract page；本地实测 real-spa-dist 与 minimal-contract 两模式均绿）。S1（auth）：匿名 /api/info 401 + WWW-Authenticate Basic（harness 侧 minih——页内 fetch 带 sec-fetch-mode: cors 不发 challenge，auth.rs challenge_request 语义）+ Basic/auth_token 两路 200 + 真实 SPA shell 渲染。S2（流式）：页内 EventSource(auth_token) 收到 `session.text.delta` 全生命周期帧（v2 无 `message.part`，schema session-event.ts:405），页内 POST /api/session/:id/prompt（注意：**prompt 端点**，/message 是 GET-only 列表），最终 assistant 文本匹配 ScriptedFakeLlm。剩余 chunk：场景 3 文件树/mentions、4 PTY、5 权限弹窗、6 Last-Event-ID 重连 |
| G-14 进展 2 | chunk 2 落地（2026-10-24，lane P3-E2E）：场景 3+6 绿 | 进行中 | S3：`RustSpawn::project_cwd` 把服务端 cwd 钉在种子工作区（默认镜像 ref/packages/cli）；页内断言 `/api/fs/list`（src/+README 可见）、`/api/fs/read/src/main.rs` 内容匹配、`/api/fs/find?query=`（SPA mention/context 自动补全走 `fsFind`，mock-api.ts:206）命中种子文件。S6（契约事实，两端源码一致）：①`/api/event` 为 **live-only**——帧是 `data:`-only 无 SSE `id:` 字段（sse.rs::frame 镜像 event-feed.ts::frame），浏览器 EventSource 永远不会带 Last-Event-ID，重连不重放断线窗口（页内实测：断线期间事件不回放、重连后 live 事件正常）；客户端靠 snapshot 重同步。②真正的 cursor 恢复面是 `GET /api/experimental/session/:id/log?after=<seq>`（ref session.log，handlers/session.ts:611）：页内 fetch 流读取实测 after=seq 重放严格大于 cursor（minSeq 11 > cursor 10）、无重复 id、`log.synced` 水位前进（10→19）、断线窗口事件全覆盖；`session.text.delta` 为 live-only 非持久（持久日志只有 text.started/ended 生命周期对）。harness 事实：nextest 每测试独立进程 → 场景串行化用跨进程 flock（目标目录 .scenario-lock），否则第二个并发 Chrome 在 gate 任务限额内 pthread_create EAGAIN 挂掉。剩余 chunk 3：S4 PTY 面板、S5 权限弹窗 + 稳定化 + runtime-gates cron 挂载 |
| G-14 进展 3 | chunk 3 落地（2026-10-24，lane P3-E2E）：六场景全绿，cron 已挂 | **完成（待 cron 首跑观察）** | S4（全部腿页内）：POST /api/pty 建 sh → live 流 `pty.created`（typed info：id/status running/pid 数值/无 exitCode）；`POST /api/pty/:id/connect-token`（需 `x-opencode-ticket: 1` 头）出 ticket；ticket-exempt WS `GET /api/pty/:id/connect?ticket=` 附着——stdin=text 帧、stdout=text 帧、单条二进制 metaframe `0x00||{"cursor":N}`；`exit` → `pty.exited{exitCode:0}` + WS close 1000；PUT 更新出 `pty.updated`。S5：build agent 默认 `*/*` allow（oc-schema/agent Info::default）→ 用**会话级 ruleset**（POST /api/session 带 `permissions:[{action:"shell",resource:"*",effect:"ask"}]`，SessionRuleResolver 会话规则覆盖 agent 规则）让 shell 工具调用触发 `permission.asked`（typed：per_ id/sessionID/action/resources）；页内 `POST /api/session/:sid/permission/:rid/reply {"decision":"once"}`（204）→ `permission.replied` + execution.succeeded + transcript 带 shell 输出标记。稳定化：全六场景连跑 3×全绿（12.8s/12.0s/12.0s，每场景 1.5-9s 串行）；minimal-contract 回退模式亦 6/6（9.7s）。cron：runtime-gates.yml 新增 `browser-e2e` job（默认 ON + schedule 臂；镜像 release.yml web-assets 暂存真 SPA dist：同 PARITY_REF tarball + 定钉 bun 构建；dist 暂存失败 → battery 降级 minimal-contract、summary 记 SKIPPED-GAP；无浏览器 → 显式 skip-gap；gates-summary 增行含 `success-minimal-contract` verdict）。剩余：观察首周 cron 实跑 |

### 1.2 行为分歧（待裁定 / 登记未修）

| ID | 分歧 | 处置状态 |
|---|---|---|
| F15 | TUI 窄宽度换行 | **已解决，前提被证伪（2026-10-24）**：live ref 证实为**截断而非换行**（dialog-select.tsx:885-919 两段式截断已 1:1 移植）；DialogPair 重移植为自定义换行对话框（此前误建模为 select 模式）；c2f59d7。残留具名豁免：QR 编码器、≥96 列横向分屏（port 恒为 stacked）、<54 列 URL 硬裁剪、Enter 关闭 affordance、h<36 scroll-clip |
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
| 7 | API 配额窗口耗尽（92%）在半夜杀 lane | 配额无监控 | 定时唤醒 + 断点复活模式（WIP 落盘后 lane 死亡也可 revive） |

### 2.2 进程与终端安全

| # | 事故 | 根因 | 现行规则 |
|---|---|---|---|
| 1 | **测试盲杀信号误杀系统服务**（生产事故） | 测试按 PID 名/号发信号，撞上复用 PID 的系统进程 | safe_signal 结构性修复：pidfd 系统调用 + boot_id/进程 starttime 世代 sidecar + 外来记录 cmdline 精确校验；全仓禁裸 PID 发信号；组杀=活 leader 钉住+/proc 扫描+逐成员校验；测试只用自有子进程句柄（process_safety guard 常设） |
| 2 | **扫描腿裸启 TUI 二进制锁死用户终端**：raw mode + alt screen 缓住真终端，两个 TUI 同写一屏→持续乱码、ctrl+c 被吃、被迫重启生产 v2 | E2 进程拉起腿未套 PTY harness，直接 `cargo run` 冒屏 | TUI 二进制任何 lane 只准在 vt/PTY harness 内或 stdout/stderr 重定向 + no-tty 模式启动；tmux/终端矩阵探针只碰自建会话；OSC/转义探针只写自有 PTY |
| 3 | **测试进程泄露**：孤儿 rig/电池进程、stale ref-bun 进程（GATE3 一轮杀 12 个）、lane 死后后台矩阵孤儿化（rig 二进制消失、配对进程残留） | 后台长跑无归拢、teardown 不完整 | 长验证一律前台分块（≤45 分钟/块）；rig teardown 卫生项常设；GATE 轮清扫；清理门=mtime<30 分钟视为在用（两次误踩活动目录后立规） |
| 4 | rig 使用陈旧二进制产物，两次被差分器抓获 | lane 换代码未重建 rig 缓存 | lane 收尾强制重建检查（已知陷阱清单） |

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
| 写隔离 | 每 lane 独立 worktree；串行合并；删除清单核对 | 写隔离制度（2026-10-11） |
| 合并 | 受影响 crate 构建+测试后推送；落地证明（git log -1） | 合并门（2026-10-18） |
| oracle | 版本断言+指纹+always-refresh | `tools/diff`（opentui-rust） |
| 磁盘 | 增量抑制（CARGO_INCREMENTAL=0）/周期 clean；清理门 mtime≥30 分钟视为在用 | 本册 2.1#6 |
| 验证 | 交互面必须 PTY/field 级；组合模型必须源码级对比；金帧=回归套件 | 方法论登记（ROADMAP §4） |
