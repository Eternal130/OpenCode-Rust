# SDD ledger — plan: docs/superpowers/plans/2026-11-13-tui-fullscreen-diff-channel-8.md

Baseline: worktree @ 4ea5777a (CI run 43403302780 seven-job green) + docs commit; guards green (cold cargo suite skipped — HEAD pre-validated).

## Pre-flight conflict scan

| Pair/Task | Surface | Finding | Ruling |
|---|---|---|---|
| T2↔T3 | "tui-diff-grid/1" schema | identical pinned fields; T3 omits stub_gaps (ref-only) | T4/T5 treat missing stub_gaps as empty — carried in briefs |
| T1→T2/T3/T7 | key vocab in SCHEMA.md | must exist before driver key mapping | sequential order already enforces |
| T4↔T2/T3 | driver CLIs | env/argv pinned in Global Constraints | none needed |
| Guards↔T2/T3/T4 | process_safety scans new .py/.ts/.rs text | http_full.py killpg allowlist is path-specific — T4 must NOT copy kill_side | Orchestrator spawns children and lets them exit; teardown via own-child terminate/wait only, no killpg, no allowlist entry. Carried in T4 brief |
| clippy↔T3 | -D warnings on new rust file | doc-backtick discipline (recent CI history) | Carried in T3 brief |
| T7 plugin-panel scenario | ref fixture harness may not host plugin boot chain | possible infeasibility on ref leg | Contingency: if ref leg cannot host plugins, declare the scenario explicitly out-of-scope in report (spec anti-silent-exemption satisfied); G-28 first-round registration face then covered via G-332 code-block scenario. Ledger only if triggered |
| T5/T6 | same file as T4, sequential | fine | none |
| T8 | docs only | fine | none |

Scan clean after rulings. Execution order: T1→T2→T3→T4→T5→T6→T7→T8→T9 (SDD forbids parallel implementers).

Ruling: bun for channel-8 ref driver = ambient bun (1.3.10) — channels 5-7 ref drivers all ran ambient bun; the 1.4.2 pin is a release.yml web-assets concern (photon patch set in packages/app), not packages/tui fixture rendering. Cost if wrong: ref-leg rendering varies across bun versions — mitigated by per-scenario double-run byte-determinism gate (T2 Step 3) which would surface any such variance immediately.
Ruling: smoke scenarios keys=[] acceptable (T1); key-feeding paths still implemented in T2/T3 drivers; interaction scenarios arrive in T7.
Task 1: fix round 0 concerns resolved (branch attached; bun ruling recorded; keys note carried to T7 brief).
Task 1: fix round 1/5 (5 addressed, 0 open; commits 342318a1..76e0d52d)
Task 1: complete (commits 976ea485..76e0d52d, review clean after R1)
  minor (deferred): session-text/markdown events 缩进风格不一致（cosmetic，终审triage）
Ruling: /api/skill 含 "kill" 子串不构成 process_safety 违规（守卫按命令模式匹配，guards 绿为准）。
Ruling: driver 输出约定钉死——`--out <dir>` 目录，每 manifest case 一个 `<case-id>.json`（T3/T4 同此）。
Ruling: T2 顺带修正 T1 场景 wire-shape（user 顶层 text、assistant text.ended 终值）——T4 基于修正后文件，已同分支提交，无需回改。
Carried to T7: ctrl/alt 键经 kitty 修饰符编码（原始控制字节会误报），交互场景键序列按 SCHEMA.md 键名表 + 此编码约定。
Task 2: complete (commits 76e0d52d..40960ea4, review clean, Approved)
  minor (deferred): ① indexed 16 色板 ref driver 用 xterm 经典表 vs ref 自身 v1-migrate 表——Ruling: 双侧 driver 统一嵌 ref v1-migrate.ts:470-495 常量表（v2 全真彩，indexed 属防御路径；rust 侧 scrollback 量化确会产 Indexed）。carry T3。② 行末 wide-char follower 丢同 span 风格 + charWidth 边缘码位——T7 加 CJK/emoji 场景前复核。
Ruling: rust driver 输出与 ref 同构——TUI_DIFF_OUT 为目录，每 case 一个 <case-id>.json（与 ref driver 一致，T4 按此收）。carry T3/T4。
Task 3: spike ruling (a) 直调 spawn_connection/store.ingest_all 等 seam + App::frame()（file:line 在 driver 头注释）；drain_bus_commands 从活循环逐字抽出（app.rs +14 行，零行为变化）。
Ledger: 冻结时钟下 rust 静息帧含 time-0 spinner 字形——T4 首比时若 ref 侧冻结相位不同，按 G-49 簇裁决（预期分歧候选，非 infra）。
Ledger: 双侧 request_census 路径数不对称（rust 11-14 vs ref 21）——census 仅供参考不比对；只有 stub_gaps 是门。
Ops flag: 实现者为 dev-gate 身份把 /root chmod 711——控制者已改 700 + setfacl u:eternal130:x（若无 setfacl 则保留 711 并向用户报告）。
Task 2 followup: indexed 色板裁定落实（commit e8de14c6，v1-migrate 表双侧一致；smoke 网格零字节变化=无 indexed cell，行为不变）。T2 侧 deferred minor ① 关闭。
Task 3: complete (commits 40960ea4..ec6c6fe5, review clean, Approved)
  minor (deferred): ①活循环 inline 副本未收编 drain_bus_commands（零行为变化成立，漂移风险）→ T9 收编候选。②Pump::drain 注释 overstated（省略 env PUT/permission POST 等）→ T7 permissions/forms 场景需补泵。③"404 OK" reason phrase 纯外观。④排序 Rust 字节序 vs JS UTF-16 序（ASCII id 等价，理论性）。⑤grapheme cluster 整格 vs ref 逐 codepoint 展开 → T7 emoji 场景已知差异清单。
Ruling: manifest scenario 路径用 STAGE 绝对路径（rust nextest cwd 限制的合理适配，双侧 driver 等价处理）。
Ruling: STAGE chmod 777 → 改 1777（sticky）——共享生产箱防互删，carry T5 顺手修。
Ledger: 断桩 gap 明细在 driver stderr 透传，结构化汇总留 T5（可选）。
Task 4: implementer session ses_e2a27ed6affet2lYVinEaXeak6（fix 轮恢复用）。
Task 4: complete (commits e8de14c6..a22aac0c, review clean, Approved)
Ruling: T4 评审 Important#1（子进程无 timeout，挂死会占死 dev-gate 全局锁）+ Minor#3（宽高不校验）#4（空 corpus 静默绿）#5（build_manifest 双跑）#6（grid 缺 key 裸崩）全部并入 T5 必办清单——T5 本就重写同文件，独立修复轮纯冗余。成本若错：无实质成本（一行级修复）。
Carry to T5 (mandatory): ① 双侧 driver subprocess timeout（ref 600s / rust 2400s 含冷编译）；② grid width/height ≠ manifest 声明 → infra 红；③ 空 corpus → exit 1；④ load/report 用 .get 默认值防 KeyError；⑤ chmod 777→1777（:102-103、:213 三处）；⑥ run() 复用已建 cases 免双跑。
Carry to T5 (note): T5 Step 4 是首次双侧真比对——smoke 分歧是基线数据不是任务失败；记录 first_diff 明细，勿追修（裁决属 T9）。
Task 5: implementer session ses_e2a23b93dffeOIlR45wTYVX7ga。smoke 首比基线：6/6 diverge exit 2——home-empty 42格 'B' 字形差；session-* r0c0 fg [255,255,255]vs[238,238,238]（差17超容差）+ 610-892 格。T9 归因素材。
Ledger: tee 掩盖 exit code——后续跑通道用 PIPESTATUS 记录真实退出码（carry T6/T9）。
Task 5: complete (commits a22aac0c..0a372844, review clean, Approved; 11/11 单测、六必办全落实)
Ruling: T5 评审 Important#1（compare zip 截断无维度校验=坏网格假 exact 风险）+ Minor（size mismatch 语义矛盾 / verdict 缺 causes 形参）并入 T6 必办——同文件连续修改，独立修复轮冗余。风险窗口评估：双侧驱动输出稠密且 load 侧有字段校验，T6 在门生效（T9）前落地，可接受。
Task 6: implementer session 待记。
Task 6: implementer session ses_e2a1dd195ffelFP7fAjDuxC1pT。五计数（smoke）：总量6/EXACT0/STRUCTURAL0/CAUSES0/未裁决6，gate exit 2（真实码经 PIPESTATUS）。
Ledger: CAUSES 三簇 matcher 绑定场景 id 前缀约定（plugin-*/session-markdown-*/panel-*）——T7 命名必须遵守，T9 改名须同步 matcher。
Task 6: complete (commits 0a372844..1fd61f94, review clean, Approved)
  控制者复核：评审 ⚠️ ref 侧引用无法验证——已亲核 /opt/opencode-ref（mini/theme.ts:81 oklab、component/panel-host.tsx、plugin/markdown.ts 均真，评审员搜了复数路径误报）。
  minor (deferred): ①G-332 first_diff 字段序致 ch+attrs 同差时连带裁决（风险极低）②G-28/G-239 前缀簇吞该前缀一切 diverge=既定设计残余风险（plugin-* 真回归会自动绿灯，T9 残余清单）③adjudicate 重复调用纯效率。
Ruling: T6 Important（报告头缺 python3 复现行）并入 T7 顺手修（T7 重生成报告）。
Task 7: implementer session ses_e2a17ffdcffe3uqWEHCGApnLTl。首基线 40 case：EXACT0/STRUCTURAL0/CAUSES0/未裁决40。系统性首差 fg[255]vs[238] 屏级（Δ17）。
T9 必办清单（滚存）：①fg 255vs238 根因首查（疑似主题 text.base 解析差，单因可翻大片）②session-tabs 加载态振荡=移植缺陷候选（64 pump 不静息，场景卡列表态）③G-332 matcher 前缀 session-markdown-（尾杠）vs 现役 id session-markdown 不匹配——改 matcher 或加后缀场景 ④mini/plugin/panel 三登记面已显式范围外（预检预案命中：fixture 无插件 boot——G-28 登记面由 G-332 markdown 场景部分承担）。
Ledger: T7 顺手修 ref driver ctrl regex 量词 bug（^ctrl+→^ctrl\+，ctrl+p 此前无法编码）。
Task 7: fix round 1/5 (1 addressed, 0 open; commits cdc3aabb..2c0ac015)
Task 7: complete (commits 1fd61f94..2c0ac015, review clean after R1)
  minor (deferred): fetch-503-model 与 home-empty 同形（42 格 logo diff）——"两侧都渲染 503 态"还是"两侧都降级 home"待 T9 判读。
Task 8: complete (commits 2c0ac015..9490a029, review clean, Approved)
  minor (deferred): INDEX 通道 1-7 总计停留第二十九轮口径（687/101/2 vs 第三十轮 717/71/2）——既有债务，T9 终局登记时顺手刷新。
T9 策略裁定:先 oracle 诊断分族（40 分歧根因+修复面），再 fixer 分波修复，循环至 exit 0——避免实现者盲扑。
Ruling (W1 方向修正): F1 "Build pill" 误诊——ref core/src/agent.ts:18 defaultID="build"，Build 是 ref 服务端默认 agent；tui-dump golden 导出条件下列表含它（golden 期望 Build，实证 99.7% 吻合唯 Build 差）。通道 ref 腿 fixture 的 agent 桩不含 build=fixture 保真度缺口。处置=①回滚 home.rs 合成默认 agent 移除（恢复生产对齐+golden 复绿）②场景 agent 桩双侧对称补 build agent（镜像 ref 服务端默认）→ F1 族经场景修正翻绿而非改 rust。W1 的 breadcrumb/driver 部分保留（未破 golden）。
Ledger: dev-gate flock 自锁教训——编排器内部已包 dev-gate（rust 腿），外层不得再包 dev-gate（嵌套 flock 死锁→2400s 超时假象）；直接 python3 调编排器。
T9-W1: complete (commits a1b94833 + 7fd401ac, re-adjudicated per ruling; 40→36 未裁决, home-empty/fetch-503 双档 exact; oc-tui 1657 全绿 fmt/clippy 干净; 零 golden 重钉)。实现者会话已终止，控制者经 retest6 独立复核确认。
Ops 教训（W2+ 必守）：①编排器不可包在 dev-gate 内跑（内部对 rust 腿再调 dev-gate，flock 自锁→超时假象）——直接 python3 调；②/tmp/opencode/logs 属主保持 eternal130；③通道每轮 ~6-10 分钟（CPU 帽），实现者应在本地尽量批量改再跑通道，避免逐案重跑。
T9-W2 补: golden 定向裁定 = 3 screen goldens 为 rust 自录陈旧捕获（非 ref 导出），用户已批准 PARITY_UPDATE 重导（2026-11-14）；tree-oracle/digest 已按预授权重生成（7/7 IDENTICAL vs real @opentui）。commit d1726585。
T9-W3: complete (commits f0ef7cf9+0804fb70+572c3bec+b2688800)——30→12 未裁决（28 exact）；T-ord 16 全清（场景 wire 归一化=ref rows.ts 生产前提，重放事件剪除裁定在 w3 报告）；leader 和弦=ref harness 编码缺陷（kitty ctrl+x，rust 正确不动）；step.started created 重置=真 bug 修复（data.ts:843）；tool_detail error 行+raised rail、scroll viewport 差一。新失配 surf1_detail_states（fixture pin vs ref oracle，W4 分类）。
Ruling: W3 场景归一化（剪除已持久化 turn 的重放生命周期事件）合法——ref 生产 rows.ts 尾部迁移是 live-rows 伪影，双侧语义对齐靠归一化实现，理由链在 w3 报告。

T9-W4 终波: 12 diverge 全清（scrim 语义/palette suggested/Status 树/Open placeholder+driver project seam/Sessions chrome），40/40 EXACT gate exit 0，无新 G-ID；3 screen golden 重导（批准）+ surf1 按 ref oracle 重钉；emission digest 重钉 0x78409566ff2310e8；通道 3×幂等、1657/1657、fmt/clippy/guards 三绿；登记终态（GAPS 三十一轮/INDEX/TUI-FULLSCREEN）。commits db829a4a d999e986 c0bfa09c。报告 t9-w4-report.md。
