---
title: 简介
description: AIPass 是什么、各组件如何协作，以及保险库如何保护你的 AI 凭据。
navTitle: 简介
order: 1
---

# 简介

AIPass 是面向 AI 工作流的本地优先凭据管理器。它将 AI 服务商的 API 密钥保存在本机的端到端加密保险库中，并帮助你安全地配置 Codex、Claude Code、Gemini CLI 等工具。

产品由三个共用一个保险库的部分组成：

- **桌面应用** —— 基于 Tauri 的 macOS 应用，用于添加、搜索、探测和归档服务商凭据，支持单服务商多密钥管理，以及加密备份的导出与导入。
- **CLI** —— `aipass` 命令行工具，用于脚本化保险库操作、向工具环境注入凭据，以及可回滚地配置 AI CLI 工具。
- **浏览器扩展** —— Chrome 扩展，可识别 AI 服务商控制台，仅在你通过桌面应用授权限时许可后，才会经由 Native Messaging 填充密钥。

## 支持的服务商

OpenAI、Anthropic、Gemini、Azure OpenAI、AWS Bedrock、OpenRouter、DeepSeek、Qwen、Moonshot、Zhipu、Volcengine Ark、Together、Fireworks、Groq、New API、One API、LiteLLM、sub2api，以及自定义 OpenAI 兼容端点和自定义 HTTP API。

## 保险库如何保护密钥

每条服务商记录都作为完整的加密信封存储——标题、域名、端点、认证方式和 API 密钥永远不会以明文写入保险库或同步文件。

- Argon2id 主密码密钥派生（新保险库默认 64 MiB 内存、2 轮迭代）。
- XChaCha20-Poly1305 认证加密，使用 256 位密钥。
- 随机 256 位保险库根密钥，由密码派生密钥和应急恢复密钥双重包裹；恢复密钥仅在创建保险库时显示一次。
- 每条记录使用随机数据密钥，由可轮换的保险库纪元密钥包裹。
- 浏览器填充采用限时授权，过期授权会被加密擦除。
- 通过 HMAC 指纹实现 API 密钥搜索，无需存储明文密钥。

同步（本地/iCloud 文件夹或 WebDAV）只复制加密对象。

## 下一步

- [快速开始](/docs/zh/quick-start)——安装应用并保存第一个密钥。
- [安装](/docs/zh/installation)——桌面应用与浏览器扩展的安装细节。
- [设置与更新](/docs/zh/settings-and-updates)——发布渠道与自动更新行为。
