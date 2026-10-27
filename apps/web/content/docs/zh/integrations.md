---
title: 工具集成
description: 配置已安装的 AI 工具、选择凭据、连接订阅与恢复旧配置。
navTitle: 工具集成
order: 6
---

# 工具集成

在凭据或本地代理路由中选择**快速接入**，即可配置已安装的 AI 工具。桌面应用与 Agent 管理共用同一工具目录；可用操作取决于安装情况、工具能力和凭据协议。

## 使用已保存的凭据

1. 选择凭据并打开**快速接入**。
2. 选择已安装的工具和密钥。条目包含多个密钥时，需要明确选择一个。
3. 检查端点、协议、模型及文件变更，再应用配置。

密钥的 API 格式、端点和模型覆盖决定生成的配置。应用前会加密备份旧配置，便于回滚。工具自身的账号或版本要求仍然适用。

CLI 也支持 Codex、Claude Code、Gemini CLI、OpenCode、Grok、Pi 和 Cursor Agent Local：

```bash
aipass configure claude-code <entry-id>          # 预览
aipass configure claude-code <entry-id> --yes    # 应用
aipass secret list <entry-id>                   # 查看密钥 ID
aipass configure codex <entry-id> --secret-id <key-id> --yes
aipass rollback <operation-id>
```

配置模式见 [CLI 参考](/docs/zh/cli#配置-ai-工具)。Helper 模式在运行时从 AIPass 读取密钥；plaintext 模式会按你的选择将密钥写入工具自己的配置。

## 使用代理路由

在**本地代理**的已启用路由中选择**快速接入**。工具使用路由的本地令牌，由 AIPass 提供上游凭据。先为路由中的凭据设置默认模型。

Grok Build 可使用 Chat Completions 或 Responses 路由。AIPass 会将带版本的 API 地址、匹配的 `api_backend`、模型及路由令牌写入 `~/.grok/config.toml`。

传输选择、协议转换和安全回退规则见[本地代理](/docs/zh/local-proxy)。

## 连接订阅

选择**连接订阅**和服务商。不同服务商使用官方 CLI 登录、浏览器或设备授权，或导入已有登录状态。登录方式需要官方 CLI 时，请先安装对应工具。

Rust Agent 负责订阅适配、模型发现、额度刷新和凭据轮换。**更多服务商**包括 ZCode、Qoder、Devin、Zed、Factory、Grok、Command Code、小米 MiMo、Cursor、Kiro 和 WorkBuddy 的不同版本；具体协议与能力以服务商为准。

订阅登录凭据仅在本机加密保存，不参与同步或保险库备份。新设备或备份恢复后需要分别登录；账号条目和路由配置仍可同步。额度显示不代表订阅资格或真实付费请求已获得服务商认可。

## 导入 CC Switch 配置

在**设置**中使用 CC Switch 导入操作，发现受支持的 API 配置，包括 SQLite 和 OpenCode 来源。不同密钥、模型、端点和协议会保留为不同配置。重复导入相同配置时会跳过；没有受支持 API 密钥的条目也会跳过。

导入只读取来源，不修改原配置。即使成功发现配置，也可能因全部被跳过而没有新增条目。
