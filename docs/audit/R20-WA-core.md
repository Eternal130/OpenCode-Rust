# 第二十轮复核 W-A（注册表/总线/插件/项目）— 2026-11-10，HEAD=967bedba

> 对第十九轮立项的未闭合项在当前 HEAD 重验（先读登记册第十九轮节，按符号重定位刷新行号）。

**G-291 注册键 bare → STILL_OPEN**：`crates/oc-core/src/location.rs:558-559`（insert(tool.name)=bare 键）、`:624/:635`（executor 同键）；MCP 桥 `mcp/registry.rs:1871-1887`（tag_tool :1539-1541 产出 bare 名，namespace 仅字段 :1884 非键）。**新事实**：派发面 `session/runner/model_request.rs:646-647` 注释自称"tool table is keyed by effectiveName (tool.ts:188)"并按 effective_name 建 snapshot——与源表 bare 键实现直接矛盾，修复时应一并解决。②子项 `tool_input_repair.rs:46` 仍 bare 键查询。

**G-292 notify 锁外 → STILL_OPEN**：`bus.rs:699-709`（commit→drop(guard):702→drop(lock):703→notify:708）；publish_all :858-868；replay :1341-1352。模块头 :5-37 的"exactly commit (seq) order"承诺与实现不符。

**G-293 热重载次序+revision → STILL_OPEN**：`supervisor.rs:1958-1988`（clear_plugin:1965+unload:1966-1972 先行，load:1977-1988 后置；失败腿 :2003-2005 仅 eprintln）；②全文件无 revision（generation:1974 仅计数器）。

**G-294 后台过期不重连 → STILL_OPEN**：`mcp/registry.rs:909-957` Changed 仍仅 tools/prompts/resources 三回调+close 腿（:944-956）；typed 判定 client.rs:155 仍仅前台消费（:1236/1341/1392/1415）。

**G-295 events.emit 通路 → STILL_OPEN**：`host/host.js:1462-1493`（:1488-1492 仅返回 {dispose}）；方法表仅 rpc.register（supervisor.rs:422）/rpc.call（lib.rs:878）。

**G-296 RPC schema/error 编解码 → STILL_OPEN**：`host/host.js:726-758`（:741 直调 handler 无 parse/encode；ctx.error 任意 type :735-738 透传 :746-749）；宿主 `lib.rs:876-915` 仅门禁+type 字符串三分（:910-913）；host.js:1467-1469 注释自认仍无一层执行。

**G-297 worktree 事件非原子 → STILL_OPEN**：`project.rs:1354-1389`（存量短路 :1355-1357→insert :1359-1365→publish :1382-1389 走 :1215 `PublishOptions::default()`）。修法基础设施已在：bus.rs:194 支持 `PublishOptions::commit`，属低成本收口。

### 汇总
7/7 STILL_OPEN；c36d2556(fmt) 及其后 clippy 提交均为格式/清理未触及语义面。G-291 修复时须一并消除 model_request.rs:646-647 与源表的键语义矛盾；G-297 有现成 commit 回调基础设施。
