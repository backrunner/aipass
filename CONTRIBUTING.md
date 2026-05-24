# Contributing

Run the full local checks before opening a pull request:

```bash
pnpm lint
pnpm typecheck
pnpm test
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Do not commit real API keys or unencrypted vault exports.
