# HypForward

基于 Rust + Tokio 的高性能 Minecraft TCP 转发代理。单个二进制、低内存、异步
全双工转发，支持 Forge/FML 握手和一个进程转发多个服务器。

## 一键安装脚本

```bash
curl -fsSL https://raw.githubusercontent.com/greepar/HypForward/main/install.sh | sh
```
Linux 自动支持 systemd、Alpine/OpenRC、SysV init 和 runit。支持 Linux
x86_64/ARM64 与 macOS Intel/Apple Silicon。

## 多服务器配置

格式为 `监听地址=目标地址`，多条规则用英文逗号分隔：

```text
0.0.0.0:25565=mc.hypixel.net:25565,0.0.0.0:25566=example.com:25565
```

也可以直接运行：

```bash
hypforward \
  --forward 0.0.0.0:25565=mc.hypixel.net:25565 \
  --forward 0.0.0.0:25566=example.com:25565
```

默认配置文件为 `/etc/hypforward.conf`。不配置时默认使用：

```text
0.0.0.0:25565=mc.hypixel.net:25565
```

## 下载

[GitHub Releases](https://github.com/greepar/HypForward/releases) 提供 Linux、Windows、
macOS 的 x86_64 和 ARM64 二进制文件及 SHA-256 校验和。

## 许可证

[GNU General Public License v3.0 only](LICENSE)
