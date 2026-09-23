# 绑定自己的域名

frp-sh 可将用户拥有的域名发布到当前设备的 loopback HTTP 服务。公网网关按已验证的 `Host` 查找在线发布者，并通过设备主动建立的 WebSocket 隧道转发请求，不要求路由器开放端口。

```sh
frp-sh domain --server https://control.test.frp.sh:18443 bind app.example.com
# 在 DNS 中添加命令输出的 TXT，并将域名 CNAME 到输出的 cname_target
frp-sh domain --server https://control.test.frp.sh:18443 verify app.example.com
frp-sh domain --server https://control.test.frp.sh:18443 publish app.example.com --target 127.0.0.1:3000
```

公益入口的访问地址为 `https://app.example.com:18443`。TXT 验证值每次随机生成、15 分钟失效；验证成功后可以删除 TXT，但必须保留 CNAME。`status` 查询状态，`unbind` 立即撤销绑定。

本机目标只允许带端口的 HTTP loopback 地址。首版只转发普通 HTTP 请求，不支持公网 WebSocket、CONNECT、任意 TCP 或 UDP；请求正文上限 1 MiB、响应正文上限 4 MiB、请求超时 30 秒。公网 443 用于 ACME TLS-ALPN 签发，业务流量使用 18443。
