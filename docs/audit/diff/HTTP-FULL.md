# HTTP-FULL Differential（差分层 · 通道 6 · 全量 HTTP 对拍）

> 同一 corpus（4 族）以相同顺序打双侧**真实 server 进程**：
> rust = `oc-server serve --port 0`（boot 输出 `http://127.0.0.1:33371`，AuthConfig password=None 语义，roots → /tmp）
> ref = bun 驱动 `tools/diff/drivers/http_ref_server.ts`（ServerFetch.make + Bun.serve，`http://127.0.0.1:39285`，roots → /tmp，ref git 树零写入）
> 归一化：键序排序、id/uuid/时间戳/pid/version/端口/绝对路径（→ `<TMP>`/`<WS>`）剥换、列表按多重集比较、SSE 只比首帧+事件名。
> 复现：`python3 tools/diff/http_full.py`（corpus 内嵌，60 请求）

## 范围声明

- 不覆盖：网络出口（provider/models-dev 实际调用，双侧均禁网）、password 鉴权态、workerd 剖面、插件安装路径。
- wp 局限：ref 侧经 ServerFetch（web-handler embedder 入口）而非 node:http ServerProcess（后者强制 password），
  故 auth 拦截层与信号处理不在对拍面内；路由/序列化层与真实进程一致。
- SSE 只比首帧形状与事件名多重集；帧序/心跳节奏声明为 ADJUDICATED（依赖 boot 期环境总线流量）。

- **MATCH：46/60** · DIVERGE：**12** · ADJUDICATED：**2** · MISSING：**0**

| 族 | 用例 | 判定 |
|---|---|---|
| L1-readonly | `info` | MATCH |
| L1-readonly | `config` | MATCH |
| L1-readonly | `agent` | MATCH |
| L1-readonly | `command` | MATCH |
| L1-readonly | `skill` | DIVERGE |
| L1-readonly | `model` | ADJUDICATED |
| L1-readonly | `model_default` | MATCH |
| L1-readonly | `provider` | MATCH |
| L1-readonly | `form` | MATCH |
| L1-readonly | `location` | MATCH |
| L1-readonly | `integration` | DIVERGE |
| L1-readonly | `fs_list_default` | MATCH |
| L1-readonly | `fs_find` | MATCH |
| L1-readonly | `project` | DIVERGE |
| L1-readonly | `worktree` | MATCH |
| L1-readonly | `mcp` | MATCH |
| L2-errors | `session_missing_get` | MATCH |
| L2-errors | `session_missing_patch` | MATCH |
| L2-errors | `session_missing_delete` | MATCH |
| L2-errors | `session_missing_context` | MATCH |
| L2-errors | `session_missing_message` | MATCH |
| L2-errors | `session_create_no_body` | MATCH |
| L2-errors | `session_create_bad_location` | DIVERGE |
| L2-errors | `agent_missing` | DIVERGE |
| L2-errors | `skill_missing` | MATCH |
| L2-errors | `command_missing` | MATCH |
| L2-errors | `shell_missing_get` | MATCH |
| L2-errors | `fs_read_missing` | DIVERGE |
| L2-errors | `config_shell` | MATCH |
| L3-side-effects | `session_create` | DIVERGE |
| L3-side-effects | `session_get` | DIVERGE |
| L3-side-effects | `session_rename` | MATCH |
| L3-side-effects | `session_context` | MATCH |
| L3-side-effects | `session_inbox` | MATCH |
| L3-side-effects | `session_list_one` | DIVERGE |
| L3-side-effects | `permission_saved_empty` | MATCH |
| L3-side-effects | `shell_create` | MATCH |
| L3-side-effects | `shell_get` | MATCH |
| L3-side-effects | `shell_list` | MATCH |
| L3-side-effects | `shell_output` | MATCH |
| L3-side-effects | `shell_delete` | MATCH |
| L3-side-effects | `shell_list_after` | MATCH |
| L3-side-effects | `session_delete` | MATCH |
| L3-side-effects | `session_list_empty_again` | DIVERGE |
| L4-sse | `sse_first_frame` | MATCH |
| L4-sse | `sse_name_sequence` | ADJUDICATED |
| L4-sse | `sse_reconnect_first_frame` | MATCH |
| L2-errors | `vcs_uninit_get` | DIVERGE |
| L2-errors | `vcs_base_uninit` | MATCH |
| L2-errors | `vcs_diff_uninit` | MATCH |
| L2-errors | `vcs_init_missing_dir` | MATCH |
| L2-errors | `vcs_init_unknown_provider` | MATCH |
| L2-errors | `worktree_get_empty` | MATCH |
| L2-errors | `worktree_create_no_body` | MATCH |
| L2-errors | `fs_list_missing_dir` | MATCH |
| L2-errors | `provider_missing` | DIVERGE |
| L2-errors | `session_missing_fork` | MATCH |
| L2-errors | `config_patch_no_body` | MATCH |
| L3-side-effects | `config_patch_shell` | MATCH |
| L3-side-effects | `debug_location` | MATCH |

## 分歧明细（双侧 file:line 锚 + 定性）
### `skill`（L1-readonly · GET `/api/skill`）
- 锚点：rust `crates/oc-server/src/routes.gen.rs:2064` · ref `packages/protocol/src/groups/skill.ts:9`
- 定性：rust 序列化 autoinvoke:null（ref 省略缺省键）+ 内置 skill 正文文本漂移
- `body` 分歧（首个结构差叶路径，rust vs ref）：
  - `$.data[0].autoinvoke: <absent on ref>`
  - `$.data[1].autoinvoke: <absent on ref>`
  - `$.data[1].content: "<!--\n  Built-in skill. Name and description are registered in code at\n  packages/core/src/plugin/skill.ts. The body below becomes the skill's\n  content.\n-->\n\n# Report an opencode Issue\n\nUse t vs "<!--\n  Built-in skill. Name and description are registered in code at\n  packages/core/src/plugin/skill.ts. The body below becomes the skill's\n  content.\n-->\n\n# Report an opencode Issue\n\nUse t`

### `integration`（L1-readonly · GET `/api/integration`）
- 锚点：rust `crates/oc-server/src/routes.gen.rs:779` · ref `packages/protocol/src/groups/integration.ts:16`
- 定性：目录条目数漂移（224 vs 229，目录版本差异 + 疑似过滤差异）
- `body` 分歧（首个结构差叶路径，rust vs ref）：
  - `$.data: list len 224 vs 229`

### `project`（L1-readonly · GET `/api/project`）
- 锚点：rust `crates/oc-server/src/routes.gen.rs:1190` · ref `packages/protocol/src/groups/project.ts:6`
- 定性：rust 输出 commands/icon/name/vcs:null；ref 省略缺省键（序列化形状）
- `body` 分歧（首个结构差叶路径，rust vs ref）：
  - `$[0].commands: <absent on ref>`
  - `$[0].icon: <absent on ref>`
  - `$[0].name: <absent on ref>`
  - `$[0].vcs: <absent on ref>`

### `session_create_bad_location`（L2-errors · POST `/api/session`）
- 锚点：rust `crates/oc-server/src/middleware.rs:386` · ref `packages/protocol/src/groups/session.ts:180`
- 定性：语义分歧：session.create 对不存在目录 rust=404 LocationNotFound；ref=200 创建悬挂 placement session（fetch.test.ts 声明 create 不 boot Location 图）
- `status` 分歧：
  - rust = `404`
  - ref  = `200`
- `body` 分歧（首个结构差叶路径，rust vs ref）：
  - `$._tag: <absent on ref>`
  - `$.data: <absent on rust>`
  - `$.location: <absent on ref>`
  - `$.message: <absent on ref>`

### `agent_missing`（L2-errors · GET `/api/agent/does-not-exist`）
- 锚点：rust `?` · ref `?`
- 定性：错误 message 文本：rust 缺失 offender 后缀（'Agent not found' vs 'Agent not found: does-not-exist'）
- `body` 分歧（首个结构差叶路径，rust vs ref）：
  - `$.message: "Agent not found" vs "Agent not found: does-not-exist"`

### `fs_read_missing`（L2-errors · GET `/api/fs/read/diff-no-such-file.txt`）
- 锚点：rust `?` · ref `?`
- 定性：错误 message 文本：rust 缺失 path 后缀（同上模式）
- `body` 分歧（首个结构差叶路径，rust vs ref）：
  - `$.message: "File not found" vs "File not found: diff-no-such-file.txt"`

### `session_create`（L3-side-effects · POST `/api/session`）
- 锚点：rust `crates/oc-server/src/middleware.rs:386` · ref `packages/protocol/src/groups/session.ts:180`
- 定性：cost 序列化 0.0(float) vs 0(int)；rust 额外输出 slug/subpath 字段
- `body` 分歧（首个结构差叶路径，rust vs ref）：
  - `$.data.cost: type float vs int`
  - `$.data.slug: <absent on ref>`
  - `$.data.subpath: <absent on ref>`

### `session_get`（L3-side-effects · GET `/api/session/{SID}`）
- 锚点：rust `crates/oc-server/src/middleware.rs:386` · ref `packages/protocol/src/groups/session.ts:180`
- 定性：同 session_create（cost 类型 + slug/subpath）
- `body` 分歧（首个结构差叶路径，rust vs ref）：
  - `$.data.cost: type float vs int`
  - `$.data.slug: <absent on ref>`
  - `$.data.subpath: <absent on ref>`

### `session_list_one`（L3-side-effects · GET `/api/session`）
- 锚点：rust `crates/oc-server/src/middleware.rs:386` · ref `packages/protocol/src/groups/session.ts:180`
- 定性：ref 列表包含 bad-location 会话（rust 已 404 拒绝创建）→ 级联差异
- `body` 分歧（首个结构差叶路径，rust vs ref）：
  - `$.data: list len 1 vs 2`

### `session_list_empty_again`（L3-side-effects · GET `/api/session`）
- 锚点：rust `crates/oc-server/src/middleware.rs:386` · ref `packages/protocol/src/groups/session.ts:180`
- 定性：ref cursor.next/previous 恒返回锚点（即使空页/单页）；rust 返回 null；且 ref 残留悬挂会话
- `body` 分歧（首个结构差叶路径，rust vs ref）：
  - `$.cursor.next: type NoneType vs str`
  - `$.cursor.previous: type NoneType vs str`
  - `$.data: list len 0 vs 1`

### `vcs_uninit_get`（L2-errors · GET `/api/vcs`）
- 锚点：rust `crates/oc-server/src/request_validation.rs:1006` · ref `packages/protocol/src/groups/vcs.ts:36`
- `body` 分歧（首个结构差叶路径，rust vs ref）：
  - `$.data: type NoneType vs dict`

### `provider_missing`（L2-errors · GET `/api/provider/does-not-exist`）
- 锚点：rust `?` · ref `?`
- `body` 分歧（首个结构差叶路径，rust vs ref）：
  - `$.message: "Provider not found" vs "Provider not found: does-not-exist"`

## 判定

- 已裁定：60/60（MATCH+ADJUDICATED+DIVERGE 逐例有归属；DIVERGE 为登记册候选，见上表锚点）
- ADJUDICATED 2 项：归一化声明见范围声明与 corpus 注释（SSE 名序 / 目录清单成员），非 DIVERGE。
- 稳定性：双侧 boot 各重试一次窗口（本次未触发即成功）、每请求独立超时 (20.0s)、SSE 短窗 4.0s、子进程按进程组回收（SIGTERM→SIGKILL）。
- ref git 树零写入（运行前后 `git status --porcelain` 均空）。
