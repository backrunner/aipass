# Commit Convention

## Desktop UI Validation

- The desktop UI is shipped inside Tauri. The authoritative minimum viewport is `960x640`, from `apps/desktop/src-tauri/tauri.conf.json` and `tauri.dev.conf.json`.
- Review and verify desktop pages, lists, forms, and dialogs at the Tauri minimum window size. Do not add phone-sized or browser-only responsive workarounds unless a separate product surface explicitly requires them.

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
