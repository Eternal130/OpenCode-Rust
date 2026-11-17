# RECORD-PROXY — opencode v2 日常流量录制代理

`tools/diff/realchain/record_proxy.py` 是一个纯 Python 3.12 标准库的
HTTP 反向代理：把 opencode v2（TS 官方版，2.2.x）日常开发里发出的
provider 线缆流量（工具调用、文件编辑、指令——全部真实交互）逐字
转发给真实上游，同时把每次交互录制为 **cassette v2** 文件，自动积
累成语料，供通道 9 回放网关（replay_gateway.py，另一 lane）未来按
原时序回放。

## 三步启用

**第 1 步 —— 准备 routes.json**（可直接复制样例 `tools/diff/realchain/routes.sample.json`）：

```json
{
  "anthropic": "https://api.anthropic.com",
  "openai": "https://api.openai.com"
}
```

**第 2 步 —— 启动代理**（终端里常驻，或 tmux/screen 里）：

```bash
python3 tools/diff/realchain/record_proxy.py \
    --routes tools/diff/realchain/routes.sample.json \
    --corpus-dir ~/oc-corpus \
    --port 8099
```

启动后打印监听地址与路由表。语料目录会自动创建。

**第 3 步 —— 改 opencode v2 配置里每个 provider 的 baseURL**。
编辑 `~/.config/opencode/opencode.json`，把每个 provider 的
`options.baseURL` 换成本地代理地址，路由名对应 routes.json 的键：

| provider 原 baseURL | 改成 |
|---|---|
| `https://api.anthropic.com` | `http://127.0.0.1:8099/r/anthropic` |
| `https://api.openai.com/v1`（AI SDK 已拼 `/v1` 的写法保持原样，见下） | `http://127.0.0.1:8099/r/openai/v1` |

规则：本地 URL 形如 `http://127.0.0.1:PORT/r/<route>/<rest>`，代理
把它原样映射为 `<routes.json 里 route 的 base URL>/<rest>`。所以
只需把官方域名部分替换成 `/r/<route>`，路径后缀不动。

## 热载验证结论（实测，2026-11-17）

**结论：opencode v2.2.7 的 config watcher 确实热载 baseURL，改完
配置无需重启，下一次对话即走新地址。**

实验方法（stub 上游，不碰真 provider）：

1. 起两个本地 stub 上游（端口 A=4721、B=4722，模拟两个不同
   baseURL，记录每次命中）；
2. 隔离 XDG 目录写 `opencode.json`（`@ai-sdk/openai-compatible`
   provider，baseURL→A），起 `opencode serve`（v2.2.7，
   /root/.opencode/bin/opencode）；
3. 经 HTTP API（`POST /api/session` + `POST
   /api/session/{id}/prompt`，Basic auth）发第一条消息 → stub 日志
   命中端口 A（2 次命中：标题生成 + 主回复，均在 A）；
4. 服务器进程**保持运行**，改写 `opencode.json` 的 baseURL→B；
   等 3 秒再发第二条消息 → 命中全部落在端口 B（A 零命中）。

证据（stub 命中日志，时间戳 Unix）：prompt A 后
`[{"port":4721,"path":"/v1/chat/completions",...},{"port":4721,...}]`；
改配置后 prompt B →
`[{"port":4722,...},{"port":4722,...}]`。
完整实验记录：`.test-logs/hotload-experiment.log`（复现脚本思路见
上文；API 鉴权为 `Authorization: Basic base64("opencode:<server
password>")`，server password 在 `opencode serve` 启动日志里）。

因此**日常接入不需要重启 opencode**：改完 baseURL，下一个新会话
（或继续中的会话下一条消息）即经代理录制。若你的版本行为不同，
回退路径是：退出 opencode → 改 baseURL → `opencode`（TUI 内
`/resume` 或 `opencode -c` 继续会话）。

## 隐私说明

* **密钥红线**：`Authorization` / `x-api-key` / `api-key`（及
  cookie）头会原样透传给上游（你的使用不受影响），但**绝不落盘**——
  cassette 里这些头一律记为 `<redacted>`。自测显式断言密钥字节不
  出现在任何落盘文件中。
* **内容存本地**：cassette 只写入你用 `--corpus-dir` 指定的本地目
  录，代理不做任何网络上报。注意：cassette **包含完整请求/响应明
  文**（那是它的用途），目录权限请自行收紧（如 `chmod 700`）。
* **崩溃安全**：每个交互完成即原子落盘（tmp+rename），代理崩溃/
  重启不损已有 cassette。
* **后续入库流程（规划）**：语料积累后，将按既定 age 流程做本地
  脱敏审查 → age 加密 → 打包进入回放语料库；未经加密不入任何仓库。
  在此之前 corpus-dir 纯属你机器上的私有数据。

## Cassette v2 格式（钉死规格）

```json
{"version": 2,
 "metadata": {"recordedAt": "...", "proxy": "record_proxy/1",
              "route": "anthropic", "model": "claude-...",
              "transport": "http"},
 "interactions": [{"transport": "http",
   "request": {"method": "POST", "path": "/v1/messages",
               "headers_redacted": {"x-api-key": "<redacted>"},
               "body": "..."},
   "response": {"status": 200, "headers_redacted": {},
                "frames": [{"blob": "event: message_start\n...",
                            "delay_ms": 342.1}]}}]}
```

`frames` 逐 SSE 事件块记录，`delay_ms` 为距上一块的毫秒数（首块
即 TTFT）；非流式 JSON 响应折叠为单帧。W5 回放网关合流时按帧+延迟
还原 TTFT/TPS 时序。

## 透传保真

* SSE 逐块转发：上游首字节到达即 flush 给客户端，TTFT 零附加延迟；
* 状态码与头忠实透传（仅剥 hop-by-hop 头；转发用 chunked 编码或精
  确 Content-Length）；
* 上游 HTTPS 走 Python 标准库 TLS；
* 并发会话安全：每连接一个线程，cassette 写出串行化。

## 自测

```bash
python3 tools/diff/realchain/record_proxy_test.py
```

4 个用例（stub 上游，不碰真 provider）：SSE 逐字节透传+cassette v2
形状+密钥脱敏、非流式 JSON 单帧、并发两请求各自成 interaction、
未知路由 404 不落盘。
