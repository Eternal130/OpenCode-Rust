# PTY-WIRE Census（最小验证 · 通道 3/∞）

> persistent-pty daemon wire op/response 双侧对拍 + 非持久 pty 事件/帧协议三轴。

> 生成命令：`python3 tools/census/pty_wire.py --write docs/audit/census/PTY-WIRE.md`
> 范围声明：仅比对 wire **op/tag 名**与协议常量；字段级差异（如 ref `rows.terminal` 必填 vs rust `Option`、rust `attached` 追加 `replay_chunked`/`replay_length`/`replay_complete`、`subscribe.chunked_replay` 能力位）不逐字段 diff，仅在差异项旁注。

## 轴 1：persistent-pty 请求 op 集（ref exchange 站点 ↔ rust `WireRequest`）

- ref（packages/core/src/persistent-pty/index.ts + daemon.ts 的 `{ op: … }`）：14 个 op
- rust（crates/oc-core/src/persistent_pty/wire.rs `WireRequest` 枚举，snake_case serde tag；`Unknown` 为 `#[serde(other)]` 兼容臂不计）：16 个 op
- **BOTH：14** · REF_ONLY：**0** · RUST_ONLY：**2**
  - 🔗 RUST_ONLY `read` — rust crates/oc-core/src/persistent_pty/wire.rs:333（ref 无此 op；见下 G-10 背书）
  - 🔗 RUST_ONLY `restore` — rust crates/oc-core/src/persistent_pty/wire.rs:361（ref 无此 op；见下 G-10 背书）
  - REF_ONLY 0：ref 全部 op rust 均实现

## 轴 2：persistent-pty 响应/流 tag 集（ref `WireResponse`/`StreamEvent` ↔ rust `WireResponse`）

- ref `WireResponse` union（daemon.ts）`Schema.Literal` tag：15 个
- rust `WireResponse` 枚举 tag：16 个
- **BOTH：15** · REF_ONLY：**0** · RUST_ONLY：**1**
  - 🔗 RUST_ONLY `replay_complete` — rust crates/oc-core/src/persistent_pty/wire.rs:459（见下背书）
- ref `StreamEvent` union tag：controller_changed, exited, foreground_process_changed, output, resized, title_changed（`output` 在 rust 侧为二进制输出帧 `[0x00][start u64BE][end u64BE][data]`，非常驻 JSON tag；`resized`/`exited`/`controller_changed`/`title_changed`/`foreground_process_changed` 已由 rust `WireResponse` 覆盖，属轴 2 BOTH）

## 轴 3：非持久 pty 事件与帧协议（ref schema/pty.ts + pty/protocol.ts ↔ rust oc-schema/pty.rs + handlers/pty.rs）

- 事件 tag：ref 4 个 ↔ rust 4 个；**BOTH：4** · REF_ONLY：**0** · RUST_ONLY：**0**
  - 双侧一致：pty.created, pty.deleted, pty.exited, pty.updated
- 帧协议常量对拍（双侧亲读核实）：
  - 协议版本 7：rust crates/oc-core/src/persistent_pty.rs:509 ↔ ref /opt/opencode-ref/packages/core/src/persistent-pty/daemon.ts:8（G-10：超集共用版本号 7）
  - 控制帧首字节 0x00：rust crates/oc-server/src/handlers/pty.rs:342 ↔ ref /opt/opencode-ref/packages/core/src/pty/protocol.ts:18
  - replay 分块 64 KiB：rust crates/oc-server/src/handlers/pty.rs:344 ↔ ref /opt/opencode-ref/packages/core/src/pty/protocol.ts:13

## Census 裁定

- 轴 1 已裁定 op：16/16（覆盖率 100%）；轴 2 已裁定 tag：16/16（覆盖率 100%）；轴 3 已裁定事件：4/4（覆盖率 100%）
- 结论：**缺口候选（REF_ONLY）全部为 0**。RUST_ONLY 共 3 项，均为**有据分歧**：
  - op `read` — rust wire.rs 标注“Extension (§3): session-keyed read against the daemon-side `current[]` map”（rust-only 扩展，daemon 侧持有 current 映射）；
  - op `restore` — rust wire.rs 标注“Extension (§6 arm B): snapshot restore — owner-only”（§6 接管路径 B 的快照恢复扩展）；
  - tag `replay_complete` — rust §3.2 分块 replay 传输的结束标记，与 `subscribe.chunked_replay` 能力位（G-383b）配套，缺省时 wire 形状与 ref v7 完全一致。
  - 三者均受 **G-10 背书**（“persistent-pty 协议超集共用版本号 7”）：rust 在同一 protocol=7 上提供超集 op，旧客户端不受影响（`Unknown` 臂吸收未知 op）。

## 验证记录

1. **交叉验证方法**：
   - 轴 1：ref 侧以 persistent-pty/{index,daemon}.ts 中全部 `{{ op: "…" }}` exchange 站点为权威（14 个：list/create/write/resize/control/input/snapshot/read_rows/terminate + ping/shutdown/prepare_handoff/own/subscribe）；rust 侧 `WireRequest` 枚举变体按 `rename_all = "snake_case"` 还原 wire tag（16 个，剔除 `#[serde(other)]` 的 `Unknown`）。两侧亲读核实（含 daemon.rs dispatch `WireRequest::*` 16 臂一一对应）。
   - 轴 2：ref `WireResponse` union 逐个 `Schema.Literal` tag（15 个）↔ rust `WireResponse` 16 变体；ref `StreamEvent`（output/resized/exited/controller_changed/title_changed/foreground_process_changed）核对 rust 覆盖路径（JSON tag 或二进制帧）。
   - 轴 3：ref schema/pty.ts 4 tag ↔ rust PtyEvent 4 rename；协议常量（版本 7、0x00 控制帧、64 KiB replay 分块）双侧 file:line 对拍。
2. **已知锚核对**：登记册 G-10 记载“persistent-pty 协议超集共用版本号 7”。本 census 显现 RUST_ONLY op `read`/`restore`（及 tag `replay_complete`、字段级 `chunked_replay`/`replay_*`），与 G-10 超集口径吻合；协议版本双侧同为 7（rust crates/oc-core/src/persistent_pty.rs:509 ↔ ref /opt/opencode-ref/packages/core/src/persistent-pty/daemon.ts:8），锚核对通过。
3. **提取规则（归一化/测试剥除）**：rust 侧先 `strip_tests` 再提取；枚举变体名按 snake_case 归一还原 wire tag；ref 侧仅扫非 test `*.ts`；`{{ op: … }}` 与 tag 同名多次出现取首现 file:line。非持久 pty 的 HTTP/WS 端点面由 HTTP-SURFACE census 覆盖，本通道只看 op/tag/常量。
4. **幂等性**：无时间戳、无随机序，全部 sorted；连跑两次输出逐字节一致（diff 于 /tmp/opencode/ 下验证）。
