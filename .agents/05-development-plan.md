# AIPass 详细开发计划

建议采用 2 周一个 iteration。MVP 目标是 12-14 周形成可用 alpha，16-18 周形成 public beta。

## Phase 0: 仓库初始化与工程底座

周期：第 1 周

任务：

- 初始化 Git 仓库、Apache-2.0 LICENSE、README、CONTRIBUTING、SECURITY。
- 初始化 pnpm workspace + Turborepo。
- 创建 `apps/desktop`、`apps/extension`、`crates/*`、`packages/*`。
- 配置 Rust workspace、TypeScript config、ESLint、Prettier、Vitest、Playwright。
- 设置 CI：lint、typecheck、test、cargo test、cargo clippy、license audit。
- 创建 docs skeleton 和 ADR 模板。

验收：

- `pnpm turbo build lint test typecheck` 可运行。
- `cargo test --workspace` 可运行。
- CI 在空实现上通过。

## Phase 1: Crypto/Vault 最小闭环

周期：第 2-3 周

任务：

- 实现 Argon2id KDF 参数模型。
- 实现 vault manifest、record envelope、AEAD encrypt/decrypt。
- 实现 P0 E2EE invariant：vault、同步对象、备份、索引、日志默认不能泄露 AI secrets。
- 实现 per-record DEK 和 Vault Epoch Key wrapping。
- 实现 epoch ratchet 和 TTL cryptographic erasure。
- 实现 lock/unlock lifecycle。
- 实现 provider entry schema v1。
- 实现 secret masking/fingerprint。
- 实现 audit log v1。
- 实现本地 CRUD API。
- 实现 fake key leak scan、stolen vault test、tamper test。
- 实现 epoch ratchet test、TTL erasure test、compromise recovery test。

验收：

- CLI 或测试工具可创建 vault、解锁、写入 provider、读取 provider。
- 篡改密文无法解密。
- 不提供 master password 时，离线复制 vault 目录不能恢复 title、domain、endpoint、认证方式或 API key。
- fake API key 不出现在 vault 文件、日志和 audit 中。
- 旧 epoch key 不能解密新 epoch 写入的对象；删除 TTL key 后过期对象不可解密。

## Phase 2: Desktop 基础 UI

周期：第 4-5 周

任务：

- Tauri v2 + SvelteKit static build 打通。
- 接入 Bits UI、lucide-svelte、design tokens。
- 实现 unlock screen。
- 实现三栏主工作台。
- 实现 entry list、detail、quick add、edit dialog。
- 实现 copy secret command。
- 实现 provider favicon fetch/fallback。
- 实现 search index v1。

验收：

- 用户可通过 GUI 完成创建、搜索、查看、复制。
- 1,000 条 mock entries 搜索流畅。
- 锁定后 detail 不保留明文。
- Playwright/screenshot 检查桌面布局无重叠。

## Phase 3: Provider Registry 与分类

周期：第 6 周

任务：

- 建立 provider registry schema。
- 内置官方 provider：OpenAI、Anthropic、Gemini、OpenRouter、DeepSeek、Moonshot、Qwen、Zhipu、Volcengine、Azure OpenAI。
- 内置自托管/聚合 templates：New API、One API、LiteLLM、sub2api、Custom OpenAI-compatible、Custom HTTP API。
- 实现 domain matcher、interface/auth scheme guess、endpoint normalization。
- 实现 provider probe v1。

验收：

- 输入常见官方控制台域名可自动分类。
- 输入 new-api/one-api endpoint 或 console URL 可进入 self-hosted template。
- Unknown provider 可通过 custom wizard 保存。

## Phase 4: CLI alpha

周期：第 7-8 周

任务：

- 实现 `aipass doctor/status/login/lock/list/search/get/copy/env/exec`。
- 输出稳定 `--json`。
- 实现 Codex config writer：helper mode、backup、diff、rollback。
- 实现 Claude Code writer：`apiKeyHelper` mode。
- 实现 Gemini CLI writer：env/wrapper/settings mode。
- 增加 temp HOME golden tests。

验收：

- 三个工具均能在临时目录写入配置并回滚。
- 默认不打印明文 secret。
- `aipass doctor --json` 可被 CI fixture 验证。

## Phase 5: Chrome Extension + Native Host

周期：第 9-10 周

任务：

- 实现 MV3 extension scaffold。
- 实现 Native Messaging host。
- 实现 ping、lookup、copy/fill、save draft 协议。
- 实现 popup locked/matched/no-match 状态。
- 实现 content script detector v1。
- 支持 OpenAI/Anthropic/Gemini/OpenRouter 官方页面保存建议。
- 支持 new-api/one-api 通用 key/token 页面保存建议。
- 实现 native host installer/repair。

验收：

- 扩展能连接本地 AIPass。
- 当前域名能查询匹配 entries。
- 检测到新 API key 后用户确认保存。
- invalid extension id 被拒绝。
- extension storage 不出现 fake API key。

## Phase 6: Sync alpha

周期：第 11-12 周

任务：

- 实现 local folder sync。
- 实现 iCloud folder sync 指引和路径选择。
- 实现 WebDAV client：PROPFIND/GET/PUT/ETag。
- 实现 encrypted object merge。
- 实现 conflict record 和 desktop conflict resolver。
- 实现 sync status UI。

验收：

- 两个本地 profile 通过文件夹同步增量记录。
- WebDAV mock server E2E 通过。
- 同 entry secret 冲突进入 conflict view。
- remote storage 中无明文 provider secret、title、domain、endpoint、认证方式、quota 或 notes。
- WebDAV mock server visibility test 证明远端 payload 只包含密文对象和最小同步元数据。

## Phase 7: Beta hardening

周期：第 13-16 周

任务：

- 性能优化：索引、虚拟列表、启动速度。
- 安全审计：日志、crash、clipboard、native messaging、Tauri capabilities。
- UX polish：keyboard shortcuts、command palette、empty states、settings。
- Packaging：macOS notarization plan、Windows installer、Linux packages。
- Documentation：用户文档、CLI docs、extension install、sync setup。
- Provider fixtures 扩展。

验收：

- 主要流程 E2E 全绿。
- fake key leak scan 全绿。
- macOS/Windows beta installer 可安装、卸载、修复 native host。
- README 能让新用户 10 分钟跑通本地开发。

## Backlog 分组

### 安全

- OS biometric unlock。
- Hardware-bound device key。
- Secure clipboard clear per OS。
- Secret access policy per domain/tool。
- Security review automation。

### Provider

- Provider health check adapters。
- Usage/quota API pull。
- Model catalog cache。
- Azure OpenAI deployment mapping。
- AWS Bedrock profile。

### CLI

- Aider、Continue、LiteLLM、Open WebUI。
- Shell plugins：fish/zsh/bash completion。
- `aipass run --env-file-template`。
- Secret references。

### Extension

- Side panel。
- More official console detectors。
- Firefox/Edge/Safari。
- Autofill provider-specific endpoint/credential fields into forms。

### Sync

- Better conflict UI。
- Snapshot compaction。
- Encrypted backup export/import。
- Device revoke。

## 每阶段质量门

- 代码合并前：lint/typecheck/test。
- 触碰 crypto/vault/sync/native host：必须有 Rust tests。
- 触碰 UI：必须做 desktop + narrow viewport screenshot。
- 触碰 extension：必须跑 fake page detector fixture。
- 触碰 config writer：必须跑 temp HOME golden diff。
- 触碰 secret handling、sync、backup、index、key schedule：必须跑 fake key leak scan、stolen vault test、tamper test、epoch ratchet test、TTL erasure test。

## 初始 Issues 建议

1. Scaffold monorepo and CI.
2. Add Rust workspace and crypto crate.
3. Define provider entry schema v1.
4. Implement encrypted record envelope.
5. Build Tauri/Svelte unlock screen.
6. Build desktop three-pane shell.
7. Add provider registry package.
8. Implement CLI doctor/list/search.
9. Implement Codex config writer.
10. Implement Native Messaging host protocol.
11. Build Chrome extension popup.
12. Implement WebDAV sync mock.
