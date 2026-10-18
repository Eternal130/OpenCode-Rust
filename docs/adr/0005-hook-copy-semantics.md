# ADR-0005: hook 事件跨界为 copy-in/copy-out，等价标准是可观察等价（JSON in/out），非内部表示位同一

- 状态：accepted（记录既有决定；依据 PARITY-LOOP §11.3）
- 日期：2026-10-18
- 上游版本：2.0.24 (ref e7a34f09bf)
- 背景：v2 的 `trigger(domain, name, event)` 按注册顺序把同一个 live event 对象按引用依次交给每个 handler，最后返回该对象（ref `packages/core/src/plugin/hooks.ts:88-95`）——进程内按引用变异。Rust 移植里 guest hook 运行在 bun 子进程（ADR-0001），事件必须序列化跨界，"同一 live 对象的引用链"在物理上不存在。
- 决定：hook 语义按**可观察等价**定义：跨进程边界的 guest handler 是 copy-in/copy-out——事件以 JSON 送入（`hook.dispatch` RPC），每个 guest handler 拿到自己的副本，修改后的副本在下一个 handler 之前流回宿主；进程内 Rust handler 保持真正的 `&mut` 顺序链。等价判据是宿主可观察行为：注册顺序触发、顺序同对象变异的净效果、整字段替换/标量改写/键删除等 v2 保证的 mutation 形状——而非内部表示位同一。
- 后果：代价是依赖"观察宿主 live 对象中途变异"的病态插件会看到副本而非 live 引用（已在 `hooks.rs` 文档化为 documented divergence, oracle gate 2 finding 3）；每次 dispatch有序列化往返 + 超时上限。收益是语义明确且可测：corpus 测试对顺序与 mutation 形状逐项断言，post-return 变异等已登记分歧被固定为文档行为而非偶发。
- 证据（path:line / 测试）：
  - `crates/oc-core/src/hooks.rs:14-27`（Mutation semantics 段：进程内 `&mut` 链保留；跨界 copy-in/copy-out 为 documented divergence）
  - `crates/oc-plugin-host/src/context/mod.rs:106-133`（`Bridge::dispatch_hook`：event 作为 `serde_json::Value` 经 `hook.dispatch` 送入、取回 `result.event`——JSON in/out 边界）
  - `crates/oc-plugin-host/src/lib.rs:48-52`（死锁协议第 3 条：所有 bridge 调用超时上限——注册 2s / 派发 5s）
  - 测试：`crates/oc-plugin-host/tests/corpus.rs:52-96`（`corpus_hook_mutation_ordering`：注册顺序、顺序同对象变异）、`corpus.rs:98-110+`（`corpus_hook_mutation_replacement_shapes`：D-03 变异形状经 copy-in/copy-out 桥可宿主观察；post-return 变异断言为文档化分歧）
  - `crates/oc-core/src/tool/tests.rs:1340`（`tool_pipeline_before_hook_mutates_input_and_after_observes_result`：before hook 变异入参、after 观察结果的进程内链路回归）
  - ref 对照：`/opt/opencode-ref/packages/core/src/plugin/hooks.ts:88-95`（v2 `trigger`：同一 event 对象按引用顺序传递并返回）
- 复审日期：Batch 2 结束
