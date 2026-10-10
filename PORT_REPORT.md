# opencode v2.0.24 → Rust 移植报告（PORT_REPORT）

> 生成：2026-10-11 ｜ 移植基线：opencode **v2.0.24**（commit e7a34f09bf，钉版只读 worktree /opt/opencode-ref）
> 交付仓库：/opt/opencode-rust（单二进制 `opencode-rust`，自报版本精确为 2.0.24）
> 终态：**全 10 阶段完成；M1/M2 里程碑达成；旗舰插件实测通过；全仓 2181/2181 测试、clippy -D warnings 0、fmt 0**

## 1. 目标与结果总览

| 目标（用户设定） | 结果 |
|---|---|
| 功能一致性（CLI+Server，不含桌面端） | ✅ openapi 语义 diff **空**（服务文档与钉版契约逐字节相同）；错误路径 7/7 逐字节；141 操作全实现；双向互替实测（官方 v2.0.24 TUI 连 Rust 服务器 3/3 交互；Rust 客户端连 v2 服务器全流程解码）；DB 双向互通（v2 写库→Rust 0 重跑续写→v2 再读；含 v2 产生的 NULL 重行与 stable 频道文件名方案） |
| 外观一致性（TUI） | ✅ golden 帧体系：37 场景/66 变体 ×（文本网格+ANSI 字节）按 Tier A/B/C1 分级验收——Tier-A 31/31 场景**文本+ANSI 逐字节**（含 tabs glow 确定性 tint 数学、composer 五态、fuzzysort 排序位精确）；Tier-B（diff）文本精确+256 降级样式；C1（markdown 文本网格/冻结 spinner/toast）；5 个按键回放脚本全过；CJK/emoji/ZWJ 宽度以 opentui 实测侧车钉死 |
| 全线 Rust 重写 | ✅ 12 个 crate（util/schema/theme/ai/core/plugin-host/server/client/tui/cli+工具），零 TS 运行时依赖（插件宿主的 bun 子进程除外，见 §4） |
| 插件支持等同 | ✅ bun 子进程宿主（决定依据=spike 实测：deno_core 结构性败于 CJS/node 内建）；**生产接线完成**（config `plugins[]` 在 oc-server 与 oc-tui 双入口启动宿主、每 location 应用注册、CodeMode 按 v2 语义核心化——零插件时 MCP/opencode.* 工具也仅经 `execute` 暴露、/api/plugin 真实）；**旗舰 oh-my-opencode-slim v3.0.3 经真实二进制路径实测通过**（自检 12 agents/12 tools/8 skills、agents 入列、命令入面板、marker 提示词往返真会话、reflect 技能端到端）；codemode 解释器全量；对抗 fuzz（kill -9/全表面超慢）有界无死锁；V1/V2 双格式 |

**里程碑**：M1（官方 v2.0.24 TUI 可用 Rust 服务端）✅ 门 6/8 达成｜M2（Rust TUI 日常全流程）✅ 门 11 GRANTED，23/23 步实测（提示→工具→权限→tab→切换→复制→主题→**$EDITOR 挂起恢复**→重启恢复→安全关机）｜M3 近似（社区插件不修改直接可用）✅ 旗舰验证。

## 2. 工程规模与质量

- **测试**：2181 通过（另有 9 个 opt-in 验收测试：M2 演练、TUI 冒烟×2、P10 电池×6——全部实测通过）
- **验收方法论**（每项一致性主张都有参照实例背书）：274 个 LLM cassette 逐事件重放（121 交互含 HTTP 错误）+132 请求体 golden；projector 重放对账（165 真实事件→投影行与参照库逐列相等）；扫描器 420/420 语料；63 主题逐字节；shell 扫描器 bun-vs-Rust 420/420；TUI 66 帧 SHA 钉版双跑稳定；混沌测试（总线 8×5 并发序、死锁风暴、插件 kill -9）
- **过程**：12 道 oracle 评审门（含 2 次驳回复审、1 次项目级 NOT APPROVED 整改）；每阶段 fmt/clippy -D warnings/nextest 全绿才提交；git 全程约 30 个里程碑提交
- **运行环境**：全程非 root 用户（eternal130）开发与验收；参照构建（bun）自愈重建；跨属主 scratch 冲突根治

## 3. 与 v2.0.24 的已文档分歧（诚实清单）

**实现选择类**
- **独立身份（Lane B）**：二进制更名 `opencode-rust`（`--version` 横幅 `opencode-rust v2.0.24`；`/api/info` 版本仍为 2.0.24 不变）；XDG 目录隔离为 `opencode-rust` 后缀（config/data/cache/state 四项，env 覆盖机制不变）；v2 存量数据**单向一次性拷贝迁移**（`oc_core::v2_migration`：数据库走 sqlite backup API、auth.json、扁平配置文件、npm/bin 插件缓存；log/repos 不拷；源目录永不改动——v2 的 v1→v2 迁移是原地升级，我们是拷贝，原因正是目录隔离；marker 文件闩死单向门，锁文件串行并发入口，硬失败仅告警不阻断启动）
- 插件宿主为 **bun 子进程**（v2 进程内）：hook 可变对象为 copy-in/copy-out（v2 引用可变）——病态插件可观测差异，良性行为等价；换来崩溃隔离
- Last-Event-ID 重订阅是我们新增（v2 服务器不支持）+ 事件 id 4096 环去重；G11-F1 首连快照/流窗口竞态已在抽头与 store 双侧加固，理论窗口仍登记在案
- 键位/主题配置解析 throw→skip（v2 会崩）；`input_paste` 的 preventDefault 对象语义未移植
- dev 模式 `bun run` 参照服务器的外来注册表停止采用**精确 basename** 身份校验（宁拒杀不误杀——生产事故后的安全决策）
- flock/watch 部分语义未移植（单进程场景无影响）；模型排序 release-date 精细序简化；/help 列表有 clamp

**延后项（post-M2，未实现即诚实缺失）**
- `opencode mini`（第二前端；`opencode run` 已覆盖非交互）、`acp`（编辑器协议）、worktree UI、声音/OS 通知（C2 动画类）、tree-sitter 语法高亮、图像/kitty/sixel、半块字形、子像素 RGBA
- rmcp 0.5 的 elicitation 能力缺口（逻辑已备，等上游）
- PTY 守护进程的跨重启交接（状态机在，进程边界未做）
- P10-N1（非 JSON body 的错误形状未钉）；usage 扇出事件序竞态（v2 projector 同样存在，行为对齐）

**已知边界**
- `serve --service` 已恢复 chdir 至 $HOME（per-location provider 覆盖已落地并实证"守护 A 服务 B"修复）
- pre-2.0.24 旧库升级路径：48 个迁移全部转译但验收只覆盖 v2.0.24 产库与空库（**首次启动建议备份数据目录**）
- 旗舰的 TUI export（dist/tui2.js）为 desktop/opentui 绑定——在 v2 桌面宿主外本就不可加载；Rust 侧以原生 ctx.ui.* slot 缝提供等价能力（6/6 slot 实测）
- openai-responses 的 WebSocket 通道延后（SSE 回退路径完整，v2 默认亦可用 SSE）
- 请求 schema 校验仅覆盖电池中的 2 条路由（session.create/list），其余路由对畸形载荷的 400 形状与 v2 有分歧（P10-N1 同类）
- TUI 非空草稿上 leader 和弦不触发（v2 键位表对和弦挂起 textarea——行为差异已登记）
- 写回带 JSONC 注释的配置：拒绝并要求 --force（v2 保留注释重写）——防数据丢失的保守选择
- TUI 本地状态（tab/会话列表）channel 目录已对齐 v2 出厂默认（stable）；从旧版 `local` 目录迁移不自动（存量状态重新累积）

**插件与 CodeMode 生产状态（门 12 审计→接线闭环）**：门 12 抓出"库级完成但生产未接线"，已由收尾接线泳道闭合并实测（真实二进制路径加载旗舰、零插件 CodeMode 快照、/api/plugin 真实列表），提交 `09bcc15`。

## 4. 打包与分发

- release 二进制（thin-LTO+strip）产物为 **`opencode-rust`**，自报 `opencode-rust v2.0.24`（`--version-rust` 暴露构建戳）；数据目录隔离 `opencode-rust` 后缀 + v2 存量单向拷贝迁移（见 §3）；发布冒烟脚本全绿（含 release 二进制的 FakeLlm run e2e 11/11）
- **bun 分发决策**（保守方案）：托管钉版下载 bun 1.3.10（sha256 f57bc018… 钉入源码，取自真实下载）至 ~/.cache/opencode-rust/bin/，发现顺序 缓存→系统→PATH；缺失且需要插件时自动引导下载，全程可禁用（OPENCODE_DISABLE_BUN_DOWNLOAD），失败优雅降级为核心功能不受影响
- install/install.sh（v2 形态安装器，校验和验证）；`opencode upgrade/uninstall` 真实现；**更新通道（Lane C）**：`opencode-rust upgrade`（无目标）查询本仓库 GitHub Releases feed，按通道规则（beta 构建=beta+final 可见，final 构建=仅 final）选候选，sha256 边车校验后原子替换；后台检查器（默认 TUI/serve 入口拉起，10 分钟周期）按 `update: disable|notify|auto` 配置（默认 notify，旧 `autoupdate` 键兼容映射，`OPENCODE_DISABLE_AUTOUPDATE` 环境斩波；dev 构建完全跳过）。**自动检查的失败隔离为超出上游的刻意加固（REGISTERED）**：网络故障（DNS/连接/TLS 垃圾/HTTP 错误/限流/解析失败）一律静默无操作——零启动延迟、无 stderr 噪声、无 panic（catch_unwind 兜底）、线程/临时文件零积累（PID 唯一缓存临时文件，失败即清、下次运行清扫）；v2 上游**没有** sha256 校验，本端口的手动与自动安装全部强制边车校验、校验不过即拒装（tamper-proof all-or-nothing）。此前"自动更新默认关闭"的门 2 裁定由本通道的失败隔离契约取代：自毁风险由"校验+原子替换+静默失败"三重保障消除，且默认策略仅 notify（不自动安装）。

## 5. 安全加固（事故后）

生产事故（测试盲杀信号误杀系统服务）后的结构性修复：safe_signal（pidfd 系统调用+boot_id/进程 starttime 世代 sidecar+外来记录 cmdline 精确校验）；全仓无裸 PID 发信号；组杀=活 leader 钉住+/proc 扫描+逐成员校验；测试只用自有子进程句柄。全程 systemctl 前后对照验证。

## 6. 复现与验收入口

- 全量门禁：`cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo nextest run --workspace`
- 验收电池（需 bun+参照）：`OC_P10=1`（openapi/错误路径/互替/DB）、`OC_M2=1`（TUI 日常演练）、`OC_TUI_SMOKE=1`（tmux 冒烟×2）
- 发布冒烟：`tools/release-smoke.sh`；TUI golden 重导出：`tools/parity/tui-dump`（--verify）
- 完整过程档案：`.slim/deepwork/rust-port/`（48 份泳道报告/评审/架构图——含每条分歧的原始依据）

## 7. 结论

以 v2.0.24 为唯一规范源，完成了 CLI+Server+TUI+插件体系的 Rust 全量重写，一致性主张全部锚定参照实例（非自述）：服务端契约逐字节、TUI 视觉逐帧逐字节、引擎行为逐事件、旗舰插件端到端（库级）。残留为已具名的延后项、理论窗口与集成边界（§3），逐项可查 `.slim/deepwork/rust-port/` 的 48 份过程档案。用户可按 §4 安装 `opencode-rust` 替换 v2.0.24 使用（首启自动单向拷贝 v2 存量数据至隔离目录，源目录原样保留；含 stable 等频道文件名方案；旧版本数据首启前建议备份）。
