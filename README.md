# frp-sh

社交化 P2P 组网工具 —— 房主建房间，朋友凭号加入，UDP 打洞直连，失败自动中继回退。纯 Rust，单文件二进制。

三个使用系列：

| 系列 | 命令 | 场景 |
|------|------|------|
| **组网（推荐）** | `frp-sh lan create/join` | 类 Tailscale：虚拟网卡整机入网，互访整机 + 对方整个局域网 |
| **游戏** | `frp-sh game create/join` | Minecraft 等联机游戏，纯端口转发（默认 25565） |
| **开发** | `frp-sh dev create/join` | 开发调试，应用层端口转发 |

## 安装 / 更新

一行命令，重复执行即更新（脚本自动检测系统与架构，从 GitHub Releases 下载最新版）：

| 系统 | 命令 |
|------|------|
| Linux / macOS | `curl -fsSL https://frp.sh/install.sh \| sh` |
| Windows (PowerShell) | `irm https://frp.sh/install.ps1 \| iex` |

首次运行 `frp-sh` 会进入交互式向导，配置你的信令服务器即可开始使用。

## 快速开始

```bash
# 组网（推荐，需 root/管理员）：房主创建房间
frp-sh lan create
# → Room created : lan-a3f9c2；访客加入后互访整机 + 访问房主局域网

# 访客加入
frp-sh lan join lan-a3f9c2
# → >>> 本地局域网直连 (LAN direct) with ...（同一 WiFi 秒连）

# 游戏联机（纯端口转发，无需管理员）
frp-sh game create --service 127.0.0.1:25565 --key mypass
frp-sh game join game-a3f9c2 --listen 127.0.0.1:25565 --key mypass

# 开发联调
frp-sh dev create --service 127.0.0.1:8080
frp-sh dev join dev-a3f9c2 --listen 127.0.0.1:8080
```

- 打洞失败自动回退中继；`--relay` 强制中继
- `--key` 启用 ChaCha20-Poly1305 端到端加密
- `--max-conns N` 限制连接数（默认无限）；`--spread N` 调打洞散布
- **同局域网自动直连**：房主通告局域网地址，同一 WiFi 下访客秒级直连（不经服务器）
- **组网（lan）**：虚拟网卡整机入网；`--expose-lan` 可将本机局域网接入隧道（默认不暴露）
- 断线自动重连：网络抖动 / NAT 映射过期后按 2s、4s、8s…退避自动重连（上限 15s）
- 每台设备有唯一 ID（UUID），虚拟网卡 IP 由 ID 稳定派生，长期不变
- 默认端口 `25565` 是 Minecraft 的默认端口，可通过 `--service` / `--listen` 改成任意端口
- 详见文档：https://frp.sh

## 文档

- 在线文档（中英双语）：https://frp.sh
- 文档源码：`web/`（VitePress，GitHub Actions 自动构建并部署到 Cloudflare Pages）

## 架构

```text
信令服务器（房间注册/地址交换/UDP 探测）── 可部署在任意公网 VPS
中继节点（打洞失败时转发流量）
frp-sh 客户端（打洞 + FRS1 可靠流 + 隧道）
```

- 打洞：PUNCH/ACK 同时握手 + 端口散布，可穿透受限锥形 NAT
- 传输：FRS1 可靠 UDP 流（滑动窗口/重传/keepalive），可选 ChaCha20-Poly1305 加密
- 隧道：CNEW 帧协议，单会话顺序多连接复用
- 详见文档「网络原理」章节

## 开发

```bash
cargo test                 # 单元 + 端到端测试（GitHub Actions 自动执行）
cargo clippy --all-targets -- -D warnings
cd web && npm run build    # 构建文档站
```

CI（GitHub Actions）：push/PR 自动运行 fmt / clippy / test / release 构建；打 `v*` 标签自动发布五平台二进制（linux/macos x86_64+aarch64、windows x86_64）；push main 自动部署文档站到 Cloudflare Pages。

## License

MIT
