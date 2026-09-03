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
      link: https://github.com/myki-jim/frp-sh

features:
  - title:  房间制社交组网
    details: 一个房间号即可建立连接，无需公网 IP 与端口映射
  - title:  UDP 打洞直连
    details: STUN 优先的公网探测 + PUNCH/ACK 同时握手，可穿透受限锥形 NAT
  - title:  端到端加密
    details: 双方 --key 口令一致即启用 ChaCha20-Poly1305 加密传输
  - title:  中继回退
    details: 打洞失败自动转 TURN 中继（可选）或服务器 TCP 中继；失败即降级不反复重试，迟到直连复查自愈不对称场景
  - title:  连接配置档案
    details: frp-sh profile 一行保存服务器/密码/房间，一键重连；面板"一键接入"自动去重合并
  - title:  双端 Web 面板
    details: 服务器与客户端面板实时显示房间、链路速率、连接拓扑与运行日志（/debug 增量拉取）
  - title:  内置 TURN
    details: serve --turn 单二进制提供标准 RFC 5766 TURN，客户端多供应商自动择优
  - title:  单文件二进制
    details: 纯 Rust 实现，约 6 MB 单文件，不依赖 frp / libp2p / webrtc
---

::: warning 开发阶段（0.x）

frp-sh 目前处于 **开发阶段**（`v0.3.x`），**1.0 之前功能与协议可能调整**，不承诺向后兼容。
版本快速迭代，建议跟随更新；求稳部署请留意更新提示与代差警告。
详见[版本策略](./versioning)。

:::

## 名字的由来？

我们叫 `frp.sh`，但跟 [FRP](https://github.com/fatedier/frp) 没有半毛钱关系 —— 纯属“域名碰瓷”。
**F**ast Room Protocol · **R**eal-time P2P · **P**layful Shell。
**我们不穿透内网，我们消灭内网的概念。** [（更多吐槽 →）](./name)

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
