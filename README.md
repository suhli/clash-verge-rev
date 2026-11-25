<h1 align="center">
  <img src="./src-tauri/icons/icon.png" alt="Clash" width="128" />
  <br>
  Continuation of <a href="https://github.com/zzzgydi/clash-verge">Clash Verge</a>
  <br>
</h1>

<h3 align="center">
A Clash Meta GUI based on <a href="https://github.com/tauri-apps/tauri">Tauri</a>.
</h3>

<p align="center">
  Languages:
  <a href="./docs/README_en.md">English</a> ·
  <a href="./docs/README_es.md">Español</a> ·
  <a href="./docs/README_fa.md">فارسی</a> ·
  <a href="./docs/README_ja.md">日本語</a> ·
  <a href="./docs/README_ko.md">한국어</a> ·
  <a href="./docs/README_pt.md">Português</a> ·
  <a href="./docs/README_ru.md">Русский</a> ·
  <a href="./README.md">简体中文</a>
</p>

## Preview

| Dark                             | Light                             |
| -------------------------------- | --------------------------------- |
| ![预览](./docs/preview_dark.png) | ![预览](./docs/preview_light.png) |

## Install

请到发布页面下载对应的安装包：[Release page](https://github.com/clash-verge-rev/clash-verge-rev/releases)<br>
Go to the [Release page](https://github.com/clash-verge-rev/clash-verge-rev/releases) to download the corresponding installation package<br>
Supports Windows (x64/x86), Linux (x64/arm64) and macOS 11+ (intel/apple).
支持 Windows (x64/x86)、Linux (x64/arm64) 和 macOS 11+ (intel/apple)。

#### 我应当怎样选择发行版

| 版本        | 特征                                     | 链接                                                                                   |
| :---------- | :--------------------------------------- | :------------------------------------------------------------------------------------- |
| Stable      | 正式版，高可靠性，适合日常使用。         | [Release](https://github.com/clash-verge-rev/clash-verge-rev/releases)                 |
| Alpha(废弃) | 测试发布流程。                           | [Alpha](https://github.com/clash-verge-rev/clash-verge-rev/releases/tag/alpha)         |
| AutoBuild   | 滚动更新版，适合测试反馈，可能存在缺陷。 | [AutoBuild](https://github.com/clash-verge-rev/clash-verge-rev/releases/tag/autobuild) |

#### 安装说明和常见问题，请到 [文档页](https://clash-verge-rev.github.io/) 查看

### TG 频道: [@clash_verge_rev](https://t.me/clash_verge_re)

---

## Updates（本分支更新内容）

本分支基于上游 Clash Verge Rev，新增并完善了 **Windows 分应用透明代理（Application Proxy）** 与 **原生实时流量 OSD**：

### 1. 分应用透明代理（ProxyBridge + WinDivert）
- **独立运行模式**：独立于 Windows 系统代理和 TUN（与 TUN 互斥），无需修改系统路由表、全局 DNS 或 Mihomo 内核，主界面保持普通权限运行，仅 WinDivert 捕获核心运行在受控的提权 Helper 进程中。
- **TCP & UDP（IPv4 / IPv6）全协议代理**：将匹配应用的流量通过本地 SOCKS5 中继转发至 Mihomo 的 `socks-port` / `mixed-port`，完全遵循 Clash 当前的规则 / 全局 / 直连模式。
- **完整规则可配置（参考 ProxyBridge）**：
  - 支持从「运行中的进程」、本地「选择 EXE」或「手动添加」创建规则，支持纯进程名（如 `xxx.exe`）、完整路径、通配符及分号多进程匹配，重复规则自动静默去重。
  - 每条规则均支持配置 **目标主机**、**目标端口**、**目标域名**（默认 `*`）、**协议（`BOTH` / `TCP` / `UDP`）** 与 **动作（`PROXY` / `DIRECT` / `BLOCK`）**。
- **代理核心增强与网络防护**：
  - 支持配置化 **「向 Mihomo 发送域名（规则模式）」**（默认开启），通过 DNS 嗅探将域名经 SOCKS5 `ATYP_DOMAIN` 传给 Mihomo；支持「Mihomo 不可用时改为直连（Fail-Open）」。
  - 自动直连放行环回地址（`127.0.0.0/8`、`::1`）、本机网卡 IPv4/IPv6 访问 Mihomo 本地监听端口（`mixed-port` / `socks-port` / `port` 等）以及局域网私网 DNS（RFC1918 UDP 53），避免代理回环与私网 DNS 超时。
  - 提权 Helper 启动时自动维护 Windows 防火墙入站放行规则，确保专用/公用网络下本地中继稳定通信。

### 2. 原生 Win32 + Direct2D / DirectWrite 实时流量 OSD
- **双模式独立开关**：支持 **全局流量 OSD** 与 **应用窗口伴随 OSD**（可分别通过界面或托盘菜单切换，最小化到托盘后持续工作）。
- **无干扰体验**：原生 Direct2D/DirectWrite 渲染，支持鼠标穿透、不抢占窗口焦点、窗口最小化/切屏即时隐藏、进程树子进程流量聚合以及 0ms 窗口跟随。
- **12 锚点与高度自定义**：支持 **应用外**（左上/中上/右上/左下/中下/右下）与 **应用内**（左上/中上/右上/左下/中下/右下）共 12 个锚点，并可在「应用代理设置」中实时预览和调整字号、缩放、不透明度、配色、刷新频率、延迟指标与位置偏移。

### 3. Windows 构建工作流
- 本地便携版构建：`pwsh -NoProfile -File scripts/portable.ps1`（自动拉取并校验 WinDivert 2.2.2、自动检测本机 Visual Studio 版本编译 `ProxyBridgeCore.dll`）。
- GitHub Actions 手动构建：[.github/workflows/windows-setup.yml](./.github/workflows/windows-setup.yml) 支持免 tag 手动触发，自动拉取 `third-party/ProxyBridge` 子模块并产出带应用代理功能的 Windows x64 NSIS 安装包（及可选便携版 ZIP）上传至 Artifacts。

---

## Features

- 基于性能强劲的 Rust 和 Tauri 2 框架
- 内置[Clash.Meta(mihomo)](https://github.com/MetaCubeX/mihomo)内核，并支持切换 `Alpha` 版本内核。
- 简洁美观的用户界面，支持自定义主题颜色、代理组/托盘图标以及 `CSS Injection`。
- 配置文件管理和增强（Merge 和 Script），配置文件语法提示。
- 系统代理和守卫、`TUN(虚拟网卡)` 模式。
- 可视化节点和规则编辑
- WebDav 配置备份和同步

### FAQ

Refer to [Doc FAQ Page](https://clash-verge-rev.github.io/faq/windows.html)

### Donation

[捐助Clash Verge Rev的开发](https://github.com/sponsors/clash-verge-rev)

## Development

See [CONTRIBUTING.md](./CONTRIBUTING.md) for more details.

To run the development server, execute the following commands after all prerequisites for **Tauri** are installed:

```shell
pnpm i
pnpm run prebuild
pnpm dev
```

`pnpm dev` preserves the Development Channel's installed service state: an
existing service is used, while a previously uninstalled service remains
uninstalled and the app starts in Sidecar mode. Use `pnpm dev:service` to
explicitly install or update the isolated development service before launch,
or `pnpm dev:sidecar` to force the unprivileged Sidecar workflow.

## Contributions

Issue and PR welcome!

## References & Acknowledgement

This fork and Clash Verge Rev are based on or inspired by the following projects:

- [clash-verge-rev/clash-verge-rev](https://github.com/clash-verge-rev/clash-verge-rev): Upstream Clash Verge Rev project.
- [InterceptSuite/ProxyBridge](https://github.com/InterceptSuite/ProxyBridge): Lightweight open-source universal transparent proxy client (TCP/UDP per-application routing core and OSD reference).
- [suhli/ProxyBridge](https://github.com/suhli/ProxyBridge): Integrated ProxyBridge submodule fork (`verge-integration` branch) used for Windows application proxy.
- [basil00/WinDivert](https://github.com/basil00/WinDivert): Windows Packet Divert driver (`v2.2.2`) used for user-mode packet capture and redirection.
- [zzzgydi/clash-verge](https://github.com/zzzgydi/clash-verge): A Clash GUI based on tauri. Supports Windows, macOS and Linux.
- [tauri-apps/tauri](https://github.com/tauri-apps/tauri): Build smaller, faster, and more secure desktop applications with a web frontend.
- [Dreamacro/clash](https://github.com/Dreamacro/clash): A rule-based tunnel in Go.
- [MetaCubeX/mihomo](https://github.com/MetaCubeX/mihomo): A rule-based tunnel in Go.
- [Fndroid/clash_for_windows_pkg](https://github.com/Fndroid/clash_for_windows_pkg): A Windows/macOS GUI based on Clash.
- [vitejs/vite](https://github.com/vitejs/vite): Next generation frontend tooling. It's fast!

## Privacy

Clash Verge Rev 不收集任何用户数据，配置与日志仅保存在本地。详见[隐私政策](./PRIVACY.md)。

Clash Verge Rev does not collect any user data; configuration and logs stay on
your own device. See the [Privacy Policy](./PRIVACY.md) for details.

## License

GPL-3.0 License. See [License here](./LICENSE) for details.
