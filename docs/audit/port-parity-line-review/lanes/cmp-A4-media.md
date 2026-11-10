# A4 结论: oc-ai/src/media 是 ref 媒体域的高保真逐行移植——23 个 provider 协议、三协议家族与 route/轮询/物化语义全面对齐——但存在 4 个缺陷级偏差（runway cancel 路径重复 `/v1`、replicate/stability unsupported 字段未强制、provider 必填字段缺失被静默默认、cancel 语义反转），且 media-client 统一调用面与模型工厂层是代码自认的未闭 seam。

## 媒体能力矩阵

前提修正：**ref 确有 `meta-images.ts`**（protocols/meta-images.ts:1-154），"ref meta 仅 chat provider"不成立，rust `image/meta.rs` 有真实对应物。`xai-responses.ts` 是 LLM chat 协议，rust 对应 `protocols/xai_responses.rs`，不在本 lane。ref 媒体协议共 23 个（stability 另含 upscale 子协议）。

| provider×模态(家族) | ref | rust | 状态 |
|---|---|---|---|
| openai-images (Stream) | openai-images.ts:278-296 | image/openai.rs:367-404 | ✅ SSE/document 双 framing、multipart vs JSON edits 分流、`stream+partial_images`、stream 模式 n>1 拒绝、usage/settings/`output_format` 回退全同 |
| openai-speech (Stream) | openai-speech.ts:135-146 | speech/openai.rs:173+ | ✅ `/audio/speech`、SSE(带格式时)或 Bytes、unsupported(language) 同 |
| openai-transcription (Stream) | openai-transcription.ts:268-282 | transcription/openai.rs:380+ | ✅ whisper/diarize/gpt-transcribe 能力矩阵、SSE/document、unsupported(speakers) 同 |
| google-images (Inline) | google-images.ts:265-276 | image/google.rs:394-404 | ✅ `:generateContent`、unsupported(mask,size,format)、candidates/RAI filtered/usage 同 |
| google-speech (Stream) | google-speech.ts:135-152 | speech/google.rs:234-252 | ✅ path/framing 按 mode 分流（`?alt=sse`）、unsupported(instructions,speed)、finishReason detail 同 |
| google-transcription (Stream) | google-transcription.ts:185-199 | transcription/google.rs:239-260 | ✅ 同构（共享模板） |
| google-video/Veo (Queued) | google-video.ts:213-228 | video/google.rs:282-334 | ✅ `:predictLongRunning`、operation token、rpc.Status 失败码表(3/7/8/9/11/16)、2 天 retention + poll auth 头侧车、RAI notices/全过滤→ContentPolicy 同 |
| bfl-images (Queued) | bfl-images.ts:204-215 | image/bfl.rs:329-399 | ✅ 模型能力矩阵(fill/expand/kontext/ultra/1.1/dev/flux-2-klein/默认8)、status=result 同 URL、Moderated→ContentPolicy、Task not found→expired、cost 结果覆盖 token、sample 10 分钟 URL 经 materialize_result 拉取 |
| fal-images (Queued) | fal-images.ts + utils/fal-queue.ts | image/fal.rs + fal_queue.rs | ✅ queue.fal.run、绝对 status/response/cancel URL token、COMPLETED+error→failed、size/aspectRatio 互斥校验 |
| fal-video (Queued) | fal-video.ts:107-114 | video/fal.rs:117-119 | ✅ unsupported(n,durationSeconds,references) |
| replicate-images (Queued) | replicate-images.ts:157-175 | image/replicate.rs:265-341 | ✅ official/version 双路径、prediction 状态表(aborted→expired)、data_removed→expired、compute 秒 usage、completed_at+1h 过期、POST cancel；⚠ R-2 |
| runway-video (Queued) | runway-video.ts:172-196 | video/runway.rs:240-313 | ✅ 三路径选择、`X-Runway-Version: 2024-11-06` 头、THROTTLED→queued、SAFETY 正则→ContentPolicy、ASSET.INVALID→InvalidRequest、24h retention、DELETE activeOnly；❌ R-1 |
| stability-images (Inline)+upscale (Queued) | stability-images.ts:187-219 | image/stability.rs:170+,282+ | ✅ `accept: application/json` 头、core 仅文生图、sd3 mode/model 字段、creative upscale 异步轮询 `/v2beta/results/{id}`；⚠ R-2 |
| xai-images (Inline) | xai-images.ts:116-124 | image/xai.rs:121+ | ✅ generations/edits 双路径、unsupported(mask,size,seed,format) |
| xai-video (Queued) | xai-video.ts:186-213 | video/xai.rs:247+ | ✅ generations/edits/extensions 三路径、`/videos/{id}` 轮询、progress 0-100→0-1、未知终态→expired、respect_moderation notice/无 URL→ContentPolicy |
| zai-images (Inline) | zai-images.ts:106-113 | image/zai.rs:124+ | ✅ `/api/paas/v4/images/generations`、7 字段 unsupported |
| meta-images (Inline) | meta-images.ts:138-152 | image/meta.rs:127-151 | ✅ generations/edits、unsupported(mask,aspectRatio,seed)、responseFormat/reasoningStrength/toolEnablement camel 改名、无 file handle |
| assemblyai-transcription (Queued+prepare) | assemblyai-transcription.ts:94-203 | transcription/assemblyai.rs:46-119,265+ | ✅ `/v2/upload` prepare（inline→upload_url 二进制上传）、speaker_labels 三条件、ms→s 换算、语言小写 |
| deepgram-speech (Stream) | deepgram-speech.ts:130-141 | speech/deepgram.rs:227-242 | ✅ `/v1/speak`、Bytes framing、PCM 头推导 |
| deepgram-transcription (Inline) | deepgram-transcription.ts:167-174 | transcription/deepgram.rs:221+ | ✅ `/v1/listen`、unsupported(prompt,speakers) |
| elevenlabs-speech (Stream) | elevenlabs-speech.ts:189-203 | speech/elevenlabs.rs:253-276 | ✅ `/v1/text-to-speech/{voice}`、timestamps===true 时 stream→Lines/generate→Document 否则 Bytes、字符 usage、unsupported(instructions) |
| elevenlabs-transcription (Inline) | elevenlabs-transcription.ts:196-204 | transcription/elevenlabs.rs:261+ | ✅ `/v1/speech-to-text`、unsupported(prompt) |
| cartesia-speech (Stream) | cartesia-speech.ts:162-178 | speech/cartesia.rs:318-336 | ✅ `/tts/bytes` vs `/tts/sse`、`Cartesia-Version: 2026-08-14`、SSE 未 done→incomplete、pcm/raw 输出描述、error 事件分类 |
| fal-queue 共享模板 | utils/fal-queue.ts:44-89 | fal_queue.rs:41-98 | ✅ token camelCase、queue_position、cancel=PUT |
| 协议三家族 Inline/Stream/Queued | route/media-protocol.ts:64-187 | protocol.rs:295-471 (traits) | ✅ 语义同；闭包工厂→trait（结构差异、语义等价） |
| 媒体 route（auth/合并/编码/轮询/重试/取消/物化） | route/media.ts:121-446 | route.rs (drivers+Generation) | ✅ 高保真；差异见 D-1/D-2/R-4 |
| generation.ts 轮询词汇 | generation.ts:4-156 | model.rs:20-147 + route.rs:947-1139 | ✅ Status/Snapshot/events/await/deadline clamp/超时错误；resultEvents 未移植（D-4） |
| Media.Source/Asset 四源 | media.ts:19-323 | schema/media.rs + protocol.rs:262-288 | ✅ headers 运行时侧车不进 serde（G-246 测试覆盖） |
| media-client 统一面（start/resume notQueued、stream 扩展） | media-client.ts:21-75 | **无** | ❌ D-3 |
| 四模态顶层 request/generate/stream/start/resume + ImageModel.fromRoute 部署工厂 | image.ts/speech.ts/video.ts/transcription.ts + media-model.ts | model.rs 仅 wire 数据形状 | ⚠ D-3/D-8 |

## 对比表（逐行精读项）

| 对比项 | ref | rust | 判定 |
|---|---|---|---|
| Source 联合/Asset.inline/bytes/materialize/下载错误文案 | media.ts:102-263 | schema/media.rs + route.rs:602-690 | ✅ `Failed to read media from {url}` 同 |
| Identity 错误契约（frameError/incomplete/unsupported/pending/contentPolicy/ended/status/failure 表） | media-protocol.ts:194-331 | protocol.rs:39-250 | ✅ 消息串逐字节同（测试逐条断言） |
| rejectUnsupported（false 计存在、空数组缺席、`=== true` 语义留给 body_from） | route/media.ts:423-444 | protocol.rs:88-103 + model.rs:737-796 | ✅ 语义同，强制点不同（D-2/R-2） |
| READ_RETRY（1s→30s×8、retryAfterMs 抬升、仅 status/result 重试、解码不重试） | route/media.ts:141-165,183-196 | route.rs:52-105,1120-1138 | ✅ 除去抖动（D-1） |
| submit：route⊕model⊕request http、三层 query 删改、multipart 边界替换 content-type、binary 签名文本、auth 差集 | route/media.ts:316-416 | route.rs:156-338,448-561,737-748,853-873 | ✅ |
| queued start/resume（token 编解码、首观察、typed resume 拒绝）、activeOnly 取消 | route/media.ts:172-245 | route.rs:784-908,1093-1114 | ✅ 除 cancel 缺省语义（R-4） |
| stream：mode 分流、局部解析状态、decoder finish flush、错误补 http | route/media.ts:248-303 | route.rs:1145-1346 | ✅ |
| Generation await/events（deadline clamp、terminal 仍走 result、Timeout 文案） | generation.ts:99-207 | route.rs:910-1062 | ✅ |
| collectResponse×3+转录（无 finish→die→Incomplete） | image.ts:156-168 等 | model.rs:640-706 | ✅ |
| MediaInput（inlineBytes/query 标量校验/dimensions/refID/decodedAsset/appendFields+repeatArrays+空数组 no-op G-286④） | utils/media-input.ts | utils.rs:23-226 | ✅ |
| SpeechStream（空块、timestamps 缺省 0、container/pcm 表、rate= 抽取、无音频报错、headerUsage） | utils/speech-stream.ts | utils.rs:658-856 | ✅ |
| 魔数嗅探/扩展名表/别名 | utils/media-type.ts | utils.rs:236-334 | ✅ |
| RFC3339→epoch ms | JS Date.parse | utils.rs:341-406 | ✅ 手写 days_from_civil |
| bfl 能力矩阵/校验/宽高/input_image_N/轮询=结果/物化/cost 回退 | bfl-images.ts:63-215 | image/bfl.rs:36-312 | ✅ |
| openai-images 保留字段集/SSE 事件/文档帧 requested 回退 | openai-images.ts:109-296 | image/openai.rs:30-349 | ✅ |
| replicate official 正则/256KB data URL 上限/状态表/过期计算 | replicate-images.ts:46-175 | image/replicate.rs:36-341 | ✅（R-2） |
| runway 状态表/失败码正则/三路径/版本头/activeOnly DELETE | runway-video.ts:59-196 | video/runway.rs:42-313 | ✅ 除 R-1 |
| google-video instances/parameters/rpc 码表/RAI/2 天+auth | google-video.ts:66-228 | video/google.rs:41-334 | ✅ |
| xai-video 降维/progress/expired/moderation | xai-video.ts:95-178 | video/xai.rs:95-214 | ✅ |
| meta-images 选项改名/拒绝 ref | meta-images.ts:68-152 | image/meta.rs:19-110 | ✅ |
| assemblyai prepare+speaker_labels+ms→s | assemblyai-transcription.ts:80-179 | transcription/assemblyai.rs:46-119 | ✅ |
| cartesia 双路径/版本头/SSE done 门/error 分类 | cartesia-speech.ts:130-178 | speech/cartesia.rs:283-336 | ✅ |
| elevenlabs framing 三态门 | elevenlabs-speech.ts:193-197 | speech/elevenlabs.rs:267-276 | ✅ |
| openai-transcription 能力矩阵/whisper 不流式 | openai-transcription.ts:112-136 | transcription/openai.rs:44-80 | ✅ |

## 行为差异清单

- **D-1 重试去抖动（有意）**：ref READ_RETRY 用 `Schedule.jittered`；rust 明确去抖动保确定性（route.rs:11-14）。retryAfterMs 抬升/30s 封顶/8 次上限一致。
- **D-2 unsupported 拒绝时机**：ref 在 route submit 入口统一拒绝（route/media.ts:356），先于 prepare 与 body 构建；rust route 驱动不读 `unsupported()`，改由各协议 `body_from` 首行 `check_unsupported`（18 个文件）。当前无协议同时有 prepare+unsupported 声明，故为潜伏差异。
- **D-3 media-client/模型工厂未移植（自认 seam）**：ref media-client.ts 的 per-modality 分发与 image.ts 等顶层 `request/generate/stream/start/resume` API、`ImageModel.fromRoute` 部署工厂（media-model.ts:70-89）在 rust 无对应；rust 用 `model: String` 替代携带 route 的 Model 对象（model.rs:5-8 注明），部署改经 `MediaRouteSpec`。代码注释自认"oc-core/oc-server 尚未消费 media generation"（route.rs:1004-1006）。
- **D-4 `resultEvents` 缺失**：generation.ts:159-181 随 D-3 一并缺失。
- **D-5 sanitizeSurrogates**：ref submit 前净化孤代理项；Rust `String` 类型上不可能存在——不适用而非缺失。
- **D-6 xai-responses 归属**：媒体目录外，`protocols/xai_responses.rs` 覆盖（核对确认非缺口）。
- **D-7 模型对象→id 字符串**：见 D-3；协议层仅需 `model.id`，语义无损。
- **D-8 multipart 边界生成**：TS 由 HTTP client 生成；rust 自渲染 `opencode-media-{n}` 边界（route.rs:197-253）——wire 等价，边界值可预测（测试友好，无安全影响）。

## 正确性风险

- **R-1 runway cancel 路径重复 `/v1`（bug）**：rust `cancel_path` 返回 `/v1/tasks/{id}`（video/runway.rs:310-312），而 base_url 已含 `/v1`（`https://api.dev.runwayml.com/v1`），`CoreTransport::resolve` 直接拼接（route.rs:473-480）→ 实际请求 `https://api.dev.runwayml.com/v1/v1/tasks/{id}` → 404。ref taskPath=`/tasks/{taskID}`（runway-video.ts:170）。status/result 路径正确，仅 cancel 错。
- **R-2 unsupported 声明未强制（行为反转）**：`replicate-images` 声明 7 个 unsupported 字段（image/replicate.rs:271）、`stability` 生成声明 `size,mask`（stability.rs:159-161）、upscale 声明 4 个（stability.rs:271）——但三者 `body_from` 均无 `check_unsupported`（replicate.rs:283-286、stability.rs:172-175），且 route.rs 从不读 `unsupported()`。ref 在 route 层 typed 拒绝；rust 静默丢弃字段——恰是 ref 注释明言"never silently dropped"的场景。其余 18 个协议已在 body_from 内自查。
- **R-3 provider 必填字段缺失被静默默认（系统性宽松）**：rust 各 decode_start/status/result 对 ref Schema 必填字段用 `unwrap_or_default()`：bfl `id/polling_url`（image/bfl.rs:220-236）、fal `request_id/status_url/...`（fal_queue.rs:44-69）、google-video `name`（video/google.rs:145-146）、runway `id`、xai `request_id`、assemblyai `id`、openai-images SSE `b64_json/output_format`（image/openai.rs:216-222,236）等。ref 侧 Schema 解码失败→`InvalidProviderOutput`；rust 侧缺失时产生空 token/空 URL 或空图像资产，错误延迟或丢失。低概率、但违背"无效 provider 文档立即失败"契约。
- **R-4 Generation.cancel 语义反转**：ref 无 cancel 端点的 generation `cancel()` 静默 no-op（generation.ts:109-111）；rust 返回 `UnsupportedOperation` 错误（route.rs:1093-1100）。对 bfl/google-video/xai/assemblyai/fal 家族中未提供 cancel 的协议，调用方行为相反。
- **R-5（低）暴露缺口**：D-3/D-4 意味着 rust 媒体域目前只有协议+route 驱动，无上层消费方；R-1/R-2 在无消费方时不触发，但 seam 关闭时将直接命中。

## 覆盖声明

- **逐行精读（双侧全文）**：ref media.ts, media-model.ts, media-client.ts, image.ts, video.ts, speech.ts, transcription.ts, generation.ts, route/media.ts, route/media-protocol.ts, protocols/utils/{media-input,fal-queue,speech-stream}.ts, protocols/{bfl-images, meta-images, google-video, runway-video, openai-images, replicate-images, xai-responses, elevenlabs-speech, cartesia-speech}.ts（其余 client.ts 为 24-31 行薄封装，已读）；rust media/{mod, model, protocol, route, fal_queue, utils}.rs, image/{bfl, meta, openai, replicate(关键段), stability(关键段)}.rs, video/{google, runway, xai}.rs, speech/elevenlabs.rs(关键段), transcription/assemblyai.rs(关键段)。
- **定向比对（路径/基址/kind/framing/状态表/失败表/unsupported/版本头/取消规格/prepare）**：fal-images、fal-video、xai-images、zai-images、google-images、google-speech、google-transcription、openai-speech、openai-transcription、cartesia-speech、deepgram-speech、deepgram-transcription、elevenlabs-transcription、replicate/stability 补充段——全部与 ref 字面一致。
- **未逐行读**：rust `body.rs`（编码层，经 route.rs 消费面交叉验证）、`schema/media.rs`（经 utils/protocol 测试与 G-246 断言间接验证）、各 rust 文件尾部 #[cfg(test)] 区（仅抽查）。
- **来源**：仅两侧源码；`.slim/deepwork/*.md` 仅用于起点导航，其"ref meta 无 images 面"的推断已被源码证伪。
