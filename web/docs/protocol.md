# 协议规范

本文档面向想要实现兼容客户端/服务器、或深入理解内部机制的开发者。所有协议均为简单文本/二进制格式，无加密（`--key` 加密在 FRS1 层，见下文）。

## 1. 信令 REST API

Base URL：`signaling_addr`（如 `http://host:8080`）。

### POST `/room/create`

请求：

```json
{ "prefix": "game", "ttl": 43200, "addr": "223.117.153.115:44276" }
```

| 字段 | 说明 |
|------|------|
| `prefix` | 房间前缀 |
| `ttl` | 有效秒数 |
| `addr` | 发起者经 UDP 探测得到的公网地址 |

响应 `200`：

```json
{ "room_id": "game-a3f9c2", "host_addr": "223.117.153.115:44276" }
```

### GET `/room/{id}`

响应 `200`：

```json
{
  "room_id": "game-a3f9c2",
  "host_addr": "223.117.153.115:44276",
  "guest_addr": "223.117.153.115:44282",
  "created_at": 1724900000,
  "expires_at": 1724943200
}
```

`guest_addr` 在访客加入前为 `null`。房间过期返回 `404`。

### POST `/room/{id}/join`

请求：

```json
{ "addr": "223.117.153.115:44282" }
```

响应 `200`：

```json
{ "room_id": "game-a3f9c2", "host_addr": "223.117.153.115:44276" }
```

### DELETE `/room/{id}`

`204` 成功，`404` 不存在。

### GET `/health`

返回 `ok`。

## 2. UDP 公网探测

客户端向探测端口（与 HTTP 同端口，或 `signaling_udp`）发送：

```text
ECHO <token>
```

服务端回显（源地址即客户端经 NAT 的公网映射）：

```text
ADDR <token> <ip>:<port>
```

`token` 由客户端生成（8 位 hex），用于关联响应。

## 3. 打洞协议

数据报均为 ASCII 文本：

```text
PUNCH <token>   # 打洞请求
ACK <token>     # 打洞确认（token 回显）
```

| 场景 | 行为 |
|------|------|
| 收到 `PUNCH <t>` | 回 `ACK <t>`（带重试），记录对端地址 → 直连 |
| 收到 `ACK <t>` 且 `t` 与本地 token 一致 | 直连成功 |
| 收到 FRS1 帧（magic `FRS1`） | 对端已进入数据阶段 → 直连成功 |

打洞目标 = 对端通告地址 ± spread 端口（排除自身端口）。窗口约 3 秒。

## 4. FRS1 可靠流协议

UDP 数据报，15 字节头 + 负载：

```text
偏移  长度  字段
0     4     magic = "FRS1"
4     1     flags: 0x01=DATA, 0x02=FIN, 0=纯ACK
5     4     seq (u32 BE)
9     4     ack (u32 BE)   — 已连续接收的最大序号+1
13    2     len (u16 BE)   — 负载长度
15    len   负载（DATA 帧；--key 时密文含 16B Poly1305 标签）
```

### 发送方状态机

- `next_seq` 从 1 递增；窗口 32 帧
- 数据帧入窗即发送，入窗帧存入重传队列
- 收到 `ack=N`：移除所有 `seq < N` 的帧；FIN 帧被确认（`ack > fin_seq`）则关闭完成
- 150ms 定时器：重传窗口内所有未确认帧
- 空闲 1s：发送纯 ACK 帧（keepalive，维持 NAT 映射）

### 接收方状态机

- `next_expected` 从 1 开始
- 收到 `seq == next_expected` 的数据帧：投递负载、`next_expected += 1`、回 ACK
- 乱序/重复帧：丢弃（go-back-N），仍回 ACK 提示发送方
- 收到 FIN：置 `rx_closed`，回 ACK
- 读缓冲满（1MB）：丢弃帧不推进序号（触发重传 = 流控）

### 加密

`--key` 启用时，DATA 帧负载 = `ChaCha20-Poly1305.encrypt(nonce=seq, plaintext)`，密文含 16 字节认证标签；单帧明文上限 1184 字节。ACK/FIN 帧不加密。

### 关闭

- `shutdown()`：发 FIN 帧（`seq = next_seq`，入重传队列），等待 `ack > fin_seq`
- 5 秒未确认：视为对端消失，**尽力关闭**（不报错）

### 错误处理

- Windows `WSAECONNRESET(10054)` / `WSAECONNREFUSED(10061)`：忽略（ICMP 毒化）
- 对端 socket 关闭：后续 recv 返回错误 → 会话结束

## 5. 隧道帧协议

FRS1 流（或中继 TCP 流）之上的字节流，用于本地 TCP 桥接与多连接复用：

```text
访客 → 房主: "CNEW"                          # 4 字节，新连接
访客 → 房主: [u32 len BE][payload]            # 数据帧，len ≤ 1 MiB
访客 → 房主: [u32 0]                          # 结束帧
房主 → 访客: [u32 len BE][payload]            # 数据帧
房主 → 访客: [u32 0]                          # 结束帧
```

- 房主等待 `CNEW` 时收到的非 `CNEW` 4 字节按「残余数据帧头」处理并跳过（关闭竞态保护）
- 结束帧对称确认：本地关闭发结束帧并等待对端；收到结束帧即回送（若未发送）并结束本连接

## 6. 中继协议

TCP 文本行协议（`\r\n` 结尾）：

```text
客户端 → 服务器: HELLO <room_id> <HOST|GUEST>
服务器 → 客户端: WAIT\r\n | OK\r\n | ERROR <原因>\r\n
```

| 响应 | 含义 |
|------|------|
| `WAIT` | 已入槽，等待对端 |
| `OK` | 对端已在等待，已配对 |
| `ERROR ROOM_EXPIRED` / `ERROR BAD_HELLO` / `ERROR BAD_ROLE` / `ERROR ALREADY_CONNECTED` / `ERROR NO_PEER` | 拒绝 |

配对成功后服务器对两端 socket 双向拷贝，客户端无感知。配对等待上限 10 分钟。

## 兼容性说明

- 打洞与中继阶段均为 ASCII/二进制直读，便于抓包调试
- 若需与 frp 等工具互通：frp-sh 协议为自研，**不兼容** frp 协议
