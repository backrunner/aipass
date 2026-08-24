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
