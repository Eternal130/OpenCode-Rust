# Census INDEX —— 缺口普查总索引（14 通道）

> 目标：把"缺口扫描"从自由读码改为**可度量普查**——双侧权威自报对拍 + 生产面提取 + 已知锚交叉验证，覆盖率=已裁定/总数。
> 每轮扫描边际成本 = 14 条幂等命令；上游 bump 时轴 1 类 diff 自动给出变更面清单。
> 生成于 2026-11-12（round-23 修复后基线）。全部报告连跑两次逐字节一致（可复现性已验证）。

## 通道总表

| 通道 | 报告 | 脚本 | 覆盖率 | REF_ONLY | RUST_ONLY（有据） | RUST_ONLY（待登记注记） |
|---|---|---|---|---|---|---|
| HTTP API | HTTP-SURFACE.md | http_surface.py | 142/142 | 0 | 1（plugin/install，G-17 族） | 0 |
| 会话事件 | EVENTS.md | events.py | 51/51 + schema 54/54 | 0 | 0 | 0 |
| pty wire | PTY-WIRE.md | pty_wire.py | 16/16 op + 16 tag + 4 event | 0 | 3（G-10 read/restore、G-383b replay_complete） | 0 |
| CLI | CLI.md | cli_surface.py | 47 cmd + 6 全局 + 62 本地 | 0 | 6（completion=RC-11；5 旗标自证） | 0 |
| 内建工具 | TOOLS.md | tools_registry.py | 18/18（名+input schema） | 0 | 0 | 0 |
| 迁移 | MIGRATIONS.md | migrations.py | 48/48（含顺序） | 0 | 0 | 0 |
| ACP | ACP.md | acp_surface.py | 11/11 + 13/13 | 0 | 0 | 0 |
| 键位动作 | KEYBINDS.md | keybinds.py | 251/251 | 0 | 0 | **8**（composer.close/tab、dialog.select.section、stats.open/close、subagent.toggle-activity） |
| 错误族 | ERRORS.md | errors.py | 33/33 + 3/3 readiness | 0 | 0 | 0 |
| 配置键 | CONFIG-KEYS.md | config_keys.py | 133/133 + 59/59 + 默认值锚全过 | 0 | 3（requireSignature=G-174、$schema、attention.bell=P11） | **1**（experimental.file_retention.cleanup_interval） |
| 环境变量 | ENV-VARS.md | env_vars.py | 179/179 | 59（round-25 复跑：LOG_LEVEL/TERM_PROGRAM_VERSION/TMUX/STY 转 BOTH；已逐项裁定：2 已文档化、3 有据、8 生态资产、8 注册表族、36 ref 独有包胶水、2 其他） | 44（逐项有据；+1 OC_PLUGIN_NODE=G-333 node 备轨产物） | 0 |
| provider 家族 | PROVIDERS.md | providers.py | 55 entrypoint + 226 catalog | 0（家族级） | 3（mantle/messages、xai/chat、anthropic-compatible——round-23 决策族） | 0 |
| provider 配置面 | 同上 | 同上 | env 226/226 · api 200/226（26 双侧皆无） | 0 | — | — |
| 插件 RPC | RPC.md | rpc_methods.py | 112/112 | **14**（桥层读面，见裁定） | 22 | 1（ctx.command.add 扁平化） |
| 插件 ctx API | PLUGIN-API.md | plugin_api.py | 124/124 + 13/13（ref tui 入口面） | **1**（round-25 复跑：transform/reload 已收口，余 rpc.events=G-295 ✅ 提取口径）+ tui 面 13（=G-239/G-331/G-332/G-148② 已登记） | 27 | 0 |
| catalog-only | 同 PROVIDERS | 同上 | 226/226（替代"~180 推定"） | 9（REF_AISDK_ONLY npm 桥，登记册豁免 typed UnsupportedProtocol） | 168 BOTH_COMPAT + 49 BOTH_NATIVE | 0 |

## 总覆盖率

**14 通道全集逐项归属 100%**（双侧并集每项都有 BOTH/REF_ONLY/RUST_ONLY/已登记裁定之一）。
**第二十四轮已入册：新立项 G-384..G-390（7 条：🟢×4 + ℹ️×3）+ G-230 勘注**；已登记项被机械化复认 9 族。
**第二十五轮复跑（2026-11-12，gap-burndown-13 收口后）：全 14 通道重跑**——ENV-VARS REF_ONLY 63→59 / BOTH 72→76；PLUGIN-API REF_ONLY 3→1；TOOLS 轴 3 翻绿（rust 18/18 output 声明+die 分支在位，G-230 盯守闭合）；RPC 新增 integration.reload/method.list rust 面；其余通道零实质变化（CONFIG-KEYS/HTTP-SURFACE 仅锚点漂移）。`tools_registry.py` 提取器随内建 output 新形状更新（轴 3 动态化）。**注：HTTP-SURFACE 的「验证记录」节为手工维护，脚本再生成后需回贴（本轮已回贴）。**

**第二十五轮增补复跑（2026-11-12，amendment lanes H-K 后）：全 14 通道重跑**——ENV-VARS RUST_ONLY 43→44（`OC_PLUGIN_NODE`=G-333 产物，有据）；RPC/PLUGIN-API 仅锚点漂移与新 rust 面（markdown.renderer.*/ui.panel.*），**无新 REF_ONLY**；EVENTS/HTTP-SURFACE 仅锚点漂移（HTTP 验证记录节已再次回贴）。四项增补（panel/markdown 渲染器/node 备轨/mini 样式化）不在 census 枚举轴内（属行为/生态面），由登记册条目钉守。

## 入册裁定（2026-11-12 复核，详见登记册第二十四轮节）

| # | 项 | 证据 | 裁定 |
|---|---|---|---|
| 1 | ctx.integration.transform 的 editor CRUD 面（list/get/update/remove/method.list） | ref adapter.ts:386-400；rust 仅 method.update（=G-24 已修半边） | 🟢 G-384 → ✅ round 25 |
| 2 | ctx.integration.reload guest 直调面 | ref adapter.ts:426；rust 后端 reload_catalog 在（integration/mod.rs:213，models-dev.refreshed 自动路径已接），guest 键缺 | 🟢 G-385 → ✅ round 25 |
| 3 | RPC 桥层 14 项 list/reload 读面 + rust command.add 扁平化 | ref adapter.ts 各处；rust 于 guest 本地实现（行为面等价性归行为通道） | ℹ️ **G-386**（打包登记） |
| 4 | OPENCODE_LOG_LEVEL 运行时日志级别旋钮 | ref util/observability/logging.ts:165；rust 生产零读取 | 🟢 G-387 → ✅ round 25 |
| 5 | TERM_PROGRAM_VERSION 终端版本探测 | ref tui/util/system.ts:17；rust 未读（自有 kitty/ghostty 探测在，口径差待核） | 🟢 G-388 → ✅ round 25（核验=无行为门控，信息面 parity 落地） |
| 6 | TOOLS output_schema die 分支前置面 | registry.rs:185 output_schema:None；ref 14 文件 18/18 声明 | G-230 勘注（0/14→0/18，NOT_YET 延续，census 盯守）→ **round 25 已闭合（18/18+die 在位）** |
| 7 | keybind rust-extra 8 动作 | table.rs:902-917/641-642/828-829；ref Definitions 无 | ℹ️ **G-389**（超集登记） |
| 8 | experimental.file_retention.cleanup_interval 旋钮 | experimental.rs:11；ref 无此键 | ℹ️ **G-390**（超集登记） |

**非缺口裁定**：rpc.events=**G-295**（登记册 STILL_OPEN 在案）；TMUX/STY/NODE_ASSETS_DIR=登记册 :1300 D3-7 有据 bounded；requireSignature=G-174 产物键；pty read/restore/replay_complete=G-10/G-383b；plugin tui 入口面 13 键=G-239/G-331/G-332/G-148②；provider 3 个 rust-native 入口=round-23 决策族（G-373 系）；completion=RC-11。

**rust-extra 已入册**：keybind 8 项=G-389 · file_retention 旋钮=G-390 · ctx.command.add=G-386 内注记。

## 已知锚核对汇总

52 事件→实为 51（旧口径分桶，三轴验证集合一致）· 230 keybind→243（ref 演进，非提取漂移）· 48 迁移 ✅ · models-dev md5 ✅（现算复现）· G-362/G-367 修复落地 ✅（三锚全过）· G-230 die 前置+die 分支 **round-25 已修复**（18/18 声明+die 在位，TOOLS 轴 3 动态报告）· G-10 超集显现 ✅

## 范围外声明（各报告详述）

enterprise/console/desktop 独立部署物 · v1 legacy schema（G-32）· MCP 动态注册 · payload 字段级与时序级语义（归行为/差分通道）· ref 独有包生态胶水 36 env · models.dev 数据驱动 env 注册表（归 providers 通道复核）
