# 通道 9 W3 报告 — ref 侧真实链路对拍 + G-431b + 断言加固

日期：2026-11-17 · worktree `beta15-field` · 基线 HEAD=b2750339（W2）

## ref boot 拓扑（一行）

`bun /opt/opencode-ref/packages/cli/src/index.ts`（default handler）→ ServerConnection.resolve → ServiceConfig.options → selfCommand() 自起托管后台 `bun <entry> serve --service`（注册 xdg-state/opencode/service-local.json）；staging 合同：运行时拷贝必须名 `bun`、solid preload 经 cwd bunfig.toml 传子进程、配置在 `$XDG_CONFIG_HOME/opencode/`（opencode.json=服务端 provider、cli.json=TUI 自身+端口）、channel-local 托管端口固定 0xc0df 需沙箱钉、devtools 条经 cli.json `{"devtools":false}` 取发布形状。

## 六场景对拍（同一网关、同键序、归一化 grid 比对）

- S1 boot 到 wordmark：**EXACT**
- S2 发消息→回复渲染（同网关各端语料）：**EXACT**（title 与 needle 分离后双双渲染 "Sandbox Replay Session"）
- S3 模型 palette（过滤视图，⑥升真 grid 断言、期望取 ref 实跑）：**EXACT**（免费层 G-434 存在性断言）
- S4 会话持久重启（tab 标题存活）：**EXACT**
- S5 侧栏 160 列（T9-5 升格双端）：**EXACT**（侧栏列区 G-435/G-436 掩码；ref 实跑形状 Context/38 tokens/0% used 钉死）
- S6 配置热载后 palette：**EXACT**（G-437 行掩码；ref palette 反映、rust API 级反映钉死）

## G-431b 处置

🟢 关闭：`oc_util::determinism::SchemaIdGen`（`msg`→AscendingIdGen=ref `SessionMessage.ID.create()` 形状，其余 uuid 兜底）换入 `EngineOptions::new`；jobs.rs:474 `msg_msg_` 双前缀顺带修正；单测钉 26 位/12 hex/62 表。W2 评审 Minor（msg_plugin_/cred_）登记未动。

## 对拍分歧归因（不许静默放宽——全部 G 标注在代码归一化处+GAPS）

- G-432 已修（switchModel 无 no-op → 首回合幻影 Switched model 行；服务端 no-op + TUI create 带 model 双点修复）
- G-433 自定义 provider 模型基座（ref limit.context=200000 vs 0 → footer `(0%)`）
- G-434 models.dev 目录 vintage（免费层列表代差）
- G-435 rust 侧栏 Context 块 rig 依赖（t9-5 单端渲染 / pair 不渲染，ref 两态都渲染）
- G-436 宽档用户消息盒高度差 1 行
- G-437 rust TUI palette 不反映热载模型（服务器 API 反映）

## harness 加固清单

①freshness 守卫（bin mtime vs crates 源，本 wave 两度实际拦截）；②needle/title 分离（网关 `--seed-title` + title-sse 种子 + 45e2 重铸）；③矩阵标注更新（①③⑥ 真机环境面待复测维持、⑥升 grid、⑤升双端——GAPS 第三十六轮）。

## 验证

- T9 全量 `tools/dev-gate.sh 'cargo nextest run -p oc-tui --test realchain_t9 --test realchain_pair --locked --no-fail-fast'`：**15/15 绿**
- oc-server 411/411；oc-tui 1725（e1_cells_paste_8k 计时 flaky 重跑绿）；oc-core+oc-util+oc-cli 2358/2358
- 通道 8 零漂移（exit 0，TUI_DIFF_BUN=/opt/opencode-rust/tools/bin/bun-1.4.2）
- clippy workspace all-targets / cargo fmt --check / guards run.sh 全绿
- 树 hash 与 commits 见下

## Concerns

- G-433/434/435/436/437 均为登记待修面（归一化处 G 标注掩码，双侧对称，修后应撤）
- ref lane 依赖 /opt/opencode-ref 树 + 主 checkout 的 tools/bin/bun-1.4.2（T9_REF_ROOT/T9_BUN 可覆写）
- .test-logs 曾现 t9-pair-* dump 被外因清除（未定位，gateway 日志幸存；终跑日志落 /tmp/opencode/w3-*.log 并同步 .test-logs）
- 本 wave root 写文件造成 worktree/.git-worktrees 属主漂移 → 已 chown 回 eternal130（guards recorded_local 恢复绿）；后续会话注意以 eternal130 身份写文件
- e1_cells_paste_8k 计时型 flaky（E1 stall-bursts 阈值 2 vs 噪声底 1），与本 wave 改动无关（重跑绿）
