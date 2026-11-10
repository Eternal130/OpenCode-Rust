# 第十九轮复核 V-C（server/CLI/TUI）— 2026-11-09

> 对 17 通道审查（docs/audit/L1-L17）产出候选逐项现行源码双侧复核，与登记册去重。15 项候选：NEW-CONFIRM 10（含 3 ℹ️ 登记类）/ RETRACT 5。

1. **pairing/ticket 淘汰"最近到期" vs LRU — RETRACT**：rust pairing.rs:52-60、tickets.rs:67-76 `min_by_key(expires_at)`；ref Cache.make LRU。全部条目 TTL 相同、消费即删、无读刷新 → 最近到期≈最旧插入≈LRU，无观察者可区分。
2. **CORS preflight Vary 集合 — NEW-CONFIRM 🟢**：rust middleware.rs:129 仅 `Vary: Origin`（:148-165 回显 method/headers）；ref 经 effect#5935 仅设 `Vary: Access-Control-Request-Headers`——**两侧 Vary 集合都不完整且互不相同**。浏览器预检缓存 key 含 method+headers 不受影响；仅共享代理/CDN 可能复用窄回显。修法一行：`Vary: Origin, Access-Control-Request-Method, Access-Control-Request-Headers`（顺带优于 ref）。
3. **run 附件 MIME 子集 — NEW-CONFIRM 🟡**：rust run/mod.rs:113-142 魔数(PNG/JPEG/GIF/WEBP)+5 扩展；ref run.ts:202 FSUtil.mimeType 全表（fs-util.ts:234-236）。`.svg` rust 走 utf8 文本分支、ref 得 image/svg+xml data URL；`.bmp/.tiff/.ico/.avif/.heic` ref 得 image/*、rust 得 octet-stream。G-278 只修 fs/read 面。修法：detect_mime 扩用 G-278 双探测器同表。
4. **devtools channel=local 默认开 — NEW-CONFIRM ℹ️**：rust store.rs:11093-11095 仅读 debug.devtools、全 TUI 无 channel 概念；ref app.tsx:457 `?? app.channel === "local"`。开发者默认态差异，登记有界偏离（或引 channel 常数补默认）。
5. **filetype_for_path 扩展表偏窄 — NEW-CONFIRM 🟡**：rust syntax.rs:399-422 仅 ~24 扩展，而 oc-syntax/queries.rs 注册表（42 filetype）已含 markdown/bash/lua/xml/scala/elixir/haskell/ocaml/fsharp/clojure/nix/r/hcl/vim 等语法；ref util/filetype.ts ~115 项。受损面：`.md .sh/.zsh .lua .xml .scala .ex/.exs .hs .ml .fs/.fsx .clj .nix .r .tf .jl .vim`——语法在位却被退化纯文本（diff 查看器 diff_viewer_solid.rs:741 整族高亮缺失）。修法：按注册表反向补映射（纯表扩零风险）。
6. **高亮缓存满 128 clear() 全清 — NEW-CONFIRM 🟢**：syntax.rs:382-384 与 oc-syntax/engine.rs:255-257 双层均 clear()；engine.rs:15 文档自述 "drops the oldest" 与实现不符。影响仅满载一次性重解析抖动。修法：LRU 或逐出最旧+修注释。
7. **valid_namespace 校验缺口 — RETRACT**：动态面（唯一外部注入路径）已有完整门——registrations.rs:166-194 `tool_registration_error` 逐字覆盖 ref tool.ts:304-326（namespaceError+name 正则+保留名），:782 拒绝并记同款日志。registry.rs:61-70 缺口仅作用静态 builtin（编译期固定）。
8. **websearch 同意流程超时 + tavily 文案 — NEW-CONFIRM 🟢**：rust websearch.rs:22,508,584 两表单各 60s（choose 可达 120s）；ref websearch.ts:141-144 一个 "1 minute" 包整个 permit。tavily 超时值 25s 已对齐但错误走泛化 to_string()（websearch_providers.rs:470），ref 专用文案 "Tavily web search request timed out"（tavily.ts:71-74）。修法：外层 60s 包两表单；tavily 分支专用文案。
9. **NODE_EXTRA_CA_CERTS 不识别 — NEW-CONFIRM 🟡**：rust oc-http/lib.rs:17-60 platform-verifier→webpki，全链无该 env（grep 零命中）；ref websocket-constructor.ts:66 Node TLS 继承增量信任。企业 MITM CA 环境下 rust 全部 HTTPS（模型 API/websearch/webfetch）握手失败。修法：platform_tls_config 读 NODE_EXTRA_CA_CERTS（Node 冒号分隔多路径语义）追加 RootCertStore。
10. **matchesVersion 谓词 — RETRACT**：ref 函数形态全仓零生产调用（死 API 面），现行等价。
11. **SSE 空闲看门狗缺失 — NEW-CONFIRM 🟡**：ref solid/connection.ts 三件套（connectTimeout 2s、首帧必须 server.connected、45s idleTimeout "Event stream stalled"）；rust oc-tui/data.rs:1067-1161 重连环无 2s 截止、首帧无类型断言、:1151 `stream.next_json().await` 无超时——对端静默挂死时无限阻塞仅靠 TCP/OS 兜底。修法：next_json 包 45s timeout 触发重连 + subscribe 2s deadline + 首帧校验。
12. **models.dev options.file — RETRACT**：OPENCODE_MODELS_PATH/URL 已接线（server_process_env.rs:76-77,106-127→EngineOptions.file/url）；`--models-file` 旗标两侧均无。
13. **serve env 旋钮透传 — RETRACT（等价）**：rust 由 boot/config 层直读 env（server_process_env.rs:70-78、config.rs:393、config/variable.rs:272、:28 DISABLE_FFF），与 ref server-process.ts:110-152 逐项对应。
14. **ssh-askpass argv 位置 — NEW-CONFIRM ℹ️（纠偏登记）**：rust ssh_askpass.rs:19-25 `args().skip(1)`；ref `argv.slice(2)` 在编译单文件形态下取空串（真实缺陷在 ref 侧），rust argv[1] 符合 OpenSSH `[程序, prompt]` 约定。ℹ️ 有意纠偏入册，不改码。
15. **boot 300ms 丢弃窗 — NEW-CONFIRM ℹ️（G-17 子注）**：app.rs:7924,8646 BOOT_DISCARD_WINDOW=300ms；登记册全文无此条，G-17 rust-extra 列举未含。建议追加 G-17 子注（注明可能吞极快首键的有界代价）。

汇总：NEW 10（🟡×4：#3/#5/#9/#11；🟢×3：#2/#6/#8；ℹ️×3：#4/#14/#15）；RETRACT 5（#1/#7/#10/#12/#13）。优先修序：#9 → #11 → #3 → #5（纯表扩最廉价）→ #8/#2/#6 微修 → #4/#14/#15 登记。
