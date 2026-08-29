# 简介

**frp-sh** 是一个纯 Rust 实现的「社交化 P2P 打洞」隧道工具。它把「建立内网穿透」这件事简化成三步社交动作：

```text
frp-sh game create          # 房主：生成一个房间号
frp-sh game join game-xxxx  # 朋友：凭房间号加入
```

加入成功后，双方建立 **UDP 打洞直连**（P2P），把访客本地端口与房主本地服务桥接起来——适合局域网联机游戏、远程访问家里电脑等服务。当 NAT 过于严格、打洞失败时，流量自动回退到信令服务器中继，保证可用。

## 核心特性

| 特性 | 说明 |
|------|------|
| 房间制社交组网 | `game-xxxxxx` 房间号，朋友凭号加入，无需配置 IP |
| UDP 打洞 | STUN 简化版公网探测 + PUNCH/ACK 同时握手，可穿透受限锥形 NAT |
| 端口散布 | `--spread` 轻量端口预测，提高对称 NAT 命中率 |
| 端到端加密 | `--key <口令>` 双方一致即启用 ChaCha20-Poly1305 |
| 多连接复用 | 同一隧道会话顺序接受多个 TCP 连接 |
| 中继回退 | 打洞失败自动转服务器 TCP 中继，另有「迟到直连」复查自愈 |
| 单文件二进制 | 不依赖 frp / libp2p / webrtc，全部逻辑自包含 |

## 适用场景

- **局域网游戏联机**：房主开个房间号，朋友 `join` 即达，无需公网 IP、无需端口映射
- **远程访问家里电脑**：把家里的 SSH / 远程桌面等服务挂到隧道上
- **临时共享服务**：快速把本机任意 TCP 服务暴露给指定的人

## 不适用场景

- 需要匿名的公网访问（房间号即访问凭证，且访客需知道房间号）
- 需要加密之外的鉴权（`--key` 提供机密性，不提供身份认证）
- 严格的对称 NAT 且无中继（依赖中继回退保证可用性）

## 架构总览

```mermaid
sequenceDiagram
    participant H as 房主 (game create)
    participant S as 信令服务器
    participant G as 访客 (game join)
    H->>S: UDP 探测 → 得知自己公网地址
    H->>S: POST /room/create → 房间号
    G->>S: GET /room/{id} → 房主地址
    G->>S: UDP 探测 → 得知自己公网地址
    G->>S: POST /room/{id}/join → 登记访客地址
    par 同时打洞
        G->>H: PUNCH 数据报（连发，±spread 散布）
        H->>G: ACK 数据报
        H->>G: PUNCH 数据报
        G->>H: ACK 数据报
    end
    Note over G,H: 直连成功 → FRS1 可靠流 → CNEW 隧道帧协议
    alt 打洞超时
        G->>S: 中继 HELLO GUEST
        H->>S: 中继 HELLO HOST
        S-->>G,H: 配对成功，服务器双向拷贝
    end
```

## 项目信息

- 语言：Rust（2021 edition），Tokio 异步运行时
- 依赖：tokio / clap / axum / reqwest / serde / chacha20poly1305 / sha2 等
- License：MIT
