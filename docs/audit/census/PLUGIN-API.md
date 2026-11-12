# Plugin-API Census（缺口普查 · 通道 3：guest 全局 API 面）

> rust guest ctx（host.js makeCtx）× ref plugin SDK ctx（adapter.ts context2）；轴 2 = ref tui 入口面。

> 生成命令：`python3 tools/census/plugin_api.py --write docs/audit/census/PLUGIN-API.md`
> 确定性：静态扫描当前代码，排序输出，无时间戳。交叉验证：oc-plugin-host/src/host/ 桥接实现（supervisor.rs 派发表，见 RPC.md 轴 1）。
> 范围声明：**API 面（命名空间×方法名）种类级对拍**；参数/返回值形状与行为语义不在范围。

## 轴 1：server-plugin ctx 面（host.js `const ctx = {` vs adapter.ts `context2`）

- rust（host.js makeCtx ctx 字面量点路径 + rpc 归一）：126 个面
- ref（adapter.ts context2 点路径 + rpc 归一）：96 个面
- **BOTH：95** · REF_ONLY：**1** · RUST_ONLY：**31**

### REF_ONLY（ref 有、rust 无 → 缺口候选）
  - 🔴 `rpc.events` — /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:77

### RUST_ONLY（rust 额外面 → 登记册背书核对）
  - 🔗 `command.add` — crates/oc-plugin-host/src/host/host.js:1156（待登记册决策）
  - 🔗 `directory` — crates/oc-plugin-host/src/host/host.js:1096（rust 自报面（ref 经 host.location 派生同源值））
  - 🔗 `hostFlavor` — crates/oc-plugin-host/src/host/host.js:1098（rust 自报面（hostFlavor: "v2"））
  - 🔗 `integration.method.list` — crates/oc-plugin-host/src/host/host.js:1725（待登记册决策）
  - 🔗 `integration.method.update` — crates/oc-plugin-host/src/host/host.js:1720（G-24（v2 IntegrationMethodRegistration 面））
  - 🔗 `keymap.layer` — crates/oc-plugin-host/src/host/host.js:2306（PLUG-TUI-SLOT（api.tsx:179-199 keymap.layer 面））
  - 🔗 `location.project.id` — crates/oc-plugin-host/src/host/host.js:1097（rust 自报面（location.project.id））
  - 🔗 `log` — crates/oc-plugin-host/src/host/host.js:2088（rust 桥日志面（console 重路由，ref 无对应））
  - 🔗 `markdown.registerCodeBlockRenderer` — crates/oc-plugin-host/src/host/host.js:2095（G-332（插件 markdown 代码块渲染器注册链缺失））
  - 🔗 `ping` — crates/oc-plugin-host/src/host/host.js:2087（rust 桥活探面（死锁规则配套，ref 无对应））
  - 🔗 `plugin.id` — crates/oc-plugin-host/src/host/host.js:1086（rust 自报面（ctx.plugin.id））
  - 🔗 `rpc.client` — crates/oc-plugin-host/src/host/host.js:1951（bounded deviation（host.js:1666-1669 注记：ref ctx.rpc 本身可调用 → rust rpc.client()））
  - 🔗 `session.list` — crates/oc-plugin-host/src/host/host.js:1250（rust SESSION_METHODS 白名单注记（flagship 探针读面））
  - 🔗 `session.messages` — crates/oc-plugin-host/src/host/host.js:1249（rust SESSION_METHODS 白名单注记（flagship 探针读面））
  - 🔗 `theme.register` — crates/oc-plugin-host/src/host/host.js:1989（wave 1 rust 面（ref v2 ctx 无 theme 域））
  - 🔗 `ui.alert` — crates/oc-plugin-host/src/host/host.js:2123（TUI-25 / G-237 / PLUG-TUI-SLOT（rust TUI 插件 UI 面；ref 无 server-ctx ui 域））
  - 🔗 `ui.confirm` — crates/oc-plugin-host/src/host/host.js:2131（TUI-25 / G-237 / PLUG-TUI-SLOT（rust TUI 插件 UI 面；ref 无 server-ctx ui 域））
  - 🔗 `ui.dialog.open` — crates/oc-plugin-host/src/host/host.js:2196（TUI-25 / G-237 / PLUG-TUI-SLOT（rust TUI 插件 UI 面；ref 无 server-ctx ui 域））
  - 🔗 `ui.dialog.register` — crates/oc-plugin-host/src/host/host.js:2180（TUI-25 / G-237 / PLUG-TUI-SLOT（rust TUI 插件 UI 面；ref 无 server-ctx ui 域））
  - 🔗 `ui.footer.setHint` — crates/oc-plugin-host/src/host/host.js:2172（TUI-25 / G-237 / PLUG-TUI-SLOT（rust TUI 插件 UI 面；ref 无 server-ctx ui 域））
  - 🔗 `ui.grouping.addRule` — crates/oc-plugin-host/src/host/host.js:2202（TUI-25 / G-237 / PLUG-TUI-SLOT（rust TUI 插件 UI 面；ref 无 server-ctx ui 域））
  - 🔗 `ui.home.addQuickAction` — crates/oc-plugin-host/src/host/host.js:2214（TUI-25 / G-237 / PLUG-TUI-SLOT（rust TUI 插件 UI 面；ref 无 server-ctx ui 域））
  - 🔗 `ui.panel` — crates/oc-plugin-host/src/host/host.js:2301（TUI-25 / G-237 / PLUG-TUI-SLOT（rust TUI 插件 UI 面；ref 无 server-ctx ui 域））
  - 🔗 `ui.prompt` — crates/oc-plugin-host/src/host/host.js:2148（TUI-25 / G-237 / PLUG-TUI-SLOT（rust TUI 插件 UI 面；ref 无 server-ctx ui 域））
  - 🔗 `ui.router.current` — crates/oc-plugin-host/src/host/host.js:2292（TUI-25 / G-237 / PLUG-TUI-SLOT（rust TUI 插件 UI 面；ref 无 server-ctx ui 域））
  - 🔗 `ui.router.navigate` — crates/oc-plugin-host/src/host/host.js:2270（TUI-25 / G-237 / PLUG-TUI-SLOT（rust TUI 插件 UI 面；ref 无 server-ctx ui 域））
  - 🔗 `ui.router.register` — crates/oc-plugin-host/src/host/host.js:2254（TUI-25 / G-237 / PLUG-TUI-SLOT（rust TUI 插件 UI 面；ref 无 server-ctx ui 域））
  - 🔗 `ui.sidebar.removeSection` — crates/oc-plugin-host/src/host/host.js:2167（TUI-25 / G-237 / PLUG-TUI-SLOT（rust TUI 插件 UI 面；ref 无 server-ctx ui 域））
  - 🔗 `ui.sidebar.setSection` — crates/oc-plugin-host/src/host/host.js:2159（TUI-25 / G-237 / PLUG-TUI-SLOT（rust TUI 插件 UI 面；ref 无 server-ctx ui 域））
  - 🔗 `ui.slot` — crates/oc-plugin-host/src/host/host.js:2225（TUI-25 / G-237 / PLUG-TUI-SLOT（rust TUI 插件 UI 面；ref 无 server-ctx ui 域））
  - 🔗 `ui.toast.show` — crates/oc-plugin-host/src/host/host.js:2108（TUI-25 / G-237 / PLUG-TUI-SLOT（rust TUI 插件 UI 面；ref 无 server-ctx ui 域））

### BOTH（双侧 file:line）
  - ✅ `agent.get` — rust crates/oc-plugin-host/src/host/host.js:1126 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:296
  - ✅ `agent.list` — rust crates/oc-plugin-host/src/host/host.js:1123 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:297
  - ✅ `agent.reload` — rust crates/oc-plugin-host/src/host/host.js:1120 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:299
  - ✅ `agent.transform` — rust crates/oc-plugin-host/src/host/host.js:1107 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:298
  - ✅ `aisdk.hook` — rust crates/oc-plugin-host/src/host/host.js:1300 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:302
  - ✅ `app.version` — rust crates/oc-plugin-host/src/host/host.js:1095 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:292
  - ✅ `command.list` — rust crates/oc-plugin-host/src/host/host.js:1190 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:308
  - ✅ `command.reload` — rust crates/oc-plugin-host/src/host/host.js:1189 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:322
  - ✅ `command.transform` — rust crates/oc-plugin-host/src/host/host.js:1157 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:309
  - ✅ `event.subscribe` — rust crates/oc-plugin-host/src/host/host.js:2006 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:325
  - ✅ `experimental.terminal.read` — rust crates/oc-plugin-host/src/host/host.js:1499 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:336
  - ✅ `generate.text` — rust crates/oc-plugin-host/src/host/host.js:1268 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:340
  - ✅ `integration.command.cancel` — rust crates/oc-plugin-host/src/host/host.js:1694 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:381
  - ✅ `integration.command.connect` — rust crates/oc-plugin-host/src/host/host.js:1686 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:373
  - ✅ `integration.command.status` — rust crates/oc-plugin-host/src/host/host.js:1690 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:377
  - ✅ `integration.connect.key` — rust crates/oc-plugin-host/src/host/host.js:1662 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:358
  - ✅ `integration.connection.active` — rust crates/oc-plugin-host/src/host/host.js:1700 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:428
  - ✅ `integration.connection.resolve` — rust crates/oc-plugin-host/src/host/host.js:1707 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:429
  - ✅ `integration.connection.status` — rust crates/oc-plugin-host/src/host/host.js:1714 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:430
  - ✅ `integration.get` — rust crates/oc-plugin-host/src/host/host.js:1654 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:356
  - ✅ `integration.list` — rust crates/oc-plugin-host/src/host/host.js:1650 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:355
  - ✅ `integration.oauth.cancel` — rust crates/oc-plugin-host/src/host/host.js:1680 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:370
  - ✅ `integration.oauth.complete` — rust crates/oc-plugin-host/src/host/host.js:1676 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:366
  - ✅ `integration.oauth.connect` — rust crates/oc-plugin-host/src/host/host.js:1668 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:361
  - ✅ `integration.oauth.status` — rust crates/oc-plugin-host/src/host/host.js:1672 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:365
  - ✅ `integration.reload` — rust crates/oc-plugin-host/src/host/host.js:1736 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:426
  - ✅ `integration.transform` — rust crates/oc-plugin-host/src/host/host.js:1746 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:386
  - ✅ `location.directory` — rust crates/oc-plugin-host/src/host/host.js:1097 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:293
  - ✅ `mcp.list` — rust crates/oc-plugin-host/src/host/host.js:1149 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:434
  - ✅ `mcp.reload` — rust crates/oc-plugin-host/src/host/host.js:1146 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:436
  - ✅ `mcp.transform` — rust crates/oc-plugin-host/src/host/host.js:1133 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:435
  - ✅ `model.default` — rust crates/oc-plugin-host/src/host/host.js:1445 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:344
  - ✅ `model.list` — rust crates/oc-plugin-host/src/host/host.js:1437 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:343
  - ✅ `model.reload` — rust crates/oc-plugin-host/src/host/host.js:1449 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:346
  - ✅ `model.transform` — rust crates/oc-plugin-host/src/host/host.js:1454 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:345
  - ✅ `options` — rust crates/oc-plugin-host/src/host/host.js:1102 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:294
  - ✅ `permission.get` — rust crates/oc-plugin-host/src/host/host.js:1283 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:442
  - ✅ `permission.hook` — rust crates/oc-plugin-host/src/host/host.js:1278 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:439
  - ✅ `permission.list` — rust crates/oc-plugin-host/src/host/host.js:1279 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:441
  - ✅ `permission.reply` — rust crates/oc-plugin-host/src/host/host.js:1287 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:443
  - ✅ `plugin.list` — rust crates/oc-plugin-host/src/host/host.js:1090 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:446
  - ✅ `provider.get` — rust crates/oc-plugin-host/src/host/host.js:1410 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:350
  - ✅ `provider.list` — rust crates/oc-plugin-host/src/host/host.js:1406 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:349
  - ✅ `provider.reload` — rust crates/oc-plugin-host/src/host/host.js:1415 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:352
  - ✅ `provider.transform` — rust crates/oc-plugin-host/src/host/host.js:1418 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:351
  - ✅ `reference.list` — rust crates/oc-plugin-host/src/host/host.js:1474 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:449
  - ✅ `reference.reload` — rust crates/oc-plugin-host/src/host/host.js:1478 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:451
  - ✅ `reference.transform` — rust crates/oc-plugin-host/src/host/host.js:1481 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:450
  - ✅ `rpc.call` — rust crates/oc-plugin-host/src/host/host.js:1931 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:77
  - ✅ `rpc.register` — rust crates/oc-plugin-host/src/host/host.js:1876 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:77
  - ✅ `session.command` — rust crates/oc-plugin-host/src/host/host.js:1253 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:591
  - ✅ `session.compact` — rust crates/oc-plugin-host/src/host/host.js:1254 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:592
  - ✅ `session.context` — rust crates/oc-plugin-host/src/host/host.js:1248 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:598
  - ✅ `session.create` — rust crates/oc-plugin-host/src/host/host.js:1245 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:584
  - ✅ `session.generate` — rust crates/oc-plugin-host/src/host/host.js:1252 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:590
  - ✅ `session.get` — rust crates/oc-plugin-host/src/host/host.js:1246 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:585
  - ✅ `session.hook` — rust crates/oc-plugin-host/src/host/host.js:1244 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:580
  - ✅ `session.interrupt` — rust crates/oc-plugin-host/src/host/host.js:1247 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:594
  - ✅ `session.move` — rust crates/oc-plugin-host/src/host/host.js:1258 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:596
  - ✅ `session.prompt` — rust crates/oc-plugin-host/src/host/host.js:1251 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:589
  - ✅ `session.remove` — rust crates/oc-plugin-host/src/host/host.js:1257 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:586
  - ✅ `session.switchAgent` — rust crates/oc-plugin-host/src/host/host.js:1260 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:587
  - ✅ `session.switchModel` — rust crates/oc-plugin-host/src/host/host.js:1261 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:588
  - ✅ `session.synthetic` — rust crates/oc-plugin-host/src/host/host.js:1255 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:593
  - ✅ `session.update` — rust crates/oc-plugin-host/src/host/host.js:1256 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:595
  - ✅ `session.wait` — rust crates/oc-plugin-host/src/host/host.js:1259 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:597
  - ✅ `shell.hook` — rust crates/oc-plugin-host/src/host/host.js:1294 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:601
  - ✅ `skill.list` — rust crates/oc-plugin-host/src/host/host.js:1213 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:455
  - ✅ `skill.reload` — rust crates/oc-plugin-host/src/host/host.js:1210 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:457
  - ✅ `skill.transform` — rust crates/oc-plugin-host/src/host/host.js:1197 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:456
  - ✅ `storage.get` — rust crates/oc-plugin-host/src/host/host.js:1964 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:460
  - ✅ `storage.remove` — rust crates/oc-plugin-host/src/host/host.js:1971 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:462
  - ✅ `storage.scan` — rust crates/oc-plugin-host/src/host/host.js:1974 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:463
  - ✅ `storage.set` — rust crates/oc-plugin-host/src/host/host.js:1968 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:461
  - ✅ `tool.hook` — rust crates/oc-plugin-host/src/host/host.js:1239 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:504
  - ✅ `tool.list` — rust crates/oc-plugin-host/src/host/host.js:1236 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:467
  - ✅ `tool.reload` — rust crates/oc-plugin-host/src/host/host.js:1233 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:466
  - ✅ `tool.transform` — rust crates/oc-plugin-host/src/host/host.js:1220 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:471
  - ✅ `vcs.base` — rust crates/oc-plugin-host/src/host/host.js:1311 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:509
  - ✅ `vcs.branch` — rust crates/oc-plugin-host/src/host/host.js:1320 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:511
  - ✅ `vcs.diff` — rust crates/oc-plugin-host/src/host/host.js:1335 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:514
  - ✅ `vcs.get` — rust crates/oc-plugin-host/src/host/host.js:1307 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:508
  - ✅ `vcs.reload` — rust crates/oc-plugin-host/src/host/host.js:1315 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:515
  - ✅ `vcs.status` — rust crates/oc-plugin-host/src/host/host.js:1331 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:513
  - ✅ `vcs.transform` — rust crates/oc-plugin-host/src/host/host.js:1347 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:516
  - ✅ `websearch.providers` — rust crates/oc-plugin-host/src/host/host.js:1517 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:540
  - ✅ `websearch.query` — rust crates/oc-plugin-host/src/host/host.js:1521 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:541
  - ✅ `websearch.reload` — rust crates/oc-plugin-host/src/host/host.js:1525 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:542
  - ✅ `websearch.transform` — rust crates/oc-plugin-host/src/host/host.js:1528 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:543
  - ✅ `worktree.create` — rust crates/oc-plugin-host/src/host/host.js:1594 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:560
  - ✅ `worktree.list` — rust crates/oc-plugin-host/src/host/host.js:1586 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:559
  - ✅ `worktree.refresh` — rust crates/oc-plugin-host/src/host/host.js:1601 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:562
  - ✅ `worktree.reload` — rust crates/oc-plugin-host/src/host/host.js:1580 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:563
  - ✅ `worktree.remove` — rust crates/oc-plugin-host/src/host/host.js:1598 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:561
  - ✅ `worktree.transform` — rust crates/oc-plugin-host/src/host/host.js:1605 · ref /opt/opencode-ref/packages/plugin/src/promise/adapter.ts:564

### 已知锚点核对（登记册背书逐项）

- G-379.x vcs 读面（get/base/branch/status/diff）与 worktree 读面（list/create/remove/refresh）：期望 BOTH —— 实测 **BOTH ✅**（ctx 层双侧同名面，branch 形状差异经 REF_CTX_MAP 归一）
- G-239 panel/PanelHost：ref 的 panel 面在 **tui 入口**（轴 2 `ui` 键的 panel 子面），server-ctx 无 panel 域 → 轴 1 不显现，轴 2 显现为 REF_ONLY ✅
- G-332 markdown 渲染器：ref 的 `markdown.registerCodeBlockRenderer` 在 **tui 入口** → 轴 2 REF_ONLY（markdown 键）✅

## 轴 2：ref TUI-plugin 入口面（packages/plugin/src/tui/context.ts 顶层键 vs rust）

- ref tui Context 顶层键：13 个；rust 无 tui 入口（host.js 无对应命名空间）→ 全部 REF_ONLY
- 登记册背书：整个 tui 入口为 G-28 族声明的生态破坏性变更（G-148② effect API 不移植的同族；solid.js TUI 插件面不在 rust 移植范围）
  - 🔴 REF_ONLY `app` — /opt/opencode-ref/packages/plugin/src/tui/context.ts:547（App 信息面（name/version/channel））
  - 🔴 REF_ONLY `attention` — /opt/opencode-ref/packages/plugin/src/tui/context.ts:551（注意力面（toast/sound/notification））
  - 🔴 REF_ONLY `client` — /opt/opencode-ref/packages/plugin/src/tui/context.ts:549（OpenCode client 面（effect 客户端直通）——G-148② 同族（effect API 面不移植））
  - 🔴 REF_ONLY `data` — /opt/opencode-ref/packages/plugin/src/tui/context.ts:550（响应式数据面（on/listen/session/project/permission/shell/form 集合流））
  - 🔴 REF_ONLY `keymap` — /opt/opencode-ref/packages/plugin/src/tui/context.ts:557（Keymap 面（tui 入口形态））
  - 🔴 REF_ONLY `location` — /opt/opencode-ref/packages/plugin/src/tui/context.ts:546（location 引用面（vcs/agent/command/… 集合的入口））
  - 🔴 REF_ONLY `markdown` — /opt/opencode-ref/packages/plugin/src/tui/context.ts:554（markdown.registerCodeBlockRenderer —— **G-332 直接载体**（插件 markdown 代码块渲染器注册链缺失，登记册 2026-11-10 转正入册））
  - 🔴 REF_ONLY `options` — /opt/opencode-ref/packages/plugin/src/tui/context.ts:545（插件 options 注入面）
  - 🔴 REF_ONLY `renderer` — /opt/opencode-ref/packages/plugin/src/tui/context.ts:548（CLI 渲染器面（solid.js 渲染根）——G-28 族生态偏离（solid TUI 面不移植））
  - 🔴 REF_ONLY `storage` — /opt/opencode-ref/packages/plugin/src/tui/context.ts:558（tui 入口 Storage 面（get/set/remove/scan 响应式形态））
  - 🔴 REF_ONLY `theme` — /opt/opencode-ref/packages/plugin/src/tui/context.ts:552（ResolvedTheme 面（tui 入口形态；rust server-ctx 的 theme.register 为另一形态））
  - 🔴 REF_ONLY `themeMode` — /opt/opencode-ref/packages/plugin/src/tui/context.ts:553（主题模式面）
  - 🔴 REF_ONLY `ui` — /opt/opencode-ref/packages/plugin/src/tui/context.ts:559（UI 面（slot/panel/tabs/router/toast/…；**panel 即 G-239/G-331 未挂载面**，plugin_slots.rs:46 明列 unmounted，维持搁置））

## 归一化规则

- 双侧同一对象字面量遍历器（ident/引号键 + 嵌套对象递归，深度 ≤3）取 ns.method 点路径。
- rpc 域双侧运行时构造：rust 面 = register/call/client（host.js:1594-1679）；ref 面 = register/call(每定义一方法)/events（adapter.ts:77-162 rpcFromEffect）——client 形态差异（ref 的 ctx.rpc 本身可调用 vs rust 的 rpc.client()）为已注记的 bounded deviation（host.js:1666-1669 注释）。
- ctx 非方法键（app/directory/location/options/hostFlavor/plugin.id 等值字段）同样计入面全集（双侧均有）。

## Census 裁定

- 轴 1 已裁定：127/127（覆盖率 127/127 = 100.0%：双侧全集逐项归属）；轴 2 已裁定：13/13（全部 REF_ONLY，逐项登记册背书标注）。
- 结论：轴 1 REF_ONLY 1 项为缺口候选；RUST_ONLY 31 项中 29 项有登记册背书；轴 2 13 项 REF_ONLY 均落 G-28 族声明范围（G-148②/G-239/G-331/G-332）。

## 验证记录

- 提取复核：轴 1 全部 BOTH/REF_ONLY/RUST_ONLY 项与轴 2 全部 13 键均亲读双侧当前代码核实（host.js makeCtx / supervisor.rs 派发表交叉验证 + ref adapter.ts context2 / tui/context.ts Context 接口）；登记册锚（G-239/G-331/G-332/G-379.x/G-148②/G-28）经 docs/GAPS-AND-PITFALLS.md 当前条目复核（G-239 维持 ℹ️ 搁置、plugin_slots.rs:46-48 注引现场确认；G-332 2026-11-10 转正在册）。
- 幂等验证：`python3 tools/census/plugin_api.py --write /tmp/opencode/plug-a.md` 连跑两次，`diff` 逐字节一致（/tmp/opencode 下执行）。
- 脚本 exit 0（census 非门禁）。
