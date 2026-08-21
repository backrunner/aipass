---
title: 安装
description: 下载并安装 AIPass 桌面应用、CLI 与浏览器扩展。
navTitle: 安装
order: 3
---

# 安装

## 桌面应用（macOS）

从[下载区](/)或直接从 [GitHub Releases](https://github.com/backrunner/aipass/releases) 下载安装包。单个**通用构建**（文件名中含 `universal`）同时支持 Apple silicon 和 Intel Mac——下载页会检测你的系统并提供一个按钮。

打开 `.dmg` 并将 AIPass 拖入「应用程序」。正式构建使用 Developer ID 证书签名并经过 Apple 公证，Gatekeeper 可直接打开。如果你安装的是未签名的本地构建，macOS 会阻止首次启动——在**系统设置 → 隐私与安全性**中允许即可。

Windows 支持正在准备中，下载页上标注为 **Coming soon**。

首次启动时应用会要求设置主密码、创建保险库，并显示一次恢复密钥。应用还会注册后台代理，让保险库会话在应用重启后仍然保留。

## CLI

`aipass` CLI 随桌面应用和代码仓库一起提供。用以下命令验证：

```bash
aipass --help
aipass doctor
```

`aipass doctor` 检查保险库清单、代理可达性、Native Host 二进制、已安装的浏览器清单以及扩展白名单——出现异常时先运行它。加 `--json` 可获得机器可读的报告。

用 `aipass completions <shell>` 打印 shell 补全脚本（bash、zsh、fish 等）。

CLI 与桌面应用使用同一个后台代理。代理的生命周期命令：

```bash
aipass agent install    # 注册代理开机自启（macOS 上为 LaunchAgent）
aipass agent status     # 注册/运行状态及锁定状态
aipass agent start
aipass agent stop
aipass agent uninstall
```

通常你不需要这些命令——CLI 会按需启动代理——但 `agent install` 可以让会话在登录后保持可用。

## 浏览器扩展（Chrome）

1. 从 Chrome 应用商店安装 AIPass 扩展。如果商店尚未上架，从 [GitHub Releases](https://github.com/backrunner/aipass/releases) 下载扩展包，在 `chrome://extensions` 开启开发者模式后加载。
2. 将扩展连接到桌面应用。应用会注册 Chrome Native Messaging Host；也可以用 CLI 完成：

```bash
aipass native-host install --extension-id <chrome-extension-id>
```

安装器会写入带有扩展白名单的 Native Messaging 清单 `dev.aipass.native.json`——在 macOS 上位于 `~/Library/Application Support/Google/Chrome/NativeMessagingHosts/`——并为 Native Host 记录允许的扩展 ID。通过 `--browser` 支持 Chromium、Edge 和 Brave：

```bash
aipass native-host install --browser brave --extension-id <id1>,<id2>
```

多次传入 `--extension-id` 或用逗号分隔，可允许多个扩展构建（例如商店版和开发版）。桌面应用的扩展设置中也能查看 Native Host 状态并修复清单。

此后扩展即可识别服务商控制台，并从已解锁的保险库请求填充授权。完整流程见[扩展指南](/docs/zh/extension)。

## 同步（可选）

要跨机器复制保险库，配置一个同步目标：

```bash
aipass sync --dir ~/Sync/AIPass                 # 任意本地文件夹
aipass sync --icloud                            # iCloud Drive（macOS）
aipass sync --onedrive                          # OneDrive 文件夹
aipass sync --webdav-url https://cloud.example/dav \
  --webdav-username "$USER" --webdav-password "$PASS"
```

同步只会传输加密对象——目标位置永远看不到明文密钥。iCloud 同步写入 iCloud Drive 中的 `AIPass` 文件夹，且仅限 macOS。冲突（同一对象在两台机器上被修改）会被隔离，并在桌面应用的同步设置中解决。
