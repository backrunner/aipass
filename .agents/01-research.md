# AIPass 完整调研

调研时间基于当前工作上下文：2026-05-24，Asia/Shanghai。本文只记录会影响产品和工程决策的信息，不做泛泛资料堆叠。

## 1. 产品与竞品观察

### 1Password 可借鉴点

1Password 的桌面体验核心是三栏工作台：左侧组织入口，中间条目列表，右侧条目详情。官方支持文档强调 sidebar 用于管理 items、vaults、categories、tags、archive/recently deleted，并且打开应用后“所有需要的东西都在手边”。AIPass 应采用相同的高效工作台骨架，但将“密码/登录项”语义替换为“AI Provider 配置/API Key/端点/额度/适配目标”。

可借鉴但不照搬的模式：

- 解锁后直接进入主工作台，不放营销页。
- 左侧低噪音导航，支持收藏、分类、标签、归档、最近使用。
- 顶部全局搜索是第一等能力。
- 条目详情页强调复制、填充、Reveal、编辑、审计，而不是大量说明文案。
- Browser extension 与 Desktop app 共享 unlock 状态，但扩展自身不应持久保存高敏数据。
- CLI 可通过引用或 helper 在运行时取密钥，减少明文写入 shell/env/config。

### API Key 管理产品差异点

AIPass 不是通用密码管理器，而是 AI Provider 配置管理器。差异化应该集中在：

- Provider 语义：官方平台、第三方平台、聚合网关、代理接口、私有部署都能被识别。
- Provider interface 语义：endpoint/base URL、console URL、接口协议、认证方式、model namespace、headers、rate limit、quota、organization/project id。
- Tool 语义：Codex、Claude Code、Gemini CLI、Aider、Continue、Open WebUI 等不同工具的配置写入和回滚。
- 安全语义：API Key 的 reveal/copy/fill/configure 都需要有可审计行为和短期授权。
- 浏览器捕捉：创建新 Key 时自动建议保存，兼容官方控制台和 new-api/one-api/sub2api 一类自托管控制台。

## 2. 技术栈调研

### Tauri v2

Tauri 的架构是 Rust core + 系统 WebView + JS/Rust API message passing。对 AIPass 的价值：

- Rust core 适合实现加密、同步、Native Messaging host、文件写入和跨平台 OS 集成。
- WebView 前端适合用 Svelte 构建高质量 UI。
- v2 的 capability/permission 模型要求显式授权，适合凭证类应用做最小权限。
- 官方 Stronghold plugin 基于 IOTA Stronghold，可作为本地 secret/key 存储候选。
- Single Instance plugin 能防止重复实例，并可在第二次打开时聚焦已有窗口。
- Deep Link plugin 可支持 `aipass://` 唤起、配置回调和 CLI/扩展跳转。
- System Tray 可实现后台常驻与快速访问。
- Autostart 可在系统启动时启动 headless agent 或最小化常驻能力。

建议：

- GUI app 与 headless agent 共享 Rust crate，但不要把所有能力塞进 WebView command。
- 对外本地服务优先使用 Native Messaging 和 Unix domain socket/Windows named pipe，而不是默认打开 localhost HTTP 端口。
- 如果必须提供 localhost API，必须绑定 loopback、随机端口、一次性挑战、Origin 校验和最小 API。

### Svelte + Bits UI + Radix

Svelte 5 的 runes 模型适合建立轻量响应式状态。Bits UI 是 Svelte headless component library，强调 accessibility、developer experience 和 styling freedom，适合 AIPass 这种需要高度自定义但不能牺牲可访问性的桌面 UI。

Radix Primitives 是 React 生态的 headless accessible primitives，不能直接作为 Svelte runtime 依赖使用，但可作为交互规范参考：dialog、popover、dropdown、select、tooltip、focus management、keyboard navigation。

建议：

- Svelte app 使用 Bits UI 作为主要 primitive 层。
- 在 `packages/ui` 封装 AIPass 自己的 design system，不让业务 app 直接依赖散落的 primitive。
- 设计 token 与状态属性采用 Radix/Bits 常见习惯：`data-state`、`data-disabled`、`data-highlighted`。
- 图标使用 lucide-svelte。

### Turbo monorepo

Turborepo 的任务模型、并行执行、缓存和 remote cache 适合 AIPass 的多端工程：desktop、cli、extension、shared packages、Rust crates。官方文档强调 tasks、outputs、inputs 和 deterministic cache。AIPass 需要严格定义 build/test/lint/typecheck 的 outputs，避免缓存失效或误命中。

建议根结构：

```text
apps/
  desktop/
  extension/
  docs/
crates/
  aipass-core/
  aipass-crypto/
  aipass-sync/
  aipass-native-host/
  aipass-cli/
packages/
  ui/
  provider-registry/
  config-writers/
  extension-shared/
  schemas/
  eslint-config/
  tsconfig/
```

## 3. 浏览器扩展通信调研

Chrome Native Messaging 允许扩展通过标准输入/输出与本地 native host 交换 JSON 消息。host manifest 需要声明 name、path、type、allowed_origins；Chrome 会启动 host 进程并用 32-bit length prefix 包裹 UTF-8 JSON。

Manifest V3 的关键影响：

- background context 是 service worker，不能假设长期常驻内存。
- 不允许远程托管代码，extension bundle 必须自包含。
- Content script 与页面 JS 隔离，适合做 DOM 探测和字段建议，但读取页面变量需要谨慎。

建议通信模型：

- Extension service worker 只做协调：tab/domain context、用户动作、Native Messaging port、短期缓存。
- Content script 只做页面探测和注入 UI，不直接接触明文 vault。
- Native host 验证 extension id、origin、协议版本、用户授权和 vault unlock 状态。
- 对每个 domain 建立 capability grant，例如 `read_provider_summary`、`fill_secret_once`、`save_detected_secret`。
- 所有 reveal/fill/save 都走显式用户手势，禁止静默暴露 API Key。

## 4. 加密与密钥管理调研

OWASP Cryptographic Storage 建议使用成熟公开算法、认证加密模式、最小化敏感数据存储，并对密钥管理生命周期做正式设计。OWASP Key Management 强调密钥不应明文存储，应有完整性保护和生命周期管理。Argon2id 是现代 password-based key derivation 的合理选择。

建议 vault 模型：

- Master Password + Argon2id 派生 Master Key。
- 每个 vault 有 Vault Key，Vault Key 由 Master Key 包裹。
- 每个 secret item 有 Data Encryption Key 或使用 record-level nonce 的 AEAD。
- AEAD 候选：RustCrypto `aes-gcm` 或 `chacha20poly1305`；跨平台性能和 nonce 设计优先考虑 XChaCha20-Poly1305。
- 强制记录 `schema_version`、`crypto_version`、`kdf_params`，支持未来迁移。
- API Key 明文只进入内存短期窗口；Reveal/Copy/Fill 后自动清理剪贴板和进程内缓存。
- P0 约束：攻击者即使复制整个 vault 目录、同步目录、备份文件、索引文件和日志，也不能恢复任何 API secret。
- 默认所有业务语义也应加密，包括 title、domain、endpoint/base URL、console URL、provider kind、auth scheme、interface type、tags、quota、notes。只允许文件格式版本、KDF 参数、object id/type、sync lamport/device id 等最小路由元数据明文。
- 搜索索引不能成为旁路明文副本：默认解锁后建立内存索引；磁盘索引要么整体加密，要么只保存不可逆 token。API Key 搜索只支持后四位、HMAC fingerprint、用户自定义 alias，不支持全量明文倒排。
- 前向安全不等于远程失效：如果攻击者已经复制了密文和当时可解密它的 key，后续无法让那份副本失效。AIPass 应通过 per-record DEK、Vault Epoch Key ratchet、TTL key destruction 和 re-encryption，让当前 key 泄露不自动暴露已销毁旧对象，并在泄露后恢复未来安全。

Tauri Stronghold：

- 可用于保存本地设备级 secret，如 wrapped vault key、device key、session unlock token。
- 不建议把完整业务数据模型全部绑定在 Stronghold 插件 API 上；应保留独立 vault 文件格式，便于 CLI/headless/sync 复用。

## 5. iCloud/WebDAV 同步调研

iCloud Drive 适合 Apple 生态自动同步文件；WebDAV 标准提供 PROPFIND、GET、PUT、LOCK/UNLOCK、ETag 等基础。Nextcloud WebDAV 文档显示常见实现提供 `etag`、last modified、file size 等属性。

建议：

- Sync 只处理密文 envelope，不接触明文。
- iCloud/WebDAV 远端只能看到密文对象和最小同步元数据，不能看到 provider title、domain、endpoint/base URL、console URL、auth scheme、interface type、API key、quota 或 notes。
- 采用 append-friendly object log + compact snapshot，而不是单 SQLite 文件直接同步。
- 每条 record 是独立密文对象，便于冲突合并。
- 使用 Lamport clock/vector clock + device id 标记变更。
- WebDAV 使用 ETag 做乐观并发；支持 LOCK 但不依赖 LOCK，因为不同服务实现质量不同。
- iCloud 模式使用普通文件夹同步，AIPass 负责冲突文件检测和合并。
- 先做手动同步和定时同步，实时同步作为后续增强。

## 6. CLI 工具生态调研

### Codex

OpenAI Codex CLI 支持 `~/.codex/config.toml` 和 `model_providers` 等配置。AIPass 应提供两类配置方式：

- 写入 config: 为用户生成 provider stanza、model_provider、model、base_url、env_key。
- helper 模式: 不直接写 API Key，而写入环境变量 helper 或 shell wrapper，在运行时从 AIPass 取 key。

### Claude Code

Claude Code 官方支持 `~/.claude/settings.json`、项目级 `.claude/settings.json`/`.local.json`、环境变量和 `apiKeyHelper`。`ANTHROPIC_API_KEY` 可作为 API key。AIPass 最佳方式应优先使用 `apiKeyHelper`，避免把 key 写进 settings/env 文件。

### Gemini CLI

Gemini CLI 官方配置包含认证与 `settings.json`，支持 `GEMINI_API_KEY`/`GOOGLE_API_KEY` 等环境变量。AIPass 可提供 `.env` 注入、shell wrapper、settings patch 三种方式。

### 其他 CLI 候选

MVP 后逐步支持：

- Aider: OpenAI/Anthropic/OpenRouter/provider-specific endpoint/env 体系。
- Continue: VS Code/JetBrains config。
- Open WebUI: 环境变量或 admin 配置。
- LiteLLM CLI/proxy: YAML/env。
- Cursor/Windsurf/VS Code extensions: 只在用户明确授权时处理本地配置。

## 7. 官方 Provider 与第三方/聚合平台调研

官方平台应内置识别规则：

- OpenAI: `platform.openai.com`、`api.openai.com`、key 常见前缀 `sk-`。
- Anthropic: `console.anthropic.com`、`api.anthropic.com`。
- Google AI Studio/Gemini: `aistudio.google.com`、`generativelanguage.googleapis.com`。
- OpenRouter: `openrouter.ai`、OpenAI-compatible interface。
- DeepSeek、Moonshot、Qwen、Zhipu、Volcengine、Azure OpenAI、AWS Bedrock 等。

第三方/聚合平台：

- New API 支持 OpenAI Responses、Realtime、Claude Messages、Gemini、OpenAI-compatible 等多种接口，包含 token 分组、模型限制、配额查询、渠道权重、重试、用户级 rate limit 等能力。
- One API 以标准 OpenAI API 格式统一访问多种大模型，提供 key 管理与二次分发。
- sub2api 一类项目通常通过订阅/代理形式提供 OpenAI-compatible endpoint，需要以“未知但可探测的自托管网关”处理。AIPass 的通用模型不能假定所有 provider 都是 OpenAI-compatible。

建议分类：

- 官方平台: 由 AIPass 内置 domain/provider profile。
- 可信第三方平台: OpenRouter、Together、Fireworks、Groq 等公开 API 平台。
- 自托管聚合/代理: new-api、one-api、sub2api、LiteLLM、Open WebUI gateway。
- 私有/未知 provider: 通过 provider-specific probes、headers、错误响应和用户输入建立 profile。OpenAI-compatible 可用 `/v1/models`，Anthropic/Gemini/Azure/Bedrock 等使用各自接口语义。

## 8. 关键结论

- AIPass 的技术风险不在 UI，而在“跨进程安全边界 + 密文同步 + 各 CLI 配置差异”。
- MVP 必须先做离线本地 vault、安全搜索、CLI helper 和 Chrome Native Messaging 最小闭环。
- 不要在早期追求自动识别所有 Provider；应做 provider registry + heuristics + 用户确认。
- “自动保存 API Key”必须设计成建议保存，而不是静默保存。
- 同步必须从第一天设计为端到端加密对象同步，否则后续很难补。
- E2EE 必须作为 release gate：fake key leak scan、stolen vault test、tamper test、sync server visibility test 不通过则不得发布。

## 9. 主要来源

- Tauri Architecture: https://v2.tauri.app/concept/architecture/
- Tauri Security/Capabilities: https://v2.tauri.app/security/
- Tauri Stronghold: https://v2.tauri.app/plugin/stronghold/
- Tauri Single Instance: https://v2.tauri.app/zh-cn/plugin/single-instance/
- Tauri Deep Linking: https://v2.tauri.app/zh-cn/plugin/deep-linking/
- Tauri System Tray: https://v2.tauri.app/learn/system-tray/
- Turborepo tasks/caching: https://turborepo.com/repo/docs/crafting-your-repository/configuring-tasks
- Svelte runes: https://svelte.dev/docs/svelte/what-are-runes
- Bits UI docs: https://bits-ui.com/docs
- Radix Primitives docs: https://www.radix-ui.com/primitives/docs
- Chrome Native Messaging: https://developer.chrome.com/docs/apps/nativeMessaging
- Chrome Manifest V3: https://developer.chrome.com/docs/extensions/develop/migrate/what-is-mv3
- OWASP Cryptographic Storage Cheat Sheet: https://cheatsheetseries.owasp.org/cheatsheets/Cryptographic_Storage_Cheat_Sheet.html
- OWASP Key Management Cheat Sheet: https://cheatsheetseries.owasp.org/cheatsheets/Key_Management_Cheat_Sheet.html
- RFC 9106 Argon2: https://www.rfc-editor.org/rfc/rfc9106.html
- RFC 4918 WebDAV: https://www.rfc-editor.org/rfc/rfc4918.html
- Nextcloud WebDAV basics: https://docs.nextcloud.com/server/20/developer_manual/client_apis/WebDAV/basic.html
- 1Password sidebar reference: https://support.1password.com/sidebar/
- 1Password CLI reference: https://developer.1password.com/docs/cli
- Claude Code settings: https://docs.anthropic.com/en/docs/claude-code/settings
- Gemini CLI authentication: https://google-gemini.github.io/gemini-cli/docs/get-started/authentication.html
- Gemini CLI configuration: https://google-gemini.github.io/gemini-cli/docs/get-started/configuration.html
- OpenAI API authentication: https://platform.openai.com/docs/api-reference/authentication
- OpenAI Codex config reference source: https://github.com/openai/codex/blob/main/docs/config.md
- New API: https://github.com/QuantumNous/new-api
- One API: https://github.com/songquanpeng/one-api
