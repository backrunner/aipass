# AIPass 需求文档

## 1. 产品定位

AIPass 是一个开源的、跨平台的 AI Provider 配置与 API Key 管理工具。它面向经常使用多个 AI 服务、多个 CLI 工具、多个代理网关和浏览器控制台的开发者。AIPass 需要像 1Password 一样安全、快速、低打扰，但语义上专注于 AI API 凭证、端点、模型能力、额度和工具配置。

## 2. 目标用户

- 独立开发者: 同时使用 OpenAI、Anthropic、Gemini、OpenRouter、DeepSeek 等 provider，需要快速复制和切换 key。
- AI 工具重度用户: 使用 Codex、Claude Code、Gemini CLI、Aider、Continue、Open WebUI 等，需要安全配置。
- 团队技术负责人: 管理团队内部 new-api/one-api/LiteLLM 网关地址、key、额度和权限。
- 跨设备用户: 希望在 macOS、Windows、Linux 间同步 API Provider 配置，但不希望服务端看到明文。

## 3. 范围边界

### MVP 包含

- 本地 vault 初始化、解锁、锁定、修改 master password。
- 创建/编辑/删除/归档 AI Provider 条目。
- 支持字段：名称、域名、favicon、provider 类型、credential 类型、API endpoint 或 console URL、API key、认证方式、接口协议、默认模型、headers、备注、标签、额度、到期时间、环境。
- 多维搜索：名称、域名、provider、endpoint/base URL、console URL、接口协议、认证方式、标签、后四位/fingerprint、备注、模型名。
- 官方平台/第三方平台/自托管聚合平台分类。
- Tauri 桌面端三栏 UI。
- Chrome 扩展通过 Native Messaging 查询、保存、填充 API Key。
- CLI `aipass` 支持 list/get/copy/inject/configure/doctor。
- Codex、Claude Code、Gemini CLI 三个配置适配器。
- 本地 E2EE vault 文件格式。
- iCloud 文件夹同步和 WebDAV 同步的 alpha 版本。
- Apache-2.0 license、基本贡献文档、安全披露文档。

### MVP 不包含

- OAuth 授权接入。
- 团队服务端、共享 vault、RBAC。
- 云端托管同步服务。
- 自动轮换官方 Provider API Key。
- 绕过 provider 使用条款的非官方登录态转 API 能力。
- 在浏览器页面静默抓取所有输入内容。

### 未来可选

- 团队 vault 和共享密钥。
- Provider 用量 API 拉取。
- 模型目录和价格表自动更新。
- SSH/Git/CI secret references。
- Firefox/Edge/Safari extension。
- 移动端查看与复制。

## 4. 核心概念

### Provider Entry

一条完整 AI Provider 配置。可代表 OpenAI、Anthropic、Gemini、Azure OpenAI、OpenRouter、Bedrock、第三方平台、自托管聚合网关或任意自定义 AI API 的凭证。OpenAI-compatible 只是其中一种接口协议，不是 AIPass 的默认假设。

字段建议：

```ts
type ProviderEntry = {
  id: string;
  vaultId: string;
  title: string;
  providerKind: "official" | "third_party" | "self_hosted" | "unknown";
  providerId?: string;
  domains: string[];
  faviconUrl?: string;
  endpoints: ProviderEndpoint[];
  interfaceType:
    | "openai_compatible"
    | "anthropic_messages"
    | "gemini"
    | "azure_openai"
    | "bedrock"
    | "custom_http";
  authScheme:
    | "bearer"
    | "x_api_key"
    | "google_api_key"
    | "azure_api_key"
    | "custom_header";
  secretRefs: SecretRef[];
  defaultModel?: string;
  modelAliases?: Record<string, string>;
  headers?: Record<string, SecretOrPlainRef>;
  quota?: QuotaInfo;
  tags: string[];
  environment: "personal" | "work" | "team" | "test" | "prod";
  notes?: string;
  createdAt: string;
  updatedAt: string;
  lastUsedAt?: string;
  archivedAt?: string;
};

type ProviderEndpoint = {
  id: string;
  kind: "api" | "console" | "auth" | "usage" | "custom";
  url?: string;
  region?: string;
  deployment?: string;
  apiVersion?: string;
};
```

### Secret

高敏值，如 API key、extra bearer token、organization id 中需要保护的部分。Secret 永远密文存储，只有在 reveal/copy/fill/configure 时进入短期内存。

### Provider Registry

内置 provider 规则库，用于域名识别、favicon、credential 字段模板、接口协议、认证方式、key fingerprint、CLI 配置模板和浏览器捕捉规则。

### Tool Profile

某个 CLI 或应用的配置目标，例如 Codex、Claude Code、Gemini CLI。包含 config path、写入策略、回滚策略、helper 策略和测试命令。

## 5. 功能需求

### Vault 与安全

- P0: 任何人只拿到本地 vault、iCloud/WebDAV 同步目录、导出备份、搜索索引、审计日志或配置 backup，都不能恢复任何 AI secret。
- 用户首次启动必须创建 master password。
- 支持自动锁定：系统睡眠、空闲超时、用户锁屏、手动锁定。
- 支持 Touch ID/Windows Hello/libsecret/KWallet 等系统能力作为解锁辅助，不替代 master password。
- 支持 emergency recovery key 或导出加密备份。
- 支持安全导出：加密 `.aipass-vault` 文件；明文导出必须二次确认并默认关闭。
- 支持审计日志：创建、编辑、复制、填充、配置写入、同步、失败登录。日志不记录明文 secret。
- 默认加密所有业务字段：API key、endpoint/base URL、console URL、domain、title、provider kind、auth scheme、interface type、tags、quota、notes、model aliases。只允许 format version、KDF 参数、object id/type、sync metadata 等最小路由信息明文。
- 每条 provider record 使用独立 Record DEK；Record DEK 由 Vault Epoch Key 包裹，禁止用同一个长期内容 key 直接加密所有 records。
- 支持 Vault Epoch Key ratchet：master password 修改、设备移除、疑似泄露、手动安全轮换后生成新 epoch，并逐步 rewrap/re-encrypt active records。
- 支持 TTL cryptographic erasure：Reveal grant、CLI 临时授权、浏览器填充授权、旧版本等临时对象到期后删除对应 key，使对象不可再解密。
- 明确限制：已经被攻击者复制且包含旧 key 的备份或会话，无法被本地后续操作远程吊销。

### Provider 管理

- 用户可以从模板创建：OpenAI、Anthropic、Gemini、OpenRouter、Azure OpenAI、AWS Bedrock、DeepSeek、Moonshot、Qwen、Zhipu、Volcengine、New API、One API、LiteLLM、自定义 HTTP API、自定义 OpenAI-compatible。
- 不同 provider 必须保留自己的认证语义：OpenAI/OpenRouter 常见 Bearer token；Anthropic 常见 `x-api-key`；Gemini 常见 `GEMINI_API_KEY`/Google API key；Azure OpenAI 常见 endpoint + api version + api key；Bedrock 常见 AWS profile/region/credential reference。
- 根据域名自动提取 favicon；失败时生成 provider initials icon。
- 支持多个 domain 与一个 entry 关联。
- 支持多个 secret：primary key、fallback key、admin key、read-only key。
- 支持额度信息：总额度、月额度、剩余额度、RPM/TPM、到期日、备注。
- 支持复制 endpoint/base URL、console URL、复制 key、复制 provider-specific curl、复制 env export、复制 config snippet。
- 支持 entry health check：OpenAI-compatible 可选调用 `/v1/models`，Anthropic/Gemini/Azure/Bedrock 使用 provider-specific probe；只显示成功/失败，不上传 key 到 AIPass 之外。

### 搜索与检索

- 全局快捷搜索：名称、域名、provider、endpoint/base URL、console URL、接口协议、认证方式、标签、模型、后四位/fingerprint。
- 支持 filters：官方/第三方/自托管、环境、标签、最近使用、即将到期、额度低。
- 搜索结果必须支持键盘操作：上下选择、Enter 打开、Cmd/Ctrl+C 复制默认 secret、Cmd/Ctrl+Shift+C 复制 base URL。
- API key 不做明文全文检索；只允许 fingerprint、后四位、用户 alias。

### Desktop

- 三栏布局：左 sidebar、中间 list、右 detail。
- 支持 quick add、quick copy、quick configure。
- 支持锁屏、解锁、设置、同步状态、扩展连接状态。
- 支持暗色/亮色跟随系统。
- 支持系统 tray：锁定、打开搜索、同步、退出。

### CLI

命令表：

```bash
aipass doctor
aipass vault status
aipass login
aipass lock
aipass list --provider openai --json
aipass list --provider anthropic --json
aipass list --provider gemini --json
aipass search "openrouter prod"
aipass get <entry-id> --field api_key --reveal
aipass copy <entry-id> --field api_key
aipass exec <entry-id> -- codex "..."
aipass env <entry-id> --format shell
aipass configure codex <entry-id> --mode helper
aipass configure claude-code <entry-id> --mode helper
aipass configure gemini-cli <entry-id> --mode env
aipass rollback <operation-id>
```

CLI 要求：

- 默认输出人类可读；`--json` 输出稳定 JSON。
- 永不默认打印 secret；必须传 `--reveal` 且有 unlock/session。
- 写配置前显示 diff 和 backup path；支持 `--yes` 非交互。
- 支持 `apiKeyHelper`/wrapper 优先，明文写入 env/config 作为显式 opt-in。

### Chrome Extension

- 在官方 provider 控制台创建 API Key 后，弹出“保存到 AIPass”建议。
- 在已知 API settings 页面显示 AIPass inline affordance。
- 在 Anthropic/Gemini/OpenAI/OpenRouter 等官方平台，以及 new-api/one-api/sub2api 等自托管页面，通过字段名、路由、DOM label、URL path、provider-specific hints 识别 key、endpoint、认证方式和接口协议。
- 用户点击扩展图标时，显示当前 domain 可用 entries。
- 支持复制/填充 endpoint/base URL、provider-specific credential 字段和 key。
- 保存新 key 时必须展示确认 sheet：domain、provider guess、key fingerprint、entry title、tags、quota。
- 扩展 locked 时显示 unlock CTA，唤起 desktop/headless agent。
- 扩展只保留短期 nonce/session，不持久保存明文 key。

### 同步

- 支持本地文件夹、iCloud Drive 文件夹、WebDAV。
- 同步层只同步端到端加密对象；远端服务端不能读取 provider title、domain、endpoint/base URL、console URL、auth scheme、interface type、API key、quota 或 notes。
- 支持同步状态：idle、syncing、conflict、offline、auth_failed。
- 支持冲突解决：自动合并不同 entry；同 entry 同字段冲突进入 conflict view。
- 支持设备列表：device id、名称、首次同步时间、最后同步时间。
- 支持远端重置和本地重建索引。

## 6. 非功能需求

- 安全：明文 secret 不落盘；日志不含 secret；clipboard 自动清理；Native Messaging 校验 extension id；本地文件、同步文件、备份文件和索引文件默认都不能泄露 AI secrets。
- 性能：1,000 entries 搜索 <50ms；10,000 entries 搜索 <150ms；启动到锁屏 <1s；解锁后首屏 <500ms。
- 可用性：核心复制/配置流程不超过 3 次动作。
- 可访问性：键盘完整可用、焦点可见、对比度 AA、screen reader label。
- 跨平台：macOS/Windows/Linux；MVP 可优先 macOS + Windows，Linux beta。
- 可维护性：共享 schema、provider registry、config writers；端到端测试覆盖关键流程。
- 开源：Apache-2.0，依赖 license audit，避免引入 AGPL 代码。

## 7. 验收标准

MVP 可发布标准：

- 用户能创建 vault，保存 5 个 provider，按域名和名称快速搜索。
- 用户能通过 Chrome 扩展在 OpenAI/Anthropic/Gemini/OpenRouter/new-api 页面保存或检索 key。
- 用户能用 `aipass configure codex`、`aipass configure claude-code`、`aipass configure gemini-cli` 完成配置，并可回滚。
- 断网状态下本地 vault 完整可用。
- iCloud/WebDAV 同步密文对象，不暴露明文。
- 攻击者复制整个 vault/sync/backup 目录且没有 master password 时，不能恢复任何 provider 配置明文、endpoint、认证方式或 API key。
- 安全测试证明日志、崩溃报告、扩展 storage、配置 backup 中无明文 API Key。
- Epoch ratchet、TTL erasure、compromise recovery 测试通过：旧 epoch key 不能解密新对象，当前 key 不能解密已销毁 key 的过期对象。
