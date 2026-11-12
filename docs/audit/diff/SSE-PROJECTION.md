# SSE-PROJECTION Differential（差分层 · 通道 3）

> 同一 SSE 事件**序列**（多事件流：正常/中断/乱序/重复/G-351、G-352 修复位专项）
> 喂双侧数据投影层，逐事件快照消息/部件投影状态后对拍。
> ref = bun 驱动 `packages/client/src/solid/data.ts` `createData`（stub api + 假事件总线，纯事件投影）；
> rust = env 门控集成测试驱动真实 `Store`（`Ingest::Event` 生产单入口 reducer）。
> 复现：`python3 tools/diff/sse_projection.py`（corpus：/opt/opencode-rust/tools/diff/corpus/sse_projection）

## 范围声明

- **仅数据模型/投影层**（消息树、部件增量、tool 状态机、live 行插入）；**渲染差异不在本通道**——
  由 opentui 差分 127 场景另行覆盖。
- `message.updated` / `message.part.updated` 全量快照投影不在此对拍：ref 侧该事件由 app 层
  消费（data.ts 无此 case），两侧投影环境不对称，声明为通道外。
- ref 侧 HTTP 失效化重取（`refresh(...)`）全部以 stub api 拒绝，保证投影纯事件驱动；
  依赖 `session.info` 的门控分支（如 `session.moved`）在纯事件流下不可达，见分歧明细。

## 归一化（双侧一致，已声明）

- JSON 键序递归排序；**剥离消息 id**（rust `live_message_id` vs ref `evt_`→`msg_` 派生源差异）。
- **剥离时间戳数值**，仅保留语义存在性：assistant `open`（completed 未设）、reasoning `completed`。
- assistant model → `providerID/id`（ref 保留对象、variant 丢弃）。
- tool `input`：streaming 期字符串累加器两侧掩码为 `<streaming-input>`（rust 无 streaming-input
  reducer——设计上 display-inert，见分歧明细）；settled 后比较字符串化 map。
- tool `metadata` 比较原始 JSON（rust `metadata_json` / ref `state.metadata`）。
- assistant tokens/usage/cost/retry/providerState 排除（ref 保留原始对象、rust 保存派生数值，模型不同构）。

## 比较字段覆盖

- row kind
- assistant agent/model/finish/open
- part order & kinds
- text accumulation
- reasoning text + completed
- tool status transitions (streaming→running→completed/error)
- tool input map (settled)
- tool metadata (raw JSON)
- tool output (joined content)
- tool error
- live rows: agent/model/location/system/synthetic/shell

- **MATCH：1/12** · DIVERGE：**11** · MISSING：**0**（序列 12 条 / 事件 93 个 / 快照 93 对）

| 序列 | 首个分歧 | 已归属已知成因 | 成因键 |
|---|---|---|---|
| `01-normal-text-flow` | event[1] parts:1vs0; 终态一致（瞬态分歧） | 是 | session.text.started |
| `02-reasoning-flow` | event[5] parts:1vs2; 终态分歧 | 是 | session.text.started, unanchored delta |
| `03-tool-lifecycle` | event[7] parts:1vs2; 终态分歧 | 是 | unanchored delta |
| `05-multipart-interleave` | event[4] parts:2vs1; 终态一致（瞬态分歧） | 是 | session.text.started |
| `06-ordinal-stress` | event[1] parts:0vs1; 终态分歧 | 是 | unanchored delta, replay dedup |
| `07-interrupted-mid-tool` | event[1] parts:0vs1; 终态分歧 | 是 | unanchored delta |
| `08-g351-whitespace-anchor` | event[1] parts:1vs0; 终态一致（瞬态分歧） | 是 | session.text.started |
| `09-g352-live-six` | event[4] rows:3vs4; 终态分歧 | 是 | moved gating, instructions empty text |
| `10-late-delta-no-message` | event[0] rows:0vs1; 终态分歧 | 是 | unanchored delta |
| `11-empty-final-anchors` | event[4] parts:2vs1; 终态一致（瞬态分歧） | 是 | whitespace reasoning delta, empty final, empty finish |
| `12-post-completion-late-delta` | event[1] parts:1vs0; 终态分歧 | 是 | session.text.started, post-completion delta |

## 分歧明细（首个分歧事件的投影快照）

### `01-normal-text-flow`
- 首个分歧：事件 #1（`01-normal-text-flow` 序列内 0 基）；终态一致
- ref  #1：`[{"agent": "build", "finish": null, "model": "anthropic/claude-sonnet-4-5", "open": true, "parts": [{"text": "", "type": "text"}], "row": "assistant"}]`
- rust #1：`[{"agent": "build", "finish": null, "model": "anthropic/claude-sonnet-4-5", "open": true, "parts": [], "row": "assistant"}]`
- ref  终态：`[{"agent": "build", "finish": "stop", "model": "anthropic/claude-sonnet-4-5", "open": false, "parts": [{"text": "Hello, world!", "type": "text"}], "row": "assistant"}]`
- rust 终态：`[{"agent": "build", "finish": "stop", "model": "anthropic/claude-sonnet-4-5", "open": false, "parts": [{"text": "Hello, world!", "type": "text"}], "row": "assistant"}]`

### `02-reasoning-flow`
- 首个分歧：事件 #5（`02-reasoning-flow` 序列内 0 基）；终态分歧
- ref  #5：`[{"agent": "build", "finish": null, "model": "anthropic/claude-sonnet-4-5", "open": true, "parts": [{"completed": true, "text": "Think step by step", "type": "reasoning"}], "row": "assistant"}]`
- rust #5：`[{"agent": "build", "finish": null, "model": "anthropic/claude-sonnet-4-5", "open": true, "parts": [{"completed": true, "text": "Think step by step", "type": "reasoning"}, {"text": "Answer", "type": "text"}], "row": "assistant"}]`
- ref  终态：`[{"agent": "build", "finish": "stop", "model": "anthropic/claude-sonnet-4-5", "open": false, "parts": [{"completed": true, "text": "Think step by step", "type": "reasoning"}], "row": "assistant"}]`
- rust 终态：`[{"agent": "build", "finish": "stop", "model": "anthropic/claude-sonnet-4-5", "open": false, "parts": [{"completed": true, "text": "Think step by step", "type": "reasoning"}, {"text": "Answer", "type": "text"}], "row": "assistant"}]`

### `03-tool-lifecycle`
- 首个分歧：事件 #7（`03-tool-lifecycle` 序列内 0 基）；终态分歧
- ref  #7：`[{"agent": "build", "finish": null, "model": "anthropic/claude-sonnet-4-5", "open": true, "parts": [{"error": null, "id": "tool_1", "input": {"filePath": "/tmp/a.ts"}, "metadata": {"done": "true"}, "name": "read", "output": "line1\nline2", "status": "completed", "type": "tool"}], "row": "assistant"}]`
- rust #7：`[{"agent": "build", "finish": null, "model": "anthropic/claude-sonnet-4-5", "open": true, "parts": [{"error": null, "id": "tool_1", "input": {"filePath": "/tmp/a.ts"}, "metadata": {"done": "true"}, "name": "read", "output": "line1\nline2", "status": "completed", "type": "tool"}, {"text": "done", "type": "text"}], "row": "assistant"}]`
- ref  终态：`[{"agent": "build", "finish": "stop", "model": "anthropic/claude-sonnet-4-5", "open": false, "parts": [{"error": null, "id": "tool_1", "input": {"filePath": "/tmp/a.ts"}, "metadata": {"done": "true"}, "name": "read", "output": "line1\nline2", "status": "completed", "type": "tool"}], "row": "assistant"}]`
- rust 终态：`[{"agent": "build", "finish": "stop", "model": "anthropic/claude-sonnet-4-5", "open": false, "parts": [{"error": null, "id": "tool_1", "input": {"filePath": "/tmp/a.ts"}, "metadata": {"done": "true"}, "name": "read", "output": "line1\nline2", "status": "completed", "type": "tool"}, {"text": "done", "type": "text"}], "row": "assistant"}]`

### `05-multipart-interleave`
- 首个分歧：事件 #4（`05-multipart-interleave` 序列内 0 基）；终态一致
- ref  #4：`[{"agent": "plan", "finish": null, "model": "openai/gpt-5", "open": true, "parts": [{"completed": true, "text": "planning", "type": "reasoning"}, {"text": "", "type": "text"}], "row": "assistant"}]`
- rust #4：`[{"agent": "plan", "finish": null, "model": "openai/gpt-5", "open": true, "parts": [{"completed": true, "text": "planning", "type": "reasoning"}], "row": "assistant"}]`
- ref  终态：`[{"agent": "plan", "finish": "tool_use", "model": "openai/gpt-5", "open": false, "parts": [{"completed": true, "text": "planning", "type": "reasoning"}, {"text": "first part", "type": "text"}, {"error": null, "id": "tool_x", "input": {"path": "true", "pattern": "todo"}, "metadata": {}, "name": "grep", "output": "3 matches", "status": "completed", "type": "tool"}, {"text": "second part", "type": "t`
- rust 终态：`[{"agent": "plan", "finish": "tool_use", "model": "openai/gpt-5", "open": false, "parts": [{"completed": true, "text": "planning", "type": "reasoning"}, {"text": "first part", "type": "text"}, {"error": null, "id": "tool_x", "input": {"path": "true", "pattern": "todo"}, "metadata": {}, "name": "grep", "output": "3 matches", "status": "completed", "type": "tool"}, {"text": "second part", "type": "t`

### `06-ordinal-stress`
- 首个分歧：事件 #1（`06-ordinal-stress` 序列内 0 基）；终态分歧
- ref  #1：`[{"agent": "build", "finish": null, "model": "anthropic/claude-sonnet-4-5", "open": true, "parts": [], "row": "assistant"}]`
- rust #1：`[{"agent": "build", "finish": null, "model": "anthropic/claude-sonnet-4-5", "open": true, "parts": [{"text": "A", "type": "text"}], "row": "assistant"}]`
- ref  终态：`[{"agent": "build", "finish": "stop", "model": "anthropic/claude-sonnet-4-5", "open": false, "parts": [], "row": "assistant"}]`
- rust 终态：`[{"agent": "build", "finish": "stop", "model": "anthropic/claude-sonnet-4-5", "open": false, "parts": [{"text": "AA", "type": "text"}, {"text": "B", "type": "text"}, {"text": "C", "type": "text"}], "row": "assistant"}]`

### `07-interrupted-mid-tool`
- 首个分歧：事件 #1（`07-interrupted-mid-tool` 序列内 0 基）；终态分歧
- ref  #1：`[{"agent": "build", "finish": null, "model": "anthropic/claude-sonnet-4-5", "open": true, "parts": [], "row": "assistant"}]`
- rust #1：`[{"agent": "build", "finish": null, "model": "anthropic/claude-sonnet-4-5", "open": true, "parts": [{"text": "working", "type": "text"}], "row": "assistant"}]`
- ref  终态：`[{"agent": "build", "finish": "error", "model": "anthropic/claude-sonnet-4-5", "open": false, "parts": [{"error": null, "id": "tool_slow", "input": {"prompt": "x"}, "metadata": {"loaded": 2}, "name": "task", "output": null, "status": "running", "type": "tool"}], "row": "assistant"}]`
- rust 终态：`[{"agent": "build", "finish": "error", "model": "anthropic/claude-sonnet-4-5", "open": false, "parts": [{"text": "working", "type": "text"}, {"error": null, "id": "tool_slow", "input": {"prompt": "x"}, "metadata": {"loaded": "2"}, "name": "task", "output": null, "status": "running", "type": "tool"}], "row": "assistant"}]`

### `08-g351-whitespace-anchor`
- 首个分歧：事件 #1（`08-g351-whitespace-anchor` 序列内 0 基）；终态一致
- ref  #1：`[{"agent": "build", "finish": null, "model": "anthropic/claude-sonnet-4-5", "open": true, "parts": [{"text": "", "type": "text"}], "row": "assistant"}]`
- rust #1：`[{"agent": "build", "finish": null, "model": "anthropic/claude-sonnet-4-5", "open": true, "parts": [], "row": "assistant"}]`
- ref  终态：`[{"agent": "build", "finish": "stop", "model": "anthropic/claude-sonnet-4-5", "open": false, "parts": [{"text": " \n\n  indented\n", "type": "text"}], "row": "assistant"}]`
- rust 终态：`[{"agent": "build", "finish": "stop", "model": "anthropic/claude-sonnet-4-5", "open": false, "parts": [{"text": " \n\n  indented\n", "type": "text"}], "row": "assistant"}]`

### `09-g352-live-six`
- 首个分歧：事件 #4（`09-g352-live-six` 序列内 0 基）；终态分歧
- ref  #4：`[{"agent": "build", "finish": "stop", "model": "anthropic/claude-sonnet-4-5", "open": false, "parts": [], "row": "assistant"}, {"agent": "plan", "previous": null, "row": "agent-switched"}, {"model": "openai/gpt-5", "row": "model-switched"}]`
- rust #4：`[{"agent": "build", "finish": "stop", "model": "anthropic/claude-sonnet-4-5", "open": false, "parts": [], "row": "assistant"}, {"agent": "plan", "previous": null, "row": "agent-switched"}, {"model": "openai/gpt-5", "row": "model-switched"}, {"directory": "/work/wt-1", "row": "location-switched"}]`
- ref  终态：`[{"agent": "build", "finish": "stop", "model": "anthropic/claude-sonnet-4-5", "open": false, "parts": [], "row": "assistant"}, {"agent": "plan", "previous": null, "row": "agent-switched"}, {"model": "openai/gpt-5", "row": "model-switched"}, {"description": "Instructions updated: AGENTS.md", "row": "system"}, {"description": "Instructions updated: AGENTS.md", "row": "system"}, {"description": "Suba`
- rust 终态：`[{"agent": "build", "finish": "stop", "model": "anthropic/claude-sonnet-4-5", "open": false, "parts": [], "row": "assistant"}, {"agent": "plan", "previous": null, "row": "agent-switched"}, {"model": "openai/gpt-5", "row": "model-switched"}, {"directory": "/work/wt-1", "row": "location-switched"}, {"description": "Instructions updated: AGENTS.md", "row": "system"}, {"description": "Instructions upd`

### `10-late-delta-no-message`
- 首个分歧：事件 #0（`10-late-delta-no-message` 序列内 0 基）；终态分歧
- ref  #0：`[]`
- rust #0：`[{"agent": "build", "finish": null, "model": null, "open": true, "parts": [{"text": "early", "type": "text"}], "row": "assistant"}]`
- ref  终态：`[{"agent": "build", "finish": "stop", "model": "anthropic/claude-sonnet-4-5", "open": false, "parts": [], "row": "assistant"}]`
- rust 终态：`[{"agent": "build", "finish": "stop", "model": null, "open": false, "parts": [{"text": "early-late", "type": "text"}, {"completed": false, "text": "early-think", "type": "reasoning"}], "row": "assistant"}]`

### `11-empty-final-anchors`
- 首个分歧：事件 #4（`11-empty-final-anchors` 序列内 0 基）；终态一致
- ref  #4：`[{"agent": "build", "finish": null, "model": "anthropic/claude-sonnet-4-5", "open": true, "parts": [{"completed": true, "text": "", "type": "reasoning"}, {"text": "", "type": "text"}], "row": "assistant"}]`
- rust #4：`[{"agent": "build", "finish": null, "model": "anthropic/claude-sonnet-4-5", "open": true, "parts": [{"completed": true, "text": "", "type": "reasoning"}], "row": "assistant"}]`
- ref  终态：`[{"agent": "build", "finish": "", "model": "anthropic/claude-sonnet-4-5", "open": false, "parts": [{"completed": true, "text": "", "type": "reasoning"}, {"text": "", "type": "text"}], "row": "assistant"}]`
- rust 终态：`[{"agent": "build", "finish": "", "model": "anthropic/claude-sonnet-4-5", "open": false, "parts": [{"completed": true, "text": "", "type": "reasoning"}, {"text": "", "type": "text"}], "row": "assistant"}]`

### `12-post-completion-late-delta`
- 首个分歧：事件 #1（`12-post-completion-late-delta` 序列内 0 基）；终态分歧
- ref  #1：`[{"agent": "build", "finish": null, "model": "anthropic/claude-sonnet-4-5", "open": true, "parts": [{"text": "", "type": "text"}], "row": "assistant"}]`
- rust #1：`[{"agent": "build", "finish": null, "model": "anthropic/claude-sonnet-4-5", "open": true, "parts": [], "row": "assistant"}]`
- ref  终态：`[{"agent": "build", "finish": "stop", "model": "anthropic/claude-sonnet-4-5", "open": false, "parts": [{"text": "final text LATE", "type": "text"}], "row": "assistant"}]`
- rust 终态：`[{"agent": "build", "finish": "stop", "model": "anthropic/claude-sonnet-4-5", "open": false, "parts": [{"text": "final text LATE", "type": "text"}, {"completed": false, "text": " late-think", "type": "reasoning"}], "row": "assistant"}]`

## 成因清单（双侧 file:line）

- **session.text.started** — ref pushes an empty text part (`session.text.started` handler); the rust reducer table has NO arm — parts are created lazily by the first delta (`with_part`)
  - ref packages/client/src/solid/data.ts:887-892
  - rust crates/oc-tui/src/store.rs:13655-13696,14298-14358 (`with_part` 惰性建件)
- **unanchored delta** — a text/reasoning delta with no prior part of that kind: ref `editText`/`editReasoning` no-op (findLast misses); rust `with_part` creates/gap-fills the part (G-288⑧ 乱序 ordinal 补位)
  - ref packages/client/src/solid/data.ts:462-483,905-913,992-996
  - rust crates/oc-tui/src/store.rs:14298-14358
- **replay dedup** — same-id re-delivery: rust F2 replay window drops exact re-deliveries before any reducer; ref data.ts has no dedup (lives in the app SSE layer)
  - ref packages/client/src/solid/data.ts:593-1946 (无去重)
  - rust crates/oc-tui/src/store.rs:12742-12766 (`is_replayed`)
- **moved gating** — `session.moved` row gating asymmetry: rust inserts when the session_index learned the id from events; ref requires `store.session.info[sessionID]` (an API read) — never loaded in a pure event-stream diff
  - ref packages/client/src/solid/data.ts:704-726
  - rust crates/oc-tui/src/store.rs:13203-13228
- **instructions empty text** — `session.instructions.updated` with text "": rust dropped (empty guard); ref inserts (guards only `undefined`)
  - ref packages/client/src/solid/data.ts:791-804
  - rust crates/oc-tui/src/store.rs:13323-13343（**已修复 G-406**：仅缺字段跳过，空串生效）
- **whitespace reasoning delta** — whitespace-only reasoning delta: rust filtered (`trim().is_empty()` drop); ref appends verbatim — G-351 fixed TEXT only
  - ref packages/client/src/solid/data.ts:992-996
  - rust crates/oc-tui/src/store.rs:13717-13739（**已修复 G-393**：与 text 臂同规则）
- **empty final** — empty/whitespace final (`text.ended`/`reasoning.ended` with ""): rust skipped (`!text.is_empty()` / `trim().is_empty()` guards); ref assigns verbatim — ref also marks reasoning completed
  - ref packages/client/src/solid/data.ts:914-917,997-1002
  - rust crates/oc-tui/src/store.rs:13680-13752（**已修复 G-398**：空串终值赋值 + reasoning 标记 completed）
- **empty finish** — `step.ended` with finish "": rust filtered to None; ref keeps ""
  - ref packages/client/src/solid/data.ts:878-889
  - rust crates/oc-tui/src/store.rs:13942-13948（**已修复 G-398**：finish 空串保留）
- **post-completion delta** — delta after the message completed (G11-F1): rust dropped (`message_already_complete`); ref appends (no such guard)
  - ref packages/client/src/solid/data.ts:905-913
  - rust crates/oc-tui/src/store.rs:13655-13679（**已修复 G-399**：守卫改为仅快照权威完成（`snapshot_authoritative`，set_messages/MessagesSnapshot/MessagesOlder 标记、step.started 清除）；流内 step.ended 完成后迟到 delta 照 ref 追加）

## 判定

- 已裁定：12/12（每例双侧一致，或分歧已归属到上列成因并给出双侧 file:line）
- MATCH 1 例；DIVERGE 11 例中全部命中已知成因族
- 未归属成因的分歧为登记册候选（本批为 0；若后续 corpus 扩充出现新分歧，先人工定性再入册）
