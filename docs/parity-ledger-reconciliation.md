# Parity Ledger 导入对账（B0-1）— 2026-10-18

> 审计通道：`tools/parity-import.py`（确定性、stdlib-only、fail-loudly）。
> 输入：`docs/parity-scan-2026-10-18/01..16` 明细报告 → 输出：`docs/parity-ledger.jsonl`。
> 种子基线：114 条 canonical 种子（IDs 与内容不可变；仅允许新增 `aliases`）。

## 1. 总量对账：407 raw → 367 ledger rows

| 项 | 数量 | 说明 |
|---|---:|---|
| raw 差异记录（解析器枚举，见 §2） | **407** | 16 份报告的明细行/条目全集 |
| → 已被种子 source_id/aliases 覆盖 | 113 | 无需新行 |
| → 折叠进种子（新增 alias） | 16 | 跨报告同根因（见 §4.1） |
| → 新建 canonical 行 | **253** | 字段按 PARITY-LOOP §3.2 |
| → 折叠进新 canonical 行（alias） | 25 | 跨报告同根因（见 §4.2） |
| **合计** | **407** | 113+16+253+25 = 407 ✓ |
| **最终 ledger 行数** | **367** | 114 种子（不可变）+ 253 新行 |

状态分布：`open` 357 / `needs-runtime` 10（种子 2：CRED-B3、AI-VERTEX-V2；新行 8：AI-V1、AI-COPILOT-C6、TUI-28、TUI-30、TUI-33、TUI-TD06、TUI-TD15、TUI-TD28 — 均为报告标注 UNVERIFIED/静态推断，notes 附运行时计划，未发明 repro 命令）。

严重度分布（全账）：S0×6、S1×108、S2×233、S3×20。簇分布：WIRE 71、TUI 55、CONFIG 44、TOOLS-EXEC 42、RUNTIME 33、AI 31、CRED 24、DB 23、PLUGIN 23、SURFACES 21。

## 2. 分报告计数 vs MASTER §0.2

| 报告 | raw（解析） | MASTER | Δ | 说明 |
|---|---:|---:|---:|---|
| 01 会话/runner | 38 | 34 | +4 | 报告自身总览（11 MISSING/2 STUB/2 PARTIAL/15 WRONG/4 EXTRA=34）少计了明细表实际行数（14/2/3/14/5=38；含 5 条 EXTRA 正向差异与 E-01 理论性条目，全部导入并标注） |
| 02 工具/执行 | 37 | 33 | +4 | 总览 33；§1.2 差异表+§2–§5 明细实际枚举 37（C9 无独立正文但携带严重度；E 组 metadata 条目计入） |
| 03 AI 协议 | 29 | 26 | +3 | 报告自计 26（S1×12+S2×14）；解析含 §3.7（copilot，16:C1 升 S1）、§5.2（abort 观察项）与 §3.6/§3.9/5.1 节级条目共 29 |
| 04 provider/鉴权 | 25 | 22 | +3 | §5 差异索引总表实际 25 行（S0×3+S1×9+S2×13）；总览统计表（S2=10）少计自身索引 3 行 |
| 05 HTTP API | 22 | 22 | 0 | F1–F22 一致 |
| 06 TUI | 34 | 34 | 0 | #1–#34 一致 |
| 07 CLI/config | 20 | 20 | 0 | F-1..F-14 + N-1..N-6；K-1..K-9 为交叉引用表（K-1/K-2/K-8 已是种子 source_id；F-1/F-5 作为 alias 折入 K-1/K-2） |
| 08 插件平台 | 23 | 20 | +3 | §4 明细枚举 23 条 D-记录（S1×10+S2×13）；MASTER 计 20（11+9）。以明细行为准 |
| 09 数据库 | 24 | 24→20 | +4 | §13 差异清单实际 24 行（S1×7+S2×16+EXTRA×1）；报告总览写 S1=6、MASTER 计 20。D23 为 EXTRA/有益加固（S3+notes 登记） |
| 10 表面 | 21 | 21 | 0 | P0×2+P1×4+P2×8+P3×6+平台 1（10 无机内 ID，按 §2/§4 锚点登记） |
| 11 core 杂项 | 26 | 26 | 0 | §16 行动清单 #1–#15 + §1 映射表中未被清单吸收的 11 行（map-1/5/6/7/11/13/14/16/17/21/22）；其余映射行（19/23–31/33–39）为清单已覆盖的交叉引用 |
| 12 HTTP 字段 | 40 | 108（12–16 合计） | — | H0+H1–H39；H0 为 §1 系统性发现（含 15 实例清单） |
| 13 config/env | 24 | — | — | §8 清单 24 行；3 行 `—` 汇总行为交叉引用（指向 07:N-1/N-4、06:#22 等），不另立记录 |
| 14 TUI 深扫 | 28 | — | — | TD-01–TD-28（TD-26 为 INFO「无独立差异」，S3 登记） |
| 15 事件 payload | 10 | — | — | EV-1–EV-10（EV-10 为跨 lane 纠错条目，双侧一致，S3 登记） |
| 16 copilot/media | 6 | — | — | C1–C6 |
| **合计** | **407** | **386** | **+21** | 12–16 子合计 108 = MASTER 108 ✓；第一轮 +21 全部为「报告自身统计口径少计明细行」，逐条可核对 |

MASTER 的 386 是各报告自报数字的合计；导入器以**明细行**为准（manifest 硬校验，缺行/多行即退出非零），因此 raw=407。所有 407 个 raw ID 均落入 ledger 的 `source_id`/`aliases` —— **零孤儿**（导入器内置断言）。

## 3. 同根因折叠明细

### 3.1 折入种子（16 条 raw → 13 个种子行新增 alias）

| 种子行 | 新增 alias | 根因 |
|---|---|---|
| AI-M1 | 01:K-06, 03:R1 | WS 传输整体 deferred（native compaction 不传 webSocket、openai-responses 默认通道） |
| AI-M3 | 03:V4 | google interactions |
| AI-M5 | 03:R2 | xai-responses 适配 |
| AI-M10 | 03:B1 | Bedrock SigV4 凭证链 |
| AI-COPILOT-C1 | 03:§3.7 | copilot responses lane（16:C1 凭证据升 S1） |
| PLUG-M02 | 08:D-23 | session.http/ws hook 无生产 dispatch 站点 |
| TOOL-C2 | 08:D-15 | shell.create.before 未派发 |
| VCS-3 | 05:F10 | /api/vcs/diff 形状不符 schema |
| CLI-K1 | 07:F-1 | serve --hostname 仅 IP |
| CLI-K2 | 07:F-5 | upgrade 方法集/更新通道 |
| SURF-BROWSER | 10:P0-browser | browser 工具 |
| SURF-LATEX-MERMAID | 10:P0-latex | LaTeX/Mermaid 渲染 |
| SURF-PLATFORM | 10:PLATFORM | 平台/分发矩阵 |

### 3.2 折入新 canonical（25 条 raw → 13 个新行）

| 新行 | source_id | aliases | 根因 |
|---|---|---|---|
| RUNTIME-R05 | 01:R-05 | 05:F11 | title 置空 → LLM 重生成 |
| AI-M6 | 03:M6 | 03:B, 03:B3 | bedrock-mantle messages 路由（Opus≥5.1 effort 门控） |
| AI-M7 | 03:M7 | 03:R3, 03:R4 | vercel gateway 协议（路由选项+元数据） |
| HTTP-F12 | 05:F12 | 15:EV-4, 15:EV-8, 15:EV-9 | 公共 SSE 事件集合多发（4+1） |
| DB-D14 | 09:D14 | 15:EV-7 | project.updated 载荷字段 |
| TOOL-IMAGE | 11:#5 | 01:P-03, 02:A6 | 图片 normalize/photon 全缺 |
| RUNTIME-REFERENCE | 11:#6 | 01:C-01, 11:map-13 | reference 物化 + 指导 stub + repo.clone |
| TOOL-SNAPSHOT | 11:#8 | 11:map-14, 11:map-22 | snapshot 捕获/diff/seed 行为分歧 |
| WIRE-VCS-DETAIL | 11:#9 | 11:map-16, 11:map-17, 11:map-21 | git/vcs 细节（branch/sync/index） |
| AI-MODAL | 11:#10 | 11:map-11 | modal 动态模型 |
| DB-WORKSPACE | 11:#14 | 09:D13 | workspace 生命周期服务 |
| RUNTIME-ENVIRONMENT | 11:#15 | 11:map-5, 11:map-6, 11:map-7 | environment 执行平面缝 |
| CONFIG-C9 | 13:C-9 | 13:T-2, 14:TD-24 | new-session-location 子系统 |

折叠判据：报告内显式互引（“同 M6 根”“并入 D3”“= 05:F12 细化”）+ fingerprint 复核（`domain|symptom-code|首个 ref/rust 文件`），并逐条读原文确认，未按标题去重。`dedupe` 复核：367 fingerprints，0 重复组。

## 4. 严重度口径

- 报告标注优先；MASTER 仅在明示重判处覆盖：**14:TD-16** 报告 S1（建议按 S0）→ 种子 TUI-TD16 已按 MASTER §0.3 定为 S0（种子既有裁定，本导入不改动）。
- MASTER §0.4 的“最接近 S0 六项”为建议性（01:S-01、01:L-01、03:5.1/04:B-6、05:F1、11:#1-3、02:C1）；种子保留 S1，本导入不改（种子不可变）。
- 03:V1 报告标 “S1（若 catalog 无完整 URL 则 S2；未证实）” → 取报告标签 S1 + `needs-runtime`（运行时计划在 notes）。
- 16:C1 将 03:§3.7 的 S2 重判为 S1（报告自身明示升级）→ 按报告标签 S1。
- EXTRA/正向差异（01:R-01/R-02/R-03/R-06/S-02、09:D23、06:#31、05:F21、13:C-4、15:EV-10、14:TD-26）：按 S3（或报告级）登记、status=open、notes 标注“登记备查/有益增量”；如裁定永久分歧后续转 `accepted`（需 oracle+用户 ADR，非本通道职权）。

## 5. 工具与不变量

- `tools/parity-import.py`：stdlib-only；固定日期 2026-10-18；无时钟/随机；稳定排序；`--check` 只校验不写。
  - **manifest 硬校验**：每份报告解析出的记录 ID 集必须与硬编码清单完全一致，缺行/多行立即 fatal（不静默丢弃）。
  - **幂等**：重跑前自动剥离本工具生成的行（notes 前缀 `导入自 `），从 114 种子基线重建；断言种子数=114。
  - **不变量断言**：ID 唯一、fingerprint 唯一（冲突时以 id 后缀消歧，绝不静默合并）、零孤儿（每个 raw ID ∈ 某 source_id/aliases）、种子除 aliases 外逐字段不变。
- `tools/parity-ledger.py` 最小修订（B0-1 授权内）：`owner` 允许为空串（导入行未认领，lane 认领时填写）；其余校验语义不变。
- 验证（本通道实际执行）：
  - `python3 tools/parity-ledger.py validate --paths` → `rows=367 unique_ids=367 errors=0`
  - `python3 tools/parity-ledger.py dedupe` → `fingerprints=367 duplicate_groups=0`
  - 连续两次导入 → ledger 字节一致（`cmp` 通过）。
