# 安装与构建

## 环境要求

- **Rust**：1.70+（推荐用 rustup 安装最新稳定版）
- **操作系统**：Windows / Linux / macOS（UDP 行为基本一致；已处理 Windows 特有的 WSAECONNRESET 问题）

## 方式一：源码构建（推荐）

```bash
git clone <你的仓库地址> frp-sh
cd frp-sh

# 调试构建
cargo build

# 发布构建（推荐分发）
cargo build --release
```

产物：

```text
target/release/frp-sh.exe   # Windows
target/release/frp-sh       # Linux / macOS
```

发布构建约 **5.4 MB**（已 strip + LTO），单文件可分发，无需运行时依赖。

## 方式二：直接编译运行

不克隆仓库时，也可把 `src/`、`Cargo.toml`、`Cargo.lock` 拷到任意目录后构建。

## 安装到 PATH

```bash
# Linux / macOS
sudo cp target/release/frp-sh /usr/local/bin/frp-sh

# Windows
copy target\release\frp-sh.exe %USERPROFILE%\bin\
```

之后即可直接使用 `frp-sh` 命令。

## 验证安装

```bash
frp-sh --help
frp-sh game --help
frp-sh serve --help
```

输出子命令与参数说明即安装成功。

## 在服务器上构建（无本地 Rust 环境）

服务器只需运行信令服务器，可在服务器上直接构建：

```bash
# 安装 Rust（约 1 分钟）
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
export PATH="$HOME/.cargo/bin:$PATH"

# 上传代码后构建
cd /opt/frpsh
cargo build --release
```

2 核 VPS 全量编译约 2.5 分钟。

## 系统要求备忘

| 平台 | 备注 |
|------|------|
| Windows | 已处理 10054（ICMP 毒化）导致的 send/recv 假错误 |
| Linux | 无需额外配置 |
| macOS | UDP 行为与 Linux 一致 |
