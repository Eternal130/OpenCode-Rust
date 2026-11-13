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

## 总量: 264 个可驱动交互，MATCH 264，DIVERGE 0

| 协议族 | 交互数 | MATCH | DIVERGE |
|---|---|---|---|
| anthropic-messages | 54 | 54 | 0 |
| cohere-chat | 4 | 4 | 0 |
| gemini | 9 | 9 | 0 |
| google-interactions | 7 | 7 | 0 |
| meta-responses | 9 | 9 | 0 |
| mistral-chat | 6 | 6 | 0 |
| open-responses | 36 | 36 | 0 |
| openai-chat | 122 | 122 | 0 |
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

无分歧。
## 定性

- 全量一致。
