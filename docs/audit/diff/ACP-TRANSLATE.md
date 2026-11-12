# ACP-TRANSLATE Differential（差分层 · 通道 2）

> 同一 session 事件 corpus 喂双侧转译纯函数（ref `packages/cli/src/acp/translate.ts`
> `fold/response/abandon` vs rust `crates/oc-cli/src/acp/translate.rs`），
> 归一化（键序排序、空 `_meta` 剥除——ref `PromptResponse` 恒带 `_meta:{}`、
> rust 侧 None 省略，属提取噪声已在归一化声明；两侧载荷值均来自 corpus，
> 无生成源不同的 id/时间戳字段需剥除）后对 ACP SessionUpdate wire 形状逐例对拍。
> 驱动：ref=bun（staging+`packages/cli/node_modules` 链接）/ rust=env 门控集成测试
> （`ACP_DIFF_CORPUS`/`ACP_DIFF_OUT`，未设 env 零成本）。
> 复现：`python3 tools/diff/acp_translate.py`（corpus：/opt/opencode-rust/tools/diff/corpus/acp，
> 由 `tools/diff/gen_acp_corpus.py` 幂等生成）。

- **MATCH：79/97** · DIVERGE：**18** · MISSING：**0**
- corpus 覆盖：97 例 · 全部 51 种 session 事件类型 + 载荷边界
- 观测到的 SessionUpdate kind（7/11 census 对齐）：agent_message_chunk, agent_thought_chunk, compaction_summary_chunk, compaction_update, session_info_update, tool_call, tool_call_update

| 用例 | 分歧字段 |
|---|---|
| `abandon-child-tool` | events |
| `abandon-tools-and-compaction` | events |
| `child-projection-meta` | events |
| `permission-ask-tracked-tool` | events |
| `reasoning-delta-ordinal-missing` | events |
| `reasoning-delta-ordinal-negative` | events |
| `reasoning-delta-ordinal-string` | events |
| `text-delta-missing-field` | events |
| `tool-called` | events |
| `tool-failed` | events |
| `tool-failed-long-message` | events |
| `tool-input-started` | events |
| `tool-lifecycle-shell` | events |
| `tool-progress` | events |
| `tool-success` | events |
| `tool-success-edit-diff-content` | events |
| `tool-success-multipart-all-parts` | events |
| `tool-success-read-preview-g339` | events |

## 分歧明细
### `abandon-child-tool` — events
- ref:
  - `{"abandon_outputs": [{"tag": "session_update", "update": {"_meta": {"opencode/child-session": {"depth": 1, "id": "ses_child1", "parentID": "ses_root"}}, "content": [{"content": {"text": "Cancelled", "type": "text"}, "type": "content"}], "kind": "read", "locations": [], "rawInput": {}, "rawOutput": {"error": "Cancelled", "metadata": {}}, "sessionUpdate": "tool_call_update", "status": "failed", "title": "read", "toolCallId": "ses_child1:call_c"}}], "events": [{"outputs": [], "response": null, "terminal": null}, {"outputs": [], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"_meta": {"opencode/child-session": {"depth": 1, "id": "ses_child1", "parentID": "ses_root"}}, "kind": "read", "locations": [], "rawInput": {}, "sessionUpdate": "tool_call", "status": "pending", "title": "read", "toolCallId": "ses_child1:call_c"}}], "response": null, "terminal": null}]}`
- rust:
  - `{"abandon_outputs": [{"tag": "session_update", "update": {"_meta": {"opencode/child-session": {"depth": 1, "id": "ses_child1", "parentID": "ses_root"}}, "content": [{"content": {"text": "Cancelled", "type": "text"}, "type": "content"}], "kind": "read", "locations": [], "rawInput": {}, "rawOutput": {"error": "Cancelled", "metadata": {}}, "sessionUpdate": "tool_call_update", "status": "failed", "title": "read", "toolCallId": "ses_child1:call_c"}}], "events": [{"outputs": [], "response": null, "terminal": null}, {"outputs": [], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"_meta": {"opencode/child-session": {"depth": 1, "id": "ses_child1", "parentID": "ses_root"}}, "kind": "read", "name": "read", "rawInput": {}, "sessionUpdate": "tool_call", "title": "read", "toolCallId": "ses_child1:call_c"}}], "response": null, "terminal": null}]}`

### `abandon-tools-and-compaction` — events
- ref:
  - `{"abandon_outputs": [{"tag": "session_update", "update": {"content": [{"content": {"text": "Cancelled", "type": "text"}, "type": "content"}], "kind": "execute", "locations": [{"path": "/tmp/opencode/diff-acp"}], "rawInput": {"command": "sleep 100", "cwd": "/tmp/opencode/diff-acp"}, "rawOutput": {"error": "Cancelled", "metadata": {}}, "sessionUpdate": "tool_call_update", "status": "failed", "title": "sleep 100", "toolCallId": "call_1"}}, {"tag": "session_update", "update": {"compactionId": "msg_cx", "sessionUpdate": "compaction_update", "status": "cancelled", "summary": null}}], "events": [{"outputs": [], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"kind": "execute", "locations": [{"path": "/tmp/opencode/diff-acp"}], "rawInput": {"cwd": "/tmp/opencode/diff-acp"}, "sessionUpdate": "tool_call", "status": "pending", "title": "shell", "toolCallId": "call_1"}}], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"kind": "execute", "locations": [{"path": "/tmp/opencode/diff-acp"}], "rawInput": {"command": "sleep 100", "cwd": "/tmp/opencode/diff-acp"}, "sessionUpdate": "tool_call_update", "status": "in_progress"`
- rust:
  - `{"abandon_outputs": [{"tag": "session_update", "update": {"content": [{"content": {"text": "Cancelled", "type": "text"}, "type": "content"}], "kind": "execute", "locations": [{"path": "/tmp/opencode/diff-acp"}], "rawInput": {"command": "sleep 100", "cwd": "/tmp/opencode/diff-acp"}, "rawOutput": {"error": "Cancelled", "metadata": {}}, "sessionUpdate": "tool_call_update", "status": "failed", "title": "sleep 100", "toolCallId": "call_1"}}, {"tag": "session_update", "update": {"compactionId": "msg_cx", "sessionUpdate": "compaction_update", "status": "cancelled", "summary": null}}], "events": [{"outputs": [], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"kind": "execute", "locations": [{"path": "/tmp/opencode/diff-acp"}], "name": "shell", "rawInput": {"cwd": "/tmp/opencode/diff-acp"}, "sessionUpdate": "tool_call", "title": "shell", "toolCallId": "call_1"}}], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"kind": "execute", "locations": [{"path": "/tmp/opencode/diff-acp"}], "rawInput": {"command": "sleep 100", "cwd": "/tmp/opencode/diff-acp"}, "sessionUpdate": "tool_call_update", "status": "in_progress", "t`

### `child-projection-meta` — events
- ref:
  - `{"abandon_outputs": [], "events": [{"outputs": [], "response": null, "terminal": null}, {"outputs": [], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"_meta": {"opencode/child-session": {"depth": 1, "id": "ses_child1", "parentID": "ses_root", "title": "Helper"}}, "content": {"text": "child says hi", "type": "text"}, "messageId": "msg_c1", "sessionUpdate": "agent_message_chunk"}}], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"_meta": {"opencode/child-session": {"depth": 1, "id": "ses_child1", "parentID": "ses_root", "title": "Helper"}}, "kind": "read", "locations": [], "rawInput": {}, "sessionUpdate": "tool_call", "status": "pending", "title": "Helper: read", "toolCallId": "ses_child1:call_c"}}], "response": null, "terminal": null}]}`
- rust:
  - `{"abandon_outputs": [], "events": [{"outputs": [], "response": null, "terminal": null}, {"outputs": [], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"_meta": {"opencode/child-session": {"depth": 1, "id": "ses_child1", "parentID": "ses_root", "title": "Helper"}}, "content": {"text": "child says hi", "type": "text"}, "messageId": "msg_c1", "sessionUpdate": "agent_message_chunk"}}], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"_meta": {"opencode/child-session": {"depth": 1, "id": "ses_child1", "parentID": "ses_root", "title": "Helper"}}, "kind": "read", "name": "read", "rawInput": {}, "sessionUpdate": "tool_call", "title": "Helper: read", "toolCallId": "ses_child1:call_c"}}], "response": null, "terminal": null}]}`

### `permission-ask-tracked-tool` — events
- ref:
  - `{"abandon_outputs": [], "events": [{"outputs": [], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"kind": "execute", "locations": [{"path": "/tmp/opencode/diff-acp"}], "rawInput": {"cwd": "/tmp/opencode/diff-acp"}, "sessionUpdate": "tool_call", "status": "pending", "title": "shell", "toolCallId": "call_1"}}], "response": null, "terminal": null}, {"outputs": [{"child": null, "data": {"id": "perm_1", "sessionID": "ses_root", "source": {"id": "call_1"}, "tool": "bash"}, "tag": "permission_ask", "tool": {"id": "call_1", "input": {}, "metadata": {}, "name": "shell", "sessionID": "ses_root"}}], "response": null, "terminal": null}, {"outputs": [{"id": "perm_1", "tag": "ask_settled"}], "response": null, "terminal": null}]}`
- rust:
  - `{"abandon_outputs": [], "events": [{"outputs": [], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"kind": "execute", "locations": [{"path": "/tmp/opencode/diff-acp"}], "name": "shell", "rawInput": {"cwd": "/tmp/opencode/diff-acp"}, "sessionUpdate": "tool_call", "title": "shell", "toolCallId": "call_1"}}], "response": null, "terminal": null}, {"outputs": [{"child": null, "data": {"id": "perm_1", "sessionID": "ses_root", "source": {"id": "call_1"}, "tool": "bash"}, "tag": "permission_ask", "tool": {"id": "call_1", "input": {}, "metadata": {}, "name": "shell", "sessionID": "ses_root"}}], "response": null, "terminal": null}, {"outputs": [{"id": "perm_1", "tag": "ask_settled"}], "response": null, "terminal": null}]}`

### `reasoning-delta-ordinal-missing` — events
- ref:
  - `{"abandon_outputs": [], "events": [{"outputs": [], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"content": {"text": "d", "type": "text"}, "messageId": "msg_asst_1:reasoning:undefined", "sessionUpdate": "agent_thought_chunk"}}], "response": null, "terminal": null}]}`
- rust:
  - `{"abandon_outputs": [], "events": [{"outputs": [], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"content": {"text": "d", "type": "text"}, "messageId": "msg_asst_1:reasoning:0", "sessionUpdate": "agent_thought_chunk"}}], "response": null, "terminal": null}]}`

### `reasoning-delta-ordinal-negative` — events
- ref:
  - `{"abandon_outputs": [], "events": [{"outputs": [], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"content": {"text": "d", "type": "text"}, "messageId": "msg_asst_1:reasoning:-1", "sessionUpdate": "agent_thought_chunk"}}], "response": null, "terminal": null}]}`
- rust:
  - `{"abandon_outputs": [], "events": [{"outputs": [], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"content": {"text": "d", "type": "text"}, "messageId": "msg_asst_1:reasoning:0", "sessionUpdate": "agent_thought_chunk"}}], "response": null, "terminal": null}]}`

### `reasoning-delta-ordinal-string` — events
- ref:
  - `{"abandon_outputs": [], "events": [{"outputs": [], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"content": {"text": "d", "type": "text"}, "messageId": "msg_asst_1:reasoning:2", "sessionUpdate": "agent_thought_chunk"}}], "response": null, "terminal": null}]}`
- rust:
  - `{"abandon_outputs": [], "events": [{"outputs": [], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"content": {"text": "d", "type": "text"}, "messageId": "msg_asst_1:reasoning:0", "sessionUpdate": "agent_thought_chunk"}}], "response": null, "terminal": null}]}`

### `text-delta-missing-field` — events
- ref:
  - `{"abandon_outputs": [], "events": [{"outputs": [], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"content": {"type": "text"}, "messageId": "msg_asst_1", "sessionUpdate": "agent_message_chunk"}}], "response": null, "terminal": null}]}`
- rust:
  - `{"abandon_outputs": [], "events": [{"outputs": [], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"content": {"text": "", "type": "text"}, "messageId": "msg_asst_1", "sessionUpdate": "agent_message_chunk"}}], "response": null, "terminal": null}]}`

### `tool-called` — events
- ref:
  - `{"abandon_outputs": [], "events": [{"outputs": [], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"kind": "execute", "locations": [{"path": "/tmp/opencode/diff-acp"}], "rawInput": {"cwd": "/tmp/opencode/diff-acp"}, "sessionUpdate": "tool_call", "status": "pending", "title": "shell", "toolCallId": "call_1"}}], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"kind": "execute", "locations": [{"path": "/tmp/opencode/diff-acp"}], "rawInput": {"command": "ls", "cwd": "/tmp/opencode/diff-acp"}, "sessionUpdate": "tool_call_update", "status": "in_progress", "title": "ls", "toolCallId": "call_1"}}], "response": null, "terminal": null}]}`
- rust:
  - `{"abandon_outputs": [], "events": [{"outputs": [], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"kind": "execute", "locations": [{"path": "/tmp/opencode/diff-acp"}], "name": "shell", "rawInput": {"cwd": "/tmp/opencode/diff-acp"}, "sessionUpdate": "tool_call", "title": "shell", "toolCallId": "call_1"}}], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"kind": "execute", "locations": [{"path": "/tmp/opencode/diff-acp"}], "rawInput": {"command": "ls", "cwd": "/tmp/opencode/diff-acp"}, "sessionUpdate": "tool_call_update", "status": "in_progress", "title": "ls", "toolCallId": "call_1"}}], "response": null, "terminal": null}]}`

### `tool-failed` — events
- ref:
  - `{"abandon_outputs": [], "events": [{"outputs": [], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"kind": "execute", "locations": [{"path": "/tmp/opencode/diff-acp"}], "rawInput": {"cwd": "/tmp/opencode/diff-acp"}, "sessionUpdate": "tool_call", "status": "pending", "title": "shell", "toolCallId": "call_1"}}], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"content": [{"content": {"text": "exit 1", "type": "text"}, "type": "content"}], "kind": "execute", "locations": [{"path": "/tmp/opencode/diff-acp"}], "rawInput": {"cwd": "/tmp/opencode/diff-acp"}, "rawOutput": {"error": "exit 1", "metadata": {}}, "sessionUpdate": "tool_call_update", "status": "failed", "title": "shell", "toolCallId": "call_1"}}], "response": null, "terminal": null}]}`
- rust:
  - `{"abandon_outputs": [], "events": [{"outputs": [], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"kind": "execute", "locations": [{"path": "/tmp/opencode/diff-acp"}], "name": "shell", "rawInput": {"cwd": "/tmp/opencode/diff-acp"}, "sessionUpdate": "tool_call", "title": "shell", "toolCallId": "call_1"}}], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"content": [{"content": {"text": "exit 1", "type": "text"}, "type": "content"}], "kind": "execute", "locations": [{"path": "/tmp/opencode/diff-acp"}], "rawInput": {"cwd": "/tmp/opencode/diff-acp"}, "rawOutput": {"error": "exit 1", "metadata": {}}, "sessionUpdate": "tool_call_update", "status": "failed", "title": "shell", "toolCallId": "call_1"}}], "response": null, "terminal": null}]}`

### `tool-failed-long-message` — events
- ref:
  - `{"abandon_outputs": [], "events": [{"outputs": [], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"kind": "execute", "locations": [{"path": "/tmp/opencode/diff-acp"}], "rawInput": {"cwd": "/tmp/opencode/diff-acp"}, "sessionUpdate": "tool_call", "status": "pending", "title": "shell", "toolCallId": "call_1"}}], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"content": [{"content": {"text": "partial", "type": "text"}, "type": "content"}, {"content": {"text": "EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE`
- rust:
  - `{"abandon_outputs": [], "events": [{"outputs": [], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"kind": "execute", "locations": [{"path": "/tmp/opencode/diff-acp"}], "name": "shell", "rawInput": {"cwd": "/tmp/opencode/diff-acp"}, "sessionUpdate": "tool_call", "title": "shell", "toolCallId": "call_1"}}], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"content": [{"content": {"text": "partial", "type": "text"}, "type": "content"}, {"content": {"text": "EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE`

### `tool-input-started` — events
- ref:
  - `{"abandon_outputs": [], "events": [{"outputs": [], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"kind": "read", "locations": [], "rawInput": {}, "sessionUpdate": "tool_call", "status": "pending", "title": "read", "toolCallId": "call_1"}}], "response": null, "terminal": null}]}`
- rust:
  - `{"abandon_outputs": [], "events": [{"outputs": [], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"kind": "read", "name": "read", "rawInput": {}, "sessionUpdate": "tool_call", "title": "read", "toolCallId": "call_1"}}], "response": null, "terminal": null}]}`

### `tool-lifecycle-shell` — events
- ref:
  - `{"abandon_outputs": [], "events": [{"outputs": [], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"kind": "execute", "locations": [{"path": "/tmp/opencode/diff-acp"}], "rawInput": {"cwd": "/tmp/opencode/diff-acp"}, "sessionUpdate": "tool_call", "status": "pending", "title": "shell", "toolCallId": "call_1"}}], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"kind": "execute", "locations": [{"path": "/tmp/opencode/diff-acp"}], "rawInput": {"command": "ls -la", "cwd": "/tmp/opencode/diff-acp"}, "sessionUpdate": "tool_call_update", "status": "in_progress", "title": "ls -la", "toolCallId": "call_1"}}], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"kind": "execute", "locations": [{"path": "/tmp/opencode/diff-acp"}], "rawInput": {"command": "ls -la", "cwd": "/tmp/opencode/diff-acp"}, "sessionUpdate": "tool_call_update", "status": "in_progress", "title": "ls -la", "toolCallId": "call_1"}}], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"content": [{"content": {"text": "total 0", "type": "text"}, "type": "content"}], "locations": [`
- rust:
  - `{"abandon_outputs": [], "events": [{"outputs": [], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"kind": "execute", "locations": [{"path": "/tmp/opencode/diff-acp"}], "name": "shell", "rawInput": {"cwd": "/tmp/opencode/diff-acp"}, "sessionUpdate": "tool_call", "title": "shell", "toolCallId": "call_1"}}], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"kind": "execute", "locations": [{"path": "/tmp/opencode/diff-acp"}], "rawInput": {"command": "ls -la", "cwd": "/tmp/opencode/diff-acp"}, "sessionUpdate": "tool_call_update", "status": "in_progress", "title": "ls -la", "toolCallId": "call_1"}}], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"kind": "execute", "locations": [{"path": "/tmp/opencode/diff-acp"}], "rawInput": {"command": "ls -la", "cwd": "/tmp/opencode/diff-acp"}, "sessionUpdate": "tool_call_update", "status": "in_progress", "title": "ls -la", "toolCallId": "call_1"}}], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"content": [{"content": {"text": "total 0", "type": "text"}, "type": "content"}], "locations": [{"pa`

### `tool-progress` — events
- ref:
  - `{"abandon_outputs": [], "events": [{"outputs": [], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"kind": "read", "locations": [], "rawInput": {}, "sessionUpdate": "tool_call", "status": "pending", "title": "read", "toolCallId": "call_1"}}], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"kind": "read", "locations": [], "rawInput": {}, "sessionUpdate": "tool_call_update", "status": "in_progress", "title": "read", "toolCallId": "call_1"}}], "response": null, "terminal": null}]}`
- rust:
  - `{"abandon_outputs": [], "events": [{"outputs": [], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"kind": "read", "name": "read", "rawInput": {}, "sessionUpdate": "tool_call", "title": "read", "toolCallId": "call_1"}}], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"kind": "read", "locations": [], "rawInput": {}, "sessionUpdate": "tool_call_update", "status": "in_progress", "title": "read", "toolCallId": "call_1"}}], "response": null, "terminal": null}]}`

### `tool-success` — events
- ref:
  - `{"abandon_outputs": [], "events": [{"outputs": [], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"kind": "execute", "locations": [{"path": "/tmp/opencode/diff-acp"}], "rawInput": {"cwd": "/tmp/opencode/diff-acp"}, "sessionUpdate": "tool_call", "status": "pending", "title": "shell", "toolCallId": "call_1"}}], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"content": [{"content": {"text": "a.ts\nb.ts", "type": "text"}, "type": "content"}], "locations": [{"path": "/tmp/opencode/diff-acp"}], "rawOutput": {"metadata": {"t": 1}}, "sessionUpdate": "tool_call_update", "status": "completed", "toolCallId": "call_1"}}], "response": null, "terminal": null}]}`
- rust:
  - `{"abandon_outputs": [], "events": [{"outputs": [], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"kind": "execute", "locations": [{"path": "/tmp/opencode/diff-acp"}], "name": "shell", "rawInput": {"cwd": "/tmp/opencode/diff-acp"}, "sessionUpdate": "tool_call", "title": "shell", "toolCallId": "call_1"}}], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"content": [{"content": {"text": "a.ts\nb.ts", "type": "text"}, "type": "content"}], "locations": [{"path": "/tmp/opencode/diff-acp"}], "rawOutput": {"metadata": {"t": 1}}, "sessionUpdate": "tool_call_update", "status": "completed", "toolCallId": "call_1"}}], "response": null, "terminal": null}]}`

### `tool-success-edit-diff-content` — events
- ref:
  - `{"abandon_outputs": [], "events": [{"outputs": [], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"kind": "edit", "locations": [], "rawInput": {}, "sessionUpdate": "tool_call", "status": "pending", "title": "edit", "toolCallId": "call_1"}}], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"kind": "edit", "locations": [{"path": "/tmp/opencode/diff-acp/src/a.ts"}], "rawInput": {"newString": "b", "oldString": "a", "path": "src/a.ts"}, "sessionUpdate": "tool_call_update", "status": "in_progress", "title": "edit", "toolCallId": "call_1"}}], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"content": [{"content": {"text": "applied", "type": "text"}, "type": "content"}, {"newText": "b", "oldText": "a", "path": "/tmp/opencode/diff-acp/src/a.ts", "type": "diff"}], "locations": [{"path": "/tmp/opencode/diff-acp/src/a.ts"}], "rawOutput": {}, "sessionUpdate": "tool_call_update", "status": "completed", "toolCallId": "call_1"}}], "response": null, "terminal": null}]}`
- rust:
  - `{"abandon_outputs": [], "events": [{"outputs": [], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"kind": "edit", "name": "edit", "rawInput": {}, "sessionUpdate": "tool_call", "title": "edit", "toolCallId": "call_1"}}], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"kind": "edit", "locations": [{"path": "/tmp/opencode/diff-acp/src/a.ts"}], "rawInput": {"newString": "b", "oldString": "a", "path": "src/a.ts"}, "sessionUpdate": "tool_call_update", "status": "in_progress", "title": "edit", "toolCallId": "call_1"}}], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"content": [{"content": {"text": "applied", "type": "text"}, "type": "content"}, {"newText": "b", "oldText": "a", "path": "/tmp/opencode/diff-acp/src/a.ts", "type": "diff"}], "locations": [{"path": "/tmp/opencode/diff-acp/src/a.ts"}], "rawOutput": {}, "sessionUpdate": "tool_call_update", "status": "completed", "toolCallId": "call_1"}}], "response": null, "terminal": null}]}`

### `tool-success-multipart-all-parts` — events
- ref:
  - `{"abandon_outputs": [], "events": [{"outputs": [], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"kind": "other", "locations": [], "rawInput": {}, "sessionUpdate": "tool_call", "status": "pending", "title": "screenshot", "toolCallId": "call_1"}}], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"content": [{"content": {"text": "captured", "type": "text"}, "type": "content"}, {"content": {"data": "aGVsbG8=", "mimeType": "image/png", "type": "image"}, "type": "content"}], "locations": [], "rawOutput": {}, "sessionUpdate": "tool_call_update", "status": "completed", "toolCallId": "call_1"}}], "response": null, "terminal": null}]}`
- rust:
  - `{"abandon_outputs": [], "events": [{"outputs": [], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"name": "screenshot", "rawInput": {}, "sessionUpdate": "tool_call", "title": "screenshot", "toolCallId": "call_1"}}], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"content": [{"content": {"text": "captured", "type": "text"}, "type": "content"}, {"content": {"data": "aGVsbG8=", "mimeType": "image/png", "type": "image"}, "type": "content"}], "locations": [], "rawOutput": {}, "sessionUpdate": "tool_call_update", "status": "completed", "toolCallId": "call_1"}}], "response": null, "terminal": null}]}`

### `tool-success-read-preview-g339` — events
- ref:
  - `{"abandon_outputs": [], "events": [{"outputs": [], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"kind": "read", "locations": [], "rawInput": {}, "sessionUpdate": "tool_call", "status": "pending", "title": "read", "toolCallId": "call_1"}}], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"kind": "read", "locations": [{"path": "/tmp/opencode/diff-acp/src/a.ts"}], "rawInput": {"path": "src/a.ts"}, "sessionUpdate": "tool_call_update", "status": "in_progress", "title": "read", "toolCallId": "call_1"}}], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"content": [{"content": {"text": "1 let a = 1\n2 let b = 2", "type": "text"}, "type": "content"}], "locations": [{"path": "/tmp/opencode/diff-acp/src/a.ts"}], "rawOutput": {}, "sessionUpdate": "tool_call_update", "status": "completed", "toolCallId": "call_1"}}], "response": null, "terminal": null}]}`
- rust:
  - `{"abandon_outputs": [], "events": [{"outputs": [], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"kind": "read", "name": "read", "rawInput": {}, "sessionUpdate": "tool_call", "title": "read", "toolCallId": "call_1"}}], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"kind": "read", "locations": [{"path": "/tmp/opencode/diff-acp/src/a.ts"}], "rawInput": {"path": "src/a.ts"}, "sessionUpdate": "tool_call_update", "status": "in_progress", "title": "read", "toolCallId": "call_1"}}], "response": null, "terminal": null}, {"outputs": [{"tag": "session_update", "update": {"content": [{"content": {"text": "1 let a = 1\n2 let b = 2", "type": "text"}, "type": "content"}], "locations": [{"path": "/tmp/opencode/diff-acp/src/a.ts"}], "rawOutput": {}, "sessionUpdate": "tool_call_update", "status": "completed", "toolCallId": "call_1"}}], "response": null, "terminal": null}]}`

## 判定

- 已裁定：97/97（每例双侧一致或分歧字段明确归属）
- 分歧 18 项逐项人工定性后入册（缺口候选/有据分歧/提取噪声）或背书

## 分歧定性（人工亲读双侧当前代码）

### A. 缺口候选（rust 侧 wire 行为与 ref 不一致，待入册）

> 2026-11-12（gap-burndown-14 lane1）：A1/A2/A3 已修复入册 G-392/G-401/G-402 并关闭，以下为历史裁定记录；对应用例仅剩 B1 有据分歧（`tool-called`/`child-projection-meta`/`abandon-child-tool` 等）或已全绿（`compaction-id-from-envelope`）。

| # | 语义 | ref 侧 | rust 侧（修复后） | 涉及用例 |
|---|---|---|---|---|
| ~~A1~~ | child toolCallId 双重前缀（G-392 已修） | `translate.ts:309-320` 发出裸 id，`child.ts:39-47` `project` 前缀一次 | `pending_update` 不再前缀，`route`→`child::project` 单次前缀（`ses_child1:call_c`） | `child-projection-meta`、`abandon-child-tool`（剩 B1） |
| ~~A2~~ | shell `rawInput` 的 `cwd:null` 不回填（G-401 已修） | `tool.ts:165-168` 真值判断（`input.cwd \|\| input.workdir`），null 视为缺省并回填 ctx.cwd | `raw_input` 改真值判断（`is_truthy`），`cwd:null`/空串回填 ctx.cwd | `tool-called`（剩 B1） |
| ~~A3~~ | compaction 起始消息 id 回退源（G-402 已修） | `compaction.ts:102-108` `inputID ?? SessionMessage.ID.fromEvent(envelope event.id)`（`evt_7`→`msg_7`） | `Event` 携带 envelope id，translate 侧 `message_id_from_event` 映射 `evt_`→`msg_` 作回退 | `compaction-id-from-envelope`（已 MATCH） |

### B. 有据分歧（SDK 序列化形状 / 超 schema 输入容错，双侧代码如实呈现）

| # | 语义 | ref 侧 | rust 侧 | 涉及用例 |
|---|---|---|---|---|
| B1 | pending `tool_call` 形状 | `tool.ts:61-75` 对象字面量：带 `status:"pending"`、空 `locations:[]`、无 `name` | `tool.rs:215-234` 经 vendored crate 序列化：`name` 恒置（tool.rs:228）、`status` 默认值被 `tool_call.rs:51` skip、空 `locations` 被 `:56` skip | 全部含 `session.tool.input.started` 的 18 例中的 14 例（`tool-*`、`abandon-*`、`permission-ask-tracked-tool`、`child-projection-meta`） |
| B2 | reasoning ordinal 异常值 | `translate.ts:277-283` 模板串接（缺省→`undefined`、负数/字符串原样） | `translate.rs:638-641` `as_u64().unwrap_or(0)` | `reasoning-delta-ordinal-{missing,negative,string}`（schema 要求 ordinal 必填非负，属超 schema 输入容差分歧） |
| B3 | text.delta 缺 `delta` 字段 | `translate.ts:283-291` `{text: undefined}` 经 JSON 序列化丢键 | `translate.rs:618-630`+`:835-837` 回退 `""` 显式上 wire | `text-delta-missing-field`（同上，超 schema 输入） |

### C. 提取噪声（已在归一化/驱动侧消解，不计分歧）

- `PromptResponse._meta` 空 `{}` vs 省略：ref `translate.ts:143-160` 恒带 `_meta`，rust `translate.rs:322-369` None 省略 → 归一化剥除空 `_meta`。
- `permission_ask`/`form_ask` 的 `tool`/`child` 键缺席 vs `null`：驱动侧统一为 `null`。
- 键序：递归排序。

## 覆盖说明

- corpus 覆盖全部 51 种 session 事件类型（`oc-schema/src/session_event.rs` 对 `ref schema/src/session-event.ts`），
  外加边界：空/超长(50k)/unicode-emoji delta、null 字段、非字符串 sessionID、异常 ordinal、
  multipart tool content（text/file/data-image/data-json/file-uri）、read 预览（G-339）、edit diff、
  retry ISO（G-340）、cancel/abandon 族（G-354/G-355）、boolean 字段、双侧 start 门控与 ask/settle 族。
- 本 fold 观测到 7 种 SessionUpdate kind（见顶部清单）；census 11 种中的其余 4 种
  （user_message_chunk / plan / available_commands_update / current_mode_update 等）由 fold 之外的路径
  （prompt echo、replay、config-option、available-commands）产生，不在本通道管线内。
