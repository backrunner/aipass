---
title: 简介
description: AIPass 是什么、各组件如何协作，以及保险库如何保护你的 AI 凭据。
navTitle: 简介
order: 1
---

# 简介

AIPass 是面向 AI 工作流的本地优先凭据管理器。它将 AI 服务商的 API 密钥保存在本机的端到端加密保险库中，并帮助你安全地配置 Codex、Claude Code、Gemini CLI 等工具。

产品由三个共用一个保险库的部分组成：

- **桌面应用** —— 基于 Tauri 的 macOS 应用，用于添加、搜索、探测和归档服务商凭据，支持单条记录多密钥管理、通过本地代理路由流量，以及加密备份的导出与导入。
- **CLI** —— `aipass` 命令行工具，用于脚本化保险库操作、向工具环境注入凭据，以及可回滚地配置 AI CLI 工具。见 [CLI 参考](/docs/zh/cli)。
- **浏览器扩展** —— Chrome 扩展，可识别 AI 服务商控制台，通过 Native Messaging 使用短时、绑定来源的授权来填充密钥。见[扩展指南](/docs/zh/extension)。

后台**代理（agent）**进程持有已解锁的保险库会话。桌面应用、CLI 和浏览器扩展的 Native Host 都通过本地套接字与同一个代理通信，因此只需解锁一次，所有入口共享会话，直到锁定为止。

## 支持的服务商

内置注册表包含 OpenAI、Anthropic、Gemini、Azure OpenAI、AWS Bedrock、OpenRouter、DeepSeek、Moonshot、Qwen、Zhipu、Volcengine Ark、Together、SiliconFlow、xAI、Mistral、Cohere、Perplexity、Cerebras、NVIDIA、Novita、MiniMax、Hugging Face、Fireworks、Groq、Replicate、New API、One API、LiteLLM、sub2api、Veloera、OmniRoute、Metapi，以及自定义 OpenAI 兼容端点和自定义 HTTP API。记录也可以不关联任何服务商 ID——注册表用于图标、控制台识别和合理默认值，并非硬性要求。

## 保险库如何保护密钥

每条服务商记录都作为完整的加密信封存储——标题、域名、端点、认证方式和 API 密钥永远不会以明文写入保险库或同步文件。

- Argon2id 主密码密钥派生（新保险库使用 64 MiB 内存、2 轮迭代、并行度 1）。
- XChaCha20-Poly1305 认证加密，256 位密钥，随机 192 位随机数。
- 随机 256 位保险库根密钥，由密码派生密钥和应急恢复密钥双重包裹；恢复密钥仅在创建保险库时显示一次。
- 每条记录使用随机数据密钥，由可轮换的保险库纪元密钥包裹。
- 浏览器填充授权 120 秒后过期；过期授权会被加密擦除。
- 通过 HMAC-SHA256 指纹实现 API 密钥搜索，无需存储明文密钥。

同步（本地文件夹、iCloud Drive、OneDrive 或 WebDAV）只复制加密对象。完整模型见[安全架构](/docs/zh/security)。

## 下一步

- [快速开始](/docs/zh/quick-start)——安装应用并保存第一个密钥。
- [安装](/docs/zh/installation)——桌面应用、CLI 与浏览器扩展的安装细节。
- [CLI 参考](/docs/zh/cli)——每个 `aipass` 命令的旗标与示例。
- [桌面应用](/docs/zh/desktop)——解锁、托盘、开机自启、集成与本地代理。
- [浏览器扩展](/docs/zh/extension)——配对、填充授权流程与密钥识别。
- [安全架构](/docs/zh/security)——加密、恢复密钥、轮换、设备、导出/导入。
- [更新渠道](/docs/zh/update-channels)——官方与 Beta 订阅源及切换方式。
- [设置与更新](/docs/zh/settings-and-updates)——设置面板与自动更新行为。
