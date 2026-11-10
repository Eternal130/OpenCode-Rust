# 第二十轮复核 W-C（工具/TUI/server）— 2026-11-10，HEAD=967bedba

> 对第十九轮立项的未闭合项在当前 HEAD 重验。行号漂移均系 c36d2556 全仓 fmt + `oc-*`→`crates/oc-*` 布局迁移；git grep G-30x/G-31x 修复提交零命中。

**G-302 filetype 表偏窄 — STILL_OPEN**：`crates/oc-tui/src/components/syntax.rs:393-416`（原 :399-422，仅 fmt 漂移）仍 ~19 臂；`.md .sh .lua .xml .scala .ex .hs .nix` 等仍缺。

**G-303 read AGENTS.md 绕过 Files seam — STILL_OPEN**：`crates/oc-core/src/tool/read.rs:925-955`（发现腿 :933 `.parent()`、:945 `is_file()`、:948 宿主 FS 上溯）。

**G-304 shell 服务层打包 — STILL_OPEN（①恶化：两处）**：① `crates/oc-core/src/tool/shell.rs:311` 与 `:664` 两处 `std::env::vars().collect()`（登记时仅一处）；② `:349` cwd `unwrap_or_default()`；③ `crates/oc-core/src/shell/select.rs:205` dedup() 仅相邻去重。

**G-305 credential 钉住漏 active=false — STILL_OPEN**：`crates/oc-core/src/integration/credential.rs:136-141` 与 `crates/oc-server/src/handlers/credential.rs:195-208` 双路均 `pinned.is_none()` 仅钉 NULL。

**G-307 patch Move to 不建父目录 — STILL_OPEN（已自文档化为有界决策）**：`crates/oc-core/src/tool/patch.rs:429-433` 注释明示 "writes WITHOUT creating parents (a missing parent stays an error)"——偏离被代码自认，但行为仍与 ref（local.ts mkdir -p）断裂。可并入 G-1 作有界决策销项。

**G-308 normalize/truncate 顺序 — STILL_OPEN（注释仍误读）**：`crates/oc-core/src/session/runner/step.rs:716-747`——:718 先 truncate、:744 后 normalize；:738-742 注释 "matching REF order" 的错误解读原样保留。

**G-310 websearch 超时分叉 + tavily 文案 — STILL_OPEN**：`tool/websearch.rs:22,508,584`（SELECTION_TIMEOUT_MS=60s 两层，可达 120s）；tavily 超时仍泛化 `error.to_string()`（websearch_providers.rs:75/:89），专用文案仅 :490 非 "timed out" 分支。

**G-311 CORS Vary — STILL_OPEN**：`crates/oc-server/src/middleware.rs:130` 仅 `Vary: Origin`，:141-167 回显 Allow-Methods/Headers。

**G-312 高亮缓存满 128 全清 — STILL_OPEN**：双层 `crates/oc-tui/src/components/syntax.rs:376-377` 与 `crates/oc-syntax/src/engine.rs:256-257`；engine.rs:15 文档 "drops the oldest" 仍失实。

### 汇总
9/9 未闭合。状态变化：G-304① panic 向量 1 处→2 处（shell.rs:311 新增）；G-307 偏离已代码自认（可并入 G-1 有界决策销项）。
