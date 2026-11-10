# OpenCode Rust 移植版 vs 参照版 v2.0.24 — 全功能逐项对比总报告

> 生成日期：2026-11-09 · 方法：17 个独立审查域，两侧逐文件逐行阅读（非抽查），一切结论以代码为准，不信任两侧任何现有文档/注释声明；每项差异均附双侧 file:line 证据，"缺失"结论均经全仓 grep 验证。
> 参照：`/opt/opencode-ref`（opencode v2.0.24, TS monorepo）· 移植：`/opt/opencode-rust`（Rust workspace, 14 crates）
> 分域详情：`docs/audit/L1-*.md` … `docs/audit/L17-*.md`

---

## 一、总体结论

移植**整体完成度很高**：核心状态机（会话生命周期、权限决策链、事件投影、工具执行协议、AI 协议转换、HTTP 路由矩阵 141/141、事件类型 104/104）达到逐行语义等价，且 Rust 侧大量以 ref file:line 为锚点并配有编号回归测试（G-xxx/RD-xx 系列）与 golden 测试体系。

主要风险不在"缺功能"，而集中在四类：
1. **两处工具注册键高危缺陷**（MCP 同名工具互相覆盖、tool_input_repair 对命名空间工具失效）；
2. **四处时序/生命周期语义偏差**（Bus 通知锁外发射、插件热重载先卸后装、MCP 后台 session 过期不重连、project 事件非原子）；
3. **有意的架构性不移植**（npm 动态 provider/plugin 包、@opencode/plugin/effect、workerd 嵌入式入口、win32 路径）——需在发布说明中升级为顶级已知限制；
4. **互操作细节**（G-226 合成错误行使 transcript 与 ref 不逐字节互操作、transcript/补偿性有界超时更激进）。

---

## 二、域级对比总表

| # | 功能域 | 参照侧规模 | 移植对应 | 对齐评级 | 主要差异（详见分域报告） |
|---|---|---|---|---|---|
| L1 | 会话生命周期（store/projector/inbox/runner/compaction/revert/fork/transfer/inbox/restart） | core/session 43 文件 ~10.5K | oc-core/session + runner + jobs + oc-server handlers | ★★★★☆ | 状态机逐行等价；G-226 额外错误行（互操作）、3 处有界超时更激进、step.failed finish 硬编码 |
| L2 | 文件类工具（read/write/edit/patch/glob/grep/file-diff/tool-output） | ~2.4K | oc-core/src/tool | ★★★★★ | 仅 patch move 不建父目录为用户可见分歧；其余为防护性偏差 |
| L3 | 非文件工具+注册表（shell/subagent/webfetch/websearch/skill/question/opencode/MCP 桥） | ~3.5K | oc-core/src/tool + mcp | ★★★★☆ | **注册表 bare-name 键控（高危）**、tool_input_repair 失效（高危）、websearch progress 时序 |
| L4 | 配置/Agent/命令/Skill/models-dev/Provider 目录 | ~5.6K | oc-core config/engine/provider_catalog + oc-schema/config | ★★★★☆ | npm 动态 provider 包加载 🔴、Model.small 待核、大小写不敏感匹配为新增宽容度 |
| L5 | 权限/凭据/OAuth/账户/Copilot | ~9K（含 copilot 4.3K） | oc-core permission/integration + oc-ai copilot | ★★★★★ | 决策链/SQL/Copilot 常量逐字对齐；anthropic-beta 头与 X-Interaction-Id 注入点待核、credential legacy 钉住边角 |
| L6 | MCP / Plugin / Codemode | ~13.4K | oc-core/mcp + oc-plugin-host | ★★★☆☆ | **@opencode/plugin/effect 🔴**、代际激活 🔴、后台过期重连 🔴、**热重载先卸后装**、事件 tap 有界丢弃、plugins/ 目录发现分歧 |
| L7 | 文件系统/VCS/Snapshot/格式化/图片 | ~3.9K | oc-core filesystem/snapshot/project + oc-server handlers | ★★★★☆ | fff 原生索引 🔴（近似替代）、snapshot diff 自研格式、restore 非 blob 静默删、find ignore/空目录语义 |
| L8 | Bus/事件/DB/KV/State/RPC/Job/Location | ~9K | oc-core bus/database/jobs/location | ★★★★☆ | **durable notify 锁外发射**、**RPC events.emit 通路 🔴**、State 通用运行时收窄、repository-cache 戳位置 |
| L9 | Shell/PTY/环境/项目/Workspace/Integration | ~7K | oc-core shell/pty/project/integration + oc-pty | ★★★★☆ | workspace 生命周期 🔴（远程 provider 前无影响）、project 事件非原子、PS legacy 解析退化、SIGKILL pid 复用窗口 |
| L10 | AI protocols（anthropic/openai/gemini/bedrock/…） | ~17.2K | oc-ai/src/protocols | ★★★★★ | 主要协议逐行等价；Anthropic null 容忍度、断点丢弃无日志、overlay 显式 null 被 retain 删除 |
| L11 | AI 核心（llm/route/cache/媒体/aisdk 管线） | ~9K | oc-ai + oc-core engine/provider_catalog | ★★★★☆ | **npm aisdk 动态桥 ⚠️/🔴**、SSE 看门狗缺失、promise.ts/experimental 🔴、tool-runtime 并入 core |
| L12 | HTTP Server API | ~4K（+protocol groups） | oc-server | ★★★★★ | 路由 141/141 全对齐、认证/SSE/错误包络逐字；workerd/fetch 嵌入式入口 🔴、plugin.install 新增 |
| L13 | CLI（commands/acp/services/run） | ~10.5K | oc-cli | ★★★★☆ | 全 flag/ACP 13 方法/exit 码等价；非交互 TTY 权限提示（默认行为差异）、v1 桥 🔴、OTEL 缺 |
| L14 | TUI 应用层（app/routes/context/config/keymap） | ~14K | oc-tui app/store/components | ★★★★☆ | 命令表/键位表逐条对齐；terminalHandoff 🔴、STORY/DRIVE 🔴、session env 重发语义、更新通知通道改构 |
| L15 | TUI 组件层（消息渲染/composer/dialog/footer/diff） | ~18K | oc-tui components + oc-tui-core | ★★★★☆ | 23 dialog/全组件有对应+golden 测试；动画 100ms 粒度、filetype 扩展名表窄、高亮缓存全清 |
| L16 | TUI mini/feature-plugins/主题/语法 | ~28K | oc-tui mini + oc-theme + oc-syntax | ★★★★☆ | mini 运行时逐行锚定；**插件 markdown 渲染器注册 🔴**、语法默认 feature 裁剪（csharp/kotlin/swift）、plugins/ 目录发现、node 运行时缺 |
| L17 | Schema/Protocol/Client/Util/v1 兼容 | ~30.8K | oc-schema/oc-client/oc-util/oc-http | ★★★★☆ | 事件 104/104、方法 141/141 全集对齐；v1 schema 契约面 🟡、**NODE_EXTRA_CA_CERTS 不识别**、solid 看门狗常量待核 |

图例：★★★★★ 逐行等价（仅边角差异）· ★★★★☆ 等价+若干需关注偏差 · ★★★☆☆ 存在结构性缺口

---

## 三、建议优先修复的行为差异（跨域汇总，按严重度）

| # | 严重度 | 问题 | 证据 | 影响 |
|---|---|---|---|---|
| 1 | 🔴 高 | 工具注册表以 bare name 为键：两个 MCP server 暴露同名工具时后者覆盖前者注册与 executor，`{srv1}_tool` 会路由到 srv2 | L3：location.rs:557-560 vs ref tool.ts:183-184 | 错误工具执行 |
| 2 | 🔴 高 | tool_input_repair 按 bare-name 查表，PipelineExecutor 传 effectiveName → `opencode_*` 与 MCP 工具全部不做入参修复 | L3：tool_input_repair.rs:48-51 vs ref tool-input-repair.ts:29 | 模型入参修复静默失效 |
| 3 | 🔴 高 | Bus durable 发布 notify 在聚合锁外：订阅者/SSE 可见 seq 乱序（A 后 B 先） | L8：bus.rs:702-708 vs ref bus.ts:469-477 | 事件序可见性破坏（模块头注释与代码矛盾） |
| 4 | 🔴 高 | 插件热重载先 clear+unload 旧代再 load 新代：失败即丢全部注册（ref 新代成功才替换） | L6：supervisor.rs:1938-1966 vs ref supervisor.ts:77-82 | 热重载失败后插件静默消失 |
| 5 | 🟠 中高 | G-226：未 surfaced 失败额外合成 step.started/step.failed 错误行 → transcript 与 ref 不可逐字节互操作（导出/导入/客户端渲染多一行） | L1：execution.rs:930-1038 | 已注册偏差+dedup，但跨版本数据互操作受影响 |
| 6 | 🟠 中高 | snapshot restore 对非 blob 树条目按 absent 递归删除（ref 报 Invalid tree entry） | L7：snapshot.rs:796-840 vs ref git.ts:587-593 | 树损坏时静默删文件，风险方向危险 |
| 7 | 🟠 中高 | patch `*** Move to:` 到新目录不建父目录（ref mkdir -p） | L2：patch.rs:431-433 vs ref local.ts:51-55 | 可复现的用户可见失败 |
| 8 | 🟠 中 | MCP 后台 list-changed 观察到 session 过期不触发重连（仅前台调用路径恢复） | L6：registry.rs:905-957 vs ref index.ts:415 | 长空闲远程 server 状态陈旧 |
| 9 | 🟠 中 | project `worktree.resolved` 行插入与事件非原子：崩溃窗口后事件永不补发 | L9：project.rs:1354-1388 vs ref project.ts:190-199 | 会话项目归属投影落后 |
| 10 | 🟠 中 | Claude-on-Copilot anthropic lane 未注入 `anthropic-beta: interleaved-thinking` 头；X-Interaction-Id 亲和注入点未证实 | L5：oc-ai copilot.rs:101-111 vs ref github-copilot.ts:495,276 | 交错思考可能被拒；Copilot 端会话亲和退化 |
| 11 | 🟠 中 | NODE_EXTRA_CA_CERTS 不被 oc-http 识别（ref 企业 CA env 注入路径失效） | L17：oc-http/lib.rs:33-61 vs ref websocket-constructor.ts:66 | 企业自签 CA 部署不等价 |
| 12 | 🟠 中 | 非交互 run 在真 TTY 弹权限交互（ref 恒自动拒绝），expect/tmux 脚本可能挂起 | L13：noninteractive.rs:267-295 vs ref noninteractive.ts:158-186 | 默认路径行为差异（CI/OPENCODE_NO_TTY_PROMPT 可关） |
| 13 | 🟠 中 | 语法高亮：csharp/kotlin/swift 为非默认 cargo feature；markdown 围栏注入降级纯文本 | L16：oc-syntax Cargo.toml:100-104 | 默认二进制高亮能力低于 ref |
| 14 | 🟡 低 | 其余：Anthropic options null 容忍度（L10）、openai_chat overlay 显式 null 被 retain 误删（L10:2759）、SSE header/chunk 看门狗缺（L11）、Bus 锁外通知（#3 同源）、websearch 同意流程 60s→120s（L3）、动画 100ms 粒度与高亮缓存全清（L15）、credential legacy 钉住（L5）、shell cwd 服务层默认（L9）、CORS Vary 细节（L12）等 | 见各分域报告 Top5 | 多为边角/已记录偏差 |

---

## 四、明确缺失功能清单（🔴，均经全仓 grep 验证）

**生态/动态加载类（有意不移植，建议写入发布说明）**
- npm 动态 provider 包加载 `Provider.loadPackage`/`npm.add`（L4/L11）
- npm AI-SDK 插件动态桥（aisdk.ts SDK hook 管线，仅静态映射表）（L11）
- `@opencode/plugin/effect` API 面 21 模块（import 即报错，仅支持 promise 形态）（L6）
- 启动期 npm 插件自动安装（改为离线解析 + CLI `plugin add`）（L6）
- 插件 markdown 代码块渲染器注册链（L16）

**平台/部署类**
- workerd 运行时档案与 ServerFetch 嵌入式 fetch 入口（L12）
- win32：cross-spawn shim、persistent-pty 前台进程探测、terminal.copy manual 默认（L9/L14）
- OTEL Observability CLI 入口层（L13；server 侧 otel feature 存在）

**生命周期/运行时类**
- workspace 共享 provision/连接池/20min 空闲挂起 janitor/destroy 竞态结算（当前仅 local provider 无行为差异）（L9）
- MCP onSessionExpired 后台重连（L6）
- 插件代际激活/fallback（(id,revision) 机制）（L6）
- 插件 RPC events.emit 宿主通路（`rpc.{id}.{name}` 生产者；客户端订阅端已实现）（L8）
- State 通用运行时 batch/shutdown/inherit/reconcile/notify（仅 agent 域实现）（L8）
- CLI→TUI renderer terminalHandoff；OPENCODE_DRIVE/STORY/ROUTE dev 入口（L14）

**次要/测试基建类**
- fff 原生文件索引层（契约级近似替代）（L7）
- `run/v1.ts` 桥、`protocol/simulation.ts`、Storybook 插件、`experimental/`（evaluation/system-one）、`promise.ts` GenerationHandle（L13/L17/L11/L16）
- v1 schema 公开类型（v1/permission、v1/filesystem、legacy-event；DB 迁移内部形状已有）（L17）
- `util/artifact.ts`、`util/binary.ts`（L17）
- ref 中本就不存在/无调用方的项未计入（如 beforeAggregateRead、KV TTL）

---

## 五、移植版新增（ref 无对应，🔵）

升级命令 local-file/github 方法与 completion；`POST /api/plugin/install`；静态 assets/app_archive 服务；safekill（PID 世代+pidfd）；panic→500 兜底；`x-opencode-build` 头；oc-pty 独立 daemon + handoff 接替梯 + wire/checkpoint 格式；SSE shutdown 主动收尾；压缩排除 SSE；托管 bun（bunman，优雅降级+kill switch）；oc-http 双栈 TLS 统一（G-26）；v2(TS)→rust 数据目录单向迁移；G-226 失败补记行；三处有界超时（TOOL_JOIN/UNWIND/REMOVE_IDLE）；RUNTIME-SWEEP-RACE 启动守卫；usage fan-out 串行链；PreparedRing；G-256 UTF-16 估算等修正族；插件监督策略（3-strike/退避/死锁协议）；attention.bell；boot 300ms 输入丢弃窗；OSC52 100KB 上限；golden 测试体系；ripgrep OPENCODE_RG_PATH/受控下载；语法 native grammar 离线化；免费 utility 标题超时等 Copilot 加固；zen 设备流变体；GHE 显式 origin。

---

## 六、分域报告索引

| 域 | 文件 |
|---|---|
| L1 会话生命周期 | docs/audit/L1-session.md |
| L2 文件类工具 | docs/audit/L2-tools-files.md |
| L3 非文件工具/注册表 | docs/audit/L3-tools-other.md |
| L4 配置/模型目录 | docs/audit/L4-config-agents-models.md |
| L5 权限/认证/Copilot | docs/audit/L5-permission-auth-copilot.md |
| L6 MCP/Plugin/Codemode | docs/audit/L6-mcp-plugin-codemode.md |
| L7 文件系统/VCS/Snapshot | docs/audit/L7-fs-vcs-snapshot.md |
| L8 Bus/事件/DB/Job/Location | docs/audit/L8-bus-db-infra.md |
| L9 Shell/PTY/环境/项目 | docs/audit/L9-shell-pty-env-project.md |
| L10 AI protocols | docs/audit/L10-ai-protocols.md |
| L11 AI 核心 | docs/audit/L11-ai-core.md |
| L12 HTTP Server | docs/audit/L12-server-http.md |
| L13 CLI | docs/audit/L13-cli.md |
| L14 TUI 应用层 | docs/audit/L14-tui-app.md |
| L15 TUI 组件层 | docs/audit/L15-tui-components.md |
| L16 TUI mini/插件/主题/语法 | docs/audit/L16-tui-mini-plugins-theme.md |
| L17 Schema/Client/Util | docs/audit/L17-schema-client-util.md |

> 遗留核对项（各域标注"待核"）：`Model.small` 家族偏好（L4）、anthropic-beta/X-Interaction-Id 注入点（L5）、serve env 旋钮透传（L13→oc-server boot）、solid 重连看门狗常量（L17→oc-tui）、openai_chat overlay 段逐行（L10 自述方法限制）、oc-tui render/ 框架层（opentui 0.5.14 参照源不在本仓，仅在应用可见行为层对照）。
