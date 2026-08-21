---
title: 快速开始
description: 安装 AIPass、创建保险库、保存密钥并配置第一个 AI 工具。
navTitle: 快速开始
order: 2
---

# 快速开始

## 1. 安装桌面应用

从[首页](/zh)下载 macOS 版 AIPass——下载按钮直接指向最新的 GitHub Release。打开 `.dmg` 并将 AIPass 拖入「应用程序」。

## 2. 创建保险库

首次启动时，AIPass 会要求你设置主密码。随后应用会创建加密保险库，并**仅显示一次恢复密钥**。请妥善保存——忘记主密码时，这是唯一的找回方式。

## 3. 添加服务商凭据

点击「添加服务商」，选择服务商（例如 Anthropic），填写端点、认证方式和 API 密钥。一个服务商条目可以挂载多个密钥，不再使用的条目可以归档。

更习惯终端？CLI 可以完成同样的操作：

```bash
aipass init
aipass add \
  --title 'Anthropic Prod' \
  --provider anthropic \
  --domain console.anthropic.com \
  --endpoint https://api.anthropic.com \
  --interface anthropic-messages \
  --auth x-api-key \
  --api-key "$ANTHROPIC_API_KEY"
```

## 4. 配置 AI 工具

通过桌面应用或 `aipass configure`，将 Codex、Claude Code 或 Gemini CLI 指向已保存的凭据。AIPass 会写入工具配置并对之前的配置做加密备份，出现问题时用 `aipass rollback` 一键恢复。

```bash
aipass configure codex --entry <entry-id>
```

## 5. 安装浏览器扩展

从 Chrome 应用商店安装 AIPass 扩展，然后将其与桌面应用连接。扩展只有在你批准限时授权后，才会向服务商控制台填充密钥——Native Messaging 的设置见[安装](/docs/zh/installation)。

至此配置完成：密钥集中保存在一个加密保险库中，每个工具都在你的授权下取用。
