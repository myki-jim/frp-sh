# frp-sh Web Panel 设计（v0.3.0 提案）

> 原则：轻量、扁平、零构建。前端 = 单文件 HTML（内嵌进二进制，`include_str!`），MDUI 2
> Web Components 走 CDN；后端 = 现有 axum 直接挂路由 + WebSocket 推送；拓扑图 = 纯 SVG 手绘。
> 全程无 npm / 无打包器 / 无新重依赖。

---

## 1. 总体架构

```
┌─────────── 服务端（serve, :8080 同端口）───────────┐    ┌──── 客户端（:6793 仅本地默认）────┐
│ REST/WS(信令)  ──  axum Router                     │    │ frp-sh lan/dev/game 会话进程      │
│   ├ /panel            → HTML（MDUI 2 单页）        │    │   └ panel task (axum, :6793)      │
│   ├ /api/panel/summary→ JSON 汇总                 │    │      ├ GET /        → HTML        │
│   ├ /api/panel/rooms  → JSON 房间明细             │    │      ├ GET /api/info → 基础信息   │
│   └ WS /api/panel/ws  → 每 2s 推送全量            │    │      ├ GET /api/links→ 链路+RTT   │
│ 读取：server state (rooms/relay/turn/uptime)      │    │      └ WS /ws       → 每 1s 推送 │
└───────────────────────────────────────────────────┘    │ 读取：StreamStats 注册表          │
                                                          └───────────────────────────────────┘
```

- 服务端 **不新增端口**：panel 挂在现有 8080（与信令 REST 同端口）。
- 客户端新开 **127.0.0.1:6793**（默认仅本地；`--panel-addr 0.0.0.0:6793` 可暴露）。
- 会话退出 panel 随进程退出，无残留。

## 2. 数据采集（最小侵入）

### 2.1 StreamStats（新增 `src/stats.rs`）

```rust
pub struct StreamStats {
    pub kind: AtomicU8,          // 0=direct 1=turn 2=relay
    pub peer: Mutex<String>,     // 对端显示（uuid 或 addr）
    pub connected_at: AtomicI64, // unix 秒
    pub sent_frames: AtomicU64,  pub sent_bytes: AtomicU64,
    pub recv_frames: AtomicU64,  pub recv_bytes: AtomicU64,
    pub rtt_last: AtomicU32,     pub rtt_ewma: AtomicU32,   // 心跳 Ping→Ack 差
    pub reconnects: AtomicU32,
}
pub static REGISTRY: OnceLock<Mutex<HashMap<String /*key*/, Vec<Arc<StreamStats>>>>>
```

- 挂接点：`UdpStream` / `RelayStream` 的 send/recv 出入口 `fetch_add`；
  心跳 Ping 帧记录 `sent_at: HashMap<u32, Instant>`，收到对应 Ack 计算 RTT（EWMA α=0.3）。
- 会话层把每条链路的 `Arc<StreamStats>` 注册进 REGISTRY（key = 本端 uuid/会话 id），断链时移除。

### 2.2 服务端 state（零新结构，读现有 + 少量补充）

- `Room`/`GuestInfo` 已含：host 版本/地址/turn_relay、guest uuid/addr/版本/assigned_ip。
- 补充：`START_TIME: OnceLock<Instant>`（uptime）、`turn_allocs: AtomicUsize`（turn_server
  Allocation 创建/过期增减）、relay 配对活跃数（现有表 len）。

## 3. API

### 服务端（:8080，auth 开启时 panel 仍可读——不含敏感字段）

| 路由 | 说明 |
|---|---|
| `GET /panel` | 面板 HTML（MDUI 2 单页） |
| `GET /api/panel/summary` | `{version, protocol, uptime_s, rooms, guests, relay_links, turn_allocs, auth}` |
| `GET /api/panel/rooms` | 房间数组（明细见下） |
| `WS /api/panel/ws` | 每 2s 推 `{summary, rooms}` |

房间明细：
```json
{ "room_id":"6492", "created_at":..., "ttl_left_s":43100,
  "host":{"addr":"223.117.153.115:5663","version":"0.2.11","vnet_ip":"10.66.0.1","turn_relay":null},
  "guests":[{"uuid":"706b…","addr":"…","version":"0.2.11","assigned_ip":"10.66.0.183","turn_relay":"…"}],
  "relay_pairs":1 }
```

### 客户端（:6793）

| 路由 | 说明 |
|---|---|
| `GET /` | 面板 HTML |
| `GET /api/info` | `{version, mode, room, signaling, my_id, vnet_ip, tun, mtu, encryption, started_at, reconnects}` |
| `GET /api/links` | `[{peer, kind:"direct|turn|relay", rtt_last, rtt_ewma, sent_bps, recv_bps, frames, since}]` |
| `WS /ws` | 每 1s 推 `{info, links}` |

## 4. 前端 UI（MDUI 2，扁平化）

统一壳：`<mdui-top-app-bar>` + 内容卡片 + 底部状态栏；MDUI 自动深/浅色。
MDUI 2 = 纯 Web Components，`<script type="module" src="https://unpkg.com/mdui@2/mdui.js">`
一行引入，零构建。离线兜底：内嵌一份精简 CSS（无 CDN 时仍可读，只是样式降级）。

### 4.1 服务端面板（/panel）

```
┌─ frp-sh Panel ──────────────── [v0.2.11] [protocol v1] [auth on] ─┐
│ ┌Rooms┐ ┌Guests┐ ┌Relay links┐ ┌TURN allocs┐   ← 4 张统计卡      │
│ │ 12  │ │  23  │ │     8     │ │     4     │                     │
│ └─────┘ └──────┘ └───────────┘ └───────────┘                     │
│ ┌ Rooms 明细（mdui-list 可折叠行）──────────────────────────────┐ │
│ │ ▸ 6492  host 0.2.11  10.66.0.1   guests 3   relay 1  TTL 11.9h│ │
│ │   └ 拓扑 SVG：host 圆心 + guest 环绕，边着色                    │ │
│ └──────────────────────────────────────────────────────────────┘ │
└─ uptime 3d 4h · ws 2s · —— 底部状态栏                              ─┘
```

### 4.2 客户端面板（:6793）

```
┌─ frp-sh ── [HOST/GUEST chip] [room 6492] [加密 on] ───────────────┐
│ ┌ 基础信息 ──────────────┐ ┌ 链接（每对端一行）────────────────┐ │
│ │ ID / vnet IP / TUN     │ │ ● 706b…  DIRECT   RTT 7ms   ↑↓   │ │
│ │ 信令 / 模式 / MTU      │ │ ● 9f2a…  TURN    RTT 158ms  ↑↓   │ │
│ └────────────────────────┘ └───────────────────────────────────┘ │
│ ┌ 拓扑 SVG（自己居中，边=链路类型着色，宽=流量）───────────────┐ │
│ └──────────────────────────────────────────────────────────────┘ │
└─ ws 1s ───────────────────────────────────────────────────────────┘
```

拓扑边色：**direct=绿 / turn=橙 / relay=红**；线宽 ∝ 当前速率；节点显示 uuid 前 6 位 + vnet IP。

## 5. 实施清单（按序）

| # | 项 | 文件 |
|---|---|---|
| 1 | StreamStats + REGISTRY + RTT/流量埋点 | `src/stats.rs`、`p2p/stream.rs` |
| 2 | 客户端 panel 服务（info/links/ws + HTML） | `src/panel/client.rs`、`panel/assets.rs` |
| 3 | 会话挂接：启动 panel task + Stats 注册 | `commands.rs`（guest/host/dev/game 会话） |
| 4 | 服务端 summary/rooms/ws 路由 + 面板 HTML | `server.rs`、`panel/server.rs` |
| 5 | CLI：`--panel-addr`（客户端）；serve 无需新参数 | `cli.rs` |
| 6 | e2e：panel API 冒烟（summary/links JSON） | `tests/e2e.rs` |
| 7 | 发版 0.3.0 + 双端实测 | — |

工作量：核心 2 个文件 + 埋点 ~300 行 Rust、前端 2 × ~200 行 HTML/JS。

## 6. 明确不做（保持轻量）

- 不做历史曲线/时序库（面板只看当下，未来可加环形缓冲）
- 不做鉴权体系（服务端只读且不含敏感字段；客户端默认仅绑 127.0.0.1）
- 不引入前端框架/打包器/图表库
