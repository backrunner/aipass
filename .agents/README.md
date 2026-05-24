# AIPass Agents Workspace

本目录用于沉淀 AIPass 的产品、设计、架构与开发计划文档。AIPass 的目标是成为面向开发者和 AI 工具用户的本地优先 AI Provider 凭证管理器：桌面端负责安全保存与检索，CLI 负责快速注入/配置主流 AI CLI，Chrome 扩展负责在浏览器中捕捉、保存、检索和填充 API Key。

## 文档索引

- [01-research.md](./01-research.md): 技术栈、竞品、浏览器扩展、本地通信、加密同步、CLI 生态、聚合网关调研。
- [02-requirements.md](./02-requirements.md): 产品定位、用户画像、范围边界、功能需求、非功能需求、数据模型。
- [03-ui-design.md](./03-ui-design.md): 桌面端、扩展端和 CLI 体验设计，包含信息架构、视觉系统、组件规范、关键流程。
- [04-architecture.md](./04-architecture.md): Turbo monorepo、Tauri/Svelte、Rust 核心、Chrome Native Messaging、CLI、同步与加密模块设计。
- [05-development-plan.md](./05-development-plan.md): 分阶段开发计划、包结构落地顺序、测试策略、验收标准。
- [06-roadmap.md](./06-roadmap.md): 1.0 milestone 需求拆解，包含阶段、需求编号、验收标准、依赖、release gate 和 1.0 范围边界。
- [07-security-e2ee-model.md](./07-security-e2ee-model.md): P0 端到端加密安全模型，定义 vault、同步、备份、索引、CLI 写配置的不可泄露约束。
- [08-implementation-status.md](./08-implementation-status.md): 当前 1.0 实现矩阵、release gate 对照、验证命令和剩余发布工程事项。

## 当前建议的核心决策

- Monorepo: `pnpm` workspace + Turborepo，所有 app/package 共享 lint、test、build、typecheck 管线。
- Desktop: Tauri v2 + SvelteKit static build。UI 优先使用 Bits UI；需要 React/Radix 生态资产时通过设计规则借鉴 Radix primitives，而不是在 Svelte app 中强行引入 React runtime。
- Native core: Rust 负责 vault、加密、索引、同步、Native Messaging host、CLI 写配置和安全审计日志。
- 常驻能力: 桌面 GUI 和 headless local agent 共用同一 Rust core。macOS 上通过 LaunchAgent/autostart 提供类似 1Password 的后台能力。
- Browser extension: Chrome Manifest V3 + Native Messaging。扩展不持久保存明文 API Key，只保存短期会话状态和用户授权偏好。
- Storage: 本地只保存加密 vault 和可安全泄露的最小元数据；搜索索引使用派生 token/hash，避免明文 API Key 进入索引。
- Sync: iCloud/WebDAV 只同步端到端加密对象。同步层不理解明文 provider 或 key。
- P0 Security Invariant: 任何人只拿到本地 vault、同步目录、备份文件、搜索索引或日志时，都不能恢复任何 AI secret。
- License: 仓库以 Apache-2.0 开源；注意 new-api 等 AGPL 项目只能作为兼容目标和调研对象，不能复制其代码或 UI。

## 已安装/使用的设计与工程 Skill

本次文档使用了本地已有的 `frontend-design`、`oh-my-codex-designer`、`ui-ux-pro-max` 来约束 UI/UX。已额外安装 `security-best-practices`、`security-threat-model`、`cli-creator`、`figma-use`、`figma-implement-design`、`figma-create-new-file`，后续需要重启 Codex 才会在技能列表中自动出现。

## 主要调研来源

- Tauri Architecture: https://v2.tauri.app/concept/architecture/
- Tauri Security/Capabilities: https://v2.tauri.app/security/
- Tauri Stronghold: https://v2.tauri.app/plugin/stronghold/
- Tauri Single Instance: https://v2.tauri.app/zh-cn/plugin/single-instance/
- Tauri Deep Linking: https://v2.tauri.app/zh-cn/plugin/deep-linking/
- Tauri System Tray: https://v2.tauri.app/learn/system-tray/
- Turborepo Tasks/Caching: https://turborepo.com/repo/docs/crafting-your-repository/configuring-tasks
- Svelte Runes: https://svelte.dev/docs/svelte/what-are-runes
- Bits UI: https://bits-ui.com/docs
- Radix Primitives: https://www.radix-ui.com/primitives/docs
- Chrome Native Messaging: https://developer.chrome.com/docs/apps/nativeMessaging
- Chrome Manifest V3: https://developer.chrome.com/docs/extensions/develop/migrate/what-is-mv3
- OWASP Cryptographic Storage: https://cheatsheetseries.owasp.org/cheatsheets/Cryptographic_Storage_Cheat_Sheet.html
- OWASP Key Management: https://cheatsheetseries.owasp.org/cheatsheets/Key_Management_Cheat_Sheet.html
- RFC 9106 Argon2: https://www.rfc-editor.org/rfc/rfc9106.html
- RFC 4918 WebDAV: https://www.rfc-editor.org/rfc/rfc4918.html
- Nextcloud WebDAV basics: https://docs.nextcloud.com/server/20/developer_manual/client_apis/WebDAV/basic.html
- 1Password sidebar/product reference: https://support.1password.com/sidebar/
- 1Password CLI/developer reference: https://developer.1password.com/docs/cli
- Claude Code settings: https://docs.anthropic.com/en/docs/claude-code/settings
- Gemini CLI auth/config: https://google-gemini.github.io/gemini-cli/docs/get-started/authentication.html
- Gemini CLI configuration: https://google-gemini.github.io/gemini-cli/docs/get-started/configuration.html
- OpenAI API key guidance: https://platform.openai.com/docs/api-reference/authentication
- OpenAI Codex config reference source: https://github.com/openai/codex/blob/main/docs/config.md
- New API: https://github.com/QuantumNous/new-api
- One API: https://github.com/songquanpeng/one-api
