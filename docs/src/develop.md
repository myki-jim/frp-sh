# 开发与测试

面向贡献者的工程结构、测试方式与设计约定。

## 项目结构

```text
src/
├── main.rs            # 入口：解析子命令，分发到 commands
├── lib.rs             # 库入口（供集成测试引用）
├── cli.rs             # clap 子命令定义
├── commands.rs        # create/join/serve 流程编排（打洞/中继/加密接线）
├── config.rs          # TOML 配置加载（信令/中继/独立 UDP 探测）
├── error.rs           # thiserror 错误类型（FrpError）
├── utils.rs           # 房间号/token/时间戳/校验
├── room/state.rs      # 会话状态（角色、对端、中继标记）
├── signaling/
│   ├── client.rs      # REST 客户端 + UDP 公网地址探测
│   └── server.rs      # axum 服务端 + UDP echo + TCP 中继配对
└── p2p/
    ├── hole_punch.rs  # PUNCH/ACK 同时握手 + 端口散布 + 自我打洞防护
    ├── stream.rs      # FRS1 可靠 UDP 流（窗口/重传/保活/加密）
    └── relay.rs       # 中继客户端传输
tests/e2e.rs           # 进程内信令服务器端到端测试
config/default.toml    # 默认配置（127.0.0.1）
docs/                  # 本文档站源码（mdBook）
deploy/                # VPS 部署脚本（含服务器凭据，注意保密）
```

## 本地开发

```bash
cargo build                # 调试构建
cargo run -- serve         # 本地起信令服务器
cargo run -- game create   # 房主（默认连 127.0.0.1:8080）
cargo run -- game join game-xxxx
```

## 测试

```bash
cargo test
```

### 单元测试（10 个）

| 模块 | 覆盖 |
|------|------|
| `utils` | 房间号格式、token 长度、前缀清洗 |
| `config` | 默认值、TOML 解析、缺省字段 |
| `hole_punch` | PUNCH/ACK 报文解析 |
| `stream` | 帧编解码、64KB 往返、20% 丢包重传、加密往返、双向优雅关闭 |

### 端到端测试（5 个）

测试在**进程内**启动完整信令服务器（HTTP + UDP echo + 中继），跑真实会话：

| 测试 | 验证 |
|------|------|
| `e2e_direct_hole_punch` | 打洞直连 + 单连接隧道 echo 往返 |
| `e2e_direct_multiconn` | 同一会话 3 个顺序连接各自 echo |
| `e2e_direct_encrypted` | `--key` 加密隧道往返 |
| `e2e_relay_fallback` | 强制中继配对 + 隧道往返 |
| `room_lifecycle_api` | 房间创建/查询/加入/删除 API 生命周期 |

测试服务器使用**独立 UDP 端口**（通过 `signaling_udp` 配置），避免 Windows 上同端口 TCP/UDP 绑定的时序问题。

## 设计约定

- **单 socket 贯穿**：公网探测、打洞、数据流使用同一个 UDP socket，保证 NAT 映射一致
- **select 安全**：涉及流式读取的并发处使用「常驻任务 + 有界 channel」，禁止在 `select!` 中直接读流（会丢弃未选中分支的半读数据）
- **Windows 兼容**：任何 UDP recv/send 错误处理都要考虑 WSAECONNRESET(10054) ICMP 毒化
- **错误分层**：库函数返回 `Result<T, FrpError>`（thiserror），命令层用 `anyhow` 聚合
- **日志**：`log` 宏 + `env_logger`，`--verbose` 输出帧级调试信息

## 常见踩坑记录

1. **帧头长度**：FRS1 帧头为 15 字节（magic4+flags1+seq4+ack4+len2），曾误写为 16 导致全部帧被丢弃
2. **自我打洞**：两端引擎临时端口相邻时，spread 目标会包含自身端口；必须按端口排除
3. **ACK 一次性发送**：Windows ICMP 毒化会吞掉首个 ACK，需重试
4. **测试端口冲突**：强杀进程后同端口 UDP 绑定短暂失败，测试服务器用独立 UDP 端口规避

## 发布流程

```bash
cargo test                          # 全绿
cargo build --release               # 产物 target/release/frp-sh
cargo build --release --target x86_64-unknown-linux-musl   # Linux 静态（可选）
```
