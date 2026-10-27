---
title: 设置
description: 桌面设置面板——锁定策略、密码与密钥轮换、同步、设备与本地代理。
navTitle: 设置
order: 9
---

# 设置

桌面应用的**设置**面板集中了保险库的所有运维操作。本页逐项介绍；自动更新行为与发布渠道见[更新](/docs/zh/updates)。

## 外观

主题与显示偏好。语言选择会由本地 Agent 统一提供给桌面应用、托盘和浏览器扩展；选择“跟随系统”时三端使用同一个系统语言解析结果。

## 锁定策略

- **自动锁定** —— 空闲 15 分钟、30 分钟、1 小时（默认）、2、4、8、24 小时后锁定保险库，或从不。
- **睡眠时锁定**（默认开启）和**锁屏时锁定**（默认开启）。

锁定会从代理内存中丢弃解密后的密钥；之后桌面应用、CLI 和浏览器扩展都需要重新输入主密码。

## 凭据与剪贴板

点击 API Key 默认显示明文。在**设置 → 安全**中可将点击行为切换为**显示**或**复制**，也可关闭默认开启的“复制凭据前确认”。独立复制按钮遵循同一确认设置。

复制的凭据默认在 45 秒后清除，可设为 1–600 秒。原生清理在应用处于后台时继续运行，锁定或退出时也会执行；仅清除仍与所复制凭据一致的剪贴板文本，保留后来复制的内容。macOS 和 Windows 会收到排除剪贴板历史的标记，但第三方剪贴板管理器可能忽略这些标记。

## 主密码

在应用中修改主密码，或使用：

```bash
aipass vault change-password --new-password "$NEW"
```

修改密码会用新密码派生的密钥重新包裹保险库根密钥——记录无需重新加密，操作很快。恢复密钥保持有效。

## 轮换密钥

轮换保险库纪元密钥，并在新纪元密钥下重新包裹每条记录的数据密钥。CLI 方式：

```bash
aipass vault rotate
```

旧纪元密钥无法解密轮换后写入的记录。吊销设备或使用恢复密钥恢复时也会自动轮换。见[安全架构](/docs/zh/security)。

## 同步

已签名的 macOS 桌面构建默认使用 CloudKit，也可使用其他同步目标：

- **本地文件夹** —— 任意目录，包括已被其他工具同步的目录。
- **WebDAV** —— URL 加用户名和密码。

CLI 还提供 `--icloud`（iCloud Drive，仅限 macOS）和 `--onedrive`：

```bash
aipass sync --dir ~/Sync/AIPass
aipass sync --icloud
aipass sync --onedrive
aipass sync --webdav-url https://cloud.example/dav --webdav-username u --webdav-password p
```

同步传输加密快照，并发修改在同步设置中处理；历史快照可能保留旧版本。订阅登录凭据仅保存在各设备本机。

## 设备、导出与导入

每台打开过保险库的机器都会注册一条加密的设备记录。CLI 方式：

```bash
aipass vault devices                     # 列出受信任设备
aipass vault revoke-device <device-id>   # 吊销并轮换纪元
aipass vault export --output backup.aipexport --export-password "$PW"
aipass vault import --input backup.aipexport --export-password "$PW"
```

导出文件由独立的导出密码加密；导入只适用于不存在保险库的目录。

## Server（本地代理）

在**本地代理**中配置监听地址、路由令牌、目标、重试等待策略及模型价格。传输和失败行为见[本地代理](/docs/zh/local-proxy)。

本地存活检测使用 `GET /health`（例如 `http://127.0.0.1:8787/health`）。该接口由本地代理直接返回，不会转发到任何上游。
