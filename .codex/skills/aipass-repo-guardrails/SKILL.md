---
name: aipass-repo-guardrails
description: Enforce AIPass architecture, storage, secret-handling, validation, and pre-push CI rules. Use whenever changing, reviewing, committing, or pushing code in the AIPass repository.
---

# AIPass Repo Guardrails

Use this skill whenever you change, review, commit, or push code in the AIPass repository, especially architecture, storage, sync, desktop integration, browser integration, CLI config writing, or IPC.

## Repository shape

- `apps/desktop`
  - Svelte UI only.
  - `src-tauri` is the desktop bridge into Rust.
- `crates/aipass-agent`
  - Primary local core service.
  - Owns vault access, sync execution, config writes, and trusted local IPC.
- `crates/aipass-agent-protocol`
  - Structured request/response types for local IPC.
- `crates/aipass-sync`
  - Local folder, iCloud-style folder, OneDrive-style folder, and WebDAV sync logic.
- `crates/aipass-native-host`
  - Browser native messaging boundary.
- `crates/aipass-cli`
  - CLI surface that must call the Rust core service instead of writing final state directly.

## Desktop UI Scope

- The authoritative desktop viewport is the Tauri window minimum of `960x640`, configured in `apps/desktop/src-tauri/tauri.conf.json` and `tauri.dev.conf.json`.
- Desktop UI validation must cover that minimum window for pages, lists, forms, and dialogs. Do not introduce phone-sized or browser-preview responsive workarounds for the Tauri desktop surface unless the product explicitly adds that target.

Read these repo docs before large changes:

- `.agents/04-architecture.md`
- `.agents/07-security-e2ee-model.md`
- `.agents/08-implementation-status.md`

## Non-negotiable architecture rules

1. Final data access must go through the Rust core service.
2. The desktop frontend must stay a UI layer, not a data or storage authority.
3. Browser extension and native host flows must never become an alternate source of truth.
4. Sync providers must reuse the core sync engine. Frontend code must not manipulate vault or sync objects directly.
5. CLI provider switching or external tool config writing must go through Rust core plans and writers.

## Storage rules

- Persistent storage belongs in Rust crates, primarily `aipass-agent`, `aipass-vault`, `aipass-sync`, and related storage helpers.
- Do not add new frontend-side persistence for vault data, provider credentials, sync state, or tool config state.
- Do not let TypeScript write final secrets, final sync objects, or final provider config files directly.
- If a new setting must persist, prefer a Rust-owned command and file format over browser or frontend local storage.

## IPC and secret-handling rules

- All IPC must use typed protocol messages from `aipass-agent-protocol`.
- Do not add unauthenticated local socket, pipe, or file-based command channels.
- Sensitive fields must use dedicated secret types such as `SensitiveString`, not plain `String`.
- Do not log, clone, or cache master passwords, API keys, recovery keys, or decrypted secrets unless absolutely required for the immediate operation.
- Zeroize sensitive buffers when practical and keep exposure windows short.
- Extension and native host paths must not accept user-entered master passwords in browser-controlled surfaces.
- Desktop-to-Rust requests that carry secrets must be minimal, short-lived, and never persisted in the frontend.

## Sync rules

- Supported sync targets are implemented by the core sync engine.
- Local folder, iCloud, OneDrive, and WebDAV flows must resolve and execute in Rust.
- Cloud folder discovery belongs in Rust. The UI may select a mode, but it must not decide the final filesystem path.
- Sync conflict inspection and resolution must go through the agent, using structured requests.
- Sync payloads remain encrypted objects; no plaintext provider data may be written into sync targets.

## Change checklist

- Does this change keep the frontend as a pure UI surface?
- Does final read/write authority stay in Rust?
- Are IPC messages typed, authenticated, and narrow?
- Are sensitive fields handled with secret-aware types?
- Does sync still operate on encrypted objects only?
- Did you avoid introducing a second code path that bypasses `aipass-agent`?

## Pre-push GitHub CI gate

`git push` is prohibited until every required check below passes for the exact commit being pushed.

1. Immediately before validation, re-read every file in `.github/workflows/`. Treat the workflows as the source of truth and include any newly added or changed check.
2. Passing this gate is necessary but does not grant permission to push. Only push when the user has explicitly requested it.
3. Finalize the intended commits first. Record the source ref and commit SHA for every intended refspec, require a clean worktree, and run the gate against each unique commit being pushed. Never include unvalidated extra refs through `--all`, `--tags`, or additional refspecs.
4. Run the complete branch CI gate for every code or documentation push, even when the changed files look unrelated to a job. Do not select checks based on the diff.
5. Match the workflow environments: run the `rust` and `node` jobs on an Ubuntu-compatible clean runner, and run `macOS desktop bundle` on macOS. Use Node 24, pnpm from the root `packageManager` field, stable Rust, and every platform dependency declared by the workflows.
6. Every setup step, command, and bundle assertion must exit successfully. A pre-existing failure is still a failure.
7. After validation, require every recorded ref to resolve to the same commit and the worktree to still be clean. Any commit, amend, rebase, merge, generated-file change, or workflow change invalidates the result and requires the full gate again.
8. Never bypass the gate with `--no-verify`, ignored exit codes, narrower package filters, skipped tests, or a claim that CI will catch the problem after pushing.
9. If a check cannot run because a toolchain, dependency, operating system, credential, or service is unavailable, stop and report the blocker. Do not push.

### Required branch CI commands

Run these from the repository root. Apply any shell wrapper required by the active agent instructions without changing the wrapped commands.

`rust` on Ubuntu:

```bash
sudo apt-get update
sudo apt-get install -y libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev patchelf
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace
```

`node` on Ubuntu with Node 24:

```bash
pnpm install --frozen-lockfile
pnpm licenses:audit
pnpm lint
pnpm typecheck
pnpm test
pnpm build
```

`macOS desktop bundle` on macOS with Node 24 and stable Rust:

```bash
pnpm install --frozen-lockfile
pnpm --dir apps/desktop tauri build --ci --bundles app
set -euo pipefail
app_path="$(find target -path "*/release/bundle/macos/AIPass.app" -type d -print -quit)"
test -n "${app_path}"
test -x "${app_path}/Contents/MacOS/aipass-desktop"
test -x "${app_path}/Contents/Resources/aipass-agent"
test -x "${app_path}/Contents/Resources/aipass-native-host"
```

Report each completed job as `rust`, `node`, and `macOS desktop bundle`, including failures or checks that could not run.

### Release tag pushes

The branch gate does not authorize pushing a `v*.*.*` tag. A release tag also triggers `.github/workflows/release.yml`, including version consistency, cross-platform CLI builds, extension packaging, macOS signing and notarization, updater verification, release metadata, GitHub draft release creation, and R2 upload.

Before a release tag push:

- Run the complete branch gate above against the tagged commit.
- Re-read `release.yml` and execute every locally reproducible command for the exact tag.
- Confirm every required GitHub, Apple, Tauri, and Cloudflare secret and external prerequisite without printing secret values.
- Do not push the tag if any release job, platform assertion, credential, or external prerequisite cannot be verified. Report the blocker instead of weakening or bypassing the workflow.
