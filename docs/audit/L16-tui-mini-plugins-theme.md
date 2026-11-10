# L16 TUI mini / 插件 / 主题 / 语法高亮 — 审查报告

> 审查方式：ref mini/ 43 文件清单+关键文件逐行；feature-plugins/plugin/theme 全覆盖；Rust 侧对应文件全读。所有"缺失"结论均经全 crates grep 验证。
> 参照：/opt/opencode-ref (v2.0.24 TS) · 移植：/opt/opencode-rust

### 1. 覆盖证明
- Ref 侧实际通读/检索：`mini/` 全部 43 文件清单+关键文件逐行（runtime/transport/lifecycle/queue、footer.7 件、tool、verbosity、turn-summary、permission、splash、stream-v2.*、parsers-config.ts 386 行）、`feature-plugins/` 23 文件（builtins 清单+notifications/attention-sounds/audio/diff-viewer-image）、`plugin/` 11 文件（discovery/structure/watch/api/render/markdown/builtins 头部+关键段）、`theme/` 全部+`packages/theme/src/tui/` 12 文件、`devtools/index.ts`、`cli/src/mini.ts`+`mini-host.ts`。
- Rust 侧：`oc-tui/src/mini/` 17 文件、`components/footer/`、`plugin_boot.rs`、`plugin_ui.rs`、`components/plugin_slots.rs`/`plugin_page*.rs`、`sound.rs`+`sound/audio.rs`、`notifications_plugin.rs`、`farewell_solid.rs`、`attention.rs`、`os_notify.rs`、`image_codec.rs`/`image_view.rs`、`oc-theme/` 全部 12 rs + assets、`oc-syntax/` 5 rs + queries、`oc-cli/src/mini.rs`、`oc-plugin-host/src` 结构。

### 2. 逐项对比表

| 功能点 | Ref 实现 (file:line) | Rust 实现 (file:line) | 状态 | 差异说明 |
|---|---|---|---|---|
| mini 运行时/事件循环 | mini/runtime.ts:1125L, runtime.boot.ts | mini/runtime.rs:2174L | ✅ | 注释逐行锚定 ref 行号，设置解析一致 |
| 传输/SSE 折叠 | mini/stream-v2.transport.ts:1898L | mini/transport.rs:2799L | ✅ | 事件折叠、toolCommit(:498)、quiet 分支齐 |
| 生命周期/退出/双击 Ctrl-C | mini/runtime.lifecycle.ts:164,323 | mini/lifecycle.rs:190-195,370 | ✅ | 两段式 exit/interrupt、exitSplash 对齐 |
| 会话队列 | mini/runtime.queue.ts:320L | mini/queue.rs:231L | ✅ | |
| 恢复/replay | cli/mini.ts:123 | mini/runtime.rs:133,239-241 | ✅ | replay 默认 true、limit 有 |
| fragment 调和器 | stream-v2.fragment.ts:19,23 | mini/fragment.rs:32-150 | ✅ | project/delta/end/restore 全有 |
| subagent 跟踪 | stream-v2.subagent.ts:203 | mini/transport.rs:1689-1697 | ✅ | |
| 权限确认 footer | mini/footer.permission.tsx:129,167,549 | components/footer/permission.rs:348-353; transport.rs:1580 | ✅ | Once/Always/Reject 三段、save 条件、esc 语义一致 |
| form footer | mini/footer.form.tsx:246,603 | components/footer/form.rs; lifecycle.rs:382-394 | ✅ | |
| 命令菜单/面板 | mini/footer.command.tsx:139-247 | lifecycle.rs:560-735; footer/mod.rs:4 | ✅ | command/model/agent/variant/settings 五面板 |
| footer 视图树/宽度 | mini/footer.view.tsx, footer.width.ts | mini/footer_tree.rs:2; lifecycle.rs:36 | ✅ | |
| 工具卡片快照 | mini/tool.ts:1358L（全量） | mini/tool_snapshot.rs:1-16 | 🟡 | 仅 Write/Edit/Patch/Task 完成态卡片；toolScroll 滚动窗口、snapQuestion/snapForm/LSP/bash 内联族缺（自述 G-134 有界偏差）；代码块高亮在 mono 卡片未画 |
| verbosity 预设 | mini/verbosity.ts:3-34 | mini/runtime.rs:856-867; lifecycle.rs:903 | ✅ | quiet/default/everything 5 旋钮一致 |
| turn summary | mini/turn-summary.ts:8 | mini/runtime.rs:152-154,339 | ✅ | |
| subagent 检查器 footer | mini/footer.subagent.tsx:16-55 | mini/footer_subagent.rs:1-48 | ✅ | 14 行 body、状态色/图标 |
| splash | mini/splash.ts:307L | mini/splash.rs:89-193 | ✅ | logo 行切片一致 |
| demo/replay 模式 | mini/demo.ts:1073L | mini/demo.rs:553L | 🟡 | 体量约一半，未逐场景核对全部 fixture |
| mono 模式 | mini/mono.ts | mini/mono.rs:194L | ✅ | |
| mini 主题桥接 | mini/theme.ts:317L | mini/theme.rs:333L | ✅ | 透明/非 RGB 直通语义一致 |
| 外部编辑器 | mini/prompt.editor.ts | oc-tui/src/editor.rs:1-12 | ✅ | VISUAL‖EDITOR、无 fallback 列表 |
| mini 入口/host | cli/mini.ts:33-135; mini-host.ts:9 | oc-cli/src/mini.rs:20-163 | ✅ | INTERACTIVE_INPUT_ERROR、/dev/tty 回退、stdin 合并 prompt |
| 通知插件 | feature-plugins/system/notifications.ts:21-80 | notifications_plugin.rs:10-347 | ✅ | 事件矩阵、去重、subagent_done、失败 toast 全对齐 |
| 声音资源/播放 | attention-sounds.node.ts:9-14; audio.ts:14-50 | sound.rs:59-227; assets/audio/*.mp3 | ✅ | 同 5 个 mp3 内嵌；miniaudio 设备+外部播放器双路径（🔵新增 fallback） |
| farewell 退出画面 | routes/session/index.tsx + util/presentation.ts | farewell_solid.rs:183-279 | ✅ | |
| 插件发现 | plugin/discovery.ts:9-53 | plugin_boot.rs:151-190 | 🟡 | Rust 走 config `plugins[]` 条目，非 ref 的 plugins/ 目录扫描约定 |
| 插件结构/槽位策略 | plugin/structure.ts:7-93 | components/plugin_slots.rs:1-92 | ✅ | 5 种 placement、takeover/hierarchy/last-wins/degradation 逐条 |
| 插件 API 面 | plugin/api.tsx:54-311 | oc-plugin-host（bun 子进程 RPC） | ⚠️ | routes/slots 有；**markdown 代码块渲染器注册（api.tsx:168-176 + markdown.ts）全 crates 无对应** 🔴 |
| 插件热重载 | plugin/watch.ts:16-40 | oc-plugin-host/supervisor.rs:262（digest 图） | 🟡 | 有 digest 热重载，但非父目录 watch 语义 |
| runtime 双实现 | runtime-plugin-support.bun/node.ts | host_js.rs（bun-only） | 🟡 | bun 缺席时优雅降级为无插件运行 |
| builtins 清单 | plugin/builtins.ts:15-25（含 Merman、Latex） | 分散：sidebar.rs:95、plugin_page.rs、render/mermaid、render/latex | 🟡 | 功能本体在；组织形态不同；Storybook 🔴 缺失（dev fixture） |
| Plugins 管理 UI | feature-plugins/system/plugins.tsx:353 | plugin_page.rs + plugin_boot.rs 投影 | ✅ | |
| sidebar context/mcp | sidebar/context.tsx:44; mcp.tsx:95 | components/sidebar.rs:95-177 | ✅ | |
| home/prompt footer、btw | home/footer.tsx:107; prompt/btw.tsx:23 | home.rs; composer/autocomplete.rs:143,200 | ✅ | btw → session.aside 命令在 |
| diff-viewer（含 image/tree/menu） | system/diff-viewer*.tsx:1233L+ | components/diff_viewer{,_solid,_mount}.rs | ✅ | 另有 golden tests |
| stats | system/stats.tsx:182 | plugin_page.rs（StatsPage 引用） | ✅ | |
| 主题 schema/解析 | packages/theme/src/tui/{schema,resolve,select,expand,v1-migrate}.ts | oc-theme/src/{schema,resolve,expand,v1_migrate}.rs; select 并入 resolve.rs:77-93 | ✅ | 含 REF_ASSET_DIGESTS 逐资产摘要校验 |
| 内置主题清单 | tui/src/theme/assets/ 34 个 JSON | oc-theme/src/assets/{v1 33+v2 1} | ✅ | 33 v1 + v2/opencode 全量对齐 |
| 主题发现/系统深浅色 | theme/discovery.ts, system.ts | oc-theme/src/discovery.rs, system.rs | ✅ | |
| TUI 组件主题 | theme/component.ts, tui/src/theme/index.ts | oc-tui/src/theme.rs:48K | ✅ | |
| 语法语言目录 | parsers-config.ts:33 filetype + opentui 内置 md/js/ts | oc-syntax/queries.rs 37 词条 | ⚠️ | csharp/kotlin/swift 为**非默认 cargo feature**（Cargo.toml:100-104），默认二进制无此 3 语言；markdown **无注入**——代码围栏内层语言不高亮 |
| grammar 来源 | wasm URL 下载（运行时） | 编译期 native crate | 🔵 | 离线、无网络依赖；版本 pin 对齐 ref |
| 别名 | parsers-config.ts:301,344（udiff/patch、makefile） | oc-syntax lib.rs（udiff/patch、makefile/gnumake、vi） | ✅ | Rust 多 gnumake/vi |
| 高亮引擎 | opentui TreeSitter（TS 侧） | engine.rs（仅 test）+ 运行时走 opentui-core TreeSitterClient | ✅ | 单次解析+缓存（128 块）+500ms 上限 |
| 图片协议 | stream-v2.image.ts; diff-viewer-image.tsx:71 | image_view.rs:12-41; image_codec.rs | ✅ | kitty/sixel 探测、kitty f=100 passthrough、sixel RGBA |
| devtools | devtools/index.ts:1-50 | components/devtools.rs:1-15 | ✅ | groups/set 注册表 + 底栏，debug.devtools 门控 |

### 3. 关键差异与风险 Top 5
1. **插件 markdown 渲染器注册缺失（🔴）**：ref `plugin/api.tsx:168`/`markdown.ts` 允许插件注册自定义语言代码块渲染器，Rust 全 crates 无对应——依赖该 API 的第三方插件在移植版静默失效。
2. **oc-syntax 默认特性裁剪（⚠️）**：csharp/kotlin/swift 在 ref 为默认可用，Rust 需 `lang-*` feature；markdown 代码围栏注入降级为纯文本。默认发布二进制用户可见高亮能力低于 ref。
3. **mini tool.ts 有界移植（🟡）**：tool_snapshot.rs 自述放弃 toolScroll 滚动窗口、snapQuestion/snapForm、LSP/bash 内联族；长输出 bash/lsp 工具在 mini 中信息量低于 ref。
4. **插件发现模型分歧（🟡）**：ref 扫描 `plugins/` 目录（项目+全局，含 symlink）；Rust 走 config `plugins[]` 条目。目录约定下的既有插件不被发现。
5. **插件运行时单轨（🟡）**：ref bun/node 双实现 + 父目录 watch 热重载；Rust 仅 bun 子进程（缺 bun 优雅降级为无插件），热重载为 digest 轮询语义。无 bun 机器上插件功能整体不可用（ref node 路径可跑）。

### 4. 缺失文件/功能汇总
- 🔴 插件 markdown 渲染器注册链（api.tsx markdown 段 + plugin/markdown.ts）
- 🔴 Storybook 插件（feature-plugins/system/storybook/ 7 文件，dev fixture 性质）
- 🟡 mini tool.ts 的 toolScroll / snapQuestion / snapForm / LSP、bash 内联工具族
- 🟡 csharp/kotlin/swift 高亮（默认 feature 关闭）、markdown 注入高亮
- 🟡 plugins/ 目录扫描发现、node 运行时路径、父目录 inode-安全 watch
