---
title: 安装
description: 下载并安装 AIPass 桌面应用与 Chrome 扩展。
navTitle: 安装
order: 3
---

# 安装

## 桌面应用（macOS）

从[下载区](/zh)或直接访问 [GitHub Releases](https://github.com/backrunner/aipass/releases) 获取安装包。发布版本包含两种构建：

- **Apple 芯片** —— 文件名中带 `aarch64` 或 `arm64` 的安装包。
- **Intel** —— 文件名中带 `x64` 或 `x86_64` 的安装包。

打开 `.dmg` 并将 AIPass 拖入「应用程序」。正式构建使用 Developer ID 证书签名并经 Apple 公证，Gatekeeper 可直接打开，无需额外操作。如果你安装的是未签名的本地构建，macOS 会阻止首次启动——请在「系统设置 → 隐私与安全性」中允许运行。

Windows 版本正在准备中，下载页会标注「即将推出」。

## 浏览器扩展（Chrome）

1. 从 Chrome 应用商店安装 AIPass 扩展。如果商店尚未上架，可从 [GitHub Releases](https://github.com/backrunner/aipass/releases) 下载扩展包，在 `chrome://extensions` 开启开发者模式后加载。
2. 将扩展与桌面应用连接。应用会注册 Chrome Native Messaging 主机；也可以通过 CLI 完成：

```bash
aipass native-host install --extension-id <chrome-extension-id>
```

安装器会写入包含扩展白名单的 Chrome 清单。之后扩展即可识别服务商控制台，并向已解锁的保险库请求填充授权。

## CLI

`aipass` CLI 随桌面应用和代码仓库一起发布。用以下命令验证：

```bash
aipass --help
```

常用命令：`init`、`add`、`list`、`get`、`copy`、`secret`、`probe`、`env`、`exec`、`configure`、`rollback`、`sync`、`native-host`，以及用于轮换、恢复、设备管理和加密导出/导入的 `vault` 命令族。
