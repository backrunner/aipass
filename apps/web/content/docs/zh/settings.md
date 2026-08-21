---
title: 设置
description: 桌面设置面板——锁定策略、密码与密钥轮换、同步、设备与本地代理。
navTitle: 设置
order: 9
---

# 设置

桌面应用的**设置**面板集中了保险库的所有运维操作。本页逐项介绍；自动更新行为与发布渠道见[更新](/docs/zh/updates)。

## 外观

主题与显示偏好。仅作用于本应用，不影响保险库。

## 锁定策略

- **自动锁定** —— 空闲 15 分钟、30 分钟、1 小时（默认）、2、4、8、24 小时后锁定保险库，或从不。
- **睡眠时锁定**（默认开启）和**锁屏时锁定**（默认开启）。

锁定会从代理内存中丢弃解密后的密钥；之后桌面应用、CLI 和浏览器扩展都需要重新输入主密码。

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

选择一个同步目标：

- **本地文件夹** —— 任意目录，包括已被其他工具同步的目录。
- **WebDAV** —— URL 加用户名和密码。

CLI 还提供 `--icloud`（iCloud Drive，仅限 macOS）和 `--onedrive`：

```bash
aipass sync --dir ~/Sync/AIPass
aipass sync --icloud
aipass sync --onedrive
aipass sync --webdav-url https://cloud.example/dav --webdav-username u --webdav-password p
```

同步只传输加密对象。当同一对象在两台机器上被修改时，冲突会被隔离并列在同步设置中，你可以逐个**接受**（保留传入版本）或**丢弃**（保留当前版本）。

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

内置代理的设置：绑定地址（默认 `127.0.0.1:8787`）、带重试策略的路由（最大尝试 1–10 次、失败阈值 1–20、熔断秒数、连接 / 首字节 / 流空闲超时），以及用于成本估算的模型定价表。见[桌面应用](/docs/zh/desktop)。
