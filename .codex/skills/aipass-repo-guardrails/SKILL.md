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

## UI copy and compactness

- Keep every UI surface concise and compact. Remove descriptions that do not help users understand an action, state, or decision. Do not add text merely to fill space.
- Prefer short labels, single-line list rows, and restrained spacing while preserving clear guidance for errors, empty states, and consequential actions.
- Avoid middle dots or bullet separators (`·`, `•`) in Chinese UI text. Use separate fields, layout groups, or natural sentences instead of concatenating unrelated labels, states, and metadata.
- Show each fact once within a page or dialog. Check headings, body copy, footers, account identity, status, and buttons for redundant information.
- Apply these rules to every locale and loading, success, empty, and failure state. See root `AGENTS.md` and `.agents/03-ui-design.md`.

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
- For UI changes, did you remove unnecessary descriptions and duplicated information, avoid dotted Chinese text, and keep rows and spacing compact?
- Did you review the affected locales and loading, success, empty, and failure states at the desktop minimum window size?

## Local validation platform

Per the permanent user instruction recorded in root `agents.md` on 2026-09-07,
local Ubuntu validation is skipped. Run local checks natively on macOS. Never
create, start, or retain Ubuntu/Linux containers or virtual machines for local
CI reproduction, including branch and nightly release gates. Do not install
Linux dependencies locally or treat unavailable local Ubuntu validation as a
blocker. GitHub Actions runs its configured Linux jobs remotely.

Per the local Rust preference in root `agents.md`, use Homebrew's latest stable
`rust` formula for local macOS validation. Check `command -v cargo rustc` and
`rustc --version`; do not prioritize rustup to work around a dependency failure.
For the macOS 27 LINKEDIT issue, Homebrew Rust 1.98.0's LLVM 22 `rust-objcopy`
misaligns stripped libraries. The verified local repair uses Homebrew LLVM
23.1.0's `llvm-objcopy` through Rust's `rust-objcopy` symlink, leaving the Rust
compiler's LLVM 22 library dependency intact. Recheck the symlink and a freshly
compiled stripped library after a Rust upgrade/reinstall. See `README.md` and
the build-toolchain entry in the pitfalls registry for details.

## Commit and push validation

Follow the proportionate validation rule in root `agents.md` (2026-10-20),
with its mandatory Clippy guard (2026-10-25).
Select checks by behavior and dependency impact, not by file count or size.
Passing checks does not grant permission to push; push only when requested.

### Mandatory Clippy guard

Before every commit, push or release, require this command to pass for the
intended code, including documentation and UI-only changes:

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

Run it from the repository root with Homebrew Rust on macOS. A crate-only check
does not satisfy this guard. Fix failures before proceeding; do not relax lint
levels, exclude workspace members or targets, or add suppressions merely to pass.
Reuse a passing result only when its source, generated inputs, Cargo manifests
and lockfile, build configuration, toolchain and relevant environment still
match. Report the command and result, and distinguish local macOS validation
from Linux and Windows CI coverage.

### Select the additional local scope

The following checks are in addition to the mandatory Clippy guard.

| Change | Required local checks |
| --- | --- |
| Documentation only | Review diff, formatting and affected links. Run `pnpm repo:check` for repository instructions, structure or dependency rules, and the skill validator for edited skills. No application build. |
| Simple copy, locale, CSS, layout or isolated UI fix | Affected package typecheck; relevant existing tests for changed behavior; inspect the affected UI at `960x640`. No new tests for copy/CSS alone, other workspace checks or desktop bundle. |
| Ordinary logic or feature change | Lint/typecheck/tests and, where compilation or output is affected, build the relevant packages/crates and dependent callers. Expand for shared contracts or uncertain impact. |
| Security, credential ownership, durable storage, sync, IPC contract, startup/update lifecycle, dependency, build or CI behavior | Complete local gate below. Review relevant pitfalls and exercise the changed failure paths. |
| Release/tag or artifact publication | Complete local gate and release checks below, against the exact release commit and finished artifacts. |

Classify the actual change: editing a label in a login screen does not change
authentication, while a one-line account-matching fix can change credential
ownership. Failures or unclear dependencies justify broader checks; routine
success does not justify repeating them.

For a desktop display fix, typical commands are:

```bash
cargo clippy --workspace --all-targets -- -D warnings
pnpm --filter @aipass/desktop typecheck
# Only when related behavior changes; pass test files directly to Vitest.
pnpm --dir apps/desktop exec vitest run src/lib/components/integration/IntegrationCard.test.ts
```

Use the actual affected package and tests. For a localized Rust logic fix outside
the high-risk areas, keep the mandatory workspace Clippy check, then use
`cargo fmt -p <crate> -- --check` and relevant
`cargo test -p <crate> <filter>` tests, including dependent callers when needed.

Shared UI locale changes require `pnpm --filter @aipass/control-panel build`
because its embedded output includes those dictionaries. Review and commit the
affected generated files together. For the full gate, verify the build leaves
`apps/control-panel/embedded` unchanged. Generate outputs before finalizing the
commit so they do not cause a second validation cycle.

### Avoid redundant work

- Validate the intended final content before or after committing. Reuse passing
  results if source, tests, build configuration and dependencies used by those
  checks still match the committed tree. Commit creation, signing and a
  message-only amend do not change those inputs.
- After a content change, rerun only checks whose inputs or expected behavior
  changed. Release verification must still identify the exact commit and
  artifact hashes; rebuilt artifacts require artifact validation again.
- Reuse installed dependencies when lockfiles and the environment are unchanged.
  Read the relevant workflow jobs as needed; a full workflow reread is required
  only for the complete gate or changes to workflows.
- Review staged files and preserve unrelated work. Do not require an otherwise
  clean repository for a focused fix. If unrelated edits affect validation,
  use an isolated worktree for the intended tree; do not copy the entire
  `target` directory merely to validate a small fix.
- Before pushing, verify the intended source refs/SHAs, committed diff and
  applicable passing checks. Push only those refs; do not use `--all` or
  `--tags` to include unvalidated work. For the complete gate, use a clean
  validation checkout with the intended commit.
- Required checks must pass. Investigate relevant failures; report unavailable
  required checks and do not push past them. Do not ignore exit codes or bypass
  hooks. Report which checks ran and which were out of scope; remote CI retains
  its configured jobs and is separate evidence. Do not claim remote CI passed
  when a branch push did not trigger it.

### Complete local gate (high-risk changes and releases)

Immediately before this gate, read every file in `.github/workflows/` and record
the intended source refs and commit SHAs. Use a clean validation checkout of
each unique commit being pushed. Afterward require the same refs/SHAs and clean
checkout; input changes invalidate affected checks. Use the configured workflow
commands, applying the permanent macOS-only local rule. Package filters are for
the focused paths above, not substitutes for this complete gate.

Run these from the repository root. Apply any shell wrapper required by the active agent instructions without changing the wrapped commands.

`rust` on macOS:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace
```

`node` on macOS with Node 26:

```bash
pnpm install --frozen-lockfile
pnpm licenses:audit
pnpm lint
pnpm typecheck
pnpm test
pnpm build
git diff --exit-code -- apps/control-panel/embedded
```

`macOS desktop bundle` on macOS with Node 26 and stable Rust:

```bash
set -euo pipefail
pnpm install --frozen-lockfile
runtime_signing_dir="$(mktemp -d)/signing"
trap 'rm -rf "$runtime_signing_dir"' EXIT
node scripts/prepare-runtime-check-build.mjs "$runtime_signing_dir"
TAURI_SIGNING_PRIVATE_KEY="$runtime_signing_dir/updater.key" \
TAURI_SIGNING_PRIVATE_KEY_PASSWORD='' \
TAURI_BUNDLER_DMG_IGNORE_CI=true \
  pnpm --dir apps/desktop tauri build --ci --bundles app,dmg \
  --config "$runtime_signing_dir/tauri.runtime-check.json"
app_path="$(find target -path "*/release/bundle/macos/AIPass.app" -type d -print -quit)"
test -n "${app_path}"
test -x "${app_path}/Contents/MacOS/aipass-desktop"
test -x "${app_path}/Contents/Resources/aipass-agent"
test -x "${app_path}/Contents/Resources/aipass-native-host"
node --test scripts/verify-macos-dmg.test.mjs
node scripts/verify-macos-dmg.mjs target/release/bundle/dmg
node scripts/verify-macos-runtime.mjs target/release/bundle runtime-check-reports
```

Per `agents.md` (2026-10-27), DMG validation is non-GUI: verify the image
checksum, read-only mounting, app metadata and executable payloads with
`verify-macos-dmg.mjs`. Do not inspect Finder windows, background artwork,
icon positions or Retina layout. Packaging still preserves the configured
installation appearance.

The runtime artifact gate is mandatory per `agents.md` (2026-09-08). Run the
finished DMG and updater archive through main/tray startup and both manual/cached
install-and-restart scenarios. Keep updater signature verification enabled and
retain failure reports with artifact hashes. A startup-only `.app` check does
not satisfy this gate. Release jobs use production-signed artifacts; the branch
bundle above uses a disposable updater key. See `docs/desktop-artifact-validation.md`.

Report each completed local job as `rust (macOS)`, `node (macOS)`, and `macOS desktop bundle`, including failures or required checks that could not run. Report remote GitHub Actions results separately; never claim local Ubuntu checks ran.

### Release tag pushes

The branch gate does not authorize pushing a `v*.*.*` tag. A release tag also triggers `.github/workflows/release.yml`, including version consistency, cross-platform CLI builds, extension packaging, macOS signing and notarization, updater verification, release metadata, GitHub draft release creation, and R2 upload.

Before a release tag push:

- Run the complete branch gate above against the tagged commit.
- Re-read `release.yml` and execute every command reproducible natively on macOS for the exact tag. Never start an Ubuntu/Linux container or VM for release validation; platform-specific Linux/Windows jobs run in GitHub Actions.
- Confirm every required GitHub, Apple, Tauri, and Cloudflare secret and external prerequisite without printing secret values.
- Verify required remote release credentials and prerequisites before pushing the tag. Validate macOS assertions locally and monitor all required GitHub Actions release jobs through completion. Missing local Linux/Windows execution is not a blocker and must not trigger container setup.
