# 命令参考

frp-sh 提供三个使用系列，按场景选择：

| 系列 | 命令 | 适用场景 | 特点 |
|------|------|----------|------|
| **组网** | `frp-sh lan` | 通用内网环境（类 Tailscale） | 虚拟网卡整机入网，双方互访整机与整个局域网 |
| **游戏** | `frp-sh game` | Minecraft 等联机游戏 | 纯端口转发，默认 25565，即开即用 |
| **开发** | `frp-sh dev` | 开发调试、任意 TCP 服务 | 应用层端口转发 |

- **lan 是最强、最通用的模式**：默认开启虚拟网卡，双方建立二层隧道后可互访整机
  （ping/SSH/共享文件），还能访问对方整个局域网（NAS、打印机等）。
- **game / dev 是纯端口转发**：把某个本地端口转发给对方，不组网、不开虚拟网卡。

本文档逐一介绍每个命令及其每个参数。所有命令均支持 `--help` 查看内置说明。
全局参数放在子命令之前，例如：

```bash
frp-sh --config config/server.toml --verbose lan create
```

---

## 全局参数

全局参数作用于整个 `frp-sh` 调用，写在子命令（`serve` / `game` / `dev` / `lan` / `config`）之前。

### `-c, --config <FILE>`

**作用**：指定配置文件路径（TOML 格式），覆盖默认配置。

- 不指定时按以下顺序查找：
  1. 命令行 `-c` 指定的路径
  2. 平台默认配置路径（Windows 为 `%APPDATA%\frp-sh\config.toml`，Linux/macOS 为 `~/.config/frp-sh/config.toml`）
  3. 内置默认值（信令 `http://127.0.0.1:8080`、中继 `127.0.0.1:8081`）
- 配置文件格式见[配置文件](./config.md)

**示例**：

```bash
frp-sh -c config/server.toml lan create
frp-sh --config /etc/frp-sh.toml serve
```

### `-v, --verbose`

**作用**：开启调试日志（`RUST_LOG=debug`），输出帧级追踪。

- 排查打洞、转发、加密问题时加上此参数，日志会更详细
- 不带此参数时默认只输出 `info` 级别日志

**示例**：

```bash
frp-sh --verbose lan join lan-a3f9c2
```

---

## `frp-sh serve` —— 启动信令服务器

在公网 VPS 上运行，提供房间注册、UDP 公网探测与 TCP 中继转发。

```bash
frp-sh serve [--addr <地址>] [--relay-addr <地址>] [--udp-addr <地址>] [--password <口令>]
```

`Ctrl-C` 优雅退出。

### `--addr <地址>`

**作用**：HTTP REST + UDP 公网探测的监听地址。

- **默认值**：`0.0.0.0:8080`
- REST 接口（房间创建/加入/查询）与 UDP 探测（客户端学习公网地址）共用此端口
- 云服务器需在安全组/防火墙放行**同端口的 TCP 与 UDP**（如 `8080/tcp` 与 `8080/udp`）

**示例**：

```bash
frp-sh serve --addr 0.0.0.0:9000          # 换成 9000 端口
frp-sh serve --addr 127.0.0.1:8080        # 仅本机可用（调试用）
```

### `--relay-addr <地址>`

**作用**：TCP 中继监听地址（打洞失败时转发流量的服务器）。

- **默认值**：`0.0.0.0:8081`
- 与 `--addr` 相互独立，可同机不同端口
- 防火墙需放行该端口（如 `8081/tcp`）

**示例**：

```bash
frp-sh serve --addr 0.0.0.0:8080 --relay-addr 0.0.0.0:9001
```

### `--udp-addr <地址>`

**作用**：独立的 UDP 公网探测监听地址（可选）。

- **默认值**：与 `--addr` 同端口（客户端配置里的 `signaling_udp` 需与此对应）
- 仅当云防火墙**无法在同端口同时放行 TCP 与 UDP** 时使用
- 使用后客户端须在配置中设置 `signaling_udp`（`frp-sh config` 向导第 3 步）

**示例**：

```bash
frp-sh serve --addr 0.0.0.0:8080 --udp-addr 0.0.0.0:8082
# 客户端 config: signaling_udp = "服务器IP:8082"
```

### `--password <口令>`

**作用**：服务器密码（可选）。设置后：

- **请求认证**：所有信令请求校验 `X-Frp-Sh-Token`，缺失/错误 → 401 拒绝
- **中继认证 + 加密**：中继连接需携带密码，通道用 ChaCha20-Poly1305 流加密
- 客户端必须在配置中设置同一 `password`（`frp-sh config` 向导第 4 步）
- 不设置时行为与旧版完全一致（兼容无密码客户端）

**示例**：

```bash
frp-sh serve --addr 0.0.0.0:8080 --relay-addr 0.0.0.0:8081 --password 你的密码
```

---

## `frp-sh lan create` —— 组网：房主创建房间

组网模式（类 Tailscale），默认开启虚拟网卡。在房主机器上执行，创建房间并等待访客加入。

**支持多访客全互联**：一个房间允许 1 个房主 + 任意多个访客同时在线。每个访客与房主建立直连隧道（打洞失败自动转中继），房主作为枢纽在访客之间转发流量——所有成员的虚拟 IP（10.66.0.x）彼此互通（如 `ping 10.66.0.2` 可直达任意访客的整机）。

```bash
frp-sh lan create [选项]
```

### `-p, --prefix <前缀>`

**作用**：房间号前缀，房间号形如 `<前缀>-6位hex`（如 `lan-a3f9c2`）。

- **默认值**：`lan`
- 仅保留小写字母、数字与 `-_`，最长 16 字符；为空时回退为 `lan`
- 自定义前缀便于识别房间用途

**示例**：

```bash
frp-sh lan create --prefix home
frp-sh lan create --prefix my-team
```

### `-t, --ttl <秒>`

**作用**：房间有效时长（秒），超时后房间自动失效，双方会话结束。

- **默认值**：`43200`（12 小时）
- 访客在房间过期后加入会得到 `room not found or expired`
- 长时间组网可调大，短时测试可调小

**示例**：

```bash
frp-sh lan create --ttl 3600        # 1 小时
frp-sh lan create --ttl 86400       # 24 小时
```

### `--relay`

**作用**：跳过 UDP 打洞，直接使用中继转发。

- **默认值**：关闭（自动打洞，失败才回退中继）
- 适用场景：NAT 过于严格、双方网络打洞必然失败、或希望流量固定经服务器
- 中继通道为明文转发；需要机密性时配合 `--key`

**示例**：

```bash
frp-sh lan create --relay
```

### `--key <口令>`

**作用**：端到端加密口令（ChaCha20-Poly1305），双方使用相同口令才能通信。

- **默认值**：无（不加密）
- 口令只提供机密性，不验证身份；口令泄露则任何人都能解密
- 双方 `--key` 不一致时报 `decryption failed (wrong --key?)`

**示例**：

```bash
frp-sh lan create --key "我们的暗号"
```

### `--spread <N>`

**作用**：打洞端口散布范围（对端端口 ±N 的相邻端口也一并打洞）。

- **默认值**：`2`
- 部分 NAT 对连续端口做连续映射，散布可提高打洞命中率
- 调大（如 `5`）提升命中率但增加数据报数量；对称 NAT 场景建议尝试

**示例**：

```bash
frp-sh lan create --spread 5
```

### `--ip <IP>`

**作用**：房主虚拟网卡 IP（组网网段默认 10.66.0.0/24）。

- **默认值**：`10.66.0.1`
- 须与访客的虚拟 IP 在同一网段
- 建议固定使用，方便朋友长期直连

**示例**：

```bash
frp-sh lan create --ip 10.66.0.1
frp-sh lan create --ip 10.66.0.10
```

### `--netmask <掩码>`

**作用**：虚拟网卡子网掩码。

- **默认值**：`255.255.255.0`（/24）
- 双方必须使用相同掩码，否则虚拟网段不一致无法互通
- 一般无需修改

**示例**：

```bash
frp-sh lan create --netmask 255.255.0.0
```

### `--mtu <N>`

**作用**：虚拟网卡 MTU（最大传输单元，字节）。

- **默认值**：`1400`
- 隧道有约 100 字节封装开销，默认 1400 可避免分片
- 网络波动/丢包明显时可适当调小（如 `1300`）

**示例**：

```bash
frp-sh lan create --mtu 1300
```

### `--guest-ips <IP1,IP2,...>`

**作用**：预留的访客虚拟 IP 池（房主分配制）。

- **默认值**：空（访客使用自己的 UUID 派生 IP，或 `--ip` 指定）
- 逗号分隔多个 IP，如 `10.66.0.2,10.66.0.3,10.66.0.4`
- 访客加入时若**未指定 `--ip`**，按加入顺序从池中分配；**同一设备（UUID）重连复用同一 IP**（地址稳定）
- 适合房主希望统一管理虚拟网段地址的团队场景

**示例**：

```bash
# 房主：预留 3 个访客地址
frp-sh lan create --guest-ips 10.66.0.2,10.66.0.3,10.66.0.4

# 访客加入（无需 --ip，自动分配，输出 Assigned IP）
frp-sh lan join lan-a3f9c2
```

### `--expose-lan`

**作用**：把**本机局域网接入隧道**（对端可访问你的局域网设备，如 NAS/打印机）。

- **默认值**：关闭。默认只互访虚拟网段（`10.66.0.0/24`），**不暴露**你的真实局域网
- 开启后：自动通告本机局域网子网（`LAN subnets`）、开启 IPv4 转发，对端自动添加
  经虚拟网卡的路由，即可访问你局域网内的设备
- 需要 root/管理员权限（改路由、开转发）

**示例**：

```bash
# 房主：把家里局域网（如 192.168.1.0/24）开放给访客
frp-sh lan create --expose-lan

# 访客加入后即可访问房主局域网（输出 Host LAN）
frp-sh lan join lan-a3f9c2
```

> 默认不暴露更安全；双方都要访问对方局域网时，各自加 `--expose-lan`。

### 组合示例

```bash
# 最小命令
frp-sh lan create

# 加密组网 + 自定义网段
frp-sh lan create --key mypass --ip 10.66.0.1 --netmask 255.255.255.0
```

房主创建后，会通告自己的局域网子网（`LAN subnets`），访客加入后可访问房主整个局域网。
详见[本地网络拓扑支持](#本地网络拓扑支持)。

---

## `frp-sh lan join <room_id>` —— 组网：访客加入房间

在朋友机器上执行，凭房间号加入房主的组网。

```bash
frp-sh lan join <房间号> [选项]
```

### `room_id`（位置参数，必填）

**作用**：房主创建房间后给出的房间号。

- 格式：`前缀-6位hex`（如 `lan-a3f9c2`），不满足格式报 `invalid room id`
- 房间不存在或已过期报 `room not found or expired`
- 大小写敏感，注意区分

**示例**：

```bash
frp-sh lan join lan-a3f9c2
frp-sh lan join home-3f9c2a
```

### `--relay`

**作用**：强制使用中继模式（跳过打洞）。

- **默认值**：关闭（自动打洞，失败回退中继）
- 房主若用了 `--relay` 创建，访客也应加 `--relay`（或让其自动回退）

**示例**：

```bash
frp-sh lan join lan-a3f9c2 --relay
```

### `--key <口令>`

**作用**：与房主一致的加密口令；不一致则无法通信。

- **默认值**：无
- 房主用了 `--key`，访客必须用相同口令
- 口令不一致时报 `decryption failed (wrong --key?)`

**示例**：

```bash
frp-sh lan join lan-a3f9c2 --key "我们的暗号"
```

### `--spread <N>`

**作用**：打洞端口散布范围（同房主侧含义）。

- **默认值**：`2`
- 通常与房主保持一致即可

**示例**：

```bash
frp-sh lan join lan-a3f9c2 --spread 5
```

### `--ip <IP>`

**作用**：访客虚拟网卡 IP。

- **默认值**：由你的设备 ID（UUID）**稳定派生**（如 `10.66.0.42`），同一台设备每次加入地址不变
- 手动指定时须与房主同一网段（如房主 `10.66.0.1`，访客可设 `10.66.0.2`）
- 自动派生地址已在同一网段，通常无需手动指定

**示例**：

```bash
frp-sh lan join lan-a3f9c2 --ip 10.66.0.2
```

### `--netmask <掩码>`

**作用**：虚拟网卡子网掩码。

- **默认值**：`255.255.255.0`（/24）
- 必须与房主一致

**示例**：

```bash
frp-sh lan join lan-a3f9c2 --netmask 255.255.0.0
```

### `--mtu <N>`

**作用**：虚拟网卡 MTU。

- **默认值**：`1400`
- 必须与房主一致，否则可能出现 MTU 不匹配导致大包无法传输

**示例**：

```bash
frp-sh lan join lan-a3f9c2 --mtu 1300
```

### 组合示例

```bash
frp-sh lan join lan-a3f9c2
frp-sh lan join lan-a3f9c2 --key mypass
frp-sh lan join lan-a3f9c2 --relay     # 强制走中继
```

访客加入后自动获得稳定虚拟 IP（`Vnet IP`），可 ping / 直连房主的整机；同时自动添加
路由以访问房主的整个局域网（见[本地网络拓扑支持](#本地网络拓扑支持)）。

---

## `frp-sh game create` —— 游戏：房主创建房间

游戏联机专用，纯端口转发（不组网）。在运行游戏的机器上执行。

```bash
frp-sh game create [选项]
```

### `-p, --prefix <前缀>`

**作用**：房间号前缀，房间号形如 `<前缀>-6位hex`（如 `game-a3f9c2`）。

- **默认值**：`game`
- 仅保留小写字母、数字与 `-_`，最长 16 字符；为空时回退为 `game`

**示例**：

```bash
frp-sh game create --prefix mc
```

### `-t, --ttl <秒>`

**作用**：房间有效时长（秒），超时后房间自动失效。

- **默认值**：`43200`（12 小时）

**示例**：

```bash
frp-sh game create --ttl 86400
```

### `--service <地址>`

**作用**：游戏服务器本地地址；隧道打通后，访客的连接会转发到该地址。

- **默认值**：`127.0.0.1:25565`（25565 是 Minecraft 的默认端口，可改成任意端口）
- 地址格式：`IP:端口`，IP 通常为本机 `127.0.0.1`
- 服务必须**已经在该端口监听**，否则会报 `cannot reach local service`

**示例**：

```bash
frp-sh game create --service 127.0.0.1:25565   # Minecraft
frp-sh game create --service 127.0.0.1:7777    # 其他游戏（如 Palworld）
```

### `--relay`

**作用**：跳过 UDP 打洞，直接使用中继转发。

- **默认值**：关闭（自动打洞，失败才回退中继）

**示例**：

```bash
frp-sh game create --relay
```

### `--key <口令>`

**作用**：端到端加密口令，双方使用相同口令才能通信。

- **默认值**：无（不加密）

**示例**：

```bash
frp-sh game create --key "我们的暗号"
```

### `--max-conns <N>`

**作用**：会话内最多接受的连接数；达到后本轮会话结束并自动重连等待下一轮。

- **默认值**：`0`（无限）
- 多连接按**顺序复用**同一条隧道（同一时刻一条连接）
- 适合严格限制连接数的共享场景

**示例**：

```bash
frp-sh game create --max-conns 5
```

### `--spread <N>`

**作用**：打洞端口散布范围。

- **默认值**：`2`
- 对称 NAT 场景建议调大（如 `5`）

**示例**：

```bash
frp-sh game create --spread 5
```

### 组合示例

```bash
# 最小命令
frp-sh game create

# 加密 + 限 5 个连接 + 打洞散布 ±3
frp-sh game create --service 127.0.0.1:25565 --key mypass --max-conns 5 --spread 3
```

---

## `frp-sh game join <room_id>` —— 游戏：访客加入房间

```bash
frp-sh game join <房间号> [选项]
```

### `room_id`（位置参数，必填）

**作用**：房主创建房间后给出的房间号。

- 格式：`前缀-6位hex`，大小写敏感

**示例**：

```bash
frp-sh game join game-a3f9c2
```

### `--relay`

**作用**：强制使用中继模式。

- **默认值**：关闭

**示例**：

```bash
frp-sh game join game-a3f9c2 --relay
```

### `--listen <地址>`

**作用**：访客本地监听地址；游戏客户端连接此地址，流量即到达房主的游戏服务器。

- **默认值**：`127.0.0.1:25565`（25565 是 Minecraft 默认端口，可改成任意端口）
- 端口被占用时换一个未被占用的端口，玩家连接新端口即可

**示例**：

```bash
frp-sh game join game-a3f9c2 --listen 127.0.0.1:25565
frp-sh game join game-a3f9c2 --listen 127.0.0.1:30000
```

### `--key <口令>`

**作用**：与房主一致的加密口令。

- **默认值**：无

**示例**：

```bash
frp-sh game join game-a3f9c2 --key "我们的暗号"
```

### `--max-conns <N>`

**作用**：会话内最多建立的连接数。

- **默认值**：`0`（无限）
- 与房主 `--max-conns` 相互独立，以先达到者为准

**示例**：

```bash
frp-sh game join game-a3f9c2 --max-conns 3
```

### `--spread <N>`

**作用**：打洞端口散布范围。

- **默认值**：`2`

**示例**：

```bash
frp-sh game join game-a3f9c2 --spread 5
```

### 组合示例

```bash
frp-sh game join game-a3f9c2
frp-sh game join game-a3f9c2 --listen 127.0.0.1:30000 --key mypass
frp-sh game join game-a3f9c2 --relay
```

---

## `frp-sh dev create` —— 开发：房主创建房间

开发调试专用，应用层端口转发（任意 TCP 服务），不组网。参数与 `game create` 完全一致，
仅默认前缀与默认端口不同。

```bash
frp-sh dev create [选项]
```

### `-p, --prefix <前缀>`

**作用**：房间号前缀。

- **默认值**：`dev`

### `-t, --ttl <秒>`

**作用**：房间有效时长（秒）。

- **默认值**：`43200`（12 小时）

### `--service <地址>`

**作用**：本机待转发服务的地址。

- **默认值**：`127.0.0.1:25565`（按需改成你的服务端口，如 `127.0.0.1:8080`）
- 服务必须已经在该端口监听

**示例**：

```bash
frp-sh dev create --service 127.0.0.1:8080     # 转发本机 Web 服务
frp-sh dev create --service 127.0.0.1:5432     # 转发数据库端口
```

### `--relay`

**作用**：跳过打洞，直接使用中继。

- **默认值**：关闭

### `--key <口令>`

**作用**：端到端加密口令。

- **默认值**：无

### `--max-conns <N>`

**作用**：会话内最多接受的连接数。

- **默认值**：`0`（无限）

### `--spread <N>`

**作用**：打洞端口散布范围。

- **默认值**：`2`

### 组合示例

```bash
# 把本机 8080 的 Web 服务转发给同事联调
frp-sh dev create --service 127.0.0.1:8080 --key devpass
```

---

## `frp-sh dev join <room_id>` —— 开发：访客加入房间

参数与 `game join` 完全一致，仅默认前缀不同（`dev`）。

```bash
frp-sh dev join <房间号> [选项]
```

### `room_id`（位置参数，必填）

**作用**：房主创建房间后给出的房间号。

### `--relay`

**作用**：强制使用中继模式。

- **默认值**：关闭

### `--listen <地址>`

**作用**：访客本地监听地址；程序/浏览器连接此地址即到达房主服务。

- **默认值**：`127.0.0.1:25565`（按需改成你的目标端口）

**示例**：

```bash
frp-sh dev join dev-a3f9c2 --listen 127.0.0.1:8080
```

### `--key <口令>`

**作用**：与房主一致的加密口令。

- **默认值**：无

### `--max-conns <N>`

**作用**：会话内最多建立的连接数。

- **默认值**：`0`（无限）

### `--spread <N>`

**作用**：打洞端口散布范围。

- **默认值**：`2`

### 组合示例

```bash
frp-sh dev join dev-a3f9c2 --listen 127.0.0.1:8080 --key devpass
```

---

## `frp-sh config` —— 交互式配置向导

首次运行或需要更换信令服务器时使用。

```bash
frp-sh config [--config <FILE>]
```

**作用**：交互式询问并保存信令服务器、中继地址等信息。

- 不传 `--config` 时保存到平台默认路径（见 `-c, --config` 章节）
- 每个问题直接回车使用默认值
- 首次运行 `frp-sh`（不带子命令）且无配置时也会自动进入向导

**示例**：

```bash
frp-sh config
frp-sh config --config /etc/frp-sh.toml
```

---

## `frp-sh`（无子命令）

直接运行 `frp-sh`：

- **未配置过**：自动进入配置向导
- **已配置**：显示当前配置摘要与常用命令提示

**示例**：

```bash
frp-sh
```

---

## 本地网络拓扑支持

**每台设备一个固定虚拟 IP（类 VLAN）**：组网默认使用 `10.66.0.0/24` 虚拟网段
（相当于一个 VLAN）。每台设备有三种方式获得地址，从优先到兜底：

1. **显式指定**：`--ip 10.66.0.5`（房主/访客都可）
2. **房主分配**：房主 `--guest-ips` 预留 IP 池，访客按加入顺序拿号、重连复用
3. **UUID 自动派生**：访客什么都不配时，由设备 ID 稳定派生（如 `10.66.0.42`），
   同一台设备永远拿到同一个 IP

**VLAN 划分（多网段）**：不同网段 = 不同 VLAN。通过 `--ip` 与 `--netmask` 组合
即可使用任意网段：

```bash
# 网段 A（默认）：10.66.0.0/24
frp-sh lan create --ip 10.66.0.1

# 网段 B：10.66.1.0/24（另一组人）
frp-sh lan create --prefix team-b --ip 10.66.1.1 --netmask 255.255.255.0

# 大网段：10.66.0.0/16
frp-sh lan create --ip 10.66.0.1 --netmask 255.255.0.0
```

> 注意：同一房间内所有成员同网段（天然互通）；要隔离就开不同房间、不同网段。

**同局域网自动直连**：房主创建房间时自动通告本机所有局域网地址，访客加入时向
「公网地址 + 局域网地址」同时打洞。双方在同一 WiFi/网线时秒级建立局域网直连
（输出 `本地局域网直连 (LAN direct)`），完全不经服务器；不在同一局域网时自动走
公网打洞，打洞失败再回退中继。三个系列（lan / game / dev）都自动生效。

**访客访问房主整个局域网（lan 系列，需房主 `--expose-lan`）**：默认**不暴露**
任何一方的真实局域网——隧道里只有虚拟网段。只有加了 `--expose-lan` 的一方才会
通告自己的局域网子网（如 `192.168.1.0/24`），对端自动为这些子网添加经虚拟网卡
的路由，即可直接访问其局域网内的设备（NAS、打印机、其他电脑）：

```bash
# 房主（需 root/管理员，自动开启 IPv4 转发）
frp-sh lan create --expose-lan
# → LAN subnets  : 192.168.1.0/24

# 访客（自动添加路由，无需额外参数）
frp-sh lan join lan-a3f9c2
# → 路由 192.168.1.0/24 → frp1 已添加
# 之后可 ping / 访问房主局域网内设备
```

双向互通：访客也想让房主访问自己的局域网时，访客 join 也加 `--expose-lan`。

注意事项：

- 需要 root/管理员权限（创建虚拟网卡、改路由、开转发）
- 若访客本机与房主在同一网段（如双方都是 192.168.1.0/24），该子网会被自动跳过
  以避免路由冲突（输出会提示 `跳过与本地同网段的房主子网`）
- 访问的是房主**当前所在**的局域网；房主换网络后重新 create 即可刷新
- game / dev 系列为纯端口转发，不提供访问对方局域网的能力

---

## 会话输出含义

| 输出 | 含义 |
|------|------|
| `Room created : lan-a3f9c2` | 房主房间创建成功 |
| `Your ID      : <uuid>` | 你的设备唯一 ID（存于 `%APPDATA%\frp-sh\identity`，用于派生稳定的虚拟 IP） |
| `LAN addrs    : 192.168.1.5:51234` | 房主局域网地址（同局域网访客将直连此地址） |
| `LAN subnets  : 192.168.1.0/24` | 房主局域网子网（lan 系列访客可访问整个局域网） |
| `Vnet IP      : 10.66.0.x` | 你的虚拟网卡 IP（lan 系列；朋友可长期用此 IP 直连你的整机） |
| `>>> 本地局域网直连 (LAN direct) with <addr>` | **同 WiFi/局域网直连成功**（不经服务器，延迟最低） |
| `>>> P2P direct link established with <addr>` | **公网打洞成功**，P2P 直连 |
| `>>> UDP hole punching failed, falling back to relay ...` | 打洞失败，转入中继 |
| `>>> late P2P link established with <addr>` | 中继等待期间补抓到直连，已切回 P2P |
| `connection N from <addr>` | 访客侧：本地新连接进入隧道 |
| `guest connection N, dialing local service ...` | 房主侧：收到访客连接，拨号本地服务 |
| `connection N closed` | 一条隧道连接正常结束 |
| `max connections (N) reached, ending session` | 达到 `--max-conns`，会话结束 |
| `session ended by peer` | 对端关闭了会话 |
| `>>> 连接已断开，N 秒后自动重连...` | 链路断开，退避等待后自动重连（2s, 4s, 8s... 上限 15s） |

---

## 退出方式

- **自动重连**：断线（网络抖动、NAT 映射过期、服务重启）后自动重连，无需手动操作
- **房主**：`Ctrl-C` 结束会话并自动删除房间；房间过期后自动结束
- **访客**：`Ctrl-C` 结束会话；房间被删除/过期后自动结束

---

## 常见错误

| 错误 | 原因与解决 |
|------|-----------|
| `invalid room id: xxx` | 房间号格式不对（需 `前缀-6位hex`） |
| `room not found or expired: xxx` | 房间不存在/已过期；让房主重新 create |
| `UDP echo timed out` | 服务器 8080/udp 未放行，或配置地址不对 |
| `decryption failed (wrong --key?)` | 双方 `--key` 不一致 |
| `relay rejected: ERROR ROOM_EXPIRED` | 房间过期后中继被拒 |
| `cannot reach local service xxx` | 房主本地服务未启动或地址错误 |
| `创建 TUN 设备失败` | 需要 root/管理员权限；Windows 需 `wintun.dll` 在可执行文件旁 |
