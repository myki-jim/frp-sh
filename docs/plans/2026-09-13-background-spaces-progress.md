# 后台空间改造实施记录

状态：开发分支 `codex/background-spaces`，尚未发布。线上版本仍为 0.5.5。

本文描述当前代码，替代此前各阶段累计记录。目标设计见 [改造计划](2026-09-13-background-spaces.md)。不能把“接口已实现”解释成“完整组网功能已交付”。

## 已实现

| 功能 | 当前范围 |
| --- | --- |
| 永久空间登记 | SQLite 保存空间、设备成员、邀请摘要；默认无到期时间；重启后保留；当前与旧 LAN 房间独立 |
| 设备认证 | Ed25519 私钥本机持久化，Windows 使用账户 DPAPI；请求签名绑定服务器、完整操作摘要、一次性挑战和期限 |
| 动态邀请 | v3 链接只含 HTTPS 服务器地址和随机令牌；默认 900 秒、默认单次；服务器可配置期限；事务兑换、幂等重试、撤销 |
| 空间 API | `/spaces/v1/challenge`、`/spaces/v1/execute`；专用有界数据库线程；创建需管理授权；其他操作按设备成员权限执行 |
| 空间 CLI | `space create/list/members/invite/redeem/revoke-invites/remove-member/leave/delete`；目前只管理登记和授权 |
| 后台监督 | client Profile 和 server 两种任务；独立子进程；配置重载、失败重试、退出清理；不打开终端 UI |
| 原生服务安装 | `agent install --job PATH`；Windows SCM 虚拟服务账户；Linux systemd 系统服务；macOS LaunchDaemon，Unix 使用原安装者非 root UID |
| 普通用户管理 | `frp-sh start/stop` 默认管理已安装 client；`--server` 管理 server；不需要提权；暂停不会卸载监督服务 |
| 状态 | `status --json` 按当前账户查询；跨 Windows 服务账户验证 SCM 进程身份；server 必须确认实际子进程监听状态才报告 serving |
| 日志 | 与终端界面分离；原生服务写入任务目录下 logs，子进程继承；有界队列、轮转和敏感字段脱敏 |

## 使用开发版验证后台服务

已有连接 Profile：

```sh
frp-sh --config /absolute/config.toml agent configure --profile friends --job /absolute/job.toml
frp-sh agent install --job /absolute/job.toml
frp-sh status --json
frp-sh stop
frp-sh start
```

服务端：配置文件的 `[server]` 表保存监听、限额和空间 API 设置。使用完整二进制：

```sh
frp-sh --config /absolute/server.toml agent configure-server --job /absolute/server-job.toml
frp-sh agent install --job /absolute/server-job.toml
frp-sh stop --server
frp-sh start --server
```

安装要求先有可信安装位置的二进制；client 还要求已经安装网络 helper。安装阶段才使用管理员权限；Unix 服务不依赖用户登录会话，Windows 服务不使用 LocalSystem 运行 client/server。以上多步开发入口尚未收敛为最终一行安装加入体验。

## 验证状态

- 空间 HTTP API、签名重放/动作篡改拒绝、管理权限、邀请期限/次数、数据库重启等回归已通过 Windows/Linux/macOS CI。
- macOS 原生服务已通过真实 LaunchDaemon 启动、普通 UID、普通用户启停、服务重启及日志目录测试。
- Windows 原生服务测试发现 PowerShell 5.1 的 `sc.exe` 参数引号问题，已修复并重新运行 CI；尚不能宣称最终服务测试通过。
- Linux 原生测试发现 systemd 工作目录配置格式错误，已修复并增加 `systemd-analyze verify`；等待重新验证。
- 新增默认暂停任务的自启状态回归通过。真实断电重启、未登录启动和多设备公网组网验收尚未完成。

## 必须继续完成

1. 永久空间接入实际虚拟网卡与 peer-pair 数据传输，owner 离线不终止其他设备；旧 `create` 目前仍使用原房间逻辑。
2. 短期成员访问凭据、刷新、在线租约、对端认证、成员撤销后的连接关闭。
3. 本地空间加入状态持久化、Windows 前台与服务账户之间的设备身份衔接。
4. 一行安装配置加入、邀请页、默认动态邀请、旧 Profile 的明确迁移路径。
5. TUI 附着后台任务、连接事件和延迟回传；当前 client 不能仅凭子进程存活报告 connected。
6. 安装器升级时协调正在运行的 client/server 服务，以及安全卸载和故障回滚验收。
7. 完整三平台、多人互通、服务重启和权限回归；通过后同步用户文档、版本和发布资源。

## 发布边界

0.5.5 的 status 和实验性 agent run 已发布；本页新增能力只在开发分支。新空间登记不等于已建立 LAN，现有邀请不能提前宣称已去除长期密码，当前进程在线也不等于网络可用。没有因此更新线上服务器或发布新版本。