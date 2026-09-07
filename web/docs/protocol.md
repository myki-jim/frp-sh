# 协议 v3

本文是 0.5.4 的实现概览，不是可独立实现客户端的完整线协议规范。兼容开发还应对照版本标签下的源码：`src/signaling/mod.rs`、`src/signaling/server.rs`、`src/p2p/relay.rs`、`src/p2p/enc.rs`、`src/p2p/stream.rs` 和 `src/services.rs`。

## HTTP 信令

`GET /health` 返回 `ok`；`GET /version` 返回程序版本、协议号和认证状态，0.5.4 起还返回配置的 `limits`。这两个接口无需认证。

其他接口使用 `X-Frp-Sh-Token`，传入服务器密码或适用于该房间的 access token。房间 access token 不能授权创建房间。房主修改操作另外要求 `X-Frp-Sh-Room-Token`。

| 路由 | 用途 |
| --- | --- |
| `POST /room/create` | 注册房间，返回 `room_id`、`host_addr`、`owner_token` |
| `POST /room/{id}/join` | 登记或更新访客，返回分配 IP 和设备名称 |
| `GET /room/{id}` | 查询房间快照，包括访客列表和发布服务 |
| `POST /room/{id}/refresh` | 房主刷新地址和候选信息 |
| `POST /room/{id}/secure` | 房主创建或轮换房间访问凭据 |
| `POST /room/{id}/services` | 房主更新服务目录 |
| `DELETE /room/{id}` | 房主删除房间 |

创建请求包含 `prefix`、`ttl`、`addr`；网卡、候选地址、版本等可选字段以请求结构体为准。默认房间号为四位数字，前缀可选。`visitor_id` 是重连使用的稳定标识。房间访问凭据与房主凭据不同，均不应记录到日志。

错误包括：400 元数据无效、401 认证失败、403 房主权限不足、404 房间不存在或过期、409 地址冲突、429 容量已满。容量包含房主，相同登记身份重连复用名额。HTTP 本身不加密，部署应使用 HTTPS。

## UDP 探测与传输

公网探测使用 `ECHO <token>`、`ADDR <token> <ip>:<port>`；打洞交换 `PUNCH <token>`、`ACK <token>`。可靠 UDP 流使用 FRS1 帧，序号、确认、重传和边界规则见 `src/p2p/stream.rs`。服务多路复用采用 `src/services.rs` 的独立帧格式，不能照旧版 CNEW 顺序转发协议实现。

## TCP 中继

受保护房间先发送 `R3 <room_id>\n`，让服务端选择房间访问凭据，然后建立凭据派生的加密流。在该流内部发送：

```text
HELLO2 <room_id> <HOST|GUEST> <visitor_id> <owner_token-or-dash>\r\n
```

房主传 owner token，访客传 `-`；匹配的 visitor ID 标识配对槽位。`WAIT` 仅表示登记后等待对端，不表示已经连接对端；`OK` 表示配对。未配对槽位 15 秒后过期。认证或握手失败可能直接关闭连接，不应依赖旧文档的固定 `ERROR` 枚举。加密帧和关闭行为以中继客户端、服务端源码为准。

## TURN

实现 STUN/TURN 的 UDP 子集，包含分配与权限操作的认证。内置 TURN 只允许向本实例有效中继端点转发。TURN 认证不等于负载端到端加密。`--relay` 选择 TCP 并跳过 TURN。
