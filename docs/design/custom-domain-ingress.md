# 自定义域名 HTTP Ingress

## 目标

用户将自己控制的域名绑定到显式发布的本机 HTTP 服务：

```text
app.example.com -> edge.test.frp.sh -> Caddy:18443 -> frp-sh ingress -> 在线发布设备的 127.0.0.1:3000
```

这不是 LAN 路由、任意 TCP 转发或出口代理。没有显式发布和已验证域名的设备不能被公网访问。

## 首版命令

```sh
frp-sh domain --server https://control.test.frp.sh:18443 bind app.example.com
frp-sh domain --server https://control.test.frp.sh:18443 verify app.example.com
frp-sh domain --server https://control.test.frp.sh:18443 status app.example.com
frp-sh domain --server https://control.test.frp.sh:18443 publish app.example.com --target 127.0.0.1:3000
frp-sh domain --server https://control.test.frp.sh:18443 unbind app.example.com
```

`dev create` 输出一条随机验证值。用户在 DNS 中设置：

```text
_frpsh-verify-<随机前缀>.app.example.com TXT frpsh-verify=<随机值>
app.example.com CNAME edge.test.frp.sh
```

验证通过多个公共 DoH 解析器查询 TXT；只接受域名的 ASCII/punycode 规范形式，拒绝 IP、通配符、控制字符和内部保留后缀。TXT 验证值只存 SHA-256 摘要，15 分钟后失效。每次绑定使用新的 TXT 名称，避免递归解析器的旧 NXDOMAIN 缓存阻断重试。

## 控制面

绑定记录由服务端 SQLite 保存：规范域名、设备公钥、发布服务 ID、验证令牌摘要、验证状态、创建/更新时间和撤销版本。所有更改均使用设备 Ed25519 挑战签名；域名文本、客户端时间和 IP 不能作为所有权凭据。

一个域名只能属于一个已验证设备。重复请求以 request_id 幂等处理。解绑立即删除映射并递增撤销版本；网关在短期缓存到期前重新校验版本。

## 数据面与 HTTPS

公网网关仅接收 HTTP/HTTPS 请求并以 `Host` 查找已验证绑定。它把请求封装在独立的 ingress stream 中，交给当前在线发布者；发布者仅可连接配置的 loopback HTTP 端点。限制包括：

- 每域名并发请求、请求头、请求体、响应体、空闲时间和总带宽上限。
- 禁止 CONNECT、升级协议、代理绝对 URI、内部地址回源和任意目标端口；首版不转发公网 WebSocket。
- 转发 `X-Forwarded-For`/`X-Forwarded-Proto` 时由网关重写，发布者输入不可信。
- 离线、撤销、超额或未验证时返回明确 404/502/429，绝不回退到其他用户服务。

TLS 在 Caddy 网关终止。用户访问 `https://<域名>:18443`；公网 443 只用于 ACME TLS-ALPN 验证，80 不作为签发依赖。Caddy 按需签发前必须调用 `/domains/v1/allow`，不能仅因收到任意 SNI 就申请证书。平台通配符证书与未知用户域名使用独立 TLS 策略，否则平台证书会抢先匹配未知 SNI。首版要求 CNAME 指向 `edge.test.frp.sh`；验证成功后可以删除 TXT，CNAME 必须保留。自定义域名证书私钥只保留在网关。

## 交付顺序

已实现域名规范化、随机 TXT 验证名、SQLite 持久化、Ed25519 签名 API、一次性发布令牌、受限 HTTP stream、Host 精确路由、TLS allowlist 和 Caddy 按需证书模板。公网端到端验收覆盖绑定、TXT 验证、WSS 发布、独立证书和 18443 回源。

后续工作是运营级每域名速率/带宽配额、审计查询，以及 WebSocket、路径路由、团队成员和自定义策略。

不在首版提供通配符域名、邮件/任意 TCP、用户自定义上游地址、平台转售证书或多区域承诺。
