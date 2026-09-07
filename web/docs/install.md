# 安装与维护（0.5.4）

安装器优先从官网下载，失败时自动切换到 GitHub；版本清单固定客户端和辅助程序的 SHA-256，防止镜像更新时混装。Windows 升级会复用已有且签名有效的 Wintun 驱动，减少重复下载。官网使用全球 CDN，并非中国大陆专属 CDN，实际速度取决于当地网络。

0.4.0 将 LAN 网络操作交给受限的系统辅助服务。安装阶段授权一次；之后请用普通账户运行客户端，建房、加入和重连不会再次请求 UAC 或 sudo。game/dev 端口转发不需要辅助服务。

> 0.5.0 包含协议 v3 与 helper 升级，请在两端重新运行安装器，并同步更新服务器。

## 安装

Windows PowerShell：

```powershell
irm https://frp.sh/install.ps1 | iex
```

Linux/macOS：

```sh
curl -fsSL https://frp.sh/install.sh | sh
```

安装器下载同一版本的客户端、辅助服务和 SHA-256 校验文件。Windows 还验证 Wintun 的 Authenticode 签名。校验和用于检查下载完整性，不等同于独立的发布签名。

| 平台 | 安装位置 | 服务 |
| --- | --- | --- |
| Windows | `%ProgramFiles%\frp-sh` | `FrpShNetwork` |
| Linux | `/usr/local/lib/frp-sh` | `frp-sh-network.service` 或 procd |
| macOS | `/usr/local/lib/frp-sh` | `com.frpsh.network` |

安装目录只有管理员可写。Windows 授权安装前的用户 SID；Unix 授权原始用户 UID。直接用 root 安装时需要指定 `FRPSH_INSTALL_UID`。Linux 需要 iproute2，以及 systemd 或 procd；OpenWrt 真机验证仍待完成。

## 普通账户检查

```sh
frp-sh doctor
frp-sh doctor --network-test
frp-sh --lang zh-CN lan create
```

`--network-test` 临时创建并关闭虚拟网卡，请先退出已有 LAN 会话。辅助服务不可用时命令报告错误，不自动提权。不要开放本地 IPC 给其他账户。

## 更新和卸载

`frp-sh update` 只检查版本并显示安装说明。更新当前仍需重新运行安装器，属于安装维护阶段；无额外授权的签名自动升级尚未实现。升级前退出活动会话。安装器保留上一对程序供回滚。

卸载目前通过系统管理员完成：停止并删除上述服务，再删除对应安装目录及 PATH 项；保留用户配置和日志，除非明确需要删除。自动卸载和跨平台升级回滚仍需验收，暂不作为正式发布保证。

## 编译

```sh
cargo build --locked --release --bins
cargo build --locked --release --no-default-features --bin frp-sh
```

默认安装精简客户端。部署服务端可下载名称中不带 client 的完整 Release 资产。默认构建包含 `serve`；精简客户端构建排除服务端和内置 TURN 服务端。LAN 安装还需配套 `frp-sh-net`，Windows 需要受保护目录中的 Wintun DLL，不能只复制客户端替代完整安装。
