# RPC-Methods Census（缺口普查 · 通道 1：插件 RPC 派发面）

> rust 桥派发表（answer_guest + 前缀族展开）× ref ctx 方法面（adapter.ts context2）。

> 生成命令：`python3 tools/census/rpc_methods.py --write docs/audit/census/RPC.md`
> 确定性：全部提取自当前代码静态扫描，排序输出，无时间戳。

## 轴 1：rust 桥派发面（supervisor.rs `answer_guest` 匹配臂 + 前缀族白名单）

- 提取 guest→host 方法 **105** 个（`crates/oc-plugin-host/src/supervisor.rs`；`session.*` 由 `crates/oc-plugin-host/src/context/mod.rs` SESSION_METHODS（17 项）展开，另含 codemode.tool / generate.text）。
  - `agent.transform` — crates/oc-plugin-host/src/supervisor.rs:574
  - `codemode.tool` — crates/oc-plugin-host/src/context/mod.rs（const 展开）
  - `command.add` — crates/oc-plugin-host/src/supervisor.rs:571
  - `event.subscribe` — crates/oc-plugin-host/src/supervisor.rs:708
  - `experimental.terminal.read` — crates/oc-plugin-host/src/supervisor.rs:670
  - `generate.text` — crates/oc-plugin-host/src/context/mod.rs（const 展开）
  - `hook.register` — crates/oc-plugin-host/src/supervisor.rs:591
  - `integration.command.cancel` — crates/oc-plugin-host/src/supervisor.rs:686
  - `integration.command.connect` — crates/oc-plugin-host/src/supervisor.rs:684
  - `integration.command.status` — crates/oc-plugin-host/src/supervisor.rs:685
  - `integration.connect.key` — crates/oc-plugin-host/src/supervisor.rs:679
  - `integration.connection.active` — crates/oc-plugin-host/src/supervisor.rs:687
  - `integration.connection.resolve` — crates/oc-plugin-host/src/supervisor.rs:688
  - `integration.connection.status` — crates/oc-plugin-host/src/supervisor.rs:689
  - `integration.get` — crates/oc-plugin-host/src/supervisor.rs:678
  - `integration.list` — crates/oc-plugin-host/src/supervisor.rs:677
  - `integration.method.list` — crates/oc-plugin-host/src/supervisor.rs:691
  - `integration.method.update` — crates/oc-plugin-host/src/supervisor.rs:590
  - `integration.oauth.cancel` — crates/oc-plugin-host/src/supervisor.rs:683
  - `integration.oauth.complete` — crates/oc-plugin-host/src/supervisor.rs:682
  - `integration.oauth.connect` — crates/oc-plugin-host/src/supervisor.rs:680
  - `integration.oauth.status` — crates/oc-plugin-host/src/supervisor.rs:681
  - `integration.reload` — crates/oc-plugin-host/src/supervisor.rs:690
  - `integration.transform` — crates/oc-plugin-host/src/supervisor.rs:584
  - `keymap.layer` — crates/oc-plugin-host/src/supervisor.rs:744
  - `log` — crates/oc-plugin-host/src/supervisor.rs:750
  - `markdown.renderer.register` — crates/oc-plugin-host/src/supervisor.rs:747
  - `markdown.renderer.unregister` — crates/oc-plugin-host/src/supervisor.rs:747
  - `mcp.transform` — crates/oc-plugin-host/src/supervisor.rs:574
  - `model.default` — crates/oc-plugin-host/src/supervisor.rs:654
  - `model.list` — crates/oc-plugin-host/src/supervisor.rs:654
  - `model.reload` — crates/oc-plugin-host/src/supervisor.rs:654
  - `model.transform` — crates/oc-plugin-host/src/supervisor.rs:583
  - `permission.get` — crates/oc-plugin-host/src/supervisor.rs:622
  - `permission.list` — crates/oc-plugin-host/src/supervisor.rs:622
  - `permission.reply` — crates/oc-plugin-host/src/supervisor.rs:622
  - `ping` — crates/oc-plugin-host/src/supervisor.rs:708
  - `plugin.list` — crates/oc-plugin-host/src/supervisor.rs:661
  - `provider.get` — crates/oc-plugin-host/src/supervisor.rs:648
  - `provider.list` — crates/oc-plugin-host/src/supervisor.rs:648
  - `provider.reload` — crates/oc-plugin-host/src/supervisor.rs:648
  - `provider.transform` — crates/oc-plugin-host/src/supervisor.rs:582
  - `reference.list` — crates/oc-plugin-host/src/supervisor.rs:664
  - `reference.reload` — crates/oc-plugin-host/src/supervisor.rs:664
  - `reference.transform` — crates/oc-plugin-host/src/supervisor.rs:583
  - `rpc.event` — crates/oc-plugin-host/src/supervisor.rs:601
  - `rpc.invoke` — crates/oc-plugin-host/src/supervisor.rs:611
  - `rpc.register` — crates/oc-plugin-host/src/supervisor.rs:595
  - `session.command` — crates/oc-plugin-host/src/context/mod.rs（const 展开）
  - `session.compact` — crates/oc-plugin-host/src/context/mod.rs（const 展开）
  - `session.context` — crates/oc-plugin-host/src/context/mod.rs（const 展开）
  - `session.create` — crates/oc-plugin-host/src/context/mod.rs（const 展开）
  - `session.generate` — crates/oc-plugin-host/src/context/mod.rs（const 展开）
  - `session.get` — crates/oc-plugin-host/src/context/mod.rs（const 展开）
  - `session.interrupt` — crates/oc-plugin-host/src/context/mod.rs（const 展开）
  - `session.list` — crates/oc-plugin-host/src/context/mod.rs（const 展开）
  - `session.messages` — crates/oc-plugin-host/src/context/mod.rs（const 展开）
  - `session.move` — crates/oc-plugin-host/src/context/mod.rs（const 展开）
  - `session.prompt` — crates/oc-plugin-host/src/context/mod.rs（const 展开）
  - `session.remove` — crates/oc-plugin-host/src/context/mod.rs（const 展开）
  - `session.switchAgent` — crates/oc-plugin-host/src/context/mod.rs（const 展开）
  - `session.switchModel` — crates/oc-plugin-host/src/context/mod.rs（const 展开）
  - `session.synthetic` — crates/oc-plugin-host/src/context/mod.rs（const 展开）
  - `session.update` — crates/oc-plugin-host/src/context/mod.rs（const 展开）
  - `session.wait` — crates/oc-plugin-host/src/context/mod.rs（const 展开）
  - `skill.transform` — crates/oc-plugin-host/src/supervisor.rs:574
  - `storage.get` — crates/oc-plugin-host/src/supervisor.rs:615
  - `storage.remove` — crates/oc-plugin-host/src/supervisor.rs:615
  - `storage.scan` — crates/oc-plugin-host/src/supervisor.rs:615
  - `storage.set` — crates/oc-plugin-host/src/supervisor.rs:615
  - `theme.register` — crates/oc-plugin-host/src/supervisor.rs:571
  - `tool.transform` — crates/oc-plugin-host/src/supervisor.rs:574
  - `ui.alert` — crates/oc-plugin-host/src/supervisor.rs:724
  - `ui.confirm` — crates/oc-plugin-host/src/supervisor.rs:724
  - `ui.dialog.open` — crates/oc-plugin-host/src/supervisor.rs:711
  - `ui.dialog.register` — crates/oc-plugin-host/src/supervisor.rs:740
  - `ui.footer.hint` — crates/oc-plugin-host/src/supervisor.rs:711
  - `ui.group.rule` — crates/oc-plugin-host/src/supervisor.rs:740
  - `ui.home.quickaction` — crates/oc-plugin-host/src/supervisor.rs:740
  - `ui.panel.close` — crates/oc-plugin-host/src/supervisor.rs:714
  - `ui.panel.open` — crates/oc-plugin-host/src/supervisor.rs:724
  - `ui.prompt` — crates/oc-plugin-host/src/supervisor.rs:724
  - `ui.router.register` — crates/oc-plugin-host/src/supervisor.rs:744
  - `ui.sidebar.section` — crates/oc-plugin-host/src/supervisor.rs:711
  - `ui.slot.claim` — crates/oc-plugin-host/src/supervisor.rs:744
  - `ui.slot.register` — crates/oc-plugin-host/src/supervisor.rs:731
  - `ui.slot.rerender` — crates/oc-plugin-host/src/supervisor.rs:731
  - `ui.toast.show` — crates/oc-plugin-host/src/supervisor.rs:711
  - `vcs.base` — crates/oc-plugin-host/src/supervisor.rs:635
  - `vcs.branch.list` — crates/oc-plugin-host/src/supervisor.rs:635
  - `vcs.diff` — crates/oc-plugin-host/src/supervisor.rs:636
  - `vcs.get` — crates/oc-plugin-host/src/supervisor.rs:635
  - `vcs.reload` — crates/oc-plugin-host/src/supervisor.rs:635
  - `vcs.status` — crates/oc-plugin-host/src/supervisor.rs:635
  - `vcs.transform` — crates/oc-plugin-host/src/supervisor.rs:583
  - `websearch.default.get` — crates/oc-plugin-host/src/supervisor.rs:628
  - `websearch.default.set` — crates/oc-plugin-host/src/supervisor.rs:629
  - `websearch.providers` — crates/oc-plugin-host/src/supervisor.rs:628
  - `websearch.query` — crates/oc-plugin-host/src/supervisor.rs:628
  - `websearch.transform` — crates/oc-plugin-host/src/supervisor.rs:582
  - `worktree.create` — crates/oc-plugin-host/src/supervisor.rs:642
  - `worktree.list` — crates/oc-plugin-host/src/supervisor.rs:642
  - `worktree.refresh` — crates/oc-plugin-host/src/supervisor.rs:642
  - `worktree.remove` — crates/oc-plugin-host/src/supervisor.rs:642
  - `worktree.transform` — crates/oc-plugin-host/src/supervisor.rs:582

## 轴 2：rust guest 实际调用面（host.js `hostRequest(` 字面量）

- 提取调用 **94** 处方法名（含 `session.<m>` 模板族）。
- **已服务但 guest 无调用者：0**（死面候选，非缺口）
- **guest 调用但未入派发表：0**（若 >0 需排查）

## 轴 3：diff（rust 派发面 vs ref ctx 方法面）

- rust：105 · ref：89 · 并集：118
- **BOTH：76** · REF_ONLY：**13** · RUST_ONLY：**29**

### REF_ONLY（ref 有、rust 无 → 缺口候选）
  - 🔴 `agent.get` — /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:296（rust 于 guest 本地实现（host.js 编辑器表/本地表），桥层缺口候选——行为等价归行为通道）
  - 🔴 `agent.list` — /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:297（rust 于 guest 本地实现（host.js 编辑器表/本地表），桥层缺口候选——行为等价归行为通道）
  - 🔴 `agent.reload` — /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:299（rust 于 guest 本地实现（host.js 编辑器表/本地表），桥层缺口候选——行为等价归行为通道）
  - 🔴 `command.list` — /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:308（rust 于 guest 本地实现（host.js 编辑器表/本地表），桥层缺口候选——行为等价归行为通道）
  - 🔴 `command.reload` — /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:322（rust 于 guest 本地实现（host.js 编辑器表/本地表），桥层缺口候选——行为等价归行为通道）
  - 🔴 `mcp.list` — /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:434（rust 于 guest 本地实现（host.js 编辑器表/本地表），桥层缺口候选——行为等价归行为通道）
  - 🔴 `mcp.reload` — /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:436（rust 于 guest 本地实现（host.js 编辑器表/本地表），桥层缺口候选——行为等价归行为通道）
  - 🔴 `skill.list` — /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:455（rust 于 guest 本地实现（host.js 编辑器表/本地表），桥层缺口候选——行为等价归行为通道）
  - 🔴 `skill.reload` — /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:457（rust 于 guest 本地实现（host.js 编辑器表/本地表），桥层缺口候选——行为等价归行为通道）
  - 🔴 `tool.list` — /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:467（rust 于 guest 本地实现（host.js 编辑器表/本地表），桥层缺口候选——行为等价归行为通道）
  - 🔴 `tool.reload` — /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:466（rust 于 guest 本地实现（host.js 编辑器表/本地表），桥层缺口候选——行为等价归行为通道）
  - 🔴 `websearch.reload` — /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:542（rust 于 guest 本地实现（host.js 编辑器表/本地表），桥层缺口候选——行为等价归行为通道）
  - 🔴 `worktree.reload` — /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:563（rust 于 guest 本地实现（host.js 编辑器表/本地表），桥层缺口候选——行为等价归行为通道）

### RUST_ONLY（rust 额外面 → 需登记册背书）
  - 🔗 `codemode.tool` — crates/oc-plugin-host/src/context/mod.rs（wave 4 Code Mode 嵌套工具桥（G-247 面内））
  - 🔗 `integration.method.list` — crates/oc-plugin-host/src/supervisor.rs:691（待登记册决策）
  - 🔗 `integration.transform` — crates/oc-plugin-host/src/supervisor.rs:584（待登记册决策）
  - 🔗 `keymap.layer` — crates/oc-plugin-host/src/supervisor.rs:744（TUI-25 / G-237 / PLUG-TUI-SLOT（rust TUI 插件 UI 面；ref v2 server-ctx 无 ui 域——ref 的 UI 面在独立 tui 入口，见 PLUGIN-API.md））
  - 🔗 `log` — crates/oc-plugin-host/src/supervisor.rs:750（待登记册决策）
  - 🔗 `markdown.renderer.register` — crates/oc-plugin-host/src/supervisor.rs:747（待登记册决策）
  - 🔗 `markdown.renderer.unregister` — crates/oc-plugin-host/src/supervisor.rs:747（待登记册决策）
  - 🔗 `ping` — crates/oc-plugin-host/src/supervisor.rs:708（rust 桥活探（死锁规则配套，无 ref 对应面））
  - 🔗 `session.list` — crates/oc-plugin-host/src/context/mod.rs（rust SESSION_METHODS 白名单注记（flagship 探针读面，ref 15 方法 Pick 外））
  - 🔗 `session.messages` — crates/oc-plugin-host/src/context/mod.rs（rust SESSION_METHODS 白名单注记（flagship 探针读面，ref 15 方法 Pick 外））
  - 🔗 `theme.register` — crates/oc-plugin-host/src/supervisor.rs:571（wave 1 rust 面（ref v2 ctx 无 theme 域））
  - 🔗 `ui.alert` — crates/oc-plugin-host/src/supervisor.rs:724（TUI-25 / G-237 / PLUG-TUI-SLOT（rust TUI 插件 UI 面；ref v2 server-ctx 无 ui 域——ref 的 UI 面在独立 tui 入口，见 PLUGIN-API.md））
  - 🔗 `ui.confirm` — crates/oc-plugin-host/src/supervisor.rs:724（TUI-25 / G-237 / PLUG-TUI-SLOT（rust TUI 插件 UI 面；ref v2 server-ctx 无 ui 域——ref 的 UI 面在独立 tui 入口，见 PLUGIN-API.md））
  - 🔗 `ui.dialog.open` — crates/oc-plugin-host/src/supervisor.rs:711（TUI-25 / G-237 / PLUG-TUI-SLOT（rust TUI 插件 UI 面；ref v2 server-ctx 无 ui 域——ref 的 UI 面在独立 tui 入口，见 PLUGIN-API.md））
  - 🔗 `ui.dialog.register` — crates/oc-plugin-host/src/supervisor.rs:740（TUI-25 / G-237 / PLUG-TUI-SLOT（rust TUI 插件 UI 面；ref v2 server-ctx 无 ui 域——ref 的 UI 面在独立 tui 入口，见 PLUGIN-API.md））
  - 🔗 `ui.footer.hint` — crates/oc-plugin-host/src/supervisor.rs:711（TUI-25 / G-237 / PLUG-TUI-SLOT（rust TUI 插件 UI 面；ref v2 server-ctx 无 ui 域——ref 的 UI 面在独立 tui 入口，见 PLUGIN-API.md））
  - 🔗 `ui.group.rule` — crates/oc-plugin-host/src/supervisor.rs:740（TUI-25 / G-237 / PLUG-TUI-SLOT（rust TUI 插件 UI 面；ref v2 server-ctx 无 ui 域——ref 的 UI 面在独立 tui 入口，见 PLUGIN-API.md））
  - 🔗 `ui.home.quickaction` — crates/oc-plugin-host/src/supervisor.rs:740（TUI-25 / G-237 / PLUG-TUI-SLOT（rust TUI 插件 UI 面；ref v2 server-ctx 无 ui 域——ref 的 UI 面在独立 tui 入口，见 PLUGIN-API.md））
  - 🔗 `ui.panel.close` — crates/oc-plugin-host/src/supervisor.rs:714（TUI-25 / G-237 / PLUG-TUI-SLOT（rust TUI 插件 UI 面；ref v2 server-ctx 无 ui 域——ref 的 UI 面在独立 tui 入口，见 PLUGIN-API.md））
  - 🔗 `ui.panel.open` — crates/oc-plugin-host/src/supervisor.rs:724（TUI-25 / G-237 / PLUG-TUI-SLOT（rust TUI 插件 UI 面；ref v2 server-ctx 无 ui 域——ref 的 UI 面在独立 tui 入口，见 PLUGIN-API.md））
  - 🔗 `ui.prompt` — crates/oc-plugin-host/src/supervisor.rs:724（TUI-25 / G-237 / PLUG-TUI-SLOT（rust TUI 插件 UI 面；ref v2 server-ctx 无 ui 域——ref 的 UI 面在独立 tui 入口，见 PLUGIN-API.md））
  - 🔗 `ui.router.register` — crates/oc-plugin-host/src/supervisor.rs:744（TUI-25 / G-237 / PLUG-TUI-SLOT（rust TUI 插件 UI 面；ref v2 server-ctx 无 ui 域——ref 的 UI 面在独立 tui 入口，见 PLUGIN-API.md））
  - 🔗 `ui.sidebar.section` — crates/oc-plugin-host/src/supervisor.rs:711（TUI-25 / G-237 / PLUG-TUI-SLOT（rust TUI 插件 UI 面；ref v2 server-ctx 无 ui 域——ref 的 UI 面在独立 tui 入口，见 PLUGIN-API.md））
  - 🔗 `ui.slot.claim` — crates/oc-plugin-host/src/supervisor.rs:744（TUI-25 / G-237 / PLUG-TUI-SLOT（rust TUI 插件 UI 面；ref v2 server-ctx 无 ui 域——ref 的 UI 面在独立 tui 入口，见 PLUGIN-API.md））
  - 🔗 `ui.slot.register` — crates/oc-plugin-host/src/supervisor.rs:731（TUI-25 / G-237 / PLUG-TUI-SLOT（rust TUI 插件 UI 面；ref v2 server-ctx 无 ui 域——ref 的 UI 面在独立 tui 入口，见 PLUGIN-API.md））
  - 🔗 `ui.slot.rerender` — crates/oc-plugin-host/src/supervisor.rs:731（TUI-25 / G-237 / PLUG-TUI-SLOT（rust TUI 插件 UI 面；ref v2 server-ctx 无 ui 域——ref 的 UI 面在独立 tui 入口，见 PLUGIN-API.md））
  - 🔗 `ui.toast.show` — crates/oc-plugin-host/src/supervisor.rs:711（TUI-25 / G-237 / PLUG-TUI-SLOT（rust TUI 插件 UI 面；ref v2 server-ctx 无 ui 域——ref 的 UI 面在独立 tui 入口，见 PLUGIN-API.md））
  - 🔗 `websearch.default.get` — crates/oc-plugin-host/src/supervisor.rs:628（G-24 websearch 域（登记册 G-24 裁决面））
  - 🔗 `websearch.default.set` — crates/oc-plugin-host/src/supervisor.rs:629（G-24 websearch 域（登记册 G-24 裁决面））

### BOTH（双侧 file:line）
  - ✅ `agent.transform` — rust crates/oc-plugin-host/src/supervisor.rs:574 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:298
  - ✅ `command.add` — rust crates/oc-plugin-host/src/supervisor.rs:571 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:309
  - ✅ `event.subscribe` — rust crates/oc-plugin-host/src/supervisor.rs:708 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:325
  - ✅ `experimental.terminal.read` — rust crates/oc-plugin-host/src/supervisor.rs:670 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:336
  - ✅ `generate.text` — rust crates/oc-plugin-host/src/context/mod.rs:· · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:340
  - ✅ `hook.register` — rust crates/oc-plugin-host/src/supervisor.rs:591 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:302
  - ✅ `integration.command.cancel` — rust crates/oc-plugin-host/src/supervisor.rs:686 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:381
  - ✅ `integration.command.connect` — rust crates/oc-plugin-host/src/supervisor.rs:684 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:373
  - ✅ `integration.command.status` — rust crates/oc-plugin-host/src/supervisor.rs:685 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:377
  - ✅ `integration.connect.key` — rust crates/oc-plugin-host/src/supervisor.rs:679 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:358
  - ✅ `integration.connection.active` — rust crates/oc-plugin-host/src/supervisor.rs:687 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:428
  - ✅ `integration.connection.resolve` — rust crates/oc-plugin-host/src/supervisor.rs:688 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:429
  - ✅ `integration.connection.status` — rust crates/oc-plugin-host/src/supervisor.rs:689 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:430
  - ✅ `integration.get` — rust crates/oc-plugin-host/src/supervisor.rs:678 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:356
  - ✅ `integration.list` — rust crates/oc-plugin-host/src/supervisor.rs:677 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:355
  - ✅ `integration.method.update` — rust crates/oc-plugin-host/src/supervisor.rs:590 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:386
  - ✅ `integration.oauth.cancel` — rust crates/oc-plugin-host/src/supervisor.rs:683 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:370
  - ✅ `integration.oauth.complete` — rust crates/oc-plugin-host/src/supervisor.rs:682 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:366
  - ✅ `integration.oauth.connect` — rust crates/oc-plugin-host/src/supervisor.rs:680 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:361
  - ✅ `integration.oauth.status` — rust crates/oc-plugin-host/src/supervisor.rs:681 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:365
  - ✅ `integration.reload` — rust crates/oc-plugin-host/src/supervisor.rs:690 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:426
  - ✅ `mcp.transform` — rust crates/oc-plugin-host/src/supervisor.rs:574 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:435
  - ✅ `model.default` — rust crates/oc-plugin-host/src/supervisor.rs:654 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:344
  - ✅ `model.list` — rust crates/oc-plugin-host/src/supervisor.rs:654 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:343
  - ✅ `model.reload` — rust crates/oc-plugin-host/src/supervisor.rs:654 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:346
  - ✅ `model.transform` — rust crates/oc-plugin-host/src/supervisor.rs:583 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:345
  - ✅ `permission.get` — rust crates/oc-plugin-host/src/supervisor.rs:622 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:442
  - ✅ `permission.list` — rust crates/oc-plugin-host/src/supervisor.rs:622 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:441
  - ✅ `permission.reply` — rust crates/oc-plugin-host/src/supervisor.rs:622 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:443
  - ✅ `plugin.list` — rust crates/oc-plugin-host/src/supervisor.rs:661 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:446
  - ✅ `provider.get` — rust crates/oc-plugin-host/src/supervisor.rs:648 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:350
  - ✅ `provider.list` — rust crates/oc-plugin-host/src/supervisor.rs:648 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:349
  - ✅ `provider.reload` — rust crates/oc-plugin-host/src/supervisor.rs:648 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:352
  - ✅ `provider.transform` — rust crates/oc-plugin-host/src/supervisor.rs:582 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:351
  - ✅ `reference.list` — rust crates/oc-plugin-host/src/supervisor.rs:664 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:449
  - ✅ `reference.reload` — rust crates/oc-plugin-host/src/supervisor.rs:664 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:451
  - ✅ `reference.transform` — rust crates/oc-plugin-host/src/supervisor.rs:583 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:450
  - ✅ `rpc.event` — rust crates/oc-plugin-host/src/supervisor.rs:601 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:77
  - ✅ `rpc.invoke` — rust crates/oc-plugin-host/src/supervisor.rs:611 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:77
  - ✅ `rpc.register` — rust crates/oc-plugin-host/src/supervisor.rs:595 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:77
  - ✅ `session.command` — rust crates/oc-plugin-host/src/context/mod.rs:· · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:591
  - ✅ `session.compact` — rust crates/oc-plugin-host/src/context/mod.rs:· · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:592
  - ✅ `session.context` — rust crates/oc-plugin-host/src/context/mod.rs:· · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:598
  - ✅ `session.create` — rust crates/oc-plugin-host/src/context/mod.rs:· · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:584
  - ✅ `session.generate` — rust crates/oc-plugin-host/src/context/mod.rs:· · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:590
  - ✅ `session.get` — rust crates/oc-plugin-host/src/context/mod.rs:· · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:585
  - ✅ `session.interrupt` — rust crates/oc-plugin-host/src/context/mod.rs:· · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:594
  - ✅ `session.move` — rust crates/oc-plugin-host/src/context/mod.rs:· · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:596
  - ✅ `session.prompt` — rust crates/oc-plugin-host/src/context/mod.rs:· · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:589
  - ✅ `session.remove` — rust crates/oc-plugin-host/src/context/mod.rs:· · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:586
  - ✅ `session.switchAgent` — rust crates/oc-plugin-host/src/context/mod.rs:· · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:587
  - ✅ `session.switchModel` — rust crates/oc-plugin-host/src/context/mod.rs:· · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:588
  - ✅ `session.synthetic` — rust crates/oc-plugin-host/src/context/mod.rs:· · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:593
  - ✅ `session.update` — rust crates/oc-plugin-host/src/context/mod.rs:· · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:595
  - ✅ `session.wait` — rust crates/oc-plugin-host/src/context/mod.rs:· · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:597
  - ✅ `skill.transform` — rust crates/oc-plugin-host/src/supervisor.rs:574 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:456
  - ✅ `storage.get` — rust crates/oc-plugin-host/src/supervisor.rs:615 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:460
  - ✅ `storage.remove` — rust crates/oc-plugin-host/src/supervisor.rs:615 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:462
  - ✅ `storage.scan` — rust crates/oc-plugin-host/src/supervisor.rs:615 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:463
  - ✅ `storage.set` — rust crates/oc-plugin-host/src/supervisor.rs:615 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:461
  - ✅ `tool.transform` — rust crates/oc-plugin-host/src/supervisor.rs:574 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:471
  - ✅ `vcs.base` — rust crates/oc-plugin-host/src/supervisor.rs:635 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:509
  - ✅ `vcs.branch.list` — rust crates/oc-plugin-host/src/supervisor.rs:635 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:511
  - ✅ `vcs.diff` — rust crates/oc-plugin-host/src/supervisor.rs:636 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:514
  - ✅ `vcs.get` — rust crates/oc-plugin-host/src/supervisor.rs:635 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:508
  - ✅ `vcs.reload` — rust crates/oc-plugin-host/src/supervisor.rs:635 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:515
  - ✅ `vcs.status` — rust crates/oc-plugin-host/src/supervisor.rs:635 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:513
  - ✅ `vcs.transform` — rust crates/oc-plugin-host/src/supervisor.rs:583 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:516
  - ✅ `websearch.providers` — rust crates/oc-plugin-host/src/supervisor.rs:628 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:540
  - ✅ `websearch.query` — rust crates/oc-plugin-host/src/supervisor.rs:628 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:541
  - ✅ `websearch.transform` — rust crates/oc-plugin-host/src/supervisor.rs:582 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:543
  - ✅ `worktree.create` — rust crates/oc-plugin-host/src/supervisor.rs:642 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:560
  - ✅ `worktree.list` — rust crates/oc-plugin-host/src/supervisor.rs:642 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:559
  - ✅ `worktree.refresh` — rust crates/oc-plugin-host/src/supervisor.rs:642 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:562
  - ✅ `worktree.remove` — rust crates/oc-plugin-host/src/supervisor.rs:642 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:561
  - ✅ `worktree.transform` — rust crates/oc-plugin-host/src/supervisor.rs:582 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:564

## 归一化规则

- rust 侧：`answer_guest` 匹配臂字符串字面量 + `session.<m>`/`codemode.tool`/`generate.text` 白名单展开；注释剥除（保行号）。
- ref 侧：adapter.ts `context2` 对象字面量的 ns.method 点路径；`rpc` 为运行时构造（rpcFromEffect，adapter.ts:77-162），按声明映射展开：
  - ref `rpc.register` ↔ rust `rpc.register`
  - ref `rpc.call(client)` ↔ rust `rpc.invoke`
  - ref `rpc.events.emit` ↔ rust `rpc.event`
- 范围：**种类级**方法全集对拍；参数/返回值/错误语义（行为级差异）不在本通道范围（G-339/G-340/G-354/G-355 类项由行为通道盯守）。

## Census 裁定

- 已裁定方法：118/118（覆盖率 118/118 = 100.0%：双侧全集逐项归属）
- 结论：REF_ONLY 13 项为缺口候选；RUST_ONLY 29 项中 24 项有登记册背书标注，其余待裁决。

## 验证记录

- 提取复核：全部 REF_ONLY / RUST_ONLY 差异项均亲读双侧当前代码逐项核实（supervisor.rs / context/mod.rs / host.js 与 ref adapter.ts 当前版本）；旧文档仅作定位线索。
- 幂等验证：`python3 tools/census/rpc_methods.py --write /tmp/opencode/rpc-a.md` 连跑两次（rpc-a/rpc-b），`diff` 逐字节一致（/tmp/opencode 下执行）。
- 脚本 exit 0（census 非门禁）。
