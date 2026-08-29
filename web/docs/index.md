---
layout: home

hero:
  name: frp-sh
  text: 社交化 P2P 打洞工具
  tagline: 房主建房间，朋友凭号加入 —— UDP 打洞直连，失败自动中继回退。纯 Rust，单文件二进制。
  actions:
    - theme: brand
      text: 快速开始
      link: /quickstart
    - theme: alt
      text: 网络原理
      link: /architecture
    - theme: alt
      text: GitHub
      link: https://github.com/frp-sh/frp-sh

features:
  - title:  房间制社交组网
    details: 一个 game-xxxxxx 房间号即可建立连接，无需公网 IP 与端口映射
  - title:  UDP 打洞直连
    details: STUN 简化版公网探测 + PUNCH/ACK 同时握手，可穿透受限锥形 NAT
  - title:  端到端加密
    details: 双方 --key 口令一致即启用 ChaCha20-Poly1305 加密传输
  - title:  中继回退
    details: 打洞失败自动转服务器 TCP 中继，迟到直连复查自愈不对称场景
  - title:  多连接复用
    details: 同一隧道顺序承载多个 TCP 连接，断线秒级重连
  - title:  单文件二进制
    details: 纯 Rust 实现，约 5 MB 单文件，不依赖 frp / libp2p / webrtc
---

## 安装

一行命令安装 / 更新（链接同一，重复执行即更新；脚本自动检测系统与架构，从 GitHub Releases 下载最新版）：

::: code-group

```bash [Linux / macOS]
curl -fsSL https://frp.sh/install.sh | sh
```

```powershell [Windows (PowerShell)]
irm https://frp.sh/install.ps1 | iex
```

:::

安装完成后运行 `frp-sh`，按提示交互式配置你的信令服务器即可开始使用。
