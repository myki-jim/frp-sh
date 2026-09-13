# 命令与键盘操作

## 后台监督、永久空间与状态（开发分支）

先在 `frp-sh profile` 保存连接。下面的 `friends` 是已有 Profile 名称，任务文件所在目录必须存在；在另一终端执行启停和查询。

```sh
frp-sh agent configure --profile friends --job ./agent.toml
frp-sh agent run --job ./agent.toml
frp-sh status
frp-sh status --json
frp-sh agent stop --job ./agent.toml
frp-sh agent start --job ./agent.toml
```

`run` 是无终端界面的长驻监督进程。`agent install --job PATH` 会安装系统服务：Windows 使用受限虚拟服务账户，Linux 使用 systemd，macOS 使用 LaunchDaemon。安装阶段才请求管理员权限；之后用 `frp-sh start`、`frp-sh stop`、`frp-sh status` 管理 client，服务器使用 `--server`。日志仍通过 `frp-sh logs` 查看。

`status` 查询当前账户有权管理的进程；Windows 会验证服务进程身份后显示跨账户服务状态。退出码：0 表示运行，1 为不可用或会话错误，2 为连接尚未验证，3 为没有可见进程，4 为权限不足。

### 永久空间

永久空间由服务端 SQLite 保存，房主退出不会删除其他成员的会话。邀请码不含服务器密码，而是默认 15 分钟、单次使用的随机令牌。空间连接只接受 HTTPS/WSS。

```sh
frp-sh space create friends
frp-sh space invite SPACE_ID
# 加入者：从标准输入读取邀请码，避免写入 shell 历史
frp-sh space redeem --stdin
frp-sh space connect SPACE_ID
```

每个成员有固定的 `10.66.0.x` 虚拟 IP 和 10 分钟会话令牌。中继仅在同一空间的已认证成员之间转发加密帧；移除成员、替换会话、到期或关闭会话会立即使旧中继失效。`space connect` 当前是前台入口，后台空间恢复和一行安装加入仍在开发中。旧 `create` / `join` 和旧邀请保持兼容逻辑，不能视为永久空间。


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

端口只是示例，没有默认 25565。缺少创建参数时终端会要求输入；脚本和 JSON 模式立即报错。支持最多 16 个房主发布的服务、32 位访客（加房主共 33 人），每个成员最多 64 条并发流，房间总计最多 256 条。

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
