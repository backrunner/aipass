---
title: 桌面应用
description: 解锁与锁定行为、托盘、开机自启、集成与本地代理。
navTitle: 桌面应用
order: 5
---

# 桌面应用

桌面应用是保险库的主要界面。它是基于 Tauri 的桌面应用，与 CLI 使用同一个后台代理，因此在应用中解锁后，`aipass` 和浏览器扩展也处于解锁状态。

## 保险库、解锁与锁定

首次启动时，应用会要求设置主密码、创建保险库，并显示一次恢复密钥——请立即离线保存。之后的启动会显示解锁界面；输入主密码即可解锁共享的代理会话。

保险库会按照**设置 → 锁定策略**自动锁定：

- **空闲自动锁定**：15 分钟、30 分钟、1 小时（默认）、2、4、8、24 小时，或从不。
- **睡眠时锁定**（默认开启）—— 电脑睡眠时锁定保险库。
- **锁屏时锁定**（默认开启）—— 锁定屏幕时锁定保险库。

退出应用或重启代理也会锁定会话。锁定是密码学意义上的：代理会从内存中丢弃解密后的密钥，因此在重新解锁前，任何入口（包括浏览器扩展）都无法读取密钥。

如果忘记主密码，可在解锁界面使用恢复密钥。恢复会设置新的主密码、显示**新的**恢复密钥，并轮换保险库纪元——旧恢复密钥随即失效。

## 主窗口

侧边栏组织你的记录：

- **保险库** —— 全部条目、收藏、最近使用。
- **服务商** —— 按类型分组：官方、第三方、自托管、自定义。
- **存储** —— Server（本地代理）、归档、回收站。

服务商详情面板显示端点、控制台 URL、掩码密钥、模型别名、配额、标签和备注，并提供复制、显示、探测、配置、归档和删除操作。每条记录可以持有多个带标签的密钥。

外部工具可以使用 AIPass 原生 deep link 打开添加服务商表单：

`aipass-provider://v1/add?title=Relay&providerId=custom_http&domain=relay.example.com&endpoint=https%3A%2F%2Frelay.example.com%2Fv1&interfaceType=openai_compatible&authScheme=bearer&apiKey=...`

路径 `v1/add` 已版本化。多个 `domain`、`endpoint`、`consoleEndpoint` 和 `tag` 可重复传入；`modelAliases`、`headers`、`quota` 使用 JSON 查询参数并进行 URL 编码。链接只会打开现有添加表单，最终记录仍由 Rust agent 负责持久化。

## 集成

从凭据或代理路由的**快速接入**配置已安装的 AI 工具。应用前检查计划，旧配置会加密备份以供回滚。CLI 支持的工具包括 Codex、Claude Code、Gemini CLI、OpenCode、Grok、Pi 和 Cursor Agent Local。

密钥选择、订阅账号及 CC Switch 导入见[工具集成](/docs/zh/integrations)。

## 本地代理

本地代理让工具使用路由令牌，服务商凭据保留在 AIPass。在**本地代理**中配置路由、目标策略、协议转换、WebSocket 偏好及按密钥计算的用量价格。

会话行为、转换限制、安全回退和局域网控制面板见[本地代理](/docs/zh/local-proxy)。

## 托盘

AIPass 常驻系统托盘或 macOS 菜单栏。托盘菜单显示代理和代理服务器状态，并提供：

- **Open AIPass** / **Hide Window**、**Refresh Status**、**Quit**。
- 启动代理、**Lock Vault**，以及为托盘安装开机自启。
- 代理服务器控制：启动、停止、刷新、打开 Server 页面。托盘还显示近期请求速率（RPM/TPM）。

## 开机自启

Agent 与托盘分别配置自启。macOS 使用 LaunchAgent，Windows 使用用户级启动注册，Linux 使用受支持的会话后端；没有 systemd 用户服务时可回退到桌面会话启动：

- **代理**保持解锁会话，并为 CLI 和浏览器扩展提供服务（`aipass agent install`）。
- **托盘**应用可开机自启，让菜单栏图标始终存在；从托盘菜单安装。

## 更新

应用会在后台检查更新，发现新版本时显示横幅；**设置 → 更新**可在官方、Beta 和 Nightly 渠道之间切换。详见[更新](/docs/zh/updates)。
