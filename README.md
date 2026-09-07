# frp-sh

## 交互式房间与个人预设

运行 `frp-sh` 进入终端应用：首页可直接创建默认 LAN 房间、加入房间、自定义创建、管理房间预设和已保存连接。也可以运行 `frp-sh preset` 打开预设页（快捷键 `7`），`frp-sh profile` 打开已保存连接（快捷键 `2`）。

```sh
frp-sh create
frp-sh 3856
frp-sh create game
frp-sh create dev
frp-sh create --preset 我的联机 --mtu 1280
```

新入口 `create game`、`create dev` 都使用 LAN 底座；目前内置场景共享安全网络默认值，不自动扫描游戏、开放物理局域网或发布开发服务。旧的 `game create`、`dev create` 服务命令继续兼容。

自定义创建：Tab 切换字段、Enter 创建、Ctrl+S 保存预设。预设页：N 新建、E 编辑或重命名、C 复制、Delete 删除、Enter 使用。内置预设只读，可复制后修改。参数覆盖顺序为默认值、个人预设、本次显式参数；本次修改不会自动覆盖原预设。预设保存有效时间、MTU、打洞范围、中继选择、前缀及场景，不保存口令、房间号或邀请。

加入页支持房间号或邀请链接，Tab 可输入可选的额外加密口令。`--key` 仍是双方必须一致的额外加密口令，不是服务器登录密码。房间内 I 打开邀请、G 打开独立日志，Q/Esc 请求离开，Enter 确认或 Esc 取消。切换房间页面不会中断连接。

多人虚拟局域网与私有服务共享。0.5.0 使用协议 v3；服务器、客户端和 helper 需要一起升级。

## 安装与更新

```powershell
irm https://frp.sh/install.ps1 | iex
```

```sh
curl -fsSL https://frp.sh/install.sh | sh
```

只有安装阶段需要管理员授权，日常从普通终端运行。服务模式不创建虚拟网卡。

## 打开终端应用与 Profile

运行 `frp-sh` 打开应用，按 `2` 查看 **Profiles / 连接配置**。也可以直接运行 `frp-sh profile`，脚本列表使用 `frp-sh --plain profile list`。

Profiles 页：`N` 新建、`E` 编辑、`D` 设为默认、`Enter` 连接、`Delete` 删除。`G` 打开独立日志页，`Tab` 切换页面。邀请导入后会自动保存 Profile。

首次使用，在应用的设置页配置服务器地址与服务器密码。新邀请只包含房间凭据，不分发服务器管理密码。不要把邀请公开发布。

## 游戏联机与 LAN

```sh
frp-sh game create
frp-sh game join 1234
# 与同一房间互通
frp-sh lan join 1234
```

先在游戏里开启局域网世界，再使用房主虚拟 IP 连接。这个方式无需为 frp-sh 指定一个游戏端口，但游戏自身可能要求输入端口；不承诺所有游戏自动出现在搜索列表。

默认只共享虚拟设备网络，不共享物理局域网。`frp-sh join 1234` 默认加入 LAN；旧版 `--network` 参数仍兼容。服务房间仍仅提供发布的服务。

## 游戏服务器 / 开发服务

```sh
frp-sh game create --kind server --tcp 25565
frp-sh game create --kind server --udp 19132
frp-sh dev create --service http://127.0.0.1:3000 --label Web
frp-sh dev create --tcp 3000 --tcp 8080 --udp 9000
frp-sh dev join 1234
frp-sh join 1234
```

端口只是示例，没有默认 25565。缺少创建参数时终端会要求输入；脚本和 JSON 模式立即报错。支持最多 16 个房主发布的服务、32 个成员，每个成员最多 64 条并发流，房间总计最多 256 条。

服务房间使用加密 TCP 中继，TCP/WebSocket/SSH 可并发；UDP 保留报文与来源隔离，但经过 TCP 中继时会受有序传输延迟影响。UDP 在收到实际流量前显示未验证。需要游戏 UDP 直连时使用 Game/LAN 网络房间。

访客默认绑定回环地址；原端口占用时自动分配空闲端口，以页面显示的实际地址为准。`C` 复制连接地址，`I` 复制邀请或一行安装加入命令，`G` 查看日志。

```sh
# 选择一个服务，指定本地访问端口
frp-sh dev join 1234 --service 1 --listen 4000
# 在房主的另一个终端执行
frp-sh dev add --tcp 8081 --label API
frp-sh dev remove 2
frp-sh dev revoke
```

服务增删会关闭现有流并重新连接，不重放应用请求。不能删除最后一个服务来留下空房间；退出房主会话可关闭整个房间。`dev revoke` 或房主页 `R` 撤销当前全部邀请和已建立连接，重新复制新邀请后才能加入。自定义 `--key` 需两端一致，邀请会携带它。


在线文档：[frp.sh](https://frp.sh/)。安全边界见 [SECURITY.md](SECURITY.md)。

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

CI（GitHub Actions）：push/PR 自动运行 fmt / clippy / test / release 构建；打 `v*` 标签自动发布 7 个平台的客户端、辅助服务和校验文件（linux x86_64+aarch64 各 glibc/musl 双版本、macOS x86_64+aarch64、Windows x86_64）；push main 自动部署文档站到 Cloudflare Pages。

## License

MIT

## 终端宠物

按 P 或运行 `frp-sh pet` 打开造型页：20 款完整字符造型，方向键选择、Enter 保存，H 显示/隐藏，M 减少动画。预览明确标记为预览，不改变网络状态。

普通 80×24 终端使用底部紧凑趴姿；104 列以上且至少 22 行时显示完整侧栏造型，更小窗口自动隐藏。宠物区域预留布局空间，不遮挡连接卡片；日志、邀请、表单不显示宠物。外观配置保存在本地，与房间预设和 Profile 分开。

宠物根据连接统计表现休息、等待、连接、直连、中继、网络波动和重连；中继本身不是故障。超过 200 ms 的持续延迟作为波动提示，质量状态保持五秒后切换；没有返回数据的中继仍显示连接中，不冒充已连接。宠物不新增网络探测。
