# Lane 11 — core 杂项模块审计（codemode / environment / v1 / modal / git / vcs / retention / reference / wellknown / workspace / image / websearch / integration）

- 参考实现：`/opt/opencode-ref`（opencode v2.0.24，TypeScript）
- 目标实现：`/opt/opencode-rust`（Rust 移植）
- 日期：2026-10-18。**只读审计**，未改动任何源码。
- 方法：逐文件枚举参考侧范围内模块的导出面与行为，在 `crates/` 中定位对应实现（含 oc-server handler 这种"core 服务的唯一调用方"），核对行为/线格式/接线（生产是否真正调用），再对照 `ROADMAP.md` / `FINDINGS.md` / `docs/audit*.md` / `.slim/deepwork/rust-port/*` 标注 KNOWN/NEW。

---

## 0. 总览

| 维度 | 结论 |
|---|---|
| 整体判断 | 本 lane 范围内 **codemode、v1 配置迁移、wellknown、location-activity、file-retention（逻辑本体）、websearch（主体）、git 身份/快照树操作（大体）已移植**；**VCS API 面、图片 resize、Modal 动态模型、git 引用物化（repository-cache）、workspace 生命周期**存在缺失或行为分歧 |
| 最高优先级（S1） | **`/api/vcs/base`、`/api/vcs/branch`、`/api/vcs/diff` 三个端点线格式与 v2 schema 不符**——生成的强类型客户端（v2 官方 TUI 与 Rust TUI 自身）无法解码；`worktree.remove` 只删 DB 行不执行 `git worktree remove` |
| 次优先级（S2） | 图片 normalize/resize 完全缺失（附件与工具图片输出均直通）；git 类型 reference 不物化（path 返回仓库 URL）；reference 指导指令是 TODO 桩；shell/tool-output 保留清理任务从未被启动（磁盘无限增长）；snapshot 对 symlink 的捕获/恢复错误、diff 上下文固定 4 行且忽略 `context` 参数、无字节上限/二进制处理；modal 动态模型同步缺失；websearch 不读取已保存的 API key 凭据、不解析 retry-after |

范围澄清：
- **`link/` 目录在参考 v2.0.24 core 中不存在**（`packages/core/src` 下无任何 `*link*` 文件，已 `find` 验证）。若指 integration 的连接（connection/credential 绑定），见 §14；无对应缺口需要单独移植。
- "环境探测/容器检测"：参考侧没有容器/docker 探测模块；`environment/` 是**工作区执行平面（文件+进程 spawn）抽象**，见 §2。
- codemode 的 JS 解释器本体（oc-plugin-host guest 运行时）与插件宿主接线属插件 lane（w9w4/w12glue 已闭环实测）；本报告只核对 core 侧 catalog/instructions/工具整形（§1）。

---

## 1. 参考模块 → Rust 对应/缺失 映射总表

状态：✅ PORTED ／ 🟡 PARTIAL ／ ❌ MISSING ／ ⚠️ WRONG（存在但行为错）／ ➕ EXTRA。「v2 主线使用」指 v2 CLI/TUI/server 是否实际调用该能力。

| # | 参考模块（ref path） | Rust 对应（或判定） | 状态 | v2 主线使用 | 严重度 | KNOWN/NEW |
|---|---|---|---|---|---|---|
| 1 | `codemode/catalog.ts`（summarize 预算算法） | `crates/oc-core/src/tool/codemode.rs:500-707` | 🟡 | 是（server+TUI 零插件也合成 `execute`） | S2 | KNOWN（w9w4/w12glue 实测闭环）|
| 2 | `codemode/instructions.ts`（render/update/diff） | `tool/codemode.rs:657-810`；`session/runner/context.rs:485,495` | ✅ | 是 | — | KNOWN |
| 3 | `codemode/tool.ts`（execute 工具/目录/hooks） | `tool/codemode.rs` + `oc-plugin-host`（guest 执行） | ✅ | 是 | — | KNOWN |
| 4 | `codemode/web.ts`（fetch 扩展 30s 超时） | `oc-plugin-host/src/host/host.js`（guest fetch） | ✅ | 是（execute 内 fetch） | — | KNOWN |
| 5 | `environment/environment.ts`（Files+spawner 缝） | 无独立缝；工具直用 std::fs（如 `tool/read.rs:178,262`） | 🟡 架构性 | 本地行为等价（两侧都注册 0 个 workspace driver） | S2 | NEW（nuance；auditE 未列） |
| 6 | `environment/local.ts` | 同上（内联本地 fs） | 🟡 | 同上 | S2 | NEW |
| 7 | `environment/exec-defaults.ts`（sh 脚本+64MiB 上限） | 无对应（各工具有自己的上限） | 🟡 | 同上 | S2 | NEW |
| 8 | `environment/memory.ts`（内存驱动，测试用） | 无（Rust 测试各自造 tmpfs/内存桩） | ➖ N/A | 否（测试设施） | — | — |
| 9 | `environment/unavailable.ts`（无执行平面 spawner） | 无 | ➖ N/A | 否（仅 workerd/serverless 用，`server/src/workerd.ts:79`） | — | — |
| 10 | `v1/config/*`（migrate/agent/command/mcp/permission/plugin/provider/provider-options/error） | `oc-core/src/config/normalize.rs`（v1 解码+迁移内嵌）+ `cli_config/v1.rs` | ✅ | 是（config loader 对 v1 文档归一化） | — | KNOWN（auditF 覆盖） |
| 11 | `modal/models.ts`（Modal 代理动态模型 load/derive） | **缺失**（catalog 仅静态 models-dev 快照含 `modal` 提供方） | ❌ | 是（用户配置 modal 提供方时；niche） | S2 | NEW |
| 12 | `git.ts` repo.discover | `oc-core/src/project.rs`（git2 discover）+ worktree handler | ✅ | 是 | — | KNOWN |
| 13 | `git.ts` repo.clone | **缺失**（仅 repository-cache 需要） | ❌ | 否（主线不触达；reference 物化需要） | S2 | NEW |
| 14 | `git.ts` repo.create（snapshot 仓库 init + `opencode.gitconfig` include + seed alternates/index 拷贝） | `oc-core/src/snapshot.rs:218-225`（仅 bare init） | 🟡 | 是（快照存储创建） | S2 | NEW |
| 15 | `git.ts` remote.get / history.head / rootCommits | `project.rs:634-706`（git2 等价） | ✅ | 是（项目身份） | — | KNOWN |
| 16 | `git.ts` history.branch（symbolic-ref）/ defaultRemoteBranch | vcs_get 用 `rev-parse --abbrev-ref`；**defaultRemoteBranch 缺失** | 🟡/❌ | 是（vcs info.branch.default、diff base） | S2 | KNOWN(auditE §1.16)+NEW 细节 |
| 17 | `git.ts` sync.*（fetchRemotes/fetchBranch/checkoutRemoteBranch/resetHard） | **缺失** | ❌ | 否（仅 repository-cache） | S2 | NEW |
| 18 | `git.ts` worktree.create | `oc-server/src/handlers/project.rs:415,431`（`git worktree add --detach`） | ✅ | 是 | — | KNOWN（auditE 记 create 曾空壳，已修） |
| 19 | `git.ts` worktree.remove（`git worktree remove [--force]`，脏树→forceRequired） | `handlers/project.rs:563-598` **只删 DB 行** | ❌ | 是 | **S1** | NEW（auditE 记的是 create 空壳；remove 的 git 侧仍未接） |
| 20 | `git.ts` worktree.list（porcelain） | `handlers/project.rs:197-226`（DB 行；v2 的 HTTP list 也是 DB 行） | ✅（HTTP 面） | 是 | — | KNOWN |
| 21 | `git.ts` index.refresh / ignored（scope 级 git 索引刷新） | `snapshot.rs:274-351`（fs 遍历 + libgit2 ignore/tracked 等价） | 🟡 | 是（snapshot capture） | S2 | KNOWN（snapshot/tests 钉行为）+NEW（symlink 等，见 §7.3） |
| 22 | `git.ts` tree.capture/write/files/diff/restore | `snapshot.rs:349-476` | 🟡 | 是（session 快照/diff/revert） | S2 | NEW（细节分歧） |
| 23 | `vcs.ts`（服务：providers/选择/initialize/事件） | `oc-server/src/handlers/project.rs:604-768`（无 provider 抽象，直连 git） | ⚠️ | 是（TUI diff viewer/footer/branch picker） | **S1** | KNOWN(auditE)+NEW（线格式细化） |
| 24 | `plugin/vcs/git.ts`（内置 git provider 适配器） | 同上（部分语义） | ⚠️ | 是 | S1/S2 | KNOWN+NEW |
| 25 | `plugin/vcs/hg.ts`（Mercurial provider） | **缺失**（vcs_get 对 hg 仓库仍跑 git 命令） | ❌ | 是（hg 项目） | S2 | KNOWN（auditE "no hg"） |
| 26 | `vcs/patch.ts`（PATCH_CONTEXT_LINES=INT_MAX、10MB 上限、emptyPatch、chunksByFile） | `oc-core/src/tool/file_diff.rs`（固定 4 行上下文）；无总量上限/二进制/空补丁回退 | 🟡 | 是（vcs.diff + snapshot diff） | S2 | NEW |
| 27 | `file-retention.ts`（mtime 截断清理） | `oc-core/src/tool_output.rs:275-300`（逻辑已移植） | 🟡 | 是 | S2 | NEW（清理任务未接线） |
| 28 | `location-activity.ts`（60min TTL + 1min 扫描 + 中断收敛） | `oc-core/src/location_services.rs:43-49,135-190`；`engine/mod.rs:183-184`（默认启动） | ✅ | 是（server boot） | — | KNOWN |
| 29 | `reference.ts`（local/git 源、缓存物化、每小时刷新、Updated 事件） | `oc-server/src/handlers/agent.rs:106-160`（只读 config 快照） | 🟡 | 是（/api/reference） | S2 | NEW（auditE 标 reference ✅，物化缺口未记） |
| 30 | `repository-cache.ts` + `repository.ts`（git 引用克隆/日更/校验） | **缺失** | ❌ | 否（仅 git 类型 reference 物化） | S2 | NEW |
| 31 | `reference/instructions.ts`（`core/reference-guidance` XML 指导） | `session/runner/context.rs:620-634` **TODO 桩**（恒"无引用"） | ⚠️ | 是（系统提示） | S2 | NEW |
| 32 | `wellknown.ts`（manifest 检查/resolve/{env:} 替换/源持久化/refresh） | `oc-core/src/integration/wellknown_config.rs`；`oc-server/src/handlers/integration.rs:59,138-200` | ✅ | 是（config 最低优先层） | — | KNOWN（BE-3b） |
| 33 | `wellknown/plugin.ts`（注册 integration 名称+login 命令法） | 未见对应注册（仅 add+文档解析） | 🟡 | 是（wellknown 登录入口） | S2 | NEW |
| 34 | `workspace.ts`（create/provision/connect/idle 挂起/destroy 生命周期） | **缺失**（仅 DB 表助手 `project.rs:407-441`；`database/bootstrap.rs:232` 建表） | ❌ | 否（v2 core 也注册 0 driver，workspaceID 仅 SDK 嵌入设置；无 HTTP API） | S2 | NEW（auditE 曾标 workspace ✅，实为表级） |
| 35 | `workspace/driver.ts` + `workspace/sql.ts`（Binding/注册表/表） | 表已建（schema 一致）；driver 接口缺失 | 🟡 | 否 | S2 | NEW |
| 36 | `image.ts`（normalize 服务+限制+State 配置） | **缺失**（`handlers/attachment.rs:13-20` 自述"无 resizer，永远直通"） | ❌ | 是（附件+工具图片输出） | S2 | KNOWN（auditE §1.17 ❌） |
| 37 | `image/photon.ts`（解码/缩放阶梯/PNG→JPEG 质量阶梯/base64 预算） | **缺失**；`oc-schema/src/config/media.rs:7-17` 解析了 `media.image` 配置但无消费者 | ❌ | 是 | S2 | KNOWN |
| 38 | `websearch.ts`（选择/随机亲和/429 冷却轮转/query） | `oc-core/src/tool/websearch.rs` + `websearch_providers.rs`；`engine/mod.rs:695-701` 接线 | 🟡 | 是（websearch 工具） | S2 | KNOWN（auditE #4 已修接线）+NEW（凭据/冷却细节） |
| 39 | `plugin/websearch/*`（exa/firecrawl/parallel/tavily/tinyfish + mcp 传输） | `websearch_providers.rs`（5 家 + mcp_call 传输） | 🟡 | 是 | S2 | KNOWN+NEW（存储 key 不用） |
| 40 | `integration/connection.ts`（Connection key 助手+schema 再导出） | `oc-core/src/integration/types.rs` 等（integration lane 主覆盖） | ✅ | 是 | — | KNOWN |

---

## 2. environment/（执行平面抽象）

**EXPECTED**（ref）：`environment.ts:24-32` — location 带 `workspaceID` 时把 Files/spawner 绑到 workspace driver，否则本地驱动；`files.ts:31-50` 定义 read/write/stat/list/remove/move/mkdir 契约（stat 不跟随最终符号链接、read/list 跟随）；`exec-defaults.ts:16-20` 进程默认实现带 64MiB/64KiB 输出上限与 44/45/46 退出码协议；`local.ts` 为宿主 fs 绑定。所有 fs 工具（read/glob/grep/edit/write/patch/shell）、file-mutation、ripgrep、MCP stdio 都经此缝（14 个消费者）。

**ACTUAL**（rust）：无该缝；工具直用 `std::fs`（`tool/read.rs:178,262-267` 等）。

**影响判定**：v2.0.24 core 的 workspace driver 注册表为空（`workspace/driver.ts:88` `registryNode({})`），CLI/TUI/server 无法产生非空 `workspaceID`（仅 SDK 嵌入），因此**主线行为完全等价**。属于"远程工作区就绪度"的架构差异，不是用户可见缺口。`unavailable.ts` 仅 workerd 目标使用（`server/src/workerd.ts:79`），Rust 无该目标，N/A。`memory.ts` 是测试设施，N/A。

- 分类：**PARTIAL（架构缝）/ 主线不影响**；S2；NEW（nuance）。

## 3. v1/config/（v1 配置迁移）

**EXPECTED**（ref `v1/config/migrate.ts`）：normalizeAction 改名（write|patch→edit、task→subagent、bash→shell，:29-34）；migrateAgent（options+temperature/top_p→request.body、color 非 # 前缀→#aaaaaa，:36-58）；migrateCommand（subtask→subagent，:60-68）；modelSelection（`^[^/#]+/[^#]+$`、variant 过滤，:70-78）；migrateMcp（enabled 取反、timeout→{catalog,execution}、oauth 字段驼峰转下划线，:80-105）；migrateProvider 的 azure-cognitive-services/google-vertex-anthropic 特例（:107-168）；migrateModel 的 context_over_200k 分层计价、interleaved compatibility、deprecated→disabled（:170-221）。

**ACTUAL**（rust `config/normalize.rs`）：全部落地并有测试钉住——改名表 `:80-90`；modelSelection `:1282-1330`；v1 agent 宽松解码 `:1492-1560`；`migrate_agent_document` `:1675-1741`（color `:1741`）；command subtask `:1854`；`context_over_200k` `:1899,2212`；provider 特例 `:2105-2108`；provider-options headers/body 拆分 `:2038-2050`。

- 分类：**PORTED**；主线使用（config loader）；无发现。KNOWN（auditF 覆盖）。

## 4. modal/（Modal 代理动态模型）

**EXPECTED**（ref `modal/models.ts:50-151` + `plugin/provider/modal.ts`）：provider `modal` 配置了 `baseURL` 且有凭据（integration key 或 `MODAL_PROXY_TOKEN`）时，`GET {baseURL}/models`（3s 超时，容错解码）拉取远端模型，与静态模板 derive 出模型清单（计价×1e6、上下文/输出上限、reasoning effort 变体、interleaved compatibility），凭据切换即刷新。

**ACTUAL**（rust）：无 `modal` 集成/提供方插件（`integration/providers.rs` 安装列表无 modal；全仓无 `MODAL_PROXY_TOKEN`）。静态 catalog 含 `modal` 提供方条目（models-dev 快照），但模型清单是快照里的静态集合，不会随代理端动态模型同步。

**影响**：Modal 代理用户在 Rust 下只能看到静态快照模型，代理端新增/变更模型不可见；无凭据刷新。niche 用户群。

- 分类：**MISSING**；v2 使用（配置该 provider 时）；S2；NEW。

## 5. git.ts / snapshot（Git 操作面）

### 5.1 已对齐
- 身份链（remote URL→commondir 缓存→root commit）：`project.rs:529-706` 用 git2 等价实现。
- worktree.create：`handlers/project.rs:415,431` 执行 `git worktree add --detach -- <dir> <ref|HEAD>`（含名称碰撞后缀），与 `git.ts:649-667` 语义一致。
- 快照存储路径 `<data>/snapshot/<projectID>/<Hash.fast(worktree)>`：`snapshot.rs:205-233`（自研 SHA-1 与 v2 一致——注意 `project.rs:25` 的 hash_fast 用 SHA-256，文档已自辩"跨宿主不持久化"，两处哈希不同属内部约定，不算分歧）。

### 5.2 repo.create 的 snapshot 配置与 seed（PARTIAL）
- EXPECTED（`git.ts:23-38,345-382`）：init 后写 `opencode.gitconfig`（autocrlf=false/longpaths/symlinks/fsmonitor=false/untrackedCache/index v4/threads）并 include 进 config；seed 时写 `objects/info/alternates` 指向源仓库 objects、拷贝源 index。
- ACTUAL（`snapshot.rs:218-225`）：仅 `Repository::init_opts(bare)`，无 gitconfig、无 alternates、无 index 拷贝。
- 影响：无 alternates 时快照库独立存对象（磁盘翻倍）；无 index 意味着每次 capture 全量走 fs 遍历（v2 借 index 增量）。功能等价、性能/磁盘差异。S2；NEW。

### 5.3 快照 capture/diff/restore 的行为分歧（PARTIAL/WRONG）
1. **symlink 捕获错误**：`snapshot.rs:327` 对 symlink 执行 `std::fs::read`（跟随链接读目标内容）却按 `0o120000` 模式入库（`:329-330`）；恢复时 `std::fs::write` 落成普通文件（`:465`）。EXPECTED（v2 `git add`/`git checkout tree -- file`）：blob=链接目标路径、恢复重建符号链接。含 symlink 仓库的 undo 会把符号链接替换成内容文件。S2；NEW。
2. **diff 上下文**：`SnapshotService::diff` 忽略 `context` 参数（`snapshot.rs:603` `_context`），`file_diff.rs` 固定 4 行上下文；EXPECTED：`git diff --unified={context ?? 3}`，且 session/diff 默认 `PATCH_CONTEXT_LINES = 2_147_483_647`（`vcs/patch.ts:6`，即全文件补丁）。`/api/session/:id/diff` 的 patch 体与 v2 不同（截断上下文），`context` 查询参数无效。S2；NEW。
3. **无字节上限/二进制处理**：EXPECTED（`git.ts:518-576` + `vcs/patch.ts:7-8,15`）：单文件/总量 10MB 上限，超限文件回退 emptyPatch，binary（numstat `-`）patch 为空；ACTUAL：无上限，二进制文件按 utf8-lossy 产出乱码 patch。S2；NEW。
4. `files(from,to)` 后按源仓库 ignore 过滤（`snapshot.ts:145-162`）：Rust `filter_ignored`（`snapshot.rs:377-390`）等价 ✅。

### 5.4 仅 reference 物化需要的函数（MISSING，随 §8 一并）
`repo.clone`、`sync.fetchRemotes/fetchBranch/checkoutRemoteBranch/resetHard`（`git.ts:246-299`）在 Rust 无任何对应——唯一消费者是 repository-cache（git 引用物化）。主线（CLI/TUI/server 的项目/快照/worktree 路径）不触达。

## 6. vcs.ts + plugin/vcs/*（VCS 抽象与 git 适配器）⚠️ 本 lane 最大问题簇

**EXPECTED**（ref）：
- 服务层（`vcs.ts`）：provider 注册表 + `selection ?? location.vcs.type` 选择；info 缓存 + 分支变化发布 `vcs.branch.updated`（`:128-151`）；监听 `.git/HEAD` 文件变更刷新（`:153-172`）；initialize 五种错误 kind（missing/conflict/unknown/unsupported/failed，`:177-194`）。
- git 适配器（`plugin/vcs/git.ts`）：全部命令带 `--no-optional-locks -c core.autocrlf=false ...` 配置组（`:152-164`）；status 映射（`??`→added、含 U→modified、A无D→added、D无A→deleted，`:166-172`），`--untracked-files=all --no-renames`、按 location 目录 scope（`-- .`）、按文件排序、真实 additions/deletions（numstat + 未跟踪单文件 stat）（`:77-100,333-342`）；branches 用 `for-each-ref --ignore-case --sort=refname --sort=-committerdate`（本地+远端、过滤 /HEAD、搜索模式转义）（`:228-242`）；base 为 **reflog 创建点推导**（`branch: Created from` → 命名 ref + 双向祖先校验 → `{name, ref, source:"reflog"}`；回退默认分支 `source:"default"`；无 base 时 DiffError "Choose a review base"）（`:286-318`）；diff 三模式 working/branch/committed（merge-base 基准、untracked 用 `diff --no-index /dev/null`、批量 patch + 逐文件回退 + 10MB 总量封顶 + emptyPatch）（`:101-118,385-607`）。
- HTTP 面（`server/src/handlers/vcs.ts:57-100`）：get→`Info{branch:{current,default?},provider?}`；base→`Base{name,ref,source}`（必填 source）或 null，失败 503；status→`FileStatus[]`；branch.list→**`BranchList = string[]`**（limit `min(limit??50,100)`）；diff→`FileDiff.Info[]`；init→204 + 400/409/VcsInitNotSupported/503 + 位置失效 + `worktree.updated` 事件。

**ACTUAL**（rust `oc-server/src/handlers/project.rs:604-768`，无 provider 抽象、直连 git 子进程）：

| 端点 | ref 行 | rust 行 | EXPECTED | ACTUAL | 判定 |
|---|---|---|---|---|---|
| `GET /api/vcs` | vcs.ts:195-197 / git.ts:67-72 | project.rs:604-626 | `{branch:{current, default}, provider}`；current 用 `symbolic-ref --quiet --short`（detached→省略） | 有 current+provider；**无 default**；`rev-parse --abbrev-ref` 使 detached HEAD 时 current="HEAD"（v2 为省略） | 🟡 WRONG 细节 |
| `GET /api/vcs/base` | git.ts:286-318 | project.rs:628-641 | reflog/default 推导，`{name, ref, source}`（source 必填）；无可用 base→503 "Choose a review base" | `merge-base HEAD @{u}` → `{name, ref}`，**缺 `source` 字段**；无 503 语义 | **S1 线格式** + WRONG 语义（上游基线 ≠ review 基线） |
| `GET /api/vcs/branch` | git.ts:228-242；schema `BranchList=string[]` | project.rs:643-667 | `string[]`（heads+remotes，committerdate 优先排序，忽略大小写模式匹配） | 返回 **`{"branches":[...]}` 对象**，仅本地分支字母序 | **S1 线格式** |
| `GET /api/vcs/diff` | vcs.ts:228-251 / git.ts:101-607 | project.rs:669-699 | `FileDiff.Info[]`（file/patch/additions/deletions/status；模式 working/branch/committed；base 推导；10MB 封顶） | 返回单个伪条目 `{path:"diff", status:{type:"modified"}, chunks:[<整个原始 diff>]}`；接受不存在的 `mode=staged`（→`--cached`）而忽略 branch/committed 语义；无封顶 | **S1 线格式** + WRONG |
| `GET /api/vcs/status` | git.ts:77-100,333-342 | project.rs:701-736 | 真实 additions/deletions、`--untracked-files=all --no-renames`、location 目录 scope、排序 | additions/deletions 恒 0；默认 untracked 粒度；在 canonical（项目根）执行而非 location 目录；未排序；状态映射粗略（`"AM "` 等字面量表） | 🟡 |
| `POST /api/vcs/init` | vcs.ts:177-194 + handlers/vcs.ts:21-56 | project.rs:738-768 | 5 种错误 kind + init 后项目重解析 + 位置失效 + `worktree.updated` | 仅 conflict（"Repository already exists"）与非 git 的 not-supported；无 missing/unknown/failed 区分、无事件、无位置失效 | 🟡 |

**线格式破坏的实证**：生成的强类型客户端（与 v2 OpenAPI 同源）声明 `VcsBase.source` 为必填（`oc-client/src/types.gen.rs:8597-8607`）、`VcsBranchList = Vec<String>`（`:8626`）、`VcsDiffResponse.data: Vec<FileDiffInfo>`（file/patch/additions/deletions/status 全必填，`:1894-1910,10524-10531`）。Rust 服务器三个端点的实际返回均无法通过这些类型反序列化。受影响客户端：
- v2 官方 TUI（diff-viewer.tsx:116-251 使用 vcs.info/base/diff/branch.list）——M1「官方 TUI 连 Rust 服务器」场景下分支选择器与 diff 视图直接报错；
- **Rust TUI 自身**（`oc-tui/src/app.rs:4139-4206` 用同一生成客户端拉 diff，期望 `FileDiffInfo`）——working/branch/committed 三种 diff 源全部解码失败。

**事件面**：`vcs.branch.updated` 已实现（`oc-server/src/announce.rs:497-573`，HEAD 轮询指纹对比，语义对齐 v2 的文件监听路径，含 filesystem.changed 联动）✅。

- 分类：vcs 整体 **WRONG/PARTIAL**；v2 主线使用（TUI diff viewer/footer）；**S1（三端点线格式）**；KNOWN（auditE §1.16 "git-subset/raw-string hunks"）+ **NEW**（线格式对强类型客户端的破坏、base 语义、branch list 形状、default 缺失、detached HEAD、status scope）。
- hg provider：**MISSING**（KNOWN，auditE "no hg"）；vcs_get 对 hg 仓库跑 git 命令 → branch.current 为空串。

## 7. worktree 的 git 侧（与 worktree lane 交叉）

- **worktree.remove 不执行 git**：EXPECTED（`git.ts:669-681` + worktree 流程）`git worktree remove [--force]`（在 commonDirectory 下执行），脏树错误含 forceRequired 判定（`git.ts:645`）；ACTUAL（`handlers/project.rs:563-598`）仅 `DELETE FROM worktree`。后果：磁盘目录与 `.git/worktrees` 管理数据残留，脏树不报 forceRequired。**S1**（功能不完整）；NEW（auditE 记的 create 空壳已修，remove 的 git 侧漏网）。
- worktree.refresh 为 no-op（`project.rs:558-560` 自注 TODO(phase-9)）：KNOWN。
- worktree.list 走 DB 行与 v2 HTTP 面一致 ✅（`project.rs:197-226`）。

## 8. reference.ts / repository-cache / reference 指导

**EXPECTED**：
- `reference.ts:51-94`：`list()` 对 git 源做 Repository.parse/分支校验（非法则丢弃），path = `Repository.cachePath(global.repos, repo, branch)`（**本地物化路径**）；后台以并发 4 物化 git 引用（`cache.ensure refresh:"daily"`），每小时轮询；变更发 `reference.updated` 事件。
- `repository-cache.ts`（249 行）：锁、clone、fetch、checkout、reset 的完整错误面。
- `reference/instructions.ts:23-88`：`core/reference-guidance` 系统指令渲染 `<available_references>` XML（仅有 description 的引用），增量 diff 更新。

**ACTUAL**：
- `oc-server/src/handlers/agent.rs:106-160`：从 config 快照拼 `/api/reference` 响应；**git 源的 `path` 字段填的是仓库 URL 本身**（`git.repository.clone()`），非本地缓存路径；无校验、无物化、无刷新、无 `reference.updated`。
- `session/runner/context.rs:620-634`：reference 指令为 TODO 桩——`read` 恒 `Removed`，initial 恒输出 "No reference files are currently loaded..."，**即使配置了引用也告知模型没有引用**；changed 恒同一句。
- repository-cache / repository：全缺（§5.4 的 clone/sync 也因此无消费者落地）。

**影响**：配置 git 引用（`references: {x: {git: {...}}}`）的用户：HTTP 面返回不可用的"路径"；模型侧被告知无引用（local 引用也受影响——桩不区分）；无每日更新。local 引用的 path 正确但指导指令缺失是主要用户可见差异。

- 分类：reference list **PARTIAL**、物化 **MISSING**、指导指令 **WRONG(STUB)**；v2 使用；S2；NEW（auditE 将 reference 标 ✅，未记物化/指令缺口）。

## 9. wellknown.ts / wellknown/plugin.ts

**EXPECTED**：`wellknown.ts:60-96` inspect（`{origin}/.well-known/opencode`）+ resolve（`{env:NAME}` 替换，变量→进程环境兜底）；`:101-184` 服务（kv `wellknown:sources` 持久化、缓存、refresh 深比较+`wellknown.updated`、add 校验 manifest.auth、remove）；`wellknown/plugin.ts`：为每个带 auth 的源注册 integration（name=origin 主机名）+ `login` command 方法，事件触发 integration.reload。config 侧（`config.ts:138-170` loadWellknownEntry）要求 auth + 最新 KEY 凭据才产出文档。

**ACTUAL**：
- `oc-core/src/integration/wellknown_config.rs:130-200` `refresh_wellknown_documents`：auth+最新 KEY 凭据门控、`{env:}` 替换、manifest.config + remote_config（Config 记录或整包体）→ Loader 最低优先层——与 ref config 消费路径逐条对齐 ✅（含 kv key `wellknown:sources` `:17`）。
- `oc-server/src/handlers/integration.rs:59,138-200`：`POST /api/experimental/integration/wellknown`（add）对齐（错误文案前缀一致）✅。
- **缺口**：未见 wellknown → integration 的 `login` command 方法注册与 integration name=hostname（`wellknown/plugin.ts:14-30`）；`remove`/`refresh`/`entries` 无 HTTP 面（v2 也无），但服务级 refresh 触发（wellknown.updated→integration.reload）不存在——Rust 的刷新是 host 定时/凭据事件驱动，语义近似。

- 分类：**PORTED（文档解析/配置层）+ PARTIAL（plugin 注册面）**；v2 使用；S2；KNOWN（BE-3b）+ NEW（login 方法注册缺口）。

## 10. workspace.ts / workspace/

**EXPECTED**：`workspace.ts:71-328` 逻辑工作区生命周期（create 冲突检测、provision 共享尝试+binding 持久化、connect 唤醒/引用计数、20 分钟空闲挂起轮询、destroy 竞态处理）；`workspace/driver.ts` Binding/Registry 契约。

**ACTUAL**：`database/bootstrap.rs:232` 建了 `workspace` 表（列与 `workspace/sql.ts` 一致）；`oc-core/src/project.rs:407-441` 仅 `insert_workspace`/`list_workspaces` 表助手（生产无调用方）；`engine/mod.rs:2067-2077` 事件盖章透传 workspaceID；`handlers/common.rs:260` 位置解析接受 workspaceID。**生命周期服务整体缺失**。

**影响判定**：v2.0.24 的 core driver 注册表为空、protocol 无 workspace API 组、CLI/TUI 不设置 workspaceID——该服务只有 SDK 嵌入（`sdk/src/internal/host.ts:42`）可达。因此对 Rust 的 v2 CLI/TUI/server 主线**无用户可见影响**；数据库层已就绪。若客户端传了非法 workspaceID：v2 在 Environment 绑定时 defect，Rust 静默当标签用（边缘）。

- 分类：**MISSING（服务）/ 表级 PORTED**；v2 主线不使用；S2；NEW（auditE 的 "workspace ✅" 仅指表/解析层）。

## 11. image.ts / image/（图片规整）

**EXPECTED**：
- `image.ts:58-93`：normalize 服务；默认限制 autoResize=true、2000×2000、base64 5MB；`media.image` 配置覆盖（`config/plugin/image.ts`）。
- `image/photon.ts:33-107`：尺寸/base64 超限且 autoResize 时按 0.75 阶梯缩放（Lanczos3），PNG→JPEG(80/85/70/55/40) 编码阶梯，按 base64 预算（`ceil(len/3)*4`）选第一个达标的；不可达→SizeError；解码失败→DecodeError。
- 消费点：附件（`session/prompt.ts:135-153`，SizeError→AttachmentError）；工具图片输出（`tool.ts:63-100`，decode/size 失败→替换为 `[N image(s) omitted: ...]` 文本注记）。

**ACTUAL**：**无 resizer**。`oc-server/src/handlers/attachment.rs:13-20` 自述"永远走直通臂"；`oc-schema/src/config/media.rs:7-17` 解析 `media.image` 配置但无消费者；工具输出图片无 omit 注记路径。

**影响**：超限图片（>5MB base64 或 >2000px）原样发给提供方（v2 会缩到合规）——大截图/照片附件在严格提供方处报错或巨量 token；损坏图片不会转为 omit 注记。用户日常可感。

- 分类：**MISSING**；v2 使用（附件+工具输出）；S2；KNOWN（auditE §1.17 ❌ normalize）。

## 12. websearch.ts / plugin/websearch/*

**对齐部分** ✅：服务主体（选择 kv `websearch:provider`、config 选择优先、random 亲和+冷却轮转、429 仅 random 轮转、ProviderRequired/NotFound/Disabled 错误面）在 `tool/websearch.rs`；5 家 provider（exa/tavily/firecrawl/parallel/tinyfish）+ MCP-over-HTTP 传输（`websearch_providers.rs`，端点 mcp.exa.ai 等与 ref `plugin/websearch/mcp.ts` 一致）；engine 接线 `engine/mod.rs:695-701`（auditE 时代的"零 provider 注册"已修复）。

**分歧**：
1. **存储凭据不可用**：ref 在 execute 时经 `ctx.integration.connection.active("exa")` 解析已保存的 API key 凭据（`plugin/websearch/exa.ts:50-53` 等）；Rust 仅认进程环境变量（`websearch_providers.rs:479-500` 自述"integration host 是后续 lane"，但该 host 现已存在且未接入 websearch）。用户经 auth 界面保存的搜索 key 在 Rust 下不生效 → ProviderRequired。S2；NEW。
2. **retry-after 不解析**：ref `websearch.ts:215-221` cooldownMillis 解析秒数或 HTTP 日期（缺省 60s）；Rust 固定 `COOLDOWN_MS=60_000`（`websearch.rs:21,237`）。S2；NEW。
3. **preferred 无清理**：ref 订阅 `SessionEvent.Deleted/Moved` 清理会话亲和项（`websearch.ts:94-101`）；Rust 的 `preferred` map 永不清理（`websearch.rs:55`）。轻微泄漏。S2；NEW。
4. 陈旧存储选择的错误类型：存了未注册 provider id 时 ref 走 defaultProvider→undefined→`ProviderRequired`，Rust `stored_selection`→`ProviderNotFound`（`websearch.rs:104-110` vs ref `websearch.ts:119-131`；ref 还会清除非法 kv 值）。S2；NEW。

- 分类：**PARTIAL**；v2 使用；S2；KNOWN（接线）+ NEW（1-4）。

## 13. file-retention.ts / location-activity.ts

- **file-retention**：逻辑已移植（`tool_output.rs:275-300` mtime 截断删除，7 天 `:79`）。**但两个生产清理任务都从未启动**：`tool_output::spawn_cleanup_task`（`:304-313`）与 shell 的等价清扫（`shell.rs:37` `RETENTION_MS` 定义后全仓无引用；v2 是 `shell.ts:104-109` 每小时扫 `<data>/shell/<project>/sh_*.out`）。后果：`<data>/shell/**` 与 tool-output 溢出文件**无限增长**。S2；NEW（函数在、接线漏）。
- **location-activity**：`location_services.rs:43-49`（TTL 3600s/扫描 60s）+ engine `activity_sweeper` 默认 true（`engine/mod.rs:183-184`，测试显式关）——已接线 ✅。KNOWN。

## 14. integration/connection.ts

ref 仅 schema 再导出 + 连接 key 助手（`credential:{id}` / `env:{name}`，`:18-26`）。Rust integration 模块（`oc-core/src/integration/`）承载等价概念（types.rs 的 CredentialValue::Key/Env、连接解析），属 integration lane 主覆盖；本 lane 无额外发现。✅ PORTED（该 lane 范围内）。

## 15. codemode（core 侧核对）

- catalog：`tool/codemode.rs:500-502` 常量（DESC 120/4 字符每 token/预算 2000）与 `catalog.ts:48-50` 一致；summarize 轮转算法对齐（`:544-650`）；searchSignature/search 输出 schema 字节级钉住（`:453-497,1068`，w9w4 实测）。instructions render/update 文案齐全（`:657-810`，含空目录文案 `:669`、更新替换文案 `:801`）。execute 描述与 `tool.ts:62-69` 一致（`:59`）。core/codemode 指令键接入 runner（`context.rs:495`）。
- **分歧**：`pinned` 未实现（`:546-548` 自述"注册项无 pinned 选项"）。v2 中 `opencode.session_move`/`session_rename`/`models` 为 `pinned: true`（`tool/plugin/opencode.ts:115`）——预算内永远展示；Rust 里它们参与轮转。实践中 opencode 命名空间极小（3 条短签名），2000 预算下几乎必然全展示，影响有限。S2；NEW。

- 分类：**PORTED（core 面）**；v2 使用；KNOWN。

---

## 16. 按严重度汇总（行动清单）

### S1（线格式/功能破坏，v2 主线可感）
| # | 问题 | ref | rust | 建议 |
|---|---|---|---|---|
| 1 | `/api/vcs/base` 缺必填 `source` 字段（强类型客户端解码失败）；且 base 语义错（merge-base @{u} ≠ reflog/default review base；无可用 base 应 503） | plugin/vcs/git.ts:286-318；schema/vcs.ts:23-27 | handlers/project.rs:628-641 | 按 VcsGitPlugin 移植 reflog 推导 + source + 503 |
| 2 | `/api/vcs/branch` 返回 `{"branches":[...]}`，schema 为 `string[]`；且仅本地分支、无 committerdate 排序/忽略大小写模式 | plugin/vcs/git.ts:228-242；schema/vcs.ts:16 | handlers/project.rs:643-667 | for-each-ref 对齐 + 返回裸数组 |
| 3 | `/api/vcs/diff` 返回 `{path,status:{type},chunks}` 伪条目，schema 为 `FileDiff.Info[]`；接受不存在的 `staged` 模式、忽略 branch/committed 语义、无 10MB 封顶/emptyPatch | vcs.ts:228-251；plugin/vcs/git.ts:101-607；vcs/patch.ts | handlers/project.rs:669-699 | 移植三模式 + 批量/逐文件补丁 + 封顶 |
| 4 | `worktree.remove` 只删 DB 行，不执行 `git worktree remove`（无 --force/forceRequired） | git.ts:669-681,625-647 | handlers/project.rs:563-598 | 接 git 子进程 + 脏树检测 |

（Rust TUI 自身 `app.rs:4139-4206` 与 v2 官方 TUI 都用强类型客户端——#1-#3 修复前，TUI 的 branch/committed/working diff 视图对 Rust 服务器不可用。）

### S2（行为分歧/缺失，按影响排序）
| # | 问题 | 位置 | 分类 |
|---|---|---|---|
| 5 | 图片 normalize/resize 全缺（附件+工具输出直通；`media.image` 配置无消费者） | ref image.ts+photon.ts vs attachment.rs:13-20, config/media.rs | MISSING（KNOWN auditE） |
| 6 | reference：git 源不物化（repository-cache/clone/sync 全缺）、`/api/reference` git path=仓库 URL、`core/reference-guidance` 恒"无引用" | ref reference.ts/repository-cache.ts vs agent.rs:106-160, context.rs:620-634 | MISSING/STUB（NEW） |
| 7 | shell/tool-output 保留清理任务未 spawn（磁盘无限增长） | ref shell.ts:104-109/tool-output.ts vs shell.rs:37, tool_output.rs:304（无调用方） | PARTIAL-接线（NEW） |
| 8 | snapshot：symlink 捕获/恢复错误；diff 忽略 context（固定 4 行 vs v2 全文件）；无 10MB 封顶/二进制空补丁；无 gitconfig/alternates seed | ref git.ts/snapshot.ts/vcs/patch.ts vs snapshot.rs:218-225,327-345,603,435-476 | PARTIAL/WRONG（NEW） |
| 9 | vcs 细节：info 缺 branch.default、detached HEAD current="HEAD"、status 的 additions/deletions 恒 0+scope 在 canonical 而非 location 目录+未排序、init 错误 kind 不全、hg provider 缺失 | plugin/vcs/git.ts vs handlers/project.rs:604-768 | PARTIAL（KNOWN+NEW 细节） |
| 10 | modal 动态模型同步缺失 | ref modal/models.ts+plugin/provider/modal.ts | MISSING（NEW） |
| 11 | websearch：不读已保存凭据（仅 env）、retry-after 不解析、preferred 无会话清理、陈旧选择错误类型 | ref websearch.ts:94-131,215-221 vs tool/websearch.rs:21,104-110,197-249, websearch_providers.rs:479-500 | PARTIAL（NEW） |
| 12 | codemode `pinned` 未实现（opencode.* 工具参与预算轮转） | ref tool/plugin/opencode.ts:115, codemode/catalog.ts:79-90 vs tool/codemode.rs:546-548 | PARTIAL（NEW） |
| 13 | wellknown 的 integration login-command 注册面缺失 | ref wellknown/plugin.ts:14-30 | PARTIAL（NEW） |
| 14 | workspace 生命周期服务缺失（表已建；v2 主线不经 HTTP 暴露，SDK 嵌入专用） | ref workspace.ts vs project.rs:407-441 | MISSING（主线不影响） |
| 15 | environment 执行平面缝缺失（两侧均无 driver，本地行为等价；远程工作区就绪度差异） | ref environment/* vs 直用 std::fs | PARTIAL-架构（NEW nuance） |

### 无发现（核对通过）
- v1 配置迁移（agent/command/mcp/permission/plugin/provider/provider-options 全特例）✅
- wellknown 文档解析与 config 最低优先层接入 ✅
- location-activity TTL 扫描（已接线）✅
- `vcs.branch.updated` 事件（HEAD 轮询等价实现）✅
- worktree.create（`git worktree add --detach`）✅
- codemode catalog/instructions 字节级钉住 ✅

---

## 17. 覆盖清单（本次实际走查的文件）

参考侧（全读）：`codemode/{catalog,instructions,tool,web}.ts`；`environment/{driver,environment,exec-defaults,files,index,local,memory,unavailable}.ts`；`v1/config/{agent,command,error,mcp,migrate,permission,plugin,provider,provider-options}.ts`；`modal/models.ts`；`git.ts`；`vcs.ts`；`vcs/patch.ts`；`plugin/vcs/{git,hg}.ts`（hg 只核注册面）；`file-retention.ts`；`location-activity.ts`；`reference.ts`；`reference/instructions.ts`；`wellknown.ts`；`wellknown/plugin.ts`；`workspace.ts`；`workspace/{driver,sql}.ts`；`image.ts`；`image/photon.ts`；`websearch.ts`；`integration/connection.ts`；`plugin/websearch/{index,mcp,exa}.ts`（其余 provider 抽查）；辅证：`snapshot.ts`、`plugin/internal.ts`、`plugin/provider/modal.ts`、`server/src/handlers/{vcs,reference}.ts`、`server/src/workerd.ts`、`schema/src/{vcs,file-diff,config/media,config}.ts`、`session/prompt.ts`、`tool.ts`、`config.ts`、`shell.ts`、`tool-output.ts`、`worktree/git.ts`、`util/git-executable.ts`、`sdk/src/internal/host.ts`。

Rust 侧（全读或关键段）：`oc-core/src/{snapshot.rs,project.rs,location_services.rs,tool_output.rs,shell.rs,tool/codemode.rs,tool/websearch.rs,tool/websearch_providers.rs,tool/file_diff.rs,session/runner/context.rs,session/diff.rs,config/normalize.rs,v2_migration.rs,integration/wellknown_config.rs,integration/providers.rs}`；`oc-server/src/{handlers/{project.rs(vcs/worktree 段),agent.rs(reference 段),integration.rs(wellknown 段),attachment.rs},announce.rs}`；`oc-schema/src/{config/media.rs,location.rs,misc.rs}`；`oc-client/src/{types.gen.rs,methods.gen.rs}`（vcs/websearch/filediff 类型与调用）；`oc-tui/src/app.rs`（diff 拉取段）；`database/bootstrap.rs`（workspace 表）；交叉文档：`ROADMAP.md`、`FINDINGS.md`、`docs/auditE-core.md`、`.slim/deepwork/rust-port/{w9w4,w12glue,progress}.md`、`MIGRATION.md`、`PORT_REPORT.md`。

### 判定口径备注
- 「v2 是否实际使用」以 v2 CLI/TUI/server 主线可达性为准（SDK 嵌入专属能力标"否"）。
- KNOWN 判定优先引用 `docs/auditE-core.md`（2026-10 前的 core 审计）与 `.slim/deepwork` 泳道结论文档；上述文档未覆盖的差异标 NEW。
- `v2_migration.rs` 是**数据目录布局迁移**（DB/auth/config/state 拷贝+锁），与 `v1/config`（v1 JSON→v2 配置归一化）是两件事，后者在 `config/normalize.rs`——两者都存在且分工正确。
