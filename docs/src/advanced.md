# 高级用法

## 端到端加密

双方使用相同口令即启用 ChaCha20-Poly1305 加密（见[网络原理](./architecture.md#加密可选)）：

```bash
# 房主
frp-sh game create --service 127.0.0.1:25565 --key "我们的暗号"

# 访客
frp-sh game join game-a3f9c2 --listen 127.0.0.1:25565 --key "我们的暗号"
```

- 口令不匹配时会话报 `decryption failed (wrong --key?)` 并退出
- 加密仅作用于 **P2P 直连**数据帧（中继通道为明文中转，见安全说明）
- 口令通过命令行传入，注意 shell 历史记录；敏感环境可用 `--config` 之外的方式自行包装

## 多连接复用

默认一个会话可接受**无限个顺序连接**（断线重连、多次连入均复用同一条隧道）：

```bash
# 限制为 3 个连接，用尽后会话自动结束
frp-sh game create --max-conns 3
frp-sh game join game-xxxx --max-conns 3
```

适合：游戏客户端断线重连、需要严格限制连接数的共享场景。

> 注意：当前为**顺序复用**（同一时刻一条连接），断线后可立即重连；并发多路复用见[路线图](./roadmap.md)。

## 强制中继

```bash
frp-sh game create --relay
frp-sh game join game-xxxx --relay
```

- 跳过打洞，直接走服务器中继
- 适合：已知双方 NAT 无法打洞、需要服务器记录流量、快速验证链路

## 打洞散布调优

```bash
# 扩大散布范围（对称 NAT 场景可试 ±3~5）
frp-sh game create --spread 5
frp-sh game join game-xxxx --spread 5
```

- 默认 `--spread 2`
- 散布越大命中率越高，但产生更多无效发包（已忽略 ICMP 噪音）
- 两端取值无需一致（各自独立散布）

## 独立 UDP 探测端口

云防火墙要求 TCP/UDP 分开时（见[配置文件](./config.md)）：

```toml
signaling_addr = "http://101.43.41.195:8080"
relay_addr     = "101.43.41.195:8081"
signaling_udp  = "101.43.41.195:8082"
```

## 代理环境（HTTP 信令走代理）

frp-sh 的 HTTP 信令客户端遵循标准代理环境变量：

```bash
# Windows PowerShell
$env:HTTP_PROXY = "http://127.0.0.1:7890"
$env:HTTPS_PROXY = "http://127.0.0.1:7890"

# Linux
export HTTP_PROXY=http://127.0.0.1:7890
export HTTPS_PROXY=http://127.0.0.1:7890
```

> 仅 HTTP 信令走代理；UDP 探测与打洞、中继 TCP 仍为直连。若 UDP 被网络阻断，请直接使用 `--relay` 模式。

## 调试

```bash
frp-sh --verbose game create
# 输出帧级日志：recv/send 帧类型、序号、ACK、重传等
```

帧日志示例：

```text
DEBUG frp_sh::p2p::stream] send to 127.0.0.1:49278: len=19
DEBUG frp_sh::p2p::stream] recv kind=Data seq=1 ack=1 len=4
DEBUG frp_sh::p2p::stream] recv kind=Ack seq=4 ack=3 len=0
```

## 房主服务常见配置

| 场景 | 房主命令 | 访客命令 |
|------|----------|----------|
| Minecraft | `create --service 127.0.0.1:25565` | `join <room> --listen 127.0.0.1:25565` |
| SSH | `create --service 127.0.0.1:22` | `join <room> --listen 127.0.0.1:2222` |
| 远程桌面 (RDP) | `create --service 127.0.0.1:3389` | `join <room> --listen 127.0.0.1:3389` |
| 任意 Web 服务 | `create --service 127.0.0.1:8080` | `join <room> --listen 127.0.0.1:8080` |
