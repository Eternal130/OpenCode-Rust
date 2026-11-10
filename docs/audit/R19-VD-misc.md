# 第十九轮复核 V-D（归类/低优先级）— 2026-11-09

> 对 17 通道审查（docs/audit/L1-L17）产出候选逐项现行源码双侧复核，与登记册 G-1..G-290 去重。

**1. 插件发现模型 — RETRACT（"仅 config 条目"线失实）**
真相：rust 自动扫描目录。`oc-plugin-host/src/resolve.rs:124-172`（`PLUGIN_SOURCE_DIRECTORIES = ["plugin","plugins"]`，`.ts/.js`/子目录/符号链接，按路径排序，Add 先于 config `plugins[]`），与 ref `source-directory.ts`（names 同为 `["plugin","plugins"]`）逐语义对齐。已接线：`oc-server/src/plugins.rs:1055`、`oc-tui/src/plugin_boot.rs:199`。

**2. npm 包启动期自动安装缺失 — NEW-CONFIRM（ℹ️/🟢）**
rust `resolve.rs:19-21` 明言"a spec without an install is a recorded failure, not an install attempt — `opencode plugin add` owns installation"（离线解析缓存 latest_install :28）；ref `plugin/module.ts:88-92` load 时 `install!==false` 即 `npm.add`。同一份 config `plugins:["pkg@x"]` 在 ref 首启即装，在 rust 报 load 失败。登记册未见（G-29 是 TUI 管理页 bounded、G-17② 是反向 rust-extra）。ℹ️ 登记设计决策 + 解析失败 wire error 提示 `opencode plugin add <target>`。

**3. 插件运行时单轨 bun-only — DUP(G-28 族)+补注**
rust `oc-plugin-host/lib.rs:1-7`、`bun.rs:1-13`（缺 bun → 子系统禁用+单条日志；另有 oc_util::bunman 托管钉定 1.3.10 下载兜底）；ref source.bun/node 双实现。归 G-28 备注（node 备轨不移植；缺 bun=禁用+托管下载）。

**4. PluginUpdate.changes() 热流 + outdated/updating 旗标 — DUP(G-29/G-62 族)+补注**
rust `context/update.rs` 仅 24h outdated 缓存（:31,:86-103），无 changes 流；`oc-server/plugins.rs:273-274,319-320` wire `outdated:None, updating:None` 恒空。ref update.ts:31-69 changes PubSub。消费方=TUI 插件管理页（G-29 bounded）+CLI（G-62）。DUP，建议 G-29 补注。

**5. RPC schema 校验/error 编解码下沉 guest 且无一层执行 — NEW-CONFIRM（🟡）**
rust 宿主 `oc-plugin-host/lib.rs:856-921` 仅门禁 id/method；guest `host.js:726-763` handleRpcCall 直接 `handler(input ?? {}, ctx)`，`ctx.error(type,…)` 接受任意 type 字符串（未声明 error 也作 typed failure 透传），无 output encode；host.js:1468-1470 自认"schema validation stays a JS-side concern"但没有任何一层执行。ref `rpc.ts:115-141,200-267` 宿主 parse/encode/encodeError（未声明 error.type 直接 die）/decodeError。G-122 只修 oc-client 定义层。修法：host.js rpc.register 保留 method.input/output/errors JSON-Schema 面，调用前后 parse/encode，未声明 error type 归一 rpc.internal。

**6. repository-cache 刷新时间戳落文件而非 KV — NEW-CONFIRM（ℹ️）**
rust `reference/repository_cache.rs:110-153` FileRefreshStore → `<repos>/.repository-cache.json`（注释自认 mirroring ref shared KV）；ref `repository-cache.ts:157,163` kv.set。仅 ref/rust 二进制共享同一 data 目录时可观察。ℹ️ 登记有界偏离（可挂 G-231 备注）。

**7. shell 超时升级 pid 复用窗口 — DUP（第十七轮 safe_signal 制度）**
`shell.rs:491-512` 走 `signal_process_group`（:809 起）：leader 存活校验→/proc 扫描→逐成员 pgrp 校验——正是登记册 2.4#1 制度实现。理论残余为制度已接受。不新立项。

**8. legacy 扫描 keep 列表/command_parameter — RETRACT**
ref `parse.ts:290-300` PS 与 bash 同一 keep 列表；rust `parse.rs:594-601` bash 分支省 `command_name_expr`，但 bash tree-sitter 语法从不产出该节点，行为等价；`command_elements` 内层循环 :604-617 与 ref :289-295 逐分支一致。

**9. 杂项服务默认值 — 打包**
- ① NEW-CONFIRM 🟢：`shell.rs:571` cwd `unwrap_or_default()`→""；ref `shell.ts:265` `?? location.directory`。调用层已兜底（G-55 HTTP 面），服务 API 语义漂移。
- ② NEW-CONFIRM 🟡（panic 向量）：`shell.rs:564-565` `std::env::vars().collect()` 遇非 UTF-8 环境变量 **panic**（std 文档行为），比丢弃更糟。修法：`vars_os()` + lossy/跳过并记日志。
- ③ NEW-CONFIRM 🟢（trivial）：`shell/select.rs:206-210` `dedup()` 仅相邻去重 vs ref `new Set()` 全局；/etc/shells 实际无重复。
- ④ RETRACT：`pty.rs:576` 显式 (80,24) = node-pty 隐式默认，无分歧。

**10. Anthropic 断点丢弃无告警 — NEW-CONFIRM（🟢）**
rust `anthropic_messages.rs:523,544` 计数 dropped，全文件无 warn/log、body_from_request 不读；ref `anthropic-messages.ts:1060-1064` `dropped>0` logWarning。修法：lower 后一行 warning。

**11. WS chunkTimeout 未透传 — RETRACT**
ref chunkTimeout 仅 HTTP 传输消费（transport/http.ts:126），WS 用独立 idleTimeout；rust 对等：HTTP route/transport.rs:262,391 chunk_timeout，WS websocket.rs:536-567,847-862 idle_timeout。两侧对齐。（"runner/mod.rs" 路径系审查线笔误。）

**12. normalizeImages/truncate 顺序 — RETRACT（候选把 ref 顺序读反）**
ref `session/runner/step.ts:122` truncate 在 tool fiber 内（settle 前），`tool.ts:150` normalizeImages 在 execute.after 内容上——ref 也是先 truncate 后 normalize；rust `step.rs:711-744` 同序且注释明确 matching REF order。〔与 V-A #11 冲突，orchestrator 仲裁中〕

**13. util/artifact.ts 与 util/binary.ts — DUP(G-17 备注)**
artifact 唯一消费方 packages/app（Web TUI 整包不移植族）；binary.ts 无非测试导入方（导出孤岛）。

**14. State 通用运行时仅 agent 域 — DUP(G-5)+补注**
`docs/design-g5-state-transform.md` Divergences #1 已覆盖 lazy rebuild/revision tracking，#2-4 覆盖 group/error/update；ref state.ts:41-114 的 batch/shutdown/notify/reconcile/inherit 是该 lazy+通知模型基建，bounded 端无对应消费。建议 G-5 补一句枚举。

**15. 三处有界超时登记 — DUP-adjacent（ℹ️ 补登记）**
TOOL_JOIN_TIMEOUT=5s（step.rs:43-44）、UNWIND_TIMEOUT=60s（execution.rs:55-56）在 `docs/runner-concurrency.md:180,264,313,419` 有设计依据；登记册仅 G-79（remove 10s）与 UNWIND 顺带提及，TOOL_JOIN 未入册。建议补一条 ℹ️ 打包。

### 汇总
新立项净额：2 🟡（#5 RPC schema 面、#9② env panic）+ 2 🟢（#10 断点告警、#9①③ 打包）+ 2 ℹ️（#2 启动安装、#6 刷新存储位置）；DUP 归并：#3(G-28)/#4(G-29,62)/#7(safe_signal)/#13(G-17)/#14(G-5)/#15(ℹ️ 补)；撤销：#1/#8/#9④/#11/#12（#12 与 V-A 冲突待仲裁）。
