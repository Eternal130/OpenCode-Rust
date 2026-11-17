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

---

## 紧凑评审（Oracle，2026-11-17，基 b2750339..b02496ba）

**Spec ✅**（六场景双端对拍、G-431b 修复、G-432 双点修复、harness ①②③加固均有代码与证据支撑；逐项核验如下）

核验通过项：
- **G-431b**：`SchemaIdGen`（determinism.rs:77-107）`msg`→AscendingIdGen（Mutex<(ts,counter)>，ts<<12+counter，线程安全），形状单测钉 26 位/12 hex/62 表；`EngineOptions::new` 换入覆盖 engine/jobs 全部 `new_id("msg")` 调用点；jobs.rs:474 双前缀顺带修正确。
- **G-432 ref 依据属实**：`/opt/opencode-ref/packages/core/src/session/session.ts:96-107` switchModel 对未变选择（providerID/id 相等 + variant `?? "default"` 归一）早退不 publish；protocol `groups/session.ts:225` create payload `model: Model.Ref.pipe(Schema.optional)`——rust 侧 create 确实持久化 model（handlers/session.rs:440），TUI create 带 model（app.rs:5040-5050）后 switchModel 落入 no-op 分支，链路自洽。variant 归一语义（`variant_of` 把 None/`"default"` 视为等价）与 ref `?? "default"` 等价。wire 键名（providerID/id/variant）与 ModelRef serde rename 一致。
- **对拍真实性**：ref 侧真起完整生产链（bun→CLI→selfCommand 自起 serve --service，staging 合同逐字取证）；键序 write_both 双端同一字节序；主等待全部条件式（wait_for needle），网关双 corpus + 双 root redact（ref root 先于 rust root，前缀扩展顺序正确）。
- **freshness 守卫**：mtime 比对 + 2s 宽限，排除 tests/benches/target（否则 harness 自身会恒触发）；失效面如实（仅 crates/**/*.rs + 清单 + lock；并行写 bin 无保护——可接受的启发式，两度实拦佐证有效）。
- **违规自曝**：worktree/Cargo.lock/测试文件属主抽查均 eternal130（漂移已修）；`core.hooksPath` 未设置（unset），W2 遗留"重打钩子是否真跑不可考"如实登记未夸大。

### 发现

**Critical**：无。

**Important**
1. **G-436/G-437 掩码无退役绊线（掩码永久化风险=假绿面）**：`_rust_palette_stale`（realchain_pair.rs:387）计算后从未断言（下划线绑定即弃）；G-436 `drop_blank_rows` 无"ref 侧确多一行边框"存在性断言。修后（G-437 palette 刷新、G-436 盒高对齐）掩码将静默留存且测试仍绿——与 G-433/434/435 的钉形状做法（S5 钉 ref `0% used`、S3 钉免费层存在）不一致。建议：G-437 断言 `rust_palette_stale` 为真（修后翻转逼撤掩码），G-436 加 ref/rust 空行计数差断言。
2. **pair lane 硬环境依赖无 skip 守卫**：`ref_stage` 对 `/opt/opencode-ref`、`/opt/opencode-rust/tools/bin/bun-1.4.2`（未跟踪文件）直接 assert panic（realchain_harness.rs:806-820），4 个 pair 测试在缺 ref 树的 CI/新机器必红——与同仓 bun 测试的 "bun unavailable; skipping" 惯例相悖。T9_REF_ROOT/T9_BUN 可覆写但 CI 不可达性未解决。建议 missing→eprintln skip。

**Minor**
3. G-433 `(N%)` 掩码（realchain_harness.rs `replace_percent_group`）全局施用于所有场景所有行，非仅 context footer——任意真实 percent 差异也会被吞。建议限定已知 footer 形状（如 `NN tokens (N%) used` 邻域）。
4. G-432 无 API 级回归测试：`handlers_integration.rs:497` 只测换模型路径，unchanged→204/无 model-selected 行未直接钉；目前仅 S2 端到端兜底，而 S2 与 Important-2 联动（缺 ref 树即不可达）。
5. `mask_right_columns(108)`（S5）列界硬编码；`replace_id_shapes` 前缀集含 `frm`/`per` 偏宽（任意 `per_xxxxxxxx` token 会被吞）。
6. `settle()` 为纯 sleep 余量（realchain_pair.rs:75）——主等待条件式所以可接受，过渡帧残余风险低但非零。

**Approved**（Important 1/2 建议随 W4 收口：1 是本通道"不许静默放宽"纪律的最后一环，2 决定 pair lane 能否进 CI）。
