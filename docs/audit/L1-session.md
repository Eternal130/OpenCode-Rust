# L1 会话生命周期 (Session) — 审查报告

> 审查方式：两侧完整逐行阅读（结构性核读+关键段逐行的部分在覆盖证明中标注）。证据格式 file:line。
> 参照：/opt/opencode-ref (v2.0.24 TS) · 移植：/opt/opencode-rust

### 1. 覆盖证明

**Ref 侧（完整逐行阅读）**：session.ts(495), session/session.ts(426), store.ts(256), projector.ts(790), message-updater.ts(487), prompt.ts(231), inbox.ts(550), execution.ts(195), run-coordinator.ts(196), runner/{index,max-steps,retry,llm,step}.ts, generate.ts(73), compaction.ts(963), context.ts(208), usage.ts(56), to-session-error.ts(105), history/provider-context/system-prompt/runner-model/title.ts, instructions 摘读, revert.ts(114), diff.ts(138), subagent-completion/job, move.ts(179), execution/restart.ts(240), shell/skill/command/environment/info/affinity/event/message/schema/sql/error/stats。

**Rust 侧（完整逐行阅读）**：mod.rs(115), store.rs(881), projector.rs(1247), message_updater.rs(968), prompt.rs(405), inbox.rs(853), execution.rs(1114), runner/{drain(1159), step(759), retry(191), max_steps, generate(234), compaction(1609), context(1703), model_request(1972), mod(812), system_prompt, identity, prompt_prepare(332)}, revert.rs(455), fork.rs(153), subagent_completion.rs(202), subagent_job.rs(156)。**结构性核读+关键段逐行**：to_llm_message.rs(762), publisher.rs(1251), title.rs(467), transfer.rs(643), diff.rs(418)。
**关键 grep**：session.move→handlers/session.rs:901-963；restart sweep→jobs.rs:1003 + oc-server/boot.rs:677-694；stats→handlers/session.rs:2369-3085；SessionEnv→oc-core/src/env.rs；affinity→model_request.rs:94；WS 通道→runner/mod.rs:198-320。

### 2. 逐项对比表

| 功能点 | Ref 实现 (file:line) | Rust 实现 (file:line) | 状态 | 差异说明 |
|---|---|---|---|---|
| Session.create 幂等 | session.ts:262-318 | store.rs:574-736 | ✅ | 并发投影竞败重读 Existing；G-250 `explicit_defaults` 为 transfer 新增🔵 |
| slug 生成 | store.ts:262 | store.rs:118-137 | ⚠️ | 本地 30×30 词表 vs 上游更大词表（RC-09 有 pin 测试，契约等价） |
| list/anchor 分页 | store.ts:99-139 | store.rs:160-244 | ✅ | 双列锚点/方向翻转/like 转义一致 |
| messages 游标分页 | store.ts:140-178 | store.rs:248-380 | ✅ | seq 边界、cursor 缺失返 [] 一致 |
| claim/release/countResume | store.ts:205-251 | store.rs:426-542 | ✅ | time_updated 固定、null-guard 再claim幂等 |
| remove 树删除 | session.ts:367-377 | mod.rs:71-97 | 🟡 | 🔵新增 10s REMOVE_IDLE_TIMEOUT（ref 无限等待） |
| fork | session.ts:319-359 + projector.ts:106-226 | fork.rs:74-153 + projector.rs:678-813 | ✅ | 子行 parent_id=NULL、fork_title、500 批复制、过滤未完成 assistant/shell/compaction、reserveSequence 全对齐 |
| projector 全事件表 | projector.ts:434-788 | projector.rs:90-419 | ✅ | 45 个 project 处理器逐一对应；time_idle 单调+1ms 推进、viewed 水位线、revert 级联删除+instruction reset 一致 |
| usage fan-out | projector.ts:776-787 | projector.rs:370-496 | ✅ | 🔵新增 per-session 串行链+`wait_usage_fanout`（恢复 v2 因果序） |
| worktree.resolved 重归属 | projector.ts:486-542 | projector.rs:818-900 | ✅ | adopted/global 候选、\uffff 区间 pin |
| message-updater 状态机 | message-updater.ts:31-484 | message_updater.rs:212-817 | ✅ | findLast 部件、step.started 复位/新行、tool success/error 状态门槛、compaction running→completed/failed、shutdown 只清 retry 全对齐 |
| step.failed finish | message-updater.ts:276 | message_updater.rs:490-494 | ⚠️ | Rust 把存在的 finish 硬编码 ContentFilter（schema 限定下等价；若 wire 出现其它值会错映射） |
| snapshot files 键语义 | message-updater.ts:22-29 | message_updater.rs:847-864 | ✅ | G-112：`files:[]` 写空数组/缺键清空，有 pin 测试 |
| inbox admit/reconcile | inbox.ts:155-228 | inbox.rs:302-396 | ✅ | 首次 admit 胜、promotedFromMessage、admitCompaction 复用 pending |
| inbox promote/控制重排 | inbox.ts:496-550 | inbox.rs:481-602 | ✅ | steer 优先、move 边界不跨 compaction、compaction 前插、queue 后补读 steer |
| inbox projectors | inbox.ts:277-384 | inbox.rs:767-853 | ✅ | pending-only 删除语义、guarded delivery flip |
| prompt 附件物化 | prompt.ts:93-231 | prompt.rs:82-405 | ✅ | data:URL 规范 base64、目录列表、20MiB、mime sniff、行切片 |
| prompt hook/skill 解析 | prompt.ts:29-91 | prompt_prepare.rs:204-291 | ⚠️ | 提交路径等价；🔵新增 drain 侧 backstop `prepare_pending`（原位 UPDATE inbox payload，v2 无此路径）+PreparedRing 防双触发 |
| runner drain 外环 | runner/llm.ts:54-196 | drain.rs:249-307 | ✅ | 入口门、stale 清算、advanceToStep 边界机、Moved/Reloaded 递推 |
| advanceToStep | llm.ts:71-186 | drain.rs:311-563 | ✅ | compaction/move 即时投递、cursor 重置规则、title fork、G-279⑥ ready 段可中断 |
| runStep 策略 | llm.ts:206-297 | drain.rs:673-875 | ✅ | 每 step 新建 RetryPolicy、auto-compact、max-steps、Continue/Compacted/RecoverFull |
| 手动/中断 compaction | llm.ts:113-157, compaction.ts:664-672 | drain.rs:436-488,647-668 | ✅ | G-252① 单次失败发布；RD-11 shutdown 竞态直接结算 |
| settleStale* | llm.ts:299-352 | drain.rs:934-1048 | ✅ | newest-first、subagent childID 后缀一致 |
| step attempt 终局顺序 | runner/step.ts:72-279 | step.rs:198-633 | ✅ | 13 步终局次序逐一对齐 |
| unknownFinish | step.ts:163-171 | step.rs:482-489 | ⚠️ | G-105：clean 流无 finish 事件也判失败（ref 依赖协议层保证；Rust 提前防御，行为差异但更安全） |
| 工具并发/中断 | step.ts:83-145 | step.rs:181-459 | ✅ | 🔵新增 TOOL_JOIN_TIMEOUT=5s 与 UNWIND_TIMEOUT=60s 有界等待（ref 无界）；bell latch 语义等价 |
| retry 策略 | runner/retry.ts:32-92 | retry.rs:55-159 | ✅ | 2s×2 指数/10s cap/10 次/±20% jitter/15min retry-after cap/timeout≤3/hook 覆写 |
| max-steps prompt | max-steps.ts:1-16 | max_steps.rs:4 | ✅ | 逐字 |
| compaction 全量 | compaction.ts:41-963 | compaction.rs:38-1609 | ✅ | due/ceiling/keep=15k/SHRINK 0.7/0.5/0.35/payload-too-large as-text 不计数/deliver 重试/flatten+exchange 丢弃/即时计费/spent 清理/native endpoint+trigger/模板与 NUDGE 逐字；G-256 UTF-16 估算修正 |
| context select/load | context.ts:74-183 | context.rs:1484-1681 | ✅ | 两阶段、location mismatch→Interrupt、权限合并、指令顺序 codemode→mcp→reference→skill→discovery→date→env→entries |
| instructions 机制 | instructions/*, instruction-state.ts | context.rs:32-475 | ✅ | observe/commit/apply/initialize/advanceEpoch/reset/initial/preview、blob 500 批、canonical JSON hash |
| history 边界 | history.ts:35-133 | context.rs:1084-1241 | ✅ | latest/local/provenance 三边界、native checkpoint 前置 system 过滤 |
| provider-context | provider-context.ts:17-84 | context.rs:1248-1293 | ✅ | 静态端点 sha256、compatible 深比较 |
| model-request | model-request.ts:43-414 | model_request.rs:26-1082 | ✅ | 四种 RequestKind 各自 hook、identity@system[1]/optimize/plan 注入、unsupportedParts/boundImages(25/15MiB)、affinity 头+promptCacheKey、model.request baseURL 守卫、http/ws hooks、G-262① openai 不发 maxTokens、effective tool 名映射 |
| to-llm-message | to-llm-message.ts:17-331 | to_llm_message.rs | ✅ | 附件渲染/内联图去重/工具未执行 result 补发/hosted providerResultState 跨模型回放/effort 切换/checkpoint 展开 |
| publisher | publish-llm-event.ts:78-600 | publisher.rs | ✅ | 100ms delta 批+deadline 唤醒、ordinal、failUnsettledTools、failAssistant/publishStepFailure、hostedContent |
| model-transport(WS) | model-transport.ts(571) | runner/mod.rs:208-320 + oc-ai websocket | 🟡 | bind/complete 续传/sticky 回退在 oc-ai 层；`chunkTimeout` 参数未见对应透传（低风险） |
| execution 服务+协调器 | execution.ts:50-175, run-coordinator.ts:60-196 | execution.rs:188-509,576-700 | ✅ | doorbell/scope 合并/中断结算窗口/settled hook/successor、claim commit-hook 同事务、shutdown 保留 claim |
| 失败终端补记 | execution.ts:138-146（仅发布 failed） | execution.rs:850-1038 | 🔵/⚠️ | G-226：未 surfaced 失败额外合成 step.started+step.failed 错误行（ref 无此行；transcript 多一行，已注册偏差+dedup） |
| 一次性 generate | generate.ts:26-73 | generate.rs:162-214 | 🟡 | ref 用 `llm.generate` 一次性响应，Rust 用 stream 聚合 text；usage 不返回一致 |
| title 生成 | title.ts:32-165 | title.rs:16-386 | ✅ | 100/8000/2000 截断、untitled 判定、small-model 回退链、expectedSequence commit 竞争守卫 |
| stats | stats.ts:85-422 | handlers/session.rs:2369-3085 | ✅ | 聚合 SQL+本地时区分桶；位于 oc-server（分层差异） |
| revert stage/clear/commit | revert.ts:17-105 | revert.rs:111-235 + projector.rs:341-368 | ✅ | original 复用/plan first-wins/restore 合并/files diff/遗留 V1 行解码 |
| diff turn | diff.ts:43-138 | diff.rs:30-274+ | ✅ | user/idle 锚点、legacy 无标记会话、running 比较 working copy、跨 location 拒绝 |
| transfer 导入导出 | transfer.ts:55-175 | transfer.rs:70-352 | ✅ | 冲突 409/parentID 校验/settled 过滤/seq 1..n+reserveSequence/cost+tokens 回填/原子 commit-hook/sanitize redact |
| subagent 完成/任务 | subagent-completion.ts:21-46, subagent-job.ts:23-58 | subagent_completion.rs:26-139, subagent_job.rs:58-155 | ✅ | envelope 逐字、resume:false/staged-revert 抑制 wake(RUNTIME-S01)、`child:startedAt` 观察去重 |
| restart 恢复 | execution/restart.ts:44-214 | jobs.rs:635-1003, boot.rs:677-694, store.rs:398-523 | ✅ | maxAttempts=10、RESUME_EXHAUSTED 终端化、shell/subagent 通知排序、releaseChildClaims；🔵`_before(boot_ms)` 进程竞态守卫 |
| move | move.ts:60-170 | handlers/session.rs:901-963 | ✅ | ~ 展开/三类 Destination 错误/sourceUnavailable 即时迁移/否则 admit+wake |
| shell/skill/command | session.ts:179-245 等 | handlers/session.rs 各段 | ✅ | started/ended+通知、evt_ id 派生、激活事件、resume join、命令分发(Plugin/Mcp) |
| environment | environment.ts:17-33 | oc-core/src/env.rs:22-40 | ✅ | 进程内 map get/set/clear |
| affinity | affinity.ts:8 | model_request.rs:94-100 | ✅ | parent→fork→self |
| to-session-error | to-session-error.ts:10-105 | runner/mod.rs:473-627 | ✅ | 全 reason 映射、token-sharing 文案表、tool 嵌套解包、contentFilterError |
| 事件 schema/sql/error | event.ts, sql.ts, error.ts | oc-schema session_event + database/schema | ✅ | 错误以类型化隧道替代 Effect die |
| info fromRow | info.ts:8-58 | database/schema SessionRow + handlers 映射 | ✅ | |

### 3. 关键差异与风险 Top 5

1. **G-226 合成失败回合错误行**（execution.rs:930-1038）：ref 失败终端只发 `execution.failed`；Rust 额外插入 step.started/step.failed 生成 assistant 错误行。**与 ref transcript 不可逐字节互操作**（导出/导入、客户端渲染多一行）；虽按 turn-key dedup，仍是最大的可观测行为差异。
2. **prompt prepare 的双路径**（prompt_prepare.rs:299-331）：drain 侧 backstop 直接 `UPDATE session_inbox SET payload` 原位改写已入队行——ref 中 prepare 只发生在 admit 前且失败行永不入队。PreparedRing(4096) 有界，重放窗口外的重试理论上可二次触发 hook。
3. **有界等待超时**（step.rs:44 TOOL_JOIN_TIMEOUT=5s、execution.rs:56 UNWIND_TIMEOUT=60s、mod.rs:59 REMOVE_IDLE_TIMEOUT=10s）：ref 均无界。卡死路径下 Rust 会把 wedged 工具/排水分类为 interrupted 并继续/删除会话——比 ref 更激进，可能在慢工具上误判中断。
4. **G-105 unknownFinish 提前防御**（step.rs:482-489）：clean 结束但无 finish 事件 → 判 incomplete-stream 失败；ref 依赖 provider 协议保证。对行为不规范的 provider，Rust 会把 ref 视为成功的流判为失败（触发 retry）。
5. **step.failed 的 finish 映射硬编码**（message_updater.rs:490-494）：`finish.map(|_| ContentFilter)` 假设失败事件 finish 只可能是 content_filter；若上游 schema 放开，Rust 会投影错误 finish 值。

### 4. 缺失文件/功能汇总

- 无整文件级缺失：ref session/ 下 43 文件全部在 Rust 找到对应实现。
- 部分偏离承载点：`model-transport.ts` 的 chunkTimeout 参数未见透传；`generate.ts` 用 stream 替代一次性 generate；runner/prompt/system.txt 以常量内联（逐字一致）；stats 位于 oc-server 而非 oc-core（分层差异）。
- 明确的 🔵 移植版新增（均带编号回归测试）：G-226 错误行、G-252①②、RUNTIME-SWEEP-RACE 启动守卫、三处有界超时、usage fan-out 串行链、PreparedRing、G-256、G-279⑤、G-206、legacy revert V1 解码。

**总体结论**：会话生命周期域移植质量非常高，核心状态机逐行语义等价；主要互操作风险集中在 G-226 的额外错误行与三处有界超时的更激进中断策略。
