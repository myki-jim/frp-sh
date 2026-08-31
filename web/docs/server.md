# 部署信令服务器

信令服务器是整个系统的中心：注册房间、交换地址、兜底中继。它本身无状态、极轻量（空闲时内存 < 2 MB），一台 1 核 512 MB 的 VPS 即可。

## 快速启动

```bash
./frp-sh serve --addr 0.0.0.0:8080 --relay-addr 0.0.0.0:8081
```

| 参数 | 默认值 | 说明 |
|------|--------|------|
| `--addr` | `0.0.0.0:8080` | HTTP REST 监听地址（UDP 公网探测复用同一端口） |
| `--relay-addr` | `0.0.0.0:8081` | 私有 TCP 中继监听地址（打洞/TURN 都失败后的兜底） |
| `--password` | 无 | 服务器密码（信令 401 校验 + 中继认证加密） |
| `--turn` | 关闭 | 内置 TURN 监听地址（如 `0.0.0.0:3478`，RFC 5766 UDP 子集） |
| `--external-ip` | 监听地址 IP | 对外公网 IP（NAT/Docker 后部署 TURN 时必填） |

## systemd 常驻服务（推荐）

创建 `/etc/systemd/system/frp-sh.service`：

```ini
[Unit]
Description=frp-sh signaling server
After=network.target

[Service]
ExecStart=/opt/frpsh/target/release/frp-sh serve --addr 0.0.0.0:8080 --relay-addr 0.0.0.0:8081
Restart=always
RestartSec=3

[Install]
WantedBy=multi-user.target
```

启用并启动：

```bash
systemctl daemon-reload
systemctl enable frp-sh
systemctl start frp-sh
systemctl status frp-sh
```

验证监听：

```bash
ss -lntup | grep -E ':8080|:8081'
# 应看到 8080/tcp、8080/udp、8081/tcp 三个监听
```

## 防火墙放行

需要放行三个端口：

| 端口 | 协议 | 用途 |
|------|------|------|
| 8080 | TCP | HTTP REST |
| 8080 | UDP | 公网地址探测 |
| 8081 | TCP | 中继转发 |

```bash
# ufw
ufw allow 8080/tcp
ufw allow 8080/udp
ufw allow 8081/tcp

# 或 iptables
iptables -I INPUT -p tcp --dport 8080 -j ACCEPT
iptables -I INPUT -p udp --dport 8080 -j ACCEPT
iptables -I INPUT -p tcp --dport 8081 -j ACCEPT
```

> 云厂商（腾讯云/阿里云等）的**安全组**也需要放行，且必须包含 UDP 8080——漏掉 UDP 会导致客户端公网探测失败，表现为 `UDP echo timed out`。

## 内置 TURN 中继（可选）

打洞失败时，默认走服务器私有 TCP 中继。也可以启用**内置 TURN 服务器**（RFC 5766），
让客户端经 UDP 中继而不是 TCP 中继，穿透性更好、与标准 STUN/TURN 生态兼容：

```bash
./frp-sh serve --addr 0.0.0.0:8080 --relay-addr 0.0.0.0:8081 \
  --password 你的密码 --turn 0.0.0.0:3478 --external-ip 你的公网IP
```

| 说明 | 值 |
|------|-----|
| TURN 用户名 | `frp-sh`（固定） |
| TURN 密码 | 与 `--password` 一致 |
| Realm | `frp.sh` |
| 客户端配置 | `turn_providers = ["turn://frp-sh:你的密码@服务器IP:3478"]` |

> **为什么需要 `--external-ip`**：TURN 服务器通告的 relay 地址取自监听地址的 IP；
> 在 NAT / Docker 后面（如监听 `0.0.0.0` 时自动取到 `172.17.0.x` 这类内网地址），
> 必须显式指定公网 IP，客户端才能连到 relay 端口。

**防火墙/安全组**：TURN 需要额外放行 `3478/udp`（STUN/TURN 控制端口），
以及分配给客户端的 **relay 端口段**。relay 端口是每个客户端随机分配的（高位端口），
建议放行一段 UDP（如 `50000-50100/udp`）或直接放行全部 UDP 入站。

```bash
ufw allow 3478/udp
ufw allow 50000:50100/udp
```

systemd 常驻示例（含 TURN）：

```ini
ExecStart=/opt/frpsh/target/release/frp-sh serve --addr 0.0.0.0:8080 --relay-addr 0.0.0.0:8081 --password 你的密码 --turn 0.0.0.0:3478 --external-ip 你的公网IP
```

客户端验证（配置了 `turn_providers` 后）：

```text
>>> UDP hole punching failed, trying TURN relay     # 打洞失败自动切 TURN
```

也可以用任意标准 TURN 客户端（coturn `turnutils_uclient` 等）直接连通性测试。

## 从本机验证服务可用

```bash
# HTTP
curl http://服务器IP:8080/health
# → ok

# UDP 探测（Linux 下可用 nc 或任意 UDP 客户端）
# 期望返回 ADDR <token> <你的公网IP>:<端口>
```

## 与既有服务共存

服务器上的其他服务（如 8000/8765 等端口）不受影响；如需换端口，改 `--addr`/`--relay-addr` 并在客户端配置中对应修改即可。

## 多服务器（可选）

frp-sh 的信令协议是纯 HTTP + 简单 UDP/TCP 文本协议，可自行实现兼容服务器（约 200 行），或直接部署多个实例，每个实例的客户端配置指向各自服务器。

## 安全建议

- 信令 HTTP 未内置 TLS：公网部署建议置于 Nginx/Caddy 反向代理之后（HTTPS 终结）
- 私有 TCP 中继在设置 `--password` 后会用密码派生密钥做 ChaCha20-Poly1305 流加密（见[配置文件](./config.md)）；端到端机密性需会话双方 `--key`（作用于 UDP 数据面：直连与 TURN 路径，见[高级用法](./advanced.md)）
- 服务端口对公网开放属预期行为（房间号即访问凭证），如担心被扫描可在安全组仅放行可信来源 IP
