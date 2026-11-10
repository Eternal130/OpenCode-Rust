# A3 结论: Rust 侧模块图完成 — 14 crates ≈505k src LOC(+75.6k tests/)；核心映射: oc-core 168.6k(session/runner 引擎+tools+bus+config), oc-ai 68k(protocols/media/route), oc-tui 163k(自绘 Frame→ANSI+~45组件+golden), oc-server 142 ops 生成路由, oc-client 326 方法生成, CodeMode=bun guest(host.js)+oc-core seam；渲染后端自绘非 ratatui

# Rust 侧模块图（opencode-rust vs ref v2.0.24 审查用）

生成/判定约定：`*.gen.rs`、`host/host.js`、`models-dev-snapshot.txt` 为生成/嵌入产物；其余为手写。仅以 .rs 为证据。

## 1. 逐 crate 模块树

### oc-util (src 3.9k LOC, 16 文件)
手写工具库：bunman(bun 进程管理), channel, determinism(时钟/ID/RNG 抽象), encode, flock, glob, global_roots(XDG 根, `APP_NAME="opencode-rust"`), hash, localecmp, mime, npm_config, pathx, procutil, retry, wildcard。

### oc-http (1 文件 394 LOC)
统一 reqwest 0.12 TLS 信任（rustls-platform-verifier，回退 webpki）。导出 `client()/builder()`。

### oc-schema (8.5k LOC, 50 文件)
全手写 wire schema（serde+schemars）。顶层文件按域命名：agent/common/event/events/session_event/misc_events/session*/permission/mcp/pty/persistent_pty/shell/skill/tool/form/filesystem/provider/model/id/identifier/location/reference/service/integration/prompt…。`config/` 子目录 = **config 键全集来源**（`Config.Info` 字段见 §2-core）；另 `config/{agent,command,compaction,experimental,formatter,lsp,mcp,media,model,plugin,policy,provider,reference,tool_output,warming,watcher,websearch,worktree}.rs`。

### oc-theme (3.9k LOC)
builtins, color(OKLCH), discovery, expand(token 展开), resolve, schema, syntax(主题→高亮映射), system, types, v1/v1_migrate(旧版主题迁移)。

### oc-ai (68k LOC, 109 文件)
- `llm.rs` 语义核心；`schema/`(messages/events/options/ids/media/errors)；
- `route/`：protocol(Protocol trait: body_from/initial/step/terminal/on_halt/effort/sanitizer), executor, framing, endpoint, client, auth, transport(+proxy/websocket)；
- `protocols/` **协议适配器全集**（一文件一协议族）：anthropic_messages, openai_chat, openai_responses, openai_compatible, azure, bedrock(converse+mantle+sigv4+event_stream+credentials), gemini, google_interactions, vertex(+vertex_messages), cohere_chat, mistral_chat, xai_responses, copilot(+copilot_responses), alibaba_responses, meta_responses, vercel_gateway, open_responses(+channel/continuation), sanitizer, shared, utils/(partial_json, tool_stream, lifecycle, responses_checkpoint/compaction/hosted_tools, gemini_json_schema, gateway_protocol, open_responses_options)；
- `media/`：protocol 家族 Inline/Stream/Queued；**image**: bfl,fal,google,meta,openai,replicate,stability,xai,zai；**speech**: cartesia,deepgram,elevenlabs,google,openai；**transcription**: assemblyai,deepgram,elevenlabs,google,openai；**video**: fal,google,runway,xai；fal_queue(共享轮询模板)；
- cache/reasoning/effort：`cache_policy.rs`(`apply_cache_policy`), `effort_updates.rs`(effort marker 注入/剥离), `tool_history.rs`, `sanitize.rs`, `provider_error.rs`, `customization.rs`, `json_schema.rs`, testing/test_support。

### oc-core (168.6k LOC, 203 文件) — 主体
见 §2。

### oc-syntax (1.9k LOC)
语法高亮 **vendor 面**：registry(20 filetype：rust/python/typescript/go/c/cpp/java/json/yaml/toml/html/css/diff/javascript/ruby/php/lua/vim/scala/make)+queries(.scm vendored)+scopes/style/engine(engine/scopes/style 已降级为 cfg(test) harness)。运行时高亮走 oc-tui `components::syntax`；toml/vim 语法来自 third_party vendored path crate。

### oc-plugin-host (8.6k src + host.js 3.7k)
bun 子进程宿主：`supervisor.rs`(NDJSON 双向 JSON-RPC、死锁协议、超时), `protocol.rs`(ops 见 §2), `host_js.rs`, `registrations.rs`(commands/themes/agents/skills/tools/MCP 快照), `resolve.rs`, `tap.rs`(BroadcastTap), `ui.rs`, `bun.rs`, `json_schema.rs`, `context/`(HostState/SessionPort/install(npm 冻结安装)/update/codemode_defaults), **`host/host.js` = 生成/打包 guest 入口，含 CodeMode 解释器限制移植**（MAX_STRING_LENGTH/MAX_ARRAY_LENGTH/MAX_PENDING_PROMISES、codemode limits、permission evaluate hook、storage/rpc/slot 等 guest API）。

### oc-pty (1.2k, 独立 bin)
PTY 守护进程：args(`daemon --runtime-dir --name --orphan-grace`), daemon(Unix socket + per-frame token auth + 孤儿宽限自退)，状态机实体在 oc-core `persistent_pty/`。

### oc-server (33.6k)
`app.rs`(dispatch 门), `auth.rs`(Basic/HMAC 会话 token/cookie/auth_token query), `sse.rs`(单 bus tap→4096 有界队列/15s 心跳), `routes.gen.rs`(118 paths/142 ops, 生成), `validation.gen.rs`(生成), `state.rs`(AppState), `boot.rs/lifecycle.rs/readiness.rs/middleware.rs/observability.rs/pairing.rs/tickets.rs/announce.rs(位置 boot/reload 事件级联)/event_manifest.rs/assets.rs(嵌入 openapi.json+静态 shell)/info.rs/plugins.rs/safekill.rs/app_archive.rs/errors.rs/request_validation.rs/server_process_env.rs；`handlers/` 20 个域文件与 routes group 一一对应。

### oc-client (20.4k)
`methods.gen.rs`(326 方法/40 namespace, 生成), `types.gen.rs`(10.6k, 生成), `transport.rs`(HTTP prepare/execute/SSE, G-288 64KiB 诊断上限), `sse.rs`, `bus_event.rs`, `shared_events.rs`, `event_ext.rs`, `rpc_typed.rs`, `error.rs`。

### oc-tui (163k, 199 文件)
- `app.rs`(主循环), `store.rs`(**Route: Home | Session{session_id,prompt}** — 页面=路由+overlay), `layout.rs/reactive.rs/presentation.rs/style.rs/wrap.rs/selection.rs/scroll_accel.rs/suspend.rs/storage.rs/theme.rs/keymap.rs/config.rs(TuiConfig 全键)/input_term.rs/terminal_pane.rs/terminal_proto.rs/editor*.rs(标准/IDE/zed)/image_*.rs/notifications_plugin.rs/os_notify.rs/plugin_boot.rs/plugin_ui.rs/replay.rs/sound.rs/stderr_redirect.rs/attention.rs/clipboard.rs/farewell_solid.rs…
- `components/`（~45 组件）：composer(+attachment/autocomplete/knight/render/golden_tests), footer(+canvas/form/permission/panel/panels/replay/state/theme/prompt_idle…), message/(list/markdown/model/rows/tool_detail), dialog(+config/open/pair/select+fuzzy/worktrees), session_screen/session_tabs(+solid/marquee)/session_tabs_rail/sidebar/ command_palette/ console/ devtools/ diff_viewer(+mount)/ home/ location_missing/ masked_text/ migration_overlay/ overlays/ picker_solid/ plugin_page/ plugin_slots/ stats_page/ subagent_picker/ syntax/ tab_pulse/ terminal_pane_solid/ toast/ tool_rows/ transcript_solid/ tree_capture/ update/ chrome_solid/ + 各 `*_solid.rs`（"solid"=v2 solid 组件双实现）；
- `render/`：**自绘 Frame/paint→ANSI（非 ratatui；crossterm 已退役，raw mode 由自 port 的 opentui Terminal 管）**；ansi(serialize_canonical)/frame/color_mode/paint/opentui_emit；latex/(parser/layout/symbols)；mermaid/(flowchart/sequence/state/gantt/gitgraph/timeline 全渲染器)；
- `mini/`（极简 UI，`opencode mini`）：runtime/transport/lifecycle/scrollback/queue/spinner/splash/theme/mono/fragment/footer_*/tool_snapshot/model_pref/demo/image/prompt_shared；
- golden 测试：`golden_tests.rs`+`screen/home/message/diff_viewer_golden_tests.rs`+components 内 3 处，读 `tools/parity/goldens/tui-frames`（text-grid+ANSI 字节比对）。

### oc-tui-core (4.7k)
@opentui/core 移植切片：animation, scroll, text_edit, **keymap/**（`opentui.rs`=分发引擎 @opentui/keymap 0.5.14 port：layers/focus/modes/interceptor/pending seq；`table.rs`=v2 keybind.ts 全默认表 922 行 + 覆盖解析；`mod.rs`=oc-tui facade Keymap）。

### oc-cli (24.8k)
`cli.rs`(clap 全命令), `main.rs`, `handlers/`(api/auth/auth_form/mcp/misc/plugin/serve/service_cmds/session/stats), `run/`(noninteractive/tool_render), `run_ui.rs`, `mini.rs`, `acp/`(**vendored ACP 客户端+agent** 20 文件：agent/service/transport/translate/prompt/turn/permission/elicitation/sessions/compaction/replay/catalog…), `services/`(server_connection/standalone/updater/update_feed/autoupdate/service_config), `ssh_askpass.rs`, `util.rs`。

### tools/openapi-gen (3.8k)
lib/model/naming/tygen/groups/emit_routes/emit_client/emit_validation/main + tests/gen.rs；产物即 oc-server/routes.gen.rs、validation.gen.rs、oc-client/methods.gen.rs、types.gen.rs、openapi.json。

### tools/parity
场景 harness（py）+ `goldens/`（scenarios/tui-frames/p10 证据 JSON）——只视为测试资产。

## 2. 枚举（全部出自代码）

**oc-core 内建工具**（`tool/`）：文件系统叶 `read write edit glob grep patch`（tool/mod.rs `register_all`，v2 internal.ts pre 顺序）；`tool_input_repair` 插件；非叶 `shell`(tool/shell.rs, NAME="shell"), `skill`, `question`, `webfetch`, `websearch`(+websearch_providers), `subagent`(NAME="subagent"), `list_mcp_resources`/`read_mcp_resource`, `models`/`session_rename`/`session_move`(opencode_tools.rs), `file_diff`/`html_markdown`/`patch_util`/`ripgrep_runner` 辅助；CodeMode：`tool/codemode.rs`（shape_request_tools/catalog/instructions/`EXECUTE_NAME="execute"` 合成工具；解释器在 oc-plugin-host `host/host.js` guest + oc-plugin-host/tests/codemode*.rs）。注意：工具名为 shell/subagent 而非 ref 的 bash/task 命名（代码事实）。

**session/engine 结构**：`engine/mod.rs`(Engine 组合根+boot), `engine/{commands,instructions,resolver,skills,skills_url,warming}.rs`；会话循环在 `session/runner/`：`step.rs`(agent loop 单步), `generate.rs`, `drain.rs`, `model_request.rs`, `compaction.rs`(压缩), `retry.rs`, `max_steps.rs`, `plan.rs`, `optimize.rs`, `prompt_prepare.rs`, `system_prompt.rs`, `title.rs`, `publisher.rs`, `to_llm_message.rs`, `identity.rs`, `context.rs`；`session/{prompt,execution,fork,revert(回退),diff,inbox,store,projector,transfer,subagent_job,subagent_completion,message_updater}.rs`；git 快照 `snapshot.rs`；权限 `permission.rs`+`permission/{saved,sql}.rs`；MCP `mcp/{client,registry,oauth,oauth_page,elicitation}.rs`；凭证 `integration/credential.rs`(+providers/copilot/vertex/jwks/wellknown_config/flows)；技能 `engine/skills.rs`(+skills_url)；插件应用 `plugin_apply.rs`；hooks `hooks.rs`。

**config 键全集**（oc-schema `config/mod.rs` Info 字段 + 子模块）：schema, shell, model, default_agent, update, share, enterprise, username, permissions, agents, snapshots, watcher, formatter, lsp, media, tool_output, mcp, compaction, skills, commands, instructions, references, websearch, plugins, worktree, warming, providers, experimental；TUI 侧键在 oc-tui `config.rs` TuiConfig（§TUI）；CLI 侧 `oc-core/cli_config/{mod,v1,jsonc}.rs`。

**bus 事件类型**（oc-core/bus/registry.rs DEFINITIONS，全量）：ephemeral 非 session：config.updated, credential.updated/switched, filesystem.changed, form.created/replied/cancelled, installation.update-available/updated, integration.updated, location.shutdown, mcp.{status,tools,prompts,resources}.changed, models-dev.refreshed, permission.asked/replied, persistent-pty.added/removed, plugin.updated, project.updated, pty.{created,updated,exited,deleted}, reference.updated, sdk.plugin.updated, shell.{created,updated,exited,deleted}, tui.{command.execute,prompt.append,session.select,toast.show}, vcs.branch.updated, worktree.{resolved,updated}；durable session 聚合：session.created/renamed/deleted(+.2)/deleted.2/forked/moved/viewed/idle/status/metadata.updated/model.selected/agent.selected/permissions/diff/retry.scheduled/instructions.updated/synthetic/shell.started/.ended/skill.activated/compaction.{started,delta,ended,failed}/execution.{started,succeeded,failed,interrupted}/inbox.{enqueued,delivered,cancelled,delivery.changed}/message.content.updated/reasoning.{started,delta,ended}/text.{started,delta,ended}/step.{started,streamed,ended,failed}/tool.{called,input.started,input.delta,input.ended,progress,success,failed}/usage.{recorded,updated}/revert.{staged,committed,cleared}。

**codemap 位置**：解释器=oc-plugin-host/host/host.js（confined interpreter limits 移植+codemode.evaluate guest runtime）；宿主侧 seam=oc-core/tool/codemode.rs（CodeModeRuntime trait、ExecutionLimits opt-in、catalog allowlist/search builtin）。

**oc-ai provider 面**：协议实现=§oc-ai protocols/ 列表（anthropic/openai/azure/bedrock/gemini/vertex/cohere/mistral/xai/copilot/alibaba/meta/vercel/open_responses…）；provider 目录=oc-core/provider_catalog/（native.rs 内置包映射：@ai-sdk/anthropic→anthropic+alibaba(qwen), @ai-sdk/openai→openai+alibaba, openai-compatible→alibaba, bedrock/mantle→amazon-bedrock, @ai-sdk/azure→azure, @ai-sdk/perplexity→perplexity；catalog.rs= models.dev 嵌入快照 models-dev-snapshot.txt；azure_sync/copilot_sync/digitalocean_sync/modal_sync 动态同步；overlay/facade/refresh/variant/support）。流式管线=route/framing→protocol.step 纯状态机；重试在 oc-core session/runner/retry.rs（is_retryable/is_context_overflow 在 runner/mod.rs）。

**TUI 组件/路由/keymap**：Route=Home/Session 二页+overlay（command_palette/dialog*/stats_page/plugin_page/devtools/console/toast…）；keymap 命令全集≈150 条（table.rs）：app.{exit,clear,console,debug,scrap,toggle.*}, agent.{cycle,cycle.reverse,list}, command.palette.show, composer.{close,shell.*,subagent.*,tab.*,terminal.*}, dialog.*（integration/mcp/move_session/plugins/prompt/select/worktree）, diff.*（24 条）, docs.open, help.show, input.*（编辑 35 条）, location.reload, mcp.list, messages.copy, model.{cycle_favorite(+reverse),cycle_recent(+reverse),dialog.*,list}, opencode.{debug,settings,status}, open.menu, pane.focus.*, permission.prompt.fullscreen, plugins.*, prompt.*（autocomplete.*/clear/editor/history.*/images.view/paste/queue/skills/stash*/submit）, provider.connect, queued_prompt.*, server.pair, service.restart, session.*（36 条）, stash.*, stats.{open,close}, terminal.*（close/select/suspend/title.toggle/toggle）, theme.{mode.lock,switch,switch_mode}, variant.{cycle,list}。TuiConfig 键：keybinds/theme(name,mode)/leader_timeout_ms/attention/session_image_preview/tabs/side bar(session_sidebar)/mouse/session_permissions/session_thinking/session_verbosity/session_tps/debug_turn_tokens/session_markdown_source/session_grouping/cursor/terminal_copy/prompt_paste_compact/path。主题 token 在 oc-theme types/builtins + oc-tui/theme.rs；mini 模式=mini/ 目录（独立 transport/runtime）；插件 UI=plugin_slots/plugin_page/plugin_ui/plugin_boot（经 oc-plugin-host registrations/tap）；语法高亮=oc-syntax catalog + oc-tui components/syntax（opentui-core TreeSitterClient 注入）；渲染后端=自绘 Frame→ANSI（render/，golden 比对 serialize_canonical 文本网格）。

**oc-cli 命令**（cli.rs Commands）：Upgrade(update)[--method], Uninstall[-c -d --dry-run -f], Acp, Api[request -d -H --param], Debug{Agents,Config,Paths}, Auth{List,Login,Logout,Export,Import,Switch}, Mcp{List,Add,Auth,Logout}, Plugin{List,Add,Check,Update,Remove}, Models, Stats[--days --year --all --project --models --tools --cost --full --limit --json], Mini[-c -s --fork --(no-)replay --replay-limit -m --agent --prompt --demo], Run[MESSAGE -c -s --fork -m --agent --format -f --title --thinking], Session{List,Delete,Export,Import,Move,Rename…}, Service{Start,Restart,Status,Stop,Get,Set,Unset…}, Reload, Pair[--url], Completion(rust-extra), Serve[--hostname --port --cors --service --stdio]；全局 flags：--print-logs/--log-level/--color/-c(config?) /-s(session)/--prompt 等（cli.rs 头部）。

**oc-server 端点**（routes.gen.rs，142 ops/118 paths；GET71 POST51 DELETE16 PATCH5 PUT5）：agent(2), command(1), config(get,shells)+experimental/config, credential(5 CRUD+activate), debug/location(2), event(SSE subscribe), experimental/fs|generate|integration|mcp|migration.v1|session.import|session.stats|session.export|instructions entries|session.log|skill|wait|persistent-pty(11), form(1), fs(4), info, integration(+oauth/command 连接流 9), location(2), mcp(5+resource), model(2), pair, permission.request/saved(4), plugin(4), project(2), provider(2), pty(7), reference, rpc.call, server(info/pair+experimental persistentPty), session(32: CRUD/agent/background/command/compact/context/diff/environment/fork/generate/inbox/interrupt/message/model/move/permission/prompt/revert/{clear,commit,stage}/shell/synthetic/view), session.form(5), session.inbox(3), session.instructions(3), session.revert(3), shell(5), skill, vcs(5+branch), websearch(2), worktree(4)（全表见 routes.gen.rs ROUTES）。

**oc-client 方法**（methods.gen.rs 40 namespace/326 方法）：与 routes 一一对应（agent,branch,command,config,connect,credential,debug,event,form,fs,generate,inbox,instructions,integration,location,mcp,message,migration,model,oauth,permission,persistent_pty,plugin,project,provider,pty,reference,request,resource,revert,rpc,saved,server,session,shell,skill,vcs,websearch,worktree）。Transport=reqwest HTTP+SSE（sse.rs SseStream）。

**oc-plugin-host API 面**（protocol.rs+host.js 双向 ops）：guest→host: command.add, theme.register, event.subscribe, ping, plugin.{list,fault,idle,busy}, rpc.register/rpc.internal, storage.{get,set,scan,remove}, slot.{materialize,click}, sidebar.content, tui.{theme,route,layout}, ui.{prompt,confirm,alert}, model.list, provider.{list,get,transform,reload}, tool.transform/tool.execute, skill.transform, agent…(registrations), worktree/websearch/vcs/reference/skill transform, permission.evaluate(hook), codemode.evaluate；host→guest: 事件派发、hook 回调、session port 查询。进程模型=受监督 `bun host.js` 子进程、NDJSON JSON-RPC、双向并发、2s/5s 默认超时、无 bun 时子系统自禁用。

## 3. 测试盘点

tests/ 目录文件数：oc-ai 20, oc-cli 8, oc-client 3, oc-core 17, oc-plugin-host 37, oc-pty 2, oc-schema 2, oc-server 38, oc-theme 2, oc-tui 60（含 pty_*/vt_*/mini_*_golden/spec_*/parity_tui_behavior）, oc-util 1；oc-http/oc-syntax/oc-tui-core 无 tests/。含 `#[cfg(test)]` 的 src 文件数：oc-tui 131, oc-core 115, oc-ai 84, oc-cli 40, oc-server 29, oc-schema 34, oc-plugin-host 13, oc-util 15, oc-theme 8, oc-tui-core 6, oc-syntax 4, oc-client 5, oc-pty 1, oc-http 1。Golden/快照：oc-tui `*_golden_tests.rs`×8（tools/parity/goldens/tui-frames text-grid+ANSI）+ tests/mini_scrollback_golden.rs、mini_footer_golden.rs、vt_harness/temporal（终端仿真黄金）；oc-ai protocols/*/tests.rs（流帧固定样例）；oc-core session/runner/tests_recorded.rs（录制会话回放）；tools/parity/goldens/{scenarios,tui-frames,p10}。

## 4. LOC 表

估算方法：`src 非测试 ≈ src总行 − (每文件自首个 #[cfg(test)] 至 EOF 行数和) − 文件名含 test 的 src 模块行`；tests/ 目录为精确 wc。

| crate | src 总 | 内联测试≈(至EOF法) | src 内 test 模块文件 | tests/ 目录 |
|---|---|---|---|---|
| oc-util | 3,952 | 1,745 | 0 | 0 |
| oc-http | 394 | 214 | 0 | 0 |
| oc-schema | 8,468 | 2,252 | 0 | 671 |
| oc-theme | 3,910 | 572 | 0 | 158 |
| oc-ai | 68,365 | 14,783 | 8,766 | 7,012 |
| oc-core | 168,607 | 45,515 | 30,839 | 9,989 |
| oc-syntax | 1,880 | 1,472 | 0 | 0 |
| oc-plugin-host | 8,602(+host.js 3,733) | 1,070 | 0 | 5,661 |
| oc-pty | 1,217 | 70 | 0 | 3,212 |
| oc-server | 33,571 | 7,777 | 0 | 16,000 |
| oc-client | 20,442 | 943 | 0 | 1,222 |
| oc-tui | 163,111 | 47,427 | 8,014 | 26,001 |
| oc-tui-core | 4,672 | 1,595 | 0 | 0 |
| oc-cli | 24,756 | 6,045 | 0 | 5,635 |
| **合计 crates** | **≈504,947(+3.7k JS)** | | | **≈75,561** |
| tools/openapi-gen | 3,772 | — | — | — |
| third_party *.rs | 79,537(含 vendored ACP+grammars+oc-uqr) | — | — | — |

## 5. 「无参考对应物 / Rust 独有」清单（代码层推断）

- `oc-pty` bin + oc-core `persistent_pty/`(frontend/transport/wire) + server persistent-pty 端点族——持久 PTY 守护进程为移植新增面（ref 无独立 daemon 二进制的对应结构）。
- `oc-syntax`（tree-sitter 原生 grammar vendor）+ third_party vendored tree-sitter-{toml,vim,kotlin,vue}——ref 走 wasm parser。
- `third_party/agent-client-protocol(-schema)` vendored ACP + oc-cli/acp/ 完整实现。
- `oc-core/v2_migration.rs` + `database/v1_migration.rs`——上游 TS 数据目录一次性迁入 `opencode-rust` 隔离根（ref 的 v1→v2 是原地迁移）。
- `oc-http`（双 reqwest 栈 TLS 统一）、`oc-util/bunman`、`tools/openapi-gen`+routes.gen/validation.gen/methods.gen/types.gen 生成管线、`tools/parity` harness、`oc-cli` Completion 命令（代码注释自认 rust-extra）、`*_solid.rs` 双组件实现、oc-tui `render/mermaid`+`render/latex` 自绘渲染器、`sound.rs`/`os_notify.rs` 等——这些在 ref 对应面上要么是生成器替代（ref 手写 client），要么为新增。
- CodeMode 解释器换壳：ref=in-process confined interpreter；Rust=bun guest(host.js)+oc-core seam——语义对齐但执行位置不同（审查重点）。
