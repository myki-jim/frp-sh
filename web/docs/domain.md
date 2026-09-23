# 绑定自己的域名

frp-sh 可以把你拥有的域名发布到当前设备上的 HTTP 服务。公网请求先到 frp-sh 网关，再通过设备主动建立的加密 WebSocket 隧道转发到 `127.0.0.1`；不需要在路由器上开放端口。

当前公益入口使用 HTTPS `18443`。例如绑定 `app.example.com` 后，访问地址是：

```text
https://app.example.com:18443
```

## 1. 创建绑定

```sh
frp-sh domain --server https://control.test.frp.sh:18443 bind app.example.com
```

命令会输出一次性的 `txt_name`、`txt_value`、`cname_target` 和 `https_port`。在域名 DNS 控制台添加输出中的两条记录：

```text
<txt_name>       TXT    <txt_value>
app.example.com  CNAME  edge.test.frp.sh
```

不要照抄示例验证值；每次 `bind` 都会生成新名称和值，并在 15 分钟后失效。根域名不能设置普通 CNAME 时，使用 DNS 提供商的 CNAME Flattening、ALIAS 或 ANAME；也可以绑定一个子域名。

## 2. 验证域名

等待 TXT 在公共 DNS 生效后运行：

```sh
frp-sh domain --server https://control.test.frp.sh:18443 verify app.example.com
frp-sh domain --server https://control.test.frp.sh:18443 status app.example.com
```

看到 `"verified": true` 后可以删除 TXT，CNAME 需要一直保留。验证绑定到本机设备密钥；换设备操作需要迁移同一个私钥，或先在原设备解绑。

## 3. 发布本机服务

假设页面运行在 `127.0.0.1:3000`：

```sh
frp-sh domain --server https://control.test.frp.sh:18443 publish app.example.com --target 127.0.0.1:3000
```

这个命令保持前台运行。需要开机自动发布时，用系统服务或现有 agent 监督该命令；进程退出后公网入口会返回 `502 publisher_offline`，域名绑定仍保留。

解绑会立即关闭在线发布者并停止证书准入：

```sh
frp-sh domain --server https://control.test.frp.sh:18443 unbind app.example.com
```

## 安全边界与限制

- 只接受普通 ASCII/punycode 域名，不接受 IP、通配符和内部保留名称。
- 本机目标必须是带端口的 `http://127.0.0.1`、`http://localhost` 或 `http://[::1]`，不能把网关变成任意内网代理。
- 首版转发普通 HTTP 请求；不支持公网 WebSocket、任意 TCP、CONNECT 或 UDP。
- 单请求正文上限 1 MiB，单响应正文上限 4 MiB，请求总等待时间 30 秒。
- 网关重写 `Host`、`X-Forwarded-Host` 和 `X-Forwarded-Proto`，并丢弃客户端提交的转发头和 hop-by-hop 头。
- Caddy 只为服务端 `allow` 接口确认过的域名签发证书。公网 `443` 必须可达，用于 ACME TLS-ALPN 验证；业务访问使用 `18443`。
- 当前 `80` 在部分中国大陆网络会被备案拦截。不要依赖 HTTP 跳转，直接分享带 `:18443` 的 HTTPS 地址。

自建网关和服务端参数见[部署信令服务器](./server#自定义域名入口)。
