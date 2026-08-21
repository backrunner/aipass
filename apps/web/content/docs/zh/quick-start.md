---
title: 快速开始
description: 安装 AIPass、创建保险库、保存密钥，并配置你的第一个 AI 工具。
navTitle: 快速开始
order: 2
---

# 快速开始

## 1. 安装桌面应用

从[首页](/)下载 macOS 版 AIPass——按钮直接指向最新的 GitHub Release。打开 `.dmg` 并将 AIPass 拖入「应用程序」。

## 2. 创建保险库

首次启动时，AIPass 会要求你设置主密码。随后应用创建加密保险库，并**只显示一次恢复密钥**——形如 `AIPASS-XXXX-XXXX-…` 的字符串。请立即将其离线妥善保存：它是忘记主密码后唯一的找回途径，且无法再次查看。

终端中的等价操作：

```bash
aipass init
```

`aipass init` 只打印一次恢复密钥，并通过 `--password` 或 `AIPASS_MASTER_PASSWORD` 环境变量接收密码。保险库默认位于 `~/Library/Application Support/dev.aipass.desktop/vault`，可用 `--vault` 或 `AIPASS_VAULT_DIR` 覆盖。

## 3. 添加服务商凭据

点击 **Add provider**，选择服务商（例如 Anthropic），填写端点、认证方式和 API 密钥。一条记录可以挂载多个密钥，不再使用的记录可以归档。

更喜欢终端？CLI 可以做同样的事：

```bash
aipass add \
  --title 'Anthropic Prod' \
  --provider anthropic \
  --domain console.anthropic.com \
  --endpoint https://api.anthropic.com \
  --interface anthropic-messages \
  --auth x-api-key \
  --api-key "$ANTHROPIC_API_KEY"
```

命令会打印新记录的 UUID。如果保险库处于锁定状态，CLI 会提示输入主密码（或读取 `--password` / `AIPASS_MASTER_PASSWORD`）。

验证密钥在真实端点上是否可用：

```bash
aipass probe <entry-id>
```

## 4. 配置 AI 工具

通过桌面应用的 Integrations 区域或 `aipass configure`，让 Codex、Claude Code、Gemini CLI 或 OpenCode 使用已保存的凭据。不带 `--yes` 时命令只打印变更预览；带 `--yes` 才会实际写入，并为旧状态保存加密备份：

```bash
# 预览
aipass configure claude-code <entry-id>

# 应用
aipass configure claude-code <entry-id> --yes
```

应用输出中包含一个操作 ID。如果出现问题，可恢复之前的配置：

```bash
aipass rollback <operation-id>
```

Helper 模式（Claude Code 的默认模式）不会把密钥写入磁盘——它让工具调用 `aipass get`，在运行时从保险库取密钥。模式与各工具的细节见 [CLI 参考](/docs/zh/cli)。

## 5. 安装浏览器扩展

从 Chrome 应用商店安装 AIPass 扩展，然后与桌面应用配对：

```bash
aipass native-host install --extension-id <chrome-extension-id>
```

在服务商控制台页面打开扩展弹窗，选择匹配的记录并点击填充。保险库必须处于解锁状态；每次填充都使用短时、绑定来源的授权。见[扩展指南](/docs/zh/extension)。

至此配置完成。密钥集中保存在一个加密保险库中，每个工具都按你的规则获取密钥。
