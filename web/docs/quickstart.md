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
- **TCP 中继**（8081/tcp）：打洞失败时的数据转发通道（可再加 `--turn 0.0.0.0:3478` 提供标准 TURN UDP 中继，见[服务器部署](./server.md)）

> 记得在云厂商安全组和系统防火墙放行 `8080/tcp`、`8080/udp`、`8081/tcp`（启用 TURN 时还需 `3478/udp`）。

服务器端也可以直接用打包好的二进制，见[部署信令服务器](./server.md)的 systemd 方式。

## 第 2 步：配置客户端

房主和访客都需要一份指向该服务器的配置。**推荐用连接配置档案**（一行命令，自动写入本机配置）：

```bash
# 保存服务器连接（--set-default 设为默认档案；不含房间时仅"配置"，进房再补全）
frp-sh profile add --server http://你的服务器IP:8080 --password 你的口令 --set-default

# 之后进房间（首次或补全房间号；同服务器+模式自动合并到同一档案）
frp-sh profile add --server http://你的服务器IP:8080 --password 你的口令 --room 7411
frp-sh profile run          # 按默认档案一键启动会话
frp-sh profile list         # 查看已有档案（密码打码）
```

> 服务器面板首页底部有"一键配置客户端"卡片，复制整条命令发给朋友即可——
> 对方粘贴运行就完成安装后的全部配置；进房间的"一键接入"命令会自动补全同一份档案。

手工方式（TOML 配置文件，默认配置指向 `127.0.0.1`，仅本机联调可用）：

```toml
# config/server.toml
signaling_addr = "http://你的服务器IP:8080"   # HTTP 信令
relay_addr     = "你的服务器IP:8081"          # TCP 中继
```

通过 `--config` 指定：

```bash
frp-sh --config config/server.toml lan create
```

## 第 3 步：按场景选择命令

frp-sh 有三个使用系列，本教程以**组网（lan）**为例，这是最强大、最通用的模式：

| 系列 | 命令 | 场景 |
|------|------|------|
| **组网（推荐）** | `frp-sh lan create/join` | 整机互访 + 访问对方整个局域网（类 Tailscale） |
| **游戏** | `frp-sh game create/join` | Minecraft 等联机游戏，纯端口转发（默认 25565） |
| **开发** | `frp-sh dev create/join` | 开发调试，应用层端口转发 |

### 组网：房主创建房间（lan）

在房主电脑上：

```bash
frp-sh lan create
```

输出示例：

```text
  Room created : lan-a3f9c2
  Signaling    : http://你的服务器IP:8080
  Your ID      : 123e4567-e89b-12d3-a456-426614174000
  LAN addrs    : 192.168.1.5:51234
  Vnet IP      : 10.66.0.1（对端可 ping/直连此 IP）
  Mode         : LAN mesh (virtual NIC)
  LAN subnets  : 192.168.1.0/24（访客加入后可访问）
  Waiting for a guest to join ...
```

把 **`lan-a3f9c2`** 发给朋友。

### 组网：访客加入（lan）

在朋友机器上：

```bash
frp-sh lan join lan-a3f9c2
```

访客自动获得由设备 ID 派生的稳定虚拟 IP（如 `10.66.0.42`），加入后：

- 可 `ping 10.66.0.1` / SSH / 共享文件访问房主整机
- 自动添加路由，可访问房主局域网内的 NAS、打印机等其他设备
- 需要 root/管理员权限（创建虚拟网卡、加路由）

```text
  Joined room : lan-a3f9c2
  Host address: 你的服务器IP:xxx
  Host vnet IP: 10.66.0.1
  Host LAN     : 192.168.1.0/24
  Mode         : LAN mesh (virtual NIC)
  Punching through NAT ...

>>> 本地局域网直连 (LAN direct) with 192.168.1.5:51234   ← 同一 WiFi 秒连！
```

## 第 4 步：游戏 / 开发系列（纯端口转发）

不想组网、只需要转发一个端口时，用 `game`（游戏）或 `dev`（开发）：

```bash
# 房主：转发本机 Minecraft（默认 25565）
frp-sh game create --service 127.0.0.1:25565

# 访客：连接本机 25565 即到达房主游戏服务器
frp-sh game join game-a3f9c2 --listen 127.0.0.1:25565
```

```bash
# 开发：把本机 8080 的 Web 服务转发给同事
frp-sh dev create --service 127.0.0.1:8080
frp-sh dev join dev-a3f9c2 --listen 127.0.0.1:8080
```

`game` / `dev` 系列为纯端口转发，不创建虚拟网卡，无需管理员权限。

如果打洞失败，会自动回退（配置了 `turn_providers` 时优先走 TURN UDP 中继）：

```text
>>> UDP hole punching failed, falling back to relay ...
>>> relay connected, waiting for host ...
```

隧道依然可用，只是数据经过服务器转发（见[网络原理](./architecture.md)）。

断线后（网络抖动、NAT 映射过期）双方都会自动重连：

```text
>>> 连接已断开，2 秒后自动重连（Ctrl-C 退出）...
```

无需手动干预，重连间隔按 2s、4s、8s…退避，上限 15s。

## 最小可用命令

| 角色 | 最小命令 | 说明 |
|------|----------|------|
| 组网房主 | `frp-sh lan create` | 虚拟网卡整机入网（需 root/管理员） |
| 组网访客 | `frp-sh lan join lan-xxxxxx` | 只需房间号 |
| 游戏房主 | `frp-sh game create` | 纯端口转发（默认 25565） |
| 服务器 | `frp-sh serve` | 监听 0.0.0.0:8080/8081 |

## 下一步

- [命令参考](./cli.md) 查看全部参数
- [高级用法](./advanced.md) 加密、多连接等进阶配置
- [故障排查 FAQ](./faq.md) 遇到问题先看这里
