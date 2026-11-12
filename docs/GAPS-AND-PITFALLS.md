# 移植缺口与坑（Gaps & Pitfalls）

> 2026-11-06 清理注记：下列各轮审查引用的一次性总报告（CODE_REVIEW_PARITY_*.md、PORT_AUDIT/PORT_COMPARISON*.md、COMPREHENSIVE_PORT_AUDIT.md 等）已清理出树，全文经 git 历史可溯（清理提交的父提交）；现行开发规范见文末「开发规范」节。

> **2026-11-06 第十三轮（未收敛缺口复核，tui-full-convergence 收官后）**：对第十二轮未收敛的 45 项（G-165..G-210 全部 ❌/⚠️/🟢；G-177 已闭、G-211/212 为 ℹ️ 登记项不在范围）8 通道双侧源码逐项复核（rust@edfc770f vs ref）：**41 项维持 STILL_OPEN、4 项 PARTIAL（G-172/G-175/G-181/G-194，勘注已就地追加）、0 项整条关闭**——tui 侧 opentui-rust 收敛改造对第十二轮 TUI 缺口的实际清偿面很小（仅 G-181 的 ②/new 与 ③resize 两子项）。关键新事实：G-197 ref 已漂移至 139 方法/118 个 404 声明（rust 仍 75/142，缺口更宽）；G-170 连 isSettled 导出过滤也缺（弱于登记）；G-187 opentui 原生图片管线在位（session.rs:790-808）但 tool_detail 全臂仍喂 `images: Vec::new()`；G-200 的 `POST /session/{id}/view` 生成方法已存在（methods.gen.rs:3998-4011）零调用；G-165 custom 车道已加缺 baseURL 硬报错守卫（facade.rs:1362-1370，catalog 车道仍静默兜底 api.openai.com）；G-204 的 `shell/result.rs:75-87 metadata()` 已含 signal/timeout 但 tool_result 未调用。ref 锚点漂移勘正：tui index.tsx→routes/session/、ai models.ts→core/src/github-copilot/、websearch.ts→tool/plugin/、compatibility.ts→config/plugin/。明细 `/tmp/opencode/review/VERIFY_R13_2026-11-06.md`。

> **2026-11-07 第十四轮 burndown 收口（gap-burndown-8）**：G-165..G-210 全部未收敛项（45 项，含补漏的 G-193）经 lanes 1-11 串行落地合并 main，逐条 ✅；无上游阻挡项。明细见文末「第十四轮 burndown 收口（2026-11-07，gap-burndown-8）」节。

> **2026-11-07 第十五轮（15 通道全功能逐项对比审查 + 4 通道缺口复核）**：对 ref v2.0.24 全功能面（TS ~24.7 万行核心链路 vs Rust ~55 万行，core 会话/工具/AI/TUI/配置/总线/DB/shell-PTY/MCP-插件/权限凭证/文件系统/CLI/server/client-schema-theme-util/杂项）逐文件对读后，对 38 项 ❌ 指控与 17 项重点 🟡 做 4 通道双侧现行源码复核并与本册 G-1..G-212 去重：**新立项 G-213..G-240（28 项：🔴×1/🟡×15/🟢×9/ℹ️×3）；G-116 ✅ 状态回退重开（三子项经代码取证均未落地）；两条旧勘误订正（repository-cache「完整移植」与 EADDRINUSE「全链路接线」断言均与代码不符）；6 项初报撤销；4 项归类既有**。总体画像：核心链路一致率 ~83%（✅1182/🟡196/❌38/➕77）。总报告 `PORT_REVIEW_v2.0.24.md`，域报告与复核记录 `/tmp/opencode/port-review/`。

> **2026-11-08 第十六轮 burndown 收口（gap-burndown-9）**：第十五轮立项的 G-213..G-240 全部可行动项（含 G-226 裁决落地）+ G-116 回退重开三子项 + lane12 抽查重开的 G-117 残半边，经 lanes 1-13 串行落地并 fast-forward 合并 main，逐条 ✅；PTY 族 lane（5/9/10）另过 `--profile pty` 序列化门。e1 canary 经盘上二分+零语义受控实验定性为**逐构建翻转噪声**（勘注见 2.2#4）。无上游阻挡项（无需 opentui-rust 未移植功能的搁置项）。明细见文末「第十六轮 burndown 收口」节。

> **2026-11-08 第十七轮（独立 19 通道全量复审 + 4 通道缺口复核）**：在十六轮收口后的当前树上，做独立于第十五轮的第二轮全量逐行对读（19 通道：session 数据/执行循环、AI providers 主+补、内置工具、MCP-插件-codemode、配置-技能、权限-凭证-集成、fs-vcs、PTY-shell、server、infra、TUI 框架/组件/mini、CLI、client-theme + integration/内置插件/AI 媒体三补审；总表 `PORT_REVIEW_INDEPENDENT_v2.0.24.md`，明细 `/tmp/opencode/review-lanes/`），再经 V-A..V-D 四通道对全部缺口指控逐项**双侧现行源码**复核并与本册 G-1..G-240 去重：**新立项 G-241..G-290（50 项：🔴×7 / 🟡×27 / 🟢×14 / ℹ️×2）；8 条既有条目勘注（G-229① 客户端泵臂未闭环、G-63② daemon Write 臂、G-234② 错误页四流、G-17 两补注、G-97/G-119/G-128/G-150）；15 项初报撤销（含两处与 G-6/G-127 ✅ 状态的矛盾指控——代码与登记册一致，系陈旧注释误导）**。核心新事实：mini 文本流**真**重复输出（G-241，自带测试断言累计全文+writer 纯追加）；MCP server 工具执行期无权限断言且绕过管线（G-242）；非持久 PTY drain/attach 多字节 panic（G-243）；zen/自托管 Console 元数据链路断裂（G-244）、WS transport 默认丢失（G-245）、媒体凭证持久化泄漏（G-246）、CodeMode 沙箱与模型面声明矛盾（G-247）、warming 零运行时（G-257）。

> **2026-11-09 第十八轮 burndown 收口（gap-burndown-10）**：第十七轮立项的 G-241..G-288 全部 48 项可行动缺口 + 三条勘注残臂（G-229① 两段消费臂、G-63②、G-234②）经 lanes 1-17 串行落地合并 main，逐条 ✅；无上游阻挡项（唯一搁置 G-287⑧ OSC-8 超链接：光栅能力已在，开启=oracle 重钉事件需升级裁决）；ℹ️ G-289/G-290 登记入册。明细见文末「第十八轮 burndown 收口」节。

> **2026-11-09 第十九轮（17 通道逐行对比审查 + 4 通道缺口复核）**：对当前树做独立于第十五/十七轮的第三轮全量逐行对读（core 会话/工具×2/MCP-插件/配置/权限认证/fs-vcs/infra/shell-pty/AI protocols/AI 核心/server/CLI/TUI×3/schema-client-util；总报告 `PORT_PARITY_AUDIT_v2.0.24.md`，域报告 `docs/audit/L1-L17`），再经 V-A..V-D 四通道对全部缺口候选逐项**双侧现行源码**复核并与本册 G-1..G-290 去重：**新立项 G-291..G-318（28 项：🔴×1 / 🟡×13 / 🟢×8 / ℹ️×6）**；既有条目补注 6 条（G-5/G-17×2/G-23/G-28/G-29/G-231）；**撤销 18 项初报**（含 4 项与已修状态冲突后经现行代码证伪的指控：copilot anthropic-beta 头（G-167 车道 BetaHeadersTransport 已注入）、SSE header/chunk 看门狗（TimeoutBudget 完整在位）、`Model.small`（逐字对齐）、models.dev options.file（OPENCODE_MODELS_PATH 已接线））；1 项跨通道冲突经 orchestrator 仲裁（normalize/truncate 顺序成立：ref tool.ts:150 在 executeTool 内先于 step.ts:122 truncate）。核心新事实：工具注册表按 bare name 键控致 MCP 同名工具互相覆盖（G-291，🔴）；复核记录 `docs/audit/R19-V{A,B,C,D}-*.md`。

> **2026-11-10 第二十轮（未闭合缺口重验）**：对第十九轮立项的 22 项可行动未闭合项（G-291..G-312 ❌/⚠️ 全集）在 HEAD=967bedba（含 c36d2556 全仓 fmt + clippy 清理波次）逐项重验：**22/22 STILL_OPEN，无一被实质修复**（该波次提交均为格式/清理，未触及语义面）；登记证据行号已随 fmt/布局漂移刷新。四处状态勘注：**G-291 新增派发面矛盾证据**（model_request.rs:646-647 注释自称 effective 键与源表 bare 键实现直接矛盾，修复时须一并消除）；**G-304① 恶化**（`env::vars()` panic 向量 1 处→2 处：tool/shell.rs:311,664）；**G-306 边角收窄**（container 改整体 `Option<Value>` 透传 :226/:1815-1816，内部 id/skills 显式 null 仍被拒）；**G-307 已被代码注释显式自认有界**（patch.rs:429-433 "writes WITHOUT creating parents"——可并入 G-1 作有界决策销项）。G-297 修复基础设施确认已在（bus.rs:194 `PublishOptions::commit`）。复核记录 `docs/audit/R20-W{A,B,C}-*.md`。

> **2026-11-10 补立（第三类悬空项入册 G-319..G-329）**：把登记册散记多年/散记各轮但从未立项的未认领差异正式补立（11 项：🟡×3 / 🟢×5 / ℹ️×3）——含第四轮即发现至今零调用的 mantle `validate_config` 死校验（当日 grep 复证）、根外 skill 源目录热重载缺失（当日复证 watch.rs 无 skill 引用）、十八轮 Deferred minors 四件、DbError::Pragma 隧道、session.diff is_public 条件性盯守、title-shimmer arrival-wipe 相位、G-143/G-36 两处 bounded 裁剪面转正入册。见文末「第三类悬空项补立」节。**同日二次补立（第四类决策面转正 G-330..G-333，体例同 G-328/G-329）**：R-01b 体积护栏未机械化（G-13 重评前提缺基建）、G-239 panel 激活规格入册、G-28 族两成员（插件 markdown 代码块渲染器注册链、node 运行时备轨）转正。见文末「第四类决策面补立」节。

> **2026-11-10 第二十一轮 burndown 收口（gap-burndown-11）**：第二十轮重验确认未闭合的 G-291..G-312 全部 22 项 + 第三类补立可行动项（G-319..G-325/G-327，其中 G-324 为决策落地）+ G-330 体积护栏 + G-128 🧹，经 lanes A-H 串行落地合并 main（e82f4037/e8de0341/9d85f709/66f8337b/bf5b2e4e/7bc929c1/488bbb8f/3e3233ff），逐条 ✅；每 lane workspace 合并门全绿（白名单项按证据链放行）。**无上游阻挡项、无搁置项**。明细见文末「第二十一轮 burndown 收口」节。
>
> **2026-11-11 第二十三轮 burndown 收口（gap-burndown-12）**：第二十二轮立项的 G-334..G-383 全部 50 项经 17 个串行 lane 落地合并 main（L1 快赢/L2 engine/L3 hooks/L4 PTY/L5+L5b plugin-host/L6 媒体/L7a+b 协议/L8 providers/L9 server-vcs/L10 MCP/L11-12 TUI/L13 CLI-ACP/L14 schema-client/L15 core 杂项），逐条 ✅；**无上游阻挡项**。G-347 布线前提经双侧源码证伪（ref 无 /api/media 面）按勘误处理，残余（decode 严格化+薄 client 门面）落地；另 4 项审计文本失实/前提不成立以证据订正。工具面硬化：dev-gate 结构化禁裸 cargo test（`c4aa35ae`）、负载门 6→8（`47cd5ab1`）。明细见文末「第二十三轮 burndown 收口」节。

> **2026-11-12 第二十四轮（census 机械化普查立项）**：基于 14 通道机械普查（`tools/census/*.py` 14 个幂等脚本 + `docs/audit/census/` 14 报告 + INDEX，双侧权威自报对拍/生产面提取/已知锚交叉验证，全部连跑逐字节可复现）：**新立项 G-384..G-390（7 条：🟢×4 / ℹ️×3）**；G-230 加勘注（die 分支前置面机械化确认：内建 output 声明 0/18，范围自 0/14 扩大，NOT_YET 延续）；9 族既有条目机械化复认（G-295 rpc.events、G-239/G-331/G-332/G-148② 插件 tui 入口面 13 键、D3-7 TMUX/STY/NODE_ASSETS_DIR、G-174 requireSignature 键、G-10 pty read/restore、G-383b replay_complete、G-373 系 provider 3 native 入口、RC-11 completion）；三处旧锚修正（事件 52→实 51 系旧口径分桶、keybind 230→243 系 ref 演进、catalog-only "~180 推定"→确凿 226=49 native+168 compat+9 npm 桥豁免——第二十二轮盲区#2 销项）。**14 通道总覆盖率 100%（双侧并集逐项归属）**；此后每轮缺口扫描边际成本=14 条命令，上游 bump 时轴 1 类 diff 自动产出变更面清单。

> **2026-11-12 第二十五轮 burndown 收口（gap-burndown-13）**：第二十四轮可行动项 G-384/G-385/G-387/G-388 + G-230 die 前置（18 内建 Output 声明）经 lanes A-C 串行落地合并 main（`b97f6ee5`/`75d746a7`/`80995995`+`58174fd4`+`7be6317b`），逐条 ✅；每 lane 独立评审（Lane C 经一轮 fix round：校验器补 `const` 关键字后复审通过）。census 复跑：PLUGIN-API REF_ONLY 3→1（余 rpc.events=G-295 ✅ 提取器口径）、ENV-VARS REF_ONLY 63→59（LOG_LEVEL/TERM_PROGRAM_VERSION/TMUX/STY 转 BOTH）、TOOLS die 面 18/18+die 在位（提取器随新形状更新）。**无 opentui-rust 依赖的上游阻挡项**；唯一上游阻挡=G-247 残余全局 Bun/process 面（Bun 运行时钉死，已标注搁置）。明细见文末「第二十五轮 burndown 收口」节。

> **2026-11-12 第二十五轮增补收口（gap-burndown-13 amendment）**：用户下令把登记/搁置态的 G-239/G-331（session.panel+PanelInput）、G-332（markdown 代码块渲染器注册链）、G-333（node 备轨）、G-272⑩（mini 样式化滚动回写）转修复，经 lanes H/I/J/K 串行落地合并 main（`78cbda37`/`b6d66a5f`/`06232e6c`/`6fea30de`）+ opentui-rust 三提交（`a8e41e9`/`5a079fa`/`34a5ac4`，oc pin 56a9eda→34a5ac4），逐条 ✅；Lane H/K 各经一轮 fix round 后复审通过；opentui 差分 127 IDENTICAL 零重钉。G-28 生态 breaking 声明面相应收窄（见 G-28 增补轮勘注）。明细见文末「第二十五轮增补收口」节。

> **2026-11-12 第二十六轮（差分层通道 1：配置解析语义差分）**：同一 corpus（43 例：合法/畸形/边界/JSONC/未知键/大小写/重复键/UNSUPPORTED 族）喂双侧**纯函数管线**（jsonc-parse → normalize → Info decode；ref=bun 驱动 staging+node_modules 链接、git 树零改动；rust=env 门控集成测试零常态成本），归一化（kind 小写、诊断排序、encoded 键序排序）后五字段对拍。**MATCH 41/43 · DIVERGE 2**：新立项 **G-391** 🟡（provider `Model.cost` 为 13 字段中唯一缺 `skip_serializing_if` 者——normalize 类型化往返物化 `"cost": null` 且经 `Info.providers` 逃逸到 `GET /api/config` wire；修法一行+差分复跑即验证）+ G-390 复认（rust 超集旋钮预期分歧，非缺口）。验证"存在性全面 ≠ 等价性全面"：census CONFIG-KEYS 曾确认键集/默认值对齐，序列化物化只有差分能抓到。工具 `tools/diff/{config_parse.py,corpus/config/,drivers/}`，报告 `docs/audit/diff/CONFIG-PARSE.md`，复现 `python3 tools/diff/config_parse.py`。后续差分通道按成本升序：ACP 转译 → SSE store 投影 → JSON Schema 校验器对拍 → /compact 管线 → HTTP 全量。明细见文末「第二十六轮」节。



> **2026-11-10 第二十二轮（全功能逐行对比审查 parity-line-review-v2024：27 lane + P1/FINAL 双门）**：对 ref v2.0.24 与当前树做第四轮独立全量逐行对读（27 条域 lane：oc-core×12 / oc-ai×5 / oc-tui×4 / server·client·cli·acp·plugin-host·schema-util×6，双侧 file:line 证据、不采信已有审查文档），两道 oracle 一致性门裁定 5 处跨 lane 矛盾、抽查 11/11 成立、0 未裁定矛盾。**新立项 G-334..G-383（50 项：🔴×12 / 🟡×22 / 🟢 簇×16）**；归类既有 12 处、撤销初报 1 条、勘正旧勘误 1 处（R17#13 locale_duration 括注）。核心新事实：CodeMode 沙箱逃逸残余具体化（`__rawFetch`/eval/Function/限位仅桥上，G-334）、WebSocketSeams 与 permission.evaluate 两条插件钩子生产断线（G-337/G-338）、非持久 PTY reader 逐 chunk lossy UTF-8（G-335）、中断后 hosted 工具多发持久 tool.failed（G-336）、媒体 provider 协议全移植但部署接线为零（G-347，产品级）、cloudflare-ai-gateway 网关语义全降级（G-348）、TUI 时长格式化 `%10_000` 取模 bug（G-341）与 stdin 双读竞争（G-342）。总报告 `docs/audit/port-parity-line-review/{REPORT,TABLE}.md`，32 份 lane/门原始报告归档 `docs/audit/port-parity-line-review/lanes/`。正面互证：keybind 230/230、主题资产 33+1 md5 全等、openapi 141 operation 字节级 0 diff、48 迁移 ID 全同、52 会话事件分类一致、models-dev 快照 md5 相同、session-message parts 全集无缺漏。

> **2026-11-03 第十二轮（全量对比审查）**：22 通道逐文件全文对读（TS ~21 万行 / Rust ~40 万行生产代码）+ 3 通道缺口核验，对 50 项缺口声称逐项双侧源码取证：**新立项 46 项（G-165..G-210，含 🔴×6：Azure 未接线/Mcp.Protocol wire bug/Copilot anthropic 车道/第三方 anthropic 根 URL/shell 注释换行权限逃逸/transfer 脱敏弱）**、撤销 4 项初报 + 6 项子句修正（见文末第十二轮勘误）、5 项归类既有条目。总报告 `CODE_REVIEW_PARITY_2026-11-03.md`，域报告与核验记录 `/tmp/opencode/review/`。

> **2026-11-02 第十一轮 burndown 收口（gap-burndown-7）**：第十轮立项的 G-105..G-164 全部 60 项经 lanes 1-9 落地并合并 main（lane1 043408d / lane2 3269839 / lane3 822d72d / lane4 88b25b5 / lane5 a28e380 / lane6 e516182 / lane7 e3395dc / lane8 3480df9 / lane9 本轮合并），逐条状态改标 ✅（G-163/G-164 维持 ℹ️ 登记项，G-161② 已修）。各 lane 有界偏离清单见文末第十一轮。

> **2026-11-02 第十轮（全量逐行复审）**：9 通道逐行对读 + 6 轮补审 + 4 通道缺口复核，对全部初报缺口逐项双侧源码取证后确认 60 项新立项（G-105..G-164）、驳回 13 项初报（见文末第十轮勘误）、若干 DUP 归并既有条目（G-6/G-17/G-32/G-49a 已追加备注）。总报告 `CODE_REVIEW_PARITY_2026-11-02.md`，明细 `/tmp/opencode/review/`。

> **2026-11-01 第九轮（七域全量复审）**：7 通道对 ref 全功能域（core 会话/工具/AI/server/TUI/CLI/schema-plugin-pty-syntax）逐行对读后，对初报缺口逐项源码复核：确认 12 项新立项（G-93..G-104）、撤销 3 项初报（见文末第九轮勘误）、5 项归类既有条目。详见文末第九轮。

> **2026-11-01 第七轮 burndown（gap-burndown-4）**：G-50..G-78 全部 + G-41..G-48 残留 + G-31/G-34/G-24 残留 + G-1 consumer 迁移（read/write/edit/glob/grep/patch/shell/pty 全走 Environment seam）经 lanes 1-9 串行落地并合并 main（merge commits 8f5c7a2..c289c2b：lane1 G-50/51/52/53/54/70；lane2 G-41..G-44；lane3 G-55/57/58/59/60/63/76/77/78；lane4 G-56/48/34/33；lane5 G-64/65+G-24 残留+G-46；lane6 G-66/67/68/69/47；lane7 G-61/62/71/72/73/74/75/45；lane8 G-31；lane9a/9b G-1）。本轮（lane10）核验并钉定 G-49a spinner 40ms 帧数学（帧索引按流逝时间派生，repaint 维持 pump tick——文档化决策），并完成文档终扫。现存未关闭项均为**文档化决策**：G-28 有界 TSX shim、G-32 v1/legacy schema 有意不移植、G-12/G-13/G-16/G-17/G-38/G-39/G-40 ℹ️ 维持不变；少量 deferred minors 见 .superpowers/sdd/ 各 lane report（如 G-68 mini restore 保留 seam、G-24 guest vcs provider info/base 闭包留 guest 侧、G-36 demo 剧本 6/13 bounded、G-9 nested scope 简化）。

> **2026-10-31 第六轮复核**：对文档现存未关闭条目逐项现状验证——G-4/G-6/G-8 已悄然修复收口；G-1 残留缩小（mcp/stdio 已迁）、G-24/G-31 部分清偿；G-34/G-41..G-48 确认仍在。详见文末第六轮。

> **2026-10-31 第五轮全量对比审查**：9 通道全量对读 + 4 通道缺口复核，新立 G-50..G-78（含 5 项🔴：G-50 snapshot 保真、G-51 persistent-pty Origin、G-52 debug config 脱敏、G-53 skill 输出、G-54 run 表单退出码），订正 G-17 断言与勘误 #4 半边，撤销 12 项初报。详见文末第五轮。

> **2026-10-31 收口轮（gap-burndown-3）**：G-18..G-27、G-29..G-31、G-33..G-37 全部落地并合并 main（单 lane 串行、worktree 隔离、dev-gate、合并门测试，日志在 .test-logs/merge-*）。残留清单与 G-1/G-6/G-8 残留项同轮清偿（MCP stdio 经 Environment seam、Shell 查看 1s 轮询、https/socks5 WS 代理）。G-26 双栈信任统一（oc-http crate，platform-verifier + webpki 降级）。G-28 维持有界化设计（声明为插件生态 breaking change）、G-32 v1/legacy schema 维持有意不移植（纯 v2 面无影响）——两项裁决为文档化决策。仍开放的工程残留：G-1 tools/handlers 全量 consumer 迁移（read/write/edit/shell/glob/grep/patch、pty）、G-24 有界外项（guest 侧 worktree host-op 调用、vcs.transform/reload、websearch editor.default.set 持久化、worktree 注册表热重载）。（G-31 内存索引已由 lane8 收口。）
>
> 2026-10-30 二次复核版；同日 burndown 完成状态更新（gb2-burndown，commits a193f4b..phase-6）。每条均经两侧源码取证（rust `/opt/opencode-rust` vs ref `/opt/opencode-ref`），未采信仓库内既有评审文档。总报告见 `COMPREHENSIVE_PORT_AUDIT.md`。
>
> 状态图例：✅ 已解决（本轮 burndown 落地）· ❌ 缺失 · ⚠️ 部分/偏差 · ℹ️ 有意决策（已文档化）· 🧹 清理项
>
> **2026-10-30 第三轮全量对比审查**：10 条并行通道对 ref 全部功能逐模块对读后，又对所报缺口做了 3 通道代码级二次核验。确认缺口收录于下方 G-18..G-40；8 条原报缺口经核验撤销（见文末"勘误"）。总报告见 `PORT_COMPARISON_FINAL.md`。

## 高优先级

### G-1 ✅ workspace 沙箱执行层（environment/）已移植
- 2026-10-30 落地：`crates/oc-core/src/environment/`（files/driver/exec_defaults/local/memory/unavailable/workspace/mod + tests，9 文件）——Files trait（lstat-vs-follow 语义、64MiB 读上限、move into-dir join）、exec-defaults sh 协议（退出码 44/45/46、LC_ALL=C、GNU stat/find/dd，字节级对齐 ref）、WorkspaceService（create/provision/connect/destroy + WorkspaceDriver provider registry，local 默认）。
- Environment 服务按 location 绑定（workspace_id → connect，否则 local driver）；`oc-server/state.rs` AppState 已接线。
- 2026-11-01 consumer 迁移 part 1（lane9a）：read/write/edit/glob/grep/patch 六个文件工具全部改走 location 绑定的 Files seam（`ToolServices.files` + `FileMutation::with_files`，engine 侧与 MCP stdio seam 同款 local 回退）。新增 Files trait 默认方法 `remove_file`（保持 `std::fs::remove_file` 的 NotFound/EISDIR 失败形状，patch delete/move 腿用）与 `write_file`（不隐式建父目录，patch move-write 腿用），以及 `FilesError -> io::Error` 映射（NotFound→NotFound、WrongKind(dir)→EISDIR 21、Failed→原始 io 字符串），本地驱动行为逐字节不变（LocalFiles 即原 std::fs 路径）。read 的分块读上限（256KiB chunk / 20MiB media）恒严于 Files 契约的 64MiB 读上限，实际生效的始终是工具自身上限。
- 2026-11-01 consumer 迁移 part 2（lane9b）——G-1 consumer 侧收口 ✅：
  - tool/shell.rs：spawn 前的工作目录存在性/类型校验从 `std::fs::metadata` 改走 `ToolServices.files` 的 follow 语义探针 `type_following`（location 绑定 Environment Files seam；本地驱动行为与原 follow-symlink metadata 等价——符号链接目录通过、悬空链接报 NotFound→"does not exist"，错误文案不变；`Files::stat` 本身是 lstat 语义，不可直接用）；cwd 默认值仍为 location 目录（services.directory，与 ref 一致），exec-defaults 协议（44/45/46）未动。
  - oc-core/pty.rs：portable_pty spawn 机制（openpty/reader/writer/spawn_command/clone_killer）原样移入 `environment/pty.rs` 的 `PtySpawner` seam（`LocalPtySpawner` 本地驱动 + `FailingPtySpawner` memory 驱动对位），Driver 增加 `pty` capability，`Environment.pty(location)` 按位置解析；`PtyService.create` 走默认（本地）spawner，新增 `create_with` 供按位置传入；会话机（缓冲/订阅/事件/保留上限）不变，killer/resizer 经中性 trait（`PtyChildKiller`/`PtyMaster`）。
  - oc-server/handlers/pty.rs：`POST /api/pty` 创建经 `state.environment.pty(location)` 解析（与 Files seam 同款 bind 失败→local 回退）；list/get/update/remove/write/attach 会话操作不经 spawn，保持原样。
- 剩余已知非迁移（文档化决策）：grep/glob 的 rg walker 子进程 spawn 仍在 host 侧（仅搜索路径探针经 Files seam；walker 属 rg 内部实现，迁移它超出 Files 协议面）；persistent_pty 仍是独立移植路径（ref 侧同样独立于 `Pty.Service` spawn 面），其迁移待非 local 驱动注册后按需进行。
- 已知有意残留（workspace.rs 模块文档列明）：ref 的共享 provision/连接池/wake wrapper 仅在非 local provider 注册后才有意义。

### G-2 ✅ 插件 guest 桥 3 个 hook 域已桥接
- `permission.evaluate` / `aisdk.sdk` / `aisdk.language` 已在 `oc-core/src/plugin_apply.rs` register_guest_hook + `oc-plugin-host/src/host/host.js` 白名单落地；`ctx.permission.hook` / `ctx.aisdk.hook` 可用；真 bun guest 集成测试覆盖。
- 已记录偏差：aisdk 句柄 copy-in/out（guest 返回的 JSON sdk 成为新 opaque 句柄；language 句柄始终透传）；guest 显式 `sdk: null` 视为保留原句柄（serde 无法区分 absent/null，已在 codec 注释记录）。

## 中优先级

### G-3 ✅ KV.scan 已移植
- `Tx::kv_scan` + `Database::kv_scan`（`oc-core/src/database/schema.rs`）：prefix 下界 + prefixEnd 上界（镜像 ref kv.ts:85-90 含其怪异边界）+ 严格 `>` 游标 + limit+1 探测 + 1..1000 钳制；5 个新单测。插件侧 scan 面本已存在（hooks.rs DatabasePluginStorage::scan）。

### G-4 ✅ worktree 策略服务已移植
- `oc-core/src/project.rs` worktree_strategy 模块：Strategy trait（create/remove/list）+ GitStrategy（对齐 ref worktree/git.ts，--detach 保留）+ 注册表（discovery 反序）；`claim_worktree`/`remove_worktree` 移植 ref setWhere 语义（发现仅认领 strategy IS NULL 行，replace:true 例外）。
- oc-server worktree create/refresh/remove 全部改经注册表，wire 形状保持。插件注册自定义 strategy 仍不支持（ref 经 plugin editor；本注册表为主机侧最小面）。

### G-5 ✅ 插件运行时配置变换链已移植（agent state 面）
- `oc-core/src/state_transform.rs`：AgentState/Editor/Transform + run_agent_transforms 对齐 ref state.ts:172-194（注册顺序、grouped 失败→整组禁用+从新候选重启、ungrouped 失败→传播）；plugin_apply 快照后运行，失败隔离。
- 有意偏差（docs/design-g5-state-transform.md）：eager 重建、仅限 state transform 组、host-side only（guest 桥禁止阻塞 IPC）。

### G-6 ✅ TUI composer 多 tab UI 与 `@`-reference 补全已移植（忠实移植）
- composer 三 tab picker（Subagents/Shell/Terminals）：注册门控、每 tab 提示/空态/wrap 语义、每 tab 键位 enable 层（修复 Shell tab 上 return 误解析为 subagent.select 的真 bug）；body 窗口滚动+选中居中（ref scrollbox 对齐）。
- `@`-补全新增 Reference::Source 条目（skills→references→agents 顺序、前导空格描述、FileMention uri-dedupe、frecency bump）；`/help` 入口已核验可达（command palette + DialogKind::Help）。
- 已记录偏差（代码注释）：Shell 输出查看器打开时取一次（ref 每 1s 轮询）；reference 行按击键重挂载；无绑定的 ctrl+d 被吞不落穿；FileMention 以 name 为去重键（无 uri）。
- 2026-11-02 第十轮勘注：本条"忠实移植"仅覆盖静态注册表面——`@`-file.find 实时模糊搜索（异步半边）未移植（autocomplete.rs:23-24,483-484,819 注释自认），见 G-127；Terminals tab 仅注册 3 键位、行数据层空壳不可达（keymap/table.rs:837-843 注释自认）。

### G-7 ✅（部分：generateObject 已移植；evaluation lane 有意不移植）
- `LLM.generateObject` → `crates/oc-ai/src/llm.rs`：强制合成 generate_object 工具 + ToolChoice::named 走既有 route/protocol 管道；ref 一致的错误文案；JSON-Schema 子集校验器（比 ref dynamic 模式更严，无新依赖）；取首个匹配 tool call（ref find 语义）。
- ℹ️ experimental evaluation lane（evaluation/evaluation-client/system-one + 3 recording 目录）有意不移植：ref 主流程不消费（G-17 同类裁决）。

### G-8 ✅（可落地部分已落地；三包维持有意不移植）
- ✅ WS 代理：`oc-ai/src/route/transport/proxy.rs`——scheme 优先级选代理（wss/https→HTTPS_PROXY/ALL_PROXY 等）、NO_PROXY（* / 点后缀 / 端口钉定，含隐式默认端口 443/80 的 effective-port 匹配）、手写 HTTP CONNECT（15s 预算、8KiB 上限）、隧道流交 client_async_tls_with_config；真桩代理 WS 端到端测试。https/socks 代理 URL 以明确错误拒绝（CONNECT-only 范围）。
- ✅ `OPENCODE_DISABLE_FFF`（ref truthiness 解析，honored——钉定纯 ripgrep 路径）与 `OPENCODE_SIMULATE`（boot 一行 "not supported in this build" 提示）已成为功能性开关（server_process_env.rs + boot.rs）。
- ℹ️ 有意不移植：`@opencode/plugin-browser`、`@ff-labs/fff-*`（纯 ripgrep 路径即目标行为）、`@opencode/simulation`、gitlab `discoverWorkflowModels`、Windows `ShellSelect.configured`（本构建非 Windows 目标）。

### G-9 ✅ Mercurial（hg）vcs 管道已移植
- oc-server handlers/project.rs：hg 适配器对齐 ref plugin/vcs/hg.ts——HGPLAIN、`status -0` 映射（A|?→added，R|!→deleted）、working diff、branch 模式经 `ancestor(., default)`（default 分支上返回 []）、committed/显式 base → 503 DiffError（ref 文案）、`hg cat` 合成删除补丁 + `?` 合成新增补丁、patch 预算、截断时仅丢弃尾部不完整 chunk（ref splitGitPatch 语义）；`/vcs/base` on hg 返回 200 null（ref vcs.ts:198-201）。
- 桩 hg 脚本 wire 测试（parity_vcs_wire::hg_vcs_wire）覆盖全部模式。
- 已知偏差（代码注释）：scope 恒为项目 canonical（与既有 git 路径一致的简化），nested location 不按子目录裁剪。

## 低优先级

### G-10 ✅ PTY WS 无效 UTF-8 已对齐为丢弃
- `oc-server/src/handlers/pty.rs`：decode_pty_input 镜像 ref protocol.ts:30-36 fatal-decode-or-drop——整 chunk 丢弃（有效前缀也不转发）；replay 分块按字符边界切 64KiB 帧。

### G-11 ✅ CORS `oc://renderer` 已改为前缀匹配
- `oc-server/src/middleware.rs` starts_with("oc://renderer")（与 ref cors.ts:15 逐字节一致，含其对 `oc://renderer-evil` 的同等宽容——parity 优先）。

### G-12 ℹ️ `context_management.edits[].type` 硬编码 `"compact_20260112"`
- `oc-ai/src/protocols/anthropic_messages.rs:390,1791`。注意：入站侧同时校验并 fail-fast（非该类型直接报错），**不会静默丢弃**；但新增 edit 类型需手改两处。（本轮未动——ref 侧同样单一类型。）

### G-13 ✅ oc-syntax 已知降级（引擎已收敛；injection=ref 同款未启用）
- markdown 代码围栏无 injection 高亮（`oc-syntax/src/lib.rs:43-45` 明示）；csharp/kotlin/swift 单独 feature、extra 14 语言 `langs-extra` feature（体积上限 67.6 MiB 裁决 R-01b）。
- 2026-11-04 勘注（lane D1d）：高亮引擎已收敛至 opentui `TreeSitterClient`（oc-syntax 降格为 grammar+query vendor，目录经 `add_default_parsers` 注册，调用点走 `highlight_once`+UTF-16→char 偏移转换；goldens 零 diff）。injection 状态未变：vendor 目录本就不含 injections.scm（markdown 注册项 `injections_query: None`），围栏内层语言仍渲染纯文本——该缺口维持本条 ℹ️ 不变。
- 2026-11-04 Gate D1 终裁：**G-13 全闭（ℹ️→✅）**——ref 的 `parsers-config.ts:154-158` 把 injections URL 与 injectionMapping 本就注释（ref 同样未启用 injection 高亮；围栏内层语言在 ref 走 markdown code-block renderer 路径，非 tree-sitter injection）。不加 injections.scm（加了反而偏离 ref）。本条剩余子项（语言 feature 门控/体积上限 R-01b）为发布裁决维持不变。
- 2026-11-10 勘注：体积护栏未机械化（CI 无测量/门，重评条件无法执行）已立目 **G-330**——本条重评前提的基建盯守位。

### G-14 ✅（retention 可配置已落地；审计轨迹差异维持有意）
- ✅ `experimental.file_retention.cleanup_interval`（毫秒；absent/0 → 每小时默认，0 守卫防 tokio interval panic）+ 调用方 Duration 透传（`cleanup_with` / `cleanup_data_root_with_retention`）；EngineOptions + boot 播种。oc-schema/config/normalize 按既有 knob 模式。
- ℹ️ event audit：维持 bus registry + SSE manifest（ref JSONL 落盘）——架构性有意决策，本地审计轨迹差异已文档化。

### G-15 ✅ 死依赖已清理
- oc-core：`once_cell`、`glob` 已移除；oc-cli：`dirs` 已移除。勘误：`unicode_width` 实际从未声明（原审计条目过时）。

### G-16 ℹ️ 长期漂移风险（维护坑，未变）
- `oc-schema` 手写 serde 类型与 ref Effect Schema 平行维护，无生成器，靠 goldens 防漂移——最大长期漂移面。
- provider 折叠架构：新增厂商需同时改 `provider_catalog` static_entries 与 `variant.rs:1089-1123` 映射表两处；建议与 ref `providers/index.ts` 厂商清单建立自动对照测试。
- `/openapi.json` 为 sha256 钉死的静态文档；ref 升级后需人工走 baseline-bump 流程（有 sha 断言兜底）。

### G-17 ℹ️ 有意省略（正确决策，无需补；未变）
- ref 死代码：`mcp/resolve.ts`、`plugin/inventory.ts`；ref 的 `ignore` 依赖在其源码中也无 import。（勘误：`debug/redact.ts` 并非死代码——ref 调用链完整 `cli/src/index.ts:41 → handlers/debug/config.ts:8,16`，见第五轮 G-52。）
- Web/GUI/云生态：web/desktop/enterprise/console/app/posts/storybook/identity/containers/function 等整包不移植（codemode/latex/merman/simulation 的**功能本体**已在 oc-core/oc-tui 内实现或按上文明示处理）。
- rust-extra 超出 ref 表面的部分（`upgrade -m local-file|github`、completion、`--force`、run 的 TTY 权限提示、serve hostname 解析报错、`$PWD` 陈旧值守卫等）均为防御性收紧且有注释。run 的**非 TTY** 路径与 ref 一致（noninteractive.rs:289 auto-reject）。
- 2026-11-02 第十轮归并：tui feature-plugins/system/storybook 工作台（~1400 行开发期 fixture）同属本条整包不移植族；workerd 运行时目标与 ServerFetch.make（结构性不可移植，Rust 仅原生 boot_and_serve）收编为本族文档化范围裁剪（登记为 G-163 ℹ️）。

## 本轮 burndown 之外的已知残留（后续工作）
- G-1 consumer 迁移：tools/handlers（read/write/edit/shell/glob/grep/patch、mcp/stdio、pty）尚未切换到 Environment Files 服务（服务本体与接线已就绪）。
- G-4 插件注册自定义 worktree strategy（ref plugin editor 面）。
- G-6 Shell 输出查看器 1s 轮询（现一次性拉取 + 64KiB tail）。
- G-8 https/socks 代理 URL 支持（TLS-to-proxy / SOCKS 握手）。

---

# 第三轮全量对比审查（2026-10-30）

以下 G-18..G-40 均经两侧源码二次取证确认仍存在。风险排序：🔴 高 / 🟡 中 / 🟢 低。

## 🔴 高

### G-18 ✅ bedrock-converse thinking block_binding 默认值缺失
- ref `packages/ai/src/protocols/bedrock-converse.ts:485-503,562-563`：Claude 5.1+ 注入 `adaptive + drop_block + anthropic_beta: thinking-binding-controls-2026-08-01`。
- rust `crates/oc-ai/src/protocols/bedrock/converse.rs:630-685`：thinking 仅从 provider_options 透传，无 block_binding/adaptive/drop_block/anthropic_beta 任何痕迹（同仓 `anthropic_messages.rs:1493-1518` 已有 `applyThinkingBindingDefault` 能力，未接到 converse 路径）。
- 后果：Bedrock Claude 5.1+ 会话中 system 提示变更 → `Invalid signature in thinking block` 硬错误，长会话断流。fixture `test-fixtures/recordings/bedrock-converse-thinking-binding/` 已拷入但无测试引用（死 fixture，可直接补测试）。

### G-19 ✅ bedrock-mantle-messages 路由整条缺失
- ref `packages/ai/src/providers/amazon-bedrock-mantle.ts`：anthropic-messages 协议跑在 mantle host（含 effort 更新、haiku system 降级、签名 thinking 回放，4 组 recorded fixture）。
- rust `cache_policy.rs:44` 白名单留名，但 `ProtocolFamily`（provider_catalog/facade.rs:269-272,391-394）仅有 BedrockMantleChat/Responses，无 anthropic-messages-on-mantle 变体；4 组 fixture 同样已拷入无引用。
- 后果：Bedrock Mantle 上 Claude 的主推理通道不可用。**修复优先级最高（与 G-18 同组）。**

### G-20 ✅ IDE/编辑器实时集成组缺失（高）
- **WebSocket 集成**：ref `tui/src/context/editor.ts:107-376`（`~/.claude/ide/*.lock` 发现、`CLAUDE_CODE_SSE_PORT || OPENCODE_EDITOR_SSE_PORT`、JSON-RPC selection 推送、`x-claude-code-ide-authorization`）。rust 全仓 grep 零命中；仅有 Zed sqlite 轮询（editor_zed.rs:83-95）+ `$EDITOR`（editor.rs:6）。守卫基线（tools/guards env_ref.txt:34）记录了该 env 但无实现。
- **terminal.suspend 死键**：键位表绑定 ctrl+z（oc-tui-core/keymap/table.rs:786），但 oc-tui 无任何 SIGTSTP/suspend 分发臂（ref app.tsx:1120 有）。按键被消费无动作。
- **session.environment 不上报**：ref app.tsx:489-499 就绪后 PUT；rust 仅 .gen 生成代码存在（methods.gen.rs:3633），无调用方。
- 三项同组：VS Code/JetBrains 编辑器上下文（@-reference 选区）完全不可用。

### G-21 ✅ tui.command.execute / tui.session.select 事件未处理
- schema/总线/客户端侧齐全（oc-schema/events.rs:306-362、oc-client/bus_event.rs:144,409、bus/registry.rs:362-367），但消费侧仅 `tui.toast.show`（store.rs:10853）。ref app.tsx:1253-1274 将其分别接到 keymap.dispatch 与 route.navigate。
- 后果：插件经 `ctx.event` 发的命令/路由定向指令被静默丢弃——插件 API 契约级缺口。

## 🟡 中

### G-22 ✅ ProviderContext/路由 hook 不兼容防护缺失（主请求路径）
- ref `model-request.ts:304-318`：每请求检查 model.request hook 改写 baseURL 是否与含 compaction part 的 opaque window 兼容，不兼容即 die。
- rust 仅压缩路径有防护（compaction.rs:434-442）；主路径 model_request.rs:747-788 应用 RoutePatch 后无任何 provenance 检查（grep 0 命中）。
- 后果：改写 baseURL 的 hook 可把加密 compaction window 发往另一部署，产生不可回放历史且无 die 信号。

### G-23 ✅ session.generate hook 定义但零调用（含死注册）
- rust `model_request.rs:653-673`：`RequestKind::Generate` 落入 `dispatch_session_context`；`dispatch_session_generate`（hooks.rs:891）全仓 0 调用点。
- 更糟的是已有 3 处注册（engine/mod.rs:2127,2255、tool/patch.rs:561）——注册了永不派发的 hook。需追查这些 handler 期望的触发点，可能已是功能性 bug 而非纯接口缺口。

### G-24 ✅ 插件 host 域缺失：websearch / vcs / worktree transform + integration methodImplementation
- ref `plugin/host.ts:468-490`（vcs get/base、websearch providers/query/reload/transform）、`:631`（integration 方法注册通道）。
- rust oc-plugin-host hook 域仅 `session|chat|tool`（context/mod.rs:120-132）；grep websearch/vcs/worktree 零命中。oc-core 有内部 OAuthImplementation 注册表但无插件暴露通道。与 G-4 残留（worktree strategy）同族。

### G-25 ✅ pty connect-token Origin 校验忽略端口（未复用既有实现）
- rust `oc-server/handlers/pty.rs:191-207`：Origin 与 Host 均剥端口比较；ref（middleware.ts:66）含端口精确比较。auth.rs:210-227 的 `origin_host` 本就是端口敏感实现但未复用。
- 同主机不同端口页面可通过检查；利用受 ticket 单次+60s TTL 缓解。修复即改为复用 `is_allowed_request_origin`。

### G-26 ✅(bounded) reqwest 0.12/0.13 双栈证书验证不一致
- Cargo.lock:3213/3255 双版本共存（有意，oc-core/Cargo.toml:44-50 有注释）。两栈同走 rustls 0.23（aws-lc-rs provider），但 0.12 栈用 `webpki-roots`（Cargo.lock:3243）、0.13 栈用 `rustls-platform-verifier`（:3277）。
- 后果：企业自签 CA/中间盒环境下两栈连接行为不一致（0.12 栈拒绝系统信任证书）。与 G-8 同属传输层债务。

### G-27 ✅(bounded) ACP vendored patch 无上游 drift 检测
- third_party/agent-client-protocol（crates.io 3.3.0 去 preserve_order 单点改动；ref 用 @agentclientprotocol/sdk 1.6.0——版本代差本身也是对齐风险）。CI（ci.yml/runtime-gates.yml）仅有 OpenAPI 与 vendored .scm drift 检测，无 ACP 上游 diff 检测。
- 上游发版需手工重打 patch，否则 feature unification 静默翻转全 workspace serde_json Map 顺序。属 G-16 族，单列因有具体 CI 补法。

### G-28 ℹ️（文档化决策：插件 TSX 渲染为有界 shim，兼容性降级）
- rust 有 JSX 面（host.js:1857-1900 @opentui/solid shim 的 createElement/setProp/insert + supervisor.rs:1051-1111 ui.slot.register/router.register），但渲染树跨 IPC 时拍平为 `{title, rows, clickable}` 有界结构（supervisor.rs:964-966）。无任意组件树/markdown code-block renderer 注册面。
- 行式插件（sidebar 类）可跑；富 TSX 布局插件无法忠实渲染。ℹ️ 属有界化设计而非疏漏，但需作为插件生态 breaking change 显式声明。
- 2026-11-10 勘注：族成员清单转正入册——**G-332**（插件 markdown 代码块渲染器注册链缺失，第十九轮 L16 发现原仅归类本条备注）、**G-333**（node 运行时备轨不移植，bun-only+托管下载兜底）；另有 G-148②（effect API）、G-289（单 guest per-Location）、G-313（npm 启动安装）为族内既有条目。发布说明应按此清单声明生态面。
- **2026-11-12 增补轮勘注（族面收窄）**：G-332（markdown 渲染器注册链，有界行形状）与 G-331/G-239（session.panel slot+PanelInput，有界 claim 形状）、G-333（node 备轨，FEASIBLE-BOUNDED）均已落地——发布说明的生态 breaking 声明面相应收窄为：TSX 渲染保真度（panel/代码块内容为有界行形状而非任意 renderable 树）、G-148②（effect 导入面）、G-289（单 guest per-Location）、G-313（npm 启动安装）、node 轨加载限制（无扩展名导入/.tsx）。

### G-29 ✅(bounded) TUI 插件管理页：安装/更新/检查流无操作
- rust command_palette.rs:2905-2909 有插件列表+active 翻转+错误详情；但 `plugins.install | dialog.plugins.install | update | check` 显式 `Dispatch::Handled` 无操作（:3033-3047，注释 "registered-but-surfaceless"）。ref system/plugins.tsx 358 行完整管理 UI。CLI `opencode plugin add` 可绕过。

### G-30 ✅(bounded) 快照用 git2 且无 opencode.gitconfig 等价配置
- ref git.ts:23,28（`autocrlf=false` 的 opencode.gitconfig）、:535（`-c core.quotepath=false`）。rust snapshot.rs:248-278 用 libgit2，grep gitconfig/autocrlf/quotepath 零命中——还会不受控地继承宿主机 `~/.gitconfig`。
- 后果：CRLF 检出工作区快照 diff 整文件误报；非 ASCII 路径引用语义不一致。低成本修复：git2 `Repository::config` set_str。

### G-31 ✅ 文件模糊搜索索引 + VCS 根判定（lane8 收口：resolver 判根 + 内存索引 10s 刷新）
- ~~rust oc-server/handlers/fs.rs:254-330：每请求 spawn_blocking 全量 walk；SCAN_ENTRY_BUDGET=100_000 统一上限~~ lane8 已收口：`is_vcs_root` 改用 project resolver（`services.project.vcs` + canonical 根相等，hg 仓库生效；仓库子目录 location 保持 100k 保守语义，与旧 `.git` 嗅探一致）；`/api/fs/find` 改读进程级 `SearchIndex`（AppState 字段）——按 canonical root 键控、10s 后台刷新（spawn_blocking walk，失败保留上次好索引）、首访同步构建（build-on-first-miss）、并发请求 single-flight；非 VCS 根 100k 预算在**索引构建时**施加。排名/打分语义不变（仅候选来源改为索引）。
- 残留偏差（文档化）：VCS 判根沿用「检出根才算」的保守语义（ref 的 `location.vcs` 对仓库内任意目录都为真值）；失效策略为 root 键控 + 新鲜度窗口重建（ref 亦无显式 config 失效钩子），未做显式 config-reload 钩子。fix1 加固：single-flight claim 由 Drop guard 持有（取消/panic 不泄漏）+ 首建等待有界（30s）；slot 自淘汰（根目录消失或 5 分钟无访问即驱逐、停刷，rebuild-on-miss 保证安全）；后台刷新每 tick 重读最新请求参数（budget/exclusions 不再冻结于 spawn 时）。另注：常驻 10s 后台刷新任务是本 lane brief 指定项（ref 实为按需 refresh-on-access），额外稳态开销已知。

### G-32 ℹ️（文档化决策：v1/legacy schema 有意不移植，纯 v2 面无影响）
- oc-schema/src/lib.rs:80-82 TODO 仍在：legacy-event、v1 session 事件、models-dev refresh、rpc.ts、pty-ticket、durable-event-manifest。仅影响旧数据互操作/回放，纯 v2 面无影响。
- 2026-11-02 第十轮复核备注：上述五个 schema 子项经逐项核验**全部在本条明文范围内**（G-87 已裁决过未明确子项，维持 DUP）；另 run 的 v1 事件输出兼容层（ref run/v1.ts：pendingStep/v1InvalidOutput/permissionReject 静默等）同属本裁决范围——消费 v1 事件流的脚本不受支持。

## 🟢 低

### G-33 ✅ shell 输出截断上限 config 失联（热重载半边由 G-48 lane4 收口，全项 ✅）
- schema 能解析 `tool_output.max_lines/max_bytes`（oc-schema/config/tool_output.rs:10-15、normalize.rs:550），`ToolOutput::configure` 也存在（tool_output.rs:209-214），但 engine 从不调用（engine/mod.rs:488 仅 `ToolOutput::new`）——config 键解析后全程无人消费；shell.rs:382-384 直接用编译期常量。默认值与 ref 一致，仅显式配置失效。修复即 boot 时 `configure(Limits::from_config)`。

### G-34 ✅ session.new_location (launch/inherit) 未消费
- 字段已有（oc-core/cli_config/mod.rs:192-193,405）、配置对话框可编辑（dialog_config.rs:193），但 command_palette.rs:2192-2197 `"session.new"` 无条件回 Home。ref config/new-session-location.ts：inherit 回当前 LocationRef。

### G-35 ✅ composer 运行态 knight-rider 渐变 spinner 缺失
- ref ui/spinner.ts:272-368（createFrames/createColors）+ prompt/index.tsx:1892 `<spinner interval=40>`；rust composer 域无帧生成器（render.rs:74-88 仅文本 hint）。mini 面有 spinner（lifecycle.rs:552-674），主 composer 无。纯视觉反馈。

### G-36 ✅(bounded) mini demo 剧本覆盖 6/13
- rust mini/demo.rs:29 KINDS 6 项；ref demo.ts:34-48 13 项（缺 motion/shell/write/edit/patch/subagent/question）。演示与回归覆盖面缩减。

### G-37 ✅(bounded) session.message.get O(n) 全量扫描
- oc-server/handlers/message.rs:156-165：拉全部消息再线性 find。正确性无误，长会话性能退化；建议走 store 按 id 直查。

### G-38 ℹ️ OTel 默认关闭
- oc-server/Cargo.toml:11-17：`otel` feature 无 default（发布构建不拉遥测依赖）。可观测性弱于 ref（ref 默认开），属发布决策。

### G-39 ℹ️ 剪贴板写仅 OSC52（有意）
- clipboard.rs:208-236：host 写臂恒 Unsupported，注释明言 "v2 parity is the OSC 52 terminal write"；读臂健全（pbpaste/xclip/xsel/wl-paste + OSC52 查询）。不支持 OSC52 的终端复制静默失败。

### G-40 ℹ️ 记录性差异（无行为风险）
- `x-opencode-build` 响应头（info.rs:17，ref 无）——golden 对比时需归一化。
- 空分页 cursor 显式 `null` 而非省略键（message.rs:125,132）。
- Markdown 语法 injection 缺失与语言 feature 门控见 G-13。

## 勘误（第三轮初报经核验撤销的条目）
以下初报缺口核验后确认**已实现或原本不存在**，勿再列入待修：
1. **桌面/权限通知**：notifications_plugin.rs 是 ref notifications.ts 完整移植（permission.asked/form/execution 事件 + os_notify OSC9/777/99 + bell 回退，app.rs:6857-6871 接线）。
2. **npm registry 插件安装链**：oc-plugin-host/context/install.rs 经 npm CLI 子进程支持 registry + git specifier（`npm view` 探测、frozen 安装、缓存布局）；与 ref 的 arborist/pacote 仅机制不同。
3. **插件 storage KV**：StoragePort 四方法全分发（supervisor.rs:55-70,374-375）+ DbStoragePort 绑定（oc-server/plugins.rs:371-378 → hooks.rs:1281-1342，`plugin:<hex>:` 命名空间）。旧 TODO 注释已过时。
4. **wellknown/credential 联动**：HostWellknownSource（integration/wellknown_config.rs:79）经 engine/mod.rs:884-885 注入；600s 刷新循环 + CREDENTIAL_SWITCHED/INTEGRATION_UPDATED 事件触发均在（engine/mod.rs:563-580）。NoWellknown 仅测试默认。
5. **formatter 命令执行**：BuiltinsFormatter 真实执行（formatter.rs:793-875，26 个内置探测）且生产已接线（engine/mod.rs:1153）。
6. **`--session-output` 调试流**：ref 侧本就无此实现，初报无依据。
7. **bus replay 键序敏感**：幂等跳过按 event.id + seq（bus.rs:984-997），不比较 payload；workspace serde_json 不开 preserve_order，键序无关。
8. **pair --url 校验时机**：handler 侧 validate_pair_url 完整（misc.rs:246-260，RB-03 对齐 ref），仅 parse/handler 时机差，行为无影响。

另有 1 条修正归类：**MCP stdio 本机 spawn** 并非新缺口——environment/ 服务本体已移植（G-1 ✅），缺的是 mcp/client.rs 的 consumer 接线，已登记在 G-1 残留清单；当前唯一 driver 为 local，行为与 ref 等价，接入非本地 workspace driver 前必须先切线。


---

# 附：开发规范与现行制度（原「踩坑记录」，2026-11-06 起易名；自 a26e145 重写前版本恢复，2026-10-30）

> a26e145 将本文件从 520 行登记册重写为 G-1..G-17 摘要时移除了原 §2/§3（踩坑记录与制度速查）；历史全文见 `git show db21631:docs/GAPS-AND-PITFALLS.md`。本节为现行开发规范——2026-11-06 由「踩坑记录」倒转为规范主体并压缩（事故依据删除；带依据版本见 `git show 298dbcec:docs/GAPS-AND-PITFALLS.md`）。

## 开发规范

### 2.1 构建门禁与资源

1. **cargo 构建/测试一律经 `tools/dev-gate.sh` 单入口**：强制 eternal130 身份 + systemd scope（CPUQuota=400%、MemoryMax=10G、TasksMax=512），1 分钟 load≥6 拒启动；lane 提示词必须写明走 dev-gate。
2. **dev-gate 只包单条 cargo/nextest 命令**：`tools/diff/run-all.sh`、`tools/parity/*.sh` 等验证脚本**直接跑、禁包 dev-gate**（锁套锁）；一次一个门，禁双发。
3. **一切测试/构建命令全量 tee 落盘**——不止收尾门，**含过滤词单测、`--exact` 复现、fix 迭代**——到 `.test-logs/<crate>-<时间>-<lane>.log`（git-ignored），禁内嵌 tail/head/grep 截断 Summary；显示用 grep 在 tee 之后单独执行。落盘日志是唯一测试证据：**日志丢失=证据丢失，直接导致重复测试与归因事故**（第十四轮教训，2026-11-07）；争议先读盘上日志，禁凭记忆重跑（见 2.2#5）。
4. 门排队/卡住先 `fuser /tmp/opencode/dev-gate.lock` + `ps` 查残留（含 PPID1 孤儿）再归因。
5. **禁 root 直跑 cargo**；root 编辑过的文件立即 chown；worktree 动工前 `find . -user root -exec chown eternal130:eternal130 {} +`（含 target/out/oracle）。
6. 磁盘：仓库级 incremental=false 已强制；lane 合并后立即删该 worktree 的 target（过清理门：fuser 无打开文件 + mtime≥30 分钟）；主 target/debug ≥两周或 >100G 时 `cargo clean -p` 粗清或整清。
7. 生产尊重：单 lane 串行；重型腿（E2 TTI/soak/电池）需用户明示或错峰，派发前看生产健康信号；配额敏感任务用定时唤醒 + 断点复活（WIP 落盘可 revive）。

### 2.2 测试执行与证据

1. canonical harness=**nextest**（.config/nextest.toml：test-threads=4、全局 retry=1、be1/pty 家族 retry=2）：全 workspace 用 `cargo nextest run --workspace --locked --no-fail-fast`；PTY 家族另跑 `--profile pty`；**禁 plain `cargo test`** 的红作回归证据（`--exact` 单测复现除外）。
2. 首轮**不过滤** + `--no-fail-fast` 拿全量结果；报告**原样引用 `test result:` 行**（禁只写 green/通过）；红测试**同轮** `--exact <name> -- --nocapture` 取回输出。
3. 终报**从磁盘核实**：`ls -t .test-logs/` 定位真实文件 + `grep -cE 'FAIL'` 核对失败计数；只引 `ls` 出的路径；终报贴 `git log -1 <branch>` 落地证明，缺证明=未完成。
4. flake 白名单（重试即过、勿误判回归、不作合并门单独阻塞）：`composer_tabs_walk` / `location_ttl_sweeper` / `mini_subagent_footer` / `scenario_permission_saved` / `e1_cells_paste_8k` / `e1_canary` / `catalog_refresh_propagates`。`e1_cells_paste_8k` 勘注（2026-11-07）：并发满套件常双败、序列化过、跨构建非单调（盘上二分证据见第十四轮）——放行依据=落盘的序列化单跑通过。勘注（2026-11-08 第十六轮受控复现）：**该 canary 逐构建翻转与代码语义无关**——a7d0caa4 原构建序列化 PASS×2（含 load 8.9 下）、同树+仅 2 行零语义注释重编即翻红；d1bb497a 全部内容回退组合（±project.rs/±repository_cache.rs）均红；失败形状恒为 typing-idle 单格恰 1 burst（bursts=1≠0），系二进制布局偏移使一次性唤醒落入 2.5s 采样窗。**序列化单跑通过不再是普适可得状态**，放行依据=白名单+受控证据链（日志 `.test-logs/e1-bisect-*`，判别实验见第十六轮节）。勘注（2026-11-10 第二十一轮）：CI 无白名单机制，run 42809491371 在 CI 构建上 3/3 触发同形状（typing-idle bursts=1、六格全 0、cadence 健康）——噪声地板已**内联进测试断言**（spec_ef2_e1_cells.rs：总 burst ≤1 放行、≥2 仍硬失败，注释引本条），本地与 CI 统一适用；该阈值即本白名单裁决的可执行化，非放松回归信号。
5. 基线日志必须绑定采集时 `git rev-parse HEAD` 树 hash（无 hash=无效基线）；测试争议先读落盘日志，禁凭记忆重跑。
6. oracle 三可证：版本硬断言 + 采集指纹（FNV-1a 双语镜像）+ always-refresh；金帧只证渲染保真，组合/路由模型必须参照源码级对比（file:line），不许 golden 自证。
7. **测试分层**（按改动范围选层，先轻后重；门命令**逐字内联** lane 提示词，禁「模板同前 lane」引用）：

   | 场景 | 命令 | 耗时 |
   |---|---|---|
   | 日常迭代（oc-tui 单点） | `tools/dev-gate.sh 'cargo nextest run -p oc-tui --locked <过滤词>'` | 秒~分钟 |
   | 渲染面·常设门（bun-free） | `tools/dev-gate.sh 'cargo nextest run -p oc-tui --locked dual_engine_matches_frozen_oracle'` | 0.1s |
   | 渲染面·opentui 差分全套（127 场景含 TS oracle 刷新） | `cd /opt/opentui-rust && tools/diff/run-all.sh`（直接跑，见 2.1#2） | ~36s |
   | 合并门/收口 | `tools/dev-gate.sh 'cargo nextest run -p oc-tui -p oc-cli -p parity --locked --no-fail-fast'` | ~7min |
   | PTY/交互族 | 合并门之上加 `--profile pty`（test-threads=1 序列化） | ~28min |
   | core/其他 crate | `tools/dev-gate.sh 'cargo nextest run --workspace --locked --no-fail-fast'` | 视范围 |

8. 树场景：加 `StateSpec`（`crates/oc-tui/tests/tree_scenarios.rs`）→ `tools/parity/tree-scenario-capture.sh` 捕获 → `TREE_ORACLE_FREEZE=1 tools/parity/tree-oracle-freeze.sh` 冻结；**oracle 帧与冻结版不符=重钉事件，stop-and-escalate，禁擅自重冻结**。

### 2.3 lane 纪律

1. 每 lane 独立 git worktree（仓库内 `.worktrees/`）+ 串行合并回 main；**禁碰 main checkout**；只碰自己 scope 的文件；收尾自查 `git status`。
2. 长任务=前台分块 ≤45 分钟（**单次工具调用同硬限**）+ 追加式日志 + 每 6 轮 WIP 提交（断点可续）。
3. 子代理禁后台任务（全部前台块）；禁向已完成会话投递；小时级后台睡眠改级联短看守（50 分钟自续）+ 用户消息兜底。
4. lane 收尾自查自派生后台进程（watchdog/sleep 等）；rig 换代码必须重建（收尾强制项）。
5. /tmp 顶层只用 PID 唯一名，未提交工作禁放 /tmp；`/tmp/opencode/dns-probe.*` 不是构建产物勿清。

### 2.4 进程与终端安全

1. 全仓**禁裸 PID 发信号**：一律 safe_signal（pidfd + boot_id/starttime 世代 + cmdline 精确校验）；组杀=活 leader 钉住 + /proc 扫描 + 逐成员校验；测试只用自有子进程句柄。
2. TUI 二进制只在 vt/PTY harness 内、或 stdout/stderr 重定向 + no-tty 模式启动；tmux/终端矩阵探针只碰自建会话；OSC/转义探针只写自有 PTY。
3. 清理门：mtime<30 分钟视为在用（勿清活动目录）；rig teardown 卫生项常设。

### 2.5 合并与发布

1. main 合并=受影响 crate 构建+测试后再推，**无例外**；harness 相关合并加 bun 内差分冒烟。
2. 合并裁决列表级核对每个 lane 的**删除清单**；证据优先方整文件恢复。
3. 交互面修复必须带 PTY/field 级验证；审计状态只是起点，不证链路端到端。
4. 发布门 scoreboard：每修配独立子代理门扫描，循环全绿才发。
5. 提交：English commits；**一律仓库既有身份（Eternal130 <Eternal130@outlook.com>，四层 gitconfig 已配）——禁 `--author`、`-c user.name/user.email`、`GIT_AUTHOR_*/GIT_COMMITTER_*`**。

## 现行制度速查（防复发汇总）

| 类别 | 制度 | 载体 |
|---|---|---|
| 资源 | 单入口构建/测试（配额+身份+负载闸） | `tools/dev-gate.sh` |
| 调度 | 单 lane 串行；重型腿需批准；生产健康前置检查 | 生产尊重制度（2026-10-18） |
| 终端 | TUI 只在 PTY/vt harness 内启动；探针只碰自有会话 | 终端隔离规则（2026-10-18） |
| 信号 | 全仓 safe_signal；禁裸 PID；测试用自有子进程句柄 | `tools/guards/process_safety.py` |
| 生命周期 | 前台分块 ≤45 分钟；WIP 提交；禁投已完成会话；子代理禁后台 | lane 派发模板 |
| 验证报告 | 首轮测试不过滤+--no-fail-fast；报告原样引用 test result 行；红测试同轮 --exact --nocapture 取回原因 | 本册 2.2#2 |
| 测试证据落盘 | 一切测试命令（含过滤/单测/--exact 迭代）tee 到 .test-logs/<crate>-<时间>-<lane>.log（git-ignored）；日志丢失=证据丢失→重复测试；报告给路径；争议先读日志禁止凭记忆重跑 | 本册 2.1#3 / 2.2#5 |
| 写隔离 | 每 lane 独立 worktree；串行合并；删除清单核对 | 写隔离制度（2026-10-11） |
| 合并 | 受影响 crate 构建+测试后推送；落地证明（git log -1） | 合并门（2026-10-18） |
| oracle | 版本断言+指纹+always-refresh | `tools/diff`（opentui-rust） |
| 磁盘 | 增量抑制（仓库级 .cargo/config.toml incremental=false，2026-10-25 起强制）/周期 clean；清理门 mtime≥30 分钟视为在用 | 本册 2.1#6 |
| 验证 | 交互面必须 PTY/field 级；组合模型必须源码级对比；金帧=回归套件 | 方法论登记（ROADMAP §4） |
| 门命令形状 | 门命令全量 tee 禁内嵌 tail/head/grep；脚本（run-all.sh、parity/*.sh）直接跑不包 dev-gate；一次一个门禁双发；终报与磁盘日志对账（ls -t 定位真实文件+grep -c FAIL 核数） | 本册 2.1#2–4 / 2.2#3 |
| git 提交身份 | 子代理一律仓库既有身份（Eternal130）；禁 --author、-c user.name/user.email、GIT_AUTHOR_*/GIT_COMMITTER_*；派发模板必含禁令 | 本册 2.5#5 |
| 测试分层 | 按改动范围选层：常设门 0.1s→差分全套 36s→合并门 ~7min→pty ~28min；命令逐字内联禁引用；树场景重钉=stop-and-escalate | 本册 2.2#7–8 |

---

---

# 第四轮复审（2026-10-31，gap-burndown-3 收口后）

> 对本轮落地的 24 项修复做 3 通道只读代码级复审（oc-ai / core+server / TUI，逐项对照 ref 源码 file:line 取证）。裁决：20 项 CONFIRMED、4 项 PARTIAL（G-30/G-31/G-34 + G-19 的死校验）。无回归；以下为新登记缺口与半接线残留。

## 既有条目状态修正

- **G-34 ✅（lane4 收口）**：原 PARTIAL 半边已接——`set_home_location` 写通共享 cell，`HttpPromptSink` 按 home_location 解析 home create-directory（inherit → 当前会话目录，ref app.tsx:718-737），home 状态行显示继承目录。
- **G-30 ℹ️（补充）**：repo-local autocrlf/quotepath 钉定当前行为惰性——快照管道全程 raw blob/OID-map/treebuilder/手写 diff，从不咨询 git config；parity 为偶得（raw passthrough 即无 CRLF 误报）。钉定仅防未来 libgit2 操作。ref 另钉 7 个 key（longpaths/symlinks/fsmonitor/untrackedCache/manyFiles/index.version/threads，ref git.ts:23-33）未移植，当前行为中性。
- **G-31 ⚠️（补充）**：`is_vcs_root` 弱于 ref `location.vcs`——仓库子目录 location 与 hg 仓库在 rust 侧仍 100k 截断（保守方向）；handler 手里已有 `services.project.vcs`（fs.rs:260 绑定 `_services`）却用文件系统嗅探。
- **G-19 ⚠️（补充）**：`validate_config`（mantle_messages.rs:199-221）零调用——ref 的 bearer-without-key fail-fast（ProviderConfigurationError）永不触发，rust 静默 SigV4 回退；`auth/profile/region/credentials` settings 键在 oc-core ModelConnection seam（baseURL/apiKey/headers，facade.rs:123-136）无处解析（承自 converse lane 的既有模式）。
- **G-24 ✅（lane5 清偿全部 bounded 外项）**：`vcs.transform`/`vcs.reload` 已接（supervisor 派发 + 注册表合并）、websearch `editor.default.set` 已持久化（引擎 KV）、worktree 注册表热重载已接（`reload_after_update` 后重跑 `plugin_worktree_registry` 播种，`AppState.worktree_strategies` 可热换）。guest 侧 worktree host-op 此前已清偿。

## 新登记缺口（G-41..G-49）

### 🟡 中
- **G-41 ✅ 代理 URL 凭据被丢弃**：已修复——userinfo 解析保留（percent-decode），CONNECT 发 Basic `Proxy-Authorization`，SOCKS5 username/password 认证。
- **G-42 ✅ WSS_PROXY/WS_PROXY 未咨询**：已修复——WS 族优先于标准族（websocket-constructor.ts:43-47 语义）。
- **G-43 ✅ 无 loopback 目标绕过**：已修复——127.0.0.1/localhost/::1 恒直连。
- **G-44 ✅ NO_PROXY bare-host 语义偏差**：已修复——bare host 精确相等、`.`/`*` 前缀后缀匹配、空白+逗号分隔。

### 🟢 低
- **G-45 ✅（lane7）**：acp_drift.py 已按 crate 参数化（每 crate 钉 sha256、独立 pristine env 覆盖 ACP_SCHEMA_PRISTINE_DIR），agent-client-protocol-schema 1.11.0 与主包同样走 upstream+LOCAL_CHANGES.patch 端到端校验，guard 对两包跑通。
- **G-46 ✅（lane5）**：dialog.plugins.error 打开失败插件的 error 详情（ErrorDetails 对话框，esc 回插件列表；无失败行为无操作）。
- **G-47 ✅（lane6）**：连接成功即复位 backoff attempt（editor.ts:223 open 监听）；空字符串 CLAUDE_CODE_SSE_PORT 现按 ref falsy `||` 落到 OPENCODE_EDITOR_SSE_PORT。
- **G-48 ✅（lane4）**：reload bridge 在失效 location 前重导 `Limits::from_entries` 并 `Engine::reload_tool_output_limits`（boot.rs ConfigReloadBridge），assemble 改读 `ToolOutput::current_limits` 活值——改 `tool_output.*` 免重启生效。
- **G-49 ℹ️ 记录性/文档债（spinner 子项收口为文档化决策，G-49a/lane10）**：spinner 帧进阶 ✅ 经核验/钉定——帧索引按渲染时刻的流逝时间取（`composer_spinner_cell(elapsed_ms)`：`elapsed_ms/40 % frames.len()`，knight.rs），ref 的 40ms 帧数学在任意重绘频率下精确成立（含 100ms pump tick 采样：index 恒 = floor(elapsed/40)），单测 `frame_progression_follows_the_40ms_cadence_math` 钉死； repaint 帧率维持 pump tick（100ms）为**文档化决策**——运行循环无 composer 区域级重绘源，全局 40ms 唤醒会扰动泵（共享 PTY harness 以 >80ms 静默窗切快照、已登记的 vt_temporal 时序测试钉定 100ms 节奏、空转字节量护栏），故按 brief 采用时间派生索引这一最小忠实修复；session.environment 每 session 单发（ref 效果重触发，PUT 幂等无害）；stdio.rs 模块头宣称 SIGTERM→SIGKILL 升级但实现为即杀（沿袭 rmcp 行为，措辞与 ref 2s 优雅等待不符）；project.rs:1103-1105 注释"插件 strategy 不支持"已过时（add_strategy 就在其下）；proxy.rs:367 注释"webpki 与 0.13 栈一致"在 G-26 后已失效（0.13 现走 platform-verifier；ws 链路留 webpki 为有意）。2026-11-02 第十轮备注：G-49a 仅钉定 dots 帧数学；TAB_SPINNERS 其余 3 样式（arcs/quadrants/line）、4 种未读标记与 `tabs.indicators` 设置的消费缺失不在其范围，另立 G-143。

## 复审判定汇总
G-18 ✅ / G-19 ✅(死校验残留) / G-20 ✅ / G-21 ✅ / G-22 ✅ / G-23 ✅ / G-24 ✅(bounded) / G-25 ✅ / G-26 ✅ / G-27 ✅(F1=新 G-45) / G-29 ✅(error 面残留) / G-30 ✅(惰性钉定) / G-31 ✅(lane8：resolver 判根 + 10s 内存索引；子目录/失效策略偏差文档化) / G-33 ✅(热重载半边由 G-48 lane4 收口) / G-34 ✅(lane4) / G-35 ✅ / G-36 ✅ / G-37 ✅ / G-6 ✅ / G-8 ✅(G-41..G-44 已由 lane2 收口) / G-41 ✅ / G-42 ✅ / G-43 ✅ / G-44 ✅ / G-1 ✅(consumer 迁移仍开放)

---

# 第五轮全量对比审查（2026-10-31）

> 9 通道全量对比（session/配置/工具/infra/AI/TUI/server/CLI/schema-plugin，逐文件对读，总报告 `PORT_AUDIT_2026-10-31.md`）后，又对全部初报缺口做 4 通道只读二次取证。本节收录经核验 CONFIRMED 的新缺口（G-50..G-78）、既有条目订正与归类、以及撤销清单（勿再立项）。

## 🔴 高

### G-50 ✅ snapshot restore/capture 不保真文件模式与符号链接（已修：restore 按 tree entry filemode 恢复 symlink/可执行位，capture 以 read_link 入库 symlink 目标；commit 022c5ef）
- rust `snapshot.rs:489-527`：`restore_blocking` 过滤 Blob 后直接 `fs::write`，tree entry 的 filemode（100755/120000）被完全忽略——symlink blob（内容=目标路径）被还原为**含目标文本的普通文件**，可执行位丢失；capture 侧同样只以 blob 内容入库，元数据双向丢失。
- ref `git.ts:596-617` 经 `git checkout <tree> -- <file>` 原生恢复 exec bit 与 symlink。用户 revert 后仓库即损坏。
- 与 G-30 同文件邻接但语义独立（G-30=gitconfig 钥匙）。

### G-51 ✅ persistent-pty WS connect 缺 Origin 前置检查（已修：connect ticket 分支复用 origin_allowed，403 且不消费 ticket；commit d5f5c00）
- rust `persistent_pty.rs:275-283`：ticket 分支仅 `consume_ticket`，`:283` `let _ = headers;` 显式丢弃；同文件 `connect_token`（:247-249）反而有 `origin_allowed`——工具在手边未复用。
- ref `persistent-pty.ts:110-115`：`isAllowedRequestOrigin` 不通过即 403 且不消费 ticket。修复即 connect 分支复用 `origin_allowed`。
- 非 G-25 重复（G-25 是 pty.rs connect-token 端口剥离，已修）。

### G-52 ✅ `opencode debug config` 脱敏弱于 ref（已修：secretName 全量对齐 + URL userinfo/query-secret 整体打码 + headers 子树传播；commit 45d7765）
- ref `handlers/debug/redact.ts:2` secretName 含 `credential|private.?key|authorization$|cookie$`；`:11-17` URL 凭据/query-secret 整体打码 + headers 子树传播脱敏。调用链完整（index.ts:41 → config.ts:8,16）——**订正 G-17 原"redact 是 ref 死代码"断言（已改）**。
- rust `oc-cli/handlers/misc.rs` `is_secret_key`（~:472-476）仅 `apikey/api_key/secret/token/password` contains；无 URL 检查、无 headers 传播。可能明文打印 ref 会遮蔽的密钥。

### G-53 ✅ skill 工具输出格式错位（已修：prepare/prompt 共享 toModelOutput 信封 + 采样路径 .slice(0,10)；commit acf2e09）
- ref：`tool/plugin/skill.ts:38` content = `Skill.prepare` → `toModelOutput`（skill.ts:36-53）= `<skill_content>` 信封 + `fs.scan` 采样**路径** `.slice(0,10)`。
- rust `tool/skill.rs:24-47`：`prepare()` 把目录**全部文件内容**拼接（非递归、无 10 上限、无信封）；执行器 :172 调用之。信封实现已存在（`prompt_text` :55-82）但仅 prompt 附件路径使用。大技能目录会撑爆上下文。

### G-54 ✅ run 非交互：非 question 表单取消退出码 0（已修：cancel_form 非 continuing 分支 record_exit(1)；commit 2f0e21a）
- ref `run/noninteractive.ts:203-204`：`formCancelled` 非 v1 → `process.exitCode = 1`。
- rust `run/noninteractive.rs:334` 仅置 `form_cancelled`；该标志在 :672-685/:1100/:1119/:1135 被用作**跳过**错误路径的条件，最终 exit 0。脚本按退出码判断 run 成败会漏检。修复：cancel_form 非 continuing 分支 `record_exit(1)`。

## 🟡 中

### G-55 ✅ shell create 缺省 cwd 继承服务器进程 cwd（已修：handler 侧 cwd None/空回退 location.directory，对齐 shell.ts:265；commit lane3）
- rust `shell.rs:562` `unwrap_or_default()`，`create_invocation` 仅非空时 `current_dir`（:648-650）。工具路径安全（tool/shell.rs:165-168 显式覆写）；**生产命中点：HTTP `POST /api/shell`**（oc-server/handlers/shell.rs:40-52 直接透传可能为 None 的 body.cwd）。ref `shell.ts:265` `input.cwd ?? location.directory`。修复：handler 侧用 location 兜底。

### G-56 ✅ 凭据/wellknown 变更不驱动 config 重载与 location 失效（勘误 #4 半边修正；lane4 收口：boot 订阅 credential.switched/integration.updated → request_reload + location 失效，watch_loader 加 .with_wellknown，refresh 变更时发布 integration.updated 驱动周期刷新后重载）
- 刷新循环与 HostWellknownSource 接线属实（engine/mod.rs:570-596、:900-903）——勘误 #4 只对了这一半。缺的半边：①全仓 `request_reload` 生产调用点为零（仅 WatchedConfig 自身文件 watch 驱动，config.rs:655,671）；②credential/integration 事件不触发 location invalidate（ConfigReloadBridge 只挂文件 watch，boot.rs:113-140）——已 assemble 的 location 持旧 wellknown 快照直到其他原因失效；③boot 的 watch_loader 无 `.with_wellknown`（boot.rs:241-266）。
- ref `config.ts:306-322` 订阅 Credential.Switched / WellKnown.Updated → `reload()`；10 分钟刷新后亦 reload（:323-341）。与 G-48 同族不同面。

### G-57 ✅ 删除非 active 凭据多发 `credential.switched(credentialID:null)`（已修：store 发布门改为仅删 active 才发 Switched，对齐 credential.ts:314-319；handler 原本已正确）
- rust `integration/credential.rs:331-360`：非 active 分支返回 `Some((Some(integration), None))`，发布门只判 integration——非 active 删除也发 Switched。oc-server/handlers/credential.rs 同病。
- ref `credential.ts:268-269,314-319`：`{switched:false}` 只发 updated；仅删 active 才发 Switched。修复：发布门加 switched 判定。

### G-58 ✅ Session.create 项目解析仅目录身份（已修：插件 session.create 传 services.project 完整解析，对齐 session.ts:271）
- rust `store.rs:604-609` project 缺省回落 `hash("directory:…")`。HTTP/命令/transfer 主路径均显式传 `services.project`（完整 Resolver，engine/mod.rs:872-887）；**生产命中点：插件 `session.create`**（oc-server/plugins.rs:1300-1313 `CreateInput::default()` → project=None）。ref `session.ts:271` 无条件 `projects.resolve`（含 realpath + VCS 发现）。

### G-59 ✅ 工具输入校验为 JSON-Schema 子集（已修：扩展 pattern/oneOf/anyOf/allOf/$defs+$ref/minItems/maxItems/uniqueItems/additionalProperties；format 保持宽松；无新依赖）
- MCP schema 有注册并校验（mcp/registry.rs:1749 → tool/registry.rs:383 `validate_input`），真实缺口是子集语义：rust `registry.rs:88-232` 仅 type/enum/min/max/required/items 等，pattern/oneOf/$defs 引用等静默放行。ref `runtime.ts:107-113,251-258` 经 jsonSchema() 全量 draft-07/2020-12 转换校验。坏参数穿透到 leaf。

### G-60 ✅ list_mcp_resources 模型内容固定占位文本（已修：content 改为 stringify(catalog) 全 JSON）
- rust `tool/mcp_resource.rs:213` content 恒 `"Listed MCP resources"`；ref `runtime.ts:57,259-264` normalizeContent → stringify(catalog) 全文。模型在直接对话路径拿不到目录内容（output JSON 仍正确）。

### G-61 ✅（lane7）：CLI auth login 无 target 时走交互集成选择器
- TTY 无 target 时弹 dialoguer 选择器：loginChoices 排序（MCP → Popular[优先级表] → Services，name/id 决胜）+ connected ✓ 标记，选中后按 id 解析；非 TTY 保持 requireInteractive 报错。

### G-62 ✅（lane7）：CLI plugin check/update 覆盖 TUI(npm) 插件
- check 合并 Server+TUI 分区输出（ref inventory.ts `format`：每行 `name version (update available|current|check failed)`，hex 版本截 7 位），TUI 项走 oc-plugin-host `npm_check`（无 24h 缓存）；update 对 TUI 项走 npm update 安装流，任一 check/update 错误 exit 1。

### G-63 ✅ persistent-pty connect 协议分歧（已修：未知 pty 一律升级后 4404 关闭；非 framed 二进制输入原样字节直传 write_bytes）
- ① 不存在的 pty：rust `persistent_pty.rs:276-278` 升级前 HTTP 404；ref（persistent-pty.ts:126,152-158）一律升级后 `CloseEvent(4404, "terminal unavailable")`——WS 客户端错误路径完全不同。
- ② 非 framed 二进制输入：rust `persistent_pty.rs:482-484` `from_utf8_lossy`（U+FFFD 污染）；ref `persistent-pty.ts:203-216` 原样字节直传。G-10 覆盖的是 pty.rs 输出方向，本条是输入方向，独立。

### G-64 ✅ 插件 Context API 面残留（G-24 API 面追加）（lane5 收口）
- 已全部落地：`ctx.provider.list/get/transform/reload`（editor 表含 models 子编辑器；transform defs 经 `apply_plugin_providers` 折入 location overlay）、`ctx.reference.list/transform/reload`（`apply_plugin_references` 入 ReferenceService）、`ctx.experimental.terminal.read`（persistent-pty Frontend.read）、`ctx.integration.list/get/connect.key/oauth.{connect,status,complete,cancel}/command.*` 与 `connection.{active,resolve,status}`（oc-plugin-host ProviderPort/ReferencePort/TerminalPort/IntegrationPort + supervisor 异步派发 + oc-server Engine*Port 槽位模式）。guest 集成测试 g64_domains.rs。注：`ctx.vcs.transform` 的 vcs provider 行进注册表快照，但 guest vcs adapter 未接 project 解析（无既有宿主缝，见 lane5 report concerns）。

### G-65 ✅ aisdk hook 注册面在、生产派发点为零（G-2 残留）（lane5 收口）
- 生产派发点已接：`SessionModelRequest::prepare`（model_request.rs）在 language model 绑定请求处派发 `aisdk.sdk` → `aisdk.language`（aisdk.ts:346-354 的 runSDK → runLanguage 链；sdk 句柄流入 language 事件；返回的 language 句柄**替换**请求目标——变换点；language 事件的 options 并入 provider options，copilot lane 的 `endpoint` 覆盖由此流入，无 hook 时内联 lane 选择保持默认）。测试 model_request.rs aisdk_dispatch_tests。

### G-66 ✅（lane6）：composer 选区以反色（fg/bg 交换，EditBufferRenderable 无 selectionBg 默认支）渲染；live 路由 shift+方向键已映射（crossterm spec 层此前丢弃）
- rust 模型在（oc-tui-core/text_edit.rs:276,372-383 anchor/range API），渲染消费缺（composer/render.rs 无 selection 绘制）；upstream `EditBufferRenderable.ts:67-68,143-144,279` 有 `selectionBg/selectionFg`。`input.select.*` 键位有绑定但屏幕无反馈。（IME 半条已撤销：upstream 亦无 composition 渲染，终端模拟器自理。）

### G-67 ✅（lane6）：剪贴板 failed/timed-out/limit-exceeded 读按 clipboard.ts 抛错语义变为 Err，粘贴路径 toast.error 展示（消息逐字对齐 ref）
- rust `clipboard.rs:388-399`：failed/timed-out/limit-exceeded 统一 `_ => None`，粘贴静默无操作。ref `clipboard.ts:30-35` 抛错，上层 `prompt/index.tsx:373-388` toast。超限粘贴（大图）用户无任何提示。

### G-68 ✅（lane6，fix1 后仅 restore 为保留 seam）：mini/fragment.rs 移植 stream-v2.fragment.ts；project/delta 覆盖消费/end 已接入 transport 折叠 + 回放生产路径（replay_history→replay_message_commits：重放投影进同一 reconciler，图片 commit 随回放触发，重复回放不重绘）；mini/image.rs 移植 stream-v2.image.ts（kind user/tool、metadata+payload 身份去重先于 mime 过滤）。restore(append|covered|conflict) 为保留 seam：rust mini 为 mono 写直写终端回滚区、无本地行缓冲重放（ref 的 restore 消费点是 resize-replay 重纳本地行），无生产消费点
- fragment reconciler（message part 重投影/restore append|covered|conflict，ref `mini/stream-v2.fragment.ts:1-83`）：rust mini 全目录 grep 0 命中；transport.rs:128-137 仅 delta 累加。
- mini 图片 commit + 去重（ref `stream-v2.image.ts:1-20`）：rust mini/ grep "image" 0 命中。
- （subagent tracker 已移植并撤销：transport.rs:455-620 对标 stream-v2.subagent.ts。）

### G-69 ✅（lane6）：bases 记忆（TUI 退出前记住）+ 按目录懒缓存（vcs.base 一次，**任何**关闭路径失效——Store::close_diff_viewer 统一失效）+ DiffBase 选择器（vcs.branch.list，Diff source 对话框 Base 行）
- rust `fetch_diff`（app.rs:6463-6551）每次 fresh 调 `vcs().base`，无记忆 Map/懒缓存；全库无 `vcs.branch.list` 消费——**无任何入口可选 base 分支**（错误文案 "Choose a base branch" 存在但无选择器）。ref `diff-viewer.tsx:110-124,280`（bases 记忆 + 懒缓存 + "Remembered until TUI exits"）。

### G-70 ✅ snapshot store 与 project id 不做 realpath（已修：store id 改为 canonicalize(worktree) 哈希；commit 022c5ef）
- rust `snapshot.rs:230-233` `hash_fast(worktree)` 无 canonicalize，且 Resolver 的 "canonical" 本身是词法的（project.rs:1252-1254、:964 自认）。ref `snapshot.ts:93-96` 先 `fs.realPath`（project.ts:338 同）。经 symlink 进入的仓库 → 不同 store，快照历史分裂。

### G-71 ✅（lane7）：prettier/oxfmt 探测改走 npm_which
- PackageJsonDependency 分支改用与 Biome 相同的 `npm_which`（全局 cache 世代 + 按需自动安装），依赖探测仍以 findUp package.json 为门；项目 node_modules/.bin 链 `npm_bin` 已删除。

### G-72 ✅（lane7）：MCP stdio 16MiB 帧上限 + stderr 尾巴诊断
- rmcp codec 拥有分帧，故上限在受控 seam 实施：交给传输的 reader 包 `FrameCapReader`（按行计数、超限 fail-fast 报 "MCP stdio frame exceeded 16 MiB"，读缓冲回滚遵守 AsyncRead 错误契约）；LocalStdioSpawner stderr 改 pipe + 后台 drain 进 8KiB 滚动尾巴，本地 server connect 失败附带尾巴文本（短 grace 等待 drain）。

## 🟢 低

### G-73 ✅（lane7）：heredoc 剥离容忍闭合标记尾随空白
- strip_heredoc 闭合标记后先 trim_end 再剥标记与前置换行，`"EOF "`/`"EOF\n"`/`"EOF \t\n\n"` 均正常剥离。（create_dir_all 半条已撤销：ref Bun.write 同样建父目录。）

### G-74 ✅（lane7）：progress 事件 + webfetch permission metadata
- 共享 `ProgressFn` 通道（runner 级 session.tool.progress 发布器，同 subagent）：shell spawn 后 `{shellID}`、websearch 结果边界 `{provider}`（rust query seam 内部定 provider，故在结果边界发）、codemode 叶子 `{toolCalls}`（runtime seam 对逐调用更新不可见，程序落定后发一次完整清单）；webfetch permission metadata 为完整输入 `{url,format,timeout}`。

### G-75 ✅（lane7）：session export/import CLI 行为对齐
- export：选择器 clack 风格状态输出全走 stderr（stdout 保留给导出内容），取消 exit 130、无会话仍 exit 0；import：先按 SessionTransfer 形状（`{info: Session.Info, messages: []}`）校验，坏载荷报清晰错误拒绝。

### G-76 ✅ persistentPty.update payload 放宽（已修：oc-schema 增 UpdateInput；size 必填、非法 400，无 80/24 回退）
- rust `persistent_pty.rs:175-186`：size 可缺省（静默跳过 resize 仍 200）、非法值回退 80/24；ref schema `size` 必填 PositiveInt，缺失 → 400（persistent-pty.ts:36-39）。oc-schema 无 UpdateInput 类型。

### G-77 ✅ location.get 的 project.directory 被 canonical 覆盖（已修：directory=检出目录、canonical=main worktree realpath）
- rust `location.rs:40-44` 两字段同源 canonical；ref `project.ts:358-362` directory=实际检出目录、canonical=realpath（:43-44 注释明言语义不同，:135 依赖二者不等分支）。

### G-78 ✅ session.log 非法 sessionID 400 文案（已修：schema ParseError 格式 `Expected sessionID to be …`）
- rust `sse.rs:493` message 为原始值回显；ref schema ParseError 形如 "Expected sessionID to be…"；rust 自身 after/follow 参数（sse.rs:504,518）已用该格式，sessionID 处不一致。一行修复。

## 归类与收编（不另立项）

- **POST /api/plugin/install（rust 独有路由）**→ G-17 rust-extra 家族（misc.rs:25 注释明言服务 G-29 安装流；ref protocol 仅 list/check/update）。
- **function 云分享服务端整包缺失** → 纯重复 G-17（:82 明列）。
- **oc-schema 缺 filesystem 非事件类型 + rpc.ts + v1/legacy** → G-32 子集（lib.rs:80-82 自认，已裁决有意）。
- **OTLP 遥测** → 纯重复 G-38（otel feature 非 default）。
- **OSC52 100KB 上限 + 空写直通** → 并入 G-39 备注：上限为防御性（终端普遍拒绝大 payload）且带显式错误；空写是对 v2 行为的有意 parity 修正（clipboard.rs:257-267 注释）。
- **codemode 沙箱架构（自研受限解释器 → Bun guest 真执行，无 Bun 即不可用）** → 收编为文档化决策（体例同 G-28/G-32）：codemode.rs:21-52 论证 + plugins.rs:241-255 接线；原仅记录于 PORT_AUDIT_2026-10-31.md M25。
- **评测/实验 lane（evaluation/system-one/opencode-zen/typesafe-ai）** → 归类 G-7/G-17；zen/typesafe-ai 唯一暴露面就是 experimental.evaluation（providers/opencode-zen.ts:1-25、typesafe-ai.ts:1-19），无常规 chat 导出。
- **AI SDK 公共 API 面（AIClient/promise/generation/tool builder）** → 归类 G-17 有意省略：ref 全仓无外部消费者（grep 0 导入），插件 tool 面已有等价（host.js:353、registrations.rs:95,574）。
- **非 Linux 音频走系统播放器 spawn** → 记录性差异：Linux（本构建目标）已进程内 miniaudio（sound/audio.rs:1-9，SOUND-CONV lane），非 Linux 为显式降级残留。
- **classify_provider_failure cause 恒 None** → 记录性差异：rust `ProviderFailure` 输入结构本无 cause 字段（provider_error.rs:317-329），错误链由 `#[source]` 上层承载；ref 中 cause 也不参与分类判定。
- **storybook/simulation** → 纯重复 G-8/G-17。

## 勘误（第五轮经核验撤销的初报缺口，勿再列入待修）

1. **read 工具丢 realpath 越界防护** — 初报对照错面：ref 工具边界同样是词法的（file-access.ts:67-95）；realpath 双栅栏属 HTTP FileSystem.read 服务，rust 侧已完整对齐（oc-server/handlers/fs.rs:119-141 `resolve_contained`：词法 contains → canonicalize → 二次 contains）。仅 AGENTS.md 发现走查用词法路径（tool/read.rs:855-868 vs ref realpath）属外观级差异。
2. **step.failed finish 强制映射 ContentFilter** — ref schema 将 finish 约束为单字面量 `["content-filter"]`（session-event.ts:379），updater `?? "error"`；rust StepFailFinish 同样只接受 content-filter（session_event.rs:383-412）——完全等价。
3. **engine.prompt 绕过 SessionPrompt.prepare** — seam 已在生产接线（engine/mod.rs:1497-1505 无条件）；drain 侧 backstop 覆盖裸 admit 路径（drain.rs:508-514 + prompt_prepare.rs PreparedRing 防双发），可观测面与 ref 等价。
4. **glob 搜索根≠location 显示路径基准不同** — 两侧 produce 相同绝对路径（glob.rs:151-154 `root.join(rel)` = glob.ts:131 `path.resolve(location.directory, rel)`），无模型可见差异。
5. **git.ts 服务未统一移植** — 唯一消费方 repository-cache 已完整移植（reference/repository_cache.rs:235-287：clone/fetch/checkout -B/reset --hard + daily 节流 + self-healing），无行为缺失。〔2026-11-07 第十五轮订正：fetch 细节并非无缺失——repository_cache.rs:272-281 仅 `git fetch origin`，ref git.ts:269-284 为 `--all --prune` 与 `+` 强制 refspec（多 remote reference 刷新不到/已删上游分支残留/非快进更新被拒）；立项 G-231。〕
6. **client permission 命名空间重组** — wire 逐路径一致（permission.rs:19-30 ↔ protocol/groups/permission.ts:23-117），仅内部函数命名不同。
7. **AIClient SDK 公共面 / cause 链 / IME / mini subagent tracker / diff turn 模式** — 见上文归类与撤销说明。

## 复审判定汇总（第五轮）
初报 35 项：CONFIRMED 新立 G-50..G-78（29 项，其中 G-63/G-64/G-68/G-69/G-75 各含 2 子项）；归类既有 G 条目 8 项（G-7/G-17 ×3/G-32 ×2/G-38/G-39）；RETRACTED 12 项（见勘误）；另订正 G-17 一处断言、勘误 #4 半边（→ G-56）。

---

# 第六轮复核（2026-10-31，第五轮同日：对文档现存未关闭条目的现状验证）

> 2 通道只读取证。目的：发现被后续提交悄然修复的项（收口标 ✅）、确认仍在的项、修正过时表述与路径。G-50..G-78 刚经第五轮双通道取证，不在本轮范围。

## 已收口（复核确认修复）

- **G-4 ✅（插件注册自定义 worktree strategy 已实现）**：guest `ctx.worktree.transform`（host.js:898-941）→ dispatch（supervisor.rs:356-369）→ `merge_worktree_strategies`（registrations.rs:494,643）→ `GuestWorktreeStrategy` 入注册表（plugin_apply.rs:685-695 → plugins.rs:2134-2151 → boot.rs:439-440）。测试 g24_domains.rs:96-98、g24_plugin_domains.rs:133。残留：注册表仍 boot 一次性（见 G-24 热重载子项）。project.rs:1103-1105 的"不支持"注释已失实（`add_strategy` 在 :1133 被调用）。
- **G-6 ✅（Shell 输出查看器 1s 轮询已实现）**：app.rs:8427-8437 对活跃 ShellOutput viewer 每秒重发 `LoopRequest::ShellView`；fetch 本体 app.rs:5635-5701。代码内 "G-6 residual" 注释已过时，可清理。
- **G-8 ✅（https/socks5 代理已支持）**：proxy.rs:10-14 模块头；socks5/socks5h 经 `tokio-socks`（socks5_connect :432-438），https 走 TLS-to-proxy（:318-320,:354），仅 socks4/4a 拒绝（:184-191，范围声明）；socks5 真桩 WS 端到端测试（:684）。proxy.rs:616 "G-8 residual" 注释已过时。
- **G-33 的"config 键无人消费"半边 ✅**：shell.rs:225 `with_output_limits` + engine assemble 已消费。**G-48 热重载半边 ✅（lane4）**。

## 部分清偿（条目修正）

- **G-1 ⚠️（残留清单缩小）**：`mcp/client.rs` 已切 Environment stdio seam（client.rs:103 `use crate::environment::stdio::{...}`，:1059-1065,1152-1158，注释明言 workspace driver 扩展点）——从残留清单移除。仍直连 std::fs/portable_pty 的 8 个 consumer：read.rs（:179,388-401,713,764）、write/edit（经 file_mutation.rs:115,127-128,253-279）、tool/shell.rs（:227）、glob.rs（:124）、grep.rs（:427）、patch.rs（:401,417,426,444,452）、oc-core/pty.rs（:439 portable_pty）+ oc-server/handlers/pty.rs。
- **G-24 ✅（lane5 清偿 bounded 外项）**：vcs.transform/vcs.reload 已接（supervisor 派发 + 注册表合并；reload 走 VcsPort——EngineVcsPort 实时计算，reload 为契约面）；websearch `editor.default.set` 已持久化（host.js → WebsearchPort::set_default → `WebSearch.select` 引擎 KV，与 ref 的 auth/config 存储等位）；worktree 注册表热重载已接（`reload_after_update` 返回是否有代际移动，plugin/update handler 重跑 `plugin_worktree_registry` 播种；`AppState.worktree_strategies` 移入 `Arc<RwLock<…>>` 可热换）。guest vcs provider 的 info/base 闭包仍留 guest 侧（无宿主缝，见 lane5 report）。
- **G-31 ✅（lane8 收口）**：~~VCS 根不限量已修~~ 进一步：`is_vcs_root` 改用 project resolver（hg 生效、仓库子目录保守=100k，与旧根嗅探语义一致）；`/api/fs/find` 改读进程级 per-canonical-root `SearchIndex`（10s 后台刷新、build-on-first-miss、single-flight；非 VCS 根 100k 在索引构建时施加）。偏差文档化见上文 G-31 条目。

## 确认仍在（不变）

- **G-34 ✅（lane4）**：home_location 已接消费端——`set_home_location` 写通共享 cell，`HttpPromptSink` 提交时按其解析 home create-directory（inherit → 当前会话目录），home 状态行显示继承目录。
- **G-48 ✅（lane4）**：config reload 路径重赋值（bridge 重导 Limits + `reload_tool_output_limits`，assemble 读 `current_limits` 活值）。
- **G-41 ✅**：代理 userinfo 已保留并消费——CONNECT 发 Basic `Proxy-Authorization`、SOCKS5 用 username/password（lane2, 0f932c2）。
- **G-42 ✅**：WSS_PROXY/WS_PROXY 已优先咨询，再回落 HTTPS/HTTP(S)_PROXY/ALL_PROXY（lane2, 17947d9）。
- **G-43 ✅**：loopback（127.0.0.1/localhost/::1）已恒直连（lane2, 2d889a9）。
- **G-44 ✅**：NO_PROXY 已改 ref 语义——bare host 精确相等、`.`/`*` 前缀后缀匹配、空白+逗号分隔（lane2, df29fd3）。
- **G-45 ✅（lane7）**：guard 已按 crate 参数化，schema 包同样守卫（LOCAL_CHANGES.patch 已提交）。
- **G-46 ✅（lane5 收口）**：dialog.plugins.error 打开失败插件的 error 详情（plugins.tsx:224-228 的 "view error"）——ErrorDetails 对话框（可滚动、esc 回插件列表；无失败行为无操作）。
- **G-47 ✅（lane6 收口）**：backoff 复位 + falsy env fallback 均已修（editor_ide.rs ConnectOutcome.opened → attempt=0；env_editor_port filter 空串）。
- **G-49 ℹ️**：4 子项仍准确（spinner 受 mini/runtime.rs:38-39 TICK=100ms 约束；stdio.rs:11,:27 vs :104 措辞与实现不符；project.rs:1103-1105 过时注释——本条与 G-4 修复同源，可随清理收口；proxy.rs:367-369,:318 webpki 一致性声明在 G-26 后仍失效）。
- **G-28 ℹ️ / G-32 ℹ️**：决策现状未变（ui.rs:1-15 有界 slot bridge；oc-schema lib.rs:80-82 TODO 自认）。

## 路径勘误

- 文档多处 `oc-ai/src/...` 应为 `crates/oc-ai/src/...`（本轮引用已按现行路径取证；proxy.rs 内部行号基本未漂移）。

## 第六轮判定汇总
复核 15 项：✅ 收口 3（G-4、G-6、G-8）+ 1 半边（G-33 消费）；⚠️ 部分清偿 3（G-1 缩小、G-24、G-31）；❌/⚠️ 不变 8（G-34、G-41..G-48 各项、G-49）；决策项 2 现状不变（G-28/G-32）。

---

# 第八轮（2026-11-01，独立九域审查 + 双通道逐项核验）

> 来源：`CODE_REVIEW_PARITY_2026-11-01.md`（9 域并行逐行审查，明细 `/tmp/opencode/review/L1..L9-*.md`）。本轮对该报告的 21 项缺口指控做 2 通道源码级核验：**确认 14 项**（下列 G-79..G-92，续编号）、**驳回 4 项**（见文末勘误，勿再立项）、**已追踪 2 项**（browser 工具→G-8 有意决策；语法高亮语言裁剪→G-13/R-01b）、**1 项并入 G-32 备注**（schema TODO 未明确子项）。

## G-79 ❌（🔴 数据完整性）Session.remove 不完整 → 孤儿子会话
- Rust：`oc-core/src/session/store.rs:557-562` 仅发布 `session.deleted`；`oc-server/handlers/session.rs:505-516` handler 直调 store.remove；projector（`session/projector.rs:148-152`）只删 session_v2 行。`execution.await_idle` 存在（handlers/session.rs:1542 他处使用）但 remove 路径未调用。
- Ref：`packages/core/src/session.ts:367-377` interrupt→awaitIdle→transport.close→**递归删子会话**→environments.clear→bus.remove。
- 影响：删除运行中会话不先中断/等待；子会话（parent_id 指向已删父）成孤儿行；environments 不清理。bus map 清理半边已有反应式等价物（`bus/routing.rs:125,168` 对 session.deleted）。
- 修法提示：remove 前置 interrupt+awaitIdle；递归收集 parent_id 后代一并删除；补 environments 清理。

## G-80 ❌（🔴 指令面泄漏）`<available_skills>` 缺 permission deny 过滤
- Rust：`session/runner/context.rs:678-704` filter 仅看 `description.is_some() && auto_invoke != Some(false)`，无 `Permission.evaluate("skill", id)`（对比同文件 :1587 `mcp_guidance_source` 显式收 permissions——照抄该模式即可）。
- Ref：`packages/core/src/skill.ts:33-34`（`available()` 过滤 `effect !== "deny"`）。
- 影响：被 deny 的技能仍进系统提示，模型调用后才被工具层拒绝，浪费一轮工具调用。

## G-81 ❌ Vercel AI Gateway gatewayProtocol 缺失
- Ref：`packages/ai/src/protocols/utils/gateway-protocol.ts:29-90`（`providerOptions.gateway` 注入请求体 + finish 事件合并 `provider_metadata.gateway` usage/cost）+ `providers/vercel-ai-gateway.ts`。
- Rust：grep 全 oc-ai/oc-core 仅见 cache_policy.rs:50 / customization.rs:303 的缓存与 auth 处理；`variant.rs:861 vercel_gateway` 只做 thinking 变体折叠，无 body 注入、无 finish 网关 metadata 合并。
- 影响：gateway 路由选项（caching/order/models 等）不下发；finish 事件丢失网关侧 usage/cost（计费可观测缺口）。

## G-82 ⚠️ tool.progress 不回写 LlmEventPublisher（死代码机制）
- Rust：`engine/mod.rs:2273-2288` ProgressFn 直发 bus；`session/runner/publisher.rs:867-881` 的 `progress()`（记录 + 发布）**全仓零生产调用点**；而 `publisher.rs:962-977` failureSnapshot 与 subagent aborted `(sessionID: …)` 后缀都依赖 `tool.progress` 已记录。
- Ref：`session/runner/publish-llm-event.ts:562-572`（progress 先记录再发布）、:336,352-355。
- 影响：live `session.tool.failed` 恒缺进度元数据；subagent 中止错误永远不带 `(sessionID: …)` 后缀（仅 stale 清算路径有）。易修：engine 的 ProgressFn 改走（或同时调用）publisher.progress。注意 G-74 只覆盖"发事件"半边，本条是"记录"半边。

## G-83 ✅ which-key 面板：11 条绑定全部接通（lane D3.1 全量化）
- 取证：面板**不是** `@opentui/keymap` addon。ref 的面板是 TUI 本地 feature plugin `packages/tui/src/feature-plugins/system/which-key.tsx`（607 行，最后随 v1 plugin runtime 存在于 4a93972a78；06290907a9 删除文件但 `config/keybind.ts:290-300` 绑定保留）；`@opentui/keymap` 只导出 `ActiveKey` 数据面。
- Rust：`oc-tui/components/which_key.rs` 全量移植——dock/overlay 布局、pending 预览、分组 tabs + prev/next、scroll/page/home/end、pending 模式全组展平、footer 提示；`oc-tui-core/src/keymap/table.rs:810-825` 的 11 条绑定全部真实派发（`command_palette.rs` 的 8 条面板命令不再 no-op）。
- 验证：字段级 `app::which_key_tests` + 组件单测 + `tests/pty_which_key.rs`（真实 kitty CSI-u：ctrl+alt+k 固定根列表、ctrl+alt+shift+k 切 dock/overlay、ctrl+alt+right 切组、ctrl+x pending/escape）。
- **勘注（D3-7b，超集移除令）**：面板按用户超集移除令整体删除（`components/which_key.rs`、store 状态面、app sync/优先拦截、palette dispatch 臂、`pty_which_key.rs`）；`keybind.ts:290-300` 的 11 条 `which-key.*` 注册行**保留**＝ref 同款死绑定态（注册、无消费者）；D3-1 的全量移植撤销。

## G-84 ❌ alibaba-responses 专用适配器缺失
- Ref：`packages/ai/src/protocols/alibaba-responses.ts`（96 行：`enable_thinking`/`previous_response_id`/`conversation` 注入、named tool_choice→`allowed_tools` 变换、`web_search`/`web_extractor`/`code_interpreter` 托管工具 onDone 还原）。
- Rust：`provider_catalog/variant.rs:1089` 将 `alibaba/responses` 归并到通用 `openai_responses`；`enable_thinking` 仅 chat 路径（openai_chat.rs:2992）、`allowed_tools` 仅 provider options 直读（open_responses.rs:1114-1120）、`web_extractor` 全仓零命中。
- 影响：阿里 Model Studio responses 面上 thinking 开关、会话续传、命名工具强制、托管工具输出还原均不可用。（同类存疑：meta-responses / bedrock-mantle responses 也归并到通用路径，见 L5 报告。）

## G-85 ⚠️ read 工具 alternatePath 缺 NFC 归一
- Rust：`tool/read.rs:790-798` canonical 仅做 NBSP/弯引号替换，缺 `name.normalize("NFC")` 前置。
- Ref：`tool/plugin/read.ts:139-143`（先 NFC 再替换）。
- 影响：macOS NFD 分解形式文件名与用户 NFC 输入无法互配 → alternatePath 静默失配报 not found。修法：canonical 前加 unic-normal NFC。

## G-86 ⚠️ opencode.models 工具 own_provider 未接线
- Rust：`tool/opencode_tools.rs:518` 注册时 `Mutex::new(None)`；:442 执行时读取；全仓无生产回填点（仅 tests 预填）。Ref：`tool/plugin/opencode.ts:134-197` 按 session.model.providerID 排 own-provider-first。
- 影响："Your own provider comes first" 恒不生效（低危，输出排序）。

## G-87 ⚠️（并入 G-32 备注）schema TODO 未明确子项
- G-32 已裁决 v1/legacy、durable-event-manifest、pty-ticket 等；**未明确**的三个子项：event-manifest 索引、snapshot store types、`Worktree.adopt` 投影（`oc-schema/src/lib.rs:80-83`、`misc.rs:121-123`）。其中 Worktree.adopt 影响会话/项目归因（worktree.remove 后会话归属），建议单独评估是否升级。

## G-88 ⚠️（安全语义）MCP OAuth RFC8707 resource 拒绝分支弱化
- Ref：`mcp/oauth.ts:168-171` `checkResourceAllowed` 不通过即 `throw "Protected resource ${resource} does not cover ${identity}"`。
- Rust：`mcp/oauth.rs:255-281` 仅 URL 推导；全 mcp 目录 grep `checkResource|does not cover` 零命中。
- 影响：服务器返回 resource 与配置 identity 不一致时不再拒绝——token 可能绑定到身份不匹配的 resource。建议补齐拒绝分支 + 测试。

## G-89 ⚠️ webfetch 超时语义分叉
- Rust：`tool/webfetch.rs:110-112` 仅 reqwest client timeout；:232-235 超时以 `Nested(error)` 落 cause，message 恒 "Unable to fetch {url}"；convert 阶段不在预算内。
- Ref：`webfetch.ts:146-148` `Effect.timeoutOrElse` 包整个流程，文案 `"Request timed out"`。
- 影响：模型可见的超时反馈不同，重试行为可能漂移。修法：外层 timeout 包全流程并输出 ref 文案。

## G-90 ⚠️ patch formatter 阶段静默容错
- Rust：`tool/patch.rs:490-493` read 失败 `continue`；:497-498 formatter sync 失败仅 `unwrap_or(current.text)`。Ref：`patch.ts:261,265-267` 分别 `fail("Failed to read/sync ${target}")`。
- 影响：部分失败时模型看到成功但 diff 缺格式化内容。

## G-91 ⚠️ session.http.response hook 的 request 回传为空
- Rust：`session/runner/model_request.rs:1128-1133` response 事件 `request` 硬编码空（url/method/headers/body）。Ref：`model-request.ts:340` `request: before.request` 完整回传。
- 影响：审计/重放类插件读不到触发请求。修法：把 before 的 request 快照透传。

## G-92 ⚠️ util 基础设施分裂（glob brace/extglob + MIME 三处漂移 + Windows spawn/npm）
- glob：`oc-util/src/glob.rs:7-8` 自认 "Braces and extglobs are not supported"（ref minimatch 支持）；:10-11 Glob.scan "deferred"——ignore 配置含 `{foo,bar}` 等模式时语义分叉。
- MIME：`FSUtil.mimeType` 仅落在 `oc-server/assets.rs:237-239`，另 `tool/read.rs:138`、`oc-tui/composer/attachment.rs:23` 各有私有表——三处漂移风险，应收敛为单一 util。
- Windows：cross-spawn-spawner（.cmd shim/PATHEXT/shebang）与 npm.ts（npm_execpath 探测）未整体移植——非 Windows 目标裁决可挂靠 G-8/G-17，但插件安装/updater 在 Windows 是已知分歧面。

## 第八轮勘误（经核验驳回，勿再立项）

- ~~迁移 journal id 不一致~~：`migrations.rs:205`=`20260410174513_workspace-name`、`:287`=`20260511173437_session-metadata`，与 ref 文件名逐一相同（`_workspace` 是另一条 20260225215848 迁移，双方同名存在）。初报对照错位。
- ~~models-dev 默认不刷新~~：`EngineOptions.catalog_fetch=false` 是 hermetic 默认，生产 boot **无条件置 true**（`boot.rs:309`），`OPENCODE_DISABLE_MODELS_FETCH` truthy→off 与 ref `fetch: !truthy(...)` 对齐（boot.rs:329）。
- ~~event-logger 缺失~~：已移植（`boot.rs:160-190` `event_logger_line`/`attach_event_logger`，:376-382 生产安装，boot.rs:1487-1545 测试）。
- ~~account 表持久化缺失~~：三表全移植（`migrations.rs:140,162-183` + `schema.rs:472-498` + bootstrap 建表）。
- ~~MCP stdio BUN_BE_BUN 仅测试断言~~：生产已注入（`mcp/client.rs:1067-1079` `stdio_overlay`，:1158 生产 spawn 路径调用）。

## 第八轮判定汇总
核验 21 项：确认立项 14（G-79..G-92，其中 🔴 数据/安全级 2：G-79、G-80；安全语义 1：G-88）；驳回 5（含第八轮勘误 5 条）；已追踪 2（G-8、G-13）。优先级建议：G-79 > G-80 > G-82 ≈ G-88 > G-81 > G-84 > G-91 > G-85 > G-89 ≈ G-90 > G-83（决策项）> G-86 > G-87 > G-92。

过时注释清理：已完成（2026-10-31 第六轮同日）——app.rs:8427/store.rs:7089/:14996（G-6 residual，共 3 处 + app.rs:7340）、proxy.rs 模块头/`:318`/`:616`/`:661`（G-8 residual + 失效的 webpki 一致性声明，改为指向 G-26 决策）、project.rs:1103-1105（插件 strategy 已支持，指向 GuestWorktreeStrategy）、environment/stdio.rs 模块头与 `StopChild` doc（SIGTERM 措辞改为与 `start_kill` 实现一致的"immediate SIGKILL + bounded reap wait"）。均为注释级修改，无行为变化；oc-ai 的 fmt 差异为 llm.rs 既有问题，与本次无关。

---

# 第八轮 burndown 收口（2026-11-01，gap-burndown-5）

> G-79..G-92 全部 14 项经 lanes A-G 串行落地并 fast-forward 合并 main（abac5af / 8a53326 / 981180a / 45c7358 / 7cfcd4e / fddac63 / 72a6422）。全程 worktree 隔离（`.worktrees/`，合并后 target 与 worktree 均已回收）、dev-gate 单入口、nextest 未过滤验证、日志落盘 `.test-logs/*-lane[ABCDEFG].log`。各 lane 报告含字面 `Summary:` 行与 `git log -1` 落地证明。

- **G-79 ✅（laneA，abac5af）**：新增 `oc_core::session::remove_tree`（session/mod.rs）——interrupt → 有界 awaitIdle（10s timeout，删除不受 drain 卡死挟持，理由代码内注明）→ 按 parent_id 递归删后代（子先父后）→ session_env.clear → store.remove/session.deleted；DELETE handler 改调 remove_tree。transport.close 无独立调用：drain 中断 unwind 已关传输通道（drain.rs close_transport），代码内注释说明。测试：递归删除、先中断后删（wedged drain 证序）、HTTP wire 父/子/孙 404。
- **G-80 ✅（laneA，abac5af）**：`skill_guidance_source` 接收 Ruleset，经 `permission::evaluate("skill", id)` deny 过滤（镜像 mcp_guidance_source 模式 / ref skill.ts:33-34）。测试：denied skill 不入 `<available_skills>`。
- **G-88 ✅（laneB，8a53326）**：MCP OAuth RFC8707 resource 覆盖校验——`check_resource_allowed`（origin 相等 + 请求路径长度 ≥ 配置路径 + 尾斜杠填充前缀比较，`/api` 不覆盖 `/api123`）+ `validate_resource_url`（缺失 resource → 配置 identity；不覆盖 → `Err("Protected resource {resource} does not cover {identity}")`），接入 discover。测试覆盖通过/拒绝两侧全矩阵。
- **G-81 ✅（laneC，981180a）**：`protocols/utils/gateway_protocol.rs`（GatewayProtocol 包装：三 API prepare、`{...upstream, gateway}` body 注入、跨帧 `merge_json_records` 合并 gateway usage/cost 到 finish 事件含 on_halt）+ `protocols/vercel_gateway.rs`（三条路由工厂 + x-session-affinity）+ facade `ProtocolFamily::VercelGateway` 接线与快照断言。
- **G-84 ✅（laneC，981180a）**：`protocols/alibaba_responses.rs` 专有协议（enable_thinking / previous_response_id / conversation / named tool_choice→allowed_tools / native.alibaba 三托管工具 onDone 还原含 web_extractor item 恢复）+ `ProtocolFamily::AlibabaResponses` 路由（variant.ts:571 侧映射核对无需求改）。**laneC 顺带发现 meta-responses 同族缺口** → laneE 收口。
- **G-82 ✅（laneD，45c7358）**：`publisher::ProgressLog` 共享记录表；engine 五处 ProgressFn 点先记录后发布（publish-llm-event.ts:562-572 对位）；failureSnapshot 与 subagent aborted `(sessionID: …)` 后缀消费记录，settle 时清理。测试：tool.failed 携带 progress。
- **G-85 ✅（laneD，45c7358）**：read canonical() 前置 NFC 归一（unicode-normalization 0.1.26 最小引入）再 NBSP/弯引号替换。测试：NFD 落盘名 + NFC 请求互配。
- **G-86 ✅（laneD，45c7358）**：`ModelsExecute` 携 SessionStore，执行时按 session.model.providerID 解析 own-provider-first（opencode.ts:134-142），engine 接线 tool_services.db。
- **G-89 ✅（laneD，45c7358）**：webfetch 全流程（fetch+collect+convert）入 tokio timeout 预算，超时文案 "Request timed out"；reqwest client 超时改 +5s 兜底；非超时失败保留 url 文案。测试：stalled server 与 refused port 两分支。
- **G-90 ✅（laneD，45c7358）**：patch formatter resync 读失败 → `fail("Failed to read {target}")`、sync 失败 → `fail("Failed to sync {target}")`；已删目标过滤出 resync 集合（patch.ts:256-258，全量跑撞出的回归同轮修复）。测试经 read-failing Files 包装。
- **G-91 ✅（laneD，45c7358）**：response 事件经 per-interceptor-pair stash 回放 before-request 快照（url/method/headers/body）。测试：request hook 改写 URL 后 response hook 观察到整形后请求。
- **G-83 ✅（laneF fddac63 有界面 → lane D3.1 全量化）**：取证裁定 ref 面板为 TUI 本地 feature plugin（非 keymap addon）；`oc-tui/components/which_key.rs` 按 607 行 ref 全量移植 layout/pending/group/scroll/page/home/end + 多列/分组/footer，11 条 `which-key.*` 绑定真实派发；PTY 用 kitty CSI-u 键序验证 layout 切换与 group 导航。
- **G-87 ✅（laneG，72a6422）**：三子项逐一裁决——**Worktree.adopt 已实现**（`oc_schema::misc::worktree_adopt`：candidate 检查含 workspace 排除与显式 adopted 列表、ref 路径归一（`.`/`..` 折叠 + Windows 盘符大小写）、subpath 投影 + 6 单测）；event-manifest 索引**本已移植**（oc-server/event_manifest.rs + bus registry + drift 守卫，TODO 注明）；snapshot store types**文档化不移植**（ref 6 行 branded-ID 模块无 wire 消费者，实体 store 已在 oc-core/src/snapshot/）。
- **G-92 ✅（laneG，72a6422，代码+文档双收口）**：三处私有 MIME 表收敛为 `oc_util::mime`（依赖方向核验：oc-util 零新依赖）；read.rs `.xml` 修正为 `application/xml`（原值对 npm mime-types 参照即漂移）；attachment 保留 ref 受限 allowlist 语义不变。glob brace/extglob 与 Windows cross-spawn/npm_execpath 以注释级"已知有界分歧"钉定（G-8/G-17 族）。
- **meta-responses ✅（laneE，7cfcd4e，laneC 新发现同族项）**：`protocols/meta_responses.rs`（770 行忠实移植：native.meta web_search/image_generation 校验、signed-ID replay 变换（provider 执行的 image_generation 结果按 itemId 回放 raw item handle）、托管工具表、text-end annotations 合入 providerMetadata、completedItems 双键去重）+ `ProtocolFamily::MetaResponses` 路由（variant.rs 映射核对无需改）。

## 第八轮 burndown 判定
G-79..G-92 全部 ✅（G-83 面板全量；G-87 三子项实现/已移植/文档化各一；G-92 代码收敛+分歧注释）。现存未关闭项维持既有文档化决策：G-28（有界 TSX shim）、G-32（v1/legacy 不移植，G-87 子项已并注）、G-12/G-13/G-16/G-17/G-38/G-39/G-40 ℹ️。

---

# 第九轮（2026-11-01，七域全量复审 + 逐项源码复核）

> 来源：7 通道并行逐行审查（core 会话/消息/Agent、内置工具、AI 层、server/API、TUI、CLI/client/SDK、schema/theme/util/plugin/pty/syntax），明细即本轮报告。对全部初报缺口逐项重新取证后：**确认 12 项**（G-93..G-104，续编号）、**撤销 3 项初报**（勘误见文末）、**归类既有条目 5 项**（不另立项）。风险排序：🔴 高 1 / 🟡 中 7 / 🟢 低 4。

## 🔴 高

### G-93 ❌ `$EDITOR` 往返丢失全部 mention 附件
- Rust：`oc-tui/src/app.rs:8165-8172` 编辑器返回后仅 `composer.set_text(&normalized)`；而 `components/composer/mod.rs:759-773` 的 `set_text` 无条件 `draft = PromptDraft::default()` 并清空 files/agents/skills 的全部 undo/redo 栈——`@文件/@agent/@skill` 附件静默全丢。
- Ref：`packages/tui/src/prompt/index.tsx`（`$EDITOR` 返回分支）调 `realignPromptInputMentions`（`prompt/mention.ts:20-140`，DP 最大匹配按最小代价把附件重挂到编辑后文本）+ `restoreExtmarksFromPrompt`。
- 修法提示：移植 realign DP，或在编辑器往返前快照附件、返回后按 mention 文本仍在文中重挂。

## 🟡 中

### G-94 ❌ `POST /api/session/:id/permission` 丢弃 payload 的 agent/metadata/source
- Rust：`oc-server/src/handlers/permission.rs:189-194` 三字段硬编码 `None`（仅 resources 透传 :191）。
- Ref：`protocol/groups/permission.ts:63` 全量透传写入 `Permission.Request`。
- 影响：依赖 request.metadata（权限弹窗上下文）/agent 归属的客户端行为缺失。

### G-95 ❌ `POST /api/session/:id/form` 丢弃 payload 的 metadata
- Rust：`oc-server/src/handlers/form.rs:115` `metadata: None` 恒置空。Ref `FormCreatePayload` 含 metadata 并透传。

### G-96 ❌ `GET /api/session/:id/context` 未按 compaction 截断
- Rust：`oc-server/src/handlers/session.rs:1226-1245` 返回**全部**消息（`ListOrder::Asc` 全量）。
- Ref：`session.ts:567` 语义为"最近一次 compaction 之后的活动上下文"（latestCompaction 之后的窗口）。
- 影响：长会话经该端点取上下文的客户端拿到被压缩掉的历史。

### G-97 ⚠️ permission.saved list/remove 引入 ref 没有的 location 装配副作用
- Rust：`oc-server/src/handlers/permission.rs:58-66`（saved_list）/remove 路径先 `resolve_location` + `services()`——冷启动触发 SSE 可见的 boot 级联、目录无效返回 404（注释自认是"v2 location middleware"，但 ref 中这两端点注册于组中间件**之前**、不经 location 解析）。
- Ref：`groups/permission.ts:37,49` 无 location 副作用、无该 404 路径。

### G-98 ⚠️ webfetch 非法 timeout 的校验时机后移至权限 assert 之后
- Rust：`oc-core/src/tool/webfetch.rs:208`（permission.assert）→ `:236` 才 `resolve_timeout`；`timeout<=0/>120` 的输入在解码期不拒绝（注释 AUD-19② 自认可知），用户会先看到授权弹窗、批准后才收到 "Invalid arguments"。
- Ref：`webfetch.ts:29` schema 解码期即拒绝，不触发权限确认。
- 注：与 G-89（超时预算/文案，已修）不同面，本条是**校验顺序**。修法：`resolve_timeout` 提到 assert 之前。

### G-99 ⚠️ `upgrade --method curl` 语义偏差（执行自有 release 下载而非官方安装脚本）
- Rust：`oc-cli/src/services/updater.rs:328` `"github" | "curl" => upgrade_from_release(...)`，`:454` 显式 `let _ = via_installer_semantics;`——curl 与 rust-extra 的 github 方法共用"下载本移植自身 release tarball"路径。
- Ref：curl 方法执行官方安装脚本（`curl -fsSL https://opencode.ai/install`）。
- 影响：用户显式 `--method curl` 得到不同发行流。修法：要么实现 installer 脚本路径，要么将 curl 从方法表移除/改名，避免静默语义替换。

### G-100 ⚠️ `LLM.generateObject` 的 jsonSchema 模式输出校验为手写子集
- Rust：`oc-ai/src/llm.rs` `validate_json_schema` 仅覆盖 type/enum/required/properties/items（+递归）；`oneOf/anyOf/allOf/$ref/minLength/pattern/minimum/additionalProperties` 等静默放行。
- Ref：Effect JsonSchema 完整解码，畸形输出报 `InvalidProviderOutputError`。
- 关联：G-59 已修**工具输入**校验子集（tool/registry），本条是 llm.rs **输出**校验器，独立半边。动态 schema（MCP/插件清单）调用方防错弱于 ref。

### G-101 ⚠️ workspace file-changes 对话框降级为纯 yes/no 确认
- Rust：`oc-tui/src/components/command_palette.rs:778-783` 仅固定文案 "This worktree has file changes." + yes/no。
- Ref：`dialog-workspace-file-changes.tsx:29-60` 渲染逐文件 `A/D/M` 状态与 `+N/−N` 增删计数（宽度自适应、8 行滚动窗）——强删 worktree/forceRequired 场景用户看不到将丢弃哪些改动。

## 🟢 低

### G-102 ⚠️ 滚轮加速模型缺失（scroll.acceleration/speed 配置无行为效果）
- Rust：`oc-tui/src/store.rs:6624` 注释自认 wheel 流无加速模型；`dialog_config.rs:303-308` 的 `scroll.acceleration` 行可切换但无消费点（默认步长 3 已对齐，默认路径一致）。
- Ref：`tui/src/util/scroll.ts:20-28`（MacOSScrollAccel / speed 默认 3）被 12 个表面消费。

### G-103 ⚠️ `app.exit` 默认键位缺 `<leader>q`
- Rust：`oc-tui-core/src/keymap/table.rs:511` 仅 `ctrl+c,ctrl+d`（注释 :510 说明裁决：`<leader>q` 单归 session.queued_prompts）。
- Ref：`config/keybind.ts:48` `ctrl+c,ctrl+d,<leader>q`（与 :131 queued_prompts 同键并存，靠注册顺序仲裁）。行为面不完全一致——习惯 `<leader>q` 退出的用户得到队列面板而非退出。

### G-104 ⚠️ `stats --limit` 缺 `>=1` 下界
- Rust：`oc-cli/src/cli.rs:348` 裸 `u32`，`--limit 0` 可解析（渲染层随后报上限提示）。
- Ref：`commands.ts:332-335` Schema 要求 `>=1`。修法：`value_parser!(u32).range(1..)` 一行。

## 归类与收编（不另立项）

- **experimental AI 模块（evaluation/system-one）缺失** → 已有裁决：G-7（有意不移植，ref 主流程不消费）/G-17。
- **markdown 代码围栏无语法 injection、csharp/kotlin/swift feature 门控** → 纯重复 G-13。
- **插件进程外 bun 沙箱（超时/崩溃重启/热重载）与 TSX 有界 shim** → G-28（文档化决策）。
- **CodeMode execute 走插件宿主 guest 运行时** → 第五轮已收编为文档化决策（codemode.rs:21-52）。
- **`POST /api/plugin/install` rust 独有端点** → G-17 rust-extra 家族（第五轮归类项）。
- **Session.remove 10s 有界等待 / restart 恢复 / model-transport** → G-79 ✅ 已收口（remove_tree + 有界等待为文档化决策；jobs.rs `prepare_resume`/`DEFAULT_MAX_RESUME_ATTEMPTS`/`RESUME_EXHAUSTED_MESSAGE` 均已实现；model_request.rs:888,1075-1186 有 http_interceptors + WS handshake 对等面）。

## 第九轮勘误（经复核撤销的初报，勿再立项）

1. **restart 恢复服务未移植** — 初报未找到等价物，实际 `oc-core/src/jobs.rs:636-726` 已有 `prepare_resume`/`RESUME_EXHAUSTED_MESSAGE`/`DEFAULT_MAX_RESUME_ATTEMPTS`（对齐 restart.ts:14-24,67-84）。
2. **model-transport 拦截层不完整** — `model_request.rs:888,915,1071-1186` 已有 `http_interceptors`（request/response 对，response 可回放 before-request 快照=G-91 修复产物）与 WS handshake 派发面。
3. **persistent-pty 回放分块 1MiB（与 G-10 的 64KiB 冲突）** — 复核 `oc-server/src/handlers/pty.rs:342` 确认按 64KiB 字符边界分块；G-10 记录正确，1MiB 说法是初报误读（1MiB 为 read 分块上限，非 WS 回放帧）。

## 第九轮判定汇总
初报 20 项：确认立项 12（G-93..G-104，🔴 1：G-93；🟡 7：G-94..G-101；🟢 4：G-102..G-104）；归类既有 5（G-7/G-13/G-17/G-28 + G-79 关联组）；撤销 3（见勘误）。优先级建议：G-93 > G-98 ≈ G-94 > G-96 ≈ G-99 > G-95 ≈ G-97 ≈ G-100 ≈ G-101 > G-102 ≈ G-103 ≈ G-104。

---

# 第十轮 burndown 收口（2026-11-02，gap-burndown-6）

> G-93..G-104 全部 12 项经 lanes 1-4 串行落地并 fast-forward 合并 main（55032d4 / 3ee4453 / d02a439 / c63e134）。全程 worktree 隔离（`.worktrees/`，合并后已回收）、dev-gate 单入口、nextest 未过滤验证、日志落盘 `.test-logs/*-lane[1-4].log`。各 lane 报告含字面 `Summary:` 行与 `git log -1` 落地证明。

- **G-93 ✅（lane1）**：`oc-tui/src/components/composer/mention_realign.rs` 全量移植 ref mention.ts realign DP（group-by-text、longest-first、保护区间、memoized 最小代价最大匹配）；`Composer::set_text_realigned` 保留幸存 @file/@agent/@skill 附件；$EDITOR 返回分支改调之。偏差文档化：char-offset DP（非 display-width）、无文本 mention 丢弃。
- **G-94 ✅（lane2）**：permission create handler 透传 payload agent/metadata/source（AssertInput 字段本已有，handler 此前硬编码 None）。注：Permission.Request 双侧均无 agent 字段，agent 流入 AssertInput（wire 断言 metadata/source）。
- **G-95 ✅（lane2）**：form create 透传 payload metadata。
- **G-96 ✅（lane2）**：`GET /api/session/:id/context` 截断至最近一次**已完成** compaction 行之后的窗口（ref history.ts latestCompaction gte 语义；running/failed compaction 不扩窗）。
- **G-97 ✅（lane2）**：`GET/DELETE /api/permission/saved` 不再 resolve_location/装配 services（对齐 ref 前置中间件注册）；无效目录不再 404、不触发 boot 级联。
- **G-98 ✅（lane3）**：webfetch timeout（<=0 / >120）校验移至 permission.assert 之前（ref 解码期拒绝语义）；测试证明非法 timeout 不触发权限弹窗。
- **G-99 ✅（lane4）**：`upgrade --method curl` 走官方安装脚本路径——下载 `https://opencode.ai/v2/install` 后 `<bash> <installer> --version <v> --no-modify-path`（对齐 ref updater.ts，非 pipe-to-sh 初稿设想）；github 保持 release 下载；`OPENCODE_INSTALLER_URL` env 覆盖；网络/spawn 桩测试。
- **G-100 ✅（lane3）**：G-59 JSON Schema 校验器上移为共享 `oc-ai/src/json_schema.rs`（oc-core→oc-ai 依赖方向合规，registry.rs re-export 保持 API）；generateObject 输出校验补齐 oneOf/anyOf/allG、$defs+$ref、minLength/maxLength/pattern、minimum/maximum、minItems/maxItems/uniqueItems、additionalProperties、type 联合；零新依赖。
- **G-101 ✅（lane1）**：workspace file-changes 对话框渲染逐文件 A/D/M + 路径 + `+N/−N` 计数（8 行窗口 + 省略提示）；数据经 worktree worker 在 forceRequired 拒绝时拉 vcs.status（失败降级空表）。偏差：固定 8 行窗口（非滚轮 scrollbox）。
- **G-102 ✅（lane1）**：`oc-tui/src/scroll_accel.rs` 字节级移植 MacOSScrollAccel（3 样本窗、150ms streak、`1+0.8·(e^(v/3)−1)` 钳 6×）+ CustomSpeed + 配置序；`Store::wheel_step` 消费 live `scroll.acceleration`/`scroll.speed`。偏差：整行取整、transcript 保持分页步长映射。
- **G-103 ✅（lane1）**：`app.exit` 默认键位补 `<leader>q`（keybind.ts:48）；队列面板存活时（queuedPrompts 非空）app.exit 对 chord 续键让位——复刻 ref 注册序仲裁。
- **G-104 ✅（lane4）**：`stats --limit` 加 `value_parser!(u32).range(1..)`，0 在解析期拒绝。

## 第十轮判定
G-93..G-104 全部 ✅。现存未关闭项维持既有文档化决策：G-28（有界 TSX shim）、G-32（v1/legacy 不移植）、G-12/G-13/G-16/G-17/G-38/G-39/G-40 ℹ️。

---

# 第十轮（2026-11-02，全量逐行复审 + 逐项源码复核）

> 来源：`CODE_REVIEW_PARITY_2026-11-02.md`（9 通道逐行对读 + 6 轮补审 + 4 通道缺口复核：core/server、ai/client、tui、cli/schema/util），明细 `/tmp/opencode/review/verify_*.md`。对全部初报缺口逐项双侧源码取证后：**确认 60 项**（下列 G-105..G-164，续编号；各明细报告草拟编号互相冲突，一律按本节重排）、**驳回 13 项**（见文末第十轮勘误）、**DUP 归并**若干（G-6/G-17/G-32/G-49a/G-7/G-92，已就地追加备注或既有裁决覆盖）。风险分区：🔴 高 4 / 🟡 中 21 / 🟢 低 32 / ℹ️ 登记 3。

## 🔴 高

### G-105 ✅ step 流截断无收尾（fragment flush 缺口）（core）
- Rust：publisher.flush 仅 StepFinish（publisher.rs:869）与 failAssistant（:1055）触发；流在 TextStart/Delta 后**无 finish 事件也无错误事件**即终止时，unknownFinish 判定（step.rs:462-463）只认"收到未知 reason"，缺 finish 不触发；终局结算门（step.rs:548）要求 finish/failure 至少一项存在 → 开放 text/reasoning part 无 trailing delta、无 text.ended，且整步不发布 step.ended（永久悬空 part）。
- Ref：publish-llm-event.ts:132 `Effect.ensuring(flush())` 包住整条流消费，任何终止路径都收尾。
- 复现：桩 provider 发 TextStart+Delta 后直接关流。修法：流消费收尾处无条件 flush + 放宽终局门。

### G-106 ✅ CodeMode 目录指令四处偏差（codemode 组）（core）
- ① catalog 变更更新恒为整文替换：codemode.rs:868-874（注释自认 Phase 10 refinement）+ 唯一调用点 session/runner/context.rs:508；ref codemode/instructions.ts:32-124 的最小增量（新增/变更行、namespace 库存、no-longer-available 清单、与整文择短）未移植——目录每次变动模型收到全量目录文本。
- ② namespace.description 整体丢弃：codemode.rs:657,708 恒 None，namespace_line（:754-759）不渲染 ` // desc`，head 预算（:652-662）不计描述成本；ref catalog.ts:64,87,105,131,173 保留。
- ③ DESCRIPTION_LIMIT 截断按字节切片：codemode.rs:588-589 `&first_line[..117]`——多字节描述跨边界直接 panic（ref catalog.ts:69 JS slice 按 UTF-16 码元，永不 panic）。
- ④ cost 按字节除 4：codemode.rs:581-583；ref catalog.ts:188 按 UTF-16 码元——非 ASCII 内容成本被高估 1~3 倍，round-robin 提前断供。
- 复现：注册带中文描述的 Code Mode 工具即触发 ③④；目录任何增删即触发 ①。

### G-109 ✅ `PATCH /api/session/:id` 空标题行为相反（core）
- Rust：oc-server/session.rs:546-555 对任意字符串（含空串）一律发布 session.renamed（注释自认 TODO(phase-9)），空标题把会话改名为空。
- Ref：handlers/session.ts:262-271 空标题触发 `SessionTitle.generate()` 用 LLM 重新生成。
- 复现：`{"title":""}`——TS 重出新标题，rust 得到无名会话且永不恢复。

### G-118 ✅ DigitalOcean 提示缓存整体失效（provider 装配层，双层）（ai）
- 第一层：digitalocean 经 native.rs:118-120 映射后不在 facade.rs:961-973 `package_has_baked_base` 表 → 走 facade.rs:881-897 generic 分支 → `openai_chat_route`（openai_chat.rs:3220-3231，`OpenAiChatExt::base()` cache_control=false；全文件 `cache_control: true` 仅 venice:3178/openrouter:3273/alibaba:3332 三处）。
- 第二层：generic 分支 RoutePatch 不改 route id → 恒为 `"openai-chat"`，而 cache_policy.rs:39-55 的 `RESPECTS_INLINE_HINTS` 含 `"digitalocean"`（:54）却不含 `"openai-chat"`——`apply_cache_policy`（cache_policy.rs:300-302）对实际路由 no-op，`"digitalocean"` 是永无路由命中的死条目。
- Ref：digitalocean.ts:22-31 `cacheControl()` wrap 落 `cache_control` 断点。修法：补 digitalocean 专属 route，或 RoutePatch 改写 id+ext 开启 cache_control 降级。

## 🟡 中

### G-107 ✅ session.move 的 subpath 复用源 row.path（core）
- Rust：oc-server/session.rs:932-935 `subpath: row.path…`（会话**当前行**的源位置值）。
- Ref：move.ts:89 按 `path.relative(目的地项目根, 目的地目录)` 计算。
- 复现：项目 A 子目录 `sub/` 的会话 move 到项目 B 根——写入 subpath=`sub`（源值）而非 `""`。影响 session.moved 投影、会话行 path 与 location-switched 消息。

### G-108 ✅ session.move 双差异：delivery 缺省相反 + 源不可用快速路径缺失（core）
- ① rust handler（oc-server/session.rs:937-939）与 engine.move_session（engine/mod.rs:1805 硬编码 Queue 且无 delivery 参数）缺省 Queue；ref move.ts:127 `?? "steer"`——默认移动从"中断本轮转向"退化为"排队到本轮结束"。
- ② ref（move.ts:104-116,123,134-147）对不活跃且源目录消失/runner 装配失败的会话在锁内取消积压 move 项并直接 publish Moved（孤儿会话立即迁移）；rust（engine/mod.rs:1784-1811）恒走 admit→wake→等 drain，源 location 已删的会话 move 永远无法落地。

### G-110 ✅ MCP OAuth 授权流四缺口（请求/生命周期侧；G-88 已修的是返回侧校验）（core）
- ① 授权 URL 的 `resource` 参数只发 `host[:port]`（mcp/oauth.rs:765-774），RFC 8707 §2 要求绝对 URI（ref 侧 TS SDK 发完整绝对 URL）——严格授权服务器拒绝请求或签发资源绑定错误的 token。
- ② 回调不校验 `iss` 发行方参数（oauth.rs:805-837 仅查 error/state/code；ref oauth.ts:379,495 经 SDK 校验）。
- ③ token 失效无 invalidateCredentials——死凭证行不删除（ref oauth.ts:290-296 会 remove；rust registry.rs:741,769 只置 needs_auth），陈旧 refresh token 反复走注定失败的刷新。
- ④ authorize 不复用已存凭证中的已注册 client（oauth.rs:719-732 每次重新 DCR；ref oauth.ts:346-351 复用，issuer 变更才丢弃），支持 DCR 的服务器侧积累孤儿注册。

### G-111 ✅ 配对浏览器过期 401 多附 www-authenticate（core）
- Rust：oc-server/pairing.rs:83-92（:89-91）browser 分支显式 insert WWW_AUTHENTICATE——触发浏览器原生认证弹窗。
- Ref：handlers/server.ts:39-42 纯文本 401 无挑战头（刻意避免弹窗）。修法：删掉该 header insert（一行）。

### G-112 ✅ message-updater snapshot.files 语义分叉（core）
- Rust：message_updater.rs:858-862 仅 files 非空时写入，缺失/空数组均保留旧值。
- Ref：message-updater.ts:27 对 `files:[]` 写入空数组、键缺失置 undefined（清除）。
- 复现：一次 step.ended 快照树相等（files=[]）后再来一次 capture 失败（files 缺失）的终局——TS 终态 files=undefined，rust 残留第一次的列表；session.get 消息形状对外可见。

### G-113 ✅ stats 未知时区静默回退 UTC vs TS 400（core）
- Rust：oc-server/session.rs:2119-2135 valid_timezone 仅形状校验；:2616 date_key `TimeZone::get(..).unwrap_or(UTC)`。
- Ref：session/stats.ts:403-404 经 Intl.DateTimeFormat 抛 RangeError → 400 "Invalid time zone"。
- 复现：`?timezone=Invalid/Zone`——TS 400，rust 200 且日期分桶全按 UTC。

### G-114 ✅ permission saved-grant 存储吞掉全部 DB 错误（core）
- Rust：permission/sql.rs:100 `unwrap_or_default()`、:123/:144 `let _ =`——add/remove 静默丢失。
- Ref：permission/saved.ts:48,77,81 三处 `.pipe(Effect.orDie)`。
- 复现：permission DB 只读/加锁后点 "always allow"——grant 静默丢失，重启后授权消失且全程无日志。

### G-115 ✅ integration 主机三差异（core）
- ① Info.metadata 恒 None（integration/mod.rs:394,412 两处硬编码）；ref integration.ts:389 投影 entry.ref.metadata（如 `{source:"mcp"}`）——注册侧元数据在 /api/integration 读取面永远丢失。
- ② command 认证 `.output()` 等进程结束（mod.rs:904-905,920-930），stderr 不增量流入 attempt.message；ref integration.ts:645-670 双流 fork 实时更新——长认证命令期间 UI 零反馈。
- ③ oauth/command cancel 只删 attempts 表项不中断在途交换（mod.rs:761-770,1005）；ref :794,:823 `Scope.close` 中断——取消后在途 token 交换继续跑完并落库陈旧凭证。

### G-119 ✅ deepinfra provider 全线缺口（ai）
- Rust：models-dev 快照 deepinfra 条目无 `api` 字段 → facade.rs:881-887 generic 分支直接 "needs a baseURL" 报错；全仓无 `ends_with("/openai")` 追加；openai_chat.rs:1808-1900 detect 三表均无 deepinfra；facade `.model(id, None)`（facade.rs:544/1145/1285）不注入 compat。
- Ref：deepinfra.ts:10 默认 `https://api.deepinfra.com/v1/openai`、:38-44 自定义 baseURL 去尾斜杠+追加 `/openai`、:46-52 compat（max_tokens、`reasoningField:"reasoning_content"`、no-store、promptCacheKey）。

### G-120 ✅ togetherai/cerebras（含 deepinfra）无默认 baseURL（ai）
- Rust：snapshot 两家均无 `api` 字段；facade.rs:879-887 generic 分支零配置直接 Configuration 报错。
- Ref：togetherai.ts:10 `https://api.together.xyz/v1`、cerebras.ts:10 `https://api.cerebras.ai/v1` 硬编码默认。修法：补 baked 默认或在 facade generic 分支注入。

### G-121 ✅ cerebras reasoningField + 三家 promptCacheKey 缺失（ai）
- Rust：openai_chat.rs:1808-1900 detect 表无 reasoningField/promptCacheKey；compat 注入点 facade.rs:544/1145/1285 全传 None；`openrouter/venice_chat_compatibility()`（openai_chat.rs:3375-3393）仅限 baked 家族；prompt_cache_key 由 compat 门控（:2677-2679，false 则 body 永不带）。
- Ref：cerebras.ts:41-48 `reasoningField:"reasoning"`+`supportsPromptCacheKey:true`；fireworks.ts:51、digitalocean.ts:52 同。修法：facade 按 provider 注入 LanguageModelCompatibility 或扩 detect 表。

### G-122 ✅ oc-client RPC 定义层未移植（ai/client）
- Rust：oc-client/src/rpc_typed.rs 全文 140 行仅泛型 `call_into`（:30），无 per-definition 类型化方法表、无 `rpc.<id>.<event>` 事件过滤、无 on()；全仓零生产消费。
- Ref：promise/rpc.ts:53-145 `makeRpc` 从 Rpc.PortableDefinition 动态生成 141 方法表 + eventType 过滤 + on()。短期无消费方可文档化降级，否则按 oc-schema RPC 清单生成。

### G-127 ✅ composer `@` 文件模糊搜索与 `/cd` 实时目录列表未移植（tui）
- Rust：autocomplete.rs:21-25,483-484,819 三处注释自认 "the async half, not ported"；reference 模式仅剩 skills/references/agents 静态行 + 空查询 recents（:483-491 Reference 分支恒空 Vec）。
- Ref：autocomplete.tsx:336,361（directoryAutocomplete / `client.api.file.find({query, limit:20, location})`）。G-6 正文不含此面（已追加勘注）。

### G-147 ✅ 插件 ctx.model 域缺失（cli/plugin）
- Rust：oc-plugin-host/src/host/host.js:723 起 ctx 构造无 `model:` 命名空间（全 host.js 无 model.list/transform hostRequest；:625 hook 白名单里的 `model.request` 非 ctx 域）；registrations.rs 无 model editor 表。
- Ref：promise/model.ts:25-28（ModelDomain：list/get/default/transform/reload）。与 G-64（provider 域已收口）互补的半边。

### G-148 ✅ ctx.plugin.list 缺失 + effect 风格插件 API 无对应（cli/plugin）
- ① ctx.plugin.list() 未实现（host.js:723 仅 `{id}`；ref promise/plugin.ts:49 Pick<PluginApi,"list">）。
- ② `@opencode/plugin/effect` 导入面完全无对应（ref effect/ 目录 21 文件）——只 import effect 风格的插件不兼容。与 G-28（TSX 有界 shim）不同面；建议在 G-28 生态 breaking change 声明中并列记录 ②。

### G-149 ✅ auth import 把任何 credential.create 失败都当"已存在跳过"（cli）
- Rust：oc-cli/handlers/auth.rs import 段 `if client.credential().create(&body).await.is_ok()` 单分支——网络/5xx 失败也计入 skip，最终 "skipped N already present" 且 exit 0。
- Ref：auth/import.ts:11-57 仅 ConflictError 计 skip，其他错误失败。网络故障下导入静默丢失且脚本无法检测。

### G-150 ✅ CLI auth `--answer` 表单系统降级为 key=value 解析（cli）
- Rust：auth.rs:259-271 parse_answers 仅 `key=Value::String`——无交互表单（text/boolean/multiselect/external）、无 when 条件激活、无 unknown/inactive field 与 pattern 校验、布尔/数值字段无法传 JSON 值；logout/switch 无 TTY 选择器（auth.rs:806-846 resolve_account 无 target 即报错）。
- Ref：commands/handlers/auth/form.ts（answerForm 全表单面）、chooseIntegration/chooseCredential。G-61 修的是集成选择器半边，非 DUP。

### G-151 ✅ mcp auth 架构分歧 + 两行为缺口（cli）
- 架构分歧为有意设计：CLI 进程内 PKCE+loopback（mcp.rs:364-365 注释自述）vs ref 服务端 integration oauth（mcp/auth.ts）。
- 行为缺口：① 无 name 时无 mcpAuthChoices 交互选择器（TTY 也强制要求 name）；② 已有有效凭据无 re-auth 确认对话（ref 先列 connections 并 remove 旧凭据）。

### G-152 ✅ 根命令缺 UpdatePreflight 与 Env.session() 注入（cli）
- Rust：misc.rs 根 handler（:17-63）无 UpdatePreflight（ref default.ts:24 升级替换期的终端提示接管）、无 Env.session() 服务会话注入；oc-cli 无 packages.prepare / TUI updater subscribe 接线。
- 10 分钟后台更新循环本身在位（main.rs:71 spawn_background_update_check，通知走 state/update-notice.log）——初报该半边不成立。

### G-156 ✅ updater/uninstall 残留（cli）
- 包名恒 `@opencode-ai/cli`（updater.rs:81 DEFAULT_PACKAGE 常量；ref 从 release metadata.package 解析）；npm 分支无条件 `--force`（:118；ref 条件加）；vp 恒 `update`（:136）无 `install --force` 跨包迁移、pnpm/yarn 跨包迁移报错分支缺失；uninstall 不执行包管理器卸载命令（execute_removals :1104-1110 仅目录/注册/shell；ref uninstall.ts:113-125 执行 removal(method).run + win32 RetainedImage）。
- 包管理器安装检测缺失为文档化有意裁剪（updater.rs:354-357 注释，G-17 族），但显式 `--method npm` 安装后的自更新/卸载链路因此断裂。G-99 只覆盖 curl 方法语义（已修）。

### G-160 ✅ 反序列化/校验松弛三处（schema/theme）
- ① untagged OAuthValue/Disabled(bool) 接受 `true`（oc-schema/mcp.rs:97-103、config/websearch.rs；ref 仅字面量 `false`）。② resolveHue 不拒绝未知 step 键（oc-theme/resolve.rs:381-398 静默忽略 vs ref tui/resolve.ts:224 `throw "Unknown hue step"`）。③ v1 主题 null 值静默跳过（oc-theme/v1.rs:82 `Json::Null => Ok(None)` vs ref 视为已定义抛错）。畸形输入在 Rust 下更宽容，配置错误无诊断。
- 2026-11-02 第十一轮收口（lane9）：① `OAuthValue`/`WebSearchSelection` 改手写 serde（`true` → 反序列化错误 "expected … the literal `false`"，对齐 ref `Schema.Literal(false)`，测试 oauth_value_rejects_true / selection_rejects_true）；② resolve_hue_entry 对未知 step 键报 `Unknown hue step at "hue.<name>.<step>"`（对齐 ref resolve.ts:222-225，测试 unknown_hue_step_errors）；③ v1 `Json::Null` 改抛 `Invalid V1 theme color`，variant 的 mode 键存在但为 null 同样抛错、present-but-null 的 `selectedListItemText` 视为已定义（ref v1-migrate.ts:354,367）；`backgroundMenu` 按 ref 真值语义（null → 回退 backgroundElement，v1-migrate.ts:372），测试 null_values_error。

## 🟢 低

### G-116 ✅ PTY 服务三差异（core）
- ① 输出 cursor/buffer_cursor 按字节计数（pty.rs:176,:188）；ref pty.ts:206 按 UTF-16 码元——多字节输出的会话 cursor 经 WS/跨进程传递与 v2 客户端不可互换（回放跳段或重复）。
- ② 订阅回调 panic 无隔离（pty.rs:173-183 持锁裸调；锁为 parking_lot 不毒化，但 panic 沿 reader 线程 unwind 致数据流静默中断、会话卡 running）；ref pty.ts:214,300 catch 后移除订阅者继续。
- ③ win32 的 LC_ALL/LC_CTYPE/LANG=C.UTF-8 注入缺失（pty.rs:459-460 仅 TERM/OPENCODE_TERMINAL；本构建非 win32 目标，低危前瞻项）。
- **2026-11-07 第十五轮勘注（状态回退 ❌）**：第十一轮对 G-105..G-164 的逐条 ✅ 标记对本条失真——现行代码三子项均未落地（① pty.rs:176 仍按字节计数；② pty.rs:173-189 `on_data` 持锁裸调、全 crate 无 catch_unwind；③ pty.rs:466-467 仍仅注入 TERM/OPENCODE_TERMINAL），第十一轮 lane 清单与 git log 均查无本条修复提交。回退为 ❌ 重开（①③ 低危/前瞻；② 生产回调仅 channel send，实际风险有限）。win32 gitbash 发现（rust 仅 oc-server/server_process_env.rs:23 透传 `OPENCODE_GIT_BASH_PATH`，ref 消费链在 shell/select.ts）并入 ③ 同族登记。详见文末第十五轮。
- **2026-11-08 第十六轮收口 ✅**：三子项经 gb9-lane5 落地（①cursor/buffer_cursor 改 UTF-16 码元计数；②订阅回调 catch_unwind 隔离+违规者摘除（on_data/replay/on_end 全臂）；③win32 LC_ALL/LC_CTYPE/LANG=C.UTF-8 注入 + shell::select gitbash 发现（OPENCODE_GIT_BASH_PATH），均 cfg(windows) 门控、纯逻辑函数未门控测试）。PTY 族 `--profile pty` 序列化门 5328/5329（唯一失败=白名单 canary）。

### G-117 ✅ 凭证存储两处微偏差（core）
- activate/update 额外更新 time_updated（integration/credential.rs:239,:285-297；ref credential.ts:124-130,227-228 的 set 不含该列）——外部可见 mtime 漂移。
- 无 integration_id 的行以 `integration_id=""` 进入 all() 结果（credential.rs:71,:132 `unwrap_or_default()`；ref :69 直接丢弃）。复现：手动插入 NULL integration_id 凭证后 `credential list`——ref 隐藏，rust 返回空串条目。
- **2026-11-08 第十六轮勘注（lane12 抽查→lane13 收口）**：time_updated 半边此前未落地（与 G-116 同型误闭，lane 清单/git log 查无修复提交）——现 oc-core CredentialStore（`2362d051`）与 oc-server HTTP 重复 SQL（`04e4ce93`，保留本地 SQL 避免改 wire）两路均已修：activate/update 仅写 active/label/value，插入仍播种 time_updated。NULL integration_id 过滤半边实际由 G-225 收口顺带落地（credential.rs:364-376 `stored_credentials` 过滤，归因订正）。

### G-123 ✅ togetherai/digitalocean env 回退链缺失（ai）
- Rust：snapshot togetherai env 仅 `["TOGETHER_API_KEY"]`、digitalocean 仅 `["DIGITALOCEAN_ACCESS_TOKEN"]`；facade.rs:551-558 bearer_auth 只按 catalog env 顺序构造。
- Ref：togetherai.ts:37 `[TOGETHER_API_KEY, TOGETHER_AI_API_KEY]`；digitalocean.ts:36 `[DIGITALOCEAN_ACCESS_TOKEN, DIGITALOCEAN_API_KEY, DO_INFERENCE_API_KEY]`。

### G-124 ✅ media Generation 缺 events()/await() 观察流（ai/client）
- Rust：oc-ai/src/media/route.rs:812-905 仅有 token/first_snapshot/status/result/cancel/poll_read；Observation 事件类型在 media/model.rs:65-144 有定义但 oc-core/oc-server 零消费。
- Ref：generation.ts:111-145 `events()`（schedule 轮询推送 queued/progress/finished，含 deadline 钳制）+ resultEvents。补 events() 或文档声明降级。

### G-125 ✅ oc-client SharedEvents 终止错误只投递一次（ai/client）
- Rust：shared_events.rs:341-344 err_yielded 守卫——只产出一次 Err 后流结束（注释自认刻意偏离，ClientError 非 Clone）；terminal_error()（:330-334）为补偿 API。
- Ref：shared-events.ts:76,123 完成时 reject 全部 pending、completion.error 后每次 next() 都 reject。跨语言消费者按 TS 语义写的循环会提前退出。建议文档标注契约差或改投递 Arc<ClientError>。

### G-126 ✅ SSE 16MiB 上限按字节 vs TS 按字符（ai/client）
- Rust：sse.rs:12 MAX_SSE_EVENT_BYTES、:39/:200 按字节计数。
- Ref：client.ts:385-386 按解码后 JS 字符（UTF-16 码元）。含 CJK/emoji 的长事件 Rust 提前报 SseEventTooLarge。按 chars().count() 或文档钉定即可。

### G-128 ✅ 通用 spring/tween 动画引擎未移植（tui）
- Rust：全 crates 无 createAnimatable/advanceSpring；session_tabs.rs:14 注释 "motion springs stay C2"；tool_detail.rs websearch 静态渲染。
- Ref：ui/animation.ts（advanceSpring）+ retry-provider.tsx:19-20（FADE=80/RESIZE=60）。连带丢失：RetryProvider fade/resize、tab 宽度 spring、pane-resize 平滑过渡、title-shimmer/DIM 回退（session_tabs_solid.rs:773-795 静态渲染，无动画分支）。

### G-129 ✅ display_width 缺文本呈现型 emoji 收窄规则（tui）
- Rust：wrap.rs:89-91 `text.width().saturating_sub(merged_cells(text))`（unicode-width；merged_cells 只处理 Brahmic）。
- Ref：string-width.node.ts:9-26（width==2 且非 EAW2 且 textEmoji 且无 VS16/20E3/ZWJ → 记 1）。❤ 等字符 Rust 记 2、TS 记 1。

### G-130 ✅ splitAddedPatch 未移植（tui）
- Ref：util/diff.ts:18-38（新增文件大补丁按 size 切块）。Rust 全 src grep `-0,0`/split_added 零命中，diff_viewer.rs 直接解析 hunk。

### G-131 ✅ dialog-select alt+↑/↓ 跨 section 跳转未移植（tui）
- Ref：dialog-select.tsx:367-374,547-562（sectionNavigation）。Rust dialog_select.rs grep alt+up/move_section 零命中。

### G-132 ✅ 导出选项交互表单折叠为 4 静态行（tui）
- Rust：command_palette.rs:1051-1059 四条 SelectOption，描述写死 "thinking off, tool calls on"/"unsanitized"。
- Ref：dialog-export-options.tsx:11-42（defaultThinking/thinking/tools/sanitize 交互态）——用户无法配置导出内容。

### G-133 ✅ openUrl 丢失 scheme 校验；仅 xdg-open（tui）
- Rust：store.rs:8519-8531 任意 scheme 直接交 xdg-open（无 http/https 校验）；dialog.rs:1697-1706 openPath 同款。
- Ref：link.tsx:27 → util/open.ts:3-12（npm open 三平台 + openUrl 校验 scheme）。openPath/openUrl 仅 xdg-open 在 Linux 目标可接受（建议文档化）；scheme 校验是真缺口。

### G-134 ✅ mini 完成态工具结构化快照未移植（tui）
- Ref：mini/tool.ts（1358 行：toolView/snapWrite/snapEdit/snapPatch/snapTask/toolScroll、代码块/diff 卡/任务卡/可滚动展开）。Rust mini/ grep 零命中；transport.rs tool_caption 自述 "caption subset"，仅保留行内 caption。

### G-135 ✅ model-preference setFavorite 写路径缺失（tui）
- Rust：mini/model_pref.rs 仅 ModelPreferenceDocument{recent,favorite,variant}（:117,126 favorite 只读保留，无 set_favorite）。`model.dialog.favorite` 键位已注册但收藏永不持久化。Ref：model-preference.ts setFavorite。

### G-136 ✅ Route 枚举无通用 prompt 携带字段（tui）
- Rust：store.rs:47-50 `enum Route { Home, Session { session_id } }`（仅 app.rs:215 arm_startup_prompt 专用通道）。Ref：context/route.tsx:12-19（HomeRoute.prompt/SessionRoute.prompt）——任意导航时预填草稿不可达。

### G-137 ✅ `session.toggle.thinking`/`scrollbar` 键位注册但无 dispatcher 臂（tui）
- Rust：oc-tui src grep `session.toggle.thinking|toggle_scrollbar` 零命中（对照 verbosity.cycle 已在 command_palette.rs:247-252 正确接线）。Ref：index.tsx:1024-1049 写 config.update。按键无任何效果。

### G-138 ✅ session.undo 缺 pending 分支（tui）
- Rust：store.rs:7721-7762 session_undo_pressed 无条件 revert_stage。Ref：index.tsx:985-992（pendingDeliveries.has → mutatePending("cancel")，否则 interrupt→wait→revert.stage）——仍在 inbox 排队的消息被错误走 interrupt+revert.stage 而非 inbox.cancel。

### G-139 ✅ crossterm 键翻译 CONTROL 分支只匹配 Char（tui）
- Rust：app.rs crossterm_key_to_spec:9133+ `match key.code { Char(ch) => …, _ => None }`——ctrl+shift+tab（BackTab+CONTROL）返回 None，`session.tab.previous` 主键位 PTY 路径失效（alt+up 别名可用）。

### G-140 ✅ session 路由长尾组（7 合 1）（tui）
- ① AssistantRetry 重试倒计时 footer 未建模（Message 无 retry 字段）；② TurnTokenUsage 调试表缺失；③ Write/Edit 后 LSP Diagnostics 行缺失；④ 导出转录裁掉工具 Input/输出正文（app.rs:6388 仅 `**Tool: name**` vs ref index.tsx:3503-3533 含 Input JSON+输出全文）；⑤ pending-steer 点击管理（Move to queue/Delete）缺失（app.rs:1120-1126）；⑥ --auto 权限失败不回滚 autoApproved（rust fire-and-forget vs ref index.tsx:266）；⑦ session.redo 无 enabled 门（ref index.tsx:169-175 revert?.messageID 条件）。

### G-141 ✅ 嵌入式 PTY 面板主题调色板注入未移植（tui）
- Rust：terminal_pane.rs grep osc/palette/oklch 零命中。Ref：terminal-pane.tsx:338-385（OSC 4/10/11 + rgbToOklch 色相匹配）——PTY 输出用终端默认配色。

### G-142 ✅ cliErrorMessage 友好错误格式化链未移植（tui）
- Rust：grep cliErrorMessage/"Did you mean" 零命中（app.rs error_to_string 原始 to_string 进 toast）。Ref：util/error.ts:5-26（递归 + "Did you mean: …" 建议、ConfigInvalidError issue 明细）。

### G-143 ✅ tabs spinner 仅 dots 样式；indicators 设置不生效（tui）
- Rust：session_tabs.rs:37,42 仅 dots（注释 "TAB_SPINNERS.dots"）；dialog_config.rs:243 有 tabs.indicators 设置行但 config.rs 不解析、渲染器无 numbers 分支。
- Ref：TAB_SPINNERS 4 样式（arcs/quadrants/line 缺）+ 4 种未读标记。G-49a 只钉定 dots 帧数学（已追加备注）。

### G-144 ✅ autocomplete 触发检测用 char 偏移而非显示宽度偏移（tui）
- Rust：autocomplete.rs:513-546 trigger_for 基于 `Vec<char>` 索引。Ref：prompt/display.ts:41-61 slashTriggerIndex/mentionTriggerIndex 基于 displaySlice + promptOffsetWidth。语义等价，但宽字符混排时 @ // 触发锚点可能与 ref 差 1+ 列（需实测）。

### G-145 ✅ 插件渲染崩溃隔离降级 + activity-summary 未实现（tui）
- ① 插件渲染失败 eprintln 降级，无 "crashed in slot" 错误 toast（ref ErrorBoundary）。② activity-summary 独立视图仅存注释引用（message/list.rs:64-68），低 verbosity 的工具/思考折叠摘要未实现。（未知键位 log+skip+toast 半边为已文档化有意偏离，不立项。）

### G-146 ✅ dialog-config "Color mode" 设置搜索关键词为空串（tui）
- Rust：dialog_config.rs:91-98 keywords: ""（相邻 colors 条目有 "color scheme, colors"）。Ref：dialog-config.tsx:35 `["dark mode","light mode","system theme"]`——palette 搜 "dark mode" 不命中。（title-shimmer/DIM 半边并入 G-128。）

### G-153 ✅ plugin list 不发现本地目录插件；版本截断不校验 hex（cli）
- Rust：plugin.rs list 段无 localPluginDirectories/localSource；:60 `version.len() == 40 || == 64` 只看长度（任意 40 字符版本串被截 7 位）。Ref：plugin/list.ts:14（本地发现）、:94 正则 `/^(?:[a-f0-9]{40}|[a-f0-9]{64})$/i`。

### G-154 ✅ api/models 输出差异（cli）
- Rust：misc.rs models 段 `models.sort()` 字节序；api.rs 仅 Basic auth。Ref：models.ts:18 Service.headers、:23 localeCompare；api.ts:29 同。oc-util 已有 localecmp.rs 却未消费。

### G-155 ✅ stats 输出降级（cli）
- ① 活动日历网格/图例/COLORFGBG 亮暗调色/ANSI 颜色缺失（stats.rs:2 注释自述 REGISTERED simplification；ref stats.ts:165-194,93,100,319-342）；② "best streak" 在 activity 行与 tool 行重复输出两次（:288,:302）；③ 日界 UTC（:123 rem_euclid 86400000、TZ||"UTC"）vs ref 本地午夜 + Intl IANA（stats.ts:40,144-163）——非 UTC 时区天数桶起点可能偏移；④ 数字无千分位。

### G-157 ✅ mini 三偏差（cli）
- ① 不 chdir 到 $PWD（mini.rs:203-211 仅字符串采纳；ref mini.ts:201-209 真 chdir 失败报错）——相对路径语义不同。② reconnect seam 恒 None 永不重连（mini.rs:234；ref handlers/mini.ts:29 接 250ms 重连循环）。③ 新增 30s 总 deadline（mini.rs:126；ref 无限重试直到 abort）。G-68（fragment/image 回放，已修）不含本条。

### G-158 ✅ server-connection 无 onStart；standalone 无 Options.command（cli）
- server_connection.rs grep onStart/Starting background 零命中（ref server-connection.ts 有 "Starting/Restarting background server..." 提示回调）；standalone.rs:116-117 恒 self_command（无自定义命令）、:160 新增 60s ready 超时（ref 无限等待）。

### G-159 ✅ ACP getSession 目录匹配用 canonicalize（cli）
- Rust：acp/service.rs:379-381 双侧 `std::fs::canonicalize`（解析 symlink）。Ref：acp/service.ts:100 `FSUtil.resolve`（词法归一）。经符号链接进入的会话目录匹配语义不同（Rust 更宽松）。建议注释钉定或改词法归一。

### G-162 ✅ util 残留两小项（util）
- ① npm registry 解析仅 4 层级联（npm_config.rs:23-29 env→项目→用户→默认）；ref 走 @npmcli/config 全量（globalconfig/builtin 等）——企业全局 .npmrc/镜像配置行为不同。② Hash.fast(sha1) 无统一 util（hash.rs 仅 sha256_hex，sha1 散落调用点各自引入）。glob brace/extglob/scan 见 G-92 已钉定。
- 2026-11-02 第十一轮收口（lane9）：① 级联扩为 env → 项目 → 用户 → globalconfig（`$PREFIX/etc/npmrc`）→ builtin（`$PREFIX/lib/node_modules/npm/npmrc`）→ 默认（npm_config.rs 模块头文档化）；有界偏离：`PREFIX` 仅取 `NPM_CONFIG_PREFIX`/`npm_config_prefix` 环境变量（@npmcli/config 会从 node/npm 安装推导，本端口不 spawn `npm config` 子进程），未设 prefix 环境变量时跳过 global/builtin 两层（测试 globalconfig_after_user / no_prefix_env_skips_global_levels）。② oc-util hash.rs 新增 `sha1_hex`，迁移机械调用点：oc-cli util.rs `hash_fast`、service_config.rs default_port/legacy_filename、oc-core project.rs `hash_fast`、snapshot.rs（删除手写 SHA-1 实现）、engine/skills_url.rs（弃 ring SHA1）、tests/database_identity.rs；oc-cli/oc-core 的独立 sha1 依赖随之移除（统一走 oc-util）。有界偏离：oc-ai（SDK crate，不依赖 oc-util）bedrock credentials 的 AWS SSO 缓存名 sha1 保留本地 `sha1` 依赖（非 `Hash.fast` 语义，不迁移）。

## ℹ️ 登记

### G-161 ✅ Global roots 两记录性偏差（util）
- ① APP_NAME="opencode-rust"（global_roots.rs:29；ref global.ts:31 "opencode"）——有意共存设计（legacy_v2_roots 提供迁移源，已有注释），补文档登记。② `Roots::ensure`（:505-519）不做 tmp realpath 规范化（ref global.ts:92-93 以 canonicalTmp 为比较基准）——macOS /private/tmp 与 /tmp 混用时路径比较可能失配，建议 canonicalize 或注释钉定。
- 2026-11-02 第十一轮收口（lane9）：② 已修——`Roots::ensure` 签名改为返回 `io::Result<Roots>`，建目录后对 tmp 做 `fs::canonicalize`（失败回退原路径），对齐 ref acquire 的 realpath 语义（测试 ensure_canonicalizes_tmp：符号链接 tmp 规范化后相等）。① 维持有意共存设计，本条登记为文档化决策（migration source = legacy v2 roots）。

### G-163 ℹ️ workerd 运行时目标与 ServerFetch.make 不移植（server）
- Rust：grep boot.rs/oc-server 无 workerd/ServerFetch，仅原生 `boot_and_serve`（axum）。Ref：server/src/fetch.ts（web-standard handler 嵌入）、workerd.ts（运行时替换层）。结构性不可移植（无 JS 运行时嵌入目标）；已收编 G-17 族文档化范围裁剪（本条为登记，见 G-17 追加备注）。
- 2026-11-02 第十一轮：登记收口——维持 G-17 族文档化范围裁剪裁决，Rust 侧仅原生 boot_and_serve（axum）为最终形态，无后续工程项。

### G-164 ℹ️ announce.rs 为位置事件级联的服务端替身（server）
- Ref 由各 core 服务自持 notify（reload/transform → bus.publish）；Rust 由 oc-server/announce.rs 按装配/请求时机合成（模块头 :1-15 自述；engine/mod.rs:652 F-5 注释佐证；含 project.updated 60s 限速、九个空 *.updated 帧复刻，有 parity 测试）。结构性 seam：新增 location 服务/事件需同步维护 announce 级联清单，漂移风险集中该文件——保持 parity 金样并在此登记。
- 2026-11-02 第十一轮：登记收口——结构性 seam 维持现状；新增 location 服务/事件时必须同步维护 announce.rs 级联清单并保持既有 parity 金样（约束性登记，无代码项）。
- 2026-11-08 勘注（G-218 落地后前提订正）：announce 不再是 `filesystem.changed` 的发布者（engine watcher 显式盯 `<gitdir>/HEAD` 唯一发布；announce 改订阅该事件按 HEAD-basename 谓词发布 `vcs.branch.updated`，弃 250ms 轮询）。模块头描述已随代码更新；「announce 替代 core 自持 notify」的原前提就此失效，其余 announce 级联清单义务与 parity 金样不变（gb9 lane7 `4877e6dd`）。

### G-211 ℹ️ split-footer / ExternalOutputCommit 整族延后（tui，mini 走薄缝）
- opentui renderer.ts:2339+ 的 split-footer（makeRoom/ExternalOutputCommit）家族不移植：主 TUI 全量 alt-screen，无消费面；mini 的 split-footer 交互于 Phase B4 落地为**薄缝**——dock 由 oc-tui 自有 GridEmitter 画在全屏网格底部行（上方恒 blank，diff 不触转录区），`insert_before` 以「擦 dock → 底行滚动 → CUP 写行」等价实现，键入走 KeyPipeline 非阻塞路径。行为由 oc-cli `mini_pty.rs`（4 测试）与 oc-tui mini 系列（含 `pty_daily::mini_subagent_footer`）钉定。
- 登记：ADR-0018（`docs/adr/0018-opentui-full-render.md`）「窄化与偏差」节；如后续出现 split-footer 消费面（如主 TUI 内嵌分屏），再决最小 makeRoom 移植。
- **2026-11-10 OBSOLETE（本条目过期，以收官注为准）**：本条「薄缝/不移植」描述已被 2026-11-05 tui-full-convergence 收官注（见本册 G-211 收官注/G-13 条目后）推翻——split-footer 整族（makeRoom/ExternalOutputCommit/ScrollbackSurface/DECSTBM）经 ADR-0019 **全量移植并消费**（opentui-core renderer/scrollback.rs、mini/lifecycle.rs:1710-1845 真引擎，pty_daily.rs:2507/2580 钉定），主 TUI 消费面即 mini 真引擎本身。本旧条目仅作历史沿革保留，勿据此处判断现状。

## 归类与收编（不另立项）

- **experimental AI（evaluation/system-one）** → G-7（附 G-17），第九轮同款裁决。
- **`POST /api/plugin/install` 客户端方法** → G-17 rust-extra 家族（第五轮已裁决，methods.gen.rs:2201 与服务端路由同源）。
- **tui feature-plugins/system/storybook 工作台** → G-17（开发期 fixture 同族，已追加备注）。
- **run 非 TTY 权限行为** → G-17（TTY 提示为有意增强，非 TTY 路径 auto-reject 与 ref 一致）。
- **run/v1.ts 输出兼容层** → G-32（v1/legacy 族，已追加备注）。
- **schema 五子项（rpc.ts/legacy-event/durable-event-manifest/models-dev 管道/v1/*）** → G-32 明文范围（G-87 已裁决，已追加备注）。
- **glob brace/extglob + Glob.scan** → G-92（注释级钉定）。
- **未知键位 log+skip+toast**（tui #20 半边）→ 已文档化有意偏离（代码注释+测试），非缺口。

## 第十轮勘误（经复核撤销的初报，勿再立项）

1. **Formatter 为 no-op** — formatter.rs 同时含生产 `BuiltinsFormatter`（:384 真实命令执行，:779 impl；接线 engine/mod.rs:1241）与仅测试用 NoopFormatter（:99）；formatter.rs:1-7 模块头"ships a no-op"为陈旧注释（建议顺手清理）。
2. **git clone/fetch/checkout/resetHard 与 vcs.ts/patch.ts 未移植** — clone/fetch/checkout -B/reset --hard 全在 oc-core/reference/repository_cache.rs:241-290；vcs 全家在 oc-server/handlers/project.rs:968-1818（split_git_patch :1709、countPatch/addPatch :853-880）；第五轮勘误 #5 结论维持。
3. **MCP reconcile 用 PartialEq 替代深度相等** — ServerConfig（oc-schema/mcp.rs:103-109）全类型化（String/BTreeMap/bool/u16/枚举），derive PartialEq 即结构化深度相等且 BTreeMap 键序恒定，与 isDeepStrictEqual 等价。
4. **子代理 prompt 绕过 SessionPrompt.prepare** — tool/subagent.rs:368-377 裸 admit，但 drain 消费边界统一兜底：runner/drain.rs:487,524-543 → prompt_prepare.rs:141 对所有 pending user 行跑 hook + @skill（第五轮勘误 #3 同族裁决）。
5. **isTransientError 丢 cause 链遍历** — oc-util/retry.rs:144 在全仓零生产调用；唯一生产瞬态分类器 MCP 侧自建链完整（mcp/client.rs:273-276 沿 source() 收集、:1375-1377 chain.join 进错误消息）。
6. **Flock 心跳/可观测性丢失** — flock(2) 崩溃恢复语义严格强于用户态锁目录协议（内核自动释放），heartbeat/stale-breaker 无行为后果；互操作边界已由 oc-tui/storage.rs:44-49 显式文档化。
7. **服务生命周期整套未移植** — 已移植在 oc-server/src/lifecycle.rs（2599 行）：discover/probe（:86-385）、ensure 三次超时强杀（:936-952）、版本不匹配重建（:980-999）、SIGTERM→SIGKILL 升级（:1115-1160）；初报只扫了 oc-client。
8. **pty-handoff sidecar 缺失** — 生产侧 lifecycle.rs pty_handoff_prepare/environment/complete/clear（:1235,:1010,:1356,:1365）、消费侧 boot.rs:774-775 读取并清除 OPENCODE_PTY_HANDOFF（:989-1065 测试），四函数一一对应。
9. **AbortSignal 取消全链路缺失** — Rust drop future 即中止在途请求（reqwest/SSE 语义等价）；SharedEvents 有显式 stop()→task.abort()（shared_events.rs:212-217）；timeout 包调用即得 deadline。
10. **pty connect 两 endpoint 生成为 execute_json** — 功能由 oc-tui/app.rs:6585 完整承接（connect-token→ticket→WS 升级流）；死封装自带 TODO(pty-ws) 注释钉定（methods.gen.rs:1959-1960,2533-2534）。
11. **connect_token Envelope 解包丢 location** — 初报配对错位：persistent-pty endpoint 两侧同取 data（TS types.ts:6382 同样只取 data）；pty endpoint 两侧都含 location（types.gen.rs:9898-9905 vs TS types.ts:6280 不解包）。逐 endpoint 全等。
12. **可执行 Tool 运行时不在 oc-ai** — 跨 crate 归属：oc-core/src/tool/registry.rs 完整承接（模块头 :2-6 自述 port of runtime.ts execute + tool.ts executeTool，:192 注释明言），非缺失。
13. **serve 缺 PTY handoff / EADDRINUSE 15s 重试** — 均在 oc-server：lifecycle.rs:1334-1377（handoff 继承+decode_inherited_handoff，boot.rs:776 接线）、:1478+（incumbent 100ms×15s 重试环）+ :1505-1512 逐字复刻的 "Managed service port…" 文案；serve.rs 经 boot::boot_and_serve（:143）全链路接线。〔2026-11-07 第十五轮订正：半失真——handoff 接线属实；但 EADDRINUSE 重试环 `bind_or_incumbent`（lifecycle.rs:1487-1522）生产零调用（全仓 grep 仅测试引用），生产链 serve.rs:103-127 仅启动前一次性 incumbent 预检、lifecycle::serve:1571 `bind(options)?` 一次绑定冲突即 Err；立项 G-217。〕

## 第十轮判定汇总
初报 79 项：确认立项 60（G-105..G-164：🔴 4 = G-105/106/109/118；🟡 21 = G-107/108/110-115/119-122/127/147-152/156/160；🟢 32；ℹ️ 3 = G-161/163/164）；驳回 13（见勘误）；DUP 归并 6 处（G-7/G-17×3/G-32×2/G-92 + G-6/G-49a 勘注）。core/ai 侧共同根因（G-118..G-121）：provider 目录装配层是 parity 录制盲区——facade generic 分支 + detect_* 表 + models.dev 快照三处都没有 TS provider 文件里 hardcode 的 baseURL/env 回退/compat 注入，建议为受影响 provider 补 baked 默认与 `*_chat_compatibility()` 表（复用 openai_chat.rs:3375-3393 既有模式）。

## 第十一轮 burndown 收口（2026-11-02，gap-burndown-7）

第十轮立项的 G-105..G-164 全部 60 项经 lanes 1-9 串行落地并合并 main（worktree 隔离、dev-gate、nextest `--workspace --locked --no-fail-fast` 证据、合并门复跑；日志在 `.test-logs/*-lane9*` / 各 lane 报告）。逐 lane 提交：

- lane1 `043408d`：G-105/106/109（+ G-106 组内 CodeMode 目录指令收口）。
- lane2 `3269839`：G-118..G-123。
- lane3 `822d72d`：G-107..G-115。
- lane4 `88b25b5`：G-147..G-156。
- lane5 `a28e380`：G-127..G-134。
- lane6 `e516182`：G-135..G-146。
- lane7 `e3395dc`：G-122/124/125/126。
- lane8 `3480df9`：G-154/155/157/158/159。
- lane9（本轮）：G-160/162 代码修复 + G-161/163/164 登记（合并 main，见文末 git log）。

状态：G-105..G-164 逐条改标 ✅（代码落地或登记收口）；G-163/G-164 维持 ℹ️（结构性登记），G-161 ② 已修 ① 文档化。第十轮判定汇总中的 🟡/🔴 分级至此全部清偿。

### 各 lane 有界偏离（bounded deviations）清单

- **G-105**：step failed-not-ended 的终态形状提升（hoisted）到统一收尾路径——与 ref 逐分支 inline 形状不同但终态集合等价。
- **G-106**：UTF-16 近似（按注记点用码点/字节近似 UTF-16 码元语义）。
- **G-108②**：仅目录已消失（directory-gone）快速路径；其余源不可用场景走常规失败分支。
- **G-122**：validated dispatch（定义级校验派发）而非 per-fn codegen——RPC 定义层等价、无逐函数生成物。
- **G-124**：stream seam——events() 观察流上游不消费（poll 驱动边界在 seam 注释钉定）。
- **G-128**：动画 pump 以 100ms 粒度驱动（ref rAF 帧）；spring/tween 数学一致。
- **G-129**：静态 emoji 表（text-presentation emoji 收窄规则以表驱动近似 ref 运行时判定）。
- **G-134**：无虚拟化/语法高亮（mini 完成态结构化快照渲染为静态卡片）。
- **G-136**：PromptInfo 仅文本承载（无 ref 的 Route.prompt 结构化字段族）。
- **G-140**：折叠摘要（7 合 1 长尾组按子项分别落地，部分交互为摘要级）。
- **G-141**：OSC 输入流注入（调色板经 OSC 4/10/11 注入，rgbToOklch 匹配为静态实现）。
- **G-143**：仅复刻 ref 默认样式（dots）；其余 spinner 样式与 indicators 设置不生效维持裁剪。
- **G-147**：快照式 editor（插件 ctx.model 域以快照提供，非 ref 的实时视图）。
- **G-148②**：effect API 仅 clear-error 钩子（ref 的 effect 风格插件 API 子集）。
- **G-150**：multiselect 自定义控件 once-per-session（会话级一次性渲染约束）。
- **G-153**：有界本地发现（localPluginDirectories 的子集扫描）。
- **G-156**：包名探针回退（release metadata.package 缺失时回退 DEFAULT_PACKAGE 探测）。
- **G-158**：Options.command 纯 seam（standalone 自定义命令以无副作用的 seam 暴露，默认仍 self_command）。
- 本轮（lane9）新增有界偏离：G-162① npmrc 级联的 `PREFIX` 仅取环境变量（不 spawn npm，不推导 node 安装前缀）；G-162② oc-ai bedrock SSO 缓存名 sha1 保留本地依赖（SDK crate 不引 oc-util）；G-161② canonicalize 失败回退原路径。

现存未关闭项仍均为文档化决策（G-17 族、G-28、G-32 等），无新增待办。


---

## 第十二轮（2026-11-03 全量对比审查）新登记缺口（G-165..G-210）

> 背景：22 条并行审查通道对端口范围内全部包（core/tui/cli/client/server/schema/theme/plugin/util/ai/codemode/merman）做逐文件全文对读（TS ~21 万行 / Rust ~40 万行生产代码），再经 3 条独立核验通道对 50 项缺口声称逐条双侧源码取证：46 项立项、4 项撤销、若干子句修正，全部与既有 G-1..G-164 去重。总报告 `CODE_REVIEW_PARITY_2026-11-03.md`，域报告与核验记录 `/tmp/opencode/review/`（session/tools/ai_*/plugin/mcp_auth/shell_pty/config/schema/bus_db/fsvcs/server/client/cli/theme_util/codemode/core_misc/tui_*/render_mermaid + verify_v1..v3）。

### 🔴 高

### G-165 ✅ Azure provider catalog 通道整体未接线（ai）
- facade.rs:346-347 把 catalog `azure` 归入通用 OpenAIResponses 族；:795-801 仅 `bearer_auth`（env 链把非密钥 `AZURE_RESOURCE_NAME` 当凭据候选，snapshot env + catalog.rs:255 + facade.rs:556-562），无 api-key 头/api-version/resourceName/deployment URL。
- openai_responses.rs:42,573 无 baseURL 兜底 → 未配 baseURL 静默错发 `https://api.openai.com/v1`；custom 通道有 azure 守卫（facade.rs:1362-1370 fail-fast）但 catalog 通道无。
- oc-ai/protocols/azure.rs（392 行，azure_auth:103-109 / resource_base_url:40 / endpoint_parts:123）生产零调用；open_responses_channel.rs:424-425 虽引用其 WS helper，但 `responses_channel_enabled`(azure.rs:177-192) 要求 `.openai.azure.com`+`api-version=v1`，generic 路由永不满足 → 事实死代码。ref 基准 ai/src/providers/azure.ts:29-33,113-128。

### G-166 ✅（wire bug）`Mcp.Protocol` serde 变体名大写（schema）
- oc-schema/src/mcp.rs:26-35 枚举无 `rename_all`（仅 Dated 有 rename="2026-07-28"）→ wire 输出 `"Legacy"/"Auto"`；ref schema/src/mcp.ts:20 为小写 literals。
- 该类型即 v2 config 类型（config/mcp.rs:20 re-export、config/normalize.rs:28 消费）→ `"protocol":"auto"` 反序列化失败。修法：`#[serde(rename_all="lowercase")]`（Dated 变体保留显式 rename）。

### G-167 ✅ Copilot anthropic 车道 + 动态模型面整体缺失（ai）
- **anthropic 逃逸通道缺**：ref 对 advertise `/v1/messages` 的模型由 models.ts:164 将 `package` 改写为 `@opencode/ai/providers/anthropic` 实现 lanes（github-copilot.ts:325-342 的 hook 不消费 "messages"）；rust copilot_responses.rs:94-101 分发不认 messages 落到 chat lane，且 CopilotModelEntry（copilot.rs:327-341）无 package 字段 → Claude-on-Copilot 错发 /chat/completions。
- **动态模型同步死代码**：load_copilot_models/derive_copilot_models（copilot.rs:347,398）全仓仅 tests/copilot_models.rs 引用；facade 唯一 copilot import 是 copilot_chat_route（facade.rs:45）→ 新模型/别名/enterprise 端点不可见，仅静态 catalog。
- **派生字段缺**：ref models.ts:129-196 另产 family/modalities/variants/released/cost(AIC→USD)/limit；rust 仅 id/model_id/name/enabled/endpoint/base_url。
- **gpt-5-chat-latest 未隐藏**（ref github-copilot.ts:259-268 enabled=false，rust 零命中）；**免费标题通道 utilityTitle（gpt-4o-mini）未移植**（ref :398,415-475 + title hook :455-475）。

### G-168 ✅ 第三方 Anthropic 根 URL 拼错（moonshot/zai-coding-plan/alibaba）（ai）
- rust catalog.rs:192-199 把 snapshot `api`（chat 根）写入 settings.baseURL；AnthropicMessages 臂（facade.rs:787-792）仅 `with_base_url`，PATH="/messages"（anthropic_messages.rs:59）→ 拼成 `{chat根}/messages`。
- ref 基准：moonshot.ts:86 `api.moonshot.ai/anthropic/v1`；zai-coding-plan.ts:45 `api.z.ai/api/anthropic/v1`；alibaba.ts:94-98 `${host}/apps/anthropic/v1`。minimax 不受影响（其 catalog api 本身是 anthropic 根）。facade 无 per-package anthropic 根特判（仅 bedrock-mantle :644 有）。

### G-169 ✅（🔴 安全）shell 权限扫描：注释后换行被跳过 → 命令合并逃逸权限分类（core）
- rust shell/scan.rs:1841-1848：`#` 注释分支 `index = newline + 1` 跳过换行；ref scan.ts:584-588 是 `index = newline - 1`（循环尾 ++ → 换行被访问，走 scan.ts:590 的 `;<>&|\n` 分隔符分支结束当前命令）。
- 后果：`echo hi # note\ncurl evil` 在 rust 合并为**单条命令** `echo hi curl…`，save 前缀按 ARITY 取 `echo *` → 注释后一行任意命令逃避独立权限分类；且注释行处 pending heredoc 不在该换行读取。修法：改处理换行（等价 ref `index = newline` 再走分隔符路径）。

### G-170 ✅ session transfer export 脱敏弱于 ref（core）
- rust 服务端有 export 端点（oc-server/handlers/session.rs:2142-2174，`?sanitize=true`），但 sanitize 仅删 info/messages 的 `metadata` 字段（:2163-2172）；oc-core session/transfer.rs 只有 import（:34-70）无 export 服务。
- ref transfer.ts:165-230 的 isSettled 过滤（:60,165-170）、redact 占位体系（:171-176）、title/directory/revert/text 脱敏（:180-230）全部缺 → sanitize 导出仍包含 ref 会脱敏的正文内容。与 G-75（CLI 面）互补不冲突。

### 🟡 中

### G-171 ✅ OpenAI/Azure gpt-5 默认选项 + chat 线 promptCacheKey 缺（ai）
- ref openai-options.ts:35-47 `gpt5DefaultOptions`（reasoningEffort:"medium"/reasoningSummary:"auto" + gpt-5 门控排除 gpt-5-chat/pro）rust 零命中；openai.ts:103-106 两 lane 均应用。
- chat 线 promptCacheKey：ref openai.ts:108/azure.ts:132 `compatibility:{supportsPromptCacheKey:true}`；rust openai/chat 绑定（facade.rs:891-897）无 compat → openai_chat.rs:2683-2686 `unwrap_or(false)` 丢弃。
- 已按 route 默认落地的不算缺：store:false + include reasoning.encrypted_content（openai_responses.rs:534-552 = ref route defaults）。G-121 根因扩展。

### G-172 ✅ MiniMax chat / Meta messages 专用协议未移植（ai）
- ref minimax.ts:58-75,91：`thinking` 选项 + `reasoning_split` 默认 true（防 `<think>` 内嵌）+ compat{store/strict:false}；rust minimax/chat → 泛化 OpenAIChat（facade.rs:383），oc-ai 无 minimax overlay。
- ref meta.ts:113-118 + meta-messages.ts:44-45：MetaMessages（web_search 降级、拒 searchContextSize、thinking adaptive 默认）+ requireSignature:false；rust meta/messages → 泛化 AnthropicMessages（facade.rs:334）。
- 2026-11-06 第十三轮勘注：meta-messages 的 **body wrap 已落地**（customization.rs:199-271 `meta_wrap`：web_search 重映射 + searchContextSize 拒绝；注册 :345、主链路调用 anthropic_messages.rs:1655/1810）——该半边收口。仍缺：minimax/chat 专用协议（thinking/reasoning_split/compat）与 meta 路由默认 thinking（adaptive/display omitted）；requireSignature 半边归 G-174。另注 ref minimax 的 messagesRoute 本就是纯 AnthropicMessages（minimax.ts:81），rust 侧 messages 泛化无差距。

### G-173 ✅ ToolSchemaProjection 未中央化（ai）
- ref route/client.ts:565 在 prepareRequest 对**所有**路由统一应用 objectRoot + 按模型名清洗；rust `project_tools` 仅 gemini.rs:1380 / mistral_chat.rs:525 / cohere_chat.rs:146 调用，anthropic_messages.rs:574、openai_chat.rs:1982、openai_responses.rs:648、bedrock/converse.rs:252 裸用 `input_schema` → 无 object 根的 schema 与 kimi 清洗在主链路缺失。修法：在 Route::prepare_request 统一调 sanitizer::resolve(model)::project_tools。

### G-174 ✅ requireSignature 语义靠启发式（ai）
- ref 显式 `requireSignature:false`：alibaba.ts:104 / meta.ts:141 / zai-coding-plan.ts:72 / vercel-ai-gateway.ts:264（仅 anthropic/ 前缀 true）；rust anthropic_messages.rs:1053-1080 仅 kimi/moonshot/xiaomi 启发式 → 四家全落 true，无签名 thinking 回放失败。
- compat 钩子在协议侧存在（:1055-1060 读 `compatibility.require_signature`）但无注入方：facade package_chat_compatibility(:1063-1072) 不含 messages 族、overlay_model_compatibility(:1096-1108) 不转发、oc-schema model.rs:71-86 Compatibility 无该字段 → 配置亦无法表达。

### G-175 ✅ chat 族 compat preset 注入不全（12+ 家）（ai）
- facade.rs:1063-1072 仅注入 deepinfra/togetherai/cerebras/fireworks/digitalocean 五家（=G-119/G-121 修复成果）；专用工厂（facade.rs:871-887）与 chat_route 工厂（openai_chat.rs:3198-3223）均不带 compat，deepseek/groq 等 preset 函数仅 `#[cfg(test)]` 引用（openai_chat.rs:3626-3633）。
- ref 注入 compat 的 chat 族：deepseek.ts:47 / groq.ts:92-98 / openrouter.ts:199 / venice.ts:43 / zai.ts:51 / zai-coding-plan.ts:68 / alibaba.ts:97 / minimax.ts:119 / moonshot.ts:111 / cohere.ts:43-48 / openai.ts:108 / azure.ts:132 / meta.ts:136 —— 全部未注入。G-118..G-121 根因扩展。
- 2026-11-06 第十三轮勘注：oc-ai 已新增 deepseek/groq/openrouter/venice/zai/alibaba 六家 preset 函数（openai_chat.rs:3390-3452，字段与 ref 对齐）但**仍仅测试消费**（tests/body_parity.rs:263-268 + openai_chat.rs:3626-3637 family_compatibility 测试助手），route 工厂与 facade 注入表均不烘焙 → 生产仍仅五家；minimax/moonshot（含 sanitizer:"moonshot"）/cohere/openai/azure/meta 六家连 preset 都没有。

### G-176 ✅ flowchart 汇流排分组键=edge_index（3/4 臂失效）（tui）
- routing.rs:899-905 `source_key()/target_key()` 恒返回 `self.edge_index` → 每组单元素 → `len()<2` 恒跳过；失效臂：horizontal_fan_in(:809)/vertical_fan_out(:841)/vertical_fan_in(:873)。**horizontal_fan_out(:738) 按真实节点 id 分组功能正常**（同文件两种写法并存）。
- ref merman routing.ts:426,468,488,509 按 `edge.from`/`edge.to` 分组。修法：三臂改用 diagram.edges[i].from/to。

### G-177 ✅ TUI Markdown 表格降级为 raw 文本（tui）——已闭环
- 闭环方式：opentui-core rev bump 3606fda→93814b4 移植 `TextTableRenderable`（opentui-rust 上游 markdown.rs 表格 token→chunk-grid→`BlockSpec::Table{TextTableOptions}`，3 个新差分场景 IDENTICAL）+ oc-tui 渲染臂补 `BlockSpec::Table` 接线（markdown.rs `render_spec` Table 臂：`TextTableState::from_options` → `update_layout` → `prepare_paint` → `paint` 进 scratch `OptimizedBuffer` → `get_span_lines` 转 styled span 行）。
- 证据指针：crates/oc-tui/src/components/message/markdown.rs `BlockSpec::Table` 臂 + `captured_span_to_styled` + 测试 `table_renders_as_a_real_grid`（钉住 `┌─┬─┐`/`├─┼─┤`/`└─┴─┘` 边框、表头首行、逐行分隔、ref 的 cellPaddingX=1 与 raw pipe 不残留）；workspace Cargo.toml opentui 三 crate rev=93814b4。

### G-178 ✅ mermaid 开放边 `A --- B` 仍渲染箭头（tui）
- drawing.rs:262-273 `draw_routed_edge` 无条件画箭头（无 arrowhead 检查）；:480-482 虽对 `!edge.arrowhead` 补画线尾，但 style.rs:169-171 `merge_flowchart_cell` 的 `is_arrow_head(existing) → return existing` 使先画的箭头胜出、线尾被丢。ref drawing.ts:187-195 在 drawRoutedEdge 内先判 `edge.arrowhead !== false`。修法：draw_routed_edge 增加 arrowhead 参数。

### G-179 ✅ mermaid seam 无流式截断恢复，宽度未接（tui）
- markdown.rs:613-657 语法错误直接回退代码块；全仓 grep last_good/truncate-retry 零命中；ref merman markdown.ts:272,290-314 有 lastGood Map + 截断重试。
- 宽度 clamp：渲染层管线在（render/mod.rs:84-119 layout_max_width.min(context_width)）但 seam 传 `MermaidRenderOptions::default()`（markdown.rs:631，context_width:None）→ 实际宽度从未接入。

### G-180 ✅ codeme 沙箱文档化决策新增子项：Logs 捕获缺失 + guest fetch 暴露（codemode）
- 沙箱架构（自研解释器→Bun guest）已收编文档化决策（见上文收编节），但两条子项为该决策下**新增**未登记面：
- ① console 捕获缺失：host.js:168-171 将 console 重路由为 NDJSON log 通知，不进执行结果 Logs（:2364-2365 自认）；ref stdlib/console.ts:9,21 捕获入 ctx.logs → 结果里 console 输出对模型不可见。
- ② guest fetch 暴露超 ref 面：ref codemode **无任何 fetch 面**（grep 零命中）；rust guest 为裸 Bun fetch 无超时/大小上限 —— 网络出口是 ref 沙箱不存在的能力面。

### G-181 ✅ mini 四缺口：model 偏好死绑定 / /new 不可用 / SIGWINCH / 断线恢复（②③第十三轮修；①④第十四轮 gap-burndown-8 lane9 收口）（tui/mini）
- ① runtime.rs:178 `let _preferences = ModelPreferenceRepository::new(...)` 下划线死绑定，全文件仅此一处（G-135 修的是 set_favorite 写路径，死绑定仍在）→ mini 内无法切换模型/变体。
- ② /new 恒 "new sessions unavailable"（runtime.rs:490-496）；③ Lifecycle::resize 存在（lifecycle.rs:1139）但 mini 全目录零调用、无 SIGWINCH（主 TUI app.rs:9583 才有）→ resize 后 wrap 宽度错；④ :232-250 断线 500ms 盲重连同 client，无 reconnect client 替换、无 hydrate 重放（transport.rs:590-591 自认 "the live port boots once and never reconnects"）→ 断线窗口事件丢失。
- 2026-11-06 第十三轮勘注（commits 5cc50385/f1f132f9 属实，②③收口）：②/new 已可用——runtime.rs:580-618 非 demo 走服务端 `create_session`（:973）并换 session_id/transport/subagents/queue/history + "new session {id}"（仅 demo 保留 unavailable）；③resize 已接——lifecycle.rs:1513/1532/1569（resize/poll_resize/observe_window_size，TIOCGWINSZ 轮询等价 SIGWINCH，ADR-0018），runtime.rs:362-364,716 调用；④250ms 重连 + hydrate 重放大半落地（runtime.rs:44,262-297,346-357：每轮新 subscribe、RECONNECT_MARKER、needs_hydrate→replay_history）但 client 实例从不替换（ref stream-v2.transport.ts:1533-1539 换 sdk）、transport.rs:590-591 旧注释已过时宜改；①读侧已接（runtime.rs:197-198 bind_model_preference→boot_model）但**写侧/切换面仍缺**（save_variant 零生产调用，mini 命令面仅 /exit /new /compact，ref runtime.ts:295-308 cycleVariant+saveVariant 无对应）。

### G-182 ✅ 消息内多文件 patch 只渲染第一个文件（tui）
- tool_detail.rs:283 `files.first()`；ref index.tsx:3333-3378 `<For each={files()}>` 每文件一张卡 → ApplyPatch 多文件变更只显示 1 个。

### G-183 ✅ Question 工具无专用展示臂（tui）
- tool_detail.rs:630-864 臂集无 "question"，走 generic `question [k=v,...]` 兜底（:839-863）；ref index.tsx:3424-3458 有专用问答块（answered → "# Questions" 问答对；pending → "Asking questions…"）。

### G-184 ✅ 内联 shell 输出无流式拉取（≠G-33）（tui）
- G-33 的 1s 轮询仅 DialogShellOutput 查看器（app.rs:4407-4412 + store.rs:15733-15748 测试）；消息内联臂（tool_detail.rs:210）仅静态 `state.output`。
- ref index.tsx:2778,2826-2889：内联臂 cursor 增量 `shell.output` 拉取 + 1s 轮询 + SHELL_DISPLAY_LIMIT=1MiB + "[earlier output omitted]"——rust 全仓零命中。

### G-185 ✅ prompt history 降级：纯字符串、无回灌（tui）
- composer/mod.rs:232-255 `Vec<String>` 环（上限 100≠ref 50），无 PromptInfo（files/pasted 恢复）；storage.rs:254-268,448-465 prompt-history.jsonl 仅启动 heal+重写，解析结果不回灌 composer（:458-463 仅日志）。（注：stash 全套已在位——见勘误 #3。）

### G-186 ✅ slash 命令表硬编码，不消费服务器 command.list（tui）
- autocomplete.rs:140-251 V2_SLASH/SLASH_COMMANDS/ARGUMENT_SLASHES 三张静态表；oc-tui 全目录 grep `command.list` 零命中；ref autocomplete.tsx:486-494 将服务端命令并入候选 → 插件/自定义命令永远不出现。与 G-127（@-file 搜索，已修）不同面。

### G-187 ✅ 工具输出图片（ToolImages）未渲染（tui）
- rows.rs:536-580 image_strip_row 唯一调用点在 user_message_row；tool_detail 全臂 `images: Vec::new()`、tool_rows.rs 零图片引用；ref index.tsx:2456-2523（SessionImages + inlineToolImages）。mini 侧 tool_image_commits（mini/image.rs:102）仅作 dedup 标记。

### G-188 ✅ 权限阻塞工具分区（partitionPending）未移植（tui）
- ref rows.ts:71 + grouping/session.ts:163-176：等待授权的工具在折叠组外独立展示并沉底；rust components/message 零匹配（model.rs:610 的 pending 是排队输入，另一概念）→ 权限等待工具的可见性丢失。

### G-189 ✅ DialogSelect 三缺：details 行 / locked / Tab action 焦点（tui）
- ① SelectOption 无 `details` 字段（dialog_select.rs:45-65；:484-485 自认 wave-2）；② 无 `locked` 冻结；③ dialog.rs:737-753 键位层无 footer-action 焦点循环（ref dialog-select.tsx:434-443 moveAction+Tab；details 行 :816-824）。G-131（section 跳转）已修（dialog.rs:751-752），不同面。

### G-190 ✅ PowerShell `separator_after` 反引号续行组未实现 → save 授权串失稳（core）
- parse.rs:201-217 docstring 写着 ``/^\s+(?:`(?:\r\n|\r|\n)\s*)?/`` 但实现只贪心消费空白；ref parse.ts:278-279 正则含续行组 → 含续行的 PS 命令两侧生成不同通配授权串，升级后既有授权失配（重复弹权限）。

### G-191 ✅ subagent / mcp-resource 工具 executor 绕过执行管道（core）
- subagent.rs:652-656、mcp_resource.rs:265-277 直接 `register_executor` 裸注册；engine/mod.rs:2200-2205/2378-2381 直接调 register。标准路径 registry.rs:136-165 PipelineExecutor（before hook :207-227 → decodeInput :229 → leaf → after hook :294-334 → normalize_content :107）。
- 后果：这两工具不吃 tool.execute.before/after hook、不 decodeInput、不 normalizeContent（ref subagent.ts:102 / mcp-resource.ts:17,49 经统一管道 tool.ts:150）。可见 bug：read_mcp_resource 纯文本资源 content=[]（TS 由 normalizeContent 回退 stringify）。修法：改走 register_builtin/PipelineExecutor 包装。

### G-192 ✅ websearch：disable 移除 hook 缺 + Form kind 元数据缺（core）
- ref websearch.ts:184-192 在 context/compaction/generate 三 hook 于 disabled 时删工具；rust websearch.rs 无任何 session hook（grep 零命中；engine/mod.rs:2455-2459 直接 register_builtin）→ 用户选 disable 后工具仍出现在请求列表。
- ref websearch.ts:78,111 `metadata:{kind:"websearch.provider"}`；rust 两处表单（websearch.rs:373 及 choose 表单 :436 附近）`metadata: None` → 依赖 kind 分发的客户端无法路由。

### G-193 ✅ 插件内建行为组 5 项缺失：plan/identity/verbosity/optimize/init+review（core）
- 全仓库零命中：plan 模式注入（ref plugin/plan.ts；engine/mod.rs:2656-2713 seed_builtin_agents 无 plan agent）、模型 identity 注入（ref identity.ts:16-21 "# Your Model"）、GPT verbosity 自动 low（oc-ai 无 textVerbosity 注入；cli_config 的 verbosity 是 v1 schema 非请求注入）、per-provider system-prompt 优化模板（ref optimize.ts:10-16：gpt/astra/kimi/meta/trinity/anthropic）、command initialize/review（ref command/ 模板）。
- （skill opencode+report **已在位**：engine/skills.rs:92-133 include_str 内嵌——见勘误 #4。）与 G-147/148（已修的 ctx.model/effect API）不同批。

### G-194 ✅ compaction `due()` 边界：checkpoint 无 measured 时继续估算（core）
- ref compaction.ts:242-251 `measured < findLastIndex(isCheckpoint)`——checkpoint 存在而 measured=-1 时 `-1 < idx` → return false（未到期）；rust runner/compaction.rs:916-941 `if let (Some,Some)` 才比较，measured=None 时继续 `estimate >= ceiling` → 刚完成 native 压缩、尚无新回复的会话可能重复触发压缩。
- 2026-11-06 第十三轮勘注：代码形状分叉属实（:930-937 双 Some 才比较），但 :918-927 的 Completed-last 守卫（ref :244-246 同款）已挡住上述主后果——剩余误触发窗口仅「压缩完成后再发一条且 estimate 仍≥ceiling」的一拍，后果较登记收窄。

### G-195 ✅ native compaction 两偏差：webSocket:false + finish=error 不重试（core）
- rust compaction.rs:638-671 prepare 硬编码 `web_socket:false`（:660）；ref compaction.ts:335 native 传 `"session"`（:569-590）→ rust native 压缩不走 session WS 缝（model_request.rs:914 仅 true 才生成 WebSocketSeams）。
- finish reason=error：ref compaction.ts:545-547 → `AIError(UnknownProviderError)` 可重试；rust compaction.rs:845-852 `FinishReason::Error => DeliverError::Failure`（Failure=永不重试）。

### G-196 ✅ LocationActivity 驱逐语义：无 sweep 播种、无中断后复查（core）
- rust location_services.rs:393-410 sweeper 对 expired 直接 interrupt+invalidate+remove；ref location-activity.ts sweep 先对 RcMap 缓存位置重新 touch（缓存位置不因空闲过期），中断 owners 后复查 execution.active，有新占用则 touch 保留不 invalidate。rust 有等价 bus touch（:364-380）但缺这两条 → 长空闲但 live 的位置被误驱逐、或驱逐刚接管的会话位置。

### G-197 ✅ oc-client：~49 个方法缺 404 declared + 全部方法无 per-request RequestOptions（client）
- 按名称对齐 ref promise/generated/client.ts（96 方法，83 个 404 declared）vs methods.gen.rs（142 op，75 个 404）：rust 缺 ~49 个 404 声明（agent.list、config.*、file.*、mcp.*、顶层 permission.*、plugin.*、project.list、provider.list、pty.create/list、session.export/import/skill/wait 等；部分已以别名路径声明如 fs.*↔file.*），另有 ~41 处多声明（命名迁移重合）。404 变 UnexpectedStatus，结构化 LocationNotFoundError 丢失。
- ref client.ts:277-282 `{signal,headers,onActivity}` 每方法尾参透传；rust 全部 142 方法签名仅 `&self, input`，grep RequestOptions/signal/on_activity 零命中。

### G-198 ✅ git/vcs：clone 无 --depth、快照 diff 无二进制检测/10MB 截断（core）
- repository_cache.rs:242-250 `clone <remote> [--branch] <path>` 无 `--depth 100`（ref git.ts:246-258 `--depth (depth ?? 100)`）→ 全量克隆。
- snapshot.rs:416-461 diff_blocking（git2）无二进制检测与 MAX_TOTAL_PATCH_BYTES=10MB 截断（ref git.ts:522-573 + vcs/patch.ts:8）；工作区 diff（oc-server/handlers/project.rs:1298-1300,1450-1535）已有 10MB cap 但同样缺 numstat "-"→patch 置空。与 G-30（git2 bounded）同域互补；BranchUpdated 事件已有（勘误 #5）。

### G-199 ✅ `~/.claude/skills` / `~/.agents/skills` 兼容 skill 根未并入 skills 扫描（core）
- rust config/discovery.rs:14,44-48 收集 .claude/.agents 根仅用于 instructions/config；skills 加载（engine/skills.rs:300-355）只扫 config skill/skills 目录+URL 源，未加 compat roots 的 `/skills`；ref compatibility.ts:39-56 明确扫描 → 从 Claude/Agents 迁移用户丢 skill。

### G-200 ✅（①session.viewed 服务端确认已接；②terminalHandoff 登记为结构性有界分歧，见第十四轮）TUI：session.viewed + 渲染器 terminalHandoff（tui）
- ref tui/context/session-tabs.tsx:209-242 view POST + 指数退避重试；rust oc-tui 仅从 session.list 的 `idle > viewed` 本地推导（data.rs:142-206，:196 自认 refinement 未做）→ 长离线 unread 陈旧。
- ref cli/default.ts:143 + tui/app.tsx:193,218,229-236 渲染器移交（preflight→terminalHandoff）；rust handlers/misc.rs:79-80 直接 `oc_tui::app::run` 自建渲染器（G-152 的 onStart 钩子在，但无 handoff）。

### 🟢 低

### G-201 ✅ Vercel Gateway VERCEL_OIDC_TOKEN 回退失效（ai）
- vercel_gateway.rs:42 `AUTH_ENV=["AI_GATEWAY_API_KEY","VERCEL_OIDC_TOKEN"]` 全仓无生产消费；实际 env 链 = catalog `["AI_GATEWAY_API_KEY"]`（经 facade.rs:815 生效）→ 仅 OIDC token 回退丢失；facade.rs:813 注释自称含 OIDC 亦不实。G-81 残留。

### G-202 ✅ 裸 `$NAME` 扫描字符集缺 [A-Za-z]（core）
- scan.rs:2809 `"_0123456789@*#?$!-"` vs ref scan.ts:1126 含字母 → `$FOO` 走普通字符路径、`$` 记入 literal → 更易置 bound_dollar → 与 variableSink/boundOpen 组合产生 ref 不会有的 dynamic-execution 拒绝（fail-closed 方向，非放行）。

### G-203 ✅ ripgrep.find 服务面无 1:1 移植（core）
- ref ripgrep.ts:52-61,193-218（exclude 模式链 `--glob=!`、onEntry 流式）唯一消费方 filesystem/search.ts:57-100；rust tool/ripgrep_runner.rs 仅移植二进制获取。fs find 端点已有原生索引等价实现（oc-server/handlers/fs.rs:254-336 + SearchIndex + ScanExclusions，注释引 ref search.ts:72）→ 记录实现路径差异（原生索引 vs rg 流式）+服务层缺口。

### G-204 ✅ shell 工具 metadata 缺 signal/timeout 字段（core）
- tool/shell.rs:568-578 metadata 仅 status/truncated?/exit?/shellID?；ref shell.ts:83-93 + shell/result.ts:36-43 展开含 signal?/timeout?。rust ShellToolOutput 的 output 本身有这两字段，缺的只是 metadata → 依赖 metadata 渲染 shell 卡片的 UI 缺信息。

### G-205 ✅ toSessionError：缺 Tool.Error 嵌套解包 + Integration.AuthorizationError→provider.auth（core）
- runner/mod.rs:506-542 仅 AiError 分支属实，但其余映射多已分布式存在（tool.execution step.rs:669,323、permission.rejected step.rs:681、no-route drain.rs:564-570、AgentNotFound context.rs:1338-1348）。真缺：ref to-session-error.ts:64-71 嵌套 AIError 递归解包+空 message 回退、Integration.AuthorizationError→provider.auth（integration/types.rs:303 有类型，runner 无映射）。

### G-206 ✅ promptCacheKey 校验接受大写 hex（core）
- model_request.rs:793-802 `is_ascii_hexdigit` vs ref model-request.ts:278 正则仅小写。实际影响≈0：两侧 Identifier 生成器均为 `ses_`+26 字符（identifier.ts:3-26 / identifier.rs:15+），生成的 id 永不命中 64-hex 分支 → 分歧仅在人为构造串可见。

### G-207 ✅ publishAll 批内 created 时间戳共享 + ephemeral data 强校验 is_object（core）
- bus.rs:696 整批一次取时钟（ref bus.ts:531-560 每事件各自取）→ 跨实现混用数据时重放幂等比较可产生假性分歧；bus.rs:625-628（publishAll :686 同）对 ephemeral 事件新增 `!data.is_object()` 拒绝（ref 无形状校验）→ Rust 附加约束，可拒合法发布。

### G-208 ✅ CLI cli-upgrade flock 缺失（cli）
- ref updater.ts:131,399 `flock.acquire("cli-upgrade",15min)`；rust oc-cli grep flock 零命中 → 并发升级竞态无互斥。（本批 update 流其余声称与 G-99✅/G-156✅ 重叠、method 探测=G-17 有意裁剪，仅此点为新。）

### G-209 ✅ Modal 动态模型发现缺失（ai）
- ref core/src/modal/models.ts + plugin/provider/modal.ts（Modal API load/derive：price/limit/variants 模板合成）；rust 全仓库零命中（provider_catalog/plugin-host 均无）。

### G-210 ✅ theme generateSystem 系统主题生成缺失（tui/theme）
- ref 实际位于 packages/tui/src/theme/system.ts:11-60（调色板→系统主题派生）；rust oc-tui/theme.rs:349-366 system 槽回退 v2 opencode 文档——有 OSC 11 模式检测（theme.rs:10-13）但无调色板派生。

## 归类与收编（第十二轮，不另立项）

- **剪贴板 host 写臂** → 重复 G-39（ℹ️ 有意，仅 Osc52Sink：app.rs:7810、store.rs:5885-5893），维持不变。
- **ai experimental/（Evaluation/system-one）** → 归类 G-7（evaluation lane 有意不移植）。
- **session.diff 经 SSE 公开** → 记录性分类面差异，不立项：registry.rs:217-220 注册致 is_public=true（event_manifest.rs:70），但全仓无发布点（仅 HTTP 端点 handlers/session.rs:1366；ref 同样从不发布）→ SSE 实际不转发。若未来开始发布 session.diff，需先并入 PRIVATE_TYPES。
- **插件 aisdk.language 句柄不可替换 / TUI 插件 API 收窄** → 已由 G-2 备注与 G-28 文档化决策覆盖；本轮新证据（plugin_apply.rs:2155）与之一致。
- **`aisdk:` npm 动态安装桥 / 动态 npm provider 工厂** → 归类 G-17 有意省略族（ref sdk-factory 运行时 npm 安装；rust typed error）。

## 勘误（第十二轮经核验撤销/修正的初报，勿再列入待修）

1. **bus log() 有界 64 静默断流** — 不属实：bus.rs:1108 channel(64) 但 `tx.send().await` 为背压等待；send 失败仅当 receiver 丢弃（消费者主动关闭）=预期取消语义；wake 读前 re-arm + cursor 基 read_through（:1114-1121,1165-1189），慢消费者不丢事件。与 ref 差异仅在缓冲策略。
2. **无状态 Generate.text 服务缺失** — 不属实：oc-server/handlers/misc.rs:42-114 已完整实现（POST /api/experimental/generate，routes.gen.rs:288-299；prompt 校验、ModelSelection/Unavailable 映射 :69-91、x-opencode-session 头 :96-100、503 UnavailableError）；差异仅为架构位置（server handler 内联 vs ref core 服务层）。
3. **prompt stash 全套缺失** — 撤销：stash 已全套在位（storage.rs:236-309、store.rs:9555-9658 stash_init 完整恢复 chips、:9757 push、:9789 pop、command_palette.rs:308-352 三命令、app.rs:7693 boot 接线）。仅 history 半边成立（→G-185）。
4. **内建 skill opencode+report 缺失** — 撤销：engine/skills.rs:92-133 include_str 内嵌已注册。
5. **VCS 无 BranchUpdated 事件流** — 撤销：`vcs.branch.updated` 已有（announce.rs:532、sse.rs:719、tests/be1_lane.rs:303）。
6. **workspace 驱动注册表"仅 DB 表"** — 撤销：environment/workspace.rs 有 Registry.register(:82-94)+local driver+create/provision/connect/destroy(:145-237)；deferred 仅 idle janitor/共享 provision/连接池（模块头 :3-10 注释，非 local provider 时才 load-bearing）。
7. **Vercel Gateway env 链为空** — 修正：AI_GATEWAY_API_KEY 经 catalog environment 生效（facade.rs:815）；仅 VERCEL_OIDC_TOKEN 丢失（→G-201）。
8. **session.diff SSE 泄露** — 降级：无发布点，实际暴露面与 ref 一致（见归类节）。
9. **codemode "ref fetch 30s 超时封装未移植"** — 撤销该出处：ref codemode 无 fetch 面（grep 零命中）；rust 裸 Bun fetch 属暴露面超 ref（→G-180②）。
10. **oc-client 404 declared 缺 ~30** — 修正为 ~49（按名称对齐后实测；另 ~41 处多声明为命名迁移重合）。

### G-212 ℹ️ TUI storybook/测试环境变量面不移植（Gate D3 裁决 4，G-17 族）
- OPENCODE_STORY / OPENCODE_ROUTE（ref app.tsx:337-349 storybook/路由注入）、OPENCODE_DRIVE（app.tsx:253）、OPENCODE_NODE_ASSETS_DIR、TMUX/STY 终端探测——均为 ref 测试/storybook 管道非用户可见面；移植=新增超集测试基建，倒置收敛目标。登记于 tui-full-convergence D3-7 终扫（docs/adr/0019 C 类）。

> **2026-11-07 第十四轮 burndown 收口（gap-burndown-8）**：第十三轮复核确认的 45 项可行动缺口（G-165..G-210 全部未收敛项，含补漏立项视角下实际存在的 G-193）经 lanes 1-11 串行落地并 fast-forward 合并 main（worktree 隔离、dev-gate 单入口、nextest 未过滤证据、每 lane workspace 合并门复跑；日志 `.test-logs/*-gb8lane*`）。逐条改标 ✅。**无上游阻挡项**（无需 opentui-rust 未移植功能的搁置项；唯一结构性裁决为 G-200② terminalHandoff，登记为有界分歧而非搁置）。明细见文末「第十四轮 burndown 收口」节。

> **2026-11-05 tui-full-convergence 收官注**：G-211（split-footer 整族，含渲染侧六缺口）已全量移植并消费（ADR-0019 终稿）；G-83 面板按超集令移除（键位=ref 死绑定态）；G-13 已闭；G-177 已闭（Phase C）；G-130 分块沿用（D3-5 消费）。GridEmitter 退役+Frame 内部货币登记见 ADR-0019。

---

# 第十四轮 burndown 收口（2026-11-07，gap-burndown-8）

> 第十三轮复核确认的 G-165..G-210 未收敛项全部落地（45 项 = 44 项计划 + 复盘中发现漏派发的 G-193，已补 lane11）。每 lane 独立 worktree（`.worktrees/gb8-laneN`）串行合并 main，合并门为 workspace 全量 nextest；门证据 tee 于 `.test-logs/*-gb8lane*`。无上游阻挡项、无需搁置项。

## 逐 lane 落地清单

- **lane1 `52af1e96`（core 快修组）**：G-166（`Protocol` 加 `rename_all="lowercase"`，`"auto"/"legacy"/"2026-07-28"` wire round-trip）、G-169（shell 扫描注释分支改走换行分隔符路径——注释后任意命令恢复独立权限分类，heredoc 跨注释行消费）、G-190（PS `separator_after` 消费反引号续行组）、G-202（`$NAME` 字符集补 [A-Za-z]）、G-204（tool_result metadata 经 `shell::result::metadata()` 带 signal/timeout）、G-205（`SessionErrorCause` 嵌套 AIError 递归解包+空消息回退；AuthorizationError→provider.auth）、G-206（promptCacheKey 谓词限小写 hex）。
- **lane2 `fe38d665`（ai provider 装配）**：G-165（Azure catalog 车道接线 `azure_responses_route`/`azure_chat_route`：api-key 头、api-version、resourceName 部署 URL、缺配置 fail-fast 不再兜底 api.openai.com）、G-168（moonshot/zai-coding-plan/alibaba 的 anthropic 根按 ref 改写，用户 baseURL 原样尊重）、G-171（gpt5DefaultOptions 双车道注入（排除 gpt-5-chat/pro）；openai/azure chat 线 supportsPromptCacheKey）、G-201（VERCEL_OIDC_TOKEN bearer 回退接入）。
- **lane3 `9ebdc295`（ai compat/协议）**：G-175（13 家 chat compat preset 全量烘焙：六家已有 preset 转正 + minimax/moonshot/cohere/meta 新 preset）、G-174（schema `Compatibility.requireSignature` + overlay 转发 + alibaba/meta/zai-coding-plan 注入 false + vercel `anthropic/` 前缀规则，kimi/moonshot/xiaomi 启发式保留为缺省回退）、G-172 收口（minimax chat thinking/reasoning_split 默认 true + moonshot chat 方言 + meta messages adaptive thinking 路由默认）、G-173（ToolSchemaProjection 中央化进 `Route::prepare_request`，anthropic/openai-chat/openai-responses/bedrock 主链路受益，gemini/mistral/cohere 重复调用移除）。
- **lane4 `c7b5ff50`（copilot）**：G-167 五子项（messages 逃逸车道 package 重写→`CopilotMessages` anthropic 车道；`CopilotSync` 动态模型同步进生产解析（boot spawn 后经 `c7b5ff50` 修正为凭据门控+1s 延迟，避免与 models-dev 刷新循环竞争——该修正由合并门红测驱动）；family/modalities/variants/released/cost(AIC→USD)/limit 派生；gpt-5-chat-latest 隐藏；utilityTitle 免费标题通道+provider 过滤 hook）。
- **lane5 `47bc1e1a`（core session）**：G-170（transfer export 服务化：isSettled 过滤 + redact 占位体系 + title/directory/revert/text 逐类脱敏，HTTP handler 改走服务）、G-194（due() 精确移植 ref 谓词：checkpoint 后无 measured 回复不到期，关一拍误触发窗）、G-195（native 压缩走 session WS seam；finish=error 改可重试 Provider 错误）、G-196（sweep 对缓存位置 re-touch + 中断后 active 复查，占用中位置不误驱逐）、G-199（`~/.claude/skills`、`~/.agents/skills` 兼容根并入扫描）、G-207（publishAll 逐事件时间戳；ephemeral 非 object 载荷不再拒绝）。
- **lane6 `d7044002`（core tools/vcs）**：G-191（subagent/mcp-resource 改走 PipelineExecutor：before/after hook、decodeInput、normalizeContent 全吃；纯文本 MCP 资源 content 修复）、G-192（websearch disable 删工具 hook（context/compaction/generate 三挂点）+ 两处表单 `kind:"websearch.provider"` 元数据）、G-198（clone `--depth 100`+覆盖缝；snapshot diff 二进制检测+10MiB 预算截断；workspace diff numstat `"-"`→空 patch）、G-203（实测发现真缺口：原生 find 索引曾收录隐藏项且不认 .gitignore——已对齐 rg 语义修复；「裸 .gitignore 无仓库」残余分歧注释钉定）。
- **lane7 `58be5914`（tui 消息渲染）**：G-182（patch 每文件一卡）、G-183（question 专用臂：`# Questions` 问答对 / `Asking questions…`）、G-184（内联 shell 游标增量拉取+1s 轮询+1MiB 显示上限+`[earlier output omitted]`）、G-187（工具图片接入既有 image_strip_row 管线）、G-188（partitionPending：权限等待工具移出折叠组沉底）。
- **lane8 `f530709e`（tui mermaid/theme）**：G-176（汇流排分组键改 diagram.edges from/to，fan_in/fan_out 三臂复活）、G-178（`draw_routed_edge` arrowhead 参数，开放边画线尾，合并优先级按 ref）、G-179（context width 接入 seam + lastGood/截断重试恢复）、G-210（generateSystem 调色板→主题派生 + boot OSC 4/10/11 探测接线（theme=system 时生效），headless 回退不变）。
- **lane9 `6e54a8dc`（tui 交互）**：G-181①④（ctrl+t 变体循环+`save_variant` 持久化+boot 反映；重连时 client 实例替换（ReconnectFactory）+hydrate 重放）、G-185（prompt 历史 PromptInfo 化：附件 chips 随历史恢复、上限 100→50、boot 回灌 composer）、G-186（slash 候选合并服务端 command.list，内置优先、失联回退）、G-189（details 行/locked 冻结/Tab footer 焦点循环）、G-200①（session.viewed POST+指数退避（250ms·2^n 钳 5s））。
- **lane10a `876c2fb5`（oc-client）**：G-197（**生成器源头修复**：`tools/openapi-gen` 增中间件错误 overlay——ref 118 个 404 声明中 45 个来自 `locationMiddleware` 错误联合（effect OpenAPI 导出会剥离而 ref client 生成器保留），按 operationId 对齐别名族；实测缺 45 非 ~49。全部 142 方法增 `x_with_options(&RequestOptions{signal,headers,on_activity})` 尾参（原 `x()` 签名零破坏保留），transport 层接入（bias 取消、逐请求头、SSE on_activity）；oc-tui viewed sink 消费 signal 取消）。
- **lane10b `1424dc6b`（codemode/cli/modal）**：G-180（console.* 捕获进执行结果 Logs + `Warnings:`/`Logs:` 段；guest fetch 改为 ref web.ts 包装——见下勘误）、G-208（cli-upgrade `flock(2)` 15min TTL，stale 接管，held-fresh 拒绝）、G-209（`modal_sync.rs`：Modal API 动态模型发现，price/limit/variants 派生数学逐字段对齐，凭据门控惰性（copilot_sync 同款 offline seed 测试模式））。
- **lane11 `fb5236b6`（补漏：内建行为组）**：G-193 五子项（plan agent 种子+enter/leave 提醒注入（`base_transcript` 缝）；`# Your Model` identity 段；GPT textVerbosity 自动 low（ref 实为 `plugin/verbosity.ts` 非 packages/ai——谓词 openai_model_id+supports_verbosity 全量移植）；六套 per-provider system-prompt 优化模板（gpt-astra/anthropic/kimi/trinity/meta，逐字节 md5 校验搬运）；init/review 内建命令（ref 模板 verbatim）。有界偏离：plan 提醒仅请求组装期注入（无合成消息落库/事件订阅）；`ResolvedModel` 增 `info` 字段）。

## 登记与勘误（第十四轮）

- **G-200②（terminalHandoff）登记为结构性有界分歧**（体例同 G-163）：ref 的 preflight→terminalHandoff 仅服务「版本不匹配后台服务替换期，把 UpdatePreflight 的 split-footer opentui 渲染器交给 TUI」——rust CLI 无 opentui 运行时、无跨进程渲染器对象可移交；`Mismatch::Replace` 在终端获取前同步解决，无缝隙窗口可填充；ref 的 off-TTY 降级文案已含于 G-152 落地（misc.rs:31-36）。
- **2026-11-10 勘注（第二十轮条件核查）**：理由在 tui-full-convergence 后部分失效但结论维持——CLI↔TUI 现**同进程**（misc.rs:80 `oc_tui::app::run(client, boot)`），移交在架构上变得可行；但 CLI 侧仍无渲染面（misc.rs:31-45 preflight=stderr）、替换窗口内无渲染器存在，ref handoff 所解决的问题（CLI 已建 split-footer 渲染器免拆建移交）在此不出现。维持文档化决策；如未来 CLI preflight 获得渲染面再重评。
- **勘误（G-180② 前提）**：第十二轮登记称「ref codemode 无任何 fetch 面（grep 零命中）」**不实**——ref 经 `packages/core/src/codemode/web.ts`（`Extension.make({name:"web", globals:{fetch}})`，30s `AbortSignal.timeout` + 净化响应对象）暴露包装 fetch。rust 侧按 ref 包装语义对齐（非移除），裸 Bun fetch 不再可达。
- **勘误（G-197 计数）**：第十三轮「缺 ~49 个 404 声明」实测为 **45**（别名族/命名迁移噪声所致高估）；另查明根因：ref 的 404 声明有 43 个来自 `locationMiddleware` 错误联合而非 OpenAPI spec 本身——生成器侧以 overlay 表达（`MIDDLEWARE_DECLARED_ERRORS`），pin 的 openapi.json 未动（sha 守卫不动）。
- **勘误（G-193 ref 锚点）**：verbosity 注入点在 `packages/core/src/plugin/verbosity.ts`（非 audit 所称 packages/ai）；command 模板在 `packages/core/src/plugin/command{.ts,/initialize.txt,/review.txt}`。
- **flake 白名单勘注（2.2#4）**：`e1_cells_paste_8k` 在本机**并发满套件门**常双败（多棵无关树同样发生），序列化（`--profile pty`）通过；2026-11-07 盘上提交级二分（base PASS→f80abaa8 FAIL→b49cfeda FAIL→21d8a019 PASS→HEAD PASS）证明其对构建布局/机器负载历史**非单调**敏感——阈值型 canary 而非代码回归信号。合并门按白名单放行，以落盘的序列化单跑为准。`be1_session_shell_executes_and_notifies_instead_of_prompting_the_model`（oc-server be1_lane）本两轮多次 try-1 超时重试即过，观察名单（暂不入白名单，复发再议）。
- **流程修正（本轮事故教训，并入此后派发模板）**：①一切门命令（含过滤/单测迭代）必须全量 tee 落盘（2.1#3 原有规定；此前提示词仅收尾门带 tee，单测证据丢失导致反复重跑与归因事故——lane8 的 e1 争议即由此放大）；②子代理一律前台分块跑门（shell 调用本身阻塞至完成），「挂后台等门」为违规（本轮两次出现、两次纠正）；③子代理提交身份违规（`-c user.name`）须当场 `--reset-author` 修正并复查双身份（本轮一起，已修正核实）。

## 第十四轮判定

G-165..G-210 全部 ✅（G-177 前轮已闭；G-211/212 维持 ℹ️ 登记）。现存未关闭项维持既有文档化决策：G-12/G-16/G-17/G-28/G-32/G-38/G-39/G-40/G-49/G-161①/G-163/G-164/G-200②/G-211/G-212。

---

# 第十五轮（2026-11-07，15 通道全功能逐项对比审查 + 4 通道缺口复核）

> 方法：15 条并行审查通道对 ref v2.0.24 全功能域（core 会话/工具/AI×2 TUI/配置/总线/DB/shell-PTY/MCP-插件/权限凭证/文件系统/CLI/server/client-schema-theme-util/杂项，TS ~24.7 万行核心链路 vs Rust ~55 万行）逐文件对读（总报告 `PORT_REVIEW_v2.0.24.md`，域报告 `/tmp/opencode/port-review/L01..L15*.md`）；随后 4 条复核通道（V1 config/skill/工具、V2 PTY/文件系统、V3 CLI/server/凭证、V4 TUI/插件/AI，记录 `/tmp/opencode/port-review/V1..V4-verify.md`）对全部 ❌ 指控与重点 🟡 逐项**双侧现行源码**取证并与本册 G-1..G-212 去重。结论：新立项 G-213..G-240（28 项）、G-116 状态回退重开、两条旧勘误订正、6 项初报撤销、4 项归类既有。总体画像：一致率 ~83%（✅1182/🟡196/❌38/➕77），新缺口集中在**动态化触发面、wire 细节、小 UI 面**。

## 🔴 高

### G-213 ✅（🔴 安全）`serve --service` 模式密码打到 stdout（cli）
- rust `oc-cli/src/handlers/serve.rs:214-218`：非 stdio 的 default 与 service **两种模式**只要 `environment_password.is_none()` 即 `println!("server password {password}")`；service 模式密码已持久化 service config（:178-180）却仍打 stdout——daemon stdout 被 journal/Desktop 捕获即落日志。
- ref `cli/src/services/server-process.ts:62,175`：仅 foreground（default）打印；service 模式密码只进 ServiceConfig（:136）。
- 修法：打印条件加 `!service`，对齐 ref。

## 🟡 中

### G-214 ✅ 命令模板 `` !`cmd` `` shell 插值缺失（core）
- rust `config/command_md.rs:242-287` `evaluate_template` 仅 `$N`/`$ARGUMENTS`/append-input；注释（:236-240）自称 "server-side execution surface 另行应用 shell blocks"，但两个执行点 `oc-server/handlers/session.rs:1190`、`plugins.rs:1677-1683` 均直接消费其结果，全仓无后置插值——注释为虚假承诺。
- ref `config/plugin/command.ts:214-239,250`：shellRegex `/!`([^`]+)`/g`，经 ShellSelect+AppProcess 以 cwd=location 执行、合并 stdout/stderr、失败报 `Shell interpolation failed`。
- 影响：模板中的 `` !`cmd` `` 字面文本进入 prompt。修法：两个 Config 执行点补 shell-interpolation 腿，或并入 evaluate_template 并传 shell select seam。

### G-215 ✅ MCP prompts 未注册为 `server:prompt` 斜杠命令（core）
- rust `mcp/registry.rs:1304-1316` `prompts()` 与 `:838/:1047` 的 `mcp.prompts.changed` **全仓零消费**；`engine/commands.rs` 三源（markdown/document/plugin）无 MCP prompt 注册；G-186 的 TUI slash 合并走服务端 command.list 同样不含。
- ref `plugin/command.ts:17-27,55-81`：`mcpCommandName`=sanitize(server)+":"+sanitize(prompt)，PromptsChanged 订阅驱动 reload，`mcp.prompt` 结果拼 prompt 文本。
- 影响：MCP prompts 在 rust 完全不可作为斜杠命令触达。

### G-216 ✅ skill 目录扫描三偏差（core）
- ① `**/SKILL.md` 深度：rust `engine/skills.rs:413-420` 只查一层子目录（注释自认 minimal）；ref `fs.scan("{*.md,**/SKILL.md}")` 任意深度——`skill/a/b/SKILL.md` 静默丢失。
- ② 大小写：rust `skills.rs:406` `eq_ignore_ascii_case("md")` 接受 `.MD`；ref glob 在 Linux 区分大小写（rust 多收文件）。
- ③ 错型 name：rust `skills.rs:432,452` 非字符串 name 回退为 id 仍注册；ref `skill-file.ts:42-43` decode 失败 → Skipped。
- 修法：walkdir 全深度匹配 SKILL.md、后缀精确比较、错型 name 跳过。（G-53/G-80 为别的 skill 面。）

### G-217 ✅ EADDRINUSE 竞争重试环为死代码（cli/server）
- rust `lifecycle.rs:1487-1522` `bind_or_incumbent` 确有 100ms×15s 重试环+逐字文案，但**生产零调用**（全仓 grep 仅测试引用）；生产链 `serve.rs:103-127` 仅启动前一次性 incumbent 预检 → `boot.rs:573` → `lifecycle::serve:1571` `bind(options)?` 一次绑定冲突即 Err。
- ref `cli/src/server-process.ts:144-171`：launch 失败且 EADDRINUSE 时进入 15s 重试环（逐 probe 重试 bind，老 daemon 退出即可接管）。
- 修法：`serve()` 改调 `bind_or_incumbent`（或 boot 层换入口）。〔本条即第十轮勘误 #13 的失真订正，见上文。〕

### G-218 ✅ 双 location-watcher 并存、HEAD 变更双份事件（core/server；G-164 前提过时）
- rust 两套都活着：① `oc-core/filesystem/location_watcher.rs`（notify 订阅）+ `engine/mod.rs:1725-1737` 装配 spawn（`file_watcher` 默认 true），发 `filesystem.changed`；② `oc-server/announce.rs:107` 250ms poller 独立对 HEAD 做 mtime 指纹（:511），既发 `filesystem.changed`（:559 自认）又发 `vcs.branch.updated`（:532）。
- ref 单套：notify watcher 发 `filesystem.changed`；`vcs.ts:158-165` **订阅** HEAD changed → refresh → `vcs.branch.updated`。
- 影响：HEAD 变更同文件双份 `filesystem.changed`（客户端双刷新）；branch 更新仅轮询路径（≥250ms 延迟，ref 事件驱动）。G-164 登记的"announce 替代 core 自持 notify"前提在 watcher 落地后已失效。修法：announce 的 head-watch 改订阅 engine watcher 的 filesystem.changed，去掉重复发布。

### G-219 ✅ snapshot 对象库不共享：无 alternates/无增量 capture（core；并注 G-30）
- rust `snapshot.rs:277-365` capture_blocking 每次全量遍历工作树并 `fs::read` 每文件再写独立 odb（模块头 :8-12 自述 bare odb；git2 odb 仅同内容去重，I/O 恒 O(worktree)）。
- ref `git.ts:345-382` repo.create 带 seed 时写 `objects/info/alternates` 指向源库 + 拷贝 index；`snapshot.ts:95-105` 消费链 → capture 走 `git add --all`（:425-436）——已跟踪未变文件不重读不重写。
- 影响：大仓每次 capture 全量读盘+全量哈希，性能显著劣化；存储重复 blob。修法：libgit2 odb alternates 或源库 OID 直引 + stat/mtime 索引缓存。〔与 G-30 同文件同族，作为其追加注记立项。〕

### G-220 ✅ `POST /api/fs/write` 缺 path → 500（server）
- rust `oc-server/handlers/fs.rs:869` `query_field("path").unwrap_or_default()` → relative="" → absolute=location 根 → 写目录失败 → :874 `unknown_ref()`（500）。
- ref `protocol/src/groups/fs.ts` WriteQuery `path: Schema.String` 必填 → schema ParseError 400。
- 修法：缺 path 直接 `invalid_request`（400，文案按 ref ParseError 格式）。

### G-221 ✅ mini stream-v2 五块展示行为缺失（tui/mini）
主链路（text/reasoning/tool/step/permission/form/subagent/reconnect/G-134 快照/G-68 fragment）确在；对照 ref `packages/tui/src/mini/stream-v2.transport.ts`，rust `oc-tui/src/mini/transport.rs` fold 缺失：
1. 服务端 inbox pending prompts（ref :196-208,620+ → footer `queued.prompts`，消费 enqueued/delivery.changed/cancelled；rust 仅 `"session.inbox.delivered" => Ignored`（transport.rs:166）——跨客户端排队提示不显示）；
2. 内联 shell 流式展示（ref :339-374,807-816,1133-1158 shellCommit/shellTerminal/截断标记/exit code 错误行；rust mini 全目录 grep "shell" 零命中；主 TUI 的 G-184 是另一面）；
3. skill 激活展示（ref :399-415 skillCommits + skill.activated/updated；rust 无命中）；
4. 服务端驱动 compaction 展示（ref :416-445 compactionCommit/Summary/Error；rust 仅本地 /compact 手工 note（runtime.rs:725），服务器自动压缩事件被 fold 忽略）；
5. `session.tool.input.ended`（ref 终结流式工具输入行；rust 只 fold started/delta）。
（①②④ 用户体验影响最大。）

### G-222 ✅ 插件热重载触发桥断裂：直接编辑插件文件不重载（plugin）
- rust 能力在（`reload_now` digest-scan，oc-plugin-host lib.rs:603；增删路径 plugins.rs:483-560），但唯一触发桥是 config 变更：`boot.rs:92-145` ConfigReloadBridge 经 `do_reload`（config.rs:676-687）——**配置 entries deep-equal 则返回 false 不触发**。直接编辑 `.opencode/plugin/*.ts` 虽触发 watch reload，但配置未变 → `on_config_updated` 不发 → 插件不重载；24h 定时与 bus 路也无。测试仅 hot_reload.rs 手动调 `reload_now`。
- ref `plugin/supervisor.ts:196-237`：sources.changes()（插件目录 fs watch）/modules.changes()/24h 定时/bus 事件四路汇流 + 100ms debounce。
- 修法：reload 谓词并入插件 digest 变化，或为插件源单独挂 notify 触发 `request_reload` 短路 deep-equal。

### G-223 ✅ MCP 过期/401 判定靠错误文本匹配且无 sessionId 门（core）
- rust `mcp/client.rs:273-334`：`error_chain` 字符串 + 解析 `"(HTTP status NNN)"` 文本；`session_expired`（:329）与 `unauthorized`（:320）无 sessionId 在场门——**任何 remote 404 都判过期重连**；reqwest/rmcp 改错误文案即碎。测试 mcp_oauth.rs:347 未钉分类器文案。
- ref `mcp/client.ts:55-58,230-243,383`：`SdkHttpError.status` 类型判定；过期判定**要求 `session.transport?.sessionId !== undefined`**（无 session id 连接的 404 不误判）；UnauthorizedError instanceof。
- 修法：向下转型取 `StatusCode`（reqwest `Error::status()`）+ 补 sessionId 门 + 分类器单测。

### G-224 ✅ websearch 三残留（core）
- ① 429 冷却不读 `Retry-After`：rust `tool/websearch.rs:21` `COOLDOWN_MS=60_000` 固定（:275 应用）；ref `websearch.ts:204,215-222` 解析秒数/HTTP 日期（非法回退 60s）。修法：ProviderError 携带响应 headers + 移植 cooldownMillis。
- ② preferred 亲和表不清理：rust `websearch.rs:56,241/245` 只 insert（:183 仅清 cooldowns），无 session.deleted/moved 订阅 → 无界增长；ref `:92-97` 订阅删除 + finalizer clear。
- ③ tavily 缺 `User-Agent` 头：rust `websearch_providers.rs:283` 仅 X-Client-Name；ref `tavily.ts:53-54` 另发 App.useragent。一行修复。

### G-225 ✅ 凭证值解码失败静默降级为空 key（core）
- rust `integration/credential.rs:371-379` `decode_value` 失败 → 回退 `KeyValue{key:""}`，被 :68/:99/:129 三个读路径采用——损坏凭据静默变"无 key"继续发请求。
- ref `credential.ts:67` `decode(row.value)` 抛错 → 读路径失败，不产出假凭据。（G-114 修的是 permission saved-grant，不同处。）
- 修法：decode 失败跳过该行并 log error（至少不得降级为可用的空 key）。

### G-226 ✅ 失败回合合成 assistant error part（bounded 偏离，需裁决登记）
- rust `session/execution.rs:850-891`：Terminal::Failed 且 `!surfaced`（pre-step 失败）时 `publish_failed_turn_error`（:902-942）在**新 assistant message id** 上合成 `session.step.started`+`session.step.failed` → message updater 落 assistant 行，随后才发 `session.execution.failed`。注释（:866-883）自陈有意模仿"v2 投影"。
- ref `session/execution.ts:134-143` 终态失败**只** publish Execution.Failed；`message-updater.ts:144` 仅置 idle，**不**合成 assistant 行。
- 影响：rust 持久化一条 ref 不存在的 assistant 消息行——transcript/message.get/导出与 ref 不可互换；重复失败累积合成行。（G-79/G-105/G-194 均不覆盖。）
- 裁决建议：登记为有意增强（bounded）并注明 wire/存储差异 + 合成行去重；或按严格对齐改为仅发 execution.failed。二者择一，现状（既非 ref 行为又无登记）不可留。
- **2026-11-08 第十六轮裁决落地（方案 A）**：合成行保留为登记的有界增强（publish_failed_turn_error 注释块注明 ref 行为（execution.ts:140-146 仅发 Execution.Failed、message-updater.ts:145 仅置 idle）与 wire/存储差异）；去重按「最后一条 user 消息」turn 键——同 turn 重复失败复用既有 error 行就地更新（store 派生，跨进程重启亦去重），execution.failed 不被抑制（gb9 lane8 `fa4730d6`）。

### G-227 ✅ `app.toggle.animations/diffwrap/file_context` 三命令无 Dispatch 臂（tui）
- rust：keymap 表已注册三键位（`oc-tui-core/keymap/table.rs:531-533`，`none` 绑定）、dialog_config.rs:104/264/324 有设置行；但 dispatcher 仅 `app.toggle.paste_summary` 臂（command_palette.rs:3131-3135）——用户绑定键位后无效果。
- ref `app.tsx:1148-1191` + `config/keybind.ts:53-55`：三命令 run 均 `config.update` 翻转。
- 修法：照 G-137 模式补三臂 + palette 条目（标题随当前值翻转）。（G-137 修的是 thinking/scrollbar，同型不同项。）

### G-228 ✅ CLI auth logout/switch 连接失败打印空错误（cli）
- rust `oc-cli/handlers/auth.rs:775` `connect(server,None).await.map_err(|_| String::new())?`（仅此一处吞错；同文件 :192/:280/:872/:946 均保留真实错误）；logout（:832-837）/switch（:852-858）`eprintln!("{message}")` → 空行 + exit 1。
- ref `ui/prompt.ts:23-42` handlePromptErrors 打印真实错误；logout.ts:17/switch.ts:17 接线。
- 修法：`.map_err(|e| error_message(&e.to_string()))` 复用 :842 打印路径。

## 🟢 低

### G-229 ✅ 持久 PTY 两差异：title/fg 事件链缺失 + 退出无条件 removed（core/oc-pty）
- ① `wire.rs:375-447` WireResponse 无 TitleChanged/ForegroundProcessChanged 变体；`persistent_pty.rs:873` `foreground_process: None` 恒空（无 /proc 探测、无 OSC title 捕获）；ref wire 定义（daemon.ts:97-98,116-117）+ 外部 opencode-pty 二进制生产事件。影响 v2 协议保真（第三方客户端订阅收不到）；rust TUI 标签恒退化为 title（store.rs:994-997 退链同构但数据源恒 None）。
- ② `persistent_pty.rs:927-936` exit-pump 子进程退出**无条件**发布 `persistent-pty.removed`（零订阅者/从未 attach 也发；:2201-2205 测试钉定为 §3.1 自declare 语义）；ref `index.ts:311-324` `removeVisibleExit` 仅对已 attach 可见终端触发，未 attach 的保留 exited 条目。修法：仅在有/曾有活跃 attachment 时发 removed，其余 mark_exited（需同步改钉定测试）。

### G-230 ✅ 工具输出 schema 编码校验（encodeOutput）缺失（core）
- ref `tool/runtime.ts:45-54,109-132`：对声明 output schema 的工具校验输出 + 两个边界（未声明却返回 output → die；声明却缺 output → error）。
- rust `tool/registry.rs:297-300` output 原样透传零校验。缓解：生产面 MCP outputSchema 仅 wire 转发（mcp/registry.rs:1535）、插件工具无 output 字段，触发面极窄。低优先补齐或文档化。（归一化半边已撤销，见勘误 #2。）
- **2026-11-08 第十六轮落地（范围裁定）**：声明 schema → 共享 G-100 校验器校验（ref 文案）；声明+缺 output → 错误。「未声明却返回 output → die」按 mcp.ts:48 `?? {}` 接受一切的范围裁定落地——rust 14 个内建工具返回 output 而无声明（ref 侧全部声明 Output），字面 die 会全断；die 分支登记为范围外，待内建声明补齐后启用（gb9 lane8 `2103cf92`）。
- **2026-11-12 第二十四轮勘注（census 机械化确认）**：TOOLS census 复核 die 分支前置面——rust 内建 output 声明 **0/18**（registry.rs:185 `output_schema: None` 硬编码；ref 14 文件 18/18 全声明），范围较本条登记时的 0/14 扩大（mcp-resource 内联两工具计入）；die 分支 NOT_YET 维持，由 `tools/census/tools_registry.py` 盯守。
- **2026-11-12 第二十五轮收口 ✅（gb13-laneC `80995995`/`58174fd4`/`7be6317b`）**：die 前置+die 分支全量落地——①18/18 内建声明 ref-exact Output（闭对象 json! 字面，逐工具 `*_output_schema()`；BuiltinTool 增 `output` 字段，register 经 `pipeline_executor_with_output` 线入，`Value::Null`=未声明 scope）；②die 分支启用（未声明却返回 output → 失败，ref runtime.ts:46 原文；Effect.die 缺陷语义以 ToolRunFailure::Error 承载=有界偏离）；③顺修 shell 输出 `shell_id`→`shellID` serde 改名（wire bug vs shell.ts:65）+ 前台 shell metadata 不带 shellID（ref shell.ts:226-234/result.ts:26-34 对齐）；④codemode execute 全量 parity（叶内构建 ExecuteOutput `{output,toolCalls,error?,files}` + 外层 content 携 file parts（tool.ts:126-133））；⑤`outputSchema` 经 ToolDefinition 上模型面定义（runtime.ts:24-25 对位；协议 lowering 与 ref 同样只投 inputSchema）；⑥共享校验器补 `const` 关键字（严格深等、无类型强转——评审发现的空转验证缺口）。census 盯守翻绿：rust 18/18 声明、die 在位（提取器随新形状更新）。

### G-231 ✅ repository-cache fetch 缺 `--all --prune`/`+refspec`（core）
- rust `reference/repository_cache.rs:272-281` 仅 `git fetch origin`（+指定 branch 时再 fetch，无 `+`、不更新 remote-tracking、无 prune）。
- ref `git.ts:269-284`：`fetch --all --prune` + `+refs/heads/x:refs/remotes/origin/x`。
- 影响：多 remote 的 reference 项目刷新不到；上游删分支后残留；非快进更新被拒。修法：换 `--all --prune` + `+` refspec。〔即第五轮勘误 #5 的订正，见上文。〕

### G-232 ✅ canonical 主 worktree 用 commondir 父目录推断（core）
- rust `project.rs:1296-1301` `common_directory().parent()`；ref `project.ts:350-356` `git worktree list` 取 main 项。标准布局等价；偏差场景：bare 仓库配 main worktree/非标准 gitdir 布局（bare 在 rust 侧 `repo.workdir()?` 直接 None，偏差面收窄）。G-77（directory/canonical 字段拆分）不覆盖。修法：`git worktree list --porcelain` 取 main。

### G-233 ✅ session.move 三条 400 文案缺 `: <dir>` 后缀（server）
- rust `handlers/session.rs:911-918`：三条错误文案均无目录后缀；另 :909 缺 directory 也走同文案（ref 是 schema 400 ParseError）。
- ref `server/src/handlers/session.ts:295-301`：`` `Directory does not exist: ${error.directory}` `` 等三条带后缀。（G-107/G-108 只覆盖 subpath/delivery/orphan 快速路径。）

### G-234 ✅ OAuth/Copilot wire 两小项（core）
- ① Copilot device/token/refresh 用 form 编码且无 Accept/User-Agent 头（`flows.rs:375-388 post_form_json`、providers.rs:407-409/:218-222）；ref `github-copilot.ts:66-69,376-385` JSON body + headers()（Accept/Content-Type/UA）。GitHub 两种编码均接受、功能风险低；建议补 `Accept: application/json` + UA。
- ② 非 MCP 集成的 loopback 回调页未复用品牌页：`mcp/oauth_page.rs`（297 行忠实移植 success/error）只接给 MCP OAuth（mcp/oauth.rs:836-841）；`integration/flows.rs:204/225` 仍用自写极简 HTML 且无 error 页；ref `oauth/page.ts` 供所有 loopback 用。修法：flows.rs 改调共享 oauth_page 并补 error 分支。

### G-235 ✅ 自写 PNG 解码器不支持 Adam7/子字节位深（tui）
- rust `image_codec.rs:6-10,100,126-128`：interlace 或 bit_depth<8 → None，调用方保 "No preview" 占位（dialog.rs:1252、message/rows.rs:481、diff_viewer_solid.rs:607；测试 :359-368 钉定拒绝）。
- ref 经 opentui 原生管线全支持。注意 rust Kitty 车道走 `NativeImage::decode`（image_view.rs:77）不受影响，仅 sixel/blocks 车道降级。登记为文档化有界分歧（G-40 族）或补 Adam7 七 pass 解包（约百行）。
- **2026-11-08 第十六轮落地（选择实现而非登记）**：Adam7 七遍解交织 + 1/2/4 位深（MSB 位拆包、灰度复制缩放到 8 位、pass 内独立反滤波 bpp=ceil(bits/8)、pass 维度按 PNG §8.2 ceil 公式）已实现；非交错 8/16 位路径字节级不变；拒绝钉定测试翻转为接受钉定；sixel/blocks 车道恢复预览（gb9 lane11 `2182cfd9`）。

### G-236 ✅ DialogOpen 打开期间 session.moved 无增量更新（tui）
- rust：oc-tui 全仓 grep `session.moved` 零命中；dialog_open.rs 打开时一次性拉取。
- ref `app.tsx:1276-1280`（moveOpenSession 原地更新）+ `dialog-open.tsx:60-80,487-498`（后台 refetch 以 changed map 合并）。影响：对话框开着时 worktree 移动后分组/子路径陈旧，重开自愈。极小。

### G-237 ✅ 通用 DialogPrompt / `ctx.ui.prompt` 缺失（tui/plugin）
- rust `oc-plugin-host/ui.rs`（244 行）无 prompt（host.js:1629-1638 仅 ui.alert/ui.confirm）；DialogKind 无通用 Prompt 变体（dialog.rs:82-87 的 prompt 族均为专用）。
- ref `ui/dialog-prompt.tsx` + `plugin/api.tsx:367-386` ctx.ui.prompt（:391 另有 select）。属 G-64/G-24 插件 API 面族残留子项，未单独登记。

## ℹ️ 登记

### G-238 ℹ️ heap 快照（SIGUSR1）/CPU profile（SIGPROF）不移植（cli；V8 平台性）
- ref `cli/src/heap.ts`（v8.writeHeapSnapshot）、`cpu-profile.ts`（inspector Profiler），index.ts:82-83 生产接线；rust 全仓 grep 零命中，V8 inspector 依赖结构性不可移植。native 侧可选替代路径：pprof/tokio-console。登记为平台性范围裁剪（G-17 族体例）。

### G-239 ✅ 插件 panel/PanelHost 双形态豁免正式登记（tui/plugin；G-28 族）——已解除豁免并激活
- ref `packages/plugin/src/tui/context.ts:165-177,501-507`（ctx.ui.panel.open/close/current + toggleFullscreen）+ `tui/src/context/panel.tsx:12-39`（width>80 双形态）+ panel-host.tsx。
- rust `oc-plugin-host/ui.rs` 无 panel API；`oc-tui/components/plugin_slots.rs:43-51` 注释明言 session.panel "deliberately UNMOUNTED documented exemption"（注引 "G-04"，非本册条目）。本条将该豁免裁决正式入册（G-28 生态 breaking 声明并列）；若后续需要再补 panel 形态。
- 2026-11-08 勘注：plugin_slots.rs 豁免注引已订正（G-04→G-239，gb9 lane10）。
- 2026-11-10 勘注：激活规格（基建评估 + 最小实现路径 ~300-450 行 + 触发条件）已立目 **G-331**；2026-11-10 X-B 核查确认现状不变（仍 unmounted、无消费方）。
- **2026-11-12 第二十五轮增补收口 ✅（gb13-laneI `34550de6`/`b6d66a5f`）**：用户下令激活——豁免解除，`session.panel` 入 MOUNTED_SLOT_PATHS（豁免注释收窄至仅 session.composer.top），panel 形态全量落地（详见 G-331 增补注）。

### G-240 ℹ️ opencode-pty 二进制免 sha256 钉扎（oc-core；RW-T2 已自declare）
- ref `persistent-pty/binary.bun.ts:17-85`：嵌入式资产安装全程 sha256 校验+缓存复验、uid 属主校验、符号链接拒绝（lstat）、0700、原子 rename。
- rust `persistent_pty/transport.rs:132-195`：仅存在性+常规文件+可执行位（:170-176 注释明示 RW-T2 有意取舍）；且 rust 不捆绑/安装二进制（无 install 路径），可比面仅剩 override 校验。登记为文档化取舍；最小加固：给 `OPENCODE_PTY_BIN` override 加属主/符号链接拒绝。
- 2026-11-08 勘注：override 最小加固已落地——lstat 拒符号链接 + same-uid 属主校验（binary.bun.ts:71-75 语义，gb9 lane5 `18cfced2`）；sha256 钉扎维持登记范围外（rust 无捆绑安装路径）。

## 状态回退与登记册订正（本轮）

- **G-116 ✅→❌ 回退重开**：三子项（①cursor 字节计数/②回调 panic 无隔离/③win32 LC_*）经现行代码取证均未落地，第十一轮 ✅ 标记失真（详见 G-116 条目内勘注）；win32 gitbash 发现并入 ③ 同族。
- **第五轮勘误 #5 订正**：repository-cache「完整移植无行为缺失」断言推翻（fetch 缺 `--all --prune`/`+refspec`，→G-231）。
- **第十轮勘误 #13 订正**：EADDRINUSE「全链路接线」断言推翻（重试环生产零调用，→G-217）；handoff 半边属实维持。
- **登记册健康提示**：G-116 的误闭提示第十一轮 burndown 的闭合核验存在盲区（lane 清单与 git log 均查无修复提交），建议专项复核第十一轮 lane1-9 清单中相邻小项（G-117 等）的落地证据。
- **2026-11-08 健康提示执行结果（第十六轮 lane12）**：抽 15 项（G-107/112/113/117/129/131/133/138/142/146/149/154/157/159/160）逐项现行源码核验——14 项 CONFIRMED（含测试证据）、G-117 PARTIAL（time_updated 半边未落地→lane13 双路收口；NULL 过滤半边系 G-225 顺带落地，归因已订正）。第十一轮其余 ✅ 项未见同型误闭信号。

## 归类与收编（不另立项）

- **CLI 进程级 OTEL 初始化缺失** → G-38 补记：ref `cli/src/index.ts:126-133` Observability.layer 于 CLI 进程本体；rust oc-cli grep otel 零命中（仅 oc-server feature 门控）。
- **aisdk V3 适配层/`aisdk:` shim** → 维持 G-17 裁决：facade.rs:95-101 `UnsupportedProtocol` 显式报错（"…has no ported protocol (yet)"），无 family 的 `aisdk:` 包即走该错。
- **session.environment PUT（G-20③）与 websearch generate 挂点（G-192）** → 现状代码确认仍在：app.rs:8744-8772（按 session 单发 PUT）；engine/mod.rs:2602-2621 三挂点（generate 经 `SessionGenerateEvent = SessionContextEvent` 别名覆盖，hooks.rs:413）。
- **skills/agents/commands watch 重建链** → 现状确认在（初报撤销）：config/watch.rs（notify+debounce）→ boot.rs:104-151 ConfigReloadBridge → engine.invalidate 逐 location，与 ref 等位；残留偏差为惰性重建+仅 watch 根内文件。

## 勘误（第十五轮经复核撤销的初报，勿再立项）

1. **wellknown.remove 缺失** — ref 服务方法仅测试消费（无路由/客户端/插件面），rust 暴露面与 ref 完全一致（handlers/integration.rs:59-60），不构成行为缺口。
2. **JSON-Schema 归一化（flattenAllOf/内联 $ref）缺失** — ref runtime.ts:172-186 仅作用于 Effect Schema 内置工具；plain JSON schema（MCP/插件）两侧同样原样透传，校验覆盖由 G-59/G-100 的共享 json_schema.rs 成立（encodeOutput 半边单立 G-230）。
3. **tool.ts Editor 动态面缺失** — rust `ctx.tool.list/get/add/update/remove` 全在（host.js:357-373）；namespace 描述差异=G-106② 已修。
4. **subagent primary 拒绝缺失** — subagent.rs:586 已实现，文案测试钉定（tests_subagent.rs:428-442）。
5. **partial-json NaN/Infinity** — serde_json 结构性不可表示 + tool_stream.rs:72,107 优雅回退空对象，非缺口（G-40 族记录性）。
6. **mini stream-v2「行数缩水」笼统指控** — 拆解后仅 5 个具体行为块缺失（→G-221），主链路确在。

## 第十五轮判定汇总

初报 38 ❌ + 17 项重点 🟡：**确认立项 28**（🔴×1：G-213；🟡×15：G-214..G-228；🟢×9：G-229..G-237；ℹ️×3：G-238..G-240）+ G-116 回退重开；**归类既有 4**（G-38/G-17/G-20③/G-192 + watch 现状确认）；**撤销 6**（见勘误）；**订正旧勘误 2**（第五轮 #5、第十轮 #13）。优先级建议：G-213 > G-214 ≈ G-215 ≈ G-217 ≈ G-220 > G-222 ≈ G-223 > 其余 🟡 > 🟢 族；G-226/G-239/G-240 需文档化裁决先行。

---

# 第十六轮 burndown 收口（2026-11-07~08，gap-burndown-9）

> 第十五轮立项的 G-213..G-240 全部可行动项（G-226 按「需要裁决先行」项以方案 A 落地）+ G-116 回退重开三子项 + lane12 抽查重开的 G-117 残半边，经 lanes 1-13 串行落地并 fast-forward 合并 main。每 lane 独立 worktree（`.worktrees/gb9-laneN`，合并后即时回收，分支已清）、dev-gate 单入口、nextest 未过滤证据、每 lane workspace 合并门复跑；PTY 族 lane（5/9/10）另跑 `--profile pty` 序列化门。一切门命令全量 tee 落盘 `.test-logs/*-gb9lane*`。**无上游阻挡项**（无 opentui-rust 未移植功能依赖；G-235 Adam7 为自研解码器内实现，未走登记分支）。

## 逐 lane 落地清单

- **lane1 `fea81a7b`（cli/server 快修）**：G-213（service 模式密码不再打 stdout——startup_announcement 纯函数化，仅 foreground 且无环境密码时打印）、G-217（EADDRINUSE 重试环接线：service 预检改走 bind_or_incumbent，端口空闲零延迟、老 daemon 15s 内退出即接管；boot 增 serve_listener 缝）、G-228（logout/switch connect 失败打印真实错误）。
- **lane2 `39900c99`（server wire + 命令/skills）**：G-220（fs/write 缺 path → 生成校验器 ParseError 400 `Missing key\n at ["path"]`）、G-233（session.move 三文案加 `: <dir>` 后缀；缺 directory → payload ParseError 400）、G-214（`` !`cmd` `` shell 插值：evaluate_template_shell 于 $N/$ARGUMENTS 之后执行、cwd=location、stdout+stderr 合并、失败文案对齐 ref；两个消费点均接）、G-215（MCP prompts 注册为 `server:prompt` 斜杠命令：双段 sanitize、`mcp.prompts.changed` bus 订阅驱动重同步、执行臂拼 mcp.prompt 提示文本；live bun fixture 验证 changed→reload 链）、G-216（skill 扫描：任意深度 `**/SKILL.md`（walkdir 不跟符号链接）、后缀精确匹配 `md`、错型 name 跳过（ref Skipped））。
- **lane4 `29994f5b`（integration/mcp/websearch）**：G-223（类型化错误分类：SessionObservingClient 记录 sessionId 协商，错误链下钻 rmcp StreamableHttpError 变体判定——无 session 的 404 不再误判过期；restart-400 分支保留 session 在场门）、G-224（①Retry-After 秒数/HTTP-date 冷却（无效回退 60s，ref cooldownMillis 逐字节）；②session.deleted/moved 亲和表驱逐 + Drop 清理；③tavily User-Agent）、G-225（凭证解码失败 fail-the-read（ref die 语义）+stderr 日志；无 integration_id 行跳过）、G-234（①copilot form 帖补 Accept+UA（编码保持 form，勘注裁决）；②loopback 回调页改共享 oauth_page（success/error/bootstrap 三态，ref OauthCallbackPage.bootstrap 对齐））。
- **lane5 `a7d0caa4`（pty 族，+pty 门）**：G-116①②③（cursor/buffer_cursor 改 UTF-16 码元；订阅回调 catch_unwind 隔离+摘除（on_data/replay/on_end 全臂）；win32 LC_* 注入 + shell::select gitbash 发现（cfg(windows) 门控，纯逻辑函数未门控测试））、G-229①（TitleChanged=OSC 0/2 增量扫描器（BEL/ST、分块容忍、4KiB 界、仅变更后首发）；ForegroundProcessChanged=TIOCGPGRP+/proc 探测 500ms 节拍（seam 选择注释钉定）；wire 变体字段名对齐 daemon.ts；oc-pty/oc-server 转发臂机械补齐）、G-229②（exit-pump 改 removeVisibleExit：仅曾 attach 终端发 removed，其余 mark_exited 保留条目；钉定测试按 gap 授权更新）、G-240（OPENCODE_PTY_BIN lstat 拒符号链接 + same-uid 校验）。
- **lane6 `c713459f`（vcs/snapshot）**：G-231（`fetch --all --prune` + `+refs/heads/b:refs/remotes/origin/b` 强制 refspec）、G-232（canonical 经 `git worktree list --porcelain` main 条目（spawn 失败回退 commondir-parent）；bare main 场景对齐）、G-219（①objects/info/alternates 指向源库 + Odb::add_disk_alternate 功能注册（跨库 blob 可达性测试钉定）；②stat-cache 边车（oid/size/mtime/mode）——二次 capture 零读盘、改动文件恰好一次读、损坏边车降级全量。有界偏离：不拷贝源 git index，边车代替）。
- **lane7 `63eb045e`（watcher/插件热重载）**：G-218（唯一 `filesystem.changed` 发布者=engine watcher 显式盯 `<gitdir>/HEAD`（ref location-watcher.ts:19-101 文件目标同款 + 25ms 同文件合并）；announce 弃自有轮询/发布，改订阅按 HEAD-basename 谓词发布 `vcs.branch.updated`（事件驱动）；live 位置种子防首事件丢失；be1 测试断言恰好 1 条 filesystem.changed + branch.updated 到达）、G-222（插件 reload supervisor：插件文件/目录 fs watch + 24h 定时 + `sdk.plugin.updated` bus 三路触发（config 腿留在 bridge），100ms 滑窗防抖、reload_lock 串行化；config 深相等不再短路插件重载；bus/oc-client 增 SDK_PLUGIN_UPDATED（RE-06 波及已过））。
- **lane8 `2103cf92`（execution/registry）**：G-226（方案 A：合成行保留为登记有界增强 + 按「最后一条 user 消息」turn 键去重（store 派生、跨进程重启亦去重）、execution.failed 不抑制）、G-230（encodeOutput 声明 schema 校验 + 缺 output 边界（ref 文案）；未声明+output 按 mcp.ts:48 `?? {}` 范围裁定（14 内建工具兼容面调查为据，die 分支登记范围外））。
- **lane9 `001b393d`（mini stream-v2，+pty 门）**：G-221 五块（①inbox 排队提示 footer `N pending`（enqueued/delivery.changed/cancelled + hydrate `session.inbox.list` 种子）；②内联 shell 流式（`$ command`/输出行/`[output truncated]`/退出码错误行 + replay）；③skill 激活行；④服务端 compaction（`─── Compaction ───` 分隔 + Summary/Error + replay 重挂）；⑤`session.tool.input.ended` 终结流式输入行（键名回退对齐真 wire）；Fold::Commits 多行变体；PTY 级测试两枚钉定 live+replay）。
- **lane10 `d40228b3`（tui 对话框/插件 UI，+pty 门）**：G-227（三 toggle 臂 + 随值翻转 palette 标题；palette 条目为 ref 超集（ref palette:undefined）有界偏离已注）、G-236（session.moved 进 store fold：打开中的 DialogOpen 行内更新（moveOpenSession 语义 + 一代 resync 的 changed-map 合并）；无事件零 refetch）、G-237（DialogKind::Prompt 端到端：title/description/placeholder/value、esc→null；host.js ctx.ui.prompt + supervisor 白名单 + guest promise 解析（wire `ok:true`→null 解包，alert/confirm 一并受益）；bun guest 测试）、G-239（豁免注引 G-04→G-239）。
- **lane11 `2182cfd9`（PNG 解码器）**：G-235（Adam7 七遍解交织 + 1/2/4 位深（MSB 位拆包、灰度复制缩放、pass 内独立反滤波 bpp=ceil(bits/8)、pass 维度 PNG §8.2 ceil 公式、IHDR interlace>1 拒绝）；非交错 8/16 位经同一单 pass 数学保持字节级不变；拒绝钉定测试翻转为接受钉定 + 9×9 五滤波器全遍轮转冒烟）。
- **lane13 `04e4ce93`（G-117 残半边，lane12 抽查发现）**：oc-core CredentialStore（`2362d051`）与 oc-server HTTP 重复 SQL（`04e4ce93`）双路修复——activate/update 不再写 time_updated（ref credential.ts drizzle `.set` 列限定语义）；插入仍播种；HTTP 路径保留本地 SQL（路由 CredentialStore 会改 create/list wire）。
- **lane12（只读核验通道，无代码）**：第十五轮健康提示的专项抽查——第十一轮 ✅ 项抽 15 项逐项现行源码核验：14 项 CONFIRMED（含测试证据）、G-117 PARTIAL（→lane13 收口；NULL 过滤半边归因订正为 G-225 顺带落地）。

## canary 判别实验（e1_cells_paste_8k；并入 2.2#4 勘注）

- 触发：lane8 合并门该测试连败且冷静箱序列化双败，与 lane5 时点（17:05/21:44 冷静箱序列化 PASS）矛盾——按第十四轮盘上二分方法取证，期间 lane6/7/8 一度疑似真回归，**两 lane 合并被推迟至判明**。
- 二分链（全部落盘 `.test-logs/e1-bisect-*`）：a7d0caa4 PASS（21:44）→ c713459f FAIL → d1bb497a FAIL → d1bb497a+project.rs 回退 FAIL → d1bb497a+repository_cache.rs 回退 FAIL → main FAIL → a7d0caa4 对照复测 PASS（22:22，load 8.9 下仍过）。
- 受控实验（定谳）：a7d0caa4 + 仅向 oc-core/src/lib.rs 追加 2 行注释（零语义改动）重编 → **FAIL**。两个单文件回退组合均败已排除内容因果，受控实验证明判别因素是「oc-core 是否被改动重建」本身。
- 失败形状（日志可复验）：typing-idle 单格恰 1 burst（bursts=1≠0，25 flush、median-gap 120ms、cadence=true），其余六格全 0——二进制布局偏移使某个一次性唤醒落入 2.5s 采样窗的阈值噪声。
- 结论：第十四轮「非单调阈值型 canary 而非代码回归信号」的受控复现；**序列化单跑通过不再是普适可得状态**，放行依据=白名单+本证据链。lane6/7/8 无行为回归，合并放行。

## 有界偏离（第十六轮新增）

- **G-214**：shell 插值经 shell::select 解析（与 shell 工具同缝）；stdout/stderr 连接（交错不可观测）。
- **G-219**：stat-cache 边车代替源 git index 拷贝；alternates 为布局 parity + 跨库可达性注册（不省首次 capture 读盘）。
- **G-221**：五块渲染遵循 mini 既有静态行卡/行模型惯例（G-134/G-68 同款）。
- **G-226**：合成失败行保留（登记的有界增强 + 去重，见条目勘注）。
- **G-227**：palette 条目为 ref 超集（ref palette:undefined）；animations 读设置行默认而非 ref 的 `?? true`。
- **G-230**：「未声明却返回 output → die」范围裁定为 `?? {}` 接受一切（die 分支登记范围外，待内建 Output 声明补齐后启用）。
- **G-116③**：win32 注入/发现 cfg(windows) 门控，本目标不可编译验证（纯逻辑函数例外，未门控测试）。
- **G-232**：spawn 失败回退 commondir-parent（注释钉定）。
- **G-234①**：保持 form 编码仅补头（gap 勘注裁决）。
- **G-237**：host.js 对话 promise 的 wire `ok:true`→null 解包（alert/confirm 一并受益，超集）。

## 第十六轮判定

G-213..G-237 全部 ✅（G-226/G-230 含上述范围裁定）；G-116 ✅（回退重开后三子项收口）；G-117 ✅（残半边双路收口 + 归因勘注）；G-238/G-239 维持 ℹ️ 登记（G-239 注引已订正）；G-240 维持 ℹ️（override 加固已落地，sha 钉扎维持登记范围外）。现存未关闭项均为既有文档化决策族：G-12/G-16/G-17/G-28/G-32/G-38/G-39/G-40/G-49/G-161①/G-163/G-164/G-200②/G-211/G-212/G-238/G-239/G-240。**无上游阻挡项。**

---

# 第十七轮（2026-11-08，独立 19 通道全量复审 + 4 通道缺口复核）

> 方法：19 条独立审查通道对当前树（十六轮收口后）做逐文件逐行对读（总表 `PORT_REVIEW_INDEPENDENT_v2.0.24.md`，域明细 `/tmp/opencode/review-lanes/L01..L19`），随后 V-A（core）/V-B（AI·integration）/V-C（tools·MCP·config）/V-D（server·TUI·CLI）四条复核通道（记录 `/tmp/opencode/review-lanes/R17-V{A,B,C,D}.md`）对全部缺口指控逐项双侧现行源码取证，并与本册 G-1..G-240 及全部勘误去重。结论：**新立项 G-241..G-290（50 项：🔴×7 / 🟡×27 / 🟢×14 / ℹ️×2）**、既有条目勘注 8 条、归类既有 12 项、撤销初报 15 项（见勘误）、订正独立审查报告失实 2 处。新缺口集中在**输出正确性主路径、权限契约、运行时触发面、wire 语义族**。

## 🔴 高

### G-241 ✅ mini 文本流重复输出 + text.ended 尾部丢失
- 四环证据：① `mini/transport.rs:256-277` `text_delta` 的 Commit.text=`update.text`——这是 `FragmentReconciler::delta()` 的**累计全文**（fragment.rs:131-136 `previous+delta`；transport.rs:1498-1501 自带测试断言第二个 delta 的 commit.text=="Hello world"）；② `runtime.rs:652-662` Fold::Commit→`lifecycle.append` 无按 part 替换；③ writer 纯追加（lifecycle.rs:1252-1261→write_scrollback :1327-1364；全 mini 无行回退/同组替换，oc-cli 测试注释自认 "the mono writer is append-only"，mini_pty.rs:805-815）；④ ref 相反：transport.ts:1169-1184 delta commit `text: event.data.delta`（纯增量）、:1185-1204 `text.ended` 补发剩余 slice。铁证：同文件 G-221 compaction 处理器（transport.rs:650-679）自己就正确发增量+ended-suffix——text 臂孤例错用。
- 后果：①每个多 delta 流式回合把前文重复打印 N 次；②`text_ended`（:279-310）Commit 为 Final 且 entry_body 对 Assistant Final 渲染空行（scrollback.rs:255-259）——服务端 final 长于流式累计时尾部永久丢失。测试盲区：全部 mini PTY 测试每 part 仅一个 delta。
- 修法：`text_delta`/`reasoning_delta`（:312-330 同病）改发增量；`text_ended`/`reasoning_ended` 按 ref 补 suffix progress commit；补多 delta PTY 断言（两 delta 只出一次全文）。

### G-242 ✅ MCP server 工具执行期无权限断言且绕过统一管线
- `mcp/registry.rs:1616-1621` McpToolExecutor.permission 仍 `#[allow(dead_code)]`（注释"runs in the C1 pipeline's execute wrapper"是初代移植许诺，从未兑现）；:1812 仍 `register_executor` 裸注册；`tool/registry.rs:214-236,238-330` PipelineExecutor 全程无 permission.assert；`location.rs:607` permission_action() 唯一消费点是 wholly_denied 可见性过滤；call_tool 内亦无门。ref `tool/mcp.ts:49-63` 每次执行前 `permission.assert(action={server}_{tool}, resources/save ["*"])` 且经统一管道吃 hook/decodeInput/normalizeContent。
- 影响：agent ruleset 对 MCP 工具的 **ask 规则完全不拦截不询问**（仅 deny-all 影响目录可见性）——权限契约级缺口。G-191 明文只覆盖 subagent+mcp-resource。
- 修法：照 G-191 模式——McpToolExecutor 改 ToolExecuteWithContext 叶子、execute 首步 assert（action=`tool_name(&server,&tool)`）、经 pipeline 包装注册（顺带声明 `tool.output_schema`，G-230 的 MCP 面同时对齐）。

### G-243 ✅ 非持久 PTY drain/attach 两处多字节 panic（G-116① 之外的独立面）
- ① `oc-core/pty.rs:258-266` 缓冲按 `..excess_bytes` 切片/drain 无 UTF-8 边界回退——多字节输出跨 2MiB 边界即 panic（调用点 :624-628 为 tokio::spawn 任务，panic 后数据流静默死亡、会话卡 running；persistent_pty.rs:1526-1544 已有 0b10xxxxxx 回退，pty.rs 漏做同款；ref pty.ts:218-222 JS slice 永不劈裂）。
- ② `pty.rs:798-803` attach 重放 `from/start` 为 UTF-16 码元（:232/:262-265 推进）却当字节下标切 String——多字节内容错位重放/panic（ref pty.ts:281-286 单位自洽）。
- 修法：drain 复用边界回退；attach 用 char_indices+len_utf16 换算字节偏移。

### G-244 ✅ zen/opencode device flow 元数据链路断裂（自托管 Console 整体失效）
- `integration/providers.rs:175-194` device 兑换仅写 `metadata={orgID, orgName:org_id}`，无 `server/accountID/email`，全无 `/api/user`、`/api/orgs` 调用；refresh（:209-215）与 console 配置拉取（console.rs:234）读 `metadata.server` 缺省即回退 `https://opencode.ai/console`——**自托管 Console 的刷新与配置拉取整体错打官方服务器**；orgName 恒显示 org_id。ref `opencode.ts:592-623`：token 后并发两 GET、写全 5 键、org 按 org_id 匹配排序、找不到报错。
- 附两子项（🟢 级）：请求体 form vs ref JSON（opencode.ts:644-650 schemaBodyJson——G-234① 对 copilot 的「保持 form」裁决不自动适用于自托管服务端）；轮询首询时机（ref 先 sleep 再首询 :567-570）。Console SSO 状态消息组织名丢失（console.rs:294 硬编码 None）同根因并入本条。
- 修法：兑换后并发两 GET、写全 5 键、org 解析对齐；顺带 JSON 编码与首询时机。现有 tests.rs:789-826 手工构造带 server 的 credential 掩盖了缺口——补"device 登录后 refresh 打到 answer.server"端到端断言。

### G-245 ✅ openai/xai/azure WebSocket transport 默认丢失（系统性）
- ref `openai.ts:262`、`xai.ts:102` provider.transform 注入 `transport ?? "websocket"`；`azure.ts:358-365` 对 responses-websocket-capable 条件注入。rust `engine/resolver.rs:596-607` 注释自知（"the provider plugins default openai/xai/azure to websocket; omitted means HTTP"）但只读 `settings.transport`；全仓无默认注入（catalog/source/normalize 零命中）；azure `responses_channel_enabled`（azure.rs:177-219）只认显式 URL。
- 影响：三家默认退化为 HTTP SSE——长连接会话通道/prompt-cache 连续性默认不可用（WS 机器本身完整且逐项对齐 model-transport.ts；G-195 修的是 native 压缩 seam，不同面）。
- 修法：resolver/catalog normalize 对 openai/xai（及 azure responses-capable 绑定）烘焙默认 `"websocket"`，用户显式 settings 胜出。级别可按产品口径降 🟡。

### G-246 ✅ media Asset 下载凭证随序列化泄漏 + 排队类结果过期即废
- ① `media/protocol.rs:278-294` `expiring_url` 把下载 headers 写进可序列化 `provider_metadata["route"]["headers"]`，:618-631 测试**钉定了泄漏形状**；ref `media.ts:207-213` headers 运行时保密、不入 source/toJSON/AssetSchema（序列化前须 materialize）。
- ② `image/bfl.rs:253-304` 返回裸 sample URL（注释自认 route-side download，但 `media/route.rs` 无任何 download/materialize）——BFL 结果 10 分钟过期即废；ref bfl-images.ts:191-192 decode 即 materialize。③Asset `bytes()/materialize()/file()` 整层缺失（ref media.ts:149-197）。④`replicate.rs:249` 过期以 now 起算；ref replicate-images.ts:143-146 以 completed_at 起算。
- 修法：headers 移出可序列化字段（per-request sidecar）+翻转 :618-631 测试+route 层补下载物化+replicate 起点改 completed_at。

### G-247 ✅ CodeMode 沙箱 import/timers 可达与模型面声明矛盾
- `host.js:2108-2112` 自述 "Imports and timers remain reachable…a documented divergence"——但"documented"仅存在于该行代码注释（登记册无此面：G-180=console+fetch 两项）。`tool/codemode.rs:76-81` EXECUTE_DESCRIPTION（与 ref codemode/tool.ts:64 逐字相同）向模型宣称 "Imports, direct filesystem access, and timers are unavailable"——ref 侧为真（受限解释器），rust 侧为假：setTimeout/setInterval 完全可用（host.js:231-238）、动态 `import()` 可达（:2-3 自认 native ESM+CJS）→ `import("node:fs")`/`import("node:child_process")` 越出边界，**prompt 注入的 MCP 内容可借 Code Mode 触达文件系统/进程**。
- 修法（择一）：①静态扫描程序体拒绝 `import(`/`require` token（最接近 ref 契约）；②描述如实声明（parity 破坏，需裁决）；③结构隔离（无 import 能力的域）。至少先登记+描述与实际二选一对齐。

- 2026-11-09 第十八轮勘注：方案①（静态扫描）落地（leaf+guest 双侧 token 级扫描 + timers 抛错占位）；`Bun.file/Bun.spawn/process/Function/eval` 仍在 guest 可达——静态扫描的固有边界，结构性收口登记为后续项（lane3 report）。
- **2026-11-12 第二十五轮上游阻挡标注（搁置）**：round-23 G-334 已消除 `__rawFetch`/eval/Function/AsyncFunction 面（Bun/process 求值作用域影子参数）；残余面=**全局 `Bun`/`globalThis.process` 直达**——Bun 运行时将自身全局钉为 non-writable，替换 `globalThis.process` 会断 Bun stdio（gb12-lane5 取证，G-334 有界注记）。该残余依赖 Bun 上游提供隔离/可替换全局面（非本移植可解，也非 opentui-rust 缺口），按上游阻挡项登记搁置；最小路径（构造处影参+权限门 Proxy）已备于 G-247 结构隔离后续项，Bun 提供能力面后按此恢复。
## 🟡 中

### G-248 ✅ steer 收件箱后不 wake
`handlers/session.rs:1962-1987` PATCH inbox 仅 `inbox.steer/queue`；`inbox.rs:405-413` 只发 delivery.changed，全链无 `execution.wake`（对照同文件 prompt :783/synthetic :822/compact :895/move :1061 均有）。ref session/session.ts:134-141 `steerInbox`=wake（queueInbox 无 wake :142-145）。空闲会话 queue→steer 滞留至下次任意唤醒。修法：steer 分支补 fire-and-forget `wake(Promotable::Steer)`。

### G-249 ✅ GET context 缺原生 checkpoint 后置 system 过滤（G-96 仅截断半边）
现实现仅 `rposition(is_completed_compaction)` 截断（含非原生）；ref store.ts:179→history.ts:102-107 `findLast(isCheckpoint)` 后丢弃 `type==="system" && seq<native.seq`（原生 checkpoint 完成即推进 instruction epoch）。原生压缩后 context 残留陈旧 instruction-update system 消息。修法：截断后再过滤（replayable 过滤对 "latest" 恒真无需移植）。

- 2026-11-09 第十八轮勘注：过滤已按 ref 落地；"latest" 视角下该过滤被截断锚点遮蔽（实际发射面=provenance 边界，已在 context.rs）——语义等价、实现保留。
### G-250 ✅ session import 非原子
`transfer.rs:124-237`：`store.create`（含 session.created 提交，独立事务）成功后另开事务插消息/回填——两事务间崩溃留半导入（重复 import 409 无法自愈）。ref transfer.ts:89-152 全部在 `bus.publish(Created, commit 钩子)` 同事务内。另：:182 INSERT OR REPLACE vs ref 裸 insert（重复 id ref 整体失败、rust 静默覆盖）；metadata 继承分歧（store.rs:615-621 None 回退继承本地 parent vs ref 传导出值）。修法：并入 create 发布事务或 tx 回调缝；OR REPLACE 改裸 INSERT；import 强制显式 metadata。

### G-251 ✅ 子会话 interrupt 默认归类 Shutdown
`execution.rs:547-551` None→`unwrap_or(Shutdown)`；`engine/mod.rs:2287-2297` `EngineChildExecution::interrupt` 传 `..Default::default()`（reason=None）→ wire reason="shutdown"（ref execution.ts:155 缺省 "user"）+ 该分支不发 claim 释放（:827-847，claim/time_suspended 保留）。**反证修正**："job 不取消"不成立——`subagent_job.rs:152-155` SubagentJob::interrupt 无条件 `jobs.cancel(child)`（与 reason 无关）；主会话用户中断恒传 Some(User)（engine/mod.rs:1874-1881）。修法一行：interrupt 显式传 `reason: Some(User)`。

### G-252 ✅ compaction 事件发布双缺陷
① 手动压缩失败双发：compaction.rs:745-766 内部 publish Failed 一次；manual_compaction→`Err(RunFailure::surfaced)`（drain.rs:614-620 注释自认已发布）而 drain 错误臂 :443-472 对所有非 Interrupted（含 surfaced）再发一次——一次失败的手动 /compact 两条 durable 事件（auto 路径无此问题 :695-698）。ref 单发（compaction.ts:237-243 matchEffect；llm.ts:137-152 只覆盖 defect/interrupt）。② reason 非法枚举值：compaction.rs:65-68/:753/:771 以裸 JSON 写 `"overflow"`——**双侧 wire schema 都只允许 ["auto","manual"]**（ref session-event.ts:588 Literals；rust session_message.rs:209-212 仅 Auto|Manual）→ typed 客户端解码失败、落进消息 data 列污染后续严格解码。ref compaction.ts:629 折叠 "auto"。修法：drain 错误臂只对 Interrupted/unsurfaced 补发；publish 折叠 Overflow→"auto"。

### G-253 ✅ 消息解码失败静默空壳
`handlers/common.rs:581-588` 损坏 data→`json!({})` 输出仅含 {id,type} 的空壳消息 200；ref history.ts:62-77→`MessageDecodeError`→显式报错+checkpoint providerContext 校验。另 :575-577 `decode_json_or_none` 使 session 行 metadata/model/permissions/revert 列 JSON 损坏时**静默从 wire 消失**。修法：类型化错误（HTTP 4xx/5xx）或至少 log。

### G-254 ✅ session.move 目的地解析弱化 + workspaceID 不透传
`handlers/session.rs:921-947` 原始 directory 直接用：无 trim、无 `~`/`~/` 展开、相对路径按**服务器进程 cwd** 解析（ref 按会话当前目录 resolve：move.ts:75-79）；`workspace_id: None` 硬编码（:941，ref 透传 :84）。G-107/108/233 之外的三小面。

### G-255 ✅ snapshot capture/restore 无 per-repository 互斥
snapshot.rs 全文件无按 gitDirectory 的串行锁（仅 :151/:337 last_error Mutex）；ref git.ts:170-172 `locks.withLock(gitDirectory)` 包住 capture/write/restore——同仓两会话并发可交错写树。修法：DashMap<gitdir,AsyncMutex> 串行三入口（顺手做条目淘汰）。

### G-256 ✅ compaction token 估算按字节（G-106④ 同型未修半边）
`compaction.rs:981-983` `input.len()/4`（字节）；ref util/token.ts:5 UTF-16 码元/4——CJK 内容估算约 3×ref，压缩触发阈值系统性漂移。修法：`chars().count()/4`（G-106④ 修 codemode.rs 时 compaction.rs 漏网）。

### G-257 ✅ warming 会话保活运行时零实现
全 crates `warming` 仅命中 oc-schema/config/{mod,warming}.rs 与 normalize.rs（schema+normalize）；engine/boot/lifecycle/hooks 零引用——**配置被静默接受零效果**。ref plugin/warming.ts 107 行完整运行时（三 hook 记账、4min keep-alive session.generate、30min 窗口、opt-in）。G-193 修复清单确不含 warming，全册无 warming 条目。修法：runner 层对活跃 root 会话记账+定时 generate；最低限度 boot 对已配置 warming 输出警告。

### G-258 ✅ models-dev 快照缺两级滤除
ref models-dev.ts:94-109：provider 级滤除 `azure-cognitive-services`/`google-vertex-anthropic`（:95-98）+ bedrock `BEDROCK_PROFILE_ONLY_IDS` 13 项（:11-25,106-108）。rust catalog.rs:164-192 available_models 两级均无——两别名 id 与 13 个需 inference profile 的模型可见可绑定、选中即错。修法：catalog 装载补两级过滤+parity 测试钉滤除后清单。

### G-259 ✅ azure 部署发现 / DO routers 凭据驱动模型清单缺失
azure：ref azure.ts:189-313（ResourceGraph→deployments 分页→模型收窄）；rust 零命中——G-165 只接静态 catalog 车道，azure 展示全量、选中未部署模型才失败（反向更差）。digitalocean：ref digitalocean.ts:137-170（routers 清单+`router:*` 注入）；rust 无。gitlab 半边已由 G-8 ℹ️ 裁决（勿重复）。修法仿 modal_sync/copilot_sync 模式（凭据门控惰性 sync+transform）。

### G-260 ✅ integration 运行时五缺陷（打包）
① GitLab 刷新 single-flight 无 60s 成功保留：providers_enterprise.rs:306-329 仅 keyed Mutex；ref gitlab.ts:50-51——并发二次消费者持已轮换 refresh_token 交换→invalid_grant 误报 revoked。② ChatGPT 缺 ext_agent_host_id/savedClientID 复用：ref chatgpt.ts:91-107、:509；rust 重授权注册新 client。③ 表单校验子集：mod.rs:1296-1335 仅 required/pattern；ref form.ts:238（Unknown field）/305-352（options 闭合集/minLength/format）；**且 field_visible（:1328-1335）只认 op=="eq"——neq 条件字段恒判不可见而静默隐藏**（比少校验更重）。G-150 修的是 CLI 面。④ Azure form 恒挂+无 per-scope token 缓存+失败文案硬编码：providers_enterprise.rs:370-441；ref azure.ts:122-123 configured 短路/:94 缓存。⑤ Console 无 baseURL 的覆盖整条丢弃：console.rs:418-424 双无 baseURL→return None；ref opencode.ts:245-261 merge 进 catalog 基底。

### G-261 ✅ openrouter 头注入 + 隐藏别名缺失
① ref openrouter.ts:9-14 注入 `HTTP-Referer: https://opencode.ai/`+`X-Title: opencode`；rust plugin_headers.rs:3-20 只烘焙 cerebras/kilo/nvidia/llmgateway 四家——缺 openrouter（附带 zenmux.ts、vercel.ts:13 同缺，zenmux 注意 ref 展开序用户头胜出、与既有四家相反）。② ref openrouter.ts:16-28 对 `gpt-5-chat-latest`、`openai/gpt-5-chat` 置 enabled=false；rust 仅 copilot 侧（G-167）——snapshot 含 5×gpt-5-chat-latest、2×openai/gpt-5-chat 可见可选、选中即错。修法：catalog 对 openrouter 包两 id 置 enabled=false+plugin_headers 补支路。

### G-262 ✅ openai omitOutputLimit + bedrock CHAIN_ENV 激活缺失（打包）
① ref openai.ts:316-322：openai provider 的 context/compaction hook **无条件 delete options.maxTokens**（ChatGPT 后端拒绝 output limit 且计入限额）；rust model_request.rs:665 只插入全链无剔除——每次 openai 上下文/压缩请求 wire 都多带 maxTokens。② ref amazon-bedrock.ts:8-52：5 个 CHAIN_ENV 或 settings.profile→激活 auto→enabled、默认 region us-east-1、settings.endpoint→baseURL 迁移；rust 零命中（providers.rs:56-81 只做了 env-key 收窄半边）→仅凭环境 AWS 凭据的用户 bedrock 恒不可见。

### G-263 ✅ bedrock 协议三残留（打包）
① 纯文本历史 system 消息 converse.rs:406-417 直推无 `<system-update>` 包裹且 cache 恒 None（ref bedrock-converse.ts:347-353 经 wrappedSystemUpdate+末段 cache）；② 流异常 :967-975→event_error 无 classifyProviderFailure（ThrottlingException 丢重试信号；ref :798-808）；③ document 命名 converse.rs:140 filename 原样/document-N（无 200 截断/去扩展/唯一后缀/Attached-file 提示块）+base64 不重验证；ref bedrock-media.ts:60-83。G-18/G-19（已修）不同面。

### G-264 ✅ permission.saved.list 无 projectID 时返回全部项目
`handlers/permission.rs:63-101` 无 query 时 `SELECT * FROM permission` 全量；ref handlers/permission.ts:87-96 缺省 `location.project.id`。与 G-97 不同面（正是该修复移除 location 解析留下的缺省语义）。两侧官方客户端恒传 projectID → 🟡。修法：按请求 location 派生缺省或裁决登记。

### G-265 ✅ agent 全局 4 条 external_directory allow 未接线
`file_access.rs:327-339` `default_agent_external_allows` 生产零调用（仅测试引用）；engine/mod.rs:2853-2932 seed 不追加；oc-schema/agent.rs:44-72 仅 5 条基线。ref agent.ts:59-83 对每个新建 agent 追加 `<data>/shell/*/*`、`<data>/tool-output/*`、`<tmp>/*`、`<config>/*` 四条 allow（findLast 语义下生效）。影响：build/plan 等代理读 shell 输出/tool-output/tmp/config 触发 ref 不会有的询问（fail-closed 噪声）。修法：seed 与 config agent 注册路径统一追加（纯接线）。

### G-266 ✅ 工具三缺陷（打包：websearch random / read limit:0 / worktree 指南）
① `websearch.rs:238` `available.first()` vs ref websearch.ts:143 Math.random——affinity 粘滞使目录第一个 provider 独占流量直至 429（修法一行 gen_range）。② `read.rs:462`（:409 同）Some(0)→空页+truncated 翻页提示（ref falsy→2000；offset 已正确镜像 falsy :461 唯 limit 漏；schema minimum:0 使 0 合法输入，可能诱发无效翻页循环；修法一行）。③ `opencode_tools.rs:9` WORKTREE_GUIDANCE 死常量（ref opencode.ts:71-81 三钩子 SystemPart；G-192 建好的三挂点即现成挂点）——Code Mode 下 session_move 迁移指引丢失。

### G-267 ✅ 工具两残留（打包：shell 通知 metadata / edit 读回失败）
① `tool/shell.rs:452-460` 后台完成通知 output:None 恒置；`shell/result.rs:101-107` notification() 在 Some 时本会并入 exit/truncated/timeout/signal；ref shell.ts:155-183 completed 传完整 Output。G-204 修的是 tool_result 路径——通知路径漏网。② `edit.rs:398-405` 两分支 `unwrap_or_default()`——读回失败静默"成功"且 diff 呈整文件删除失真；ref edit.ts:205-209 失败即整调用失败。G-90 修的是 patch，同型不同文件，照抄修法。

### G-268 ✅ credential HTTP handler 旁路 core 正确实现（双实现漂移）
① create 激活语义：`handlers/credential.rs:124` `activate=as_bool().unwrap_or(false)`、:122 label 缺省 ""、无 current 检查、无 legacy pin；ref credential.ts:110/126/131-133（label ??"default"；`activate !== false || !current` 缺省激活+首凭证强制；legacy pin）——**经 HTTP/API 创建某集成首个凭证默认 inactive→集成无选中凭证、登录后模型不可用**。② list：:87-93 仅 active==Some(true) 入选（纯 legacy NULL-active 库全显 inactive；ref handlers/credential.ts:53-63 排序取每集成末行 selected）；:106-107 value 解码失败降级 Null+NULL integration_id 行以 null 输出——G-225 修的是 core store 的 fail-the-read 与 NULL 过滤，HTTP handler 未用同一实现。**oc-core CredentialStore::create（integration/credential.rs:115-160）实现完全正确**——同仓两套路径行为不一致（G-117 lane13 有意保留 handler 本地 SQL 只修了 time_updated，本条是该决策的代价面）。修法：handler 事务改走（或复刻）CredentialStore 语义，保持 wire 形状不变。

### G-269 ✅ persistent-pty WS framed 输入不校验
`handlers/persistent_pty.rs:564-575` decode_framed 接受任意 kind（0→control，其余一律当输入）且不校验 cols/rows=0，畸形帧直达 persistent.input/control（:517-535）；ref persistent-pty.ts:213-218 三条件丢弃。附带：framed 模式下 ref 对 Text 帧也走帧解析（:202-205），rust 对 Text 恒走 write。与 G-63②（非 framed，已修）、G-76（REST，已修）不同面。修法：补三条件丢弃+framed Text 对齐+wire 测试。

### G-270 ✅ server wire 语义族（打包 10 子项；①②④ 🟡，余 🟢）
① agent.list：agent.rs:30-34 过滤 !hidden 且不重排（ref agent.ts:123-129 全量+默认置首）——hidden agent 不可见、默认 agent 不置顶。② reference.list（族内最重）：agent.rs:106-164 弃 `services`（`let _services`）自行拼装——git 源 path=URL 原串、非法条目保留、description/hidden 恒 null、无物化（"看得见用不了"）；**core 侧正确实现已在位**（reference/mod.rs:195 ReferenceService::list() 含校验+cache_path+invalid 丢弃，tests.rs:443 钉定）——修法即 handler 改用 services.references.list()+接 refresh。③ fs.find 返回绝对路径（fs.rs:5-6 自认 "absolute-shape gap"；ref RelativePath）。④ vcs.init：project.rs:1826-1866 非 git 一律 501（ref 未知 provider 400 field:"provider"、注册无 init 才 501）、conflict 文案无 resource、git 失败 500（ref 503）、成功后无 location invalidate+无 worktree.updated。⑤ 未就绪 /api/info 恒含 capabilities（ref 仅 {version,pid,urls,paths}）。⑥ pty.connect location 回退伪造未校验 LocationRef 进 WS（ref locationMiddleware 404 先行）。⑦ worktree.list 强制 projectID==当前 location 否则 404（ref 仅按 DB 行）。⑧ form 重复 id `_tag`=FormAlreadySettled vs ref ConflictError（typed 客户端误分类）。⑨ ID 前缀 ses_/per_ vs ref ses/per（记录性）。⑩ credential Entry 多 time、Session.Info 多 slug（记录性→G-40 族）。

### G-271 ✅ TUI 交互五缺陷（打包）
① select_terminal 在 UI 线程同步落盘：store.rs:5526-5560 每次 open+read+write ChannelStorage 于泵线程（建议移持久化 drain）。② select_worktree 忽略 directory：store.rs:5331-5335 `_directory` 显式忽略只回 Home——G-34 的 set_home_location 缝在（/cd 路径用）此入口未接，选定工作树后新会话目录可能错；修法即选中时 `set_home_location(directory)`。③ DialogMcp needs_auth 无集成跳转：toggle 恒走 mcp.connect HTTP（app.rs:5934-5958）；ref dialog-mcp.tsx:103-105→DialogIntegration autoConnect。④ integration method.form 多字段表单流缺失：IntegrationSink（store.rs:2137-2149）仅 key/oauth/command 三流；ref formAnswer（G-150 是 CLI --answer 面）。⑤ session 列表搜索退化为本地 substring（SE-F4）：ref dialog-session-list.tsx:40-78 150ms 防抖→服务端 session.list{search}——本地窗口外的旧会话永远搜不到。

### G-272 ✅ mini 剩余功能面族（打包 15 子项，对照 G-181/G-221/G-134 已修面）
catalog（无 agent catalog/无刷新，runtime.rs:275-276/1140）；footer 路由族（FooterKey 仅 10 键 lifecycle.rs:98-112——ctrl+p/agent/model/variant/settings/queued-menu（ref footer.ts:752）与 findFiles 不可达）；每轮 switchAgent/switchModel（仅 ctrl+t）；prompt parts 附件（RunPrompt 仅 {text,shell,delivery} queue.rs:20-26；ref types.ts:37-52 RunPromptPart+`<file name>` 包裹）；`!` shell 直通**半接线**（queue.rs:22-23 解析 shell 模式但 run_turn runtime.rs:792-820 无执行臂，死字段）；inbox 队列操作 UI（仅 len 显示 runtime.rs:682-686；ref QueuedPromptAction）；session.background；根会话 session.renamed（子代理标题在 transport.rs:1395）；**SIGINT（ref runtime.lifecycle.ts:9 两击退出+草稿清理；rust kill -INT 默认终止）**；终端标题；verbosity 三档预设；样式化滚动回写 surface（Text/Code/Markdown/高亮/主题色/图片预览整层缺）；TOOL_RULES 完整文案/view 门/structuredFallback；usage 显示+step.failed 错误行（transport.rs:206 step.* 全 Ignored）；**重连 hydrate 不恢复 pending permission/form（族内最重）**：runtime.rs:890-916 仅消息+inbox；ref transport.ts:952-1004 并行 permission.list+form.list+session.active→syncBlockers。建议拆 2-3 条落地，优先 hydrate blockers ≈ footer 路由族/catalog。

- 2026-11-09 第十八轮勘注：15 子项中 ①-⑨ + lane1 的 5 个优先块全部落地（hydrate blockers/footer 路由族/catalog/usage+step.failed/renamed 由 gb10-lane1 落地）；⑩ 样式化滚动回写 surface 登记为有界偏离（mini mono 直写惯例，G-68/G-134 同族先例；ref 为 ≈1410 行 renderable 层）。
- **2026-11-12 第二十五轮增补收口 ⑩ ✅（gb13-laneK，opentui-rust `a8e41e9`/`5a079fa`/`34a5ac4` + oc `fad7fd8`/`6ae37997`/`6fea30de`）**：用户下令落地——opentui-core `ExternalOutputCommit` 增 styled 载荷（`Option<Vec<CapturedLine>>`，None=plain 路径字节恒等；`transcript_bytes`/`row_widths` 分派：styled 转储 SGR 行+行宽按 span 文本图距计（不数 ANSI，含 raw-mode CR 排除修），`commit_rows_styled` 经 `get_span_lines()` 提取）+ PORT-MATRIX 记账；差分 127 IDENTICAL 零重钉（无场景构造 commit，可证不受影响）；oc 侧 rev bump 56a9eda→34a5ac4 + `mini/entry_styled.rs` 按 ref kind 映射样式化发射（assistant markdown 走 `render_markdown`、reasoning→code(markdown)、工具 Code 快照 filetype 委托 G-302 `filetype_for_path` 表（含 js→typescript 归一）、user/system/error 按 entryLook 着色、oklab 量化色），经 `write_styled_to_scrollback`→styled commit 走同一 FIFO/flush；mono 开关全程保留（`Lifecycle::set_mono` 实时达写入手，footer.ts:932 对位；mono=true=旧 plain 路径字节恒等、exit splash 不动）；append-only 契约保持。有界偏离：逐 chunk 独立渲染（ref 为累计内容+committedRows/Blocks 高水位——跨 chunk 构造按块解析，mono 同粒度）；主题基座 fallback_dark（OSC 探针实时换肤 v1 范围外）；图片预览 v1 范围外。
### G-273 ✅ 编辑器集成族（打包四子项，前二为主）
① at_mentioned 完全缺失（ref editor.ts:242-247→mentionListeners；editor_ide.rs 零命中）。② IDE 锁文件仅启动解析一次：editor_ide.rs:213-227 重连恒用同一连接（注释自认）；ref editor.ts:175-176 每次 connect() 重跑 resolveEditorConnection——中途重启/切换 IDE 后上下文静默失效。③ Zed contents 失败不回退读盘（editor_zed.rs:132-135 仅 Ok(None) 读盘，Err 直接 Unavailable；ref editor-zed.ts:63-67 失败或 null 都 readFile）。④ is_dir vs existsSync（🟢）。修法：②重连重跑 discover；③Err 臂补读盘。

### G-274 ✅ CLI 全局 flag 超集静默忽略
cli.rs:33-76 六个 `global=true` flag（--print-logs/--standalone/--server/--auto/--yolo/--dangerously-skip-permissions），而 mcp.rs:31/323/401/618 等用 `ServerParams::default()`——`opencode mcp list --server URL` 解析成功但**静默忽略**（查的是本机后台服务）。G-17 "rust-extra" 备注不覆盖（那是新增能力；这是吞掉用户意图的排障陷阱——ref 会立刻报未知 flag）。修法：mcp/debug 等族透传 ServerParams，或收窄为 per-subcommand 参数。

## 🟢 低（打包条目）

### G-275 ✅ 事件载荷两小项
① `session.step.started` 缺 snapshot 字段：publisher.rs:343-354 无 snapshot 入参（schema 有列 session_event.rs:321-322；step.rs:199-201 捕获了 start_snapshot 未送入）；ref publish-llm-event.ts:103-110。② fragment provider-state 整体替换：publisher.rs:122-124 vs ref :161/:189 合并——分片携带部分 state 时 Text.Ended 载荷丢字段。

### G-276 ✅ infra 四小项 + canonical POSIX 归一（末项 ℹ️ 倾向）
① Form publish 失败不回滚且先改状态后发布（forms.rs:137-140/:250-307 vs ref form.ts:145-146/:188-194）。② bus replay 用 serde Value 相等（1≠1.0 假性 diverged；bus.rs:936-943 vs ref isDeepStrictEqual）。③ per-aggregate locks 无淘汰（bus.rs:405-417；wakes 有清理 :434-437，仅 locks 慢泄漏）。④ v1 迁移手写子集校验+字节序排序（v1_migration.rs:239-283/:300 vs ref 完整 Schema+localeCompare）。⑤ canonical 键 POSIX 恒归一 `\`→`/`（location.rs:75-80 注释自认；ref 仅 win32）——建议登记文档化偏差或改条件归一。DbError::Pragma 字符串隧道（bus.rs:493-511）无现实触发路径，随重构改类型化即可，不单独立项。

### G-277 ✅ shell/PTY 三小项
① is_login_shell 未小写化（pty.rs:825-828；ref select.ts:141-144 toLowerCase——shell/select.rs 自身已小写仅 pty.rs 助手漏）。② 捕获文件清扫 `sh_*` 前缀过滤弱（shell.rs:63-66 vs ref `/^sh_[0-9a-f]{12}.*\.out$/`——过界删除方向）。③ daemon 默认程序仅 `$SHELL||/bin/sh` 无校验回退链（oc-pty/daemon.rs:424-429 vs ref select 链）——$SHELL 悬空 spawn 直接失败。

### G-278 ✅ fs/vcs 三小项
① MIME 缺 BMP/AVIF 魔术+isText 控制字符启发式（read.rs:122-138、fs.rs:34-49 vs ref mime.ts:3-33；G-92 收敛的是扩展名表）。② find 索引额外排除 target 目录（fs.rs is_excluded_dir；ref ripgrep 不排除——rust 项目自身文件 find 不可见，超收收紧）。③ repository file://：host 当路径段（ref fileURLToPath 拒绝）+canonicalize 触碰文件系统改变 cachePath 键（repository.rs:261-274）。

### G-279 ✅ session/runner 六小项
① session_message.time_updated 不推进（projector.rs:1090-1109/:624-648；wire/分页不受影响，受众=直接读 DB 的外部消费者；同族 G-117）。② usage 扇出失败仅 eprintln（projector.rs:399-405）。③ title 流取消丢 usage 记账（runner/title.rs:130-158；ref onInterrupt）。④ attachmentLocation 不解码百分号（to_llm_message.rs:47-62 vs ref URL.parse+fileURLToPath）。⑤ 估算用注册名而非 effective 名（compaction.rs:1037/:1063；registry.rs:34-38 已有 effective_name 未消费）。⑥ advance_to_step 不观察 shutdown（drain.rs:311-521；中断延迟窗口，外层 UNWIND_TIMEOUT 兜底）。

### G-280 ✅ MCP/插件三小项
① 瞬态重试第二跳 500ms（client.rs:537 DELAYS_MS=[250,500]、:1511 同，注释自称 ×4 实为 ×2；ref client.ts:170-182 =250/1000）。② elicitation number/integer 合并为 Integer（elicitation.rs:416-426——浮点无法输入；Field::Number 变体与处理臂都在，一行改映射；format 白名单半边非缺口——ref wire 字面量同四项）。③ `./rpc` 入口探测与 features.rpc 缺（ref host.ts:17-43/module.ts:115-121；rust install.rs:264-281 仅 server/tui——plugin list 的 rpc 字段面，极小）。

### G-281 ✅ config 四小项
① jsonc→json5 宽松解析（config.rs:535 vs ref config.ts:106 jsonc-parser——坏配置静默接受）。② experimental.mcp_timeout/mcp.timeout.* 接受 0（normalize.rs:812-816/:886-933 vs ref PositiveInt）。③ WatchHub sync_channel(1024)+try_send 风暴丢弃（watch.rs:165,193,205 vs ref unbounded+sliding(1)——reload 幂等，丢的主要是逐文件通知）。④ update_shell 无锁并发写交错（config.rs:695-712 vs ref updateLock）。

### G-282 ✅ websearch/tools 六小项（含 G-150 残边）
① providers() 字节序排序（websearch.rs:104 vs ref localeCompare；G-154 同族）。② selection 无效 KV 不清理（websearch.rs:124-140；卫生级）。③ escape_destination 连续空白逐字符 %20（html_markdown.rs:1672-1686 vs ref 一个 run 一个）。④ Exa 日期仅 ISO 前缀（websearch_providers.rs:238-253 vs ref Date.parse）。⑤ grep match 截断按 char vs UTF-16 码元（grep.rs:273-274；与 G-116①/G-126 同族并注）。⑥ --answer skip 写入 "" vs ref 键不写入（auth_form.rs:200,300 vs ref form.ts:95,46——G-150 落地面未覆盖的残边）。glob cause 前缀/cf 头大小写两记录性面归 G-40。

### G-283 ✅ credential/oauth 小项（打包）
① oauth success 页 provider 未转义（oauth_page.rs:25；error/bootstrap 已转义，一行补齐；autoClose:false 为 ref dead API 不立项）。② CLI key 提示不优先 method.label（resolve_method 本身已按 label，仅提示面）。③ auth import 坏 value 降级空 Key 凭证（ref 严格 schema 报错）。④ logout/switch 集成过滤与文案偏差（G-228 修的是 connect 空错误，不同面）。⑤ wellknown URL 探测大小写敏感（auth.rs:289；ref URL.canParse 接受 HTTP://）。

- 2026-11-09 第十八轮勘注：本条为十八轮唯一漏派项（lane 计划遗漏，审计发现后补 lane17 全量落地）；路径勘误：oauth_page 实位于 `mcp/oauth_page.rs`（原登记 integration/）。
### G-284 ✅ integration 五小项（打包）
① connection.status 无变更去重（mod.rs:568-584 无条件 notify；ref Equal.equals 跳过）。② xAI 空 refresh_token 静默入库+无 JWT exp 回退（providers.rs:557/:560-561）。③ copilot entitlement 单文案+自造 contains("copilot") 分支（ref github-copilot.ts:365-371 双文案且 chat_enabled!==false 先放行——可能反向误拒；G-167 不含此面）。④ editor.remove 整合删除缺失（ref integration.ts:149/309）。⑤ wellknown Auth.command 不解析+嵌套 config 键偏好缺失（wellknown_config.rs:31-34/:196-200 vs ref wellknown.ts:13-16/:95-96）。

### G-285 ✅ provider 三小项（打包）
① prompt-cache-key 11 家目录商 supportsPromptCacheKey 置位缺失（ref prompt-cache-key.ts 的 abliteration-ai/aihubmix/…/upstage；交集仅 venice 已覆盖——G-121/171/175 均不含，grep 零命中）。② snowflake 400 "conversation complete" 假流响应改写缺失（ref snowflake-cortex.ts:222-235 http.response hook；401→刷新 retry 半边待取证）。③ zenmux/vercel 的 http-referer/x-title 头（并入 G-261 修复亦可）。

- 2026-11-09 第十八轮勘注：①③✅、② 的假流响应改写已落地；**401→刷新 retry 半边已于同日收口 ✅**（gb10-lane19 `29f9858f`：取证裁定 ref snowflake-cortex.ts:236-246 为 retry-hook 形状（401+attempt==2+browser-OAuth+无 env token→强制刷新→延迟 0 重试一次）；落地为 auth_retry_401 transport seam + IntegrationHost::connection_force_refresh + snowflake 接线，Authentication 全局分类未动；有界偏离：刷新失败回显原始 401 而非 ref 的 orDie 流杀）。
### G-286 ✅ 媒体/WS 七小项（打包）
① google-images n>1 错误消息漏 format!（google.rs:77 字面 {n}，一行）。② google-speech 3.8-tts 漏 `-tts-*` 后缀变体（google.rs:30-33 strip_suffix vs ref 正则）。③ MediaUsage details 缺 provider 包裹+google 丢 candidates 明细+bfl/fal null 字段序列化为 null（ref 省略键）。④ append_fields 空数组追加 "[]"（utils.rs:180-183/:199 vs ref 无操作）。⑤ SessionChannel poison 无 active 也计数（websocket.rs:1140-1152 vs ref model-transport.ts:178-181——更快永久 HTTP 固化；Interceptor/指标缺失附注）。⑥ meta-messages thinking raw 直挂绕过 budget 校验（customization.rs:199-207 vs ref 委托泛型；G-172 修的是路由默认，显式 enabled 无 budget 的校验差距残留）。⑦ open-responses minted id 确定性计数 vs randomUUID（open_responses.rs:1449-1527 代码自注；记录性）。

### G-287 ✅ TUI 🟢 小项族（打包）
① Recording sink 默认桩无"生产已装配"debug_assert。② DialogUpdate 按钮模型列表行 vs ref 横向按钮+焦点循环（化妆级；标题六态已对齐 update.rs:140-157）。③ 终端右 pane 无拖拽调宽。④ terminal.copy 默认值平台无关化（dialog_config.rs:404-406 恒 "select"；ref win32→"manual"，修法 cfg!(windows)）。⑤ 崩溃屏三行文本（update.rs:176+ vs ref 258 行 Copy report/issue 链接/stack）。⑥ 模型选择器 Favorites/Recent 分区与搜索期收藏优先（G-135 修写路径，dialog 渲染面缺）。⑦ DialogOpen attention 排序+ses_ 直查（noMatch 提示已在）。⑧ 超链接渲染（markdown.rs:181-184 hyperlinks_supported:false，缝在光栅未接）。⑨ parseFileLineRange 的 #N-N 校验残边（query-base 已随 G-127 落地）。⑩ transcript 共享 E1 walk O(all-rows)+40ms 临时预算（solid 路径 Wave-B 窗口化已落；登记性能债并澄清 frame_budget_tests 注释）。

- 2026-11-09 第十八轮勘注：①②④⑤⑥⑦⑨⑩ 落地（⑩ 为登记性注释）；③ 终端 pane 拖拽调宽已于同日收口 ✅（gb10-lane19 `998bf2e4`：手柄命中/拖拽跟随/clamp/<300ms 双击复位/paneWidth 持久化+boot 恢复（含 legacy terminalWidth）；默认渲染字节级不变）；**⑧ OSC-8 超链接已于同日收口 ✅**（用户批准重钉后 gb10-lane18 `229d64b2` 落地：seam 改探针驱动（upstream `ctx.capabilities?.hyperlinks`，headless 默认 false）+ 发射器 interned link-id 包裹；headless profile 保持 caps=off → 金帧字节级不变，**批准的重钉实际未用到（0 帧变化）**；差分 127 IDENTICAL 不变；点击路径核验已在位。有界偏离：OSC-8 矩形取 transcript OpenLink hit rows 而非 Frame link 字段）。
### G-288 ✅ CLI/client 🟢 小项族（打包）
① run --standalone 会清扫取消 global 表单（run/mod.rs:460-462 vs ref run.ts:149 恒 attached；:445-453 注释误引 ref，🧹 一并修）。② session import --directory 不绝对化（session.rs:323-326 vs ref path.resolve）。③ pair --url 带路径保留 base path（ref new URL 绝对路径替换；反代场景 rust 行为更合理——注释钉定或对齐）。④ acp 失败原因平文（acp/mod.rs:137 vs ref code/signal/MiB 细化）。⑤ uninstall 无条件自删二进制（misc.rs:224-229 vs ref curl 安装仅打印提示——破坏性命令更激进）。⑥ service set cors 接受带 path 的 origin（service_config.rs:313-320 已拒尾斜杠但 trim 不掉 path；ref url.origin!==origin 连 path 拒）。⑦ UnexpectedStatus body 无上限（transport.rs:242-244 vs ref 仅 status；cap 64KiB 或对齐）。⑧ with_part 乱序 ordinal 丢弃（store.rs 仅 seen==ordinal 补建；SSE 有序前提下安全，跨连接乱序边界可能少一段文本）。

## ℹ️ 登记

### G-289 ℹ️ 单 bun guest vs 每 Location 插件实例（G-28 族，需裁决）
ref supervisor 为 location 节点、每 Location 独立插件实例与作用域（supervisor.ts:241-258）；rust 全进程单 bun guest（supervisor.rs:2118-2127、plugin_apply.rs:1186-1199 用 PLUG-D07 按位置过滤注册）。多 location 并存时插件模块级可变状态共享。登记册/ADR 均无登记——按 G-28 生态 breaking change 声明并列补裁。

- 2026-11-09 第十八轮登记（裁决=登记不改码）：维持 G-28 生态 breaking change 声明并列项——rust 全进程单 bun guest（PLUG-D07 按位置过滤注册）为文档化架构决策；per-Location 插件实例隔离改造登记为后续工程项，非缺口。
### G-290 ℹ️ 对话框 ctrl+c 一次即清空并关闭（TUI-35 有界偏离入册）
rust dialog.rs:211-232 注释自declare 有意决策（clear-and-stay 会吞掉 ctrl+c×N 退出链，引 90s wedge 事故）；ref dialog.tsx:135-150 首按仅清空保持打开。按 G-226 体例登记有界偏离，不改代码。

- 2026-11-09 第十八轮登记（裁决=登记不改码）：有界偏离正式入册（G-226 体例）——ctrl+c 一次即清空并关闭为防 90s wedge 事故的有意决策（dialog.rs 注释自declare），不改代码。
## 归类与收编（不另立项）

- **MCP OAuth 登录入口仅 CLI** → G-151（mcp/oauth.rs:19-25 模块头已自declare；headless 部署需求属新范围决策）。
- **websearch progress 结果边界一次** → G-74（已登记有界偏离）。
- **根外 skill 目录热重载** → 第十五轮归类注记维持（config/watch.rs 仅覆盖根内；ref 对每个 skill 源目录 watch）——升格建议：若做 G-281③ 同 lane 可顺带。
- **perplexity/sap-ai-core 无承载** → G-17（依赖 aisdk: 通道；facade.rs:2945 测试断言 UnsupportedProtocol）。**G-17 补注**：variant.rs:1137-1138 为 sap-ai-core/gitlab 持续产出 variant 而 facade 无法绑定——目录 UI 展示变体、运行时必错（可选小修：variant 分发对无绑定包的 `aisdk:` 前缀停发）。
- **double_decode / client lifecycle 位置 / session.environment 全量 env / os_notify auto 档启发式 / 非 Linux 音频外部播放器** → G-40/G-16 记录性（os_notify 协议名与 env 已对照原生二进制核验，仅 auto 档选择为自declare 启发式；非 Linux 音频与 G-116③ 同族）。
- **aisdk: 通道 / experimental 评测** → G-17/G-7 复认。
- **glob InvalidPattern cause 前缀 / webfetch cf 头大小写** → G-40 记录性。
- **state host-side only / Effect 插件 / codemode 真引擎 / aisdk.language 句柄** → G-5/G-28/第五轮收编/G-2 复认（C12 五项核对无新增面）。
- **$PWD 策略分叉（run 守卫式 vs mini 真 chdir）** → G-17 勘注（建议择一统一）。

## 既有条目勘注（登记册订正）

- **G-229①（修复未达面）**：TitleChanged/ForegroundProcessChanged 的**客户端泵臂**仍丢弃（persistent_pty/transport.rs:1612-1636 `_ => {}`）——gb9 lane5 补的是 daemon/oc-server 两臂；daemon 为默认后端（frontend.rs:41-56）→ **默认配置下 G-229① 实际未闭环**（随 G-243 修复或单独收尾）。
- **G-63②（修复未达面）**：daemon 后端 Write op 强制 `String::from_utf8`（oc-pty/daemon.rs:677-684 失败即 error；Input op :715-731 接受字节）——同 API 两后端行为分叉；修法：Write op 走字节缝或服务端非 framed 分支改走 Input op。
- **G-234②（残半边）**：共享错误页仅 Poe 挂 Branded（flows.rs:330-337）；GitLab/Snowflake/ChatGPT×2 四条 code 流仍 Html 成功页（providers_enterprise.rs:259/735/941/1180）；另 rust 重定向即回包 vs ref 交换完成才 respond。
- **G-119（勘注）**：detect 表含 deepinfra 为有界超集（openai_chat.rs:1821/1856-1857，G-119 落地时写入；仅畸形配置有差）。
- **G-128（勘注）**：title-shimmer 静止态残留未列入有界偏离清单（sidebar_solid.rs:15-28 自declare：待 rename-pending state 入 store 再落）。
- **2026-11-10 勘注标闭（第二十轮条件核查）**：条件早已满足且实现已落地——`store.rs:2067-2068/:4422-4428` title_pending/set_title_pending 在位、`app.rs:9144-9152` POST rename 写入、`components/sidebar.rs:435-469` shimmer 动效本体已渲染（sustained 40% dim + 1200ms 扫光带）并被 sidebar_solid 消费；静止态=ref at-rest 分支自洽。残留仅为 sidebar_solid.rs:15-28 **stale 模块头注释**（仍 declare "no rename-pending input"）——🧹 文档清理项，随任意 TUI lane 顺手改（已清：2026-11-10 gb11-laneF `7bc929c1`，sidebar_solid 模块头更新为现行 rename-pending 状态）。
- **G-97（归因订正）**：该修复移除 location 解析后留下的缺省全量语义 → G-264。
- **G-150（残边）**：--answer skip 语义（skip→"" vs ref 不写入）→ G-282⑥。
- **G-17（两补注）**：①$PWD 策略分叉；②variant 为无绑定 aisdk: 包产出覆盖（见归类节）。

## 勘误（第十七轮经核验撤销的初报，勿再立项）

1. **DialogShellOutput 为静态快照无轮询** — 不实：app.rs:9695-9712 对 topmost viewer 1s 轮询（注释明引 ref；store.rs:16239+ 测试钉定；G-184 inline 臂同拍）——G-6/G-33/G-184 ✅ 与代码一致，初报读漏。
2. **composer @ 文件补全无 live source** — 不实：G-127 异步半边完整在位（autocomplete.rs:1539-1736；app.rs:2619-2697/:5626-5657/:9293-9317）；初报引用的 autocomplete.rs:515/:953 是 G-127 落地前**陈旧注释**（🧹：清理两处）。
3. **session-tabs 鼠标拖拽/5s 长按关闭缺失** — 不实：app.rs:132-136 strip_drag、:840-887 拖放持久化+close-hold、:1328-1346 拖拽臂、MOUSE_CLOSE_HOLD_MS+测试全部在位（spring 宽度动画亦 G-128 引擎覆盖）。
4. **DialogUpdate 标题不随状态切换** — 不实：update.rs:140-157 六态标题+busy spinner 在（按钮模型差异保留为 G-287② 化妆级）。
5. **MCP add/remove overrides 无持久化** — 撤销：ref HTTP 路径同样只写内存 overrides（index.ts:634-636,660-663）；两侧持久化都在 CLI 写配置，语义一致。
6. **插件事件 tap headless 不送达/rpc.\* 缺失** — 撤销：PLUG-D13 attach_event_tap（plugins.rs:1640-1662，:2137 生产调用）+is_public 含 rpc.\* 前缀（event_manifest.rs:65-73 测试钉定）；残留仅慢消费者有界语义（G-40 族）。
7. **websearch 全冷却错误类型漂移** — 大部不实：G-224① 落地后 websearch.rs:283-325 穷尽时已返回 last_cooldown；仅"冷却来自先前查询（本次无 429）"边界残留（记录级并入 G-282）。
8. **question 兜底消息参数快照失真** — 证据不足撤回：可见路径对齐（question.rs:183-201 vs question.ts:95-107）。
9. **mini 首个 text-start 事件顺序分歧** — 撤销：pending 批次存在前提是已有 fragment start（每个 start 臂双侧都先发 step.started），flush 位置差异不可观察。
10. **kv_scan 无默认 limit** — 撤销：hooks.rs:1367-1370 插件存储面（唯一生产消费方）已实现缺省 100（G-3 ✅ 成立）。
11. **git history.rootCommits 缺失** — 撤销：project.rs:1544-1560 root_commit_id（revwalk parent_count==0 取首）等价；ref 唯一消费者也只取 [0]。
12. **service set cors 接受尾斜杠** — 该子句不实：service_config.rs:313-320 已拒；姊妹发现（带 path 的 origin 接受）转 G-288⑥。
13. **mini duration_label 自拟** — 行为面撤销：生产用 locale_duration（model.rs:915-931，ref locale.ts 逐字数学）；queue.rs:132-212 duration_label 为零调用死代码（🧹：删除或转发）。
14. **media 缺 sanitizeSurrogates** — 撤销：Rust String 结构上无法携带孤立 UTF-16 代理项（sanitize.rs:5-16 已文档化 no-op），无行为后果。
15. **os_notify 协议映射未经核对** — 半面不实：os_notify.rs:1-23 模块文档——协议名与 env 开关已对照 0.5.14 原生二进制字符串核验；仅 auto 档 TERM→协议选择为自declare 启发式（→G-40 登记）。

另订正独立审查报告（PORT_REVIEW_INDEPENDENT_v2.0.24.md 所据 L18 通道）一处失实：「openrouter HTTP-Referer/X-Title grep 无命中」不实——plugin_headers 已有四家（cerebras/kilo/nvidia/llmgateway），缺的是 openrouter/zenmux/vercel（→G-261）；zenmux 的头优先级方向与既有四家相反（ref 用户头胜出），修法需单独核对。

## 第十七轮判定汇总

独立复审初报约 90 项缺口指控：**确认立项 50**（🔴×7：G-241..G-247；🟡×27：G-248..G-274；🟢×14：G-275..G-288；ℹ️×2：G-289/G-290）；**归类既有 12**（G-151/G-74/G-17×3/G-7/G-40 族×6 等）；**撤销 15**（见勘误，含与登记册 ✅ 状态矛盾的两处——均系陈旧注释/读漏，非回归）；**既有条目勘注 8**（G-229①/G-63②/G-234②/G-97/G-119/G-128/G-150/G-17——其中两条为十六轮修复的未达臂）。优先级建议：G-241 > G-242 ≈ G-247（安全契约）> G-243 ≈ G-244 ≈ G-246 > G-245 ≈ G-257（系统性默认/零实现）> G-268（凭证激活）≈ G-272-hydrate ≈ G-270② > 其余 🟡 > 🟢 族。

---

# 第十八轮 burndown 收口（2026-11-09，gap-burndown-10）

> 第十七轮立项的 G-241..G-288 全部 48 项可行动缺口 + 三条勘注残臂（G-229① 客户端泵臂与 oc-tui 消费臂、G-63② daemon Write 字节缝、G-234② 四条 code 流共享品牌页）+ G-17② 可选小修，经 lanes 1-17 串行落地并 fast-forward 合并 main（lane13b 为审计发现的 G-259 listing 残半边补_lane；lane17 为审计发现的漏派项 G-283 补_lane）。每 lane 独立 worktree（`.worktrees/gb10-laneN`，合并后即时回收，分支已清）、dev-gate 单入口、nextest 未过滤证据、每 lane workspace 合并门复跑；PTY 族 lane（1/4/14/15）另过 `--profile pty` 序列化门。一切门命令全量 tee 落盘 `.test-logs/*gb10-lane*`（含 --exact 复现与 canary 隔离复跑）。**无上游阻挡项**；唯一搁置项 G-287⑧（见下）。ℹ️ G-289/G-290 本轮登记入册。

## 逐 lane 落地清单

- **lane1 `71534002`（mini 文本流+stream-v2 优先块）**：G-241（text/reasoning delta 改纯增量 commit + ended 仅补 unseen suffix 为 Progress + 首块 `Thinking: ` 标记；多 delta PTY 钉定「全文一次 + final 更长补尾」）；G-272 优先五块（重连 hydrate 并行 permission.list+form.list+session.active→syncBlockers 挂载 + `--no-replay` boot hydrate；footer 路由族 ctrl+p/`<leader>q` 队列菜单/agent/model/variant/settings/queued 经共享 PanelShell；boot loadCatalog（agent.list+model.list）+8 catalogEvents+重连刷新按 location 过滤；step.ended→usage（tokens/占比/cost 进 statusline）+step.failed 错误行每消息一次；根会话 session.renamed→Fold::SessionTitle）。findFiles 依赖 prompt parts 归 lane15。
- **lane2 `ae5a8538`（MCP）**：G-242（McpToolExecutor 改 ToolExecuteWithContext 叶子，execute 首步 permission.assert（action=`{server}_{tool}`、resources/save `["*"]`），经 PipelineExecutor 包装吃 hook/decodeInput/encodeOutput/normalizeContent；server `outputSchema` 进 G-230 encodeOutput）；G-280（瞬态重试第二跳 1000ms（ref max(exp(250,4))=2 hops 取证）；elicitation number→Field::Number；`./tui`+`./rpc` 探测与 `/api/plugin` features 面）。
- **lane3 `fe8f9cd7`（CodeMode 沙箱）**：G-247 方案①——`scan_program` token 级静态扫描（剥注释/字符串/模板体，`${}` 插值按代码扫）拒绝 `import(`/`require`，leaf 与 guest host.js 双侧同规则同文案；setTimeout/setInterval/setImmediate 改抛错占位（soft-timeout 改模块级 native timer）；EXECUTE_DESCRIPTION（ref 逐字）现为真；14 拒/19 收语料 + bun guest e2e×3。有界偏差（`.import(` 属性豁免等）测试钉定。
- **lane4 `ba348458`（PTY 族，+pty 门）**：G-243（drain 回退 UTF-8 字符边界（0b10xxxxxx 同 persistent_pty）；attach 光标 UTF-16 码元→字节偏移换算 utf16_byte_offset；真 2MiB 缓冲测试）；G-229① 客户端泵臂转发 Title/Foreground（默认 daemon 后端闭环——oc-tui 消费臂由 lane14 补）；G-63②（daemon Write op 走 write_bytes 原样字节，0xFF 真进程往返测试）；G-269（framed 三条件丢弃 + framed Text 对齐帧解析 + wire 测试）；G-277（is_login_shell 小写化；捕获清扫 `/^sh_[0-9a-f]{12}.*\.out$/`；daemon 默认程序走 ShellSelect 校验链（$SHELL 悬空测试））。
- **lane5 `c8a1934c`（integration）**：G-244（device 兑换后并发 /api/user+/api/orgs 写全 5 键 metadata（server/accountID/email/orgID/orgName）、org 按 org_id 匹配、JSON 请求体、先 sleep 再首询、SSO 组织名；自托管 refresh/config 打到 answer.server 的 e2e 断言）；G-260（GitLab 刷新 60s 成功保留；ChatGPT ext_agent_host_id+savedClientID 复用；表单校验全量（Unknown field/options 闭合集/minLength/format）+ **field_visible 补 neq**；Azure form configured 短路+per-scope token 缓存+真实失败文案；Console 无 baseURL 覆盖 merge 进 catalog 基底）；G-234②（GitLab/Snowflake/OpenAI/ChatGPT 四条 code 流共享品牌 oauth_page；GitLab+ChatGPT 交换完成才 respond（held connection），Snowflake/OpenAI 按 ref 即时）；G-284（connection.status Equal 去重；xAI 空 refresh 回退+JWT exp；copilot 双 entitlement 文案（自造 contains 分支移除）；editor.remove；wellknown Auth.command 注册+嵌套 config 键偏好）。
- **lane6 `172d2d2d`（media）**：G-246（下载 headers 移到 runtime-only serde-skipped sidecar——`serde_json::to_value(asset)` 不含 headers 的安全测试；route 层 materialize（BFL decode 即物化）；Asset bytes()/materialize()/file()；replicate 过期自 completed_at）；G-286①-⑥（format! 修复；3.8-tts 正则变体；MediaUsage provider 包裹+google candidates+bfl/fal 省键；append_fields 空数组 no-op；poison 仅 active 计数；meta thinking 经泛型 budget 校验）+⑦（minted-id 确定性计数注释钉定）。
- **lane7 `21139879`（provider catalog）**：G-245（openai/xai 无条件、azure responsesWebSocketCapable 条件（含 baseURL regex 端口）烘焙 `transport ?? "websocket"` 默认，用户显式 settings 胜出；settings.transport 进 connection overlay）；G-258（provider 级滤除 azure-cognitive-services/google-vertex-anthropic + bedrock 13 项 profile-only 两级过滤，embedded+refresh 双路径，计数钉定）；G-261（openrouter 两 chat 别名 enabled=false + openrouter/zenmux/vercel 头注入（zenmux 用户头胜出序，与既有四家相反））；G-285①③（12 家 supportsPromptCacheKey；snowflake 400 假流→200 SSE 终止符内建拦截器；⑨ zenmux/vercel 头并入）+②半边（401→刷新 retry 缓置，需 retry-hook 脚手架）；G-17②（variant 分发对无绑定 `aisdk:` 包停发）。
- **lane8 `8361c211`（engine/tools）**：G-257（`engine/warming.rs` 全量移植 warming.ts：三 hook 记账、per-session keep-alive generate（`min(last+interval, expires)`）、30min 滑窗、warm-prompt 自检测、opt-in、bundle 关停；MutableClock+ManualSleep seam 7 测试）；G-265（`default_agent_external_allows` 四条接线：内建 seed（ref 顺序）+ config `RegistryAgentEditor::ensure`，golden 测试）；G-266（websearch `random_provider` 均匀选取（with_picker 固定种子 seam）；read limit Some(0) falsy→2000 双分支；WORKTREE_GUIDANCE 三钩子注入）；G-267（shell 后台完成通知携带完整 Output metadata（completed 门控）；edit 读回失败传播 `Unable to edit {path}`（FailSecondRead seam））。
- **lane9 `03445bcf`（session 执行）**：G-248（steer 分支 fire-and-forget `wake(Promotable::Steer)`，queue 分支不 wake；HTTP 测试空闲 queue→steer 排空）；G-249（G-96 截断后补 `drop_pre_checkpoint_system_rows`（ref-exact）；"latest" 下被锚点遮蔽勘注）；G-250（import 回放并入 `session.created` commit 钩子（create_with_commit+explicit_defaults）；裸 INSERT 重复 id 原子回滚；metadata 显式不继承）；G-251（子会话 interrupt 显式 `reason: Some(User)` + claim 释放，wire 断言）；G-252（drain 错误臂对 surfaced 不补发——手动压缩失败恰好 1 条 durable 事件；`Reason::wire()` 折叠 overflow→"auto"，typed 严格解码钉定）；G-256（token 估算 `encode_utf16().count()/4`）。
- **lane10 `b0ce39ed`（server/infra）**：G-253（损坏 data/列 JSON→log+500 UnknownError（ref failedMessageDecode），不再静默空壳/丢列）；G-254（destination trim+`~` 展开+按会话目录 resolve+workspaceID 透传）；G-255（`DashMap<gitdir,AsyncMutex>` 串行 capture/files/diff/restore + 空闲自淘汰）；G-275（step.started 携带 start snapshot（None 省略）；fragment provider-state delta+end 合并）；G-276（forms 先发布后改状态+回滚；bus replay `json_deep_equal`（1==1.0）；per-aggregate locks 三处淘汰；v1 迁移 localeCompare 决胜+mode/path 校验；canonical `\`→`/` 归一 cfg(win32) 门控）；G-279（time_updated 推进（钉 event.created）；title 流取消经 Drop guard 仍发 usage；attachmentLocation 百分号解码；估算用 effective_name；advance_to_step ready 边界观察 shutdown；usage 扇出 stderr 文档化）。
- **lane11 `88e2700b`（server wire/credential）**：G-264（saved.list 缺省 projectID 经轻量 NoEvents resolver 按请求 location 派生——G-97 显式路径旁路保留）；G-268（handler 复刻 CredentialStore 语义：label 缺省 default、`activate!==false||!current` 缺省激活+首凭证强制、legacy NULL pin、list 排序 selected-last+NULL integration 丢弃+解码 fail-the-read、409 带 resource；wire 形状不变（HTTP 测试钉定））；G-270 全十子项（agent.list 全量+默认置首；reference.list 改 per-location ReferenceService（cache_path/invalid 丢弃/description/hidden）+config 变换；fs.find 相对路径；vcs.init 错误族（未知 provider 400 field:"provider"/无 init 501/conflict 带 resource/git 失败 503）+invalidate+worktree.updated；/api/info capabilities 仅 Ready；pty.connect 不伪造 LocationRef（404 先行）；worktree.list 按 DB 行；form 重复 id ConflictError；⑨⑩ 注释钉定记录性差异）。
- **lane12 `a2016099`（ai 协议）**：G-262（openai Primary/Compaction 请求无条件不带 maxTokens；bedrock CHAIN_ENV（ref 原文 5 变量）∨Bearer→激活、region 缺省 us-east-1、settings region/profile/endpoint 贯通（mantle auth 臂同步））；G-263（历史 system 消息 `<system-update>` 包裹+末段 cachePoint（非文本 typed error）；流 exception 帧经 classify_provider_failure（Throttling/ServiceUnavailable 可重试）；document 命名按 bedrock-media.ts（去扩展/字母序/200 截断/` N` 去重/Attached-file 提示块）+base64 严格重校验）。
- **lane13 `c9234998` + lane13b `4782f072`（凭据驱动发现）**：G-259（`azure_sync.rs`：ResourceGraph→management deployments 分页（origin 守卫 nextLink）→数据面回退（Entra/API key）→deployment 收窄快照（未部署模型移除、catalog 事实保留）；`digitalocean_sync.rs`：browser-OAuth routers 清单→`router:*` 注入（token 变更清理/503 保留/过期跳过/5min 节奏）；凭据门控惰性+offline seed 测试模式（modal/copilot 先例）；gitlab 半边维持 G-8 ℹ️ 裁决）。**lane13b（审计发现 G-259 listing 残半边）**：四个动态 sync（copilot/modal/azure/DO）经 `dynamic_sync_snapshot` 骑上 `/api/model`+`/api/model/default` listing seam（与 bind 同源同序：customs/console 之上、catalog tier 替换之下）；azure 收窄对 listing 可见；offline 惰性；config custom 同 id 优先。附带修复 boot 尾部 await 时序（DO 凭据预检移入 deferred task）。
- **lane14 `5d2935a5`（TUI，+pty 门）**：G-229① oc-tui 消费臂（WS fold 消费 title_changed/foreground_process_changed→TerminalRecord 活标签（foreground 优先链）——G-229① 端到端闭环）；G-271 五子项（terminal 选择持久化移 run-loop outbox drain；select_worktree→`set_home_location(directory)`；DialogMcp needs_auth→DialogIntegration autoConnect 跳转（不打 mcp.connect）；`integration_form.rs` 完整 method.form 流（shaping/校验/when-guards/状态机，answer 骑 key/oauth POST 体，esc=CANCELLED）；session 搜索 150ms 防抖→服务端 `session.list{search}`）；G-273 四子项（at_mentioned→composer `@path#start-end` chip；每连接重解析 IDE 锁/env（缺失时轮询）；Zed contents Err→readFile 回退；cwd 门 exists()）。
- **lane15 `18692a77`（mini 后半+TUI 🟢，+pty 门）**：G-272①-⑨（RunPrompt parts+`<file>` 包裹+parts-aware POST；`!`shell 执行臂经 session.shell；每轮 switchAgent/switchModel；队列项 steer/queue/cancel/undo；ctrl+b session.background；SIGINT 两击退出+草稿清理；OSC 2 终端标题；verbosity 三档持久化 cli.json；TOOL_RULES view 门+structuredFallback）——⑩ 样式化 surface 登记有界偏离；G-287（① 生产 sink 装配 debug_assert；② update 对话框 confirm 焦点；④ terminal.copy cfg!(windows)；⑤ 崩溃屏 copy-report/issue URL/stack 捕获（PTY 钉定 ref 标题）；⑥ 模型选择器 Favorites/Recent+搜索期收藏优先；⑦ DialogOpen attention 排序+ses_ 直开；⑨ 行范围解析残边收口；⑩ frame-budget 性能债注释指针）；③ 登记成本裁决、⑧ 搁置（见登记节）；🧹 两处经查已由 `5cd2b94c` 修复（ALREADY_PRESENT）。
- **lane16 `ac2c03d3`（cli/config/fs）**：G-274（--server/--standalone 透传 mcp 族+debug agents/config（stub server 钉定 `mcp list --server URL` 真打 URL））；G-288 八子项（run 恒 attached（standalone 清扫取消废除）；import --directory 绝对化；pair --url 路径保留注释钉定；acp 失败 code/signal/MiB 细化；uninstall 不自删（curl 打印 rm 提示）；service cors 拒带 path origin（url.origin 比较）；UnexpectedStatus body 64KiB cap；with_part 乱序 ordinal gap-fill；duration_label 已由 lane15 处理）；G-281（严格 JSONC 解析器 `jsonc_parse.rs`（json5 宽松拒绝）；mcp timeouts PositiveInt（0→诊断）；WatchHub unbounded+sliding（5000/5000 风暴测试）；update_shell 进程锁）；G-282 六子项（providers localeCompare（oc-util 消费）；无效 KV selection 清理（kv_delete）；空白 run 一个 %20；Exa Date.parse 三类（ISO+offset/RFC-822/长月名）；grep 截断 UTF-16 码元（共享 tool_output helper）；--answer skip 键不写入）；G-278（BMP/AVIF 魔术字节双探测器+fs isText 控制字符比率+read 工具 MEDIA_MIMES 门（bmp/avif 检出但不摄取=ref parity finding）；find 不再排除 target/；`file://host` 拒绝+词法归一稳定 cachePath 键）。
- **lane17 `0d355cb3`（补漏：credential/oauth）**：G-283 五子项（oauth success 页 provider HTML 转义；key 提示两面优先 method.label；import 严格 `Credential.Entry` 整包拒绝（空 Key 降级废除；错误文案单行为有界偏离）；logout/switch 凭据连接过滤+"No stored credentials found"/"Select integration"/真实动词（env-auth 提示可触发）；wellknown scheme 大小写不敏感（HTTP:// 可发现；WHATWG shorthand 全量解析登记——无 url crate））。

## 登记与搁置（第十八轮）

- **G-287⑧ 搁置（唯一）**：OSC-8 超链接渲染——光栅能力已在（opentui-core frame.rs OSC-8 编码），端口现对 headless-golden parity 钉 off；开启即改渲染金帧=**oracle 重钉事件（规范 2.2#8 stop-and-escalate）**，按制度禁擅自重冻结，待用户批准重钉后开启。
  - 补记（2026-11-09 当日）：用户批准重钉后 lane18 落地——探针驱动启用使 headless profile 保持 caps=off，金帧字节级不变（0 帧），批准的重钉未用到；G-287⑧ 就此 ✅（commit `229d64b2`，差分 127 IDENTICAL 不变）。
- **G-289 ℹ️ / G-290 ℹ️ 正式入册**（条目内已附登记注）：单 bun guest 架构与 ctrl+c 清空关闭均为文档化决策，登记不改码。
- **G-272⑩ 样式化滚动回写 surface**：登记有界偏离（mini mono 直写惯例，G-68/G-134 同族；ref ≈1410 行 renderable 层）。
- **G-287③ 终端 pane 拖拽调宽**：登记成本裁决（persistence+pointer seam 基建在）。
  - 补记（2026-11-09 同日）：用户批准收口后 lane19 落地 ✅（`998bf2e4`）。
- **G-285② snowflake 401→刷新 retry**：缓置（需内建 retry-hook 脚手架；Authentication 现分类不可重试，oc-ai provider_error.rs:271-289）。
  - 补记（2026-11-09 同日）：用户批准收口后 lane19 落地 ✅（`29f9858f`，取证=ref retry-hook 形状，Authentication 分类未动）。
- **G-247 残余面**：`Bun.file/Bun.spawn/process/Function/eval` guest 可达（静态扫描方案①固有边界）；结构性隔离（无 import 能力域/深度 wrapper）登记为后续工程项。
- **Deferred minors**：MCP decode-failure 腿不发 tool.execute.after（ref tool.ts:130-137 发；g191 钉定的既有形状）；worktree.list 排序未对齐 ref（未立项面）；engine 不消费 OPENCODE_DISABLE_PROJECT_CONFIG（既有）；repo-wide `cargo fmt --check` 在 main 有 ~160 文件历史漂移（各 lane 仅 fmt 自身文件，勿全仓 fmt）；`node_modules` find-skip 无 ref 引据（保留+登记）。
- **flake 观察**：oc-pty `second_subscribe` 族多次 try-1 重试即过（gb10-lane10/lane16/lane17 三见）——入观察名单，复发三次以上再议白名单。补记（lane19）：`provider_refresh_engine_boot_loop_updates_and_publishes` 一次高负载 try-1 红（隔离 0.5s PASS、复跑 FLAKY→pass）——并入观察名单。

## 勘误/勘注（第十八轮）

- **G-283 漏派**：本条为十八轮 lane 计划唯一遗漏项（审计对账发现，补 lane17 全量落地）——多 lane 打包条目派发时需逐条对账（已入 ledger 流程）。
- **G-283 路径勘误**：oauth_page 实位于 `mcp/oauth_page.rs`（第十七轮登记 integration/ 有误）。
- **G-249 注**：过滤 ref-exact 落地，但 "latest" 视角被截断锚点遮蔽（发射面=provenance 边界）。
- **G-259 补面**：原登记「修法仿 modal_sync/copilot_sync 模式」落地后审计发现 listing 面未随 bind 收窄（「azure 展示全量」原文可观察面）——lane13b 补齐 listing seam，四个动态 sync 一并受益。

## 第十八轮判定

G-241..G-288 全部 ✅（含登记子项：G-270⑨⑩/G-286⑦/G-288③ 记录性钉定；G-272⑩ 有界；G-287③/G-285② 当日经用户批准补收口 ✅，见登记节补记）；G-289/G-290 维持 ℹ️（登记入册）；G-287⑧ 已于同日经用户批准后收口 ✅（探针驱动启用，重钉实际未用到，见登记节补记）。三条勘注残臂（G-229①/G-63②/G-234②）全部闭环。现存未关闭项=既有文档化决策族（G-12/G-16/G-17/G-28/G-32/G-38/G-39/G-40/G-49/G-161①/G-163/G-164/G-200②/G-211/G-212/G-238/G-239/G-240）+ 本轮新增登记/搁置项。**无上游阻挡项**（无 opentui-rust 未移植功能依赖；G-287⑧ 为流程性搁置非能力缺失）。

---

# 第十九轮（2026-11-09，17 通道逐行对比审查 + 4 通道缺口复核）

> 方法：17 条独立审查通道对当前树（十八轮收口后）做逐文件逐行对读（总报告 `PORT_PARITY_AUDIT_v2.0.24.md`，域报告 `docs/audit/L1..L17`）；随后 V-A（core/注册表/总线）、V-B（AI/provider）、V-C（server/CLI/TUI）、V-D（归类/低优先级）四条复核通道对全部缺口候选逐项双侧现行源码取证，与本册 G-1..G-290 及全部勘误去重。结论：**新立项 G-291..G-318（28 项：🔴×1 / 🟡×13 / 🟢×8 / ℹ️×6）**、既有条目补注 6 条、撤销初报 18 项（见文末勘误）、1 项跨通道冲突仲裁。新缺口集中在**工具注册键、事件通知时序、插件宿主契约、Copilot 头面、企业 TLS env、TUI 语法表**。

## 🔴 高

### G-291 ✅（🔴）工具注册表按 bare name 键控 → MCP 跨服务器同名工具互相覆盖（含 tool_input_repair 键失配子项）
- **2026-11-10 第二十一轮收口 ✅（gb11-laneA `e82f4037`）**：注册/执行器/移除/派发快照/权限与修复表全链改 `effective_name` 键控（MCP=`{server}_{tool}`）；②子项随主键修复自动对齐；model_request 自称 effective 的矛盾注释消除。测试：双 server 同名共存、不遮蔽 builtin、修复表命中。
- rust：`oc-core/src/location.rs:558-559`（`tools.lock().insert(tool.name.clone(), …)` 键=bare name）、`:624-625`（executor 同键）；MCP 桥 `mcp/registry.rs:1879-1887`（`ToolRegistration::new(name=bare tool.name)` + `register_executor(name…)` :1894-1904）。
- ref：`tool.ts:182-184` 以 `effectiveName(tool)`（MCP=`{server}_{tool}`）为键——前缀天然隔离同名工具与同名 builtin。
- 后果：两个 MCP server 各暴露 `search` 时后者覆盖前者的注册与 executor，`{srv1}_search` 路由到 srv2 的执行器（错误工具执行）；MCP bare name 还可覆盖同名 builtin；派发面 `model_request.rs:1024-1038` 靠 snapshot 的 effective→bare 映射，源表已丢行。G-242 修的是执行路径（PipelineExecutor/permission），不覆盖注册键。
- ②子项：`tool/tool_input_repair.rs:46` 用 bare 键表查 `event.tool`（=effectiveName，registry.rs:157/1897）→ MCP 与 `opencode_*` 命名空间工具的入参修复静默失效；ref `tool-input-repair.ts:29` 同键必命中。随主键修复自动对齐。
- 修法：`ToolRegistryState::register/register_executor/remove/executor` 统一改 `effective_name(namespace, &name)` 为键（或注册时展开 effective 键）。

## 🟡 中

### G-292 ✅ Bus durable 发布 notify 在聚合锁外（订阅者可见 seq 乱序）
- **2026-11-10 第二十一轮收口 ✅（gb11-laneA `e82f4037`）**：durable publish/publish_all/replay 的 notify 移入聚合锁存活区间（监听者唤醒全同步，锁下无 await）；模块头「锁内 notify」承诺兑现。
- rust `oc-core/src/bus.rs:697-708`（drop(guard)→drop(lock)→notify），replay `:1341-1352`、publish_all 逐事件 notify `:858` 同型；模块头自述"锁内 notify"与实现不符。
- ref `bus.ts:469-477` notify 在 `durableLocks.withLock` 内。同聚合并发发布时订阅者/SSE 可能先见 seq6 通知再见 seq5。G-207 只修时间戳。
- 修法：inner.notify 移回 guard 存活区间（listener 异步广播，不长期持锁）。

### G-293 ✅ 插件热重载先卸后装 + 代际激活模型缺失（同文件两半边）
- **2026-11-10 第二十一轮收口 ✅（gb11-laneD `66f8337b`）**：staged swap：新代在旧代仍活时加载、成功才原子换装、失败整体回滚旧代（keeps its place）；revision=import-graph 内容指纹，失败 revision 不重试、再变更才重试。
- ① rust `oc-plugin-host/src/supervisor.rs:1942-1951` 先 `clear_plugin`+`unload` 旧代→`:1955-1966` load 新代→失败仅 eprintln（旧注册已清空）；ref `supervisor.ts:70-86` 新代成功才替换（"keeps its place"）。G-222 只修触发面（三路触发+防抖）不修次序。
- ② rust 全文件无 revision 概念（generation 仅计数器 :1952，reload 全量卸装，无 (id,revision) 前缀 diff/失败修订不重试/fallback 恢复）；ref `plugin.ts:100-180` + `supervisor.ts:87-90`。
- 修法：先 load 成功后再切换；②随①移植 revision diff + fallback（或先登记 bounded）。

### G-294 ✅ MCP 后台 list-changed 观察 session 过期不重连
- **2026-11-10 第二十一轮收口 ✅（gb11-laneB `e8de0341`）**：client watch 增 `session_expired` 回调（typed 分类 + sessionId 在场门），registry 触发 stop+start recover，连接同一性护栏防与前台 replay 双重重启。
- rust `mcp/registry.rs:909-957` watch 仅 close/tools/prompts/resources 四回调，无 session-expired 腿（typed 判定 client.rs:155/499 仅前台路径消费 :1236/1341/1392/1415）；ref `mcp/index.ts:415` `onSessionExpired(() => fork(recover(name,…)))`（注释明言后台刷新靠此重连）。G-223 修的是前台判型+sessionId 门。
- 后果：长空闲远程 server 的后台推送见过期仅报错/掉线，状态陈旧直至下一次前台调用。
- 修法：client watch 增 `session_expired` 回调，registry 挂 replay_after_recovery 同款 stop+start。

### G-295 ✅ 插件 RPC events.emit 宿主通路缺失
- **2026-11-10 第二十一轮收口 ✅（gb11-laneD `66f8337b`）**：host.js `rpc.register` 返回 `events.emit`（声明名校验）→ hostRequest `rpc.event` → 宿主 RpcEventPort；oc-server 侧 EngineRpcEventPort 绑定 engine bus，按 boot location 作用域发布 `rpc.{id}.{event}` ephemeral（`rpc.*` 公开性测试钉定）。
- rust `host.js:1462-1494` `ctx.rpc.register` 仅返回 `{dispose}`，hostRequest 方法表无 `rpc.event*`（supervisor.rs:488 仅 rpc.register）；客户端订阅侧在（oc-client/rpc_typed.rs:222-247）但全链无发布方。
- ref `rpc.ts:91-103` register 返回 `{dispose, events:{emit}}`，emit 经 bus 发布 `rpc.{id}.{name}` ephemeral 事件。
- 注：第十七轮勘误 #6 撤销的是 SSE/bus 侧 `rpc.*` 指控（is_public/attach_event_tap 已在），与本条 ctx.rpc.events.emit 宿主通路不同面；G-2/G-64/G-147/G-148/G-122 均不含。
- 修法：host.js rpc.register 返回值补 events.emit（按 definition.events 校验后 hostRequest `rpc.event` → 宿主按 Location 发 bus ephemeral）。

### G-296 ✅ 插件 RPC schema 校验与声明式 error 编解码无任何一层执行
- **2026-11-10 第二十一轮收口 ✅（gb11-laneD `66f8337b`）**：oc-plugin-host 内嵌 JSON-Schema 子集校验器：input 违规→`rpc.invalid_input`（handler 不跑）、output 违规→`rpc.invalid_output`、`ctx.error` 未声明类型按 ref encodeError die 路径归一 `rpc.internal`。
- rust 宿主 `oc-plugin-host/src/lib.rs:856-921` 仅门禁 id/method（三态错误）；guest `host.js:726-763` handleRpcCall 直接 `handler(input ?? {}, ctx)`，`ctx.error(type,…)` 接受**任意** type 字符串（未声明 error 也作 typed failure 透传），无 output encode；host.js:1468-1470 自认"schema validation stays a JS-side concern"但无一层执行。
- ref `rpc.ts:115-141,200-267`：宿主 parse input→`rpc.invalid_input`、encode output→`rpc.invalid_output`、encodeError 未声明 error.type 直接 die、decodeError 编解码 data。
- 风险：wire 契约松弛（非法输入直达插件、未声明错误漏成 typed failure）。G-122 只修 oc-client 定义层。
- 修法：host.js rpc.register 保留 method.input/output/errors 的 JSON-Schema 面，调用前后 parse/encode，未声明 error type 归一 rpc.internal。

### G-297 ✅ project worktree.resolved 行插入与事件非原子
- **2026-11-10 第二十一轮收口 ✅（gb11-laneA `e82f4037`）**：行插入经 `PublishOptions::commit` 与 `worktree.resolved` 事件同事务（ref project.ts:184-199 语义）；NoEvents/测试生产者保留独立插入路径。
- rust `oc-core/src/project.rs:1354-1364`（存量行短路→先 insert）→`:1381-1388`（后 publish）；ref `project.ts:184-199` insert 在 durable 事件 `commit` 回调内（同事务）。
- 后果：崩溃落在 insert 与 publish 之间后，重启被存量行短路，`worktree.resolved`（及 adopted 重挂靠）永不补发。
- 修法：publish 传 commit 回调把 insert 挪进事务（rust bus 已支持 `PublishOptions::commit`，bus.rs:684）。

### G-298 ✅ Copilot 主车道 X-Interaction-Type / X-Interaction-Id 未注入
- **2026-11-10 第二十一轮收口 ✅（gb11-laneE `bf5b2e4e`）**：copilot 三车道（chat/responses/messages）注入 `X-Interaction-Type`（kind/child 矩阵）+ `X-Interaction-Id`（affinity ?? sessionID）+ 非 conversation-agent 时 `x-initiator: agent`；header seam 改请求级胜出。有界偏离：kind 在 oc-ai 层由会话缝头推导（Compaction 经 ContentPart、Title 不可区分归 agent 环路）。
- ref `github-copilot.ts:272-280` model.request hook 注入 `X-Interaction-Type = interactionType(kind, child)` 与 `X-Interaction-Id = SessionAffinity.get(session) ?? sessionID`，且 `interaction !== "conversation-agent"` 时强制 `x-initiator: agent`。
- rust `oc-ai/…/copilot.rs:59-66` `interaction_type()` 已移植但**零生产调用**（仅 tests.rs:42-45）；`copilot_client_headers`(:38-44)/`copilot_request_headers`(:101-111) 均无 Interaction 头；`X-Interaction-Id` 仅 utility-title（:840）；model_request.rs:767-778 只注入通用 `x-session-affinity`/`X-Session-Id`，非 Copilot 语义。
- 影响：chat/responses/messages 三车道缺 session affinity 与 interaction 词汇，可能影响 Copilot 侧限流归类/免费 utility 配额判定。
- 修法：copilot route headers seam 补两头（复用 `session_affinity()` + `interaction_type()`），非 agent 交互升格 x-initiator。

### G-299 ✅ `NODE_EXTRA_CA_CERTS` 不被 oc-http 识别
- **2026-11-10 第二十一轮收口 ✅（gb11-laneE `bf5b2e4e`）**：oc-http 读 `NODE_EXTRA_CA_CERTS`（Node 单 PEM 路径语义，出处核验注释）：系统根（rustls-native-certs）+ 额外 PEM 加性信任，OnceLock 一次读取，坏文件 warn-忽略；本地 TLS 桩双向测试。
- rust `oc-http/src/lib.rs:17-60` platform-verifier→webpki 回退，全链无该 env（全仓 grep 零命中）；ref `websocket-constructor.ts:66` Node TLS 继承（注释明言该 env 增量生效）。
- 影响：企业 MITM/代理 CA 环境下 rust 全部 HTTPS（模型 API/websearch/webfetch）握手失败。G-26 只统一双栈。
- 修法：`platform_tls_config` 读 NODE_EXTRA_CA_CERTS（Node 冒号分隔多路径语义）把 PEM 追加进 RootCertStore。

### G-300 ✅ 客户端 SSE 空闲看门狗缺失（半开连接检测）
- **2026-11-10 第二十一轮收口 ✅（gb11-laneF `7bc929c1`）**：2s connect+首帧窗、首帧必须 `server.connected`、每读 45s 空闲截止（Event stream stalled）→ 统一重连路径，Last-Event-ID 保留；桩服务器补握手帧、重连 overlay 按连接状态族锚定。
- ref `packages/client/src/solid/connection.ts:32,35,81,91,106-107` 三件套：connectTimeout 2s、首帧必须 `server.connected`、45s idleTimeout（"Event stream stalled"）。
- rust `oc-tui/src/data.rs:1067-1161` 重连环：订阅无 2s 截止、首帧无类型断言（is_server_connected 仅状态栏用）、`:1151` `stream.next_json().await` 无超时——对端静默挂死时无限阻塞，仅靠 TCP/OS 兜底。Last-Event-ID/G-197① 与本项无关。
- 修法：next_json 包 `tokio::time::timeout(45s)` 触发重连 + subscribe 2s deadline + 首帧 server.connected 校验。

### G-301 ✅ run 附件 MIME 探测为魔数+5 扩展子集
- **2026-11-10 第二十一轮收口 ✅（gb11-laneG `488bbb8f`）**：detect_mime 改共享 oc-util mime 表 + 魔数嗅探：svg→`image/svg+xml` data URL、bmp/tiff/ico/avif/heic→image/*。有界偏离：bmp/tiff/heic 扩展名补在 oc-cli 本地表（oc-util 当时不在 lane scope），待 hoist。
- rust `oc-cli/src/run/mod.rs:113-142`；ref `run.ts:202` FSUtil.mimeType 全表（fs-util.ts:234-236）。
- 真实差：`.svg` rust 走 utf8 文本分支（内联 `<file>` 文本块）vs ref `image/svg+xml` data URL；`.bmp/.tiff/.ico/.avif/.heic` ref 得 image/*、rust 得 octet-stream。G-278 只修 fs/read 面。
- 修法：detect_mime 扩用 G-278 双探测器同表。

### G-302 ✅ `filetype_for_path` 扩展名表偏窄（语法在位仍退化纯文本）
- **2026-11-10 第二十一轮收口 ✅（gb11-laneF `7bc929c1`）**：扩展名表 ~70 项（镜像 ref filetype.ts 的已注册语言映射），覆盖测试断言每个已注册 filetype 至少一个扩展可达（javascript/markdown-inline 例外钉定）。
- rust `oc-tui/src/components/syntax.rs:399-422` 仅 ~24 扩展，而 `oc-syntax/queries.rs` 注册表（42 filetype）已含对应语法；ref `util/filetype.ts`（~115 项）。
- 受损面：`.md .sh/.zsh .lua .xml .scala .ex/.exs .hs .ml .fs/.fsx .clj .nix .r .tf .jl .vim`——diff 查看器（diff_viewer_solid.rs:741）整族高亮缺失。
- 修法：按注册表已含 filetype 反向补映射（纯表扩，零风险）。

### G-303 ✅ read 工具 AGENTS.md 发现绕过 Files seam
- **2026-11-10 第二十一轮收口 ✅（gb11-laneB `e8de0341`）**：AGENTS.md 发现腿从宿主 FS 迁 Files seam（type_following 探针，符号链接文件计入），nearest-first 与错误行为不变。
- rust `oc-core/src/tool/read.rs:931-941` `std::path::parent()`+`is_file()`（宿主 FS 词法路径；读腿已迁 seam，此半边未迁）；ref `read.ts:89-100` 经 `fs.resolve`/`fs.up`（Environment seam，remote/workspace 环境同样发现）。
- 影响：非本地 Environment 下 AGENTS.md 注入静默丢失（本地等价）。G-1"read 已迁 seam"只覆盖读腿。
- 修法：发现腿改走 Files seam 的 up/resolve，保留 nearest-first 序。

### G-304 ✅ shell 服务层杂项打包（① panic 向量 ②③ 语义漂移）
- **2026-11-10 第二十一轮收口 ✅（gb11-laneB `e8de0341`）**：①`env_utf8()`（vars_os 跳过非 UTF-8）替换全部 env::vars() panic 向量（登记 1 处实修 3 处）；②空 cwd 明确拒绝 spawn（服务层无 location 上下文，文档化）；③shells_from 全局去重保序。
- ① 🟡 rust `shell.rs:564-565` `std::env::vars().collect()` 遇非 UTF-8 环境变量 **panic**（std 文档行为，比丢弃更糟）。修法：`vars_os()` + lossy/跳过记日志。
- ② 🟢 `shell.rs:571` cwd `unwrap_or_default()`→""（ref `shell.ts:265` `?? location.directory`；调用层已兜底= G-55 HTTP 面，服务 API 语义漂移）。
- ③ 🟢 `shell/select.rs:206-210` `dedup()` 仅相邻去重 vs ref `new Set()` 全局（/etc/shells 实际无重复，等价面 99.9%）。

## 🟢 低

### G-305 ✅ credential legacy 钉住仅 NULL、漏 active=false（G-268 残边）
- **2026-11-10 第二十一轮收口 ✅（gb11-laneG `488bbb8f`）**：钉住条件放宽为「active 非真即钉」（NULL 或 false），CredentialStore 与 HTTP handler 本地 SQL 双路一致。
- ref `credential.ts:129` `!current.active` 对 false 与 null 都为真（顶行 active=false 也钉回 true）；rust 双路均只钉 NULL：`oc-core/integration/credential.rs:139-144`、`oc-server/handlers/credential.rs:198-207`。G-268 落地文案即 "legacy NULL pin"。
- 发生条件：integration 全部行显式 active=false（低频边角）。修法：钉住条件放宽为「active 非真即钉」。

### G-306 ✅ Anthropic options optionalNull 族显式 null 被拒 + null 键丢弃
- **2026-11-10 第二十一轮收口 ✅（gb11-laneE `bf5b2e4e`）**：NullableString 三态（Absent/Null/Value）：user_id、inference_geo/inferenceGeo（双拼 ?? 合并）、container 内 id/skills 显式 null 合法且 body 重建原样回发；service_tier 经核对 ref 为 Schema.optional（非 optionalNull）保持拒 null——登记面收窄为 ref 忠实。
- ref `anthropic-messages.ts:303/:309-310/:352-353`（metadata.user_id、container.id/.skills、inference_geo/inferenceGeo）optionalNull——显式 null 合法且保留（:1086 原样回传 wire）。
- rust `anthropic_messages.rs:238-244` `want_string` 对 `Value::Null` 抛 InvalidRequest（"Expected a string, received null"），作用于 :449（service_tier 顺带）、:456（user_id）、:480-486（container/skills/inference_geo）；:458-460 重建丢 user_id 键为同根因下游。
- 修法：该组字段改 nullable 解码（null→Ok(None)），body 重建保留显式 null 键。

### G-307 ✅ patch `*** Move to:` 新目录不建父目录
- **2026-11-10 第二十一轮收口 ✅（gb11-laneB `e8de0341`）**：move-write 腿改 `Files::write`（建父目录=ref local.ts:50-55 mkdir -p 语义）；普通写腿建父行为本就正确并补钉定测试；误导注释清理（R20 备选②「并入 G-1 登记」不再需要）。
- rust `tool/patch.rs:430-433` `write_file`（G-1 条目自述"不隐式建父目录"为 lane9a 机制注）；ref `environment/local.ts:50-55` files.write 隐式 mkdir -p（G-73 勘误确认 ref Bun.write 同样建父目录）。
- "move 到新目录 ref 成功、rust 失败"的 parity 断裂此前从未作为缺口登记（G-1 文本只记录了 rust 侧机制）。
- 修法：move-write 腿先 `create_dir_all` 再写；或并入 G-1 明确登记有界决策。

### G-308 ✅ normalizeImages 与 truncate 顺序颠倒（跨通道冲突仲裁成立）
- **2026-11-10 第二十一轮收口 ✅（gb11-laneB `e8de0341`）**：step.rs 换序为 normalize→truncate（ref tool.ts:150 先于 step.ts:120-122），错误注释重写；图片省略注记参与行预算的翻转用例钉定。
- rust `session/runner/step.rs:716-747` 先 :718 `truncate` 后 :744 `normalize_tool_content`（:738-742 注释对 ref 顺序的解读有误）。
- ref：`tool.ts:113-156` `executeTool` 返回前即 `normalizeImages`（:150，在 after-hook 内容上），`step.ts:120-122` 随后才 `flatMap(toolOutput.truncate)`——**ref = normalize → truncate**。
- 影响：内联图片参与 50KiB 预算/剔除时截断点与行数漂移（低危）。修法：step.rs 换序并修正注释。

### G-309 ✅ Anthropic 4 断点预算丢弃仅计数无告警
- **2026-11-10 第二十一轮收口 ✅（gb11-laneE `bf5b2e4e`）**：lowered>0 时输出 ref 同形 warning（消息进纯函数 `breakpoint_drop_warning` 钉定；oc-ai 无日志 facade，沿 oc-http eprintln 惯例）。
- rust `anthropic_messages.rs:523,544` 计数 `dropped`，全文件无 warn/log、`body_from_request` 不读；ref `anthropic-messages.ts:1060-1064` `dropped>0` 时 `Effect.logWarning`。行为（丢弃）一致，缺观测。
- 修法：lower 完成后输出一行 warning（含 cap 数）。

### G-310 ✅ websearch 同意流程超时预算分叉 + tavily 专用文案
- **2026-11-10 第二十一轮收口 ✅（gb11-laneB `e8de0341`）**：单个 60s 预算包住整个同意流程（双表单共享，choose 路径不再可达 120s）；tavily 超时映射专用文案 "Tavily web search request timed out"。
- rust `websearch.rs:22,508,584` 两张表单各 60s（choose 路径可达 120s）；ref `websearch.ts:141-144` 一个 "1 minute" 包整个 permit 流程。
- tavily：超时值 25s 已对齐（websearch_providers.rs:52），但错误走泛化 `to_string()`（:470）；ref 专用文案 "Tavily web search request timed out"（tavily.ts:71-74）。
- 修法：外层 60s 包两表单；tavily 分支映射专用文案。

### G-311 ✅ CORS preflight `Vary` 集合不完整（两侧互不相同）
- **2026-11-10 第二十一轮收口 ✅（gb11-laneG `488bbb8f`）**：preflight `Vary` 补全为 `Origin, Access-Control-Request-Method, Access-Control-Request-Headers`（规范正确超集，顺带优于 ref）。
- rust `middleware.rs:129` 仅 `Vary: Origin`（:148-165 回显 method/headers）；ref 经 effect 库仅设 `Vary: Access-Control-Request-Headers`（effect#5935）。回显型 allow-* 按 spec 必须配 `Vary: Access-Control-Request-Method, Access-Control-Request-Headers`。
- 影响：浏览器预检缓存 key 含 method+headers 不受影响；仅共享代理/CDN 可能复用窄回显。修法一行（顺带优于 ref）。

### G-312 ✅ 语法高亮缓存满 128 `clear()` 全清 + engine.rs 注释失实
- **2026-11-10 第二十一轮收口 ✅（gb11-laneF `7bc929c1`）**：双层缓存（oc-tui syntax + oc-syntax engine）`clear()` 改 FIFO 逐出最旧；engine.rs 文档与实现对齐。
- rust `syntax.rs:382-384` 与 `oc-syntax/engine.rs:255-257` 双层均全清；engine.rs:15 文档自述 "drops the oldest" 与实现不符。影响仅满载时一次性重解析抖动。
- 修法：LRU 或逐出最旧插入（并修注释）。

## ℹ️ 登记

### G-313 ℹ️ 插件 npm 包启动期自动安装不移植（`opencode plugin add` 拥有安装）
- rust `oc-plugin-host/src/resolve.rs:19-21` 明言"a spec without an install is a recorded failure, not an install attempt"（离线解析缓存 :28）；ref `plugin/module.ts:88-92` load 时 `install!==false` 即 `npm.add`——同一份 config `plugins:["pkg@x"]` 在 ref 首启即装、rust 报 load 失败。G-29（TUI 管理页）/G-17②（反向 rust-extra）均未覆盖此方向。
- 登记：设计决策；最小改进=解析失败 wire error 提示 `opencode plugin add <target>`。

### G-314 ℹ️ custom/local/console provider 大小写不敏感兜底（rust-extra 宽容）
- rust `engine/resolver.rs:245-254/:394-397/:364/:406/:445` exact-first + `eq_ignore_ascii_case` 兜底（注释自证有意宽容，容忍 beta.3.1 `Own/model` 类缺陷）；ref `model.ts:255-257` 精确 Map（报 provider.no-route）。纯宽容度差（rust 多收配置），无失败面。登记不改码。

### G-315 ℹ️ devtools 缺 `channel==="local"` 默认开分支
- rust `store.rs:11093-11095` 仅读 `debug.devtools`（全 TUI 无 channel 概念）；ref `app.tsx:457` `?? app.channel === "local"`。开发者默认态差异，不影响发行用户。登记（或引 channel 常数补默认）。

### G-316 ℹ️ ssh-askpass prompt 参数位 argv[1]（对 ref 编译形态缺陷的纠偏）
- rust `ssh_askpass.rs:19-25` `args().skip(1)`；ref `ssh-askpass.ts` `argv.slice(2)` 在 `bun <entry> <prompt>` 直跑形态取 prompt，但 OpenSSH exec 约定为 `[程序, prompt]`——**ref 编译为单文件二进制后 slice(2) 取空串（真实缺陷在 ref 侧）**，rust argv[1] 符合约定。登记有意纠偏，不改码。

### G-317 ℹ️ 有界超时制度打包登记（remove 10s / TOOL_JOIN 5s / UNWIND 60s）
- `TOOL_JOIN_TIMEOUT=5s`（step.rs:43-44）、`UNWIND_TIMEOUT=60s`（execution.rs:55-56）设计依据在 `docs/runner-concurrency.md:180,264,313,419`；登记册仅 G-79（remove 10s）与 :1694 顺带提及 UNWIND，TOOL_JOIN 未入册。统一登记：ref 无界等待在 wedge 场景由 rust 有界超时收口（更激进中断策略的文档化代价）。

### G-318 ℹ️ repository-cache 刷新时间戳落文件而非 KV（G-231 备注）
- rust `reference/repository_cache.rs:110-153` `FileRefreshStore` → `<repos>/.repository-cache.json`（注释自认 mirroring ref shared KV）；ref `repository-cache.ts:157,163` `kv.set`。仅 ref/rust 二进制共享同一 data 目录时可观察（互不可见对方节流戳）。登记有界偏离，不改码。

## 归类与补注（不另立项）

- **G-5 补注**：ref state.ts:41-114 的 batch/shutdown/inherit/reconcile/notify 通用原语随 lazy+通知模型一并不移植（design-g5-state-transform.md Divergences #1-4 已覆盖对应语义面，bounded 端无消费）。
- **G-17 子注追加（③ boot 残键丢弃护盾）**：`app.rs:7924,8646` BOOT_DISCARD_WINDOW=300ms 防"上次退出残键"（有意新增，可能吞极快首键）。
- **G-17 备注（util/artifact.ts + util/binary.ts）**：artifact 唯一消费方 packages/app（Web TUI 整包不移植族）；binary.ts 为导出孤岛（无非测试导入方）。
- **G-23 补注（copilot 编码裁决复认）**：V-B 复核确认 G-167 车道 beta 头完整（见勘误 #1），form 编码保持的既有裁决不受影响。
- **G-28 补注**：插件运行时单轨 bun-only（缺 bun=子系统禁用+单条日志+oc_util::bunman 托管下载兜底）；node 备轨不移植。
- **G-29 补注**：wire 上 `outdated/updating` 恒 None（plugins.rs:273-274,319-320）、`PluginUpdate.changes` 热流缺（context/update.rs 仅 24h outdated 缓存）——消费面即 G-29 bounded 的 TUI 管理页与 G-62 CLI 输出。
- **G-231 备注**：刷新戳存储位置分歧见 G-318。

## 勘误（第十九轮经复核撤销的初报，勿再立项）

1. **Copilot anthropic 车道缺 anthropic-beta 头** — 不实：`facade.rs:908-935` CopilotMessages 经 `anthropic_messages.rs:2861` BetaHeadersTransport，:1657-1680/:1603 send 时按 body 计算（恒含 `interleaved-thinking-2025-05-14`），tests.rs:1482 钉定。G-167 车道头注入完整。
2. **requestMetadata x-initiator 判定简化** — 行为等价：messages 分支逐字等价；chat/responses 的 user-only-tool_result 形状在各自 lowering 下不会以 user 消息出现；vision Media 判定覆盖三种 wire 形状。
3. **openai_chat overlay 显式 null 被 retain 误删** — 不构成：全部 overlay（groq/minimax/moonshot/openrouter/venice/zai/alibaba）对 null 先过滤再 insert，从不写顶层 `Value::Null`；ref overlay 用 undefined 同样不产出显式 null。
4. **SSE headerTimeout/chunkTimeout 看门狗缺失** — 不实：`oc-ai/route/transport.rs:250-270`（TimeoutBudget）、:310-336（header 预算）、:356-400（chunk idle+total deadline，typed 错误）完整在位且已接主发送路径（route/client.rs:419,432；media/route.rs:528-656），有行为测试。
5. **Route.compact 通用 lane 缺失** — 不实：ref 的 Compact/Trigger 操作全仓仅 responses 两处实现；rust responses_compaction.rs（:83/:167/:395）+ responses_checkpoint.rs（:217/:306）经 openai_responses.rs:586-596 暴露、runner/mod.rs:305 触发。唯一实现族全覆盖，仅架构摆放差异。
6. **`Model.small` 家族偏好缺失** — 不实：`context.rs:1497-1516` 与 ref model.ts:266-278 逐字对齐（同过滤+同偏好序）。
7. **LocationModelResolver 双层 fallback 为新增语义** — 不成立：与 ref 合并视图（server/routes.ts:130-135 Config.configured 层叠）产出同构，忠实实现。
8. **models.dev options.file 无接线** — 不实：OPENCODE_MODELS_PATH/URL 已接线（server_process_env.rs:76-77,106-127→EngineOptions，坏文件降级 embedded）；`--models-file` 旗标两侧均无。
9. **pairing/ticket 淘汰序分歧** — 不可观察：全部条目同 TTL、消费即删、无读刷新 → 最近到期≈LRU。
10. **valid_namespace 校验缺口** — 动态面已有完整门：registrations.rs:166-194 逐字覆盖 ref tool.ts:304-326（:782 拒绝+同款日志）；registry.rs:61-70 缺口仅作用编译期固定 builtin。
11. **matchesVersion 谓词缺失** — ref 函数形态全仓零生产调用（死 API 面），现行等价。
12. **serve env 旋钮未透传** — 等价：rust 由 boot/config 层直读 env（server_process_env.rs:70-78、config.rs:393、variable.rs:272），与 ref server-process.ts:110-152 逐项对应。
13. **插件发现仅 config 条目（不扫 plugins/ 目录）** — 失实：resolve.rs:124-172 自动扫描 `plugin/plugins` 目录（.ts/.js/子目录/symlink，Add 先于 config 条目），与 ref source-directory.ts 逐语义对齐，oc-server/plugins.rs:1055 与 oc-tui/plugin_boot.rs:199 已接线。
14. **shell 超时升级 pid 复用窗口** — 已被 2.4#1 safe_signal 制度覆盖：signal_process_group（shell.rs:809 起）= 活 leader 钉住 + /proc 扫描 + 逐成员 pgrp 校验；理论残余为制度已接受。
15. **PS/bash legacy 扫描 keep 列表与 command_parameter 丢失** — 行为等价：bash tree-sitter 语法从不产出 `command_name_expr` 节点；`command_elements` 内层循环两侧逐分支一致。
16. **pty 默认尺寸 (80,24) 与 node-pty 隐式默认不同** — 相同，无分歧。
17. **WS chunkTimeout 未透传** — 两侧对齐：ref chunkTimeout 仅 HTTP 传输消费（transport/http.ts:126），WS 用独立 idleTimeout；rust 对等（HTTP route/transport.rs:262,391；WS websocket.rs:536-567,847-862）。初报"runner/mod.rs"路径系笔误。
18. **normalizeImages/truncate 顺序（V-D 撤销裁定）** — 跨通道冲突经 orchestrator 仲裁**推翻撤销**：ref tool.ts:150 normalizeImages 在 executeTool 内、先于 step.ts:122 truncate → 立项 G-308。

## 第十九轮判定汇总

17 通道初报缺口候选约 57 项：**确认立项 28**（🔴×1：G-291；🟡×13：G-292..G-304；🟢×8：G-305..G-312；ℹ️×6：G-313..G-318）；**归类既有/补注 7**（G-5/G-17×2/G-23/G-28/G-29/G-231 + safe_signal 制度）；**撤销 18**（见勘误，含 4 项与 G-167/G-278 等已修状态冲突后经现行代码证伪的指控）；**跨通道冲突仲裁 1**（G-308 成立）。优先级建议：G-291（含②子项，同 lane）> G-292 ≈ G-293 ≈ G-296 ≈ G-297（时序/契约）> G-294 ≈ G-295 ≈ G-298 ≈ G-299 ≈ G-300 > G-301 ≈ G-302 ≈ G-303 ≈ G-304① > 🟢 族 > ℹ️ 登记族。复核记录：`docs/audit/R19-V{A,B,C,D}-*.md`。

---

# 第二十轮复核（2026-11-10，未闭合缺口重验）

> 对第十九轮立项的 22 项可行动未闭合项（G-291..G-312 全部 ❌/⚠️）在 HEAD=967bedba 重验（W-A core/注册表/总线/插件、W-B AI/TLS/客户端、W-C 工具/TUI/server 三通道；记录 `docs/audit/R20-W{A,B,C}-*.md`）。该波次更新（c36d2556 全仓 fmt、clippy -D warnings 清理、CI 修复）均为格式/清理，未触及语义面。**裁定：22/22 STILL_OPEN**；下列证据锚点为 fmt/布局漂移后的刷新值。

| 条目 | 裁定 | 刷新锚点（HEAD=967bedba） |
|---|---|---|
| G-291 🔴 注册键 bare | STILL_OPEN | location.rs:558-559/:624/:635；mcp/registry.rs:1871-1887（tag_tool :1539-1541 bare，namespace 仅字段 :1884）；②tool_input_repair.rs:46 |
| G-292 notify 锁外 | STILL_OPEN | bus.rs:699-709（drop guard→drop lock→notify）；publish_all :858-868；replay :1341-1352 |
| G-293 热重载次序+revision | STILL_OPEN | supervisor.rs:1958-1988（先 clear+unload :1964-1973，失败 :2003-2005 仅 eprintln）；无 revision（:1974 仅计数器） |
| G-294 后台过期不重连 | STILL_OPEN | mcp/registry.rs:909-957（Changed 仅三回调+close 腿 :944-956） |
| G-295 rpc.events.emit 通路 | STILL_OPEN | host/host.js:1462-1493（:1488-1492 仅 {dispose}）；方法表 supervisor.rs:422 / lib.rs:878 |
| G-296 RPC schema/error 编解码 | STILL_OPEN | host/host.js:726-758（:741 直调无 parse/encode；ctx.error 任意 type :735-749）；lib.rs:876-915 仅 type 字符串三分 |
| G-297 worktree 事件非原子 | STILL_OPEN | project.rs:1354-1389（发布 :1215 走 default()，未用 bus.rs:194 PublishOptions::commit——低成本收口） |
| G-298 Copilot Interaction 头 | STILL_OPEN | protocols/copilot.rs:59-67（零生产调用）；:38-46/:100-113（无 Interaction 头）；:831-832；通用注入 model_request.rs:776 非 Copilot 语义 |
| G-299 NODE_EXTRA_CA_CERTS | STILL_OPEN | 全仓 grep 零命中；oc-http/src/lib.rs:34-54 无 env 读取 |
| G-300 SSE 空闲看门狗 | STILL_OPEN | oc-tui/src/data.rs:1093-1097（无 2s deadline）、:1158（next_json 裸 await 无 45s）；文件头 :1-54 自述无看门狗 |
| G-301 run 附件 MIME | STILL_OPEN | oc-cli/src/run/mod.rs:118-152（5 魔数+5 扩展；svg/bmp/tiff/ico/avif/heic 零处理） |
| G-302 filetype 表窄 | STILL_OPEN | components/syntax.rs:393-416（~19 臂；.md/.sh/.lua/.xml/.scala/.ex/.hs/.nix 仍缺） |
| G-303 read AGENTS.md seam | STILL_OPEN | tool/read.rs:925-955（:933 parent()/:945 is_file()/:948 宿主上溯） |
| G-304 shell 杂项打包 | STILL_OPEN（①恶化） | ①**tool/shell.rs:311 与 :664 两处** `env::vars()` panic 向量（登记时 1 处）；②:349 cwd unwrap_or_default；③shell/select.rs:205 dedup 相邻 |
| G-305 credential 钉住 | STILL_OPEN | integration/credential.rs:136-141；oc-server/handlers/credential.rs:195-208（双路仅钉 NULL） |
| G-306 anthropic optionalNull | STILL_OPEN（container 边角收窄） | anthropic_messages.rs:238-244 want_string 拒 null；:448-449/:456-461（:458-460 丢键）/:464-486；container 现整体 Option\<Value\> 透传（:226/:1815-1816）——内部 id/skills null 仍拒 |
| G-307 patch move 父目录 | STILL_OPEN（已代码自认有界） | tool/patch.rs:429-433 注释明示 "writes WITHOUT creating parents"——建议并入 G-1 作有界决策销项（行为 vs ref 仍断裂） |
| G-308 normalize/truncate 顺序 | STILL_OPEN | runner/step.rs:716-747（:718 truncate→:744 normalize；:738-742 误读注释原样） |
| G-309 断点丢弃无告警 | STILL_OPEN | anthropic_messages.rs:523,544（仅计数，warn!/log:: 零命中） |
| G-310 websearch 超时+tavily | STILL_OPEN | tool/websearch.rs:22,508,584；websearch_providers.rs:75/:89（泛化 to_string） |
| G-311 CORS Vary | STILL_OPEN | middleware.rs:130（仅 Vary: Origin；回显 :141-167） |
| G-312 高亮缓存全清 | STILL_OPEN | components/syntax.rs:376-377 + oc-syntax/engine.rs:256-257（文档 :15 仍失实） |

## 状态勘注（本轮新增事实）

- **G-291**：派发面 `session/runner/model_request.rs:646-647` 注释自称 "tool table is keyed by effectiveName (tool.ts:188)" 并按 effective_name 建 snapshot 索引——与源表 bare 键实现（location.rs:559）**直接矛盾**；修复时须一并消除该注释/实现分叉。mcp/registry.rs:1858-1863 注释自认 "bare `tool.name` + namespace 字段" 为 v2 形状，但与 ref tool.ts:182-184 effective 键语义不符。
- **G-304①**：panic 向量从 1 处扩为 2 处（tool/shell.rs:311 新增）——`std::env::vars()` 在非 UTF-8 环境变量下 panic，建议随修复一并清两处。
- **G-306**：container 字段改为整体 `Option<Value>` 透传属 PARTIAL 性边角收窄（:226/:1815-1816），登记主根因（want_string 拒 null + user_id 键丢弃）未动。
- **G-307**：偏离已由代码注释显式自认（patch.rs:429-433 "a missing parent stays an error"）——按第十九轮修法备选②，可并入 G-1 明确登记为有界决策销项（无需改码），或按备选①补 create_dir_all。

## 第二十轮判定

G-291..G-312 全部维持未闭合（22/22）；本轮更新波次（fmt/clippy/CI）无一项触及语义修复。修复优先序维持第十九轮建议不变，另确认两条低成本收口：G-297（`PublishOptions::commit` 基础设施已在 bus.rs:194）与 G-302（纯表扩）；G-304① 因恶化建议提前。ℹ️ 族（G-313..G-318）与既有文档化决策族维持不变。复核记录：`docs/audit/R20-W{A,B,C}-*.md`。

## 搁置项条件核查（2026-11-10 追加，X-A/X-B 双通道）

> 对登记册全部「搁置/缓置/待条件/Deferred minors」项在 HEAD=967bedba 核查修复/启用条件是否具备（记录 `docs/audit/R21-X{A,B}-*.md`）。已同日收口的三项（G-287⑧ OSC-8=229d64b2、G-287③ pane 拖拽=998bf2e4、G-285② snowflake retry=29f9858f）不在列。

| 搁置项 | 条件 | 裁定 | 说明 |
|---|---|---|---|
| **Deferred minor d：repo-wide fmt 漂移** | 全仓 fmt | **已清账** | c36d2556 `chore(fmt): repo-wide cargo fmt`（~160 文件漂移已消，本项闭环） |
| **Deferred minor a：MCP decode-failure 腿不发 after hook** | 无阻塞 | **CONDITION_MET** | registry.rs:269-275 decodeInput 失败直接 return Err 未走 emit_after（ref tool.ts:130-137 对 decode 失败也发 after）；修法=复用 leaf 失败臂（:305-317）同型塑形，10-15 行+1 hook 测试，半小时级 |
| **Deferred minor b：worktree.list 排序** | 无阻塞 | **CONDITION_MET** | rust handlers/project.rs:177 仅 `ORDER BY directory`；ref worktree.ts:140 `orderBy(desc(time_created), asc(directory))`。修法=SQL 双序（确认 time_created 列，无则加迁移）+钉定测试，~20 行，1 小时级 |
| **Deferred minor c：OPENCODE_DISABLE_PROJECT_CONFIG 残面** | 无阻塞 | **CONDITION_MET（残面收窄）** | ref 真实存在（server-process.ts:113-116→config.project:false）；rust 主体已在（server_process_env.rs:86-98 configure_loader，boot.rs 等五处调用）——残面=engine 内部 Loader::new 未过 configure_loader（engine/mod.rs:1108、skills_url.rs:977/1032/1076、provider_catalog/overlay.rs:203、config.rs:768），统一包一层即收口，半 lane |
| **Deferred minor e：node_modules find-skip** | ref 引据 | **无据（维持超收）** | ref ripgrep.ts:193-218 无 node_modules 显式跳（靠 gitignore/hidden；protected.ts 无条目）；rust fs.rs:454 显式跳=超收收紧（G-203 后 gitignore 已认，有 gitignore 处冗余）。登记"可放开为 gitignore-only"，非必改 |
| **G-128 title-shimmer 静止态** | rename-pending 入 store | **CONDITION_MET（实质已落地）** | store.rs:2067-2068/:4422-4428 在位、app.rs:9144-9152 接线、sidebar.rs:435-469 动效本体已渲染；仅 sidebar_solid.rs:15-28 stale 注释待清（🧹 文档项，见 G-128 勘注标闭） |
| **G-272⑩ mini 样式化滚动回写 surface** | renderable 层 | **CONDITION_MET（基建已备）** | opentui-core `ScrollbackSurface`+renderables 全家（text/code/markdown/text_table）在位（renderer/scrollback.rs:123+）；唯一闸口=`ExternalOutputCommit.lines: Vec<String>` 纯文本载荷（scrollback.rs:85-92）。修法=styled commit 变体+mini 按.kind 挂 renderable，~150+500-800 行含差分重钉，**3-5 天中等 lane** |
| **G-239 插件 panel/PanelHost** | 消费方出现 | **PARTIAL（维持搁置）** | panel API 仍无（plugin_slots.rs:46 明列 unmounted）；但 slot 投影+dock 几何+共享树 mount 基建已可复用（~300-450 行可落）。无 shipping plugin 认领 → 维持搁置合理，需求出现即可立项 |
| **G-211 split-footer 整族** | 消费面出现 | **OBSOLETE（条目过期）** | 收官注已宣布全量移植并消费（ADR-0019 真引擎）；旧条目已标 OBSOLETE 指向收官注（本轮勘注） |
| **G-200② terminalHandoff** | CLI 渲染面 | **维持（补勘注）** | 同进程事实使移交架构可行，但替换窗口仍无渲染器存在、ref 所解决问题不出现；见登记处 2026-11-10 勘注 |
| **G-230 die 分支** | 14 内建 Output 声明补齐 | **NOT_YET** | registry.rs:159-163 内建硬编码 output_schema:None（已声明 0/14）；ref 13 文件全声明 Output（read.ts:27,46 等）。若做=BuiltinTool 加声明字段+逐工具移植 Output→JSON Schema（复用 G-100 校验器），1-2 天中等 lane——**前置投资，做完即解锁 die 分支** |
| **G-247 残余（Bun.file/spawn/process/Function/eval）** | 结构性隔离 | **NOT_YET** | host.js:2688 AsyncFunction 全量 guest 全局执行，无 wrapper/Proxy；最小路径已定位=构造处插影参 Bun/process+权限门 Proxy+eval/Function 置换，双侧同规则测试 |
| **G-289 per-Location 插件实例** | supervisor 架构改造 | **NOT_YET** | 仍单 guest（supervisor.rs:2134-2140 "the one-guest approximation" 注释在） |
| **G-240 sha256 钉扎** | 捆绑安装路径引入 | **NOT_YET（维持）** | oc-pty 仍无 include_bytes/install/bundle；解析序 OPENCODE_PTY_BIN→exe-sibling→PATH 不变，无可比面 |
| **G-13/R-01b csharp/kotlin/swift 门控** | 体积预算复核 | **NOT_YET（维持发布裁决）** | CI 无二进制体积记录/门（grep 零命中）；无新体积数据，条件无变化 |

### 汇总与建议

- **条件已具备、可立即立项**（按成本升序）：Deferred minor a（半小时）→ b（1 小时）→ c（半 lane）→ G-128 stale 注释清理（<30 分钟，🧹）；**中等**：G-272⑩（3-5 天）；**前置投资型**：G-230（先补 14 内建 Output 声明 1-2 天，解锁 die 分支）。
- **维持搁置（条件未变/无消费方）**：G-247 残余、G-289、G-240、G-13/R-01b、G-239、G-200②。
- **清账**：repo-wide fmt（c36d2556）；G-211 旧条目 OBSOLETE；G-128 勘注标闭。
- 复核记录：`docs/audit/R21-X{A,B}-*.md`。

---

# 第三类悬空项补立（2026-11-10，G-319..G-329）

> 把登记册各轮「归类注记/Deferred minors/bounded 裁剪」中散记但**从未立项、也无既有条目实质覆盖**的差异正式入册。其中 G-319/G-320 的现状为 2026-11-10 当日 grep 复证；G-321..G-324 即十八轮 Deferred minors 四件（条件已核查具备）；G-328/G-329 为既有 bounded 裁剪面的转正登记（决策不变，只是获得条目号便于盯守）。

## 🟡 中

### G-319 ✅ mantle `validate_config` 死校验：bearer-without-key fail-fast 永不触发
- **2026-11-10 第二十一轮收口 ✅（gb11-laneE `bf5b2e4e`）**：`bedrock_mantle_messages_route` 构造首步调用 `validate_config`：bearer 缺 key → ProviderConfigurationError fail-fast（不再静默 SigV4 回退）。
- rust：`crates/oc-ai/src/protocols/bedrock/mantle_messages.rs:199` `pub fn validate_config` **全仓零调用**（2026-11-10 grep 复证；第四轮勘注即发现，从未认领）。
- ref：`providers/amazon-bedrock-mantle.ts` 的 configure 守卫——bearer 模式缺 api key 时 `ProviderConfigurationError` fail-fast。
- 后果：bearer 模式缺 key 时 rust 静默 SigV4 回退，用户看到的是困惑的 AWS 凭据错误而非清晰配置错误。
- 修法：mantle 路由构造处调用 `validate_config`（校验本体已在，纯接线，半小时级）。

### G-320 ✅ 根外 skill 源目录热重载缺失
- **2026-11-10 第二十一轮收口 ✅（gb11-laneC `9d85f709`）**：skill 源根（compat 根 + URL staging）由 loader 枚举进 watch plan 递归 watch；签名（path/size/mtime）变化驱动既有 reload→engine 失效级联。有界偏离：URL 源 watch staging 总根（slot 惰性创建）。
- rust：`config/watch.rs` 仅 watch 配置根内文件（2026-11-10 grep 复证：无任何 skill 源引用）；G-199 并入扫描的 compat roots（`~/.claude/skills`、`~/.agents/skills`）与 URL skill staging 目录均不在 watch 面。
- ref：对每个 skill 源目录 watch（第十五轮归类注记，supervisor/config sources 机制）。
- 后果：编辑 compat-root/根外 skill 不触发重扫，变更直到其它原因触发 config reload 才生效（可能无限期陈旧）。
- 修法：watch 注册面扩到 skill sources 清单（复用 G-281③ unbounded+sliding 通道），半小时~1 lane 视覆盖面。

### G-321 ✅ MCP 工具 decode 失败腿不发 `tool.execute.after`（原 Deferred minor a）
- **2026-11-10 第二十一轮收口 ✅（gb11-laneB `e8de0341`）**：decodeInput 失败腿经 `emit_after(HookToolOutcome::Error)` 同型塑形后返回 Err（g191 当年钉定的分歧形状按 ref tool.ts:130-137 修正）。
- rust：`tool/registry.rs:269-275` decodeInput 失败直接 `return Err`，未走 `emit_after`（leaf 失败臂 :305-317 已走）；g191 落地时刻意钉定的形状。
- ref：`tool.ts:130-137` 对含 decode 失败在内的全部 Tool.Error 发 after（decodeInput 在 runtime.ts:30 于 execute 内被 catchTag 捕获）。
- 影响：订阅 execute.after 的审计/观测插件看不到 decode 失败的执行。
- 修法：Err 改经 `emit_after(…, HookToolOutcome::Error{…})` 同型塑形（10-15 行 + 1 hook 测试）。

## 🟢 低

### G-322 ✅ worktree.list 排序未对齐 ref（原 Deferred minor b）
- **2026-11-10 第二十一轮收口 ✅（gb11-laneC `9d85f709`）**：SQL 双序 `ORDER BY time_created DESC, directory ASC`（列已存在，无迁移），HTTP wire 排序钉定。
- rust：`oc-server/handlers/project.rs:177` 仅 `ORDER BY directory`；ref `worktree.ts:140` `orderBy(desc(time_created), asc(directory))`。修法：SQL 双序（确认 time_created 列，无则加迁移）+ 排序钉定测试（~20 行）。

### G-323 ✅ OPENCODE_DISABLE_PROJECT_CONFIG engine 内部残面（原 Deferred minor c）
- **2026-11-10 第二十一轮收口 ✅（gb11-laneC `9d85f709`）**：oc-core 新增共享 `configure_loader`（双环境变量名 OR 语义维持），五处引擎内部 `Loader::new` 全部经过；oc-server 侧委托同一函数。
- 主体已在（`server_process_env.rs:86-98` configure_loader；ref 语义 server-process.ts:113-116→config.project:false）；残面=engine 内部 `Loader::new` 未过 configure_loader 的 5 点：`engine/mod.rs:1108`、`skills_url.rs:977/1032/1076`、`provider_catalog/overlay.rs:203`、`config.rs:768`。修法：统一包 configure_loader（半 lane）。

### G-324 ✅ fs.find 显式跳 node_modules 为无据超收（原 Deferred minor e）
- **2026-11-10 第二十一轮收口 ✅（gb11-laneC `9d85f709`）**：裁决落地：保留防御性 node_modules 跳过（方向=少结果，安全），fs.rs 注释钉定 ref ripgrep.ts:193-218 无显式跳过的事实与决策依据。文档化决策收口，无行为变化。
- rust：`oc-server/handlers/fs.rs:454` 显式跳 node_modules；ref `ripgrep.ts:193-218` 无显式跳（仅 `--glob=!**/.git/**` + hidden 过滤，靠 gitignore）。G-203 后 rust 已认 gitignore——有 gitignore 处该跳过冗余；**无 gitignore 的仓库 rust 比 ref 少结果**（超收收紧方向）。修法可选：放开为 gitignore-only 对齐 ref；维持现状亦可（防御性，方向安全）。

### G-325 ✅ DbError::Pragma 字符串错误隧道
- **2026-11-10 第二十一轮收口 ✅（gb11-laneA `e82f4037`）**：`DbError::BusInvalid` 类型化变体取代 `\u{0}bus-invalid\u{1}` Pragma 字符串隧道，lift_db_error 按类型匹配，行为零变化。
- `oc-core/src/bus.rs:493-511`：pragma 失败经字符串隧道传播（第十轮明确"不单独立项"）；无现实触发路径。随 bus 错误类型化重构顺手收口，不单开 lane。

### G-327 ✅ title-shimmer 缺 arrival-wipe 相位
- **2026-11-10 第二十一轮收口 ✅（gb11-laneF `7bc929c1`）**：到达一次性 450ms coast-eased wipe（前缘缓动+4 列羽化+旧字形右存留，sustain dim 240ms 衰减；ref title-shimmer 数学），store→sidebar 接线；oracle 门通过（金帧零变化，无重钉）。有界偏离：逐帧无状态建模代替 ref 光栅 previous 缓冲、smootherstep 近似 blend。
- rust `components/sidebar.rs:435-469` 已有 sustained dim + 1200ms 扫光（G-128 勘注已标闭）；ref `title-shimmer.tsx` 另有**到达时一次性 wipe** 相位，rust 未实现。严格对齐 +40 行（X-B 核查估值）。纯视觉，随任意 TUI lane 顺手。

## ℹ️ 登记

### G-326 ℹ️ session.diff 注册为公开 ephemeral 但无发布点（条件性盯守）
- `oc-core/src/bus/registry.rs:150-152` 仍注册（第十二轮归类：is_public=true 但全仓无发布点，SSE 实际不转发——与 ref 暴露面一致）。**盯守条件：若未来开始发布 `session.diff` 事件，必须先将其并入 PRIVATE_TYPES**，否则工作区 diff 经 SSE 泄露给订阅者。本条为该条件的常驻盯守位。

### G-328 ℹ️ TAB_SPINNERS 裁剪面转正入册（原散记于 G-49/G-143）
- 现状仅复刻 dots 样式（G-49a 钉定 40ms 帧数学）；ref 另有 **arcs/quadrants/line 三样式、4 种未读标记、`tabs.indicators` 设置的消费**——G-143 收口时裁定"维持裁剪"。本条将裁剪面正式入册（决策不变，获得条目号便于将来按需恢复）。

### G-329 ℹ️ mini demo 剧本 6/13 转正入册（原 G-36 bounded）
- rust `mini/demo.rs` 6 场景 vs ref `demo.ts` 13 场景（缺 motion/shell/write/edit/patch/subagent/question 七类）。演示与回归覆盖面裁剪，决策维持；如需扩充按 ref 剧本逐场景补 fixture。

## 补立判定汇总

11 项入册：🟡×3（G-319/G-320/G-321）· 🟢×5（G-322..G-325/G-327）· ℹ️×3（G-326/G-328/G-329）。其中 G-319（纯接线半小时）、G-321（10-15 行）、G-322（~20 行）为最低成本三项，建议与搁置项条件核查节确认的 G-128 stale 注释清理（🧹）打包为一个半日 lane；G-320/G-323 各半 lane；G-324/G-325/G-327 顺手项；ℹ️ 族仅盯守。自此登记册「散记不立项」差异全部有号可查——剩余未闭合面 = G-291..G-312（22 项，第二十轮重验）+ 本节 11 项 + 文档化决策族（第四类，见各 ℹ️ 条目）。

---

# 第四类决策面补立（2026-11-10，G-330..G-333）

> G-13/R-01b、G-239、G-28 三条第四类决策，其**可行动/条件面**此前只散落在裁决注与归类备注中、无独立条目盯守（体例同 G-328/G-329 的转正做法）。决策本身不变，本节为面立目。

### G-330 🧹（同日用户裁决移除）R-01b 体积护栏机械化——加入后即日拆除，勿再立项
- 2026-11-10 第二十一轮曾机械化落地（gb11-laneH `3e3233ff`，size-guard workflow：体积记录 + 67.6 MiB 硬门 + 95% 预警，孤儿数字出处补记）。门首次运行即测得 **101.89 MiB**（10-16 M-retire 64.98 MiB 数据点后三周无人测量，tui-full-convergence/ADR-0019 全量引擎搬运为主要增量——护栏按设计抓到真实漂移）。
- **同日用户裁决：不设二进制体积上限**——workflow 整体移除，R-01b 的 67.6 MiB 裁决上限就此作废（孤儿数字销项）。G-13（被砍语法回归）重评前提回落为"需时手工测量"，无 CI 门盯守；后续轮次不得再把体积护栏机械化立项（如用户重新要求除外）。
- 裁决史：R-01b（beta.5，ROADMAP.md:37）size guardrail 触发（72.7 MiB > 67.6 cap）→ 砍 swift/c-sharp/kotlin 三语法（~+8.7 MiB）→ RA-07 改为非默认 `lang-*` feature。cap 数值推导无书面记录（孤儿数字，效力依赖裁决先例）。
- 缺口：护栏纯手工——CI（ci.yml/release.yml）无任何二进制体积记录/门（2026-11-10 X-A 核查 grep 零命中）；M-retire 后 release 68,144,160 B ≈ 64.98 MiB 的数据点只活在 ROADMAP 日志里。**后果**：G-13 的重评条件（体积复核）无法执行；后续任何 lane 都可能在无感知下把默认二进制推过 67.6 顶。
- 修法：CI 增 release 构建体积记录 + 软门（超 67.6 MiB 报警/硬门按裁决），一条 workflow 的事。立本项目体：把「护栏存在但不可执行」从隐性状态转为盯守项。

### G-331 ✅ G-239 激活规格入册：session.panel slot 与 PanelInput API 的最小实现路径
- 现状（2026-11-10 X-B 核查）：`oc-plugin-host/ui.rs` 无 panel 符号；`plugin_slots.rs:46` session.panel 明列 deliberate unmounted，`tests/plugin_ui_slot.rs:146-150` 断言其保持 unmounted；无 PanelInput（focus/close，ref context.ts:168-178）对应面。
- 激活规格（X-B 评估，基建已部分具备）：① `session.panel` 入 MOUNTED_SLOT_PATHS；② dock 区 renderable 承载 claim 输出（mini split-footer dock 几何 + 共享树 mount 通道 plugin_boot.rs:376-381 现成）；③ PanelInput focus()/close() 以宿主有界命令对实现（render_slot_claim 同款通道）；④ 键路由 KeyPipeline 分流。规模 ~300-450 行。
- 触发条件：出现认领该 slot 的 shipping plugin。未触发前维持 G-239 豁免（无消费方不建面）。
- **2026-11-12 第二十五轮增补收口 ✅（gb13-laneI `34550de6`）**：用户下令激活，规格全量落地——①`session.panel` 入 MOUNTED_SLOT_PATHS+豁免测试翻转；②**右侧 pane**（对齐 ref session-frame：rightPane 优先级 panel>terminal>sidebar、与 terminal 互斥、宽度>80 docked 于 session_pane_width、窄幅 fullscreen 接管隐藏会话列、路由切换关闭），pane 体经 plugin_page_solid 承载有界 claim 输出；③guest 面 `ui.panel.open`（deferred reply 返回 ref 布尔路由判定）/`close`（owner 范围释放）/`current()`（宿主推送 tui.panel 视图），PanelInput props `{name,sessionID,width,presentation,focused}` 驱动指纹重物化；④KeyPipeline `panel_focus_live` 臂（对话框先行、escape 关闭、插件键位层可达、未绑定消费）+鼠标命中聚焦。PTY `pty_session_panel.rs`（docked/窄幅 fullscreen/聚焦/关闭）+ CI 双 filter 更新。有界偏离：claim 内容为 G-28 有界行形状（非活 TSX）；focus/toggleFullscreen 闭包不可跨线（focus=鼠标/键宿主面，toggleFullscreen v1 省略）；面板行点击 v1 未接 transcript 命中层。

### G-332 ✅ G-28 族成员转正①：插件 markdown 代码块渲染器注册链缺失
- ref：`packages/plugin/src/tui/api.tsx:168-176` + `plugin/markdown.ts`——插件可注册自定义语言代码块渲染器。
- rust：全 crates 无对应（第十九轮 L16 通道发现，当时仅归类 G-28 备注，从未立目）。**用户可见后果**：依赖该 API 的第三方插件在 rust 宿主静默失效（无报错）。
- 维持 G-28 生态 breaking change 声明族；本条转正入册供发布说明引用与将来按需恢复（恢复路径：host.js markdown 注册面 + oc-tui markdown.rs 渲染分派缝）。
- **2026-11-12 第二十五轮增补收口 ✅（gb13-laneH `c9c7afb6`/`4b93504d`/`78cbda37`）**：用户下令落地——guest 面 `ctx.markdown.registerCodeBlockRenderer`（空语言/插件内重复报错文案 ref 逐字、dispose 幂等、G-293 快照/复辟/退役全镜像）→ supervisor `markdown.renderer.register/unregister` 臂 → registrations `markdown_renderers` 表（Clone 保持）→ facade `render_markdown_block`（硬超时）→ oc-tui `plugin_markdown.rs` 分派表（`info_string_to_filetype` 归一键控；**插件表先于 latex/mermaid 内建咨询**=ref last-wins 遮蔽语义）+ 内容哈希缓存（表指纹+syntax_hash 盐；miss→本帧默认路径+`offload_bounded` 异步渲染→缓存+重绘；in-flight 去重防每帧重派）+ `BlockSpec::CodeBlock` 臂接入。空表=默认路径字节恒等（goldens 未动全绿）。有界偏离：注册期键归一在消费侧（"ts"+"typescript" 同插件不报重复、错误文案用原始语言名）；未归一语言静默死信（ref 报错）；行形状为有界 {title,rows}（title 并入首行）；transcript 行 v1 不可点击；缓存 miss 首帧为默认渲染（ref 同帧同步替换）。评审 F1（websearchExecutors 快照行误删）经 fix round 修复复原。

### G-333 ✅ G-28 族成员转正②：插件运行时单轨 bun-only（node 备轨不移植）
- ref：`plugin/src/source.bun.ts` / `source.node.ts` 双运行时实现。
- rust：仅 bun 子进程（`oc-plugin-host/lib.rs:1-7`、`bun.rs:1-13`）；缺 bun 时=插件子系统禁用+单条日志，另有 oc_util::bunman 托管钉定 1.3.10 下载兜底（非纯降级，2026-11-09 V-D 核查）。**用户可见后果**：无 bun 且不可下载环境（离线/受限网络）插件功能整体不可用——ref 的 node 路径可跑。
- 维持 G-28 族裁决；转正入册供发布说明引用。
- **2026-11-12 第二十五轮增补收口 ✅（gb13-laneJ `06232e6c`）**：用户下令落地——`GuestRuntime{Bun,Node}`+解析链 `OC_PLUGIN_BUN`（显式终决，同 bunman 语义）→ **`OC_PLUGIN_NODE`（显式强制 node，即使 bun 在场——实现者裁决修正：置于 bun 链前，否则 override 在有 bun 机器上恒空转；双 override 并设 bun 胜出）** → bun 链（cache//usr/local/PATH/~/.bun）→ PATH node → bunman 下载 → 禁用；spawn 按轨选二进制（node 加 `--disable-warning=ExperimentalWarning`，ref nodeExecArgv 姿态）；sandbox 物化 `package.json {"type":"module"}`（双运行时确定性 ESM）；host.js `BunShim`（模块作用域、**不上 globalThis**）覆盖生产面全部 3 个 Bun API（`file().text()`→node:fs、`Transpiler.scanImports`→有界正则扫描返回字符串、`version`→node-<ver>）。真 node 22.22 本机 e2e×3（hello_world/hot_reload/globalThis.Bun undefined）+解析单测 6 行裁决表+全量 bun 套件不动全绿。有界偏离：node 轨插件代码不可加载无扩展名相对导入与 `.tsx` 条目（无 registerHooks/JSX 移植）；热重载 import-graph 为正则保真（失败按不可解析降级）；node 下 `globalThis.Bun===undefined`（沙箱面收窄=更安全）。评审注记：坏 `OC_PLUGIN_BUN` 显式终决后若仍有插件条目会走 bunman 下载兜底（既有 bunman 行为，模块文档已澄清）。

## 第四类补立判定

4 项入册：⚠️×1（G-330——唯一可行动项，一条 CI workflow）；ℹ️×3（G-331/G-332/G-333——规格/面转正，触发条件与恢复路径已记）。G-13/G-239/G-28 本体状态不变（✅/ℹ️/ℹ️），新条目分别锚定其重评基建、激活规格与族成员清单。内联勘注已同步三条本体条目。



---

# 第二十一轮 burndown 收口（2026-11-10，gap-burndown-11）

> 第二十轮重验确认未闭合的 G-291..G-312（22 项）+ 第三类补立可行动项（G-319..G-323/G-325/G-327 代码修复 + G-324 决策落地）+ G-330 体积护栏机械化 + G-128 🧹 残注清理，经 lanes A-H 串行落地并 fast-forward 合并 main。每 lane 独立 worktree（`.worktrees/gb11-laneX`，合并后 target 与 worktree 即时回收、分支已清）、dev-gate 单入口（root 编辑 → eternal130 门跑，chown 规则全程遵守）、nextest 未过滤证据、每 lane workspace 合并门复跑。**无上游阻挡项（无 opentui-rust 未移植功能依赖）、无搁置项**。

## 逐 lane 落地清单

- **laneA `e82f4037`（oc-core 注册键/总线/项目）**：G-291（🔴：ToolRegistryState 注册/执行器/移除/executor 查找全链改 `effective_name` 键控；MCP 桥 `tool_name(server,tool)` 同键；派发快照、权限、CodeMode 路径解析、plugin_apply 消费面逐一核对；②tool_input_repair 随主键修复命中；「自称 effective」的矛盾注释消除；双 server 同名共存/不遮蔽 builtin/修复表命中三测）+ G-292（durable/publish_all/replay notify 移入聚合锁内存活区间，并发发布者 seq 单调测试）+ G-325（`DbError::BusInvalid` 类型化）+ G-297（worktree.resolved 插入经 `PublishOptions::commit` 与事件同事务；NoEvents 生产者保留独立插入）。
- **laneB `e8de0341`（oc-core tools/mcp/runner）**：G-294（watch 增 `session_expired` 腿 → stop+start recover + 连接同一性护栏）+ G-321（decode 失败腿发 `tool.execute.after`）+ G-303（AGENTS.md 发现腿迁 Files seam）+ G-304（`env_utf8` 三处防 panic/空 cwd 拒 spawn/select 全局去重）+ G-307（move-write 建父目录=ref mkdir -p）+ G-308（normalize→truncate 换序）+ G-310（同意流程单 60s 预算 + tavily 专用文案）。
- **laneC `9d85f709`（config/engine/server）**：G-320（skill 源根热重载：loader 枚举进 watch plan、签名驱动既有 reload 级联）+ G-323（共享 `configure_loader`，五处 `Loader::new` 全过 OPENCODE_DISABLE_PROJECT_CONFIG）+ G-324（决策落地：保留防御性跳过+注释钉定）+ G-322（worktree.list 双序）。
- **laneD `66f8337b`（plugin-host + oc-server 接线）**：G-293（staged swap：新代成功才换装、失败回滚旧代 keeps-its-place；revision=import-graph 指纹、失败 revision 不重试）+ G-295（`rpc.register` events.emit → hostRequest `rpc.event` → RpcEventPort → oc-server EngineRpcEventPort 绑 engine bus 按 boot location 发布 `rpc.{id}.{event}` ephemeral；`rpc.*` 公开性测试钉定）+ G-296（内嵌 JSON-Schema 子集校验器：invalid_input/invalid_output/未声明 error 归一 rpc.internal；三处旧测试按 ref 形状更新）。
- **laneE `bf5b2e4e`（oc-ai + oc-http）**：G-298（copilot 三车道 Interaction 头注入 + header seam 请求级胜出）+ G-306（optionalNull 三态族；service_tier 经核对 ref 非 optionalNull 保持拒 null）+ G-309（断点丢弃 warning）+ G-319（mantle validate_config 接线 fail-fast）+ G-299（NODE_EXTRA_CA_CERTS 加性信任：系统根+额外 PEM、OnceLock、坏文件 warn-忽略）。
- **laneF `7bc929c1`（oc-tui/oc-syntax）**：G-300（SSE 看门狗三件套：2s connect+首帧窗/首帧必须 server.connected/45s 逐读截止；桩服务器补握手帧；重连 overlay 按状态族锚定防闪烁）+ G-302（扩展名表 ~70 项 + 全注册 filetype 覆盖测试）+ G-312（双层缓存 FIFO 逐出）+ G-327（到达 wipe 相位；oracle 门通过零重钉）+ G-128🧹（sidebar_solid 模块头更新）。
- **laneG `488bbb8f`（oc-server/oc-cli/oc-core 凭证）**：G-305（钉住条件放宽为 active 非真即钉，双路）+ G-311（preflight Vary 补全，规范正确超集）+ G-301（run 附件 MIME 走共享表+魔数；svg→data URL）。
- **laneH `3e3233ff`（CI；主检出直接提交，无 crate 面）**：G-330（size-guard workflow：路径过滤触发、体积入 summary、>67.6 MiB 硬失败+95% 预警、孤儿数字出处补记；G-13 重评前提基建就位）。

## 证据与 flake 记录

- 合并门日志（每 lane）：`.test-logs/workspace-*-gb11lane{A..G}-merge*.log` —— A 5640/5640；B 5651/5651；C 5658/5658（文件名实测 `workspace-20261110-141418`）；D 5674/5674（merge2，0 FAIL）；E 5689+e1（见下）；F 5697+composer/e1 双白名单；G 5702/5702（zen_oauth TRY-1 自愈）。终局主检出复验见下节。
- **e1_cells_paste_8k**：laneE worktree 构建中合并门与 base 提交（66f8337b）基线均红（`e1_cells_baseline-20261110-163830`）、同树主检出构建 PASS（`e1_canary-main-*-gb11.log`）——R16「逐构建翻转噪声」结论再证；按白名单+证据链放行。laneF 构建内隔离重跑 PASS（`retry2-20261110-181623`）。
- **composer_tabs_walk**：laneF 合并门 TRY2 败、隔离重跑 PASS（`retry1-20261110-181538`）——白名单放行。
- **oc-pty `subscribe_connections_end_at_stream_end`**：laneF 合并门 TRY3 PASS（retry=2 已毕业家族）。
- **新观察**：`zen_oauth_flow_round_trip_via_routes`（laneG 门内 TRY-1 败重试过，单次目击）——入观察名单，复发三次再议。
- 流程注记：一次 lane 报告的合并门日志文件名与盘上不符（时间戳笔误），已按 2.2#3 以 `ls -t` 实测文件核验后放行；本轮起 lane 报告的日志路径一律以盘上为准复核。

## 有界偏离（第二十一轮新增，均已代码注释或落地注记）

- G-291：测试 fixture 注册顺序与真实引擎相反（键冲突才是受测面）；`hooked_names` 闭包同义 reshape 单独成提交（绕白名单 canary 的构建布局敏感，语义零变化）。
- G-293：revision 为内容指纹（ref 为模块/身份 revision），diff/不重试语义等价；失败代保留 Active 功能 + status=Failed（ref 同形）。
- G-295：单 guest 架构下事件按 boot location 作用域发布（PLUG-D07 单位置近似）；事件 data 不做 schema 校验（ref EventData 透传）。
- G-296：声明 error 的 data 在解码边界校验（ref 在编码侧），可观察结果一致；Effect-Schema 骨架对象宽松放行（文档化）。
- G-298：request kind 在 oc-ai 层由会话缝头推导（Title 不可区分归 agent 环路；utility-title 自带词汇已在）。
- G-301：bmp/tiff/heic 扩展名补在 oc-cli 本地表（后续可 hoist 进 oc_util::mime）。
- G-304①：跳过非 UTF-8 条目不带 debug 日志（oc-core 无日志 facade），语义完整。
- G-306：service_tier 保持拒 null（ref 为 Schema.optional，登记面收窄为 ref 忠实而非缺口）。
- G-320：URL 源 watch staging 总根（slot 惰性创建，同步侧无法 await 网络拉取）。
- G-327：逐帧无状态建模 + smootherstep 近似（视觉数学等价，金帧零变化）。
- G-330：以 host x86_64-unknown-linux-gnu release 构建为 shipped linux 工件的规范化代理（OC_RELEASE_TAG 体积影响可忽略）；oc-pty 不在裁决范围不计。

## 第二十一轮判定

G-291..G-312 全部 ✅；G-319/G-320/G-321/G-322/G-323/G-325/G-327 ✅（代码修复）；G-324 ✅（文档化决策落地：保留防御性跳过）；G-330 同日修订——护栏落地后经**用户裁决整体移除**（不设体积上限，R-01b 上限作废，见 G-330 勘注；laneH 提交 `3e3233ff` 为历史沿革）；G-128 🧹 清理完毕。G-313..G-318/G-326/G-328/G-329/G-331..G-333 维持 ℹ️ 登记（决策/规格/盯守项，触发条件未变）。现存未关闭项=既有文档化决策族（G-12/G-16/G-17/G-28/G-32/G-38/G-39/G-40/G-49/G-161①/G-163/G-164/G-200②/G-211(已 OBSOLETE)/G-212/G-238/G-239/G-240/G-289/G-290 + 上列 ℹ️）。R20 搁置项条件核查中的 G-272⑩（样式化滚动回写 surface，3-5 天中等 lane）与 G-230 die 分支（先补 14 内建 Output 声明 1-2 天）维持登记不自动立项——两者在登记册中均为已裁决的 bounded/范围外项，非未闭合缺口；条件已具备，按需再立项。**无上游阻挡项。**

---

# 第二十二轮（2026-11-10，27 通道逐行全量对比 + P1/FINAL 双门复核）

> 对 ref v2.0.24（/opt/opencode-ref）与当前树做第四轮独立全量逐行对读：27 条域 lane（oc-core×12 / oc-ai×5 / oc-tui×4 / server·client·cli·acp·plugin-host·schema-util×6），逐行读码、双侧 file:line 证据、不采信既有审查/parity 文档；P1 门（12 份 C/T 报告）+ FINAL 门（15 份 A/S 报告）两道 oracle 一致性门裁定 11 处交叉点、抽查 11/11 成立、**0 处未裁定矛盾**。两门内部编号 G-01..G-63 经去重/归并/撤销后映射为正式编号 **G-334..G-383（50 项）**。原始报告：`docs/audit/port-parity-line-review/`（REPORT.md / TABLE.md + lanes/ 32 份）。

## 🔴 高

### G-334 ✅ CodeMode 沙箱逃逸残余面具体化（G-247 残余 + 新证据 `__rawFetch`）
- **2026-11-11 第二十三轮收口 ✅（gb12-lane5 86165976）**：`__rawFetch` 全局键消除（原生 fetch 仅存 wrapper 闭包+WeakMap）；eval/Function/AsyncFunction 置换抛错 Proxy（含 prototype.constructor 逃逸面，19-accept 语料零合法依赖）；Bun/process 求值作用域影子参数。全局 Bun/process 面 Bun 自身钉死（non-writable）/换 globalThis.process 断 Bun stdio——有界残差归 G-247 结构隔离后续项。
- rust：`oc-plugin-host/src/host/host.js:2833-2856` guest 求值前仅换装 console/fetch/timers 三面；**:2841 把原生 fetch 挂到 `globalThis.__rawFetch`**（换装后仍可直连）；:2863 AsyncFunction 在 guest 全局执行；:104-139/:2666-2668 结构上限（MAX_STRING/ARRAY）**仅桥接层生效**，guest 内部可自造超大对象；静态扫描只拦 import()/require token——`eval("require(…)")`/`(0,eval)`/`new Function` 全部可绕。
- ref：interpreter.ts 结构性无进程面。
- 后果：Bun.file/Bun.spawn/process/env/`__rawFetch` guest 全可达——G-247 登记的"静态扫描固有边界"残余被本轮证据具体化且加重（raw fetch 残留为全局键）。
- 修法：求值期全局白名单 Proxy + 删 :2841 `__rawFetch` + eval/Function 置换；最小步=先删 `__rawFetch` 一行（P1 门建议）。
- 出处：cmp-C8 / P1-G01+G05（门抽查 ✅）。

### G-335 ✅ 非持久 PTY reader 逐 chunk lossy UTF-8
- **2026-11-11 第二十三轮收口 ✅（gb12-lane4 35bfd4b1）**：Utf8StreamDecoder 携带缓冲增量解码（最长合法前缀发射、确定非法即替换、EOF lossy 冲洗）；逐字节读 mock 端到端钉定。
- rust：`oc-core/src/pty.rs:623-638`（:631）reader 线程对每次 ≤8192B read 结果直接 `String::from_utf8_lossy`——多字节序列跨 read 边界即损坏（CJK/emoji 乱码）；persistent_pty 全程字节缓冲无此问题。
- ref：node-pty 解码层按流缓冲。
- 修法：字节缓冲/增量解码器（P1 门建议"pty 字节缓冲化"）。
- 出处：cmp-C7a / P1-G02（门抽查 ✅）。

### G-336 ✅ 中断后 hosted 工具多发持久 `tool.failed(result-missing)`
- **2026-11-11 第二十三轮收口 ✅（gb12-lane1 afea43cd）**：RESULT_MISSING 清扫门补 `!stream_interrupted`；中断路径 hosted 工具由 §9 tools_interrupted 收口，不再误落持久 tool.failed(result-missing)。
- rust：`oc-core/src/session/runner/step.rs:557` RESULT_MISSING 清扫门只查 `stream_error.is_none() && !provider_failed`，未排除 :482 已独立存在的 `stream_interrupted` 标志——中断路径把正常收尾的 hosted 工具误落失败并持久化。
- ref：step.ts:220 以 `Exit.isSuccess` 判定，中断不算失败。
- 修法：门补 `&& !stream_interrupted`。
- 出处：cmp-C2a / P1-G03（门抽查 ✅）。

### G-337 ✅ WebSocketSeams（experimental.ws.*）构建后零消费
- **2026-11-11 第二十三轮收口 ✅（gb12-lane3 a4799e1e）**：seams 经 `SessionChannels::bind_with_seams` 骑上会话 WS 通道：handshake 在 affinity 键前重写 url/headers（重写即重开）、send 在 driver.create 后变换出帧、receive 在 driver.observe 前变换入帧（model-transport.ts 三拦截点对位）；生产 prepare→channel 边界测试覆盖。
- rust：`oc-core/src/session/runner/model_request.rs:974-1318` 构建 handshake/send/receive 三 seam 并存入 Prepared（:473/:1065）；全仓消费点仅 `tests/plugin_guest_wiring.rs:276-307`，SessionChannel::execute / open_responses_channel 均不接收——插件 WS 改写静默失效。
- ref：WS channel 消费 seams（第九轮勘误 #2 修的是握手派发面，与此"hook seam 死线"不同层）。
- 出处：cmp-A2+cmp-S4a / FINAL-G21（门抽查 ✅，互证裁定#1）。

### G-338 ✅ permission.evaluate 生产派发断线（G-2 修复未达面）
- **2026-11-11 第二十三轮收口 ✅（gb12-lane3 a4799e1e）**：permission.evaluate 生产派发接入 Permission 引擎：managed-policy（ref config policy 即首个注册 hook）→插件 hook 顺序组合、规则硬 deny 短路最先、hook 返回 effect/message 胜出（permission.ts:176-195 对位）；handler Err 保 pre-trigger 裁决（HookOverride 无错误通道，注释钉定）。
- rust：`oc-core/src/hooks.rs:1959`、`oc-core/src/plugin_apply.rs:2727` —— `dispatch_permission_evaluate` 全仓仅 2 个调用点且均在 `#[tokio::test]` 内；Permission 引擎只消费 managed-policy 的 HookOverride，guest hook 注册成功（G-2 ✅ 的桥接面）但永不生效。
- ref：permission engine 对插件 hook 派发。
- 后果：G-2 的 ✅ 只覆盖注册/白名单面；行为面断线。
- 修法：Permission 决策点接入 dispatch_permission_evaluate（叠加 managed-policy 语义按 ref 优先序）。
- 出处：cmp-S4a / FINAL-G22（门抽查 ✅）。

### G-339 ✅ ACP `edit` 工具 added 文件权限 diff 预览全丢
- **2026-11-11 第二十三轮收口 ✅（gb12-lane1 afea43cd）**：added 文件按 ref permission.ts:98-99 对空基线应用补丁，产 `Preview{old_text:None}` 新内容预览，不再 continue 跳过。
- rust：`oc-cli/src/acp/permission.rs:194-197` `status=="added"` → `old_text=None` → `let Some(old_text) = … else { continue }` 直接跳过，不产预览（对照同文件 patch 工具 :210-219 对 add hunk 正确推 `Preview{old_text:None}`）。
- ref：permission.ts added 文件产出 new-content-only 预览。
- 出处：cmp-S3b / FINAL-G23。

### G-340 ✅ ACP `session.retry.scheduled` 的 nextRetryAt 恒空串
- **2026-11-11 第二十三轮收口 ✅（gb12-lane1 afea43cd）**：数值 epoch-ms `at` 读取 + `iso_from_epoch_ms` 渲染 ISO 时间串（ref toISOString 对位，无新依赖）。
- rust：`oc-schema` session_event.rs:594 `at: NonNegativeInt`（数字）；`oc-cli/src/acp/translate.rs:529` 经 `string(&event.data,"at")`（as_str→unwrap_or("")）取值——恒回 `""`。
- ref：nextRetryAt 为 ISO 时间串。
- 修法：数字 → ISO 时间串格式化。
- 出处：cmp-S3b / FINAL-G24（门抽查 ✅）。

### G-341 ✅ TUI locale_duration 秒档 `%10_000` 取模 bug（10–60s 时长全错）
- **2026-11-11 第二十三轮收口 ✅（gb12-lane1 afea43cd）**：删 `%10_000` 取模；15_000→"15.0s"、59_999→"60.0s"（ref toFixed(1) 舍入，brief 预期值按 ref 修正）。
- rust：`oc-tui/src/message/model.rs:916-917` `f64::from(input as u32 % 10_000)/1000.0`——15000ms→"5.0s"（应为 15.0s）；10s–60s 区间全部显示 `x−10.0s`。
- ref：locale.ts 秒档直接 `/1000`。
- 修法：删 `% 10_000` 一行。
- ⚠️ 勘正：第十七轮勘误 #13 括注"locale_duration（model.rs:915-931，ref locale.ts 逐字数学）"与代码不符——T3 lane 与 FINAL 门两次独立读码均证实取模存在（见本轮勘误节#2）。
- 出处：cmp-T3 / FINAL-G25（门抽查 ✅）。

### G-342 ✅ TUI stdin 双读竞争（OSC52 查询 vs 输入线程）
- **2026-11-11 第二十三轮收口 ✅（gb12-lane11 089a8a2d）**：STDIN_READ_LOCK 单一 fd0 所有权 + pushback 分流：OSC52 查询窗内竞态击键回送输入线程而非吞键；顺带修 BEL 终止符 2 字节窗永不匹配隐性 bug；150ms 查询窗/无响应不死锁保持。
- rust：`oc-tui/src/terminal_proto.rs:474-519` `clipboard_read_osc52` 自行 poll/read fd 0；`oc-tui/src/app.rs:8717-8830` 输入线程并发 `read_session_input(tty_input_fd,…)`——两路并发消费同一 stdin，OSC52 查询窗口可吞用户按键。
- ref：单一输入读取路径。
- 修法：stdin 读取串行化（查询经输入线程代理或互斥）。
- 出处：cmp-T1 / FINAL-G26。

### G-343 ✅ Sessions 对话框 pin/unpin 死键（且 pinned 仅内存不持久）
- **2026-11-11 第二十三轮收口 ✅（gb12-lane11 089a8a2d）**：session.pin.toggle（ctrl+f）接线光标行；pinned 集持久化 `<state>/session.json {pinned}`（=v1→v2 迁移落点同形，tmp+rename 原子写）；session.deleted 修剪；boot 回灌。点击面 ref 本就仅 footer 提示（无处理器）——键位面对齐即闭环。
- rust：`oc-tui/src/store.rs:6929` `toggle_pin` 全仓 3 处——定义 + 测试 :14443/:14445，无 UI 分发；pinned 集合也不落盘（ref 持久化）。
- 出处：cmp-T2 / FINAL-G27（门抽查 ✅；不持久子项自 FINAL-G50 并入）。

### G-344 ✅ runway cancel 路径重复 `/v1` → 404（当前无消费方缓冲）
- **2026-11-11 第二十三轮收口 ✅（gb12-lane1 afea43cd）**：cancel_path 改 `/tasks/{id}`；拼接 URL 单一 `/v1/` 段钉定（`https://api.dev.runwayml.com/v1/tasks/...`）。
- rust：`oc-ai/src/media/video/runway.rs:20` `DEFAULT_BASE_URL` 已含 `/v1`；:310-312 `cancel_path` 返回 `"/v1/tasks/{id}"` 再拼一次 → `…/v1/v1/tasks/…`。与 G-347（媒体未接线）叠加故暂无用户可见面。
- 修法：cancel_path 改 `/tasks/{id}`。
- 出处：cmp-A4 / FINAL-G28。

### G-345 ✅ replicate/stability `unsupported` 字段声明未强制（行为反转）
- **2026-11-11 第二十三轮收口 ✅（gb12-lane6 dd1f0a6f）**：replicate 7 字段/stability generate 2+upscale 4 字段 check_unsupported 于 body_from 强制；错误文案=ref route/media.ts:423-448 逐字节（"{provider}/{route} does not support {fields}"）。
- rust：`oc-ai/src/media/image/replicate.rs:270` 声明 `fn unsupported()` 但 body_from 无 check_unsupported、route 不读（对照 openai.rs:151,373 / google.rs:106,399 / xai.rs:57,126 均有强制）；stability 同。
- ref："never silently dropped"——unsupported 请求参数必须显式报错。
- 出处：cmp-A4 / FINAL-G29。

## 🟡 中

### G-346 ✅ 错误体先 2KB 截断后分类：嵌套 error.code 漏检误判 retryable
- **2026-11-11 第二十三轮收口 ✅（gb12-lane7a 08d620ae）**：分类读上限 256KB（MAX_ERROR_BODY_CHARS）、展示截断 2KB 保持（provider_message=ref 截断位）；client.rs/responses_compaction/checkpoint/media status_failure 五处对齐；>2KB 嵌套 error.code 可重试分类测试钉定。
- rust：`oc-ai/src/route/client.rs:457`（frame path）`read_body_limited(response.body, MAX_BODY_CHARS)` + `route/executor.rs:13` `MAX_BODY_CHARS=2000` + `route/transport.rs:171-186`——截断先于 classifyProviderFailure，>2KB 的嵌套 `error.code` 丢失 → 429/限流类被误判不可重试。（与 G-288⑦ 的客户端 64KiB 诊断上限不同位点。）
- 修法：分类先行、截断仅用于展示。
- 出处：cmp-A2 / FINAL-G31。

### G-347 ✅ 媒体 provider 部署接线整体缺失（产品级，13+ 家协议不可达）
- **2026-11-11 第二十三轮收口 ✅（gb12-lane6 dd1f0a6f + 勘误）**：**布线前提失实（勘误撤销）**：ref v2.0.24 无 /api/media HTTP 面（protocol 组清单无 media、无 SSE；媒体=packages/ai SDK 库层，monorepo 零消费者——G-17 AIClient 族同类裁决，exlorer+orchestrator 双侧 file:line 取证）。可行动残余落地：①10 处 decode unwrap_or_default 严格化（required 字段→InvalidProviderOutput，逐站 ref Schema 对照；route.rs:179/utils.rs:206 为序列化非位点、protocol.rs:535 无生产调用，取证保留）②薄 media client 门面（client.rs：generate/stream/start/resume 按 lane kind 分发、非队列 lane start/resume=UnsupportedOperation，media-client.ts:35-62 对位）。
- rust：oc-core/oc-server 对 `oc_ai::media` **零引用**；`media/model.rs:6-8`、`media/route.rs:1004-1006` 注释自认 seam（"deployment lives with a future media-route lane"）；13+ 家 image/video 协议+route 已移植但无任何 HTTP 路由挂载；decode 族 `unwrap_or_default` 系统性宽松。
- ref：/api/media 路由族完整消费。
- 出处：cmp-A3+cmp-A4 / FINAL-G32（互证裁定#1，产品级单列）。

### G-348 ✅ cloudflare-ai-gateway 网关语义全降级（产品级）
- **2026-11-11 第二十三轮收口 ✅（gb12-lane8 941ebcd7）**：cloudflare-ai-gateway 原生车道全量：REST 根 `api.cloudflare.com/client/v4/accounts/{id}/ai/v1` + passthrough 根合成（encodeURIComponent 语义）、cf-aig-* 计费/缓存头、native 车道 cf-aig-authorization Bearer 透传 vs REST Authorization、`gatewayApiKey ?? apiKey → CLOUDFLARE_API_TOKEN → CF_AIG_TOKEN` 链、openai/anthropic（dot→dash id 重写）/workers-ai/else 前缀分派、requireAccountId typed error；CloudflareGatewayConnection 设置项+connection overlay 接线。
- rust：URL 合成 / `cf-aig-*` 计费头 / 前缀分派 / passthrough 鉴权全缺；仅 plain openai-chat 需用户显式 baseURL——经网关的 anthropic/google 等流量语义降级为直连。
- ref：gateway provider 完整实现。
- 出处：cmp-A3 / FINAL-G34（产品级单列）。

### G-349 ✅ shell DELETE 非幂等（404 vs 204）
- **2026-11-11 第二十三轮收口 ✅（gb12-lane1 afea43cd）**：remove 分支恒 204（NotFound 吞并对齐 ref delete handler）；HTTP wire 双删测试。
- rust：`oc-server/src/handlers/shell.rs:119-122` remove 分支映射 404；ref 吞 NotFoundError 恒 204。
- 出处：cmp-S1 / FINAL-G35（门抽查 ✅）。

### G-350 ✅ vcs.init 插件 provider 不可达（硬编码 git|hg 旁路）
- **2026-11-11 第二十三轮收口 ✅（gb12-lane9 405af21c）**：vcs.init 插件 provider 经 guest `vcs` hook 域（init 闭包表+provider 过滤+snapshot/reinstate+dispose 清理）派发；`Command::new(provider)` 直执行路径删除；git/hg 内建走显式子进程；缺 init 501/未知 400（G-270④ 面）保持；bun guest e2e + HTTP e2e。
- rust：`oc-server/src/handlers/project.rs:1876-1898` `provider != "git" && provider != "hg"` → 400，随后 `std::process::Command::new(&provider)` 直执行——G-270④ 修的错误族未触及执行通道；插件注册的 VCS provider 永不可达。
- ref：经 Plugin.awaitActivation → vcs.initialize 走插件 provider。
- 出处：cmp-S1+cmp-C5b / FINAL-G36。

### G-351 ✅ 空白 delta 丢弃（流式空行/缩进丢失）
- **2026-11-11 第二十三轮收口 ✅（gb12-lane1 afea43cd）**：session.text.delta 空白 delta 原样追加（仅缺字段跳过）；同根终值门 text.ended `!trim().is_empty()`→`!is_empty()`（present-but-whitespace 终值落位、真空串仍跳过）；"Hello"/" "/"world" 增量钉定。
- rust：`oc-tui/src/store.rs:13193`（"session.text.delta" reducer）`delta.trim().is_empty() → return`；ref data.ts:909-913 原样追加——纯空白 delta 丢弃后终值可与 transcript 不一致（同根终值非空门）。
- 出处：cmp-S2 / FINAL-G37。

### G-352 ✅ live 投影缺口：6 类 transcript 事件延迟到 resync；usage 只取 cost
- **2026-11-11 第二十三轮收口 ✅（gb12-lane11 089a8a2d）**：六类事件 live reducer：agent.selected/model.selected（gated on transcript）/moved 扩 location-switched 行/instructions.updated（textless 跳过）/synthetic/shell.started+ended（原地状态/退出更新）——全部经 project_message 插入（msg_{event.id}），resync 幂等去重兜底保持；usage reducer 增 tokens（session_tokens 信号）。
- rust：agent/model/location-switched/system/synthetic/shell 六类事件无 live reducer（仅 resync 填充）；渲染变体全集在（model.rs:262-361，T3 互证）——"能渲染、但延迟到下一次 resync"；`session.usage.updated` 只取 cost 漏 tokens。
- 出处：cmp-S2+cmp-T3 / FINAL-G38（拼合口径见 FINAL 门裁定#3）。

### G-353 ✅ mini 重连条件过宽：任何非 "Session not found" 错误无限重试
- **2026-11-11 第二十三轮收口 ✅（gb12-lane13 14c8c5f0）**：重连收窄为 transport 类错误（ResolveError{transport} 分类，ref mini.ts:157-180 对位）；非 transport 错误浮出失败、重连计数=0 钉定。
- rust：`oc-cli/src/mini.rs:122-147`（:137 仅排除 "Session not found"）任意 `Err(String)` 进 250ms 无上限 reconnect；ref 仅对 Transport 错误重连。G-181④/G-157 修的是重连存在性，条件宽度为残面。
- 出处：cmp-S3a / FINAL-G39。

### G-354 ✅ ACP `Sessions::release` 身份判定恒真
- **2026-11-11 第二十三轮收口 ✅（gb12-lane13 14c8c5f0）**：release 改 `Arc::ptr_eq` 附件身份判定（ref 引用相等语义）；teardown 顺带清 registered-MCP 映射；re-attach 保持/owner 释放两测。
- rust：`oc-cli/src/acp/sessions.rs:325-330` `entries.get(&attached.id).is_some_and(|current| current.id == attached.id)`——以 attached.id 查表再比 id 必然相等，条件 ≡ 条目存在 → 无条件删除；注释自称"only when this exact attachment still owns the slot"与实现不符。
- ref：按 attachment 身份判定（sessions.ts:169-172），并发重 attach 后不误拆。
- 出处：cmp-S3b / FINAL-G40。

### G-355 ✅ ACP cancel/drain 语义族（打包）
- **2026-11-11 第二十三轮收口 ✅（gb12-lane13 14c8c5f0）**：①drain 观察取消不再短路（observe_cancel=false 两臂）、静默流经取消 select 臂有界收敛（2.2s 收敛于 8s 停流测试）②$/cancel_request 预取消短路移除（标记在 turn 注册后武装，ref turn.ts:372-387 序）；ask 折叠链取证结构性等价。
- turn 取消后首事件短路 drain 丢弃收尾更新 + 事件流静默则 prompt 永不返回（FINAL-G41）；`$/cancel_request` 预取消不 interrupt server + ask 阻塞折叠循环（FINAL-G42）。
- 出处：cmp-S3b / FINAL-G41+G42。

### G-356 ✅ vertex Express+tuned 守卫缺失 + apiKey/accessToken 互斥缺
- **2026-11-11 第二十三轮收口 ✅（gb12-lane8 941ebcd7）**：vertex 四守卫（google-vertex-shared.ts:44-46 互斥、tuned `endpoints/` 拒 Express key、chat/messages 拒 API key）+ `access_token` 从 api_key 拆分（ModelConnection/overlay/resolver ADC 绑定改道——ADC token 不再静默覆盖已配 Express key）。
- rust：`oc-core/src/integration/vertex.rs:212-274` ADC binding 把 token 塞 `api_key`（:274），:273 注释自认 "apiKey cannot combine with accessToken upstream" 但代码不拒绝组合只静默取先者；Express 模式/tuned 模型守卫（ref 明确拒绝的组合）全缺。
- 出处：cmp-A1a / FINAL-G43。

### G-357 ✅ /compact 响应宽容解析 → "成功但空/残"历史
- **2026-11-11 第二十三轮收口 ✅（gb12-lane7a 08d620ae）**：compaction 响应严格化：encrypted_content 必填、summary 数组（空数组仍重放一空 part=ref `summary.length ? : [{text:""}]`）、role 字面量、content 必填非空、assistant 纯文本、input_image/file 字段族、phase 枚举；未知 item→InvalidProviderOutput 整体失败（responses-compaction.ts:52-77+open-responses.ts 对位）。
- rust：`oc-ai/src/protocols/utils/responses_compaction.rs:253-264`（compaction 缺 `encrypted_content` 填 `""`）、:311-319（未知 item → 空 user 消息；未知 role 一律归 User）；ref 严格 union 校验整体失败。
- 出处：cmp-A1a / FINAL-G44。

### G-358 ✅ MCP 目录分页截断（单发不跟 nextCursor）
- **2026-11-11 第二十三轮收口 ✅（gb12-lane10 1bc113f3）**：tools/prompts/resources/templates 四类 list 跟随 nextCursor（rmcp PaginatedRequestParams::with_cursor；1000 页安全上限=有界，ref/SDK 无界）；页大小 2×5 项 mock 服务器合并序测试。真实分页服务器对测：仓库无 env-gated 真实集成缝，mock 服务器为规范最低门（闭环）。
- rust：`oc-core/src/mcp/client.rs:863-869` ListTools/ListResources 等 `Request::default()` 单发直取 `result.tools`，全文件无 nextCursor 跟随（rmcp all-variants 未用）；>页大小目录静默截断。
- 修法：跟随 cursor 循环或启用分页变体；**放行后以真实分页服务器对测闭环**（P1 门条件#4）。
- 出处：cmp-C6a / P1-G07。

### G-359 ✅ MCP 运行期 OAuth token 过期无 refresh/恢复
- **2026-11-11 第二十三轮收口 ✅（gb12-lane10 1bc113f3）**：运行期 401（typed is_unauthorized，G-223 分类族）→ OAuth refresh（旋转持久化）→ 锁下 stop+start 重连换 bearer（headers 烘焙于连接期，重连即换牌——注释钉定）→ 单次重放不循环；缺凭证/无 refresh token/刷新失败→NeedsAuth 发布（拒绝的 grant 按 G-110③ 弃行）；hermetic loopback OAuth 桩三测。
- rust：`oc-core/src/mcp/client.rs:1496-1502` 连接期静态 bearer 快照（`input.bearer` 直插 header）；运行期 401 仅 unauthorized 文本，不续期、不转 needs_auth。
- ref：OAuthClientProvider 由 SDK 自动 refresh（client.ts:149-151）；与 G-110（已修的失效凭证清理）不同面。
- 出处：cmp-C6a / P1-G08。

### G-360 ✅ MCP 工具桥缺 sessionID 元数据 + 桥同步竞窗 + 注册面无名称校验
- **2026-11-11 第二十三轮收口 ✅（gb12-lane10 1bc113f3）**：①call_tool 携带 sessionID 元数据（execute_with_context 线程，ref tool/mcp.ts:69,124）②桥同步竞窗以桥级 SyncMutex 串行（ref Semaphore.makeUnsafe(1)；12 路并发 catalog 突变无损注册测试）③注册名 namespaced_registration_error（segment `^[A-Za-z0-9_-]{1,64}$`/名≤128/charset/Code Mode execute 保留，ref tool.ts:177-183/304-320；违规 skip+log）。
- rust：`oc-core/src/mcp/registry.rs:1672` `call_tool(..., None)` sessionID 丢弃（ref tool/mcp.ts:33,69,124 透传）；:1831-1882 初始注册与去抖 reload 的桥表同步存在竞窗；P1 裁定#2：MCP 工具注册直插 BTreeMap，**完全没有** ref tool.ts:177-218 的 RegistrationError 名称校验（超长/非法名静默注册）。
- 出处：cmp-C6a / P1-G14+裁定#2。

### G-361 ✅ vcs git 命令族偏差（打包）
- **2026-11-11 第二十三轮收口 ✅（gb12-lane9 405af21c）**：①`--no-optional-locks`+5×`-c core.*` 旗标组（ref plugin/vcs/git.ts:158-168）全 git spawn 点②status cwd=location 目录+`--untracked-files=all --no-renames -z -- .` 裁剪（scope 收回）③增删计数 numstat HEAD+untracked `--no-index` 回退④状态字母经共享 diff_kind 矩阵——AM→**added**（**审计文本「ref 映射 modified」失实：git.ts:171-178 实为 added，按代码真值对齐**）；fake-git argv 捕获钉定。
- rust `oc-server/src/handlers/project.rs`：:1255/:1522/:1684/:1724/:1736 `Command::new("git")` 直行、无 `--no-optional-locks`/`-c` cfg 标志组（ref git.ts:63-99）；:1833-1837 列表命令 cwd=`services.project.canonical` 而非 location.directory → **scope 放大至全仓**；:1849-1850 git status `additions/deletions` 恒 0（hg 侧 :885-894/:992 有 count_patch，git 侧无）；:1857-1863 `"AM "` 映射 added（ref 映射 modified）。
- 出处：cmp-C5b / P1-G11。

### G-362 ✅ `snapshots:false` 配置键无消费者 + 文档级 `mcp.timeout` 未合并（双键打包）
- **2026-11-11 第二十三轮收口 ✅（gb12-lane2 095a7d32）**：①snapshots 键 latest-wins 消费→SnapshotService::configure：false=capture no-op 返 None（=ref snapshot.ts:83-87,115 语义，restore 同门）②mcp.timeout 文档级 per-field spread 合并（`{...timeout,...server.timeout}`，Local/Remote 双变体；per-server 胜出/未设继承/无文档 timeout 不动三测）。
- rust：`GitSnapshotStore::configure` 全仓仅 `snapshot/tests.rs:168-170` 调用，engine 只 configure compaction/tool_output——`snapshots:false` 被接受但快照恒写（ref config/plugin/snapshot.ts:15-19→snapshot.ts:83-87,115 为开关）；`engine/mod.rs:3330-3337` `mcp_servers_from_config` 只读 `mcp.servers`，ref config/plugin/mcp.ts:44-59 的 `{...timeout,...server.timeout}` 文档级合并缺失（per-server timeout 生效 ✅——P1 门裁定#1 两口径）。
- 出处：cmp-C4+cmp-C5b+cmp-C6a / P1-G04+裁定#1。

### G-363 ✅ 默认模型无"首个可用 text 模型"回退
- **2026-11-11 第二十三轮收口 ✅（gb12-lane2 095a7d32）**：first_available_text_model 回退链尾（model-resolver.ts:402-417+model.ts:259-289：available_models 同 listing 可用性折叠+supportsText+release 降序）；**仅叶子 resolver opt-in**——链序 pin→location default→engine default→首个 text 模型（location overlay 误触发回归驱动的设计修正）；空 catalog→NotSelected 保持。
- rust：`oc-core/src/engine/resolver.rs:660-675` 仅 pin/config default，否则直接 NotSelected；ref model-resolver.ts:402-417 明确回退链。
- 出处：cmp-C6b / P1-G06（门抽查 ✅）。

### G-364 ✅ location 关闭缺 rpc.close
- **2026-11-11 第二十三轮收口 ✅（gb12-lane2 095a7d32）**：shutdown 四步序=ref（isClosed 先置→permissions→forms→close_rpc→publish）；rpc.close 以 per-location 门实现（closed/rpc_closed 标志+engine invalidate 钉 shutdown 标记+重装配清除）——/api/rpc 对失效位置回 400 rpc.unavailable（ref rpc.ts:105-107 文案）；单 guest 架构（G-289）下为门而非注册表丢弃（注释钉定）。
- rust：`oc-core/src/location.rs:978-992` shutdown 仅 permissions.close→forms.close→publish 三步；ref location-lifecycle.ts:44-54 四步含 rpc.close+race——失效 location 的 RPC 域仍可经 /api/rpc 命中。
- 出处：cmp-C7b / P1-G09。

### G-365 ✅ plugin install 无原子性（staging 从未创建）
- **2026-11-11 第二十三轮收口 ✅（gb12-lane5 86165976）**：`.staging-<ms>-<pid>-<seq>` 暂存目录（=update.rs GC 1h 清扫布局）→ manifest+installer+verify 全程暂存 → 成功 rename 落位（入口路径重定基+stamp 冲突重试）；失败 best-effort 清扫；latest_install 只认数字代。失败不可复用/成功暂存清空两测。
- rust：`oc-plugin-host/src/context/install.rs` 直接写最终目录；staging 目录仅存在于 GC 代码、从未创建——残缺安装可被 `latest_install` 复用。⚠️ 置信度受限：S3a updater/plugin 安装路径未逐行（盲区#1）。
- 出处：cmp-S4a / FINAL-G46。

### G-366 ✅ google-interactions 事件形状零校验 + options 硬失败违背 lenient
- **2026-11-11 第二十三轮收口 ✅（gb12-lane7a 08d620ae）**：step.start/delta/stop 要求数值 index、step/delta 须对象（缺省→frame_decode_error，不再折 0 串扰；ref Event schema index:Number 必填）；options 解码 lenient 化（wrong-type→None、未知键忽略，ref lenient+knownString 对位）——不再 invalid_request 硬失败。
- rust：`oc-ai/src/protocols/google_interactions.rs` index 缺失折叠 0 → 步骤串扰；step 缺失错误语义错位；options 校验硬失败（ref lenient 容忍未知字段）。
- 出处：cmp-A1a / FINAL-G47+G63（options 子项）。

### G-367 ✅ session.tps 默认值反转
- **2026-11-11 第二十三轮收口 ✅（gb12-lane11 089a8a2d）**：三解码路径默认翻 true（ref config `?? true`）；显式 false 存活（unwrap_or 仅缺键适用）；组件级 TranscriptOptions::default 未动→**零金帧变化**（oracle 门通过无重钉）。
- rust：`oc-tui/src/config.rs:234/378/501` `session_tps: false`（:181 注释自证对应 ref index.tsx:1892）；ref 默认 true。
- 出处：cmp-T4 / FINAL-G48。

## 🟢 低（打包簇）

### G-368 ✅ T1 运行时边角簇
- **2026-11-11 第二十三轮收口 ✅（gb12-lane12 6529e522）**：标题 fallback regex+插件路由臂（session-title-fallback.ts 对位）/选区 ctrl+c 清除选择并放行中断（ref 不 preventDefault 语义）/帧率 100ms 钉注（vs ref targetFps:60 理由）/storage 写 flock（storage.tsx:52,72-80 对位）；SIGHUP=异步信号安全超集、kitty 探测门控、量化近似既有注释三取证无差；fs.watch 活面有界（端口无反应式 entry map）。
标题 fallback regex 缺 + 插件路由标题臂缺 / SIGHUP 无 epilogue `_exit(1)` / 选区 ctrl+c 吞键 / kitty 探测时序 / 颜色量化经验推导（上游漂移风险）/ 帧率 100ms 上限 / storage 无跨进程 flock+watch。（剪贴板宿主写臂→G-39、boot 300ms 窗→G-17③，归类不重复。）
- 出处：cmp-T1 / FINAL-G49。

### G-369 ✅ T2 composer 对话框簇
- **2026-11-11 第二十三轮收口 ✅（gb12-lane12 6529e522）**：附件复用键 uri(≡mime+data)+name 且 Image-only（ref attachment.ts:85-90）/非图像 data-URI（image/*+pdf 外）拒装不再误标 `[PDF N]`/历史导航 edited-draft 守卫（history.tsx:106-110）；palette 插件命令合并取证已在位（G-186 落地面覆盖 palette 而非仅 slash）；ctrl+f 五层声明=ref keybind.ts 真值钉注。
附件标签复用判据 mime+data vs ref uri+meta+text / 非 image data: URI 误标 `[PDF N]` 而非报错 / 命令面板静态表不含插件命令 / 历史导航守卫缺 / ctrl+f 双绑定注记。（pinned 不持久已并入 G-343。）
- 出处：cmp-T2 / FINAL-G50。

### G-370 ✅ T3 渲染簇
- **2026-11-11 第二十三轮收口 ✅（gb12-lane12 6529e522）**：低 verbosity messagePath=[Activity,Instructions] 包裹/question 空组/synthetic 空描述跳过/turn 边界=ref idle-marker 规则/纯 thought 组 Thinking busyLabel（运行中 thought 胜后续工具）/subagent 模型后缀 `#variant`+Background 徽章/inputIndex 回溯（synthetic 计输入）8 修；diagnostics 臂取证在位并补测；mount 权重/tok/s 基准（time.streamed 未上 wire，重基需 oracle 重钉）/RetryProvider 渐变 3 项有界注释钉定。零金帧重钉。
低 verbosity messagePath 缺 activity,instructions 包裹 / question 包进 Activity / pending 不重排行尾 / synthetic 空 description 裸 ◈ 行 / mount flat 权重 / turn 摘要"下一行 User"启发式 / 纯-thought 组无 Thinking busyLabel / tok/s 口径 / Diagnostics 行 / RetryProvider 动画 / subagent 模型后缀原始串 + Background 徽章 + 点击跳转 / 输入回溯不认 synthetic。（语法高亮默认构建窄面→G-13。）
- 出处：cmp-T3 / FINAL-G51。

### G-371 ✅ A1a/A1b 协议簇
- **2026-11-11 第二十三轮收口 ✅（gb12-lane7b 83d0b84b）**：AWS 默认凭证链四腿（credential_process shell→JSON、role_arn→SigV4 STS AssumeRole+单跳 source_profile、web-identity→AssumeRoleWithWebIdentity、ECS RELATIVE/FULL_URI+token；env→env web-id→profile(static→process→web-id→assume-role→SSO)→ECS→IMDS 序=ref bedrock-auth.ts；多跳链有声失败=有界）+gemini 命名空间展平保留 provider_executed/cache/metadata+baseURL `${VAR}` 模板（未解析→UnresolvedProviderVariablesError 语义）+converse metadataKey=provider 派生+event-stream rawBody 2048B 诊断+copilot kind 经 `x-opencode-request-kind` 会话缝权威化（title→conversation-background）+media:617 分类先于截断并案；mistral 哈希（无 8 位截断，取证）/thinking delta 五路径（取证无差）/partial-json NaN（不可表示+fallback 等价）/轻校验+路由字段（makeRouteLanguageModel 两查已在）/D-Metric 四取证无差；bedrock_invoke_route 钉超集。
gemini 命名空间扁平重写丢 provider_executed/cache/metadata（FINAL-G30）/ partial-json NaN 宽容 / thinking delta 丢失路径 / error.code 截断族 / 轻校验 / route 字段 / baseURL `${VAR}` 模板（FINAL-G63）+ bedrock converse metadataKey 硬编码 "bedrock" / 事件流错误无 rawBody 诊断 / AWS 默认凭证链无 credential_process·assume-role·web-identity·ECS / bedrock_invoke_route rust 独有 / mistral converse 哈希 `&0xFF` 截断 / copilot kind（FINAL-G52）。
- 出处：cmp-A1a+cmp-A1b / FINAL-G30+G52+G63。

### G-372 ✅ A2 管线簇
- **2026-11-11 第二十三轮收口 ✅（gb12-lane7b 83d0b84b）**：pending values() 迭代序=JS Object.values（整键数值升序→字符串插入序；"10">"2" 分歧测试）+providerExecuted Some(false) 不再被 `.map(|_| true)` 强转真（保持本地执行）+complete() 全终点触发（错误交换无 pending 暂存=no-op，可观察行为=ref）；D-Metric 取证 ref 路由层双侧皆无；send_text 30s 钉有界超集（SendFn 防梗塞界）。
pending BTreeMap id 序 vs 插入序 / `Some(false)` 不入 pending（providerExecuted 布尔回案同根）/ complete() 仅 clean 结束调用 / D-Metric 计数器缺 / send_text 30s 超集。（typed generateObject 子集=G-100 已修，归类不重复。）
- 出处：cmp-A2 / FINAL-G53。

### G-373 ✅ A3 provider 簇
- **2026-11-11 第二十三轮收口 ✅（gb12-lane8 941ebcd7）**：alibaba region 表+`{workspaceID}.{region}.mams...` host 派生（chat/responses `compatible-mode/v1`、messages `apps/anthropic/v1`；未知 region typed error、用户 baseURL 胜出；cn 区不再静默打 intl）/zai bigmodel.cn `api/coding/paas/v4→api/anthropic/v1` 重写入表/xai chat `x-grok-conv-id`（prompt_cache_key→header 缝=vercel affinity 同款）/custom `package` 透传至 native lane（providers_from_config provider+model 双级；无 key 声明包保持匿名）/anthropic-compatible 车道（authToken→Authorization Bearer、apiKey→x-api-key、互斥错、keyless 可选）。
alibaba region/workspaceID host 派生缺 → cn 流量打 intl / zai bigmodel.cn messages 根未入重写表 / xai chat `x-grok-conv-id` 缺 / 自定义 provider `package` 字段解而不用 / anthropic-compatible 自定义 host 降级 openai-chat + authToken bearer 缺。
- 出处：cmp-A3 / FINAL-G54。

### G-374 ✅ A4 媒体簇
- **2026-11-11 第二十三轮收口 ✅（gb12-lane6 dd1f0a6f）**：READ_RETRY jittered_delay [0.8,1.2]×（Effect Schedule.jittered 语义，64 样本界测试）+multipart 边界 `opencode-media-{2×16hex}` 随机；cancel 取证**本就正确**——无 CancelSpec 即 UnsupportedOperation（route.rs:1145-1151，审计「no-op 反转」断言不成立，证据关闭）。
Generation.cancel no-op vs ref UnsupportedOperation（语义反转）/ READ_RETRY 去抖动 / multipart 边界可预测。（unsupported 拒绝时机后移已并入 G-345 主面。）
- 出处：cmp-A4 / FINAL-G55。

### G-375 ✅ S1 server 簇
- **2026-11-11 第二十三轮收口 ✅（gb12-lane9 405af21c）**：events.persist 服务级开关（ServerOptions 对位：EngineOptions.event_persistence+BootOptions.events_persist 嵌入者可关；**默认维持开=文档化分歧**——ref CLI 缺省 Bus false 但端口 event-log 消费面 load-bearing，仅 workerd 钉 true）/persistent-pty 游标 JS Number() 宽容集（trim/""→0/指数/0x/0o/0b/符号规则+isSafeInteger 门）/pty.rs decode 注释对齐实现（ref fatal-drop 全 chunk 真值）；pairing 逐出取证等价（TTL+消费即删+零读→FIFO-by-expiry≡LRU）；fs find=fuzzysort 3.1.0 忠实移植（G-383⑦ 并案：go/algorithm/prepare/bitflags 全量，bun 探针逐字 fixture 钉序；无音调折叠/UTF-16 码元算术=精确，平手按输入序=有界）。
events.persist 无服务器级开关 / persistent-pty cursor `Number()` 宽集合 vs `usize::parse` / pairing 容量逐出策略 / pty.rs:415-424 注释与实现矛盾。（workerd/simulation 剖面→G-163/G-8；replay 分块按字符边界→G-10 关联、G-105 已修。）
- 出处：cmp-S1 / FINAL-G56。

### G-376 ✅ S2 client 簇
- **2026-11-11 第二十三轮收口 ✅（gb12-lane14 97629310）**：RPC 订阅共享单 SSE 连接（client 级 SharedEvents OnceLock 缓存多路复用，ref make() 一连接模型；两 on() 连接数=1 测试）/probe URL 绝对路径替换（`new URL("/api/info", base)` 语义，live server 反代测试）/DiscoverOptions 版本谓词（string|谓词，exact 优先，matchesVersion 对位）；deny_unknown_fields 取证 no-delta（ref promise client json()=JSON.parse+cast，唯一严面 rpc-runtime 已由 call_into 边界镜像）；shared-events 错误完成先放缓冲再终错（Failed vs Overflow 分裂=ref finish(result,false)）+4096 off-by-one 取证一一对位（第 4097 个未投递事件即错）。
RPC 订阅每 `on()` 独占 SSE 连接（连接放大）/ probe URL 保留 basePath 与 ref 相反 / serde 宽松解码无 deny_unknown_fields 门 / DiscoverOptions 版本谓词仅 string / shared-events 错误完成弃缓冲 + 4096 off-by-one。（effect 面→G-28/G-148。）
- 出处：cmp-S2 / FINAL-G57。

### G-377 ✅ S3a cli 簇
- **2026-11-11 第二十三轮收口 ✅（gb12-lane13 14c8c5f0）**：run 新会话 title 更新失败浮出（`let _` 移除，ref run.ts:130-136 无守卫 await 对位）；--param 重复键取证=ref Object.assign 后写胜（no-delta）；service stop pty 策略取证 ref 未显式（默认 Clear 两侧同，no-delta）。
api `--param` 重复键后写覆盖 / title 更新失败静默 / service stop pty 策略 Clear vs ref 未显式（待核）。（升级换源/端口错开/TTY 权限交互/诊断信号缺席→G-17 rust-extra 族、G-238、G-38；ssh-askpass→G-316。）
- 出处：cmp-S3a / FINAL-G58。

### G-378 ✅ S3b ACP 簇
- **2026-11-11 第二十三轮收口 ✅（gb12-lane13 14c8c5f0）**：_meta opencode/child-session 注入（child update+fallback 双面）/MCP 跨 attach 去重（同 config 不重发）+失败回滚+owner 检查移除/后台 follow 扩全非失败终点（Succeeded 含）/command 提交等待 HTTP 结果/available_commands Responded 门（session new/load/resume/fork 四口）/PromptResponse usage 全字段（vendored crate unstable_end_turn_token_usage 门开）/readDisplayText 解包（text-page envelope→entries join）/zed:// path 全查询位+百分号解码/replay data-url 按 cwd 组 file:// URL/directories realpath+词法回退；child-session 创建取证 ref 本就无（仅服务端 session.created）；API 签名小改（subscribe/on 同步化=共享多路复用后无 HTTP await）。
child 投影缺 `_meta opencode` / child-session / MCP 跨 attach 去重缺 + 失败回滚缺 / handoff 仅 Interrupted 跟随 / command prompt 后台不等结果 / available_commands 先于响应 / PromptResponse usage 缺（unstable feature 未启用）/ readDisplayText 跳过 / zed:// 仅 ?path= 首参 / replay file URL 无目录 / directories 缺 realpath。
- 出处：cmp-S3b / FINAL-G59。

### G-379 ✅ S4a 插件宿主簇
- **2026-11-11 第二十三轮收口 ✅（gb12-lane5 86165976 + gb12-lane5b 2a8fee08）**：宿主簇 7 项（tui 子路径探测统一 no-fallback/vcs dispose 所有权索引修复（原谓词反置恒 false 清整表）+默认 editor 实现（共享内存 selection get/set）/merge_skills 严格 id（ref 品牌化 id 必填）/事件 best-effort 丢弃计数（BroadcastTap.dropped_count）/plugin.list 补 source|state(active|failed+error)|features(server/tui/rpc) 投影/命令调用形状=ref CommandInvocation）+guest 调用面 2 项（rpc.invoke 客户面：ctx.rpc.call/client→共享 rpc_dispatch（/api/rpc 同核），跨插件 typed e2e×5；vcs branch.list/status/diff+worktree list/create/remove/refresh 端口化：VcsPort 扩三读面+新 WorktreePort，HTTP handler 与插件面共乘同 data core，真 git 工程 bun e2e）；sidebar 行可点击取证 ALREADY_PRESENT（PluginSectionHits/plugin_section_click/slot_click e2e 在位）。
tui 子路径探测自相矛盾（probe 无回退 vs resolve_subpath 有）/ vcs dispose 过滤恒 false 清空整表 / vcs default editor no-op stub / merge_skills `id??name` 键回退 / 事件 best-effort 丢弃计数 / ui.sidebar.section 行不可点击 / rpc client 调用面缺 / vcs branch.list·status·diff 与 worktree list·create·refresh·remove guest 调用面缺（G-24 修的是 transform/reload，读面三件未覆盖）/ plugin.list 投影 4 字段 / 命令调用形状近似。（markdown 渲染器→G-332；effect 拒载→G-28/G-148。）
- 出处：cmp-S4a / FINAL-G60。

### G-380 ✅ S4b schema 簇
- **2026-11-11 第二十三轮收口 ✅（gb12-lane14 97629310）**：NonEmptyArray 解码门（common.rs non_empty::{vec,option_vec}→ToolState.content×2/ToolSuccess.content/ToolFailed.content/form fields×2 六面，decode-only=ref 解码期校验、序列化/openapi 面零影响）/identifier 同毫秒计数器 Mutex 串行化（CAS 方案仍有碾压窗——注释载明；16×64 前缀并发唯一性测试）/websearch published→Option<f64>（JSON 构造性有限=ref Finite；iso8601/parse_date_millis 适配；openapi 钉基线零 diff——served doc 本就 number）；BTreeMap 键序有界维持（workspace R-03 刻意 patch 掉 preserve_order 保排序确定性——如需对 ref 输出序做字节级金样再升级）。
BTreeMap 键序重排影响逐字节快照 / NonEmptyArray→Vec 宽松 / identifier check-then-act 竞窗 / websearch published Finite→NonNegativeInt。（simulation→G-8；flock 简化=第七轮勘误 #6 已裁定；glob 子集→G-92；theme null→G-160③ 已修。）
- 出处：cmp-S4b / FINAL-G61。

### G-381 ✅ T4 键位簇
- **2026-11-11 第二十三轮收口 ✅（gb12-lane12 6529e522）**：顶层 BindingObject 按 BindingValueSchema union 解析（不再丢弃）/win32 terminal.suspend 解绑+ctrl+z 前插 input.undo（用户覆盖除外，ref config/index.tsx:288-296；纯逻辑+测试，cfg(windows) 门控接线）/prompt.editor 门控 editor-context 附加；meta→alt·hyper→super 折叠钉注（4 修饰引擎匹配安全）；scroll.speed/terminal.title/animations 消费取证在位。
BindingValue 顶层对象不可解析被丢弃 / meta→alt·hyper→super 折叠 / win32 ctrl+z suspend/undo 反转 / 解析未消费键（scroll.speed/prompt.editor 等）。（插件目录发现初报经十九轮勘误 #13 证据复核**撤销**，见勘误节#1。）
- 出处：cmp-T4 / FINAL-G62（去掉已撤销子项）。

### G-382 ✅ core 杂项簇（C 系 residual）
- **2026-11-11 第二十三轮收口 ✅（gb12-lane15 161e6d99）**：question 布尔回案→[String(value)]（ref question.ts:99-107）/ModelUnavailable deprecation notice 文案（azure-cognitive-services/google-vertex-anthropic 迁移提示）/空 provider state `{provider:{}}` 保留（ref 仅丢 undefined）/越界 location→BootError::LocationOutsideProject typed error（相对拼写先 fs.resolve 归一；parity 测试自弃 TempDir 隐性 bug 顺修）/全局 AGENTS.md 读失败跳过（仅 project 源可 unavailable=ref instruction.ts 语义）/mime 嗅探收敛 oc_util::mime::detect_mime（PNG 8 字节全签名+GIF8 4 字节=ref；read.rs/prompt.rs 私副本删除）/canonical_json 浮点 JSON.stringify parity（1.0→1，instruction hash 面）；no-finish 事件面（G-105/G-336 已覆盖）/"opencode" 常量/lsp 键/subagent 回扫首完成即 NO_TEXT/instruction scrub（ref 无该 scrub——前提不成立）五项取证无差；reload_all 无生产调用（返回失败数据=ref fail-first 超集）；replay 无逐型 schema 解码登记 DEVIATION（EventDefinition 不载 data schema，52 型解码器属跨切工程且会破旧行回放——bus.rs 注释+本条盯守）。
question 布尔回案映射（question.rs:160-167）/ 事件面族：无 finish 判失败超集、弃用提示文案缺、experimental generate service 恒 "opencode"、空 provider state `{provider:{}}` 省略（misc.rs:39、mod.rs:445、to_llm_message.rs:36-44）/ location 越界静默全仓快照（engine/mod.rs:1807，与 G-362 叠加放大）/ AGENTS.md 全局文件读失败过度降级 + lsp 键惰性（instruction.rs:104-107）/ mime 三副本不一致（read.rs:126-151 PNG 4 字节签名/GIF 双值）+ reload_all 吞失败 / subagent completion_text 空文本回扫旧消息 / replay 不做 schema 解码（宽容 vs ref die）/ canonical_json 浮点序列化差异 / instruction scrub 不中止在途交换。（output schema 豁免→G-230 bounded；60s/10s 有界窗→G-317；boot 悬置点已由 P1 门条件#2a 关闭——boot.rs:670-686 有守卫。）
- 出处：cmp-C1+C2a+C3+C4+C5a+C6b+C7b / P1-G12+G13+G16+G17+G18+G19+G20。

### G-383 ✅ pty/shell 服务簇（C7a residual）
- **2026-11-11 第二十三轮收口 ✅（gb12-lane4 35bfd4b1 + gb12-lane9 405af21c）**：reader 携带缓冲解码（=G-335 同 lane 落地）+pty/shell 服务 ShutdownGuard teardown（pty 杀全部活子进程+终订；shell 解阻 waiter/abort 超时纤维，子进程按 ref shell.ts:141-147 不杀；pump/watcher 改持 Arc<Inner> 防 pin）/wire 超集以 chunked-replay 订阅 opt-in 谈判收口（`subscribe{chunked_replay}` 字段：老服务器忽略、ref 客户端永不收未知形状；title/foreground 两变体取证本就在 ref daemon.ts:97-98 非超集）/buffer 保留上限 UTF-16 码元口径（cursor-buffer_cursor 不变量 O(1)；persistent_pty 字节环=设计保持）/pty.exited exitCode Option+省略序列化（信号路径不再 unwrap_or(0)，node-pty undefined 对位）/发布时序+20ms try_wait 合并窗注释钉定良性/daemon prepare_handoff IO 出锁（所有权检查锁内、handoff 文件 IO 锁外、重锁后复查）；fs find 评分并案 G-375。
服务无 teardown（pty/shell 子进程与 waiter 无终态化，无 Drop；ref pty.ts:128-134）/ persistent-pty wire 协议超集共用版本号 7 的互操作风险（建议版本协商或升 v8）/ pty buffer 字节 vs ref UTF-16 单位口径差（更保守）/ pty.exited exitCode `unwrap_or(0)` / 事件异步发布时序 / watcher 25ms 合并窗 / fs find 无 fff+贪心评分（G-324 关联面）/ daemon prepare_handoff 持锁 IO。
- 出处：cmp-C7a / P1-G10+G15+FINAL-G60（超时/进程模型子项）。

## 归类与补注（不另立项）

- **G-7/G-17 补注（evaluation/SystemOne 域）**：本轮 A3 复认缺失面并细化——typesafe-ai 在 ref 是 **provider**（rust provider 矩阵缺行）、zen evaluation 与 openrouter/vercel `/evaluate` 缺；维持 G-7 ℹ️"有意不移植"的既有裁决，FINAL-G33 归此不另立。
- **G-39 补注**：剪贴板宿主写臂（T1 复认）维持 ℹ️。
- **G-163/G-8 补注**：workerd 剖面与 OPENCODE_SIMULATE 拒绝（S1/S4b 复认）维持 ℹ️。
- **G-332 补注**：guest markdown 渲染器注册缺失经 S4a/T4 双侧互证（FINAL-G45 归此）。
- **G-317 复认**：TOOL_JOIN 5s / UNWIND 60s / remove 10s 有界族（C2a/C2b 同族偏差合并跟踪）。
- **G-316 复认**：ssh-askpass argv[1] 参数位（S3a）。
- **G-230 bounded 复认**：question/output schema 豁免（C1）。
- **G-92 复认**：glob 子集（S4b）。
- **G-324 复认**：node_modules 显式跳过=超收收紧（C7a fs find）。
- **G-13 复认**：语法高亮默认构建窄面（T3）。
- **G-28/G-148 补注**：S4a 发现 effect 导入面整体拒载（detectEffectImports），与 G-148②"仅 clear-error 钩子"的 bounded 表述关系待核（不改变既有裁决）。
- **G-105/G-10 复认**：replay 分块按字符边界切 64KiB 帧（S1 D-5 与 C1 D-7 重复发现，P1 裁定#4 归并）。

## 勘误与勘正（第二十二轮）

1. **撤销本轮 T4 初报"插件目录自动发现走 config 声明"**——与十九轮勘误 #13 证据冲突：`resolve.rs:124-172` 自动扫描 plugin/plugins 目录、`plugin_boot.rs:199` 已接线（T4 审的 oc-tui plugin_boot 与 oc-plugin-host resolve 为不同层，目录放置发现**在**）。勿再立项。
2. **勘正第十七轮勘误 #13 括注**——"locale_duration（model.rs:915-931，ref locale.ts 逐字数学）"与代码不符：`model.rs:916-917` 存在 `% 10_000` 取模（T3 lane 与 FINAL 门抽查两次独立读码），立项 G-341。该勘误的主结论（mini duration_label 死代码撤销）不受影响。
3. **早前审查清单两处证伪记录（防复发）**：36 主题实为 33+1（md5 全等）；OKLCH 两侧同源。已录 REPORT.md。
4. **C6a 对 C4 mcp.timeout 的"证伪"不成立**（P1 门裁定#1）：`client.rs:1262-1278` 消费的是 per-server timeout；文档级 `mcp.timeout.*` 合并确实缺失——两口径并入 G-362。

## 正面互证（本轮全量对齐面）

keybind 230/230 全对齐；主题资产 33+1 md5 全等；openapi 141 operation 字节级 0 diff；48 迁移 ID 全同；52 会话事件分类一致；models-dev 快照 md5 相同；session-message parts 全集无缺漏。

## 审查盲区（10 项，如实披露）

1. S3a 窄读：rust plugin.rs:151-863、updater.rs:121-2329、update_feed.rs 全文、auth.rs:701-1518、mini.rs:161-398——**G-365 installer 细节未逐行复核（最高优先披露）**。
2. A3：~180 个 catalog-only provider 未逐一验证 api/env 字段（推定）。
3. T1：store.rs 其余 ~13k 行、components 逐行（靠 T2/T3+注释引用密度覆盖）。
4. T3：store.rs SSE 增量 append 逐事件对拍未做；solid 画笔依赖 golden；history prepend 位置未定位。
5. S1：rust model.rs/fs.rs/plugins.rs/lifecycle.rs/errors.rs 未逐行（路由表+抽查）。
6. C8：host.js codemode 解释器内部按任务书跳过（逃逸面结论已独立核验）。
7. C 系遗留：variant.ts 其余 ~40 家族常量 diff、optimize/*.txt 字节 diff、persistent_pty/frontend.rs(583L)、repository_cache.rs 结构级。
8. A1a/A1b：open-responses 主文件仅签名级、responses-hosted-tools 三端交叉验证、bedrock SSO/IMDS 细节、copilot 转换器全量。
9. T4：mini footer.prompt/command 子特性、theme expand/resolve/syntax 字段级（结构面+md5 替代）。
10. S4b：theme resolve.rs 后段、v1_migrate 函数级、低风险 util 仅确认落位。

明细见 `docs/audit/port-parity-line-review/lanes/gate-FINAL.md`。

## 第二十二轮判定汇总

27 lane 初报候选经两门合并登记册（G-01..G-63）→ 去重/归并/撤销后**正式立项 50 项**：🔴×12（G-334..G-345）/ 🟡×22（G-346..G-367）/ 🟢 打包簇×16（G-368..G-383）；**归类既有 12 处**、**撤销初报 1 条**（T4 插件目录发现）、**勘正旧勘误 1 处**（R17#13 locale_duration 括注）。产品级缺口单列不混入行为偏差：G-347（媒体部署接线）、G-348（cf-ai-gateway）、evaluation 域（归 G-7/G-17）。修复优先序（两门建议合并）：小成本高收益先行——G-341（删 `% 10_000` 一行）→ G-334 最小步（删 `__rawFetch`）→ G-336（补 `!stream_interrupted`）→ G-343（toggle_pin 分发）→ G-344（cancel_path 去 /v1）→ G-339/G-340（ACP 预览/时间串）→ G-337/G-338（插件钩子断线对）→ G-335（pty 字节缓冲化）→ G-342（stdin 串行化）→ G-362（snapshots/mcp.timeout 双键）→ G-363（默认模型回退）→ 其余 🟡 → 🟢 簇按域随手收；G-358 放行后以真实分页服务器对测闭环。质量口径：0 处未裁定矛盾、抽查 11/11 成立、盲区 10 项如实披露。记录：`docs/audit/port-parity-line-review/{REPORT,TABLE}.md` + `lanes/`（32 份）。

---

# 第二十三轮 burndown 收口（2026-11-11，gap-burndown-12）

> 第二十二轮立项的 G-334..G-383 全部 50 项（🔴×12 / 🟡×22 / 🟢 簇×16）经 17 个串行 lane（L1 快赢、L2 engine、L3 hooks、L4 PTY、L5 plugin-host、L5b guest 调用面、L6 媒体、L7a/b 协议、L8 providers、L9 server/vcs、L10 MCP、L11 TUI 核心、L12 TUI 簇、L13 CLI/ACP、L14 schema/client、L15 core 杂项）落地并 fast-forward 合并 main，逐条 ✅。每 lane 独立 worktree（`.worktrees/gb12-*`，合并后即时回收）、dev-gate 单入口、nextest 未过滤证据、每 lane workspace 合并门复跑（日志 `.test-logs/*gb12*`）；PTY 面 lane（L4/L11）另过 `--profile pty` 序列化门。**无上游阻挡项**（无 opentui-rust 未移植功能依赖；全部有界面为文档化决策而非搁置）。工具面随轮硬化两件：dev-gate 结构化禁裸 cargo test（`c4aa35ae`）与负载门 6→8（`47cd5ab1`，owner ruling）。

## 逐 lane 落地清单

- **lane1 `afea43cd`（跨 crate 快赢）**：G-336/339/340/341/344/349/351。
- **lane2 `095a7d32`（oc-core engine）**：G-362a/b、G-363（回退链尾设计修正）、G-364（rpc 门语义）。
- **lane3 `a4799e1e`（插件契约断线对）**：G-337 WS seams 全链消费、G-338 permission.evaluate 生产派发（managed-policy+插件 hook ref 序组合）。
- **lane4 `35bfd4b1`（PTY 族，+pty 门）**：G-335 增量解码、G-383a-f（teardown/谈判/UTF-16 口径/exitCode/锁外 IO）。
- **lane5 `86165976`（plugin-host）**：G-334 沙箱逃逸面（__rawFetch 消除+eval/Function 置换+作用域影子）、G-365 安装原子性、G-379 宿主簇 7 项。
- **lane5b `2a8fee08`（guest 调用面）**：G-379.7 rpc.invoke 客户面（共享 dispatch）、G-379.8 vcs/worktree 读+操作端口化（HTTP/插件同核）。
- **lane6 `dd1f0a6f`（媒体，范围按勘误调）**：G-345 unsupported 强制、G-374 jitter/边界（cancel 取证已正确）、G-347 残余（decode 严格化×10+薄 client 门面）。
- **lane7a `08d620ae`（协议聚焦）**：G-346 分类先于截断、G-357 compaction 严格化、G-366 interactions 形状/lenient。
- **lane7b `83d0b84b`（协议/管线簇）**：G-371（AWS 凭证链四腿优先）+G-372（pending 序/complete 时机）。
- **lane8 `941ebcd7`（providers）**：G-348 cf-ai-gateway 原生车道、G-356 vertex 守卫+token 拆分、G-373 五项。
- **lane9 `405af21c`（server/vcs）**：G-350 vcs.init 插件派发、G-361 git 族（含 AM 映射审计文本订正）、G-375 簇+fs find fuzzysort 移植（G-383⑦ 并案）。
- **lane10 `1bc113f3`（MCP）**：G-358 分页跟随、G-359 运行期刷新+NeedsAuth、G-360 三子项。
- **lane11 `089a8a2d`（TUI 核心，+pty 门）**：G-342 stdin 串行化、G-343 pin 接线+持久化、G-352 live reducer 族、G-367 tps 默认（零金帧）。
- **lane12 `6529e522`（TUI 簇）**：G-368/369/370/381（28 子项：20 修、8 有界/无差取证；零 oracle 重钉）。
- **lane13 `14c8c5f0`（CLI/ACP）**：G-353/354/355、G-377、G-378（11 修 4 无差）。
- **lane14 `97629310`（schema/client；rebase 后 ff）**：G-376（SSE 复用门为最大项）、G-380（BTreeMap 键序按 R-03 有界）。
- **lane15 `161e6d99`（core 杂项簇；rebase 后 ff）**：G-382（8 修 3 无差 2 有界，含 scrub 前提证伪）。
- **工具硬化（主检出直提）**：`c4aa35ae` dev-gate 结构化禁裸 cargo test；`47cd5ab1` 负载门 6→8。

## 勘误与前提订正（对第二十二轮审计）

1. **G-347 布线前提失实**：ref v2.0.24 无 /api/media HTTP 面（protocol 组清单枚举无 media、无生成 SSE；媒体=packages/ai SDK 库层经 AIClient 暴露、monorepo 零消费者）——「ref：/api/media 路由族完整消费」不成立，按 G-17 AIClient SDK 面同类裁决处理；可行动残余（decode 宽松+client 门面）已落地。
2. **G-361④ 审计文本失实**：ref git.ts:171-178 状态矩阵实为 AM→**added**（登记称 ref 映射 modified）；按代码真值对齐。
3. **G-374① 断言不成立**：cancel 无 CancelSpec 时本就 UnsupportedOperation（route.rs 现行实现），非 no-op 反转；证据关闭。
4. **G-382⑪ 前提不成立**：ref v2.0.24 无 instruction scrub（仅 integration-attempt scrub 与 transfer sanitize，均已移植）；「不中止在途交换」为 ref 本征行为。
5. **取证无差族（勿再立项）**：mistral converse 哈希（无 8 位截断）、thinking delta 五路径、partial-json NaN（serde 不可表示+fallback 等价）、轻校验/路由字段（makeRouteLanguageModel 两查已在）、D-Metric（ref 路由层双侧皆无）、--param 重复键（ref Object.assign 后写胜）、pairing 逐出（TTL+FIFO≡LRU）、shared-events 4096 off-by-one（第 4097 个事件即错=一一对位）、subagent 回扫（首完成即 NO_TEXT）、SIGHUP epilogue（异步信号安全超集）、deny_unknown_fields（ref promise client 本就不校验）、G-379 sidebar 可点击（L5b 取证在位）、G-378 child-session 创建（ref 本就仅服务端）。

## 有界偏离（本轮新增，均已代码注释/落地注记）

G-334c 全局 Bun/process 面（Bun 钉死 non-writable/stdio 依赖——归 G-247 结构隔离后续）；G-335 确定非法序列即替换（流式 TextDecoder 语义）；G-338 hook Err 保 pre-trigger 裁决；G-343 键位面（ref footer 仅提示）；G-347 client 门面 `'_'` 借用+RandomState 随机（非加密）；G-352 model-switched 行 previous:None（wire 无活源）；G-358 分页 1000 页上限；G-359 token 换牌=重连；G-363 回退仅叶子 resolver；G-364 rpc.close=per-location 门（G-289 单 guest）；G-367 组件默认未动（零金帧）；G-368 fs.watch 活面/kitty 探测门控；G-370 mount 权重/tok/s 基准（time.streamed 未上 wire）/RetryProvider 渐变；G-371 多跳 assume-role 链有声失败+bedrock_invoke_route 超集钉定；G-372 send_text 30s 防梗塞界；G-373 无 key 声明包保持匿名；G-375 events.persist 默认开（ref CLI 缺省关、端口 event-log 消费 load-bearing）+fuzzysort 无音调折叠/平手输入序；G-376 subscribe/on 同步化；G-379.7 `ctx.rpc.client(definition)`（ref 为 callable ctx.rpc）；G-380 BTreeMap 键序（R-03）；G-382 replay 无逐型 schema 解码（DEVIATION 盯守）；G-383 persistent_pty 字节环/chunked-replay opt-in 谈判（超集仅对声明客户端）。

## flake 白名单/观察名单勘注（2.2#4）

- **oc-pty `second_subscribe_on_a_connection_is_rejected` 族（含 `subscribe_connections_end_at_stream_end`）入白名单**：gb10-lane10/16/17 三见后又于 gb12 L4/L13/L14 门内四见（均 try-N 重试或 --exact 复现通过）——满足「复发三次以上再议」条件，正式入白名单（重试即过、勿误判回归）。
- **新增观察名单**：`scenario_config_touch`（oc-server，L12 门内首见——frame 19 金序列分歧 command.updated vs skill.updated，try-2 过）、`parent_up_returns`（oc-tui pty_session_route，L6 门内首见，重试过）、`provider_refresh_engine_boot_loop_updates_and_publishes`（第二见，R18 已观察在册）。复发三次以上再议白名单。
- 本轮全部 lane 门证据（含白名单/观察名单 --exact 复现日志）在 `.test-logs/*gb12*`。

## 制度速查订正

- **2.1#1**：负载门 1-min load **≥8** 拒启动（owner ruling 2026-11-11，`47cd5ab1`；原 6——稳态常驻 6.x 频繁卡门）。
- **2.2#1 补强**：dev-gate **结构化拒绝**裸 `cargo test`（exit 4；`c4aa35ae`）——canonical harness=nextest 由脚本强制，唯一豁免 `--exact` 单测复现（引号内嵌套调用同受检）。tee 铁律（2.1#3）仍为纪律面（tee 在调用方管道、dev-gate 参数外不可结构化），L14/L15 两起违规均当日 steer 纠正+nextest 重跑取证。
- **2.3 补注**：lane 分支若因主检出工具提交前移而非最新基，rebase 后 ff（L14/L15 先例）；lane 报告日志路径一律以盘上 `ls -t` 实测为准。

## 第二十三轮判定

G-334..G-383 全部 ✅（含 4 项审计勘误撤销/订正、取证无差族 13 项以证据关闭）；有界偏离 30+ 项就地登记；G-347 布线前提经双侧源码证伪按勘误处理。现存未关闭项=既有文档化决策族（G-12/G-16/G-17/G-28/G-32/G-38/G-39/G-40/G-49/G-161①/G-163/G-164/G-200②/G-211(已 OBSOLETE)/G-212/G-238/G-239/G-240/G-247 残余/G-289/G-290/G-313..G-318/G-326/G-328/G-329/G-331..G-333）+ 本节新增有界项。**无上游阻挡项。**

---

# 第二十四轮（2026-11-12，census 机械化普查立项 G-384..G-390）

> 方法：14 通道机械普查（`tools/census/` 14 个幂等脚本 ← `http_surface.py` 模板 + `common.py` 共享库；`docs/audit/census/` 14 份报告 + INDEX.md）。每通道三件套：双侧权威自报对拍（spec/枚举/注册表 dump）→ 生产面静态提取（测试区剥除、多行解析、归一化声明）→ 已知锚交叉验证。全部脚本连跑两次逐字节一致；差异项经 orchestrator 逐项亲读双侧当前代码复核后裁定。**14 通道总覆盖率 100%**（双侧并集逐项归属 BOTH/REF_ONLY/RUST_ONLY/已登记裁定）。11/14 通道 REF_ONLY=0。

## 🟢 低

### G-384 ✅ ctx.integration.transform 的 editor CRUD 残余半边（G-24 后续）
- ref：`packages/plugin/src/promise/adapter.ts:386-400` transform(editor) 回调暴露 `{list, get, update, remove, method:{list, update}}` 全 CRUD。
- rust：`host.js:1551` 仅有 `integration.method.update`（=G-24 已修半边，authorize/refresh 包装在内）；transform 键与 editor 的 list/get/update/remove/method.list 全缺。
- 影响：插件无法枚举/修改/删除集成方法定义（只能 update 单个）。
- 修法：host.js integration 命名空间补 transform + editor 表（复用 G-379.7 vcs 读面同型模式）。
- 出处：census PLUGIN-API（docs/audit/census/PLUGIN-API.md）。
- **2026-11-12 第二十五轮收口 ✅（gb13-laneA `b97f6ee5`）**：transform editor 全 CRUD 落地——guest 编辑器（list/get 复用 integration.list/get、method.list 新读面、update/remove/method.update/method.remove）→ 物化 `integration.transform` 同步臂 → registrations `merge_integrations` 插件归属表（卸载保留清除）→ engine location 激活环（PLUG-D07 过滤）→ `apply_plugin_integrations` 落 IntegrationHost 写手（update_integration/set_integration_metadata/update_method/remove_method/remove_integration）+ notify_updated；method.update 原文复用 G-24 hook 派发（`integration/authorize|callback|refresh`），无第二套闭包转运。bun guest e2e（transform 往返/创建缺省 name=id/删除/method.list/dispose 回撤/integration.updated）+ 单测。
- 有界偏离（评审登记）：①注册期物化（ref 为读时惰性重放，G-5 先例）；②注册/卸载效果落在下一次 location 激活 pass（G-24 integration_methods 同型）——含 integration.updated 发布时序与 removal 标记存活窗（评审 F1/F2，零现役消费方）；③editor.list 返回完整 Info[]（ref 为 Ref[] 超集）。

### G-385 ✅ ctx.integration.reload guest 直调面（后端在、键缺）
- ref：`adapter.ts:426` `reload: () => run(host.integration.reload())`。
- rust：后端等价物在——`oc-core/src/integration/mod.rs:213-220` `reload_catalog`（注释自证 v2 `ctx.integration.reload()` 对应，`models-dev.refreshed` 自动路径已接）；`host.js` integration 命名空间无 `reload` 键。
- 修法：host.js 补 `reload: () => hostRequest("integration.reload", …)` + supervisor 派发（半小时级纯接线）。
- 出处：census PLUGIN-API / RPC。
- **2026-11-12 第二十五轮收口 ✅（gb13-laneA `b97f6ee5`）**：host.js `integration.reload` 键 → supervisor 异步臂 → EngineIntegrationPort `"reload"` → `reload_catalog`（catalog 来源同 models-dev 自动路径）+ `notify_updated()`（ref reload 必发 integration.updated，integration.ts:355；models-dev 拉取维持自动路径不重触）。单测钉发布。

### G-387 ✅ OPENCODE_LOG_LEVEL 运行时日志级别旋钮缺失
- ref：`packages/util/src/observability/logging.ts:165` 运行时读 env 调日志级别。
- rust：生产代码零读取（仅测试区出现）——env 旋钮面缺口（rust 日志级别如何配置另行核对，至少 env 面不等价）。
- 出处：census ENV-VARS。
- **2026-11-12 第二十五轮收口 ✅（gb13-laneB `ec4897c2`）**：`oc_util::logging` 落地——`Level{Debug,Info,Warn,Error}`、`minimum_log_level()`（OPENCODE_LOG_LEVEL 大写映射，非法/缺省→INFO，OnceLock；ref logging.ts:164-173 逐字）、`log(level,line)`=文件恒写（sink 自 oc-server errors.rs **字节级搬迁**：路径/append/trim 50/25MiB/陈旧锁/OPENCODE_LOG_FILE 覆盖全保，oc-server 侧 re-export 调用方零变化）+ stderr 仅 `OPENCODE_PRINT_LOGS=="1"`（逐次读，ref 同型）。路由面=有 ref logWarning 对应位的站点：errors.rs tracing_like_warn/debug_cause（其注释自认的偏差就此闭）、oc-core engine models-dev+skills_url、server_process_env 警告；oc-ai 断点告警（G-309 位）经本地 `warn_gated`（oc-ai 不依赖 oc-util，G-162② 先例）。有界偏离：oc-ai 侧无文件 sink（SDK crate 不可达 host roots，代码注释）；~85 站 eprintln 全量清扫范围外。预期行为变化（parity 方向）：被路由站点默认 stderr 静默、改入日志文件。

### G-388 ✅ TERM_PROGRAM_VERSION 终端版本探测缺失（口径差待核）
- ref：`packages/tui/src/util/system.ts:17` 读 TERM_PROGRAM_VERSION 参与终端类型/版本判定。
- rust：未读；TUI 有自有 kitty/ghostty 探测（G-368 簇）——探测策略差异是否产生可观察行为差（如版本门控的转义序列选择）待行为通道核。
- 出处：census ENV-VARS。
- **2026-11-12 第二十五轮收口 ✅（gb13-laneB `75d746a7`）**：核验结论——ref 的 TERM_PROGRAM_VERSION **无任何行为门控**（全树唯一消费=describeTerminal 拼串，喂 debug 对话框行与 buildIssueURL 的 `terminal=`/`os=` 参数，error-component.tsx:221-233）；rust 各能力面全部探针驱动无版本门。按信息面 parity 落地：oc-tui `system.rs` `describe_terminal()`（system.ts:15-19 逐字：TERM_PROGRAM‖TERM‖"unknown" + ` ${TERM_PROGRAM_VERSION}` + " in tmux"/" in screen"）+ `describe_os()`（linux 读 /proc/sys/kernel/osrelease，他平台省 release=有界）接入崩溃 issue URL `terminal=`/`os=` 参数（编码对齐 URL.toString()）。TMUX/STY 由此进入 describe 串（信息面），D3-7 bounded 面不受影响。

## ℹ️ 登记

### G-386 ℹ️ 插件 RPC/ctx 桥层形状差打包（ref 14 读面 + rust command.add 扁平化）
- ref 经桥派发而 rust 于 guest 本地实现的 14 项读面：`agent.get/list/reload`（adapter.ts:296-299）、`command.list/reload`（:308,:322）、`integration.reload`（:426，直调面单立 G-385——**已于第二十五轮收口 ✅，本清单实际余 13 项**）、`mcp.list/reload`（:434,:436）、`skill.list/reload`（:455,:457）、`tool.list/reload`（:467,:466）、`websearch.reload`（:542）、`worktree.reload`（:563）。rust 对应能力在 host.js 编辑器表/本地表（行为面等价性归行为/差分通道复核；`vcs.transform`/`vcs.reload` 已于 G-24 lane5 清偿，不在列）。
- 反向：`ctx.command.add`（host.js:1061）为 rust 对 ref transform editor 之 add 的扁平化直调（transform 两键并存）——形状超集，登记不改码。
- **2026-11-12 第二十五轮复扫补注（census）**：G-384 修复新增同型扁平化键并入本打包——`ctx.integration.list/get`（host.js 编辑器直调经 `integration.list`/`integration.get` hostRequest）与 `ctx.integration.method.list`（:1630，ref 仅在 transform editor 内暴露 method.list）——RPC 通道 RUST_ONLY 23→25、PLUGIN-API 28→29 均系此因，无行为缺口。
- 出处：census RPC / PLUGIN-API。

### G-389 ℹ️ keybind rust-extra 8 动作打包（超集，登记不改码）
- `composer.close`（table.rs:915）、`composer.subagent.toggle-activity`（:902）、`composer.tab.next/prev`（:917/:916）、`dialog.select.section.next/prev`（:829/:828）、`stats.open/close`（:641/:642）——ref `Definitions`（keybind.ts）无此可绑定键；ref 侧 stats 为运行时命令（stats.tsx:197）未开成键位。用户配置面超集，无破坏性。
- 出处：census KEYBINDS。

### G-390 ℹ️ experimental.file_retention.cleanup_interval rust 加固旋钮（超集键）
- rust `oc-schema/src/config/experimental.rs:11`；ref `experimental.ts` 无此键——rust 独有的清理间隔调优面，登记不改码。
- 出处：census CONFIG-KEYS。

## 归类与机械化复认（不另立项）

- **G-295**（rpc.events.emit 通路）：PLUGIN-API census 显现为 ref 独有键（adapter.ts:77），登记册 STILL_OPEN 状态与代码一致。
- **G-230**：见上文勘注（0/18，NOT_YET 延续，census 盯守）。
- **G-239/G-331/G-332/G-148②**：ref 插件 tui 入口面 13 键（Context interface）在 PLUGIN-API 轴 2 全数显现为 REF_ONLY，与各条登记一致（plugin_slots.rs:46-48 现场复核）。
- **D3-7/tui-full-convergence 终扫**（本册 :1300）：TMUX/STY/OPENCODE_NODE_ASSETS_DIR 维持有据 bounded，census 归类一致。
- **G-174**：`providers.models.compatibility.requireSignature` 键为 G-174 修复产物（CONFIG-KEYS census 归类）。
- **G-10/G-383b**：persistent-pty wire rust 超集 op（`read` wire.rs:333、`restore` :361）与 `replay_complete` tag（:459）census 显现一致，双侧版本钉定 v7（persistent_pty.rs:509 ↔ daemon.ts:8）。
- **G-373 系（round-23 决策族）**：provider 3 个 rust-native 入口（`amazon-bedrock/mantle/messages` facade.rs:119、`xai/chat`、`anthropic-compatible`）——ref 走 aisdk/npm 桥或不在 builtins 表，rust native 化为已裁决架构分歧。
- **RC-11**：CLI `completion` 命令 rust 独有，既有背书维持。

## 旧锚修正（勘正，勿沿用旧数字）

1. **"52 会话事件"→实为 51**：EVENTS census 三轴（ref union tag ↔ rust serde 枚举 ↔ event_type() 匹配臂）双侧均 51、集合一致；"52"系旧口径分桶计数，非差异、非提取漂移。
2. **"keybind 230/230"→243/243**：ref `Definitions` 已演进至 243 个可绑定动作，rust 同步 243 + 8 超集（G-389）。
3. **"~180 catalog-only 推定"→确凿 226 = 49 BOTH_NATIVE + 168 BOTH_COMPAT + 9 REF_AISDK_ONLY**：第二十二轮盲区#2 销项；9 家 npm 桥（aihubmix/gitlab/merge-gateway/perplexity/qvac/salad-cloud/sap-ai-core/v0/watsonx）为登记册豁免的 typed `UnsupportedProtocol`（facade.rs:2151-2156）。

## 第二十四轮判定汇总

census 把"缺口扫描"从自由读码改为可度量普查：14 通道覆盖率 100%、11 通道零缺口、新立项仅 7 条（🟢×4 可修 + ℹ️×3 登记）——**这正是收敛的证据：枚举面已不再产出大批新缺口，剩余缺口密度集中在插件 ctx 桥层与两个 env 旋钮**。修复优先序：G-385（纯接线半小时）> G-384（vcs 读面同型）> G-387 > G-388（先核行为差）> G-230 die 前置（1-2 天中等 lane 维持 R21 建议）。本轮起扫描循环的操作定义=14 条幂等命令 + INDEX 复核；上游 bump 工作流=重跑 census 取轴 1 diff → diff-scoped lane。工具与报告：`tools/census/`、`docs/audit/census/`（INDEX.md 为总索引）。

---

# 第二十五轮 burndown 收口（2026-11-12，gap-burndown-13）

> 第二十四轮立项的可行动项 G-384/G-385/G-387/G-388 + 第二十四轮优先序末位的 G-230 die 前置（R21/R24 建议：先补 18 内建 Output 声明再启 die 分支），经 lanes A-C 串行落地并 fast-forward 合并 main。每 lane 独立 worktree（`.worktrees/gb13-lane*`，合并后即时回收、分支已清）、dev-gate 单入口、nextest 未过滤证据、每 lane 合并门复跑、独立评审（oracle）过门后合并；Lane C 评审 REQUEST_CHANGES 一轮（校验器 `const` 空转）→ fix round → 复审 ADDRESSED-APPROVE 后合并。一切门命令全量 tee 落盘 `.test-logs/*gb13*`（含 e1 白名单隔离复跑证据）。

## 逐 lane 落地清单

- **laneA `b97f6ee5`（插件 integration ctx 桥）**：G-385（`ctx.integration.reload` guest 面：host.js 键 → supervisor 异步臂 → EngineIntegrationPort `"reload"` → `reload_catalog`+`notify_updated`）；G-384（`ctx.integration.transform(editor)` 全 CRUD：guest 编辑器+物化同步臂+registrations `merge_integrations` 插件归属表+engine 激活环应用+notify_updated；method.update 原文复用 G-24 hook 派发；method.list 新读面）。合并门 2299/2299（`.test-logs/final-20261112-011601-gb13laneA.log`）。评审 APPROVE（F1/F2 时序偏离按 G-24 先例登记有界，见 G-384 条目）。
- **laneB `ec4897c2`+`75d746a7`（env 旋钮）**：G-387（`oc_util::logging`：level 旋钮+共享文件 sink（oc-server errors.rs 字节级搬迁）+stderr 逐次 PRINT_LOGS 门；路由 ref logWarning 对应站点；oc-ai 本地 `warn_gated`（无文件 sink=有界））；G-388（describe_terminal/describe_os + 崩溃 issue URL `terminal=`/`os=` 参数；核验 ref 无版本行为门控）。合并门 4911/4911（`final-20261112-014542-gb13laneB.log`）。评审 APPROVE（4 minors 入台账）。
- **laneC `80995995`+`58174fd4`+`7be6317b`（G-230 die 前置+die 分支）**：18/18 内建 Output 声明（ref-exact 闭对象）、BuiltinTool.output 字段、die 分支启用（ref 原文）、shell `shellID` 输出改名+前台 metadata 对齐、codemode execute ExecuteOutput 全量 parity+外层 content file parts、ToolDefinition.outputSchema 上定义面（协议侧与 ref 同剥）、校验器补 `const`。工作区门 5881/5881 全绿（`workspace-1794427105-gb13laneC-fix1.log`；e1 白名单放行依据=隔离 try-2 过+基线单跑过）。评审一轮 REQUEST_CHANGES→fix→复审通过。
- **census 随轮更新（主检出直提，无 crate 面）**：`tools_registry.py` 提取器随内建 output 新形状重写（轴 3 动态化：per-tool 声明计数+die 分支在位检测，幂等复跑逐字节一致）；HTTP-SURFACE 手写「验证记录」节再生成后回贴（该节为手工维护面，复跑需回贴——已在 INDEX 注明）。

## census 复跑对账（14 通道全跑）

- ENV-VARS：REF_ONLY 63→**59**、BOTH 72→**76**（OPENCODE_LOG_LEVEL/TERM_PROGRAM_VERSION/TMUX/STY 转 BOTH；后两者为 describe 串信息面读取，D3-7 bounded 不受影响）。
- PLUGIN-API：REF_ONLY 3→**1**（integration.transform/reload 转 BOTH；余 `rpc.events`=提取器对运行时构造 rpc 域的口径限制，G-295 ✅ 在册背书）；RUST_ONLY 28→29（integration.method.list 新读面，G-384 产物）。
- TOOLS：轴 3 翻绿——rust 18/18 声明+die 在位（G-230 盯守项就此闭合）。
- RPC：integration.reload/method.list 新增 rust 面（行锚随漂移）；其余 10 通道零变化（CONFIG-KEYS/HTTP-SURFACE 仅锚点行漂移/手写节回贴）。

## 有界偏离（本轮新增，均已代码注释或条目登记）

- G-384：注册期物化（ref 读时重放）；注册/卸载效果落下次激活 pass（含 integration.updated 时序与 removal 标记窗，G-24 同型）；editor.list Info[] 超集；跨插件同 id 归属边角（评审 F3，台账）。
- G-387：oc-ai warn_gated 无文件 sink；~85 站 eprintln 全量清扫范围外；被路由站点默认 stderr 静默=预期 parity 变化。
- G-388：describe_os 非 linux 平台省 release。
- G-230：die 以 ToolRunFailure::Error 承载（Effect.die 缺陷语义无对应变体）；session_move 以 schema-only 覆盖（其余 17 工具真执行验证）；MODULE_LOADING_REJECTED 早退腿合成 output=注释自证扩展。

## 上游阻挡项（按用户指示标记搁置）

- **G-247 残余（全局 Bun/process guest 直达）**：Bun 运行时将全局钉为 non-writable、替换 `globalThis.process` 断 Bun stdio——依赖 Bun 上游能力面，非本移植可解（详见 G-247 条目 2026-11-12 标注）。无 opentui-rust 依赖项。

## 第二十五轮判定

G-384/G-385/G-387/G-388 ✅；G-230 die 前置+die 分支 ✅（census 盯守翻绿）；G-386 注记（14→13 项）；G-389/G-390 维持 ℹ️ 超集登记。现存未关闭项=既有文档化决策族（G-12/G-16/G-17/G-28/G-32/G-38/G-39/G-40/G-49/G-161①/G-163/G-164/G-200②/G-211(已 OBSOLETE)/G-212/G-238/G-239/G-240/G-247 残余(上游阻挡)/G-272⑩/G-289/G-290/G-313..G-318/G-326/G-328/G-329/G-331..G-333/G-386/G-389/G-390）——全部为文档化决策/登记项，无可行动缺口。**无 opentui-rust 依赖的上游阻挡项。** 证据：lane 报告 `.superpowers/sdd/gb13/`、门日志 `.test-logs/*gb13*`。


## 第二十五轮复扫注记（2026-11-12，census 独立复扫）

- **复扫结论**：14 通道全量重跑，12/14 与修复方刷新后的报告逐字节一致（HTTP 附录与 ENV-VARS 行号漂移为预期差）；G-384/G-385/G-387/G-388 与 G-230 die 面**经代码级独立确认**（host.js:1641/:1651、oc-util/logging.rs:54、oc-tui/system.rs:47——TMUX :51 一并接线、超出翻绿清单；registry.rs:422 die 分支在位、18/18 声明，oc-core tool::registry 17/17 PASS）。PLUGIN-API 现 BOTH 95 / REF_ONLY 1（rpc.events=G-295 维持）；RPC BOTH 76 / REF_ONLY 13（integration.reload 转双侧）。修复轮新增 3 个扁平化键并入 G-386 注记。
- **flake 观察登记（一次性）**：`oc-plugin-host::g384_integration` 于本次复扫中出现单次失败（同调用内 2 try 均 ~2s 快败），随后单跑与原过滤条件合计 **18/18 PASS（含 10 连压测）**，不复现。假设：测试前四个文件读（reload/list/methods/got）为无轮询立即读，`wait_ready` 只保证宿主就绪，guest 写文件存在竞窗（done.json 已用 wait_for，前四读未用）。建议低成本加固：前四读同样包 `wait_for`。按白名单纪律先登记观察，两次独立复现再升级处置；**不构成产品缺口**。

---

# 第二十五轮增补收口（2026-11-12，gap-burndown-13 amendment：G-239/G-331/G-332/G-333/G-272⑩）

> 用户下令将登记册中登记/搁置态四项转修复。经 lanes H/I/J/K 串行落地：每 lane 独立 worktree（`.worktrees/gb13-lane{H,I,J,K}`，合并后回收、分支已清）、dev-gate 单入口、nextest 未过滤证据、每 lane 合并门复跑 + 独立评审（oracle）过门后合并；Lane H/K 各一轮 REQUEST_CHANGES→fix→复审通过。Lane K 跨仓：opentui-rust 三提交直落其 main 并推送（git rev pin 解析所需，G-177 先例）+ oc 侧 rev bump；**opentui 差分 127 IDENTICAL 零重钉**（可证：无场景构造 ExternalOutputCommit）。一切门命令全量 tee 落盘 `.test-logs/*gb13*`。

## 逐 lane 落地清单

- **laneH `c9c7afb6`+`4b93504d`+`78cbda37`（G-332 markdown 渲染器注册链）**：guest `ctx.markdown.registerCodeBlockRenderer`（ref 逐字错误文案/dispose 幂等/G-293 快照镜像）→ supervisor 臂 → registrations 表 → facade `render_markdown_block` → oc-tui `plugin_markdown.rs` 分派表（归一键控、**插件表先于 latex/mermaid**=ref last-wins）+ 内容哈希缓存 + offload 异步升级 + `BlockSpec::CodeBlock` 接入；空表默认路径字节恒等。合并门 1769/1769。评审 fix round：websearchExecutors 快照行误删复原 + in-flight 清理 + 缓存失效钉定测试。
- **laneI `34550de6`+`b6d66a5f`（G-239/G-331 panel 激活）**：session.panel 入 MOUNTED_SLOT_PATHS；右侧 pane（panel>terminal>sidebar 优先级、terminal 互斥、>80 docked/窄幅 fullscreen、路由切换关闭）；guest `ui.panel.open`（deferred 布尔回执）/close（owner 释放）/current（tui.panel 推送）；PanelInput props 指纹重物化；KeyPipeline panel_focus_live 臂+鼠标聚焦；PTY `pty_session_panel.rs`+CI 双 filter。合并门 2213/2213 + pty profile 1644/1644。
- **laneJ `06232e6c`（G-333 node 备轨）**：`GuestRuntime{Bun,Node}` 解析链（OC_PLUGIN_BUN 显式终决→**OC_PLUGIN_NODE 显式强制（置于 bun 链前——实现者裁决修正，否则有 bun 机器上恒空转）**→bun 链→PATH node→bunman 下载→禁用）；spawn 选轨；sandbox `package.json {"type":"module"}`；host.js BunShim（模块作用域，覆盖生产面全部 3 个 Bun API，不上 globalThis=沙箱面收窄）。真 node e2e×3 + 解析单测 6 行裁决表 + bun 套件不动全绿（541/541×2）。
- **laneK（G-272⑩ 样式化滚动回写）**：opentui-rust `a8e41e9`（ExternalOutputCommit styled 载荷 + transcript_bytes/row_widths 分派（SGR 行宽不数 ANSI）+ commit_rows_styled）+ `5a079fa`（PORT-MATRIX）+ `34a5ac4`（raw-mode CR 行宽修）；oc `fad7fd8`（rev bump 56a9eda→34a5ac4 + `mini/entry_styled.rs` 按 ref kind 映射样式化发射：markdown/代码高亮/entryLook 着色/oklab 量化）+ `6ae37997`（fix round：`set_mono` 实时达写入手（footer.ts:932 对位）+ filetype 委托 G-302 表（js→typescript 归一）+ 逐 chunk 粒度偏离文档化）+ `6fea30de`（fmt）。工作区 5907/5907 + pty profile 1973/1973 + 差分 127 IDENTICAL。

## census 增补复跑（14 通道全跑）

- ENV-VARS：RUST_ONLY 43→**44**（`OC_PLUGIN_NODE` 新增，G-333 产物有据）；EVENTS/PLUGIN-API/RPC 仅锚点漂移与新 rust 面（markdown.renderer.*/ui.panel.*），无新 REF_ONLY。
- HTTP-SURFACE 手写「验证记录」节再生成后回贴（维护口径同前）。

## 有界偏离（增补轮新增，均已代码注释或条目登记）

- G-332：消费侧归一（同插件 "ts"/"typescript" 不报重复、错误文案用原始名）；未归一语言静默死信；有界行形状（title 并入首行）；首帧默认渲染后异步升级；transcript 行 v1 不可点击。
- G-239/G-331：claim 内容 G-28 有界行形状；focus/toggleFullscreen 闭包不跨线（focus=宿主鼠标/键面，toggleFullscreen v1 省略）；面板行点击 v1 未接；面板 deferred open 无超时（与 dialog seam 共有属性，评审注记）。
- G-333：node 轨不可加载无扩展名相对导入/.tsx 条目；热重载 import-graph 正则保真；`globalThis.Bun===undefined`（更窄=更安全）；坏 OC_PLUGIN_BUN + 插件条目存在时仍走 bunman 下载兜底（既有语义，文档澄清）。
- G-272⑩：逐 chunk 独立渲染（ref 累计+高水位）；fallback_dark 主题基座（OSC 实时换肤范围外）；图片预览范围外；mono=true 旧 plain 路径字节恒等保留。

## 第二十五轮增补判定

G-239 ✅ / G-331 ✅ / G-332 ✅ / G-333 ✅ / G-272⑩ ✅（五项全部从 ℹ️/bounded 转 ✅）；G-28 生态 breaking 声明面收窄（见 G-28 增补轮勘注）。census 增补复跑无新 REF_ONLY。现存未关闭项=文档化决策族（G-12/G-16/G-17/G-28（收窄面）/G-32/G-38/G-39/G-40/G-49/G-148②/G-161①/G-163/G-164/G-200②/G-211(OBSOLETE)/G-212/G-238/G-240/G-247 残余(上游阻挡)/G-289/G-290/G-313..G-318/G-326/G-328/G-329/G-386/G-389/G-390）——均为登记项。**无 opentui-rust 依赖的上游阻挡项**。证据：lane 报告 `.superpowers/sdd/gb13/lane{H,I,J,K}-report.md`、门日志 `.test-logs/*gb13lane{H,I,J,K}*`、opentui 差分 `opentui-diff*-gb13laneK.log`。

## 第二十五轮增补复扫注记（2026-11-12，census 独立复扫）

- **复扫结论**：14 通道重跑，12/14 与修复方刷新后报告逐字节一致（HTTP 手工附录、ENV-VARS 背书计数 6→8 系登记册本轮新提及所致，非分歧变化，已刷新落盘）。**G-239/G-331、G-332、G-333、G-272⑩ 经代码级独立确认**：host.js:2095 `markdown.registerCodeBlockRenderer`（G-332 guest 面）与 :2301 `ui.panel`（G-239 激活面）、supervisor.rs:2758/lib.rs:935（session.panel 活态推送）、resolve.rs:14+（node 备轨 npm 缓存）、mini entry_styled（G-272⑩ 样式化滚动回写）。
- **运行时证据**：oc-plugin-host 全量 134/134 PASS（含 panel/markdown/node 族）、oc-tui entry_styled 12/12 + scrollback 14/14 PASS。
- **census 侧新增记账**：PLUGIN-API RUST_ONLY 29→31（`markdown.registerCodeBlockRenderer`、`ui.panel`——G-332/G-239 产物，ref 对应面在 tui 入口而非 server-ctx，属入口形状差非缺口）；ENV-VARS RUST_ONLY 43→44（`OC_PLUGIN_NODE`=G-333 旋钮，已由 435ee145 记账）。**REF_ONLY 无新增，缺口候选 0**。
- **上轮 flake 跟踪**：`g384_integration` 本轮复扫（oc-plugin-host 全量含该测试 134/134）未见复发；观察维持登记。

---

# 第二十六轮（2026-11-12，差分层通道 1：配置解析语义差分）

> 方法：同一 corpus（43 例，合法/畸形/边界/JSONC/未知键/v1 形状）喂双侧**纯函数管线**（jsonc-parse → normalize → Info decode），归一化（kind 小写、诊断排序、encoded 键序排序）后逐字段对拍。ref 侧 bun 驱动（staging + node_modules 链接，ref 仓库 git 树未动，`bun install --frozen-lockfile` 4833 包）；rust 侧 env 门控集成测试（`CONFIG_DIFF_CORPUS/OUT`，未设时零成本）。工具：`tools/diff/{config_parse.py,corpus/config/,drivers/config_ref.ts}` + `crates/oc-core/tests/config_diff_driver.rs`；报告 `docs/audit/diff/CONFIG-PARSE.md`。

## 结果

**MATCH 41/43 · DIVERGE 2 · MISSING 0**（覆盖率 43/43；json-parse/normalize 判定/诊断三元组/decode 判定四字段全对齐，含 malformed、JSONC 容错、未知键、大小写、重复键、UNSUPPORTED 族）。

### G-391 🟡 provider `Model.cost` 序列化缺 `skip_serializing_if` → 往返物化 `"cost": null`（wire 可观察）
- rust：`oc-schema/src/config/provider.rs:130-131` `cost: Option<CostList>` 是该结构体 13 个字段中**唯一**缺 `skip_serializing_if = "Option::is_none"` 的字段——normalize 的类型化往返（normalize.rs `decode_provider_entry` decode→re-serialize）把缺省 cost 物化为显式 null；`Info.providers`（config/mod.rs:134）内嵌同型，`GET /api/config`（handlers/config.rs:39 `serde_json::to_value(entry)`）在 wire 上为每个已配置 provider model 输出 `"cost": null`。
- ref：normalize（schema 变换，optional 保留缺省）与 GET config 均不出现该键（provider.ts:88 optional）。
- 影响：客户端可见 wire 差异；Info decode 不受影响（null 与缺省同归 None，decode_ok 双侧一致）；潜在语义面=任何把"显式 null"当"覆盖为免费/未知"的消费者。
- 修法：一行——`Model.cost` 补 `skip_serializing_if = "Option::is_none"`（保留 `deserialize_with`）；差分复跑即验证。
- 出处：diff CONFIG-PARSE `13-provider-models`。

### 归类与复认
- `16-experimental` 分歧（rust 保留 `experimental.file_retention.cleanup_interval`、ref 静默剥除）= **G-390** rust 超集旋钮的预期表现，有据分歧非新缺口。ref 剥除未知 experimental 键不发诊断——与 rust 行为一致（18-unknown-nested 对齐）。

## 第二十六轮判定

差分层首通道即产出 1 项 wire 级新缺口（G-391）+ 1 项既有背书复认（G-390），验证了"存在性全面 ≠ 等价性全面"的预判：census CONFIG-KEYS 曾确认键集与默认值对齐，但**序列化物化**只有差分能抓到。修复优先序：G-391（一行）→ 差分复扫。后续差分通道按成本升序：ACP 转译 → SSE store 投影 → JSON Schema 校验器对拍 → /compact 管线 → HTTP 全量。
