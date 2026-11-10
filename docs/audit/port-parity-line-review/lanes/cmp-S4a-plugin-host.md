# S4a 结论: Rust 宿主面是 ref 插件 API 的「有界投影 + 子进程化重实现」——20 域中 14 域 ops 全量对齐、6 域部分缺失（vcs 读面/worktree 调用面/integration transform 全 editor/rpc client/effect API/tui 深层面），且存在两个生产级断线：`permission.evaluate` guest hook 注册后永不被派发、`experimental.ws.*` 三 seam 构建后无任何消费者（A2 从宿主侧确认）。

## API 面全量对照表（ref effect/promise 20 域 × rust ops）

| ref 域 | ref 面（host.ts） | rust guest（host.js makeCtx:861-1901） | rust 宿主 op（supervisor.rs answer_guest:456-612） | 判定 |
|---|---|---|---|---|
| agent | get/list（跨 location）/transform/reload（host.ts:120-156） | transform/reload/list/get（list/get 读 guest 本地表） | `agent.transform` | ✅ 读写面齐；跨-location get/list 缩为单 guest 本地表（D-13） |
| aisdk | hook(sdk/language, providerID 过滤) | `ctx.aisdk.hook` | `hook.register`；引擎派发 model_request.rs:796-830 | ✅ |
| command | list/transform/reload（CommandEditor 仅 add） | add/transform/reload/list | `command.add` | ✅（keybinds/palette 为 Rust 扩展） |
| event | subscribe()（过滤 isServer‖rpc.*） | subscribe 双形态（callback/async-iterable） | `event.subscribe` + `dispatch` 泵 | ✅ 过滤移至 embedder（plugins.rs:2083-2095 `is_public_event`） |
| experimental.terminal | read | read | `experimental.terminal.read` | ✅ |
| generate | text | text | `generate.text`（经 SessionPort） | ✅ |
| integration | list/get/connect.key/oauth×4/command×3/connection×3/reload/transform 全 editor（:274-373,631-672） | 读面全 + `method.update` | `integration.*` 13 方法透传 + `integration.method.update` | ⚠️ `transform` 的完整 editor 与 `reload` 无 guest 面 |
| mcp | list（跨 location）/transform/reload | transform/reload/list | `mcp.transform` | ✅ |
| model | list/default/transform/reload | list/default/reload/transform（G-147 override 表） | `model.list/default/reload` + `model.transform` | ✅ |
| permission | hook(evaluate)/list/get/reply（:404-427） | hook/list/get/reply | `permission.list/get/reply` + hook.register | ⚠️ guest/注册面齐，**引擎无生产派发点（R-1）** |
| plugin | list（:428-430） | list（id/name/version/active 投影） | `plugin.list` | ⚠️ 有界投影（ref 含 source/state/features） |
| provider | list/get/transform/reload | list/get/reload/transform（含 models 子 editor） | `provider.list/get/reload` + `provider.transform` | ✅ |
| reference | list/transform/reload | list/reload/transform | `reference.list/reload` + `reference.transform` | ✅ |
| rpc | register/events.emit/client 调用 | register + events.emit | `rpc.register`/`rpc.event`/`rpc.call`（G-296 宿主侧 schema 校验） | ⚠️ **client 面（插件作为调用方）无 guest 对应**（D-7） |
| session | hook 12 点 + 15 方法 Pick | 17 方法（15 + messages/list 扩展）+ hook | `session.*`（SESSION_METHODS 白名单，经 SessionPort） | ✅ |
| shell | hook(create.before) | hook | hook.register；派发 shell.rs:628 | ✅ |
| skill | list/transform/reload | transform/reload/list | `skill.transform` | ✅ |
| storage | get/set/remove/scan（`plugin:<utf16hex>:` 命名空间） | get/set/remove/scan | `storage.*`（命名空间/limit clamp 1..1000 逐位对齐） | ✅ |
| tool | transform/reload/list/hook(before 可 fail) | transform/reload/list/hook | `tool.transform`/`tool.execute`（含注册校验） | ✅ |
| vcs | get/base/branch.list/status/diff/transform/reload | get/base/reload/transform | `vcs.get/base/reload` + `vcs.transform` | ⚠️ **branch.list/status/diff 三读面无 guest 对应** |
| websearch | providers/query/transform/reload | providers/query/reload/transform（含 default.get/set） | `websearch.providers/query/default.*` + `websearch.transform` | ✅ |
| worktree | list/create/refresh/remove/reload/transform | transform（create/remove/list 闭包经 hook 回派） | `worktree.transform` + GuestWorktreeStrategy | ⚠️ **list/create/refresh/remove 调用面无 guest 对应** |

**TUI 承载**：已对齐 slot claim（五 placement 互斥单 key 校验 = api.tsx:298-312）、router.register/navigate、keymap.layer、ui.toast/alert/confirm/prompt（deferred reply）、dialog.register/open、sidebar_content、theme/route/layout 推送。**确认并补全 T4 缺失清单**（ref 有、guest 无）：`markdown.registerCodeBlockRenderer`、`ui.tabs.*`、`ui.model.current/variant.*`、`ui.panel.*`、`ui.format.path`、`ui.dialog.show/set/clear/select`、`keymap.dispatch/shortcuts/commands/pending/active/mode.*`、`data.*` 全部集合、`storage.store/memory`、`attention.notify`（stub 恒 `{ok:false,"attention_disabled"}`）、`renderer`（仅 requestRender）、完整 ResolvedTheme（tui.theme 仅 5 色 sidebar 子集）。

## 对比表（进程监督 / 加载 / 装载更新）

| 面 | ref | rust | 判定 |
|---|---|---|---|
| 入口解析 | Host.resolve：Target→{server,tui,rpc}，node 解析器吞 6 类错码 | install.rs resolve_entry + probe_feature_entry（tui/rpc 无 main 回退，G-280③）+ resolve_subpath_entry（**有 main 回退，与 G-280③ 矛盾** → R-4） | ⚠️ |
| 三源加载 | bun（require.cache 逐文件失效 + Transpiler.scan）/node（registerHooks + `?__opencode_reload`）/package | guest：require.cache 目录前缀失效 + `?t=<gen>` cache-bust + Bun.Transpiler.scanImports；宿主 digest 指纹 | ✅ 语义对齐 |
| 热重载 | watcher→100ms debounce→staged activate；失败保留旧代 | G-293 双侧快照 staged swap + revision 指纹 + failed 不重试 + vanished=显式移除；**触发源在 embedder** | ✅ 语义 / ⚠️ 触发面（D-10,D-14） |
| 崩溃策略 | 无（进程内；模块错误=Failed 状态） | 死锁协议三规则 + 100ms→4s 退避 + 3 strike 禁用 + 8 次放弃 + codemode kill 豁免 + PDEATHSIG | Rust 新增面（子进程化必然） |
| 超时 | 无硬限 | 注册 2s / 查询 5s / mutating 120s | D-5 |
| install | npm.ts：`.staging-` 目录 + 完成后落位 + GC(keep2/7d/staging 1h) | install.rs 直接写最终目录（**staging 仅存在于 GC 代码，从未创建**）+ bun --frozen-lockfile/npm ci 优先 + 无 bun 时托管下载 | ⚠️ R-3 |
| update | 24h 节流 check + per-target 锁 + 失败保留旧值 | UpdateCache 逐项对齐（24h、失败保旧值、pinned 不可变、GC） | ✅ |
| source-directory | plugin/plugins 目录 .ts/.js/dir/symlink 排序发现 | discover_directory_ops 逐条对齐 | ✅ |
| identity.ts | session.hook context/compaction/generate 注入 "# Your Model" | oc-core hooks.rs 站点存在 | ✅（承接面确认） |

## 行为差异清单 D-n

- **D-1 effect API 整体拒载**（detectEffectImports 抛清晰错误）——ref 双 API→rust 仅 promise；v1 `server()` 导出容忍并忽略（与 ref v2 一致）。文档化生态破坏性变更（G-148）。
- **D-2 进程模型**：ref 每 Location 一套 PluginHost；rust 全进程一个 bun guest，`location.update` 通知近似（PLUG-D07）。
- **D-3 transform 语义**：ref editor 回调按序重放、逐插件生效；rust「整表物化」跨线（merge_* 保留 provenance）——净效果等价。
- **D-4 事件通道**：ref 插件直连 bus（必见）；rust 经 BroadcastTap（4096 有界 + envelope-id 去重）→ dispatch 泵（5s 超时、超时**丢事件并计数**）。契约从「必见」弱化为 best-effort。
- **D-5 超时上限**：mutating hook 120s / 注册 2s；ref 无硬限。重型 setup>2s 即 Failed（revision 不再重试）。
- **D-7 rpc client 面缺失**：ref `RpcDomain` 让插件作为客户端调用任意 rpc definition；rust `ctx.rpc` 仅 register/emit。
- **D-8 plugin.list 投影**：四字段 vs ref 完整 Plugin.Info。
- **D-9 命令调用形状**：guest 构造 `{sessionID, prompt:{text}, delivery:"steer"}` vs ref 完整 PromptInput.Prompt + Delivery；近似。
- **D-10 重载触发面**：crate 内无 watcher/24h check/sdk.plugin.updated 订阅；由 embedder 驱动。
- **D-14 SdkPlugins/InstancePlugins 通道**（sdk.ts register→bus `sdk.plugin.updated`→所有 Location 重载；instance.ts birth-bound）无 Rust 对应。
- **D-16 Rust 扩展（ref 无）**：`theme.register`、command keybinds/palette、`ui.grouping.addRule`/`ui.home.addQuickAction`、`plugin.fault` 通知、`websearch.default.get/set`、event.subscribe callback 简写。
- **D-17 UI 请求为帧间队列**而非 ref 的同步进程内调用；deferred jsonrpc reply（语义等价，时序不同）。
- **D-18 event.subscribe 错误吞噬**：per-plugin 错误聚合进 reply 而 ref 会 fail 订阅——无害化差异。

## 正确性风险 R-n

- **R-1（高）`permission.evaluate` 生产断线**：guest `ctx.permission.hook("evaluate")` → `hook.register` → plugin_apply.rs:1103 注册进 HookRegistry，但 `dispatch_permission_evaluate` 全仓库无生产调用（仅测试）；Permission 引擎只接 managed-policy `HookOverride`，自有 seam 未与 HookRegistry 桥接 → **插件权限 hook 注册成功但永不生效**（静默）。
- **R-2（高）A2 宿主侧确认：ws seams 构建后未消费**。`ws_seams()`（model_request.rs:1250-1310）正确派发三个 hook 点，结果存 `Prepared.web_socket_seams`（:473,:1065）；`WebSocketSeams` 类型在 oc-ai route 全无消费者，仅 tests/plugin_guest_wiring.rs 断言存在 → `experimental.ws.{handshake,send,receive}` 插件重写永不触达 socket。
- **R-3（中）install 无原子性**：run_installer 直接在最终 `<stamp>/` 目录执行；失败中断留下部分树，`latest_install` 复用判据仅 `node_modules/<pkg>/package.json` 存在，残缺安装可被复用。ref 的 `.staging-*`+落位在 Rust 只剩 GC 侧解析逻辑，从未被创建。
- **R-4（低）tui 子路径探测自相矛盾**：`probe_feature_entry` 按 G-280③ 无 main 回退，但 supervisor 实际走的 `resolve_subpath_entry` **有** main 回退 → main-only 包会把 server entry 当 tui entry 跑 `load.tui`。
- **R-5（低）vcs.transform dispose 语义反转**：dispose 时过滤 `pluginId:` 前缀键，而 `domainStores.vcs` 的键是裸 `definition.id` → 过滤条件几乎恒 false → dispose **清空整表**（其它插件的 vcs 行一并丢失）。worktree/websearch 的同型 dispose 用前缀键（正确）。
- **R-6（低）vcs.transform 的 default editor 是 no-op stub**；ref 真实生效。
- **R-7（低）merge_skills 键回退 `id ?? name`**——ref 以 id 为键；回退可产生不同去重行为。
- **R-8（观测）事件丢弃**：D-4；`dropped_events` 有计数，但插件侧无法感知丢失。
- **R-9（低）`ui.sidebar.section` 行不可点击**（clickable 恒 false）——仅 `slots.register` 物化路径带 click。

## 覆盖声明

已逐行读完：ref `packages/plugin/src/` 全部 64 文件（effect 22 + promise 24 + host/source×4 + app/rpc/storage/worktree/options + tui×4）；ref `core/src/plugin/` 指定 11 文件；rust `oc-plugin-host/src/` 全部 15 文件；`host/host.js` 全读（codemode 解释器段 2309-2962 仅读边界注释与限位常量，按任务书跳过语义细节）；`oc-core/plugin_apply.rs`（1-1309 逐行）、`hooks.rs` 全读。**跳过**（按任务书）：已审内容重读仅作对照；host.js codemode 解释器内部。交叉验证：model_request.rs 派发点、engine/mod.rs Permission 装配、oc-server/plugins.rs tap 转发过滤、oc-ai WebSocketSeams 消费者检索（确认 R-2）。不信任 .md，全部结论出自 file:line。
