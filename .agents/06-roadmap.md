# AIPass 1.0 Milestone Roadmap

本文定义 AIPass 1.0 上线前的阶段拆解和详细需求。1.0 是第一个稳定开源版本，不是实验版；所有列入 1.0 的需求必须有清晰验收标准、质量门和安全边界。

## 1.0 目标

AIPass 1.0 要交付一个本地优先、端到端加密、跨平台可用的 AI Provider 凭证管理器，包含桌面端、CLI、Chrome 扩展、本地后台能力和 iCloud/WebDAV 密文同步。

1.0 用户必须能够：

- 在桌面端创建、搜索、复制、Reveal 和管理 AI Provider 配置。
- 安全保存 OpenAI、Anthropic、Gemini、OpenRouter、DeepSeek、Qwen、Moonshot、Zhipu、Volcengine、Azure OpenAI、AWS Bedrock、New API、One API、LiteLLM、自定义 HTTP API、自定义 OpenAI-compatible 等配置。
- 通过 CLI 快速配置 Codex、Claude Code、Gemini CLI，并可回滚。
- 通过 Chrome 扩展在浏览器中保存和使用 API Key。
- 在多设备间通过 iCloud/WebDAV 同步密文对象。
- 在本地 vault、同步目录、备份、索引、日志被复制的情况下，保护 AI secrets 不被恢复。

## Release Gate

1.0 发布必须同时满足：

- `fake_key_leak_scan` 为 0。
- `stolen_vault_test` 通过：没有 master password 时，离线复制 vault/sync/backup/index/log 不能恢复任何 API secret 或敏感 provider 配置。
- `tamper_test` 通过：篡改 ciphertext、AAD、object type、sync metadata 后必须解密失败或 quarantine。
- `epoch_ratchet_test`、`ttl_erasure_test`、`compromise_recovery_test` 通过。
- Desktop、CLI、Extension、Sync 四条主路径 E2E 全绿。
- 默认配置不会把 API key 明文写入第三方工具配置。
- Chrome extension storage、Native Messaging logs、Tauri logs、crash diagnostics 不含明文 key。
- macOS 和 Windows installer 可安装、升级、卸载、修复 native host。
- Apache-2.0 license、SECURITY.md、CONTRIBUTING.md、README、用户文档齐全。

## 阶段总览

| Phase | Milestone | 目标 | 预计周期 | 进入条件 | 退出条件 |
|---|---|---|---:|---|---|
| P0 | Project Foundation | 仓库、CI、工程规范可用 | 1 周 | 空仓库 | 基础命令和 CI 通过 |
| P1 | Crypto & Vault Core | E2EE vault 和安全测试闭环 | 2 周 | P0 完成 | 加密、ratchet、leak tests 通过 |
| P2 | Desktop Local App | 桌面端本地管理体验可用 | 2 周 | P1 API 可用，P3-R01 可并行先行 | GUI 可创建、搜索、复制、Reveal |
| P3 | Provider Registry | Provider 模板和分类可用 | 1 周 | P0-R04 完成，可与 P2 并行 | 官方/第三方/自托管识别 |
| P4 | CLI Integrations | CLI 和三大工具配置可用 | 2 周 | P1 vault API 稳定 | Codex/Claude/Gemini 配置和回滚 |
| P5 | Browser Extension | Chrome 扩展和 native host 可用 | 2 周 | P1/P2 unlock API 可用 | 扩展查询、保存、填充闭环 |
| P6 | Sync | iCloud/WebDAV 密文同步可用 | 2 周 | P1 object format 稳定 | 两设备同步和冲突处理 |
| P7 | Beta Hardening | 安全、性能、安装、文档完善 | 3 周 | P2-P6 功能闭环 | Public beta 可用 |
| P8 | 1.0 Release | 稳定开源发布 | 1 周 | Beta hardening 完成 | 1.0 tag 和 release artifacts |

## P0 Project Foundation

目标：建立可长期维护的大仓底座，让后续桌面、CLI、扩展、Rust core 可以并行开发。

### P0-R01 Monorepo 初始化

优先级：P0  
范围：`pnpm` workspace、Turborepo、Rust workspace、目录结构。

需求：

- 初始化 `apps/desktop`、`apps/extension`、`crates/*`、`packages/*`。
- 配置 `pnpm-workspace.yaml`、`turbo.json`、root `package.json`。
- 配置 Rust workspace，至少包含 `aipass-crypto`、`aipass-vault`、`aipass-core`、`aipass-cli` 占位 crate。
- 建立统一的 `build`、`dev`、`test`、`lint`、`typecheck` 任务。

验收：

- `pnpm install` 成功。
- `pnpm turbo build`、`pnpm turbo lint`、`pnpm turbo typecheck` 可运行。
- `cargo test --workspace` 可运行。
- 新开发者按 README 可在 10 分钟内跑起空项目。

依赖：无。

### P0-R02 代码质量与 CI

优先级：P0  
范围：GitHub Actions 或等效 CI。

需求：

- CI 执行 TypeScript lint/typecheck/test。
- CI 执行 Rust fmt/clippy/test。
- CI 执行 license audit。
- CI 区分 required checks 和 optional platform checks。
- 约定 PR 合并前必须通过所有 required checks。

验收：

- 创建空 PR 时 CI 自动运行。
- 任意 lint 或 test 失败会阻止合并。
- license audit 能识别 AGPL/GPL 风险依赖。

依赖：P0-R01。

### P0-R03 开源基础文件

优先级：P0  
范围：开源仓库治理。

需求：

- 添加 Apache-2.0 `LICENSE`。
- 添加 `NOTICE` 模板。
- 添加 `README.md`，说明定位、开发命令、架构概览。
- 添加 `SECURITY.md`，说明安全披露方式和支持版本。
- 添加 `CONTRIBUTING.md`，说明分支、测试、提交要求。
- 添加 ADR 模板，用于记录 vault、sync、extension、CLI 关键决策。

验收：

- 仓库根目录具备所有基础文件。
- README 包含本地开发最短路径。
- SECURITY.md 明确不要公开提交真实 API key。

依赖：无。

### P0-R04 Shared Schema 基础

优先级：P1  
范围：跨 Rust/TypeScript 的 schema 定义。

需求：

- 定义 `ProviderEntry`、`SecretRef`、`QuotaInfo`、`ToolProfile`、`ProviderDefinition` 的 schema。
- 选择 schema 生成策略：Rust serde + TS type generation，或 JSON Schema 作为中间格式。
- 所有 IPC、Native Messaging、CLI JSON 输出必须基于 schema。

验收：

- TS 和 Rust 能从同一 schema 生成或校验基础类型。
- schema 变更需要测试更新。
- schema 中不包含会诱导明文 secret 落盘的字段。

依赖：P0-R01。

## P1 Crypto & Vault Core

目标：先把安全核心做扎实。桌面端、CLI、扩展和同步都必须依赖同一套 vault core。

### P1-R01 Vault 初始化与 Master Password

优先级：P0  
范围：首次创建 vault、解锁、锁定、修改 master password。

需求：

- 用户首次创建 vault 时必须设置 master password。
- 使用 Argon2id 从 master password 派生 Master Key。
- KDF 参数写入 vault header，但不包含明文 secret。
- 支持 unlock、lock、change master password。
- 弱密码提示不阻断高级用户，但必须明确风险。

验收：

- 正确 master password 可解锁。
- 错误 master password 不暴露解密差异细节。
- 修改 master password 后旧密码失效，新密码可解锁。
- vault header 不包含 provider title、domain、endpoint、auth scheme、interface type、API key。

依赖：P0-R04。

### P1-R02 Record Envelope 与 E2EE 文件格式

优先级：P0  
范围：本地 vault 持久化格式。

需求：

- 所有 provider 业务字段默认进入 record plaintext 后整体加密。
- 每个 record 使用独立随机 Record DEK。
- Record DEK 由 Vault Epoch Key 包裹。
- envelope 明文只允许 format version、crypto id、object id/type、sync metadata、KDF params 等最小信息。
- AAD 覆盖 vault id、object id、object type、schema version、crypto version、关键 sync metadata。

验收：

- 创建 provider 后，grep vault 目录不能找到 title、domain、endpoint、auth scheme、interface type、API key、notes。
- 修改 ciphertext 或 AAD 后解密失败。
- record 可按 schema version 解析。
- 不同 record 使用不同 DEK。

依赖：P1-R01。

### P1-R03 Vault Epoch Ratchet

优先级：P0  
范围：泄露后恢复和 key schedule。

需求：

- Vault 维护单调递增 epoch。
- epoch advance 引入 OS CSPRNG 新随机数。
- 新 epoch key 不能由旧 epoch key 推导。
- 当前 epoch key 不能推导已销毁旧 epoch key。
- master password 修改、设备移除、疑似泄露、手动安全轮换触发 epoch advance。
- active records 支持后台逐步 rewrap/re-encrypt 到最新 epoch。

验收：

- `epoch_ratchet_test` 证明旧 epoch key 不能解密新对象。
- `compromise_recovery_test` 证明模拟旧 key 泄露后，新 epoch 写入对象不可被旧 key 解密。
- epoch advance 中断后可恢复，不破坏 vault。

依赖：P1-R02。

### P1-R04 TTL Cryptographic Erasure

优先级：P0  
范围：短期授权和过期对象。

需求：

- Reveal token、Chrome fill grant、CLI 临时 env grant、旧版本草稿、sync staging 使用 TTL key 或 per-object expiry key。
- 到期后删除 wrapped DEK 或 TTL bucket key。
- 删除 key 后密文可以保留，但不可解密。
- active provider entry 默认不使用自动 TTL 删除，避免用户数据意外丢失。

验收：

- `ttl_erasure_test` 证明删除 TTL key 后过期对象不可解密。
- active record 不受 TTL key 删除影响。
- TTL 到期事件写入审计日志，但不写明文。

依赖：P1-R03。

### P1-R05 Secret Handling 与 Redaction

优先级：P0  
范围：内存、日志、错误、剪贴板边界。

需求：

- secret buffers 使用 zeroize 或等效清理。
- Rust core 是默认唯一可接触明文 secret 的层。
- 错误信息、debug log、panic hook、audit log 必须 redaction。
- API key display 默认只显示 masked suffix。
- copy/reveal/fill 操作必须记录审计事件，但不记录明文。

验收：

- `fake_key_leak_scan` 扫描 logs、audit、vault、index、backup 无 fake key。
- 锁定后 secret API 拒绝访问。
- Reveal 超时后明文从 UI state 清除。

依赖：P1-R02。

### P1-R06 Local CRUD API

优先级：P0  
范围：Provider Entry 本地增删改查。

需求：

- 支持 create、read、update、archive、delete provider entry。
- 支持多个 domain、多个 secret、headers、quota、tags、environment、notes、model aliases。
- 删除默认为 archive，永久删除需要二次确认。
- 每次变更产生审计事件和 sync object。

验收：

- 单元测试覆盖完整 CRUD。
- archive 后默认列表不显示，但可在 Archive 中恢复。
- permanent delete 后无法通过 UI 恢复，但密文对象清理策略明确。

依赖：P1-R02、P0-R04。

### P1-R07 Security Test Harness

优先级：P0  
范围：安全测试工具。

需求：

- 实现 `fake_key_leak_scan`。
- 实现 `stolen_vault_test`。
- 实现 `tamper_test`。
- 实现 `epoch_ratchet_test`。
- 实现 `ttl_erasure_test`。
- 实现 `compromise_recovery_test`。

验收：

- CI 中 crypto/vault 变更必须运行这些测试。
- 测试失败会阻止合并。
- fake key pattern 覆盖 OpenAI、Anthropic、Gemini、OpenRouter 常见形态。

依赖：P1-R01 至 P1-R06。

## P2 Desktop Local App

目标：交付桌面端本地管理闭环，视觉克制、效率高、可键盘操作。

### P2-R01 Tauri + Svelte App Shell

优先级：P0  
范围：桌面应用基础。

需求：

- 使用 Tauri v2 + SvelteKit static build。
- 接入 Bits UI、lucide-svelte、design tokens。
- 配置 Tauri capabilities，默认最小权限。
- 支持 light/dark 跟随系统。
- 支持窗口状态保存。

验收：

- macOS 和 Windows 开发环境可启动。
- WebView 无任意文件系统权限。
- UI 基础主题在 light/dark 下对比度达标。

依赖：P0-R01。

### P2-R02 Unlock / Lock Experience

优先级：P0  
范围：锁屏、解锁、自动锁定。

需求：

- 首次启动进入创建 vault。
- 已有 vault 启动进入 unlock screen。
- 支持手动 lock。
- 支持空闲超时自动 lock。
- 系统睡眠或用户锁屏后自动 lock。
- 锁定后清空 UI 中所有 secret 相关 state。

验收：

- 错误密码不泄露具体原因。
- 锁定后 back navigation 不显示旧 detail secret。
- 自动锁定时间可在 settings 调整。

依赖：P1-R01、P2-R01。

### P2-R03 三栏主工作台

优先级：P0  
范围：sidebar、list、detail。

需求：

- Sidebar 包含 All、Favorites、Recent、Official、Third-party、Self-hosted、Tags、Archive、Settings。
- List 显示 favicon、title、domain/endpoint、provider badge、interface/auth hint、masked key suffix、quota/status。
- Detail 显示 provider header、secret fields、endpoint fields、tool config actions、quota、activity。
- 小屏下支持 sidebar collapse 和单栏 drill-in。

验收：

- 1280x800、1440x900、390x844 无重叠。
- 1,000 条 mock entry 列表滚动流畅。
- 键盘 tab 顺序符合视觉顺序。

依赖：P2-R01、P1-R06。

### P2-R04 Quick Add / Edit Provider

优先级：P0  
范围：创建和编辑表单。

需求：

- 支持从模板创建 provider。
- 支持自定义 provider。
- 支持填写 title、domain、endpoint/base URL、console URL、API key、认证方式、接口协议、default model、headers、quota、tags、notes。
- API key 输入后显示 fingerprint 和 masked preview。
- 高级字段折叠，默认不压迫主流程。

验收：

- 新用户 3 分钟内可创建第一条 API key。
- 未填必填项时错误显示在字段附近。
- 保存后立即进入 detail。

依赖：P2-R03、P3-R01。

### P2-R05 Search & Filters

优先级：P0  
范围：本地搜索体验。

需求：

- 支持 title、domain、provider、endpoint host、console URL、interface type、auth scheme、tag、model、fingerprint、后四位搜索。
- API key 不做明文全文检索。
- 支持过滤 official、third-party、self-hosted、environment、tag、recent、quota low。
- 锁定状态不保留明文搜索索引。

验收：

- 1,000 entries 搜索 <50ms。
- 10,000 entries 搜索 <150ms。
- 搜索无结果时可直接创建 custom provider。

依赖：P1-R05、P2-R03。

### P2-R06 Copy / Reveal / Clipboard

优先级：P0  
范围：secret 使用操作。

需求：

- 支持 copy API key、copy endpoint/base URL、copy provider-specific curl、copy env export、copy config snippet。
- Reveal 默认超时隐藏。
- Clipboard 默认 30-60 秒清理，用户可调。
- 所有 copy/reveal 需要审计事件。
- 前端默认不持久保存明文 secret。

验收：

- Copy 成功有明确状态反馈。
- Clipboard 到期后清理。
- 审计日志不含明文。

依赖：P1-R05、P2-R03。

### P2-R07 Settings

优先级：P1  
范围：Security、Sync、Extension、CLI、About。

需求：

- Security: auto-lock、clipboard clear、biometric placeholder、plaintext export 开关、fast locked search index 开关。
- Sync: sync provider、status、last sync、conflicts。
- Extension: native host status、extension id、last connection、repair。
- CLI: supported tools、config status、repair。
- About: version、license、security disclosure。

验收：

- 设置修改持久化。
- 高风险开关默认关闭并有明确说明。
- Extension/CLI repair 状态可见。

依赖：P2-R01。

## P3 Provider Registry

目标：让 AIPass 理解通用 AI Provider、不同接口协议、不同认证方式，以及 OpenAI-compatible 这类自托管/代理网关语义。

### P3-R01 Provider Definition Schema

优先级：P0  
范围：provider registry 数据结构。

需求：

- 定义 provider id、display name、kind、domains、interfaces、auth schemes、endpoints、key patterns、favicon strategy、tool defaults。
- 支持 official、third_party、self_hosted、unknown。
- 支持 browser detection hints 和 CLI writer hints。

验收：

- registry 可被 Desktop、CLI、Extension 共用。
- 添加新 provider 不需要改核心业务代码。

依赖：P0-R04。

### P3-R02 Official Provider Templates

优先级：P0  
范围：官方平台。

需求：

- 支持 OpenAI、Anthropic、Google Gemini、Azure OpenAI、AWS Bedrock、DeepSeek、Moonshot、Qwen、Zhipu、Volcengine。
- 每个 provider 包含 domain matcher、endpoint、interface type、auth scheme、key fingerprint hint、默认 headers。
- Anthropic 使用 Anthropic Messages 语义；Gemini 使用 Gemini/Google API key 语义；Azure OpenAI 支持 resource endpoint、deployment、api version 字段；Bedrock 支持 region/profile/credential reference。

验收：

- 输入官方控制台域名可自动分类。
- 创建表单自动填入合理默认值。
- 错误 key pattern 只提示，不阻断保存。

依赖：P3-R01。

### P3-R03 Third-party Provider Templates

优先级：P1  
范围：公开第三方平台。

需求：

- 支持 OpenRouter、Together、Fireworks、Groq 等高频平台。
- 支持第三方平台的原生接口协议和 OpenAI-compatible endpoint。
- 支持 provider-specific headers，如 Anthropic version、OpenRouter referer/title、Google API key、Azure api-key 等作为非 secret 或 secret ref。

验收：

- OpenRouter 可生成正确 endpoint 和默认 env/config snippet。
- 第三方 provider 在 UI 中区别于 official。

依赖：P3-R01。

### P3-R04 Self-hosted Gateway Templates

优先级：P0  
范围：new-api、one-api、LiteLLM、sub2api/custom。

需求：

- 支持 New API、One API、LiteLLM、sub2api、Custom OpenAI-compatible、Custom HTTP API。
- 根据 URL path、页面文本、provider-specific probe 推断自托管网关和接口协议。
- 支持自定义 endpoint/base URL、console URL、admin token、user token、custom headers、quota notes。
- 不复制 AGPL 项目代码或 UI，只做兼容识别。

验收：

- 输入 new-api/one-api endpoint 或 console URL 可进入 self-hosted template。
- Unknown endpoint 可通过 custom wizard 保存。
- license audit 不引入 AGPL 代码。

依赖：P3-R01。

### P3-R05 Provider Probe

优先级：P1  
范围：可选连接测试。

需求：

- 对 OpenAI-compatible endpoint 可尝试 `/v1/models`。
- 对 Anthropic/Gemini/Azure OpenAI/Bedrock 使用 provider-specific probe。
- Probe 必须由用户触发。
- Probe 结果只显示 success/failure、status、model count，不记录 response 中敏感内容。

验收：

- 无网络时 UI 不阻塞保存。
- Probe failure 不阻止保存。
- logs 不记录 Authorization header。

依赖：P3-R02、P3-R03、P3-R04。

## P4 CLI Integrations

目标：让 AIPass 成为开发者 CLI 工作流的凭证源，而不是只会复制 key 的桌面应用。

### P4-R01 CLI Command Framework

优先级：P0  
范围：`aipass` 二进制。

需求：

- 支持 `doctor`、`vault status`、`login`、`lock`、`list`、`search`、`get`、`copy`、`env`、`exec`、`configure`、`rollback`。
- 支持 `--json` 稳定机器输出。
- 支持 shell completion。
- 错误输出不包含 secret。

验收：

- `aipass --help` 展示所有主命令。
- `aipass doctor --json` 输出稳定 schema。
- 未解锁时 secret 命令返回明确错误。

依赖：P1-R06。

### P4-R02 CLI Secret Access Policy

优先级：P0  
范围：CLI 访问控制。

需求：

- `get --reveal` 必须显式传参。
- 默认不打印 API key。
- `exec` 只对子进程注入临时 env。
- 临时授权使用 TTL cryptographic erasure。
- CLI 访问写入 audit log。

验收：

- `aipass get <id>` 默认只返回 masked。
- `aipass exec` 结束后 env 不留在 shell。
- CLI logs 不含明文 key。

依赖：P1-R04、P1-R05、P4-R01。

### P4-R03 Config Writer Framework

优先级：P0  
范围：写第三方工具配置。

需求：

- 提供 plan/diff/apply/rollback 模型。
- 写入前展示目标路径和 diff。
- 每次写入生成 operation id。
- 所有原文件 backup 由 AIPass 加密保存。
- 默认使用 helper/wrapper，不写明文 key。
- 明文写入必须 advanced opt-in、二次确认、审计。

验收：

- apply 后可 rollback 到原状态。
- temp HOME golden tests 覆盖 diff 和 rollback。
- backup 中如果原文件含明文 key，也必须加密保存。

依赖：P4-R01。

### P4-R04 Codex Integration

优先级：P0  
范围：OpenAI Codex CLI。

需求：

- 支持检测 `~/.codex/config.toml`。
- 支持写入 `model_providers`、`model_provider`、endpoint/base URL、env/helper 引用。
- 支持 OpenAI-compatible provider；其他 provider 通过各自的 tool integration 处理，不强制进入 Codex writer。
- 支持 helper mode 优先。
- 支持 rollback。

验收：

- temp HOME 中可生成可解析 config。
- 不默认写明文 API key。
- 重复执行不会产生重复 provider block。

依赖：P4-R03、P3-R02、P3-R04。

### P4-R05 Claude Code Integration

优先级：P0  
范围：Claude Code。

需求：

- 支持用户级和项目级 `.claude/settings.json`。
- 优先使用 `apiKeyHelper`。
- 支持 `ANTHROPIC_API_KEY` wrapper mode。
- 支持 settings merge，不覆盖无关用户配置。
- 支持 rollback。

验收：

- temp HOME/project fixture 写入后 JSON 合法。
- 原 settings 中无关字段保持不变。
- 默认不写明文 key。

依赖：P4-R03、P3-R02。

### P4-R06 Gemini CLI Integration

优先级：P0  
范围：Gemini CLI。

需求：

- 支持 `GEMINI_API_KEY`、`GOOGLE_API_KEY` env/wrapper。
- 支持 settings path 检测。
- 支持 wrapper mode 和 env snippet。
- 支持 rollback。

验收：

- temp HOME fixture 通过。
- 生成 shell snippet 不直接落盘明文，除非用户显式选择。
- 默认不破坏既有 Gemini CLI auth 配置。

依赖：P4-R03、P3-R02。

## P5 Browser Extension

目标：让用户在浏览器中创建或使用 API key 时，AIPass 能安全介入。

### P5-R01 Chrome MV3 Extension Scaffold

优先级：P0  
范围：扩展基础。

需求：

- 使用 Manifest V3。
- 包含 service worker、content script、popup。
- 权限最小化：`nativeMessaging`、`activeTab`、必要 storage、按需 host permissions。
- 不使用远程托管代码。

验收：

- 可在 Chrome developer mode 加载。
- Extension lint/build 通过。
- storage 中不含 secret 明文。

依赖：P0-R01。

### P5-R02 Native Messaging Host

优先级：P0  
范围：扩展与本地 AIPass 通信。

需求：

- 实现 Chrome Native Messaging stdio protocol。
- 校验 extension id。
- 校验 message schema 和 protocol version。
- 支持 ping、context lookup、secret fill、save detected、unlock request。
- 所有 secret 操作需要 capability grant 和用户手势。

验收：

- invalid extension id 被拒绝。
- malformed message 被拒绝且不 crash。
- Native host logs 不含明文 secret。

依赖：P1-R06、P5-R01。

### P5-R03 Extension Popup

优先级：P0  
范围：扩展用户界面。

需求：

- Locked 状态显示 unlock CTA。
- Current site matched 状态显示当前域名匹配 entries。
- No match 状态支持 search 和 save new provider。
- 支持 copy/fill endpoint/base URL、provider-specific credential 字段和 key。
- 支持连接状态和错误修复入口。

验收：

- 360x520 下布局无重叠。
- 键盘可操作。
- Copy/fill 需要用户点击。

依赖：P5-R02。

### P5-R04 Content Detection

优先级：P0  
范围：页面 API key 检测。

需求：

- 支持官方 OpenAI、Anthropic、Gemini、OpenRouter 页面保存建议。
- 支持 new-api/one-api 常见 token/key 页面。
- 支持通过 label、input name、URL path、button text、key pattern 生成 draft。
- 只生成 draft，不静默保存。
- 用户可对站点选择 ignore。

验收：

- fake provider console fixtures 通过。
- 误报可被用户忽略并持久化 preference。
- content script 不持久化 key。

依赖：P3-R01、P5-R02。

### P5-R05 Save Detected Secret Flow

优先级：P0  
范围：从浏览器保存到 vault。

需求：

- 保存前展示确认 sheet。
- 展示 domain、provider guess、masked key、fingerprint、endpoint/base URL、认证方式、接口协议、tags、quota。
- 用户可修改 title、provider kind、environment。
- 保存后 entry 出现在 desktop 和 extension lookup。

验收：

- 未确认不会写 vault。
- 保存成功后 audit log 不含明文 key。
- extension storage 不含明文 key。

依赖：P5-R04、P1-R06。

### P5-R06 Native Host Install / Repair

优先级：P1  
范围：安装与修复。

需求：

- Desktop installer 安装 native host manifest。
- Settings 中显示 native host 状态。
- 支持 repair native host。
- 支持卸载时清理 manifest。

验收：

- macOS/Windows 安装后扩展可连接。
- manifest path 错误时 repair 可恢复。
- 卸载后不留下失效 host path。

依赖：P5-R02。完整 installer 集成在 P7-R04 收敛。

## P6 Sync

目标：提供 iCloud/WebDAV/本地文件夹端到端加密同步，不让同步服务看到明文。

### P6-R01 Sync Object Model

优先级：P0  
范围：同步对象格式。

需求：

- 同步对象直接复用 encrypted record envelope。
- object 名称不包含 title、domain、provider、endpoint、auth scheme 或 interface type。
- 支持 device id、lamport、hash、object type。
- 支持 checkpoint 和 tombstone。

验收：

- sync directory 中 grep 不到 provider 明文。
- object hash 可用于完整性检查。
- tombstone 不泄露删除对象业务语义。

依赖：P1-R02。

### P6-R02 Local Folder / iCloud Folder Sync

优先级：P0  
范围：本地文件夹和 iCloud Drive 文件夹。

需求：

- 用户选择同步目录。
- 支持手动同步和定时同步。
- 支持 iCloud Drive 文件夹指引。
- 支持 offline 状态。
- 支持远端重置和本地重建索引。

验收：

- 两个本地 profile 通过文件夹同步 100 条记录。
- 断网/目录不可用时本地 vault 可继续用。
- sync status 在 desktop settings 可见。

依赖：P6-R01、P2-R07。

### P6-R03 WebDAV Sync

优先级：P0  
范围：WebDAV 客户端。

需求：

- 支持 URL、username/password 或 app password。
- 使用 PROPFIND、GET、PUT、DELETE。
- 使用 ETag/If-Match 做乐观并发。
- 不依赖 LOCK 作为安全边界。
- 支持 auth_failed、offline、server_error 状态。

验收：

- WebDAV mock server E2E 通过。
- Nextcloud 实测通过。
- 服务端 payload 不含明文 provider 配置。

依赖：P6-R01。

### P6-R04 Conflict Detection & Resolver

优先级：P0  
范围：同步冲突。

需求：

- 不同 entry 自动合并。
- 同 entry 不同字段可自动合并。
- 同 entry 同字段冲突生成 conflict record。
- Secret 冲突不自动覆盖。
- Desktop 提供 conflict resolver。

验收：

- 两设备同时修改同一 secret 时进入手动选择。
- conflict resolver 显示 masked secret 和 metadata，不泄露明文到日志。
- 解决冲突后生成新 encrypted record。

依赖：P6-R02、P6-R03、P2-R07。

### P6-R05 Device List & Revoke

优先级：P1  
范围：设备管理。

需求：

- 显示 device id、device name、首次同步、最后同步。
- 支持 revoke device。
- revoke 触发 Vault Epoch Key advance。
- 提示用户如真实 API key 可能泄露，需要去 provider 平台轮换。

验收：

- revoke 后旧设备不能继续写入有效同步对象。
- epoch advance 后新对象不能被旧 epoch key 解密。
- UI 明确说明 revoke 的边界。

依赖：P1-R03、P6-R01。

## P7 Beta Hardening

目标：把功能闭环打磨到 public beta 质量。

### P7-R01 Performance

优先级：P0  
范围：启动、搜索、列表、同步。

需求：

- 启动到锁屏 <1s。
- 解锁后首屏 <500ms。
- 1,000 entries 搜索 <50ms。
- 10,000 entries 搜索 <150ms。
- 列表虚拟化。
- 同步不阻塞 UI。

验收：

- 性能 benchmark 在 CI 或 nightly 中运行。
- 低端设备 profile 不出现明显卡顿。
- 性能回退超过阈值需要阻止 release。

依赖：P2、P6。

### P7-R02 Security Review

优先级：P0  
范围：安全硬化。

需求：

- 审查 Tauri capabilities。
- 审查 Native Messaging boundary。
- 审查 extension permissions。
- 审查 logs/crash redaction。
- 审查 config writer plaintext mode。
- 执行 dependency audit 和 license audit。

验收：

- 无未解释的 broad filesystem/network capability。
- 无 P0/P1 security finding。
- 所有高风险开关默认关闭。

依赖：P1-P6。

### P7-R03 UX Polish

优先级：P1  
范围：产品体验。

需求：

- Command palette。
- Keyboard shortcuts。
- Empty states。
- Settings polish。
- Error recovery states。
- Sync conflict UX。
- Extension error repair UX。

验收：

- 核心复制/配置流程不超过 3 次动作。
- 无主要布局重叠。
- 可访问性 AA 基础检查通过。

依赖：P2、P5、P6。

### P7-R04 Packaging

优先级：P0  
范围：安装包。

需求：

- macOS app bundle/dmg。
- Windows installer。
- Linux AppImage 或 deb/rpm beta。
- CLI 随 desktop 安装，也支持 standalone。
- native host manifest 安装/修复/卸载。
- 版本号和 auto-update 策略明确。

验收：

- macOS/Windows clean machine 安装成功。
- 升级不破坏 vault。
- 卸载不删除用户 vault，除非用户显式选择。

依赖：P2、P4、P5。

### P7-R05 Documentation

优先级：P0  
范围：用户和开发文档。

需求：

- User guide: 创建 vault、添加 provider、搜索、复制、扩展、CLI、同步。
- Security model: E2EE、前向安全边界、泄露后处理。
- CLI reference。
- Extension install/troubleshooting。
- Sync setup: iCloud/WebDAV。
- Contributor guide。

验收：

- 新用户按 docs 可完成第一条 provider 保存。
- 开发者按 docs 可跑起 desktop/CLI/extension。
- 安全文档明确不能远程吊销已被复制且含旧 key 的副本。

依赖：P1-P6。

## P8 1.0 Release

目标：稳定开源发布。

### P8-R01 Release Candidate Freeze

优先级：P0  
范围：版本冻结。

需求：

- 冻结 vault schema v1。
- 冻结 CLI JSON contract v1。
- 冻结 Native Messaging protocol v1。
- 冻结 Provider Registry v1。
- 只接受 P0/P1 bug fix。

验收：

- 所有 contract 有 version 字段。
- migration tests 通过。
- changelog 记录 breaking changes。

依赖：P1-P7。

### P8-R02 Final Security Gate

优先级：P0  
范围：发布前安全门。

需求：

- 运行全部 E2EE tests。
- 运行 fake key leak scan。
- 运行 extension storage scan。
- 运行 config backup scan。
- 运行 dependency/license audit。
- 检查 release artifact 不含测试 key。

验收：

- 所有安全检查全绿。
- 无 P0/P1 安全问题。
- 安全例外必须写入 ADR，且不能涉及明文 API key 泄露。

依赖：P7-R02。

### P8-R03 Release Artifacts

优先级：P0  
范围：发布产物。

需求：

- macOS dmg/app。
- Windows installer。
- Linux package。
- Standalone CLI binaries。
- Chrome extension package。
- Checksums。
- SBOM。
- Release notes。

验收：

- 每个 artifact 可下载并安装。
- checksum 可验证。
- release notes 包含已知限制。

依赖：P7-R04。

### P8-R04 1.0 Launch Documentation

优先级：P0  
范围：上线文档。

需求：

- README 更新到 1.0。
- Installation docs。
- Security model docs。
- Migration/backup docs。
- Troubleshooting docs。
- Provider support matrix。
- CLI integration matrix。

验收：

- docs 链接无 404。
- 安装、扩展、CLI、同步四条路径文档完整。
- 明确 Apache-2.0 license 和安全披露方式。

依赖：P7-R05。

## 1.0 Out of Scope

以下不进入 1.0：

- OAuth Provider 接入。
- 云端托管同步服务。
- 团队 vault、RBAC、共享 vault。
- 自动轮换官方 Provider API Key。
- Firefox/Edge/Safari 扩展。
- 移动端 app。
- Provider usage/quota API 自动拉取。
- 非官方登录态转 API 或绕过服务条款的能力。

## 1.1+ 候选方向

- Aider、Continue、LiteLLM、Open WebUI、Cursor/Windsurf 配置适配。
- Firefox/Edge/Safari extension。
- Provider usage/quota pull。
- Model catalog 和价格提示。
- Encrypted team export/import。
- Mobile read-only companion。
- Secret references for CI/CD。
- Local automation API，默认关闭且强授权。

## 风险与缓解

### 安全风险

风险：持久化文件没有真正 E2EE，导致复制 vault/sync/backup 后恢复 secrets。  
缓解：P0 E2EE invariant、per-record DEK、Vault Epoch Key ratchet、fake key leak scan、stolen vault test。

风险：明文 secret 泄露到日志、剪贴板、crash dump、config backup。  
缓解：redaction、secure clipboard、encrypted backup、secret handling tests。

风险：Browser extension 被恶意页面诱导泄露。  
缓解：用户手势、capability grant、origin verification、Native Messaging schema validation。

风险：WebDAV/iCloud 冲突覆盖 secret。  
缓解：secret conflict 永不自动覆盖，进入 conflict resolver。

### 产品风险

风险：自动保存误报太多，用户关闭扩展。  
缓解：只做保存建议、可忽略站点、fixtures 驱动检测规则。

风险：Provider/CLI 适配过多导致维护失控。  
缓解：1.0 只支持核心 provider 和 Codex/Claude/Gemini，其他进入 1.1+。

### 工程风险

风险：跨平台 native host 安装复杂。  
缓解：installer + repair flow + 状态检查 + 文档。

风险：WebDAV 实现差异。  
缓解：mock server、Nextcloud 实测、ETag 乐观并发、不依赖 LOCK。

## 近期执行顺序

1. 创建 P0 工程底座 issue。
2. 创建 P1 E2EE 安全模型 ADR。
3. 创建 vault file format ADR。
4. 创建 provider entry schema v1。
5. 实现 crypto/vault test harness。
6. 再进入 Desktop UI shell，避免 UI 先行导致安全模型返工。
