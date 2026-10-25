# Commit Convention

## Source ownership and module size

- Subscription authentication, quota and protocol conversion belong to AIPass's Rust crates. Define behavior against vendor protocols and the official CLIs; maintain the implementation and architecture in this repository. Do not import third-party provider engines, source checkouts, generated adapter downloads or runtime plugin loaders. Keep required third-party license notices accurate.
- Run `pnpm repo:check` after structural or dependency changes. `pnpm lint` and CI run the same gate.
- Keep new production source files at or below 1,000 lines and test files at or below 1,500 lines. Split by responsibility using real modules, not `include!` fragments or generated concatenation.
- Existing oversized files are tracked with owner, reason and a no-growth limit in `scripts/source-size-policy.json`. Split the affected responsibility before expanding them; remove stale entries when a file falls below the limit. Do not increase a limit merely to make a check pass.

## Desktop UI Validation

- The desktop UI is shipped inside Tauri. The authoritative minimum viewport is `960x640`, from `apps/desktop/src-tauri/tauri.conf.json` and `tauri.dev.conf.json`.
- Review and verify desktop pages, lists, forms, and dialogs at the Tauri minimum window size. Do not add phone-sized or browser-only responsive workarounds unless a separate product surface explicitly requires them.

## UI copy and compactness

- Keep all UI surfaces concise and compact. Remove descriptions that do not help users understand an action, state, or decision; do not add explanatory text merely to fill space.
- Prefer short labels, single-line list rows, and restrained spacing. Keep necessary guidance for errors, empty states, and consequential actions clear and actionable.
- Avoid joining Chinese UI text with middle dots or bullet separators (`·`, `•`). Prefer separate fields, layout groups, or natural sentences; do not concatenate unrelated labels, states, and metadata into dotted text.
- Do not repeat the same information within a page or dialog, including heading/body/footer descriptions, account identity, status, or button labels. Give each fact one clear place.
- Review UI changes for unnecessary descriptions, duplicated copy, dotted Chinese text, and excess spacing alongside minimum-window validation. Apply the same rules to every locale and loading, success, empty, and failure state.

Use this format for all commits:

`op(component): desc`

Rules:

- `op` should be a short verb such as `add`, `fix`, `refactor`, `docs`, `test`, or `chore`.
- `component` should name the main subsystem, crate, or app.
- `desc` should be short, imperative, and lowercase.
- Keep each commit scoped to one concern whenever possible.

Examples:

- `chore(repo): tighten ignore rules`
- `refactor(native-host): split request handling`
- `fix(sync): handle webdav conflict metadata`

## Proportionate Local Validation

User instruction, 2026-10-20: keep checks for simple fixes focused and efficient.
This replaces the former full-workspace gate for every commit or push, except
for the mandatory Clippy guard below (2026-10-25).

- Choose checks from the behavior and dependencies affected by the intended diff. A small diff that changes authentication, secrets, persistence, sync, IPC, startup, updates or packaging is not automatically low risk.
- Documentation-only edits need a diff/format/link review. Run `pnpm repo:check` when changing repository instructions, structure or dependency rules; validate edited skills with their skill validator.
- Simple copy, locale, CSS, layout and isolated UI fixes need the affected package's typecheck, relevant existing tests when behavior changes, and a `960x640` visual check for visible changes. A copy/CSS-only fix does not require new tests, a workspace build, Rust checks beyond the mandatory Clippy guard, or a DMG build.
- Ordinary logic changes need the affected packages/crates and their dependent callers checked. Use focused regression tests; expand the scope for shared contracts, failures or uncertain impact.
- Changes to security, credential ownership, durable storage, sync, IPC contracts, runtime lifecycle, dependencies, build configuration or CI require the complete local gate in `.codex/skills/aipass-repo-guardrails/SKILL.md`. Every release also requires that gate and the artifact checks below.
- Refresh affected tracked generated files before committing. Shared UI locales are included in the embedded control panel: rebuild `@aipass/control-panel` when changing them and include its generated output in the same concern.
- Reuse passing checks when their source, tests, configuration and dependency inputs still match the committed files. A commit, signature or message-only amend does not invalidate checks. Rerun checks affected by later content changes; do not repeat unrelated passing checks.
- Keep unrelated work intact and out of the commit. An unrelated dirty worktree alone does not require a temporary checkout or copying build caches; isolate when it is needed to validate the intended files independently.
- Install dependencies when missing or changed. Read the relevant CI jobs when needed to select checks; reserve a complete workflow reread for the full gate or workflow changes. Remote CI keeps its configured coverage; distinguish local results, skipped checks and remote results.

## Mandatory Clippy Guard

User instruction, 2026-10-25: all Rust code must pass Clippy.

- Before every commit, push or release, require a successful `cargo clippy --workspace --all-targets -- -D warnings` from the repository root for the intended code. This applies even to documentation and UI-only changes; checking only the edited crate is insufficient.
- Any Clippy error or warning blocks completion of that gate. Fix the cause; do not remove `-D warnings`, exclude workspace members or targets, or add lint suppressions merely to make it pass.
- Reuse a passing result only while its source, generated inputs, Cargo manifests and lockfile, build configuration, toolchain and relevant environment still match. Otherwise rerun the full command. Record the command and result when reporting validation.
- Run the local guard natively on macOS with Homebrew Rust. Keep Linux and Windows Clippy checks in CI; a local pass does not prove platform-specific code passes remotely.
- Other checks remain proportionate to the change. This guard does not require unrelated tests or desktop bundles for a simple edit.

## Desktop Artifact Release Gate

User instruction, 2026-09-08: validate future artifacts to prevent startup and
update crashes from reaching releases.

- Run `scripts/verify-macos-runtime.mjs` against the finished DMG and updater
  archive before publication. Require main/tray startup at 960×640, responsive
  frontend and Agent, explicit install/restart and cached install/restart.
- Keep the production updater signature checks enabled. An early exit, hang,
  missing restart, wrong running version or leftover update cache fails the gate.
- Preserve failure diagnostics and artifact hashes. Signature/notarization or
  build success alone is not a runtime validation result.
- See `docs/desktop-artifact-validation.md`; local checks remain macOS-only.

## Local Validation Platform

User instruction, 2026-09-07: permanently skip local Ubuntu validation.

- Run local Rust, Node, and desktop bundle checks natively on macOS.
- Do not create, start, or retain Ubuntu/Linux containers or virtual machines to
  reproduce CI checks, including before branch pushes and nightly releases.
- Do not install Linux dependencies or block a local push/release on missing
  local Ubuntu validation. This overrides older workflow environment-matching
  instructions in repository skills and documentation.
- GitHub Actions remains responsible for its configured Linux jobs. Report
  local macOS results and remote CI results accurately and separately.

## Local Rust Toolchain

User instruction, 2026-09-07: keep local Rust managed by Homebrew and use the
latest stable version available from Homebrew's `rust` formula. Repair its
dependencies when needed; do not switch local builds to rustup as a workaround.
This preference applies to the local macOS machine, not remote CI toolchain setup.
