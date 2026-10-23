# 缺口与踩坑登记册（GAPS & PITFALLS）

> 建立：2026-10-23 ｜ 来源：ROADMAP 变更日志、FINDINGS.md、PORT_REPORT.md、docs/host-issues.md、移植进度审计（2026-10-23）
> 用途：§1 是**当前仍然开放**的缺口（ID 与 ROADMAP.md 台账一致，此处为汇总视图）；§2 是开发全程踩过的坑，每条按「事故 → 根因 → 现行规则」沉淀；§3 是现行防复发制度速查。
> 原则：坑的价值在于结构化修复——本册只收录已转化为规则或已明确登记的教训，不收纯流水账。

---

## 1. 当前缺口（截至 2026-10-23，beta.13 + M-engine-retire 之后）

### 1.1 功能缺口

| ID | 缺口 | 状态 | 说明 |
|---|---|---|---|
| R-10 | rmcp 0.5 elicitation 能力 | **blocked（上游）** | 逻辑已备，等 rmcp 上游补 elicitation |
| R-11 | 平台矩阵（aarch64/musl/macOS/Windows） | 待用户排期 | 非功能性缺口，独立决策 |
| R-04 | Web UI 前端本体 | 范围外（裁定） | 静态托管 + 发布资产内嵌已完成；前端不移植 |
| — | PTY 守护进程边界（Phase 9） | 延后 | 状态机/handoff/adopt 已备（beta.7）；socket/child keepalive 进程边界未做（`oc-core/src/persistent_pty.rs:7`） |
| — | 企业集成 RS256/JWKS 签名校验 | 未实现 | 无 JWT 库；`oc-core/src/integration/providers_enterprise.rs:1255`，依据 docs/be3b.log |
| — | Skill URL 源下载通道 | 延后 | `oc-core/src/engine/skills.rs:294` |
| — | console capability/cost/limit 合并 | 延后 | `oc-core/src/integration/console.rs:211` |
| — | oc-client sse 测试时序敏感 | 登记跟进 | 33 次未复现、已特征化并引用代码；硬化项排队中 |

### 1.2 行为分歧（待裁定 / 登记未修）

| ID | 分歧 | 处置状态 |
|---|---|---|
| F15 | TUI 窄宽度换行 | 登记未修——需 option-row 高度模型重构，非快赢项 |
| E12-F1 | 会话列表搜索：子会话泄漏 | **已裁定非分歧（2026-10-23 live ref 复核）**：live v2.0.24 从不渲染子会话行（服务端 `parentID:null` + `options()` 再过滤）；首轮 F1 系裸谓词孤立测试的伪影。证据 docs/e12-live-ref-recheck/（12/12 查询 parity） |
| E12-F6 | 空白查询返回空 | **已裁定：裁剪后空查询 = 全量列表**（live ref 与本port一致返回全部父会话行）；谓词层保留子会话但渲染层永不含子行。与我们 `filter_indices` 语义一致，无需改码 |
| E12-F4 | fuzzy picker vs 专用 substring 过滤 | **已裁定：substring（live ref 无 fuzzy：`zln`/`loginredirect` 均无结果）**；本port `session.list` 已挂 `.substring_filter()`——语义正确，闭环 |
| E12-F2 | 重音等价 | **已裁定：无重音折叠**（`cafe`↔`café` 永不互配；`café` NFC 查询只配 NFC 标题）——live ref 与本port byte-exact 一致，维持 pinned 语义 |
| E12-F3 | NFD | **已裁定：双方均不归一化**（NFD 查询对 NFC 标题零匹配），维持 pinned 语义 |
| E12-F5 | 空白折叠 | **已裁定：无空白折叠**（双空格标题不配单空格查询，双方一致），维持 pinned 语义 |
| E2 | boot-stretch 门行为断言 | 待注入 harness（实测中位 ~569ms 已越过 500ms 显示阈值，门可达） |
| R1–R5 | 插件 hook 永久注册分歧（aisdk 对象 / 返回后突变 / 交错序 / 非 JSON stash / retry 失败通道） | 永久登记——换取崩溃隔离，依据 FINDINGS.md §5 |
| — | pre-2.0.24 旧库升级路径 | 48 个迁移已转译但验收只覆盖 v2.0.24 产库与空库；**首次启动前建议备份数据目录** |

### 1.3 验证盲区（已知未自动化）

- 最严格的验证门是 **opt-in/manual，不在 CI**：OpenAPI 启动差分与 DB 互操作电池需 `OC_P10=1`，跨二进制/TUI 冒烟同理——声明的"empty diff"依赖手动执行，存在跳过风险。
- 旗舰插件（oh-my-opencode-slim）验收测试仅在有安装缓存的机器真跑，CI 自跳过（PLUG-D07 之后的回归正是它抓到的）。

---

## 2. 踩坑记录（事故 → 根因 → 现行规则）

### 2.1 资源与调度

| # | 事故 | 根因 | 现行规则 |
|---|---|---|---|
| 1 | **多子 agent 同时编译 → CPU 饱和 → 共享主机的 newapi 在线服务卡死** | 本机与在线服务（newapi、49374）共享 16 核/22G；并行 lane 各自 cargo build 叠加无上限 | 一切本地构建/测试经 `tools/dev-gate.sh` 单入口：强制 eternal130 身份 + systemd transient scope（CPUQuota=400% 四核硬顶、MemoryMax=10G、TasksMax=512）、1 分钟 load≥6 拒启动；lane 提示词必须声明走 dev-gate |
| 2 | root 直跑 cargo，三次属主污染 | root 产物属主混入用户目录 | dev-gate 强制降权身份；CI 例外（不占本机） |
| 3 | 共享同一检出目录的两个 lane 互相删文件 | 写所有权无隔离 | lane 写隔离=每 lane 独立 git worktree（`.worktrees/`）+ 串行合并回 main；只碰自己 scope 的文件；收尾前重查 `git status` |
| 4 | /tmp/opencode 被整体清空，5 条 lane 未提交工作全失 | API 配额耗尽杀 lane 后，外部清场连证据目录一起清 | lane worktree 移入仓库内 `.worktrees/`；里程碑 WIP 提交强制（小步提交、事后 squash）；/tmp 顶层只用 PID 唯一名 |
| 5 | SCAN-EF 扫描腿（并行编译 + 探针连发 release 二进制）严重拖慢同机生产的 v2 服务，被用户击杀 | 验证负载无节制 | 生产尊重制度：**单 lane 串行**；扫描腿小批量带暂停；重型腿（E2 TTI/soak/电池）需用户明示或错峰；派发前看生产健康信号 |
| 6 | `target/debug` 无界增长至 **476G**（debuginfo 主导 + 增量编译无指纹 GC），磁盘 99% 满 | cargo 增量缓存只增不减、多 profile 叠加 | 2026-10-23 清理 486G（保留 release 3.2G）；建议日常 `CARGO_INCREMENTAL=0` 或周期 `cargo clean`；无 worktree 残留（已核） |
| 7 | API 配额窗口耗尽（92%）在半夜杀 lane | 配额无监控 | 定时唤醒 + 断点复活模式（WIP 落盘后 lane 死亡也可 revive） |

### 2.2 进程与终端安全

| # | 事故 | 根因 | 现行规则 |
|---|---|---|---|
| 1 | **测试盲杀信号误杀系统服务**（生产事故） | 测试按 PID 名/号发信号，撞上复用 PID 的系统进程 | safe_signal 结构性修复：pidfd 系统调用 + boot_id/进程 starttime 世代 sidecar + 外来记录 cmdline 精确校验；全仓禁裸 PID 发信号；组杀=活 leader 钉住+/proc 扫描+逐成员校验；测试只用自有子进程句柄（process_safety guard 常设） |
| 2 | **扫描腿裸启 TUI 二进制锁死用户终端**：raw mode + alt screen 缓住真终端，两个 TUI 同写一屏→持续乱码、ctrl+c 被吃、被迫重启生产 v2 | E2 进程拉起腿未套 PTY harness，直接 `cargo run` 冒屏 | TUI 二进制任何 lane 只准在 vt/PTY harness 内或 stdout/stderr 重定向 + no-tty 模式启动；tmux/终端矩阵探针只碰自建会话；OSC/转义探针只写自有 PTY |
| 3 | **测试进程泄露**：孤儿 rig/电池进程、stale ref-bun 进程（GATE3 一轮杀 12 个）、lane 死后后台矩阵孤儿化（rig 二进制消失、配对进程残留） | 后台长跑无归拢、teardown 不完整 | 长验证一律前台分块（≤45 分钟/块）；rig teardown 卫生项常设；GATE 轮清扫；清理门=mtime<30 分钟视为在用（两次误踩活动目录后立规） |
| 4 | rig 使用陈旧二进制产物，两次被差分器抓获 | lane 换代码未重建 rig 缓存 | lane 收尾强制重建检查（已知陷阱清单） |

### 2.3 宿主 / 会话生命周期

| # | 事故 | 根因 | 现行规则 |
|---|---|---|---|
| 1 | **60 分钟 location-TTL 收割器多次静默杀后台 lane**（含 9E 后台矩阵中断） | 宿主对后台会话的位置 TTL 驱逐 | 长任务=前台分块 ≤45 分钟 + 追加式日志 + 每 6 轮 WIP 提交（断点可续）；**单次工具调用硬上限 45 分钟**（一次 80 分钟单调用险触 TTL） |
| 2 | 完成后的会话收到排队消息 → 幽灵重启（M6 幸无实害） | 排队投递无终态检查 | 禁止向已完成会话投递；干预一律 kill + 新 lane |
| 3 | H-1：复活子代理的第二份报告永远到不了父级（completed→completed 不是迁移） | 宿主缺陷，非本仓可修 | 子代理**禁止跑后台任务**（全部前台块）；父级 watcher 允许；完整分析见 docs/host-issues.md；候选上报上游 |
| 4 | 小时级后台睡眠被宿主收割（40 分钟级偶成） | 宿主对长睡眠的回收 | 级联短看守（50 分钟自续完成通知）+ 用户消息兜底 |

### 2.4 验证方法论

| # | 事故 | 根因 | 现行规则 |
|---|---|---|---|
| 1 | **陈旧 oracle 缓存掩盖真回归**：bun 全局缓存以 @opentui/core **0.6.1** 顶替钉版 0.5.14，病灶窗口内全部基准标错；"从未真绿"的提交穿门而过（双病灶之一） | oracle 缓存无版本/新鲜度证明 | oracle 三可证：版本硬断言（fail unless 0.5.14）+ 采集指纹（driver+scenario FNV-1a 双语镜像）+ always-refresh 终门；main.rs 自愈陈旧缓存、连续漂移硬错 |
| 2 | **"实现了 ≠ 链路接通"**（两案：F16 leader>e 审计标 done 实战失效；sidebar 遮罩有渲染无关闭键层，模糊键全进不可见输入） | 审计只证实现存在，未证链路端到端 | 交互面修复必须带 PTY/field 级验证；审计状态只是起点提示 |
| 3 | **双端差分 rig 五轮门只抓 fmt/goldens/flake，漏掉全部真实用户可见 bug**（omo-slim 侧栏、子代理切换、marquee、spinner 色、thought gutter 五案）→ 用户裁定退役 | 参照平价看不见用户插件内容；语料 vanilla-stub；时序节奏与色 token 绑定从未步进验证 | 差分测试 2026-10-17 退役；扫描循环接棒（后按裁定于 FIXW2 终止）；golden 降级为回归套件；常设仪器补：真实插件沙箱 + 时序 + token 覆盖 |
| 4 | 金帧字节自洽放过"屏幕组合模型"错误（错 tab 模型、错 boot 路由都活到 beta.7 实战） | 金帧证明渲染保真，不证明组合/路由模型 | 交互模型类（路由/tabs/列表拉取）必须做参照源码级对比（file:line），不许 golden 自证 |
| 5 | **合并静默回滚他 lane 成果**（C lane 自动合并回滚 B 的 availability 折叠、删测试；E2 被回退数个版本无人察觉，M0–M5 带伤建设） | 三/四方自动合并吃掉语义；rig 上差异被误读为"其他 lane 在飞" | 合并裁决必须列表级核对每个合并 lane 的删除清单；证据优先方整文件恢复；每 lane 合并后跑受影响 crate 测试 |
| 6 | **幽灵提交**两案：lane 报"已提交"而分支仍在 base | lane 自述无凭据 | 派发模板要求最终报告贴 `git log -1 <branch>` 落地证明；缺证明=未完成 |
| 7 | **无测试合并**：DERIG 推送未跑测试，main 的 parity 构建断一天 | 合并无门 | 每次 main 合并至少受影响 crate 构建+测试后再推，无例外 |
| 8 | merge 后必须补 bun 内差分冒烟（纯 cargo 门看不见 JS 侧破坏，O2a×O2b 融合后 driver.ts 语法断裂存活） | 纯 Rust 门对 JS 侧盲 | harness 相关合并加 bun 冒烟 |

### 2.5 工程卫生

- 发布门 scoreboard：beta.13 起每修必配独立子代理门扫描，循环至全绿才发（GATE1–GATE5）。
- lane 派发模板强制项：English commits、分块调用（≤45 分钟）、worktree 隔离、写明 dev-gate。
- DNS 探针文件（/tmp/opencode/dns-probe.*）不是构建产物，勿清。
- 仓库根曾经堆积垃圾文件（`None`、`refprobe.pid`）——2026-10-23 清除；收尾自查 `git status` 为模板必查项。

---

## 3. 现行制度速查（防复发汇总）

| 类别 | 制度 | 载体 |
|---|---|---|
| 资源 | 单入口构建/测试（配额+身份+负载闸） | `tools/dev-gate.sh` |
| 调度 | 单 lane 串行；重型腿需批准；生产健康前置检查 | 生产尊重制度（2026-10-18） |
| 终端 | TUI 只在 PTY/vt harness 内启动；探针只碰自有会话 | 终端隔离规则（2026-10-18） |
| 信号 | 全仓 safe_signal；禁裸 PID；测试用自有子进程句柄 | `tools/guards/process_safety.py` |
| 生命周期 | 前台分块 ≤45 分钟；WIP 提交；禁投已完成会话；子代理禁后台 | lane 派发模板 |
| 写隔离 | 每 lane 独立 worktree；串行合并；删除清单核对 | 写隔离制度（2026-10-11） |
| 合并 | 受影响 crate 构建+测试后推送；落地证明（git log -1） | 合并门（2026-10-18） |
| oracle | 版本断言+指纹+always-refresh | `tools/diff`（opentui-rust） |
| 磁盘 | 增量抑制（CARGO_INCREMENTAL=0）/周期 clean；清理门 mtime≥30 分钟视为在用 | 本册 2.1#6 |
| 验证 | 交互面必须 PTY/field 级；组合模型必须源码级对比；金帧=回归套件 | 方法论登记（ROADMAP §4） |
