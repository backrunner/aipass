# Provider pages and subscription connection verification

Date: 2026-10-03. The authoritative desktop minimum is 960×640 in Tauri.

## Mounted paths

| Surface | Implementation and Agent contract |
| --- | --- |
| Provider detail | `ProviderDetailPane` mounts `ProviderRuntimePanel`; `provider_runtime_get/set` read and save encrypted runtime options, then refresh active proxy credentials/configuration. |
| Quota and balance | Agent refresh preferences, official/community usage and bounded custom HTTP/JSON-path balance query; unknown usage stays unknown. |
| Outbound proxy | Provider-specific override or inherited global setting; proxy URL secrets never enter plaintext UI summaries. |
| Health and notifications | Agent passive status, opt-in webhook configuration, explicit test delivery. Opening a panel does not send notifications or save settings. |
| Subscription login | Existing `OAuthConnectDialog` opens `CommunityConnectDialog`; Agent catalog/start/poll/code/cancel and ticket-bound browser opening. |
| Community provider | After login, reload provider entries; detail action refreshes model catalog and usage, and local routes execute the native Rust protocol adapter. |
| Routing | Existing local route editor exposes quota-aware strategy in addition to fallback and round-robin; real backend selection respects fresh and model-scoped windows. |

Standalone legacy prototype files are not individually advertised as mounted
features. The integrated surfaces above are the source of truth.

## Automated checks

- 241 desktop tests / 34 files passed, including failed secret-save retention,
  read-only panel opening, pending login teardown, late poll cancellation and
  commit-versus-cancel races.
- `pnpm --filter @aipass/desktop typecheck`: zero errors and zero warnings.
- Desktop production frontend build passed; existing chunk-size warning remains.
- Rust integration covers credential encryption/CAS, bounded process cleanup,
  conversion/SSE, native signatures, quota eligibility and distinct Copilot
  editor/CLI sessions. Native Rust adapter tests exercise HTTP/2 duplex, outbound
  proxy selection, early quota errors, callback ownership and durable ACK refusal.

## Native desktop evidence and remaining gate

The native Rust build was rechecked using the disposable app at
`/tmp/aipass-runtime-recheck-20261003/AIPass.app`. It uses an isolated
empty vault, a local-only sync configuration and a
960×640 window. Production frontend resources were embedded using Tauri's
`custom-protocol` feature. `result.json` reports `ok: true`; startup stage logs
reach `complete`, and the runtime observer checked rendered DOM, native event-loop
responsiveness, Agent IPC and window dimensions for 20 iterations. Both the
desktop executable and bundled Agent were copied from the new Rust build.
This startup check does not exercise real provider login or generation.

The first re-review fixture path produced a 106-byte Unix socket pathname and
macOS rejected it with `AF_UNIX path too long`. Its failed startup report and logs
remain at `/tmp/aipass-runtime-subscription-rereview-20261003`. The same binaries
passed using the shorter fixture path above; `build-hashes.json` preserves their
SHA-256 values.

Initial attempts exposed stale local Agent binary version 12 versus protocol 13;
building the Agent explicitly with `rtk proxy cargo build -p aipass-agent --bin
aipass-agent` and copying that exact binary fixed the mismatch. The first Vite
resource startup also had an empty document; the production-resource run passed.
These failed attempts remain in `/tmp/aipass-runtime-subscription-review-20261002`
for diagnostics.

The native computer-control tool still returns `cgWindowNotFound` when selecting
the new running Rust review app. Consequently screenshots,
layout inspection, scrolling and click-through at 960×640 are **not signed off**.
Do not promote the startup/IPC evidence to visual acceptance. No DMG/updater
install/restart validation, real account login, paid generation, publication or
remote CI run was performed for this review.

## 中文说明

已接通实际供应商详情、运行时设置、社区登录和路由页面，后端读写由 Agent 负责。
自动测试与原生启动/IPC/窗口尺寸检查通过。电脑控制工具无法取得窗口，960×640 的
视觉和点击验收仍未完成；不得将测试或进程存活描述成视觉验收通过。
