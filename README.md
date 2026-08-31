# HypForward

基于 Rust 和 Tokio 的高性能 Minecraft/Hypixel 异步转发代理。它监听本机 TCP
端口，重写客户端首个 Minecraft 握手包中的目标主机与端口，然后进行双向全双工
转发。

相较于旧版 `forward.sh`，本版本不修改 `iptables`、不启用系统 IP 转发，也不会
关闭 UFW。程序是单个可执行文件，适合低内存服务器和高并发连接。

## 功能

- Tokio 多线程异步运行时，每个连接使用轻量异步任务。
- 使用 `copy_bidirectional` 进行全双工转发并正确处理 TCP 半关闭。
- 将握手目标改写为 `mc.hypixel.net:25565`。
- 保留握手主机字段中的 Forge/FML NUL 后缀，例如 `\0FML\0`。
- 对握手帧、主机长度、VarInt 和连接状态进行边界校验。
- 无需 root 权限；监听 `25565` 时只需确保防火墙已放行该 TCP 端口。

## 一键安装

Linux 和 macOS 会自动识别系统及 CPU 架构，从最新 GitHub Release 下载并校验
SHA-256：

```bash
curl -fsSL https://raw.githubusercontent.com/greepar/HypForward/main/install.sh | sh
```

默认安装到 `/usr/local/bin/hypforward`。Linux 会自动探测初始化系统，安装、启用
并立即启动服务，支持 systemd、Alpine/OpenRC、SysV init 和 runit。

systemd 查看状态和日志：

```bash
systemctl status hypforward
journalctl -u hypforward -f
```

Alpine/OpenRC 查看状态和日志：

```bash
rc-service hypforward status
```

也可以显式指定服务管理器：

```bash
curl -fsSL https://raw.githubusercontent.com/greepar/HypForward/main/install.sh | \
  HYPFORWARD_INSTALL_SERVICE=openrc sh
```

安装指定版本或自定义安装目录：

```bash
curl -fsSL https://raw.githubusercontent.com/greepar/HypForward/main/install.sh | \
  HYPFORWARD_VERSION=v0.1.0 HYPFORWARD_INSTALL_DIR="$HOME/.local/bin" \
  HYPFORWARD_INSTALL_SERVICE=0 sh
```

支持的安装变量：

| 变量 | 默认值 | 说明 |
| --- | --- | --- |
| `HYPFORWARD_VERSION` | `latest` | Release 版本，例如 `v0.1.0` |
| `HYPFORWARD_INSTALL_DIR` | `/usr/local/bin` | 二进制安装目录 |
| `HYPFORWARD_INSTALL_SERVICE` | `auto` | `systemd`、`openrc`、`sysv`、`runit`、`1` 或 `0` |
| `HYPFORWARD_REPOSITORY` | `greepar/HypForward` | Release 所属 GitHub 仓库 |

Windows 请从 [Releases](https://github.com/greepar/HypForward/releases) 下载对应 ZIP，
解压后运行 `hypforward.exe`。

## 环境要求

- Rust stable 工具链（推荐通过 [rustup](https://rustup.rs/) 安装）
- 可访问 `mc.hypixel.net:25565` 的 Linux、macOS 或 Windows 主机

Debian/Ubuntu 如启用了 UFW，可手动放行端口：

```bash
sudo ufw allow 25565/tcp
```

云服务器还需要在提供商控制台的安全组中放行 TCP `25565`。

## 编译

```bash
git clone https://github.com/greepar/HypForward.git
cd HypForward
cargo build --release
```

生成的单文件程序位于：

```text
target/release/hypforward
```

## 运行

```bash
./target/release/hypforward
```

默认配置：

| 配置 | 值 |
| --- | --- |
| 监听地址 | `0.0.0.0:25565` |
| 目标主机 | `mc.hypixel.net` |
| 目标端口 | `25565` |

如需修改监听地址或目标服务器，请编辑 `src/main.rs` 顶部的 `LISTEN_ADDR`、
`TARGET_HOST` 和 `TARGET_PORT`，然后重新执行 release 编译。

## 测试

```bash
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

## 自动构建

GitHub Actions 在 main、Pull Request 和手动触发时构建并保存以下 artifacts；推送
`v*` 标签时还会自动创建 Release，并发布压缩包及 `SHA256SUMS`：

| 系统 | 架构 | Rust 目标 |
| --- | --- | --- |
| Linux | x86_64 | `x86_64-unknown-linux-musl` |
| Linux | ARM64 | `aarch64-unknown-linux-musl` |
| macOS | Intel | `x86_64-apple-darwin` |
| macOS | Apple Silicon | `aarch64-apple-darwin` |
| Windows | x86_64 | `x86_64-pc-windows-msvc` |
| Windows | ARM64 | `aarch64-pc-windows-msvc` |

创建发布版本：

```bash
git tag v0.1.0
git push origin v0.1.0
```

## 许可证

本项目采用 [GNU General Public License v3.0 only](LICENSE) 许可证。
