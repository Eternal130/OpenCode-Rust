# 与 ref 的已知分歧（Known Divergences）——opencode-rust 移植登记册

> **说明（2026-11-22 用户裁定）**：本册为公开的已知分歧登记（原 43 轮判定史精简拆分而来）。全部历史——逐轮判定、G-1..G-464 已修项 forensics、事故记录、清理注记——完整判定史不随公开仓分发（2026-11-22 隐私清理已历史重写）；全文副本=本地备份与仓外归档（不入库）。现役分歧唯一活登记=tools/diff/realchain/adjudications.json（裁决注册表）；开发规范见本地私有册（git-ignored，不入克隆）。

## 一、已知与 ref 的分歧（截至 2026-11-22）

### 现状

- 在册未清偿缺口（🟡/❌）：**零**——G-1..G-464 全部收口（末批 2026-11-21/22：G-428 reasoning 定位、G-430 animations 六面、G-93 $EDITOR 附件、G-464 skill 附件、G-94/95/96/400 服务端 API 对齐、G-439 托管文案、空流残余）。
- realchain 裁决注册表：`tools/diff/realchain/adjudications.json`（10 条全良性：framework-diff×6 / expected-shape×3 / env-artifact×1；exit 语义与检查器见 `tools/diff/realchain/adjudicate.py`）。
- 真机观察挂起（1 项）：busy 呼吸灯症状⑦''②——树内未复现、功能在、帧序列钉死；等用户复现材料（截图 + OC_TUI_DEBUG=1 trace）。

### 文档化决策族（31 条 ℹ️——与 ref 的有意分歧，非缺口）

（原 34 条：G-211 split-footer 经 ADR-0019 已全量移植翻 ✅ 出族、G-328/G-329 已复审翻 ✅ 出族——见「已收口登记」。）

**① 有意不移植（6）**

| G号 | 一句话分歧 | 裁决出处 |
|---|---|---|
| G-32 | v1/legacy schema 族（legacy-event、v1 session 事件、rpc.ts、pty-ticket、durable-event-manifest、run/v1 兼容层）有意不移植——仅影响旧数据互操作/回放，纯 v2 面无影响 | 第七轮收口裁决（文档化决策） |
| G-163 | workerd 运行时目标与 ServerFetch.make 不移植（无 JS 运行时嵌入目标）——server 终态为原生 boot_and_serve(axum) | 第十一轮登记收口（G-17 族） |
| G-212 | TUI storybook/测试环境变量面不移植（~1400 行开发期 fixture） | Gate D3 裁决 4 |
| G-238 | heap 快照(SIGUSR1)/CPU profile(SIGPROF) 不移植——V8 inspector 平台性；native 替代路径 pprof/tokio-console | 第十五轮 ℹ️ 登记（G-17 族体例） |
| G-313 | 插件 npm 包启动期自动安装不移植——安装权归 `opencode plugin add`；解析失败 wire 提示已落地 | 第十七轮登记；2026-11-22 复审勘注 |
| G-38 | OTel feature 非 default（ref 默认开），发布构建不拉遥测依赖 | 第三轮 ℹ️（发布决策） |

**② 有意纠偏/超集（9）**

| G号 | 一句话分歧 | 裁决出处 |
|---|---|---|
| G-403 | HTTP session create 坏 location：rust 404 拒绝 vs ref 200+悬挂会话（ref 侧可疑行为）；客户端按 404 处理 | 第二十八轮裁决=有意纠偏 |
| G-407 | ref 生产校验器把 pattern/结构化 const 编译错洗成 accept-all（生产根本不校验），rust 忠实实现有意超严；依赖 ref 宽松性的 schema 会被 rust 拒 | 第二十七轮 JSON-SCHEMA 裁决 |
| G-314 | custom/local/console provider 名大小写不敏感兜底（exact-first + eq_ignore_ascii_case）——纯宽容度差，多收配置无失败面 | 第十七轮登记 |
| G-316 | ssh-askpass prompt 取 argv[1] 符合 OpenSSH 约定——ref 编译为单文件二进制后 slice(2) 取空串系 ref 侧真实缺陷 | 第十七轮有意纠偏 |
| G-317 | 有界超时制度：session.remove 10s / TOOL_JOIN 5s / UNWIND 60s——ref 无界等待在 wedge 场景由 rust 有界收口 | 第十七轮打包登记 |
| G-389 | keybind rust-extra 8 动作超集（composer.close/tab.next/prev、subagent.toggle-activity、dialog.select.section.next/prev、stats.open/close）——ref 无此可绑定键 | 第二十四轮 census KEYBINDS 登记 |
| G-390 | `experimental.file_retention.cleanup_interval` rust 独有加固旋钮（超集键） | 第二十四轮 census CONFIG-KEYS 登记 |
| G-318 | repository-cache 刷新时间戳落文件 `<repos>/.repository-cache.json` 而非 KV——仅 ref/rust 二进制共享同一 data 目录时可观察 | 第十七轮有界偏离登记 |
| G-240 | opencode-pty 二进制免 sha256 钉扎（rust 无捆绑安装路径，无可比面）；OPENCODE_PTY_BIN override 属主/拒符号链接加固已落地 | RW-T2 有意取舍；gb9 lane5 加固 |

**③ 有界 shim/架构替代（6）**

| G号 | 一句话分歧 | 裁决出处 |
|---|---|---|
| G-28 | 插件 TSX 渲染为有界 shim——渲染树跨 IPC 拍平为 `{title,rows,clickable}` 行形状；markdown 渲染器注册链与 panel slot 已落地，生态声明面收窄为有界行形状+node 轨加载限制 | 第三轮起多轮；2026-11-12/22 收窄勘注 |
| G-289 | 全进程单 bun guest vs ref 每 Location 独立插件实例与作用域——多 location 并存时插件模块级状态共享；per-Location 隔离改造为后续工程项非缺口 | 第十八轮裁决=登记不改码 |
| G-386 | 插件 RPC/ctx 桥形状差：13 项读面 guest 本地实现（agent/command/mcp/skill/tool 等 list/reload）+ ctx.command.add / integration 扁平化直调超集（RPC RUST_ONLY 25 键）——无行为缺口 | 第二十四轮 census；二十五轮复扫补注 |
| G-164 | announce.rs 为 location 事件级联的服务端替身（project.updated 60s 限速、空帧复刻、vcs.branch.updated 订阅级联）——新增 location 服务/事件须同步维护级联清单并保 parity 金样 | 第十一轮登记收口；G-218 后前提订正 |
| G-326 | session.diff 注册为公开 ephemeral 但全仓无发布点（SSE 实际不转发，与 ref 暴露面一致）——盯守条件：若未来开始发布必须先并入 PRIVATE_TYPES 防工作区 diff 泄露 | 第十二轮归类；条件性盯守位 |
| G-39 | 剪贴板写仅 OSC52（读臂健全 pbpaste/xclip/xsel/wl-paste）——不支持 OSC52 的终端复制静默失败 | 第三轮 ℹ️ 有意 |

**④ 纯记录性（10）**

| G号 | 一句话分歧 | 裁决出处 |
|---|---|---|
| G-12 | `context_management.edits[].type` 硬编码 `"compact_20260112"`（入站同时 fail-fast 校验，非静默丢弃）；新增 edit 类型需手改两处 | 第三轮 ℹ️ |
| G-16 | 手写 serde 与 ref Effect Schema 平行维护靠 goldens 防漂（最大长期漂移面）；provider 折叠需双处改；openapi.json sha256 钉死需人工 baseline-bump——**厂商清单自动对照测试未建（族内唯一真待办）** | 第三轮 ℹ️ 维护坑 |
| G-17 | 有意省略：ref 死代码（mcp/resolve.ts、plugin/inventory.ts）、web/desktop/enterprise/cloud 整包不移植、rust-extra 防御性超集（upgrade -m、completion、--force 等）均有注释 | 第三轮 ℹ️ 正确决策 |
| G-40 | 记录性 wire 差异：`x-opencode-build` 响应头（ref 无）、空分页 cursor 显式 null 而非省略键——golden 对比归一化即可 | 第三轮 ℹ️ |
| G-290 | 对话框 ctrl+c 一次即清空并关闭（ref 首按仅清空保持打开）——防吞 ctrl+c×N 退出链致 90s wedge 事故的有意决策 | 第十八轮入册（G-226 体例） |
| G-315 | devtools 默认开缺 `channel==="local"` 分支（rust 无 channel 概念）——仅开发者默认态差异，不影响发行用户 | 第十七轮登记 |
| G-408 | 数字/相等语义记录族：integer ≥2^53 ×3（effect 安全整数域 vs u64/i64 全域）、`5.0` integer、enum `1.0`、uniqueItems 结构相等 vs 引用恒等 ×2——JS 数字模型固有差，无行动面 | 第二十七轮 JSON-SCHEMA |
| G-409 | SSE 投影架构差 4 族：无锚 delta 建件+乱序补位（rust 超集）、text.started 惰性建件（瞬态差终态收敛）、replay 去重层位（Store 内 vs app SSE 层）、session.moved 纯事件流门控 | 第二十七轮 SSE-PROJECTION 成因族 |
| G-410 | HTTP wire 记录性 5 项：cost `0.0` vs `0`、session 多 slug/subpath 超集键、project/skill 缺省键 null vs 省略、错误 message 缺 offender 后缀、integration 目录 224 vs 229（成员新鲜度差） | 第二十七轮 HTTP-FULL |
| G-411 | ACP 有据形状差 3 项：pending tool_call 恒置 name+缺省 status/locations（vendored patch 域）、异常 ordinal `unwrap_or(0)` vs ref 模板串、缺 delta 字段 `""` 上 wire vs ref 丢键 | 第二十七轮 ACP-TRANSLATE B1-B3 |

### 已收口登记（摘要）

- G-1..G-464 修法/取证全文：完整判定史不随公开仓分发（2026-11-22 隐私清理已历史重写）；全文副本=本地备份与仓外归档（不入库）。
- 出族近期翻正：G-211 split-footer 整族经 ADR-0019 全量移植并消费；G-328 TAB_SPINNERS 四样式全量落地；G-329 mini demo 剧本 13/13（2026-11-22 复审翻正，第五次登记滞后实例）。
- 覆盖台账（原 docs/audit/diff/COVERAGE.md，12 族，四轮审收敛）与差分台账族（JSON-SCHEMA/HTTP-FULL/TUI-FULLSCREEN/ACP-TRANSLATE 等 11 本）：随本批出树，完整判定史不随公开仓分发（2026-11-22 隐私清理已历史重写）；**覆盖完备性裁定（§2.2#13）持续有效**——新增可测面发现时（用户报告/代码审读/上游 diff）从 git 历史复活台账或重建。
- 观察名单（活）：`pty_composer_cycle_type_edit_submit_reply_stats_solid_path`（一见）、`pty_wheel_after_real_terminal_probe_replies`（一见）、W38 settle 竞态第四臂已落（再现即升级）、`tl_tool_loop_pair`（两见）。flake 白名单见 §2.2#4。


## 指针

- 开发规范与现行制度速查 → 本地私有册 `docs/GAPS-AND-PITFALLS.md`（git-ignored；规范权威源=tools/lane-brief.sh 逐字抽取，见 `AGENTS.md`）
- 架构对比 → `ARCHITECTURE-VS-REF.md`
- 历史全文（43 轮判定、G-1..G-464 forensics）→ 本地备份与仓外归档（不入库；完整判定史不随公开仓分发，2026-11-22 隐私清理已历史重写）
