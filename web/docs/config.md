# 配置文件

frp-sh 使用 TOML 配置文件（通过 `--config` 指定）。不指定时使用内置默认值（指向 `127.0.0.1`，仅供本机联调）。

## 首次运行向导

直接运行 `frp-sh`（不带参数）或 `frp-sh config` 可打开**交互式配置向导**：

```text
  frp-sh 配置向导
  ==================
  1. 信令服务器地址 (HTTP) [默认 http://127.0.0.1:8080]
  > http://101.43.41.195:8080
  2. 中继服务器地址 (TCP) [默认 101.43.41.195:8081]
  >
  3. UDP 探测使用独立端口？(y/N)
  >
  4. 服务器密码（可选，服务器用 --password 启动时填写）
  > 
  5. STUN 服务器（可选，如 stun.cloudflare.com:3478）
  > 
```

- 地址可省略 `http://` 前缀（自动补全）
- 中继地址默认由信令地址推导（同主机 + 8081），直接回车即可
- 第 4 步密码与服务器 `--password` 一致；第 5 步 STUN 用于更精准的公网地址学习
- 保存后自动做一次连通性检查（`/health`）
- 配置文件保存到平台默认位置，之后所有命令自动加载：
  - Windows：`%APPDATA%\frp-sh\config.toml`
  - Linux/macOS：`~/.config/frp-sh/config.toml`
- 重新配置：`frp-sh config`；也可用 `--config <路径>` 指定其他配置文件

## 配置字段

| 字段 | 必填 | 默认值 | 说明 |
|------|------|--------|------|
| `signaling_addr` | 否 | `http://127.0.0.1:8080` | 信令服务器 HTTP 基地址 |
| `relay_addr` | 否 | `127.0.0.1:8081` | 中继 TCP 地址（打洞与 TURN 都失败后的私有 TCP 兜底） |
| `signaling_udp` | 否 | 与 HTTP 同端口 | UDP 公网探测地址（独立端口时设置） |
| `password` | 否 | 无 | 服务器密码（服务器 `serve --password` 时必填） |
| `stun_addr` | 否 | 无 | STUN 服务器（如 `stun.cloudflare.com:3478`）；公网地址学习优先走 STUN，失败回退自建 UDP 探测 |
| `turn_providers` | 否 | 空 | TURN 供应商列表（`turn://user:pass@host:port`），打洞失败时按 RTT 择优自动中继 |

> `uuid`（设备唯一 ID）不写在配置文件里，独立存放于 `%APPDATA%\frp-sh\identity`，用于派生稳定的虚拟 IP（`lan` 系列）与 UUID 键控的中继配对。

## 示例：连接远程服务器

```toml
# config/server.toml
signaling_addr = "http://101.43.41.195:8080"
relay_addr     = "101.43.41.195:8081"
```

使用：

```bash
frp-sh --config config/server.toml game create
frp-sh --config config/server.toml game join game-a3f9c2
```

## 示例：独立 UDP 探测端口

某些云防火墙要求 TCP 与 UDP 端口分开管理时使用：

```toml
signaling_addr = "http://101.43.41.195:8080"
relay_addr     = "101.43.41.195:8081"
signaling_udp  = "101.43.41.195:8082"   # UDP 探测走 8082
```

对应服务器侧需以 `--addr 0.0.0.0:8082` 单独提供 UDP 探测（或自行部署兼容端点）。

## 示例：TURN 供应商（打洞失败自动走 TURN）

服务器启用内置 TURN（`serve --turn 0.0.0.0:3478 --password 你的密码`）后，客户端配置：

```toml
signaling_addr = "http://101.43.41.195:8080"
relay_addr     = "101.43.41.195:8081"
password       = "你的密码"                          # 必须与服务器一致
turn_providers = ["turn://frp-sh:你的密码@101.43.41.195:3478"]
```

- 可配置**多个**供应商（内置 TURN / 自建 coturn / Cloudflare TURN 等），客户端并行测速，自动选 RTT 最快者
- 打洞失败（含 `--relay` 强制）时按顺序尝试：TURN 中继 → 私有 TCP 兜底
- 用户名固定为 `frp-sh`（内置 TURN 服务器约定）；自建 coturn 可自定义用户名密码

## 字段说明

### signaling_addr

REST API 基地址：

- `POST /room/create` — 注册房间
- `GET /room/{id}` — 查询房间
- `POST /room/{id}/join` — 访客登记
- `DELETE /room/{id}` — 关闭房间

### signaling_udp

UDP 公网探测端点：客户端发送 `ECHO <token>`，服务端回 `ADDR <token> <ip>:<port>`，客户端由此得知自己经 NAT 映射后的公网地址。

> 探测必须使用**与打洞相同的 socket**（同一本地端口），保证通告的地址就是打洞可用的映射。

### relay_addr

打洞失败时的私有 TCP 中继端点（配置了 `turn_providers` 时，TURN 优先于它）。连接后发送 `HELLO <room_id> <HOST|GUEST>` 完成配对。

### password

服务器密码。服务器以 `serve --password <口令>` 启动时，客户端必须配置相同密码，否则信令请求 401、中继被拒。

### stun_addr

可选 STUN 服务器（如 `stun.cloudflare.com:3478`）。公网地址学习（`learn_public_addr_auto`）**优先走 STUN**（RFC 5389 Binding），失败时回退到自建 UDP 探测（`ECHO`/`ADDR`）。服务器自身的 `--addr` UDP 探测仍可用作兜底。

### turn_providers

TURN 供应商列表，元素格式 `turn://[user:pass@]host:port`。客户端并行连接所有供应商做 Allocate 测速，选 RTT 最快者；打洞失败时经 TURN 交换 relay 地址建立数据面（`create_permission` + FRS1 over TURN），TURN 也不可用时回退 `relay_addr` 私有 TCP。

## 配置优先级

```text
命令行参数 > 配置文件 > 内置默认值
```

当前 CLI 中，`serve` 的监听地址仅由命令行参数控制；客户端地址由配置文件控制。
