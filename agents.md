# Commit Convention

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
