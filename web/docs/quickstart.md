# 快速开始

从拿到二进制到打通第一条隧道，大约 5 分钟。全程只需要三样东西：

1. 一台有公网 IP 的服务器（信令服务器，租的 VPS 即可）
2. 房主电脑（跑游戏/服务的那台）
3. 访客电脑（朋友那台）

## 第 1 步：部署信令服务器

在公网服务器上执行：

```bash
# 构建（首次较慢）
cargo build --release

# 启动信令服务器
./target/release/frp-sh serve --addr 0.0.0.0:8080 --relay-addr 0.0.0.0:8081
```

启动后三个服务同时就绪：

- **HTTP REST**（8080/tcp）：房间注册与查询
- **UDP 公网探测**（8080/udp）：客户端学习自己的公网地址
- **TCP 中继**（8081/tcp）：打洞失败时的数据转发通道

> 记得在云厂商安全组和系统防火墙放行 `8080/tcp`、`8080/udp`、`8081/tcp`。

服务器端也可以直接用打包好的二进制，见[部署信令服务器](./server.md)的 systemd 方式。

## 第 2 步：配置客户端

房主和访客都需要一份指向该服务器的配置（默认配置指向 `127.0.0.1`，仅本机联调可用）：

```toml
# config/server.toml
signaling_addr = "http://你的服务器IP:8080"   # HTTP 信令
relay_addr     = "你的服务器IP:8081"          # TCP 中继
```

通过 `--config` 指定：

```bash
frp-sh --config config/server.toml game create
```

## 第 3 步：房主创建房间

在运行游戏/服务的那台机器上：

```bash
frp-sh game create --service 127.0.0.1:25565
```

其中 `--service` 是房主本地服务的地址（默认 `127.0.0.1:25565`）。输出示例：

```text
  Room created : game-a3f9c2
  Signaling    : http://你的服务器IP:8080
  Local service: 127.0.0.1:25565
  Waiting for a guest to join ...
```

把 **`game-a3f9c2`** 发给朋友。

## 第 4 步：访客加入

在朋友机器上：

```bash
frp-sh game join game-a3f9c2 --listen 127.0.0.1:25565
```

`--listen` 是访客本地要监听的端口（默认 `127.0.0.1:25565`）。输出示例：

```text
  Joined room : game-a3f9c2
  Host address: 你的服务器IP:xxx
  Local listen: 127.0.0.1:25565
  Punching through NAT ...

>>> P2P direct link established with 你的服务器IP:xxx   ← 打洞成功！
```

此时朋友连接自己电脑的 `127.0.0.1:25565`，流量即到达房主的 `127.0.0.1:25565`。

如果打洞失败，会自动回退：

```text
>>> UDP hole punching failed, falling back to relay ...
>>> relay connected, waiting for host ...
```

隧道依然可用，只是数据经过服务器转发（见[网络原理](./architecture.md)）。

## 最小可用命令

| 角色 | 最小命令 | 说明 |
|------|----------|------|
| 房主 | `frp-sh game create` | 所有参数均有默认值 |
| 访客 | `frp-sh game join game-xxxxxx` | 只需房间号 |
| 服务器 | `frp-sh serve` | 监听 0.0.0.0:8080/8081 |

## 下一步

- [命令参考](./cli.md) 查看全部参数
- [高级用法](./advanced.md) 加密、多连接等进阶配置
- [故障排查 FAQ](./faq.md) 遇到问题先看这里
