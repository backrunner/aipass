---
title: 安装
description: 安装桌面应用、独立 CLI 和 Chromium 浏览器扩展。
navTitle: 安装
order: 3
---

# 安装

从 [GitHub Releases](https://github.com/backrunner/aipass/releases) 下载。每个 release 都列出平台制品与变更；官方、Beta 和 Nightly 的区别见[更新](/docs/zh/updates)。

## 桌面应用

### macOS

下载 `AIPass-macOS-<version>.dmg`。Universal 应用同时支持 Apple silicon 和 Intel Mac。打开 DMG，将 AIPass 复制到 Applications 后启动。公开的 macOS 应用和 DMG 均经过签名与公证。

### Linux

按机器架构选择 x86_64 或 aarch64：

- Debian/Ubuntu 系统使用 `.deb`。
- RPM 系统使用 `.rpm`。
- 便携运行可用 `.AppImage`，启动前需授予执行权限。

使用适合发行版的包格式与权限。没有 systemd 用户服务的会话可使用桌面会话自启后端。

### Windows

提供独立 Windows CLI 压缩包。Windows 桌面安装包会在 CI 中验证；请以所选 release 实际附带的制品为准。

首次启动时创建保险库，并离线保存只显示一次的恢复密钥。步骤见[快速开始](/docs/zh/quick-start)。

## 独立 CLI

解压 `aipass-cli-macos.tar.gz`、`aipass-cli-linux.tar.gz` 或 `aipass-cli-windows.zip`。将 `aipass`、`aipass-agent` 和 `aipass-native-host` 保留在同一目录内（Windows 带 `.exe` 后缀），并把该目录加入 `PATH`。

```bash
aipass --help
aipass doctor
aipass agent status
```

需要时 CLI 会启动同目录的 Agent。`doctor` 检查保险库、Agent、Native Host、浏览器注册及扩展允许列表；加上 `--json` 可获得机器可读输出。

使用 `aipass agent install` 设置登录自启；`start`、`stop` 和 `uninstall` 分别管理启动、停止与移除。`aipass completions <shell>` 输出 shell 补全。

## 浏览器扩展

下载并解压 `aipass-edge-extension.zip`。在 `edge://extensions` 或 `chrome://extensions` 打开开发者模式，加载解压后的文件夹。使用已安装扩展的 ID 注册 Native Messaging：

```bash
aipass native-host install --extension-id <extension-id>
aipass native-host install --browser edge --extension-id <extension-id>
```

支持 Chrome、Chromium、Edge 和 Brave。桌面应用也提供扩展状态与修复操作。填充凭据时需要解锁保险库；详见[扩展指南](/docs/zh/extension)。

## 同步与新设备

已签名的 macOS 桌面构建默认使用 CloudKit。文件夹和 WebDAV 目标复用同一加密快照引擎。CLI 还提供显式文件夹、OneDrive 和旧 iCloud Drive 路径，见 [CLI 参考](/docs/zh/cli#同步)。

同步和备份不包含设备本地的订阅登录凭据。新设备或恢复后需要分别登录这些账号，详见[安全](/docs/zh/security#同步)。
