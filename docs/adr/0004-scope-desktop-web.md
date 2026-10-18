# ADR-0004: desktop / web（营销站）/ gui-extensions 不在 parity 程序范围内

- 状态：accepted（记录既有决定；依据 PARITY-LOOP §11.3）
- 日期：2026-10-18
- 上游版本：2.0.24 (ref e7a34f09bf)
- 背景：v2.0.24 monorepo 含 desktop（`packages/desktop`，Tauri shell）、`packages/web`（Astro 营销/文档站）、`gui-extensions`（desktop 配套的 SSH/WSL 远程管理件）与 Web UI（`packages/app`）。2026-10-11 的用户 scope ruling 扩容了范围：除 desktop 外全部移植，**包括 Web UI**（ROADMAP `R-04`）。PARITY-LOOP §5 Batch 4 与 §13 据此把 desktop/web/gui-extensions 固定为 out-of-scope（此处 "web" 指 `packages/web` 营销站，不是 Web UI），并不许反复拉回队列。
- 决定：parity 程序不移植 `packages/desktop`（Tauri shell）、`packages/web`（Astro 营销/文档站，除非用户另行指示）与 `gui-extensions`（desktop 伴侣件，随 desktop 一并排除；若发现桌面无关能力可个案重评）。Web UI（`packages/app`）**在范围内**（R-04，`serve` 托管 + 前端功能等价），不受本 ADR 影响。范围问题不再进 ledger 队列。
- 后果：代价是这三个表面永久缺位，相关用户询问统一指向本 ADR 与 ROADMAP scope ruling；同时存在文档不一致：`README.md:20` 与 `README.md:112` 仍写 "web UI 不在范围/桌面端与 Web UI 不在范围内"，与 ROADMAP 的 scope ruling（Web UI 已入范围）矛盾。收益是队列不再被范围问题反复扰动（§13 第 6 条）。
- OPEN（未决，非本 ADR 决定）：§12.4 退出条件要求 "README/ROADMAP 的范围声明与实际一致（Web UI 不再写'不在范围'）"。README 的上述两处是否/何时更新（连同 PORT_REPORT §3 deferred 尾单里的 "sounds" 措辞）留待 release 前裁决；本 ADR 不改动 README（write scope 仅 `docs/adr/**`）。
- 证据（path:line / 测试）：
  - `docs/PARITY-LOOP.md` §5 Batch 4（L986-990，"desktop/web/gui-extensions 保持 out-of-scope"）
  - `docs/PARITY-LOOP.md` §13 第 6 条（L1660，"不要把 out-of-scope 的 desktop/web/gui-extensions 反复拉回队列"）
  - `docs/PARITY-LOOP.md` §12.4（L1605，README/ROADMAP 范围声明一致性退出条件）
  - `ROADMAP.md:3`（scope ruling：除 desktop 外全部移植，含 Web UI）
  - `ROADMAP.md:9-11`（IN/OUT 清单：IN 含 `packages/app`；OUT 含 `packages/desktop`、`packages/web`；`gui-extensions` 随 desktop 排除、个案重评）
  - `ROADMAP.md:47`（R-04 Web UI 任务条目，P1）
  - `README.md:20` 与 `README.md:112`（现状用户侧范围声明；与 ROADMAP 的矛盾即上方 OPEN）
- 复审日期：release 前
