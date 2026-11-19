# MECHANISM-FIDELITY — 机制保真度台账

用户裁定（2026-11-19④）：**输出对拍过关 ≠ 机制移植到位**。滚轮（鼠标所有权倒置）、选区（clamp 近似 vs ref 端点扩展模型）证明存在"自建近似替代 ref 机制"的整类缺口——对拍沙箱难暴露（输出可收敛一致），真机交互暴露。本台账系统性清偿：逐子系统比对**机制**（谁拥有状态 / 什么驱动调度 / 事件流路径 / 状态机转移 / 生命周期），不只比对输出。

## 分类法

| 级 | 定义 |
|---|---|
| M0 | 机制移植到位：驱动方/所有权/时序语义与 ref 对齐（双 file:line 锚） |
| M1 | 行为近似：输出近似但机制自建替代——高伪装风险（重点猎物） |
| M2 | 部分移植：ref 机制在但缺组件/分支 |
| M3 | 未移植：ref 机制整体缺席 |
| X | 刻意分歧：有登记理由的偏离（须引登记条目） |

## 规则

- 每条须双锚（ref file:line + rust file:line）+ 证据；M3 记 ref 锚 + rust 侧模块枚举缺席证据。
- M1/M3 → 开 G 条目进波排期；修复后升 M0 并留锚。SLA 同在册 🟡（两波内不修须显式说明）。
- 波次覆盖表：每波审过的子系统族在此登记；**未审族 = 未覆盖，不得静默**（对齐 COVERAGE.md 纪律）。
- 判定禁 golden 自证：机制结论必须源码级对比（GAPS 2.2#6）。

## 波次状态

| 波 | 族 | 状态 |
|---|---|---|
| MF-1 | 渲染管线 / 时序调度 / 状态同步 / 插件桥机制 / 终端控制 / 文本布局 / 配置装载 / 键盘 / 剪贴板 / 进程拓扑 | **完成**（2026-11-19，oracle 只读审计；报告 .superpowers/sdd/mf1/report.md） |
| MF-1b（可选） | components/* 对话框组件级机制、prompt/*、theme/* | 未开——组件级非机制族，控制者视需排 |
| MF-2 | 鼠标滚轮 / 选区 / spinner 动画 / omo 侧栏渲染 | 待 G-457 合并后复审（活动面，禁双写） |

## 条目

MF-1 主表见报告；此处维护跨波索引（新发现 → G 条目 → 修复闭环）。

| # | 族 | ref 锚 | rust 锚 | 级 | 证据 / 行动 |
|---|---|---|---|---|---|
| S-1 | spinner 动画 | session-tabs.tsx:197 `<spinner>` 元素渲染器自调度 | session_tabs.rs:48-50 重绘时刻派生 | **M0（G-457 修）** | 2026-11-19 f7dbb92a：busy 期间 clock 钉 spinner 80ms 帧边界（marquee pin 同构）；trace 落精确 80ms 网格 |
| S-2 | 鼠标派发 | opentui 按帧原生派发鼠标（非每主循环 pass 一报） | app.rs 每事件一次全量 pump pass（~9ms/报）→?1003 motion 洪峰无界积压 | **M0（G-457 修）** | 2026-11-19 f8be1e5c：select 唤醒后批量 drain（保序、每 pass 一次 render）；滚轮生效 1767ms→26ms。注：原"所有权倒置"假设证伪（使能字节全程存活） |
| S-3 | 选区模型 | selection.ts（cell/word/line + 端点扩展 + 真文本提取）+ text-buffer-view.zig:871-891 expandSelectionRange + renderer nextClickBehavior | clamp 近似 | **M0（G-457 修）** | 2026-11-19 083ac2d2：SelectionBehavior 全套移植（500ms 多击循环、word 跨行端点联合、line 整行、retained 可复制）；clamp 未触碰 |
| S-4 | SSE 空闲判定 | connection.ts 每读复位（注释帧也算活性） | sse.rs `next_json_alive` + data.rs 换接 | M0 | 2026-11-19 断连热修（45s 看门狗盲心跳→周期误杀） |
| S-5 | spinner 字形 | opentui-spinner 0.0.7 renderSelf 仅 fg 色 drawChar（永不加粗） | live `<spinner>` 面误带 fallback 文本路径 BOLD | **M0（G-457 修）** | 2026-11-19 d6570c42：live 面 never-bold（animations 门），静态 fallback 保留 bold pins；"字形大一点"=braille 加粗 |
| MF-1 #7 | 状态同步：重连退避 | connection.ts:33,181 固定 1s；:158 稳定≥1s 复位 attempt | data.rs:309-329/1158 指数 500ms→8s；attempt 不复位 | M1 | **G-458 立**——S4 同类残口；断连体验族（长断后 8s vs ref 1s） |
| MF-1 #19 | 键盘：孤立 ESC 判定 | opentui complete_unit_len 超时窗 | input_term.rs:17-20/311-313 读块边界立即判（注释锚退役 crossterm=锚错树） | M1 | **G-459 立**——CSI 跨读分裂先发假 Escape 再吞残字节 |
| MF-1 #3 | 渲染管线：退出标题 | renderer.ts:3-7 destroy setTerminalTitle("") | teardown（app.rs:8217/9799）无标题复位 | M2 | **G-460 立**——退出后终端标题残留 |
| MF-1 #10 | 状态同步：强制 resync | connection.ts:212-217 resync(reason) | 无（仅 watchdog/错误路径） | X/N-A | 已裁：ref TUI 无终端侧触发点，双侧同靠看门狗 |
| MF-1 #12 | 插件桥：内容热重载 | watch.ts modify 事件 + context.tsx:274 仅重启变更件 | lib.rs:658-663 digest 扫描 reload_changed；增/删独立路径 | M0 | 已裁：digest 扫描覆盖 modify 语义 |
| MF-1 #2 | 渲染管线：簇发射 | opentui grapheme 延迟 pass | opentui_emit.rs:32-38 inline 打印 | M1(登记) | 维持登记；真机含簇帧分帧 bytes 不同 |
| MF-1 #15 | 终端控制：输入读模型 | opentui 阻塞读 | app.rs:8975-8985 50ms poll tick | M1(登记) | 上界 50ms 通常不可感；备查 |
| MF-1 #16 | 文本布局：折行 | UAX-29/UAX-14 全表 | wrap.rs 语料钉定近似 | M1(登记) | 语料外脚本未锚定；扩 corpus 备查 |
| MF-1 #18 | 配置：热载 watch | fs.watch 目录 | mtime 轮询 ≤100ms | M2(登记) | 可接受；零延迟需求时换 notify |
