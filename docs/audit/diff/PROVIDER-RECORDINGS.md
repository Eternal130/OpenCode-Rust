# PROVIDER-RECORDINGS — provider 协议解码差分（通道 7）

同一录制的响应体（SSE 文本 / JSON body，ref HttpRecorder cassette 真实 provider 流量）
喂双侧协议解码器，diff 归一化后的 LLMEvent 解码事件流。请求体不参与解码，仅提取
provider/model id 作为解码器 initial 状态的 harness 上下文。

- rust 侧: `crates/oc-ai/tests/provider_recordings_diff_driver.rs`（env 门控
  `PROVIDER_DIFF_MANIFEST/OUT`）— 生产 route + `MockTransport` 逐响应体回放（与
  cassette parity 测试同一入口）。驱动面: crates/oc-ai/tests/provider_recordings_diff_driver.rs:1
- ref 侧: `tools/diff/drivers/provider_recordings_ref.ts`（bun staging，node_modules →
  ref/packages/ai/node_modules）— 直接驱动 packages/ai/src/protocols/* 的真实
  step 机（stream.event 解码 + initial + step + terminal/onHalt + 路由级 SSE framing）。
  驱动面: tools/diff/drivers/provider_recordings_ref.ts:1

归一化声明: 键序递归排序；text-*/reasoning-* 事件的本地 part id 剥离（生成源差异，
双侧一致剥离；tool-call id 为 wire 数据保留）；finish reason raw/normalized 小写归一；
数值按值比较（0==0.0）；usage/providerMetadata 全量保留比较；解码错误只比较 ok 位与
错误前事件前缀（错误文案不比较）——双侧都错=MATCH，单侧错=DIVERGE。

## 总量: 264 个可驱动交互，MATCH 241，DIVERGE 23

| 协议族 | 交互数 | MATCH | DIVERGE |
|---|---|---|---|
| anthropic-messages | 54 | 54 | 0 |
| cohere-chat | 4 | 4 | 0 |
| gemini | 9 | 9 | 0 |
| google-interactions | 7 | 1 | 6 |
| meta-responses | 9 | 9 | 0 |
| mistral-chat | 6 | 6 | 0 |
| open-responses | 36 | 36 | 0 |
| openai-chat | 122 | 105 | 17 |
| openai-responses | 15 | 15 | 0 |
| xai-responses | 2 | 2 | 0 |

## 范围外: 144 个交互（cassette 目录 32 组整组范围外 + 混合 cassette 内未映射端点）

整组范围外（原因）:

| 组 | cassette 数 | 范围外交互 | 原因 |
|---|---|---|---|
| assemblyai-transcription | 1 | 5 | no LLM protocol decoder mapping for this channel |
| bedrock-converse | 4 | 5 | binary AWS event-stream framing (not text SSE/JSON decode) |
| bedrock-converse-cache | 1 | 2 | binary AWS event-stream framing |
| bedrock-converse-thinking-binding | 2 | 4 | binary AWS event-stream framing |
| bfl-images | 1 | 4 | no LLM protocol decoder mapping for this channel |
| cartesia-speech | 2 | 2 | no LLM protocol decoder mapping for this channel |
| cloudflare-ai-gateway | 6 | 8 | gateway-wrapped provider translation |
| deepgram-speech | 2 | 2 | no LLM protocol decoder mapping for this channel |
| deepgram-transcription | 2 | 3 | no LLM protocol decoder mapping for this channel |
| elevenlabs-speech | 2 | 2 | no LLM protocol decoder mapping for this channel |
| elevenlabs-transcription | 2 | 3 | no LLM protocol decoder mapping for this channel |
| fal-images | 1 | 5 | no LLM protocol decoder mapping for this channel |
| fal-video | 1 | 11 | no LLM protocol decoder mapping for this channel |
| google-images | 2 | 2 | no LLM protocol decoder mapping for this channel |
| google-speech | 2 | 2 | no LLM protocol decoder mapping for this channel |
| google-transcription | 2 | 2 | no LLM protocol decoder mapping for this channel |
| google-video | 1 | 9 | no LLM protocol decoder mapping for this channel |
| meta-images | 3 | 4 | no LLM protocol decoder mapping for this channel |
| openai-images | 3 | 3 | no LLM protocol decoder mapping for this channel |
| openai-responses-websocket | 3 | 5 | websocket channel transport |
| openai-speech | 2 | 2 | no LLM protocol decoder mapping for this channel |
| openai-transcription | 4 | 4 | no LLM protocol decoder mapping for this channel |
| opencode-zen-evaluation | 1 | 1 | evaluation fixture (not a protocol decode lane) |
| openrouter-evaluation | 1 | 1 | evaluation fixture |
| replicate-images | 1 | 3 | no LLM protocol decoder mapping for this channel |
| runway-video | 1 | 12 | no LLM protocol decoder mapping for this channel |
| stability-images | 2 | 8 | no LLM protocol decoder mapping for this channel |
| typesafe-evaluation | 1 | 1 | evaluation fixture |
| vercel-native | 9 | 17 | gateway-wrapped step machine (usage/cost merge seam) |
| xai-images | 2 | 2 | no LLM protocol decoder mapping for this channel |
| xai-video | 1 | 7 | no LLM protocol decoder mapping for this channel |
| zai-images | 1 | 1 | no LLM protocol decoder mapping for this channel |

混合 cassette 部分范围外:

| 组 | cassette 数 | 纳入 | 范围外 | 原因 |
|---|---|---|---|---|
| pdf | 10 | 9 | 2 | interaction endpoint not in this channel's family table (e.g. bedrock binary / media) |

cassette 总数（目录内 .json）: 270；语料含 4 个顶层散装 fixture（evaluation，非 cassette 格式，范围外）。

## 分歧明细

### deepinfra-chat/deepinfra-llama-3-3-70b-tool-loop#0 — openai-chat
- 差异: `$.events[5].usage.providerMetadata.deepinfra.estimated_cost: rust=2.878e-05 ref=2.8780000000000002e-05`
- rust ok=True events=7 error=False
- ref  ok=True events=7 error=False

### google-interactions/continues-tool-results-with-previous-interaction-id#0 — google-interactions
- 差异: `$.events[1].providerMetadata: missing on rust side`
- rust ok=True events=9 error=False
- ref  ok=True events=9 error=False

### google-interactions/continues-tool-results-with-previous-interaction-id#1 — google-interactions
- 差异: `$.events[1].providerMetadata: missing on rust side`
- rust ok=True events=9 error=False
- ref  ok=True events=9 error=False

### google-interactions/replays-native-tool-results-and-signatures-statelessly#0 — google-interactions
- 差异: `$.events[1].providerMetadata: missing on rust side`
- rust ok=True events=9 error=False
- ref  ok=True events=9 error=False

### google-interactions/replays-native-tool-results-and-signatures-statelessly#1 — google-interactions
- 差异: `$.events[1].providerMetadata: missing on rust side`
- rust ok=True events=9 error=False
- ref  ok=True events=9 error=False

### google-interactions/streams-reasoning-and-retains-thought-signatures#0 — google-interactions
- 差异: `$.events[1].providerMetadata: missing on rust side`
- rust ok=True events=25 error=False
- ref  ok=True events=25 error=False

### google-interactions/streams-text-and-reports-usage#0 — google-interactions
- 差异: `$.events[1].providerMetadata: missing on rust side`
- rust ok=True events=8 error=False
- ref  ok=True events=8 error=False

### venice-chat/claude-opus-4-8-preserves-reasoning-through-a-tool-loop-and-follow-up#0 — openai-chat
- 差异: `$.events[10].providerMetadata: missing on ref side`
- rust ok=True events=14 error=False
- ref  ok=True events=14 error=False

### venice-chat/claude-opus-4-8-preserves-reasoning-through-a-tool-loop-and-follow-up#1 — openai-chat
- 差异: `$.events[9].providerMetadata: missing on ref side`
- rust ok=True events=12 error=False
- ref  ok=True events=12 error=False

### venice-chat/claude-opus-4-8-preserves-reasoning-through-a-tool-loop-and-follow-up#2 — openai-chat
- 差异: `$.events[3].providerMetadata: missing on ref side`
- rust ok=True events=6 error=False
- ref  ok=True events=6 error=False

### venice-chat/deepseek-v4-flash-preserves-reasoning-through-a-tool-loop-and-follow-up#0 — openai-chat
- 差异: `$.events[15].providerMetadata: missing on ref side`
- rust ok=True events=19 error=False
- ref  ok=True events=19 error=False

### venice-chat/deepseek-v4-flash-preserves-reasoning-through-a-tool-loop-and-follow-up#1 — openai-chat
- 差异: `$.events[16].providerMetadata: missing on ref side`
- rust ok=True events=19 error=False
- ref  ok=True events=19 error=False

### venice-chat/deepseek-v4-flash-preserves-reasoning-through-a-tool-loop-and-follow-up#2 — openai-chat
- 差异: `$.events[14].providerMetadata: missing on ref side`
- rust ok=True events=17 error=False
- ref  ok=True events=17 error=False

### venice-chat/gemini-3-1-pro-preview-preserves-reasoning-through-a-tool-loop-and-follow-up#0 — openai-chat
- 差异: `$.events[6].providerMetadata: missing on ref side`
- rust ok=True events=10 error=False
- ref  ok=True events=10 error=False

### venice-chat/gemini-3-1-pro-preview-preserves-reasoning-through-a-tool-loop-and-follow-up#1 — openai-chat
- 差异: `$.events[5].providerMetadata: missing on ref side`
- rust ok=True events=8 error=False
- ref  ok=True events=8 error=False

### venice-chat/gemini-3-1-pro-preview-preserves-reasoning-through-a-tool-loop-and-follow-up#2 — openai-chat
- 差异: `$.events[5].providerMetadata: missing on ref side`
- rust ok=True events=8 error=False
- ref  ok=True events=8 error=False

### venice-chat/gemini-preserves-per-call-signatures-through-parallel-tools#0 — openai-chat
- 差异: `$.events[5].providerMetadata: missing on ref side`
- rust ok=True events=11 error=False
- ref  ok=True events=11 error=False

### venice-chat/gemini-preserves-per-call-signatures-through-parallel-tools#1 — openai-chat
- 差异: `$.events[7].providerMetadata: missing on ref side`
- rust ok=True events=10 error=False
- ref  ok=True events=10 error=False

### venice-chat/gemini-preserves-per-call-signatures-through-parallel-tools#2 — openai-chat
- 差异: `$.events[3].providerMetadata: missing on ref side`
- rust ok=True events=6 error=False
- ref  ok=True events=6 error=False

### venice-chat/openai-gpt-54-mini-preserves-reasoning-through-a-tool-loop-and-follow-up#0 — openai-chat
- 差异: `$.events[94].text: rust="\n\n" ref="\n\n__ENCRYPTED_REASONING__id=rs_014406f0f45e19c6016ac312f01d2487d2a5f2bdb7fbc42f76\ngAAAAABqwxLxEN4vtmI2WcZ_aDDttdmPwAHgnV2gLZBo_f_mmTdGm4oZzE2JVzcj72AeXHyABm`
- rust ok=True events=100 error=False
- ref  ok=True events=100 error=False

### venice-chat/openai-gpt-54-mini-preserves-reasoning-through-a-tool-loop-and-follow-up#1 — openai-chat
- 差异: `$.events[67].text: rust="\n\n" ref="\n\n__ENCRYPTED_REASONING__id=rs_014406f0f45e19c6016ac312f26e4887d2af2f777fa50f9801\ngAAAAABqwxL0Nl6Jl5EZuOS3A9KuObO5gPHpF1y4xbTEmtnZFQ3yz-u6TiK9EQuW1YliegqQQP`
- rust ok=True events=72 error=False
- ref  ok=True events=72 error=False

### venice-chat/openai-gpt-54-mini-preserves-reasoning-through-a-tool-loop-and-follow-up#2 — openai-chat
- 差异: `$.events[60].text: rust="\n\n" ref="\n\n__ENCRYPTED_REASONING__id=rs_014406f0f45e19c6016ac312f4e99887d2b9c8c820d5e078c9\ngAAAAABqwxL2s7RKHoOP95Fkym7ikKzN-DEIDdKIvEaCL9uxm3gpgY3MBOf5vMOPJi2RVbKfFS`
- rust ok=True events=65 error=False
- ref  ok=True events=65 error=False

### venice-chat/qwen-disables-reasoning#0 — openai-chat
- 差异: `$.events[5].providerMetadata: missing on ref side`
- rust ok=True events=8 error=False
- ref  ok=True events=8 error=False

## 定性

### deepinfra estimated_cost float precision（1 个交互）
- 判定: 同一 wire 值 2.8780000000000002e-05：ref 原样保留 f64 wire 数值，rust 的 map_usage→providerMetadata 通路把数值精度截断（2.878e-05，疑似 f32 中转），usage.providerMetadata 数值不再逐位等于 wire
- ref : ref packages/ai/src/protocols/openai-chat.ts（usage passthrough）
- rust: rust crates/oc-ai/src/protocols/openai_chat.rs:432-457（map_usage/json_value_map）
- 交互: deepinfra-chat/deepinfra-llama-3-3-70b-tool-loop#0

### google-interactions 空 providerMetadata 命名空间（6 个交互）
- 判定: ref 在 reasoning-start 等事件上无条件附着 providerMetadata 命名空间对象（metadata(state,step) 无内容时为 {google:{}} 空对象）；rust 仅在存在实际字段（如 thoughtSignature）时附着——空命名空间对象的附着语义差异
- ref : ref packages/ai/src/protocols/google-interactions.ts:291,377
- rust: rust crates/oc-ai/src/protocols/google_interactions.rs（事件投影按内容条件附着）
- 交互: google-interactions/continues-tool-results-with-previous-interaction-id#0, google-interactions/continues-tool-results-with-previous-interaction-id#1, google-interactions/replays-native-tool-results-and-signatures-statelessly#0, google-interactions/replays-native-tool-results-and-signatures-statelessly#1, google-interactions/streams-reasoning-and-retains-thought-signatures#0, google-interactions/streams-text-and-reports-usage#0

### venice 空 providerMetadata.venice 命名空间（13 个交互）
- 判定: 与 google-interactions 相反方向: rust 在 tool-input-end/reasoning-end 等事件上附着空的 {venice:{}} 命名空间对象；ref 仅在存在实际字段（messageThoughtSignature/thoughtSignature/encryptedReasoningContent）时附着
- ref : ref packages/ai/src/protocols/venice-chat.ts:325-345（条件附着）
- rust: rust crates/oc-ai/src/protocols/openai_chat.rs:72,1477（venice 状态机无条件附着空命名空间）
- 交互: venice-chat/claude-opus-4-8-preserves-reasoning-through-a-tool-loop-and-follow-up#0, venice-chat/claude-opus-4-8-preserves-reasoning-through-a-tool-loop-and-follow-up#1, venice-chat/claude-opus-4-8-preserves-reasoning-through-a-tool-loop-and-follow-up#2, venice-chat/deepseek-v4-flash-preserves-reasoning-through-a-tool-loop-and-follow-up#0, venice-chat/deepseek-v4-flash-preserves-reasoning-through-a-tool-loop-and-follow-up#1, venice-chat/deepseek-v4-flash-preserves-reasoning-through-a-tool-loop-and-follow-up#2, venice-chat/gemini-3-1-pro-preview-preserves-reasoning-through-a-tool-loop-and-follow-up#0, venice-chat/gemini-3-1-pro-preview-preserves-reasoning-through-a-tool-loop-and-follow-up#1, venice-chat/gemini-3-1-pro-preview-preserves-reasoning-through-a-tool-loop-and-follow-up#2, venice-chat/gemini-preserves-per-call-signatures-through-parallel-tools#0, venice-chat/gemini-preserves-per-call-signatures-through-parallel-tools#1, venice-chat/gemini-preserves-per-call-signatures-through-parallel-tools#2, venice-chat/qwen-disables-reasoning#0

### venice encrypted reasoning text passthrough（3 个交互）
- 判定: Venice 把加密推理以 `__ENCRYPTED_REASONING__` 文本内联在 reasoning delta 中流回；ref 原样保留该 marker 文本（后续靠 venice-chat.ts 的 assistant 回放约定消费），rust 侧把 marker 段从 reasoning 文本中剥除——两侧对 marker 文本的流内语义不同
- ref : ref packages/ai/src/protocols/venice-chat.ts:180（MARKER）
- rust: rust crates/oc-ai/src/protocols/openai_chat.rs:72,1477（VENICE_MARKER 剥除逻辑）
- 交互: venice-chat/openai-gpt-54-mini-preserves-reasoning-through-a-tool-loop-and-follow-up#0, venice-chat/openai-gpt-54-mini-preserves-reasoning-through-a-tool-loop-and-follow-up#1, venice-chat/openai-gpt-54-mini-preserves-reasoning-through-a-tool-loop-and-follow-up#2

