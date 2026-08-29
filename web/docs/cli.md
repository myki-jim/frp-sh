# 命令参考

所有命令均支持 `--help` 查看完整说明。全局参数放在子命令之前。

## 全局参数

| 参数 | 说明 |
|------|------|
| `-c, --config <FILE>` | 配置文件路径（TOML），见[配置文件](./config.md) |
| `-v, --verbose` | 输出调试日志（`RUST_LOG=debug` 级别，含帧级追踪） |

```bash
frp-sh --config config/server.toml --verbose game create
```

## `frp-sh serve` —— 启动信令服务器

```bash
frp-sh serve [--addr <地址>] [--relay-addr <地址>]
```

| 参数 | 默认值 | 说明 |
|------|--------|------|
| `--addr` | `0.0.0.0:8080` | HTTP REST + UDP 公网探测监听地址 |
| `--relay-addr` | `0.0.0.0:8081` | TCP 中继监听地址 |

Ctrl-C 优雅退出。

## `frp-sh game create` —— 房主创建房间

```bash
frp-sh game create [选项]
```

| 参数 | 默认值 | 说明 |
|------|--------|------|
| `-p, --prefix` | `game` | 房间前缀（房间号 = `前缀-6位hex`） |
| `-t, --ttl <秒>` | `43200` (12h) | 房间有效时长，过期后自动失效 |
| `--service <地址>` | `127.0.0.1:25565` | 房主本地服务地址，隧道打通后向其转发 |
| `--relay` | 关 | 跳过打洞，直接使用中继 |
| `--key <口令>` | 无 | 端到端加密口令（双方一致才可通信） |
| `--max-conns <N>` | `0` (无限) | 会话内最多接受的连接数 |
| `--spread <N>` | `2` | 打洞端口散布范围（±N 端口） |

示例：

```bash
# 最小命令
frp-sh game create

# 完整示例：加密 + 限 5 个连接 + 打洞散布 ±3
frp-sh game create --service 127.0.0.1:25565 --key mypass --max-conns 5 --spread 3
```

## `frp-sh game join <room_id>` —— 访客加入房间

```bash
frp-sh game join <房间号> [选项]
```

| 参数 | 默认值 | 说明 |
|------|--------|------|
| `room_id` | （必填） | 房间号，如 `game-a3f9c2`；格式校验 `前缀-6位hex` |
| `-r, --relay` | 关 | 强制中继模式（跳过打洞） |
| `--listen <地址>` | `127.0.0.1:25565` | 访客本地监听地址，玩家连接此端口 |
| `--key <口令>` | 无 | 与房主一致的加密口令 |
| `--max-conns <N>` | `0` (无限) | 会话内最多建立的连接数 |
| `--spread <N>` | `2` | 打洞端口散布范围 |

示例：

```bash
frp-sh game join game-a3f9c2
frp-sh game join game-a3f9c2 --listen 127.0.0.1:30000 --key mypass
frp-sh game join game-a3f9c2 --relay     # 强制走中继
```

## 会话输出含义

| 输出 | 含义 |
|------|------|
| `Room created : game-a3f9c2` | 房主房间创建成功 |
| `>>> P2P direct link established with <addr>` | **打洞成功**，P2P 直连 |
| `>>> UDP hole punching failed, falling back to relay ...` | 打洞失败，转入中继 |
| `>>> late P2P link established with <addr>` | 中继等待期间补抓到直连，已切回 P2P |
| `connection N from <addr>` | 访客侧：本地新连接进入隧道 |
| `guest connection N, dialing local service ...` | 房主侧：收到访客连接，拨号本地服务 |
| `connection N closed` | 一条隧道连接正常结束 |
| `max connections (N) reached, ending session` | 达到 `--max-conns`，会话结束 |
| `session ended by peer` | 对端关闭了会话 |

## 退出方式

- **房主**：`Ctrl-C` 结束会话并自动删除房间
- **访客**：`Ctrl-C` 结束会话；或在 `--max-conns` 用尽后自动结束

## 常见错误

| 错误 | 原因与解决 |
|------|-----------|
| `invalid room id: xxx` | 房间号格式不对（需 `前缀-6位hex`） |
| `room not found or expired: xxx` | 房间不存在/已过期；让房主重新 create |
| `UDP echo timed out` | 服务器 8080/udp 未放行，或配置地址不对 |
| `decryption failed (wrong --key?)` | 双方 `--key` 不一致 |
| `relay rejected: ERROR ROOM_EXPIRED` | 房间过期后中继被拒 |
| `cannot reach local service xxx` | 房主本地服务未启动或地址错误 |
