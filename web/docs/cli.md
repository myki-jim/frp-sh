# 命令与键盘操作

`frp-sh --help`、`frp-sh game --help`、`frp-sh dev --help` 和 `frp-sh profile --help` 显示当前二进制的完整参数。

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

默认只共享虚拟设备网络，不共享物理局域网。通用命令 `frp-sh join 1234 --network` 明确允许整机网络访问；服务访问不自动获得此权限。

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

## 自建服务器

完整服务器二进制与普通客户端包不同：从 Release 下载完整资产，或 `cargo build --release`。

```sh
./frp-sh serve --addr 0.0.0.0:8080 --relay-addr 0.0.0.0:8081 --password "$FRPSH_SERVE_PASSWORD"
```

公网监听必须配置非空密码。开放 TCP/UDP 8080 和 TCP 8081；使用 HTTPS 保护信令凭据。内置 TURN 可选，但只允许本实例有效中继端点之间通信，不能代理任意 UDP 地址。
