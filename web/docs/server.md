# 部署信令服务器
> 0.5.0：公网监听必须有非空密码。systemd 示例需先创建仅 root 可读的 `/etc/frp-sh/server.env`，设置 `FRPSH_SERVE_PASSWORD`。信令请使用 HTTPS。


服务器在内存中保存房间注册信息、交换地址并提供兜底中继，重启会清空房间。CPU、内存和公网带宽需求取决于活跃房间、轮询频率及中继流量，应通过压测确定，不保证固定空闲内存或承载人数。

## 快速启动

```bash
./frp-sh serve --addr 0.0.0.0:8080 --relay-addr 0.0.0.0:8081 --password "$FRPSH_SERVE_PASSWORD"
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
EnvironmentFile=/etc/frp-sh/server.env
ExecStart=/opt/frpsh/target/release/frp-sh serve --addr 0.0.0.0:8080 --relay-addr 0.0.0.0:8081 --password ${FRPSH_SERVE_PASSWORD}
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

## 日志与排障

使用 `frp-sh logs path` 查找当前账户的日志目录，`frp-sh logs tail --follow` 查看 JSONL 诊断记录。每个进程独立文件，单文件 5 MiB，保留三份轮转；清理时跳过存活进程。没有 HTTP 日志接口。服务部署应为运行账户设置固定 HOME。

## 与既有服务共存

服务器上的其他服务（如 8000/8765 等端口）不受影响；如需换端口，改 `--addr`/`--relay-addr` 并在客户端配置中对应修改即可。

## 多服务器（可选）

兼容实现必须遵循协议 v3 的认证、帧格式和资源限制，见[协议](./protocol.md)。可部署多个实例，让客户端分别配置对应服务器。

## 安全建议

- 信令 HTTP 未内置 TLS：公网部署建议置于 Nginx/Caddy 反向代理之后（HTTPS 终结）
- 中继传输加密使用服务器或房间凭据，不代表对中继运营者保密。服务房间可通过共同的 `--key` 增加端到端负载加密。
- 房间号是标识符，不是访问凭证。保护房间邀请和服务器凭据，按需限制监听端口的来源。

## 公益服务器容量限制（0.5.4 起）

在原有 `serve` 启动参数后追加，例如：

```bash
--max-rooms 20 --max-members 8 --max-total-members 100
```

| 参数 | 默认值 | 范围 / 含义 |
| --- | --- | --- |
| `--max-rooms` | 1024 | 1–1024 个未到期房间 |
| `--max-members` | 33 | 每房间 2–33 台登记设备，包含房主 |
| `--max-total-members` | 33792 | 全服 1–33792 个成员名额，包含所有房主 |

三个限制同时生效，由服务器强制校验，客户端不能提高上限。满额后创建或加入返回 HTTP 429；已登记设备使用相同 UUID 重连不增加计数，满员时仍可重连。同一设备加入不同房间分别占用名额。

这是登记容量，不是实时在线人数：暂时断线不释放名额，房间删除或过期后释放。修改 systemd 的 `ExecStart` 后执行 `systemctl daemon-reload` 和 `systemctl restart frp-sh` 生效；重启会清空内存房间，已有连接需要重新创建/加入。`/version` 的 `limits` 字段可核对当前设置。

容量限制不等于带宽限速或流量配额；中继带宽仍需单独管理。
