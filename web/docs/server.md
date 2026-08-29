# 部署信令服务器

信令服务器是整个系统的中心：注册房间、交换地址、兜底中继。它本身无状态、极轻量（空闲时内存 < 2 MB），一台 1 核 512 MB 的 VPS 即可。

## 快速启动

```bash
./frp-sh serve --addr 0.0.0.0:8080 --relay-addr 0.0.0.0:8081
```

| 参数 | 默认值 | 说明 |
|------|--------|------|
| `--addr` | `0.0.0.0:8080` | HTTP REST 监听地址（UDP 公网探测复用同一端口） |
| `--relay-addr` | `0.0.0.0:8081` | TCP 中继监听地址 |

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
- 中继流量未加密：需要机密性时，会话双方使用 `--key`（仅 P2P 直连生效，见[高级用法](./advanced.md)）
- 服务端口对公网开放属预期行为（房间号即访问凭证），如担心被扫描可在安全组仅放行可信来源 IP
