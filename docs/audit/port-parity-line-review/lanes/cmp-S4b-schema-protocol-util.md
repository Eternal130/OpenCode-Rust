# S4b 结论: schema/protocol/theme/util 四域移植质量很高——session-message parts 与 openapi 生成物与 ref 逐操作字节级一致，patch 算法逐行等价；发现 1 个 rust 独有端点、1 个未移植的 simulation 协议域及若干有界文档化偏差，无 wire 兼容性硬伤。

## session-message parts 全集对照表

ref `packages/schema/src/session-message.ts` (320 行) ↔ rust `crates/oc-schema/src/session_message.rs` (554 行，含 wire 测试)。

| ref 类型/tag | rust 对应 | 字段对齐 | 备注 |
|---|---|---|---|
| `Info` union, tag=`type` kebab | `enum Info` `#[serde(tag="type", kebab-case)]` (:239) | ✅ 11 变体全集 | AgentSwitched/ModelSwitched/LocationSwitched/User/Synthetic/System/Skill/Shell/Assistant/Compaction/Idle 一一对应 |
| Base `{id, metadata?, time.created}` | 每变体 id/metadata:Option/time | ✅ | metadata omit-none ≈ ref optional |
| AgentSelected("agent-switched") | `Info::AgentSwitched` :242 | ✅ agent/previous? | |
| ModelSelected("model-switched") | `Info::ModelSwitched` :252 | ✅ model:Ref/previous? | |
| LocationSwitched | :262 | ✅ location/projectID?/subpath?/previous | `projectID` rename ✓ |
| User | :276 | ✅ text/files?/agents?/skills? | attachments=prompt.rs 三结构 ✓ |
| Synthetic | :290 | ✅ text/description? | |
| System | :300 | ✅ text/description? | |
| Skill | :312 | ✅ skill:ID/name/text | |
| Shell | :322 | ✅ shellID/command/status/exit?/output?/time | |
| Assistant | :337 | ✅ agent/model/content[]/snapshot/finish?/rawFinish?/providerState?/cost?/tokens?/error?/retry?/time | |
| AssistantContent tag=`type` | `enum AssistantContent` :105 | ✅ text/reasoning/tool 三态 | tool: id/name/executed?/providerState?/providerResultState?/state/time ✓ |
| ToolState tag=`status` | `enum ToolState` :56 | ✅ streaming/running/completed/error | ⚠️ R-2 |
| Compaction（双重 tag） | `Info::Compaction(Compaction)` + `enum Compaction` :158 | ✅ running/completed/failed；reason auto\|manual；completed 带 model?/providerState?/cost?/tokens? | 双 tag 序列化经测试钉死 (:514-533) |
| Idle outcome | `Info::Idle` + IdleOutcome lowercase | ✅ succeeded/failed/interrupted | |
| AssistantRetry | :138 | ✅ attempt:PositiveInt/at/error | |

结论：**parts 类型全集无缺漏、无多余变体**，tag 大小写/命名与 ref Literals 完全一致，每个 union 都有 round-trip wire 测试。

## 对比表

| 域 | ref | rust | 结论 |
|---|---|---|---|
| prompt/prompt-input | prompt.ts, prompt-input.ts | oc-schema/prompt.rs | ✅ Mention/FileSource(inline\|uri)/FileAttachment/InputFileAttachment/Agent/Skill 全对齐 |
| form | form.ts | form.rs | ✅ 6 字段类型 + When(eq/neq) + State 三态 + Answer(untagged)；Fields NonEmptyArray→Vec（宽松） |
| question | question.ts | misc.rs:333-355 | ✅ |
| skill/command | skill.ts/command.ts | skill.rs/misc.rs:229 | ✅ |
| credential/integration/connection | 三 ts | integration.rs（三合一） | ✅ OAuth/Key、Method 全集、Attempt/AttemptStatus、Connection 全集；needs_auth snake ✓ |
| pty/persistent-pty/ticket | pty.ts 等 | pty.rs + persistent_pty.rs + wire.rs + tickets.rs | ✅ 分布式落位但齐全 |
| shell | shell.ts | shell.rs | ✅ Status 4 值/Info/Output |
| snapshot/vcs(+event) | 两 ts | misc.rs:11-62 | ✅ Branch/Base/Mode/FileStatus |
| websearch | websearch.ts | misc.rs:237-274 | ✅（time.published Finite→NonNegativeInt，D-6） |
| worktree(+event) | worktree.ts | misc.rs:64-225 | ✅ **`adopt()` 纯投影 1:1 移植**（windows 盘符大小写、`..` 折叠、global 采纳），测试 6 例 |
| workspace/project/reference | 各 ts | misc.rs + location.rs | ✅ |
| instruction(+entry) | 两 ts | session_support.rs:113-188 | ✅ Key 正则/Hash/Values/Delta/Entry/MaxValueBytes 256KiB→413 |
| location(+event) | 两 ts | location.rs | ✅ |
| models-dev/model/llm/provider/money/token-usage/file-diff | 各 ts | model.rs/common.rs/provider.rs | ✅ FinishReason 6 值 kebab、TokenUsage+total、USD 族、FileDiff+LegacyInfo |
| tool Content/Error | tool.ts | tool.rs | ✅ text/file 两态、Error 三字段 |
| identifier | identifier.ts | identifier.rs | ✅ 26 字符、48bit ts*0x1000+counter、62 表、descending 取反；随机段 xorshift64*（D-5） |
| rpc（类型层） | rpc.ts | oc-client/rpc_typed.rs + errors.rs | ✅ 系统错误集五类对应齐全 |
| v1/ | v1/*.ts | oc-core/database/v1_migration.rs + filesystem.rs | ✅ 落位于 core 迁移层 |
| **util/patch** | patch.ts (381 行) | oc-core/tool/patch_util.rs (700 行) | ✅ **逐行等价**：heredoc 剥离、Environment ID、Move to、End of File 状态机、@@ context、seek 四级比较阶梯（弯引号/短横/NBSP 字符类逐字符一致）、末尾空行 pop 重试、BOM 保真、错误文案逐句对齐 |
| util/encode | encode.ts | oc-util/encode.rs | ✅ URL-safe 无 padding base64；FNV-1a over UTF-16 code units（与 JS charCodeAt 一致）+ 采样 checksum |
| util/hash | hash.ts | hash.rs | ✅ sha1/sha256 hex |
| util/glob | glob.ts (minimatch dot:true) | oc-util/glob.rs | ✅ 子集，**G-92：不支持 brace/extglob**（文档化） |
| util/path | path.ts | pathx.rs | ✅ |
| util/bom | bom.ts | file_mutation::bom_split/join | ✅ |
| util/flock | flock.ts（锁文件+心跳+stale 60s 接管） | flock.rs（真 flock(2)+超时） | ⚠️ D-2 机制简化 |
| util/npm-config | npm-config.ts（@npmcli/config 全级联） | npm_config.rs（6 级手工级联） | ✅ PREFIX 仅认 env（文档化） |
| util/global-roots | global-roots.ts | global_roots.rs | ✅；APP_NAME="opencode-rust"（已知隔离） |
| util/retry | retry.ts | retry.rs | ✅ 8 条 transient 消息表逐条在列 |
| util/observability | observability/* | oc-server/observability.rs（otel feature-gated）+ Logging trim/压缩/锁 | ✅ |
| theme expand/resolve/v1/syntax/discovery | 各 ts | oc-theme 各 rs | ✅ 函数级对齐；syntax 50+ 规则 spot-check 全同；D-3 null 微差、D-9 坏文件跳过 |
| protocol errors | errors.ts（30 类） | oc-server/errors.rs | ✅ 全部 30 类 `_tag` envelope + 字段名 + 状态码逐一实现并有 wire 测试 |
| middleware authorization/schema-error | 两 ts | auth.rs + errors.rs | ✅ |
| protocol api/client | api.ts 33 group、client.ts | openapi-gen 生成 + sha256 钉死 + 字节级 drift 测试 | ✅ |
| openapi 对照 | ref openapi.json | tools/openapi-gen/openapi.json | ✅ **141/141 共有 operation 逐对象 sort-keys 字节级相同**；path 117 vs 118：rust 多 `/api/plugin/install` |
| protocol simulation | simulation.ts（691 行 WS JSON-RPC 输入注入） | 未移植；OPENCODE_SIMULATE 显式拒绝 | ❌ D-4 |

## 行为差异清单 D-n

- **D-1 rust 独有端点 `/api/plugin/install`**（routes.gen.rs:1156）。加性扩展；ref openapi 消费者会看到额外路径。
- **D-2 flock 机制替换**：ref 锁文件+心跳+60s stale 接管+指数退避；rust 直接 flock(2)+超时（自述 "semantics reduced"）。单机等价，跨发行 stale 锁语义不同。
- **D-3 theme mergeTheme null 语义**：ref 只跳过 undefined（null 保留）；rust 连 null 一起跳过。仅显式 null 值主题不同。
- **D-4 simulation 协议未移植**：WS JSON-RPC 握手/capabilities/ui.type/ui.press 输入注入无 rust 对应；rust 用 golden/录制回放体系替代——工具链内部等效，外部测试客户端不可用。
- **D-5 identifier 随机段**：crypto.getRandomValues → xorshift64*。同毫秒碰撞概率略高；另 ts/counter 更新非原子窗口多线程理论上可产生重复 counter（ref 单线程无此问题）。
- **D-6** WebSearch.Result.time.published：Finite→NonNegativeInt。极端历史/小数时间戳被 rust 拒收。
- **D-7 glob 有界子集（G-92）**；**D-8** npm PREFIX 只认 env；**D-9** theme discovery 坏文件跳过；**D-10** APP_NAME 隔离（已知）。

## 正确性风险 R-n

- **R-1（低）** oc-schema 各 Record/metadata 用 BTreeMap——键序按字典序重排。JSON 语义等价，逐字节快照/签名比对消费者看到不同键序。
- **R-2（低）** NonEmptyArray 约束在 rust 是普通 Vec——空数组可被反序列化通过；仅校验宽松，无 wire 破坏。
- **R-3（低）** identifier 的 LAST_TIMESTAMP/COUNTER check-then-act 竞窗（非 CAS）：高并发同毫秒可能 counter 回绕/重复；建议 compare_exchange。
- **R-4（信息）** `/api/plugin/install` 为 rust 扩展，未来 ref 上游同名端点会冲突。
- 未发现 tag 拼写、可选性、数字类型、时间单位层面的 wire 不兼容。

## 覆盖声明

- **全读**：ref schema/src 本 lane 范围文件（session-message.ts 逐行；其余全部目标文件）；protocol errors.ts 全量 30 类、middleware 两个、client.ts、simulation.ts 头 260 行；util patch.ts 全量逐行、encode/bom/hash/retry/glob/path/npm-config/flock/global-roots；theme expand/resolve(重点段)/syntax(前 20 规则+清单)/discovery/v1-migrate。
- **对照读**：rust 对应全部目标文件（session_message.rs 全读；errors.rs 全读；patch_util/encode/hash/glob/pathx/npm_config/flock/identifier/expand/resolve(230-440)）。
- **机制核验**：openapi-gen 生成机制（pinned spec + sha256 + 字节级 drift 测试）+ 两份 openapi.json 141 operation 全量逐对象比对（0 diff）+ path 集差异 + `/api/plugin/install` 追溯确认。
- **抽样/清单级**：theme resolve.rs 440 行后段、v1_migrate 函数级对照；ref util 的 artifact/activity-calendar/open/npm/process/cross-spawn/effect/runtime 仅确认 rust 侧落位，未逐行 diff（低风险）。
- **依赖他人 lane**：config 键面(C4)、事件 schema(C5a)、agent(C3)、mcp(C6a)、theme 资产(T4)、ai schema(A2) 按分工未重审。
