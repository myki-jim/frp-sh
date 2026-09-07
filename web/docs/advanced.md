# 高级用法

## 端到端加密

双方使用相同口令即启用 ChaCha20-Poly1305 加密（见[网络原理](./architecture.md#认证与加密)）：

```bash
# 房主
frp-sh game create --service 127.0.0.1:25565 --key "我们的暗号"

# 访客
frp-sh game join game-a3f9c2 --listen 127.0.0.1:25565 --key "我们的暗号"
```

- 口令不匹配时会话报 `decryption failed (wrong --key?)` 并退出
- 服务房间设置 `--key` 后，在 TCP 中继之上增加端到端加密流，两端口令必须一致。房间凭据提供的传输加密本身不代表对中继运营者保密。
- 口令通过命令行传入，注意 shell 历史记录；敏感环境可用 `--config` 之外的方式自行包装

## 并发服务

服务房间支持最多 16 个发布服务、32 位访客，每位访客最多 64 条并发流、房间总计 256 条。使用 `frp-sh dev create --tcp 3000 --tcp 8080` 和 `frp-sh dev join 1234`。`--max-conns` 属于旧式转发选项，不是服务房间并发配额。LAN 房间传输虚拟网络流量，不是单条顺序 TCP 连接。

## 强制中继

```bash
frp-sh game create --relay
frp-sh game join game-xxxx --relay
```

- 跳过 UDP 打洞和 TURN，强制使用 TCP 中继。
- 适合：已知双方 NAT 无法打洞、需要服务器记录流量、快速验证链路

## 配置 TURN 供应商

打洞失败时默认走私有 TCP 中继；配置 `turn_providers` 后可优先走 **TURN 中继**（UDP，
标准 RFC 5766，可与 coturn/Cloudflare TURN 等互操作）。在 `config.toml` 中：

```toml
# 单个供应商（内置 TURN 服务器：serve --turn 0.0.0.0:3478）
turn_providers = ["turn://frp-sh:你的密码@服务器IP:3478"]

# 多个供应商：并行测速，自动选 RTT 最快者
turn_providers = [
  "turn://frp-sh:你的密码@101.43.41.195:3478",   # 自己的 VPS
  "turn://user:pass@turn.example.com:3478",       # 第三方 TURN
]
```

- 普通打洞失败时可尝试 TURN，再回退 TCP；`--relay` 跳过 UDP 与 TURN，直接使用 TCP
- 无需为每个房间配置，属于全局配置；`frp-sh config` 向导暂不询问 TURN，需手改配置
- 内置 TURN 的部署方式见[服务器部署](./server.md#内置-turn-中继可选)

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
