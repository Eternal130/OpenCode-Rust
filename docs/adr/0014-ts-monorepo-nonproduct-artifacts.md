# ADR-0014: TS monorepo 非产品工件不移植（script / simulation / effect-drizzle-sqlite / identity / console / posts / GitHub Action）
- 状态：accepted
- 日期：2026-10-21
- 上游版本：2.0.24 (ref e7a34f09bf)
- 背景：ledger SURF-P2-SCRIPT、SURF-P2-SIMULATION、SURF-P2-DRIZZLE、SURF-P2-IDENTITY、SURF-P3-CONSOLE、SURF-P3-POSTS、SURF-P1-GHA（10-surfaces.md 遗留的 P2/P3 表面 + P1-GHA）。这七条目的 "expected" 都是 v2 monorepo 里的包/目录。逐项核查 ref 树后确认：它们要么是上游自用的开发/测试工装，要么是发布管线 / 品牌 / 内容 / 托管站点形态，要么在本 commit 根本没有功能源码。ROADMAP 的 scope ruling（除 desktop 外全部移植）针对的是**产品行为**；这些工件不承载 CLI / server / TUI 的任何用户可见行为契约。判据同 ADR-0013（"v2 自身不要求我们缺失的行为"）与 ADR-0012（npm 打包形态属分发渠道决策，不是行为 parity）。
- 决定：不移植上述七项，逐项判据：
  1. `packages/script` = 上游发布自动化脚本（bun 版本门 + npm registry 版本推进 + changelog bot/team 过滤，`src/index.ts` 共 86 行），纯发布管线；Rust 侧发布由 cargo workspace + `tools/` 承担。
  2. `packages/simulation` = 上游内部测试 harness，仅 `OPENCODE_SIMULATE` 为真值时经动态 import 替换 HttpClient 层（`server-process.ts:103`、`routes.ts:151-156`）；env 未设时 ref 与 rust 生产行为逐点等价。其能力位（对录制/假后端驱动整服务器）由我们的 cassette / fake-server 差分机架承担（ADR-0006）。**`OPENCODE_SIMULATE` 开关本身不被消费由 CLI-N4（S1，open）单独跟踪，不因本 ADR 结案。**
  3. `packages/effect-drizzle-sqlite` 在 v2.0.24 仅含 `examples/migrations/20240101000000_create_users/migration.sql` + `sst-env.d.ts`，无 package.json / src / exports，全仓零 importers——无可移植面；SQLite 生态位由 rusqlite 承担（drizzle 的实际使用方是 `packages/stats/core`，与本包无关）。
  4. `packages/identity` = 纯品牌资产（mark.svg / mark-light.svg / mark-96/192/512 PNG），零代码零 package.json；产品侧视觉 parity 由 TUI 车道承担。
  5. `packages/console/*` = 厂商**托管** SaaS 控制台（Vite + Stripe + SST 六子包），与 `packages/web` 同类的托管基础设施；产品侧契约（org-config 拉取、`SsoRequired` / `SignedOut` 错误形、console 下发 provider → resolver）已移植并接线。
  6. `packages/posts` = `"private": true` 的 Astro 博客（Cloudflare 部署），内容/营销面，与 ROADMAP OUT 清单里的 `packages/web` 同类。
  7. GitHub Action（ref 仓根 `github/action.yml`，非 `packages/github/`——后者是空目录）= 拉取已发布 release 二进制进 runner 缓存再调用的 composite action，属 ADR-0012 已固化的分发渠道族（npm / AUR / Homebrew / Action marketplace）成员，且依赖托管 `api.opencode.ai` 的 OIDC 交换，是**发布期渠道决策**而非行为 parity。
- 后果：代价是这七个表面永久缺位，GitHub marketplace 不出现我们维护的 action（用户可用任意 published 二进制 + 通用 workflow 步骤，或继续指向上游 action）；`OPENCODE_SIMULATE` 在 rust 侧为无操作（该 env 缝归 CLI-N4）。收益是 parity 队列剔除全部非产品工件，SURF 族只剩真实行为面。**本 ADR 不覆盖** SURF-P2-STORYBOOK / CONTAINERS / RECORDER / CODEGEN、SURF-P3-ENTERPRISE / SERVICES / STATS / FUNCTION 等兄弟条目——它们由各自条目另行裁定。
- 证据（path:line / 测试）：
  - ref `packages/script/src/index.ts:1-61`（bun 版本门 / npm 版本推进 / bot 列表）
  - ref `packages/cli/src/server-process.ts:103` + `packages/server/src/routes.ts:151-156` + `packages/simulation/src/backend/index.ts:8-19`（simulation 仅 env 门控动态加载）+ `packages/simulation/test/`（10 个上游自用测试）
  - ref `packages/effect-drizzle-sqlite/`（仅 examples/migrations + sst-env.d.ts；grep `@opencode/effect-drizzle-sqlite` 零 importers；drizzle 实际使用方为 `packages/stats/core`）
  - ref `packages/identity/`（仅 mark.svg / mark-light.svg / mark-*.png）
  - ref `packages/console/app/package.json`（vite + VITE_STRIPE_PUBLISHABLE_KEY + sst stage）；rust `crates/oc-core/src/integration/console.rs:1-35,44-77` + `crates/oc-core/src/engine/resolver.rs:93,143`（产品侧契约已移植并接线）
  - ref `packages/posts/package.json`（`"private": true`, astro/wrangler）+ `.github/workflows/deploy-posts.yml:8-10`
  - ref `github/action.yml:1-45`（curl releases/latest + actions/cache + install）+ `.github/workflows/publish-github-action.yml`
  - rust grep 佐证：`OPENCODE_SIMULATE` 在 crates/ 零命中（env 缝归 CLI-N4）
- 覆盖 ledger ids：SURF-P2-SCRIPT、SURF-P2-SIMULATION、SURF-P2-DRIZZLE、SURF-P2-IDENTITY、SURF-P3-CONSOLE、SURF-P3-POSTS、SURF-P1-GHA
- 复审日期：release 前
