# opencode Rust 移植版 全功能逐项对比审查报告

- **参考实现(TS)**:`/opt/opencode-ref`(锚定 **v2.0.24**,commit e7a34f09)
- **移植实现(Rust)**:`/opt/opencode-rust`(15 crates,约 55 万行)
- **审查方式**:15 个独立审查 lane 并行进行,以**代码为准**,不信任任何文档/README/ROADMAP/GAPS/注释中的完成度声明。ref 核心链路(core/tui/cli/ai/client/server/schema/theme/util/plugin ≈ 24.7 万行)全覆盖;周边产品包(desktop/web/console/enterprise/storybook 等)不在移植范围。
- **分领域明细报告**:`/tmp/opencode/port-review/L01..L15*.md`(共 15 份,含逐功能行号级对照表)

## 一、总体结论

| 指标 | 数值 |
|---|---|
| 对比功能点总数 | ~1500 项(去重后表格行) |
| ✅ 一致(语义/文案/边界对齐) | **1182** |
| 🟡 部分实现或行为差异 | **196** |
| ❌ 缺失 | **38** |
| ➕ rust 新增能力(ref 无) | **77** |
| 一致率(✅/总数) | **~83%**(若计入"有偏差但主链路在"则 >96%) |

整体判断:**这是一次质量极高的移植**。核心链路(会话状态机、事件总线、数据库 schema 与 48 条迁移、HTTP 路由 141/141、client 端点 136/136、内置工具全部 15 个、MCP 全传输+OAuth、权限引擎、Provider 协议层)逐行对照一致,大量模型可见文案(工具描述、错误消息、事件信封)做到逐字节保真,并以 parity/golden/对拍测试钉住。缺失项集中在:实验特性、开发工具、平台特定(V8/workerd)与少数动态化机制。

## 二、分领域对比总表

| 领域 | ref 位置 | rust 位置 | ✅/🟡/❌/➕ | 领域结论 |
|---|---|---|---|---|
| L1 Session/消息生命周期 | core/src/session/,generate.ts,codemode | oc-core/session/,engine/,runner/ | 76/16/0/6 | 状态机/inbox/投影/compaction/revert/subagent 逐事件一致;并发模型重构(单写者循环)但不变量保持并有超时兜底 |
| L2 内置工具集 | core/src/tool/(15 叶子+5 基础件) | oc-core/tool/ | 97/12/2/0 | 15 工具全数移植、文案逐字节;缺 JSON-Schema 归一化与统一输出编码校验 |
| L3 AI/Provider 适配 | packages/ai + core aisdk/provider/variant | oc-ai/(73k 行)+provider_catalog | 71/5/3/4 | 核心协议(anthropic/openai/gemini/bedrock…)逐行对齐+19 个 parity 套件;aisdk V3 shim、SystemOne/typesafe-ai 未移植 |
| L4 TUI 编辑器/输入/渲染 | tui editor/prompt/keybind/theme/util | oc-tui 基础层+oc-tui-core+oc-syntax | 78/13/0/4 | 编辑引擎为真移植(簇/词界/undo 语义);偏差为有界近似(PNG 解码、emoji 宽度表) |
| L5 TUI 组件/路由/状态 | tui components/routes/context/mini/plugins | oc-tui components/mini/store | 129/13/7/9 | 280 文件全量映射,dialog 全家族在;缺 PanelHost 分栏、storybook、3 个 toggle 命令 |
| L6 配置/agents/commands/skills | core/src/config/+agent/command/skill/instructions | oc-core/config/,cli_config/,engine/ | 77/12/4/2 | 配置字段/迁移逐项一致;缺命令 shell 插值、MCP prompts→斜杠命令、文件级 watch 重扫 |
| L7 总线/数据库/状态 | bus.ts,database/,state.ts,kv/,job.ts | oc-core/bus*,database/,jobs.rs | 102/8/2/6 | 52+36 事件类型、20 表、48 迁移逐条一致;仅 state.ts 作用域机制缺失 |
| L8 shell/PTY/persistent-pty | core/src/shell/,pty*,persistent-pty/ | oc-core/shell*,oc-pty | 68/13/0/6 | 扫描状态机 3800 行逐语句移植;daemon 服务端为 rust 新增完整实现 |
| L9 MCP/插件/集成 | core/src/mcp/,plugin/,integration/ + packages/plugin | oc-core/mcp/,plugin_apply,oc-plugin-host | 104/16/0/2 | MCP 全传输+OAuth 全链路;插件架构重构为子进程隔离宿,API 面对齐甚至超集 |
| L10 权限/凭证/OAuth/Copilot | permission*,credential*,github-copilot/,oauth/ | oc-core/permission*,oc-server/auth*,integration/ | 60/4/0/0 | 忠实度极高的一块:每个 OAuth provider 逐常量核对一致 |
| L11 文件系统/项目/VCS | filesystem*,git.ts,project*,worktree/,vcs/,snapshot,ripgrep | oc-core/filesystem*,snapshot*,oc-server handlers | 80/27/5/7 | git/vcs 每个操作一行对照;缺 fff 索引后端、workspace 空闲挂起 |
| L12 CLI 命令与启动 | packages/cli(109 文件) | oc-cli(47 文件) | 57/19/4/8 | 全命令对照;缺 heap/cpu-profile(V8 依赖)、OTEL、EADDRINUSE 竞争重试 |
| L13 HTTP Server/API | packages/server + protocol/groups(141 操作) | oc-server(44k 行,142 操作) | 78/6/1/4 | 路由/中间件/认证/SSE 全量对齐;rust 进程管理(safekill/handoff)超越 ref |
| L14 client/schema/theme/util | packages/client, schema, theme, util | oc-client,oc-schema,oc-theme,oc-util | 46/17/5/13 | 端点/事件/字段 100% 对齐;缺 effect/solid 变体与 function/ 云端包(by design) |
| L15 核心杂项 | app/instance/environment/wellknown/reference/rpc/v1/websearch/image/formatter | oc-core env/engine/reference/formatter/image 等 | 59/15/5/6 | 格式化器 26 个逐条一致;缺 wellknown.remove、websearch 亲和清理 |

## 三、❌ 缺失清单(38 项,按影响排序)

### A. 用户可感知的功能缺口(建议优先补)
| # | 缺失项 | ref 位置 | 说明 |
|---|---|---|---|
| 1 | 命令模板 `` !`cmd` `` shell 插值 | config/plugin/command.ts:214-239 | 含 shell 插值的自定义命令行为错误;rust 注释称"server 端处理"但全仓无实现 |
| 2 | MCP prompts → `server:prompt` 斜杠命令 | plugin/command.ts:56-84 | client 有 prompts API 但未接入命令表 |
| 3 | 插件 PanelHost / panel 双形态(侧栏分栏) | context/panel.tsx + component/panel-host.tsx | 宽屏下 diff/stats 侧栏与 `ctx.ui.panel.*` API 整链缺失 |
| 4 | `app.toggle.animations/diffwrap/file_context` 运行时切换命令 | app.tsx:1148-1191 | config 键在,命令面板/键绑定无 |
| 5 | 持久 PTY `title_changed`/`foreground_process_changed` 事件 | daemon.ts:97-117 | wire 定义了但机型永不产生;前端无法显示前台命令 |
| 6 | `wellknown.remove`(取消已添加 wellknown 集成) | wellknown.ts:167-183 | 只能 add 不能移除 |
| 7 | serve 端口被占时的 EADDRINUSE 15s 竞争重试 | cli server-process.ts:148-171 | rust 只有启动前预检,冲突直接失败 |
| 8 | websearch 429 时 `Retry-After` 解析 | websearch.ts:227-234 | rust 固定 60s 冷却 |
| 9 | Workspace 空闲挂起 janitor(20min) | workspace.ts:154-184 | trait 已留,未实现(本地 driver 下无行为差异) |
| 10 | websearch 会话亲和表随 session 删除/移动清理 | websearch.ts:52-58 | 内存缓慢泄漏(轻微) |
| 11 | fff(aiMode)文件搜索后端 | filesystem/search.ts:116-196 | 以原生索引+greedy fuzzy 近似替代,排序非精确复刻 |
| 12 | JSON-Schema 归一化(flattenAllOf/内联 $ref)+统一输出编码校验 | tool/runtime.ts:109-257 | 外部(MCP/插件)工具 schema 的 provider 兼容性弱一档 |
| 13 | tools Editor 动态面:update/remove/namespace 描述表 | tool.ts:166-205 | 折叠为一次性注册 |
| 14 | state.ts group/batch/shutdown/notify 作用域机制 | state.ts:50-132 | 以快照缝替代(行为面影响小) |
| 15 | skills/agents/commands 文件级 watch 重扫 | config/plugin/skill.ts 等 | 启动期一次发现;引擎级 config reload 在但技能表不重建 |
| 16 | 插件目录文件监听自动热重载(watcher/bus/24h 四路触发) | plugin/supervisor.ts:196-237 | 热重载能力在,但需外部调用 reload_now |
| 17 | 插件 vcs branch/status/diff、worktree list/create/remove 读操作桥 | plugin/host.ts:468-526 | transform 域在,查询操作缺 |
| 18 | wellknown 凭据切换/10min 刷新触发 config reload | config.ts:290-326 | 仅保留 seam |
| 19 | bus beforeAggregateRead 钩子 / finalizer 关停清理 | bus.ts:175,272-282 | ref 侧亦无内部使用方(影响≈0) |
| 20 | kv_scan 默认 limit 100 | kv.ts:61 | rust 由调用方传(有 clamp) |
| 21 | instance/service.ts Session 能力选择 seam | instance/service.ts:1-19 | 小接缝 |
| 22 | job errorText 的 "Caused by" 错误链渲染 | job.ts:210-244 | 诊断信息变少 |
| 23 | VCS provider 插件注册表 | vcs.ts:60-88 | 内建 git/hg,无插件扩展点 |
| 24 | LocationWatcherPolicy reload/transform 面 | location-watcher-policy.ts | 未暴露 |
| 25 | noop snapshot layer | snapshot.ts:212-222 | 由非 git 门控等价覆盖 |

### B. 平台/运行时原因(基本不可移植或低价值)
| # | 缺失项 | 原因 |
|---|---|---|
| 26 | heap 快照(SIGUSR1)/CPU profile(SIGPROF) | V8 inspector 依赖 |
| 27 | CLI 侧 OTEL 接线 | (server 侧 OTel 已有,CLI 未接) |
| 28 | workerd 运行时 profile | Cloudflare 部署形态 |
| 29 | effect/solid 变体客户端、runtime-import、global.ts/effect 层 | Effect 框架机制本身 |
| 30 | function/(Cloudflare 同步服务:JWT+GitHub App+R2) | 云端 infra,大概率有意不做 |
| 31 | experimental/SystemOne+evaluation、typesafe-ai provider、opencode-zen 的 SystemOne 评测 | 实验特性 |
| 32 | OPENCODE_ROUTE/OPENCODE_STORY + Storybook 插件族(7 文件) | 开发工具 |
| 33 | win32 专属:gitbash 发现、ConPTY Ctrl+C 修正、LC_* env、retained-image | 非当前目标平台 |
| 34 | Windows cross-spawn / 作业对象树杀 | 同上 |
| 35 | markdown.d.ts(TS 类型声明) | 无需 |
| 36-38 | (L5 计 7 项 ❌ 中已列 panel/storybook/toggle;其余为 storybook 相关文件组) | |

## 四、🟡 行为差异 Top(节选高影响)

1. **凭证解码失败静默降级为空 key**(ref 直接 die)— 可能掩盖数据损坏(L10)。
2. **失败回合合成 assistant error 行**:pre-step 失败 rust 额外落 `step.started/failed` 行,消息树比 ref 多一类行(L1)。
3. **identity/optimize/plan 插件内联化**:plan 提醒不落 synthetic 持久行(L1)。
4. **Copilot device 流 form vs JSON 体**;OAuth 回调页简化(无品牌/错误页)(L10)。
5. **persistent-pty exit 无条件发 removed**(ref 仅对已 attach 可见终端)(L8)。
6. **opencode-pty 二进制免 sha256 钉扎**(ref 有属主+哈希校验;rust 注明有意取舍)(L8)。
7. **`serve --service` 多打印一行密码;auth logout/switch 连接失败打印空错误**(L12)。
8. **fs.find 原生索引+贪心模糊替代 rg+fuzzysort;fs.write 缺 path→500;session.move 400 文案缺目录后缀**(L13)。
9. **快照对象库不共享(alternates 缺失)→ 大仓每次 capture 全量写 blob**(L11)。
10. **MCP 会话过期/401 判定靠 reqwest 错误文本匹配**(脆弱点)(L9)。
11. **mini stream-v2/tool 细粒度展示变体缩水**(主链路在)(L5)。
12. **websearch progress 时机、generate 面禁用钩子未注册**(L2)。
13. **read 的 AGENTS.md 发现走宿主 fs 而非 Files seam**(workspace 场景可能漂移)(L2)。
14. **skill 扫描只一层子目录、`*.md` 大小写不敏感、错型 name 不跳过**(L6)。
15. **credential 解码、kv limit、job Caused-by、tavily UA 头、copilot 20 个适配文件未逐文件核对**等小项(各 lane)。

## 五、➕ rust 超越 ref 的新增能力(77 项,代表性)

- **可靠性**:进程组安全 kill(/proc 扫描+leader 校验)、safekill(pidfd+boot_id+start_time 防 PID 回收误杀)、bootstrap 跨进程双检锁、WAL busy 重试、panic→500 容器、信号安全 crash 恢复、stderr 守卫。
- **运维**:daemon ensure/stop/竞争者仲裁、PTY handoff 跨重启、5xx 永久日志 sink+原地裁剪、OTel、boot 时间戳护栏、`x-opencode-build` 兼容探测、v2→rust 数据迁移。
- **架构**:插件子进程隔离宿(bun+NDJSON、崩溃监督 3-strike)、Code Mode guest 求值器、VT100 真实终端仿真、分块 replay 协议、检查点 v1 二进制格式、自研 reactive 微图、代理传输层。
- **测试体系**(ref 所无):19 个 AI parity 套件、ANSI golden/帧预算、事件 golden(g12)、HTTP contract 对拍、与 ref 真实 SQLite 对拍(bun.rs)、PTY 键族回归、openapi sha 钉死。
- **CLI**:`completion` 命令、`upgrade --method local-file|github`+升级锁、`--force` 逃生门、`plugin.install` 端点、rg 捆绑+三级发现。

## 六、审查方法与覆盖度诚实声明

- 各 lane 均先从 ref 代码提炼函数级清单再到 rust 仓全量定位对照;**未采信任何现有文档**。
- 完全逐行精读:L1(ref session 全 39 文件)、L2(19+17)、L7(14+48 迁移+13)、L8(19+15)、L10(14+17)、L11(34+22)、L13(ref 50/rust 27 全文)、L14(脚本全量 diff:端点/类型名/符号)。
- 全量枚举+关键段逐行的 lane:L3(~110 ref 文件结构化+8 逐行)、L4(24+22 逐行,store.rs 等大文件 grep 定位)、L5(280/198 全符号映射+8 逐行)、L6(31+14)、L9(26+全文核心)、L12(70 精读+35 略读)、L15(31+28)。
- 已知未覆盖死角(后续可补查):oc-plugin-host/host.js(3319 行,按 API 面 grep)、oc-tui/store.rs(16k 行,composer/prompt 域定位)、ref github-copilot chat/responses 17 个适配文件(仅核 rust 模块存在+关键函数)、双方 acp 内部 18-20 文件(模块级)、v1_migration 两侧大文件(函数级)、生成文件字段级(types.gen 仅名级)。

## 七、建议行动优先级

1. **P0(行为错误)**:命令 `` !`cmd` `` 插值(#1)、MCP prompts 命令(#2)、auth 空错误消息、fs.write 400。
2. **P1(功能缺口)**:PanelHost/panel、PTY title/fg 事件链、EADDRINUSE 重试、wellknown.remove、websearch Retry-After。
3. **P2(一致性打磨)**:凭证解码 die 语义、pty exit removed 条件化、二进制哈希钉扎、fs.find 排序精确化、快照 alternates。
4. **P3(有意取舍,记录即可)**:V8/workerd/effect 系、experimental、storybook、win32 专属。
