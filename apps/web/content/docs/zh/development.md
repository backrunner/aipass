---
title: 开发与发布
description: 构建 AIPass、了解组件职责、执行检查与准备发布。
navTitle: 开发
order: 10
---

# 开发与发布

仓库的[贡献指南](https://github.com/backrunner/aipass/blob/main/CONTRIBUTING.md)和 [Agent 指令](https://github.com/backrunner/aipass/blob/main/agents.md)定义当前的验证与提交规则。

## 环境

- Node.js 26（`.nvmrc`）和 pnpm 12.3.4（`package.json`）。
- 稳定版 Rust；macOS 本地使用 Homebrew 的最新 `rust` formula。
- 对应平台的 [Tauri 依赖](https://v2.tauri.app/start/prerequisites/)。

```bash
nvm install
nvm use
npm install --global pnpm@12.3.4
pnpm install --frozen-lockfile
pnpm --filter @aipass/desktop dev
```

开发构建使用独立的 `aipass-dev://` scheme。运行 `pnpm --filter @aipass/desktop dev:register`，可为浏览器扩展注册开发应用的启动入口。发布构建使用 `aipass://`。

运行 `pnpm --filter @aipass/extension build` 构建扩展，产物位于 `apps/extension/build/aipass-extension.zip`。

### macOS 工具链

检查 `command -v cargo rustc` 和 `rustc --version`，确认使用 Homebrew Rust。macOS 27 的 `mis-aligned LINKEDIT string pool` 问题曾定位到 LLVM 22 的 `rust-objcopy`；已验证的本地修复仅将 Rust 的 `rust-objcopy` 符号链接指向 Homebrew LLVM 23 的 `llvm-objcopy`，保留 Rust 自身的 LLVM 库依赖。升级或重装 Rust 后需重新检查，详见[工具链注意事项](https://github.com/backrunner/aipass/blob/main/.codex/skills/aipass-revise/references/pitfalls.md)。

## 组件职责

```text
Desktop UI ─────────┐
CLI ────────────────┼──> Rust Agent ───> Encrypted vault
Browser extension ──┘          ├──> Encrypted sync
                               └──> Tool configuration writers
```

- `apps/desktop`：Svelte 界面和 Tauri 桥接。
- `crates/aipass-agent`：可信本地服务与最终凭据访问。
- `crates/aipass-vault`、`aipass-sync`：保险库存储和加密同步。
- `crates/aipass-cli`、`aipass-native-host`：CLI 与浏览器边界。
- `apps/extension`：Chromium 扩展。
- `apps/web`：文档站与更新订阅源 Worker。
- `packages/ui`：共享界面组件与语言文件。

前端只负责界面，存储、凭据写入、同步和最终工具配置变更均由 Rust 负责。订阅适配器在本仓库维护，不在运行时加载第三方服务商引擎。

## 验证

按 `agents.md` 根据改动影响选择检查。每次提交、推送或发布前，必须通过完整工作区 Clippy：

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

涉及安全、存储、同步、IPC、启动与更新、依赖、构建、CI 或发布时，还需完成完整本地检查：

```bash
pnpm install --frozen-lockfile
pnpm licenses:audit
pnpm lint
pnpm typecheck
pnpm test
pnpm build
git diff --exit-code -- apps/control-panel/embedded
cargo fmt --all -- --check
cargo test --workspace
cargo build --workspace
```

完整检查还包含已生成的 macOS DMG/更新包，以及四项运行场景；命令见[仓库验证规则](https://github.com/backrunner/aipass/blob/main/.codex/skills/aipass-repo-guardrails/SKILL.md)。本地在 macOS 执行，Linux 和 Windows 交给 GitHub Actions。桌面界面改动需在 Tauri 最小窗口 960×640 下检查。

不要提交真实 API 密钥、更新签名私钥或明文保险库导出。

## 发布

[Release 工作流](https://github.com/backrunner/aipass/blob/main/.github/workflows/release.yml)接受签名 tag：`vX.Y.Z`、`vX.Y.Z-beta.N` 和 `vX.Y.Z-nightly.YYYYMMDD.N`。它从 tag 写入版本，构建桌面、CLI 与扩展，检查签名及运行行为，再发布 release 和更新清单。

1. 验证精确的源提交及所需远程检查。
2. 确认 GitHub Actions secrets 已配置签名、公证、provisioning profile 和更新签名凭据。
3. 推送签名 tag，并跟踪全部发布任务完成。
4. 在发布说明中感谢合并的 PR 及贡献者。
5. 下载公开制品，核对哈希、签名、版本、公证、运行行为及对应渠道的更新订阅源。

不要覆盖已发布的 tag。详见[桌面制品验证](https://github.com/backrunner/aipass/blob/main/docs/desktop-artifact-validation.md)、[CloudKit 发布配置](https://github.com/backrunner/aipass/blob/main/docs/cloudkit-release.md)和[更新渠道](/docs/zh/updates)。

文档站有独立的 [Cloudflare 部署流程](https://github.com/backrunner/aipass/blob/main/apps/web/DEPLOYMENT.md)。更新文档不需要发布新的桌面版本。
