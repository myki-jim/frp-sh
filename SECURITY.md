# 安全与 0.4.0 迁移

0.4.0 使用协议 v2，不能与旧版加密流、UDP 流或 TCP 中继握手互通。请停止旧会话，同时升级服务端和客户端，然后重新创建房间。此次提交不会自动发布新的安装包。

## 凭据与边界

- `--password` 是信令服务器的共享访问密码；请使用 HTTPS 保护信令请求。它不是端到端密钥。
- `--key` 启用端到端保护，覆盖 UDP、TURN 数据链路及 TCP 回退。两端必须使用相同的高强度随机密钥。未设置该选项时，不提供端到端加密。
- v2 在流握手中交换随机挑战、校验 HMAC，并派生独立方向密钥；UDP 认证整个数据和控制帧，绑定双方会话并拒绝重放。此预共享密钥方案不提供前向保密，弱口令仍可能被离线猜测。
- 房间创建返回独立的所有者令牌。刷新、删除、房主流量报告及中继 HOST 握手需要该令牌。令牌保存在创建进程内存中，进程重启后应重新创建房间。
- 0.4.0 已移除所有内嵌管理页面及其数据接口，不再使用 FRPSH_ADMIN_TOKEN。
- LAN 使用本地受限辅助服务。安装目录必须保持管理员所有，IPC 仅允许安装时授权的账户；客户端不接受任意特权命令或路径。
- Profile 可以保存 `key`，本地配置文件仍为明文。请限制文件访问权限，不要提交配置、日志或本地部署脚本。

## 已修复与验证

本次修复包括：TCP 回退遗漏端到端密钥、nonce 重用、UDP 控制帧认证和重放、接收缓冲溢出错误确认、加密流 flush/shutdown 与背压、房间所有权、面板密码暴露和跨站访问、HTTPS 客户端支持、STUN/TURN 事务标识校验，以及面板 query token 解码。

同时修复中继在全局锁内等待网络、过期任务误删替代连接、Profile 重命名冲突与密钥丢失、域名/IPv6 中继地址解析和端口溢出。更新提示仅提供安装指引，不再自动覆盖运行中的程序，非交互进程不会等待输入。

运行 `cargo test --locked --test security_regressions -- --test-threads=1` 验证核心安全回归；完整检查使用 `cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings` 和 `cargo test --locked -- --test-threads=1`。

这不是完整的第三方安全认证。多人组网仍需将房间成员视为可信网络参与者，并用主机防火墙约束访问；组网队列容量、转发源地址策略、TURN 资源生命周期和大规模负载仍需进一步加固。TUN 管理员权限行为与外部 TURN 服务需要在实际部署环境单独验证。

## 泄露凭据处理

外部 TURN 测试仅从 `TURN_SERVER`、`TURN_USERNAME` 和 `TURN_PASSWORD` 环境变量读取凭据，测试默认忽略。不要将真实凭据放入测试代码或仓库文件。

历史清理只移除可达 Git 历史中的已识别凭据，不会撤销已经泄露的凭据。应在服务端更换旧密码/密钥，并让协作者重新克隆，避免重新推回旧提交。GitHub 缓存、PR 引用及第三方 fork 需另外处理，参见 [GitHub 官方说明](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/removing-sensitive-data-from-a-repository)。
