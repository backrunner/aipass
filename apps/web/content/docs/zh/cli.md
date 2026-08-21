---
title: CLI 参考
description: 每个 aipass 命令的旗标、默认值与示例。
navTitle: CLI
order: 4
---

# CLI 参考

`aipass` CLI 是与桌面应用共用一个保险库和代理的脚本化接口。读取保险库数据的命令都经过后台代理；如果保险库已锁定且终端可交互，CLI 会提示输入主密码，之后会话在代理中保持解锁，供后续命令使用。

## 全局旗标

以下旗标对所有命令有效：

- `--json` —— 以 JSON 而非人类可读文本输出结果。
- `--vault <dir>` —— 使用非默认的保险库目录（macOS 默认为 `~/Library/Application Support/dev.aipass.desktop/vault`）。也可从 `AIPASS_VAULT_DIR` 读取。
- `--password <password>` —— 非交互地提供主密码。也可从 `AIPASS_MASTER_PASSWORD` 读取。脚本中建议使用环境变量，避免密码出现在 shell 历史中。

其他环境变量：`AIPASS_INPUT_API_KEY`（`--api-key` 的值）、`AIPASS_EXPORT_PASSWORD`（`--export-password` 的值）、`AIPASS_EXTENSION_ID`（`--extension-id` 的值）、`AIPASS_WEBDAV_URL` / `AIPASS_WEBDAV_USERNAME` / `AIPASS_WEBDAV_PASSWORD`（同步旗标）、`AIPASS_ALLOWED_EXTENSION_IDS`（逗号分隔的 Native Host 白名单覆盖）。

## 会话与诊断

```bash
aipass init                 # 创建新保险库；只打印一次恢复密钥
aipass login                # 在代理中解锁保险库会话
aipass lock                 # 立即锁定会话
aipass vault status         # 是否存在 / 是否锁定 / 锁定策略 / 保险库目录
aipass doctor               # 健康检查：保险库、代理、Native Host、白名单
aipass completions zsh      # 打印 shell 补全（bash、zsh、fish 等）
```

`init` 需要通过 `--password` 或 `AIPASS_MASTER_PASSWORD` 提供密码。`doctor` 是只读的，可随时安全运行。

## 管理记录

```bash
aipass add --title 'Anthropic Prod' --provider anthropic \
  --domain console.anthropic.com \
  --endpoint https://api.anthropic.com \
  --interface anthropic-messages --auth x-api-key \
  --api-key "$KEY"
```

`add` 的必填旗标：`--title`、`--interface`、`--auth`、`--api-key`。可选旗标：

- `--provider <id>` —— 注册表中的服务商 ID（例如 `anthropic`）。省略时会根据第一个 `--domain` 猜测。
- `--domain <host>`（可重复）和 `--console-url <url>`（可重复）——供浏览器扩展识别控制台。
- `--endpoint <url>` —— API 基础 URL。
- `--favicon-url <url>`、`--notes <text>`、`--tag <tag>`（可重复）。
- `--default-model <model>` 和 `--model-alias alias=model`（可重复）。
- `--header name=value`（可重复）—— 随请求发送的额外请求头。
- `--quota-label`、`--quota-limit`、`--quota-remaining`、`--quota-reset-at` —— 配额展示元数据。

`--interface` 接受 `openai-compatible`、`anthropic-messages`、`gemini`、`azure-openai`、`bedrock`、`custom-http`。`--auth` 接受 `bearer`、`x-api-key`、`google-api-key`、`azure-api-key`、`aws-profile`、`custom-header`。

```bash
aipass list                        # 活跃记录
aipass list --provider anthropic   # 按服务商 ID 过滤
aipass list --archived             # 仅已归档记录
aipass list --all                  # 活跃 + 已归档
aipass search 'claude'             # 搜索标题、域名、指纹

aipass update <id> --title 'New title' --endpoint https://api2.example.com
aipass archive <id>                # 移入归档（可恢复）
aipass restore <id>                # 取消归档
aipass delete <id> --yes           # 永久删除；必须加 --yes
```

`update` 接受与 `add` 相同的旗标（均为可选）；未提供的字段保持原值。`update <id> --api-key "$KEY"` 用于轮换主密钥。

## 单条记录多密钥

一条记录可以持有多个带标签的密钥（例如每个网关分组一个密钥）：

```bash
aipass secret list <id>
aipass secret add <id> --label backup --api-key "$SECOND_KEY"
aipass secret remove <id> --label backup
```

用 `aipass get <id> --field secret:backup --reveal` 显示带标签的密钥。

## 读取与使用密钥

```bash
aipass get <id>                        # 掩码后的主密钥（默认字段：api_key）
aipass get <id> --field api_key --reveal   # 在标准输出打印明文密钥
aipass get <id> --field endpoint       # 基础 URL
aipass get <id> --field curl           # 可直接运行的 curl 片段
aipass get <id> --field env            # 使用 `aipass get --reveal` 的 export 行
aipass get <id> --field config         # 记录的 JSON 摘要
aipass get <id> --field fingerprint    # 密钥的 HMAC 指纹
```

其他字段：`title`、`provider`、`provider_kind`、`domain`、`console_url`、`interface`、`auth`、`default_model`、`tags`、`notes`。密钥字段（`api_key`、`secret:<label>`、`key:<label>`）在未加 `--reveal` 时以掩码打印。

```bash
aipass copy <id>                       # 复制主密钥到剪贴板
aipass copy <id> --field endpoint      # 复制任意字段
aipass probe <id>                      # 对端点做实时检查
aipass probe <id> --timeout-seconds 30 # 默认超时 15 秒
```

## 环境注入

```bash
aipass env <id>                        # 打印 `export NAME='key'`（shell 格式）
aipass env <id> --format json          # 打印 {"NAME": "key"}
aipass exec <id> -- claude             # 在子进程环境中注入密钥并运行命令
aipass inject <id> -- codex --help     # exec 的别名
```

环境变量名取决于服务商：`ANTHROPIC_API_KEY`、`GEMINI_API_KEY`、`OPENROUTER_API_KEY`、`DEEPSEEK_API_KEY`、`MOONSHOT_API_KEY`、`DASHSCOPE_API_KEY`（Qwen）、`ZHIPUAI_API_KEY`、`ARK_API_KEY`（Volcengine）、`GROQ_API_KEY`、`TOGETHER_API_KEY`、`FIREWORKS_API_KEY`、`REPLICATE_API_TOKEN`、`AWS_PROFILE`（Bedrock）、`AZURE_OPENAI_API_KEY`，其余一律为 `AIPASS_API_KEY`。`exec`/`inject` 只为子进程设置这一个变量；密钥不会进入 shell 历史。

## 配置 AI 工具

```bash
aipass configure <tool> <id>           # 预览计划中的变更
aipass configure <tool> <id> --yes     # 应用变更
aipass rollback <operation-id>         # 恢复应用前的状态
```

工具：`codex`、`claude-code`、`gemini-cli`、`opencode`。模式（`--mode`，默认 `helper`）：

- `helper` —— 密钥不落盘。Claude Code 会在 `~/.claude/settings.json` 中写入运行 `aipass get <id> --field api_key --reveal` 的 `apiKeyHelper`；Gemini CLI 会在 `~/.aipass/tools/gemini-cli.env` 中以同样方式导出 `GEMINI_API_KEY`。
- `env` —— 基于环境变量的配置。
- `plaintext` —— 写入真实密钥（例如选择 `--codex-api-key-mode auth-json` 时写入 Codex 的 `config.toml` 和 `auth.json`；另一种选择是 `experimental-bearer-token`）。

应用配置时会把旧文件快照保存为加密的 `.aipbackup`（位于工具配置旁的 `.aipass-backups` 目录），并返回供 `rollback` 使用的操作 ID。

## 保险库维护

```bash
aipass vault change-password --new-password "$NEW"
aipass vault rotate                    # 轮换保险库纪元密钥
aipass vault rotate --reason key.compromise
aipass vault devices                   # 列出受信任设备
aipass vault revoke-device <device-id> # 吊销设备并轮换纪元
aipass vault export --output backup.aipexport --export-password "$EXPORT_PW"
aipass vault import --input backup.aipexport --export-password "$EXPORT_PW"
```

轮换会在新纪元密钥下重新包裹每条记录的数据密钥；旧纪元密钥无法解密轮换后写入的记录。吊销设备总会触发纪元轮换。导出生成由独立导出密码（而非主密码）保护的 `aipass-encrypted-vault-export` 文件；导入只适用于不存在保险库的目录。见[安全架构](/docs/zh/security)。

## 同步

```bash
aipass sync --dir ~/Sync/AIPass
aipass sync --icloud          # iCloud Drive，仅限 macOS
aipass sync --onedrive        # 自动检测 OneDrive 文件夹
aipass sync --webdav-url https://cloud.example/dav \
  --webdav-username u --webdav-password p
```

每次运行只能选择一个目标。同步只复制加密对象（`objects/`、`grants/`、`devices/`）；冲突版本会被隔离，在桌面应用中手动解决。`AIPASS_ICLOUD_ROOT` 和 `AIPASS_ONEDRIVE_ROOT` 可覆盖自动检测的云文件夹。

## Native Host 与代理

```bash
aipass native-host manifest --extension-id <id>          # 打印清单 JSON
aipass native-host install --extension-id <id>           # 为 Chrome 安装（默认）
aipass native-host install --browser edge --extension-id <id>
aipass native-host install --extension-id <id> --output ./dev.aipass.native.json
```

浏览器：`chrome`、`chromium`、`edge`、`brave`。`--host-path` 可覆盖自动检测的 `aipass-native-host` 二进制。

```bash
aipass agent install | uninstall | status | start | stop
```

代理是持有已解锁会话的后台进程。在 macOS 上，`agent install` 注册 LaunchAgent 使其开机自启。桌面应用会替你管理这些；这些命令适用于无头或脚本化环境。
