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

## UI interaction tests

Install the browser binaries once after installing workspace dependencies. Run the suite from the repository root with Node 26, pnpm and Cargo.

```sh
pnpm exec playwright install chromium webkit
pnpm test:e2e
pnpm exec playwright show-report
```

The suite builds the applications, starts loopback servers, and runs these scenarios:

| Project | Entry point and test boundary |
| --- | --- |
| desktop / desktop-webkit | Shipped Svelte workspace at 960×640, with a per-page Tauri command and event fixture. Covers authentication, provider lifecycle, credentials, subscriptions, proxy routes, usage, integrations, settings and failed operations. |
| panel / panel-webkit | Embedded control panel served by the Rust Agent test fixture with a disposable vault and tool configurations. Includes desktop and narrow-browser navigation, authenticated mutations and readback. |
| extension | Unpacked extension in a fresh Chromium profile, with real tabs, storage, content scripts and service worker. Its test copy substitutes Native Messaging and clipboard writes. |
| web / web-webkit | Built English and Chinese site, documentation navigation, search, themes and downloads with fixture release metadata. |

Filter a run with `pnpm test:e2e --project=panel`. After rebuilding the affected apps, `pnpm exec playwright test --project=desktop desktop-auth` runs a single suite directly. The HTML report contains layout captures and failure diagnostics. To run suites concurrently, build the apps once and use `pnpm exec playwright test` for each suite. Give each additional process a distinct port offset, at least four ports apart, such as `AIPASS_E2E_PORT_OFFSET=100`. Its results and report are written under `artifacts/ui-e2e-100/`.

On macOS, `swift test --package-path apps/desktop/src-tauri/swift-tray` exercises the SwiftUI tray controls in AppKit and the right-click menu. After building a runtime-check app, test the packaged interfaces with the following commands.

```sh
node scripts/ui-e2e/packaged-extension.mjs target/release/bundle/macos/AIPass.app runtime-check-reports
node scripts/ui-e2e/native-desktop.mjs target/release/bundle/macos/AIPass.app runtime-check-reports/native-desktop
```

The extension check registers the bundled native host in a disposable Chromium profile and reads the resulting vault state through the bundled CLI. The desktop check sends native keyboard input to an isolated copy of the app. It needs a macOS GUI session with Accessibility permission for the invoking terminal. Its JSON report records window geometry and Agent lock state. Window screenshots also require an unlocked screen and Screen Recording permission. Packaged startup and update tests use the [desktop artifact checks](docs/desktop-artifact-validation.md).
