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
```

- 地址可省略 `http://` 前缀（自动补全）
- 中继地址默认由信令地址推导（同主机 + 8081），直接回车即可
- 保存后自动做一次连通性检查（`/health`）
- 配置文件保存到平台默认位置，之后所有命令自动加载：
  - Windows：`%APPDATA%\frp-sh\config.toml`
  - Linux/macOS：`~/.config/frp-sh/config.toml`
- 重新配置：`frp-sh config`；也可用 `--config <路径>` 指定其他配置文件

## 配置字段

| 字段 | 必填 | 默认值 | 说明 |
|------|------|--------|------|
| `signaling_addr` | 否 | `http://127.0.0.1:8080` | 信令服务器 HTTP 基地址 |
| `relay_addr` | 否 | `127.0.0.1:8081` | 中继 TCP 地址 |
| `signaling_udp` | 否 | 与 HTTP 同端口 | UDP 公网探测地址（独立端口时设置） |

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

打洞失败时的 TCP 中继端点。连接后发送 `HELLO <room_id> <HOST|GUEST>` 完成配对。

## 配置优先级

```text
命令行参数 > 配置文件 > 内置默认值
```

当前 CLI 中，`serve` 的监听地址仅由命令行参数控制；客户端地址由配置文件控制。
