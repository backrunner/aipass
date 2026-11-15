# Subscription audit — 2026-11-13

The native subscription surface was incomplete. This review adds the missing
connection/import flows, account/usage presentation and provider runtime editor.
It is a component checkpoint, not acceptance of the complete GPUI migration.
The follow-up below implements the OAuth CLI integration; real-window/presentation
acceptance remains open. The later [renderer follow-up](renderer-effects-20261113.md)
implements subtree scaling and backdrop blur, including input-coordinate handling.

## Follow-up while visual verification is deferred

The user confirmed the Mac is locked and requested source repairs first, with
interactive verification after returning. No native window was launched, no new
visual capture was run, and no presentation sampling was attempted in this follow-up.
The earlier screenshot matrix below describes its original hashed checkpoints,
not these changed sources.

- Official Codex/OpenAI and Claude OAuth entries now expose their matching installed
  CLI integration. Native plans use `Official` mode and the exact secret ID; OAuth
  still cannot enter API-key writers. The card hides API-key selection/auth-mode
  controls, shows the bound official account, and offers switch/re-login actions.
- The re-login dialog uses existing English/Chinese copy and theme tokens. Agent
  owns start/poll/code/cancel and all credential writes. A fresh preview is obtained
  after `LoginRequired`; reconnect targets the CLI-bound account even when another
  provider is selected. Restore can continue through login. Current CLI credentials
  remain under the existing Agent transaction and backup policy; no CLI is killed.
- Closing or replacing a login cancels known tickets and cleans up late starts.
  Cleanup requests serialize with visible requests without consuming their pending
  state. Lock releases the UI's in-flight marker because Session discards retired
  replies. Transport failures return to the owning panel. A committed login triggers
  catalog refresh, including when its visible dialog was already dismissed.
- Authorization codes are cleared with undo history on submission and dismissal.
  Terminal status without an applied result remains an error. A dialog's polling
  timer ends on close/error; browser opening is user initiated and URL validated.
- Connect/provider-list/import scroll edges now use the same bounded fade as the
  settings editor. Reversing a partly entered popup no longer halves its vertical
  offset; position and opacity interpolate independently and honor reduced motion.
- Request dispatch, integration control, native login orchestration and login UI are
  separate Rust modules. All new production files remain under 1,000 lines.

Automated regression coverage includes exact native selection, reconnecting another
bound account, renewed preview IDs, late-start cancellation, cleanup failure during
a new preview, lock/unlock, transport errors, committed-login refresh, terminal
login failure, single-use code/undo erasure and continuous modal reversal. Native
parity-feature suite (148 tests), workspace tests/build, formatting, workspace
Clippy and parity-feature Clippy passed on macOS. The repository gate checked 867
source files; its 19 pre-existing oversized files remain unchanged by this patch.
The revised pitfalls skill also passed its validator. Source-check logs are under
`artifacts/desktop-parity/subscription-patches-20261113/`.

## Reviewed surface and changes

| Surface | Implemented and checked | Evidence boundary |
| --- | --- | --- |
| Connect directory | Agent-owned catalog of 41 providers, localized names, search, empty/error/retry states, brand artwork | Both locale dictionaries and all catalog method/prompt metadata checked; vendor login not performed |
| Connection flow | Claude CLI/browser/code flow, community CLI/API/device/code methods, region choices, install/retry, polling/cancel, reconnect exact account | Typed requests, synthetic states and GPUI event/state regressions |
| Local account import | 15 provider choices, default/custom directory, progress, imported/existing/needs-login/missing/failed/cancelled outcomes, retry only eligible sources | Synthetic captures; no scan of the user's real CLI accounts |
| Account and quota | Identity, scopes, refresh/reconnect/console, stale/empty/console-only/unknown quota, progress/reset dates/credits, expansion/collapse | Unknown/invalid usage never becomes zero; explicit Agent quota support overrides historical provider-name heuristics |
| Subscription settings | Quota tracking/interval/cap, paid/reset credit policy and native-switch route preferences | Existing Agent DTOs, account-scoped completions |
| Connection settings | Inherit/system/direct/environment/custom proxy and authentication retain/clear semantics | Real disposable Agent get/set/locked-read roundtrip |
| Balance settings | Enable/disable, GET/POST, URL/JSON path/unit, multiline body, request header add/remove, saved-configuration probe | Draft lifecycle regressions; external balance endpoint not contacted |
| Notifications | Enable/disable, URL/authentication, explicit secret removal, 8 event choices, add/remove with an 8-hook limit, saved-configuration test action | Synthetic UI and typed Agent request wiring; no webhook sent |
| Keyboard and motion | Separate picker focus, Escape, tab arrow navigation, initial selected-provider scroll, disclosure reversal, popup/overlay timings, 150 ms switches, settings scroll-edge fade | GPUI test platform and offscreen captures, not physical input or presentation timestamps |

The toolbar now opens the subscription directory and local import flow. Successful
account commits are acknowledged independently of catalog refresh. Late starts
are cancelled; late refresh/settings replies cannot update another account,
including A → B → A selection. Opening a connection dialog does not discard an
independent quota refresh or leave its spinner stuck.

Settings retain draft inputs on failed saves, matching the original dialog.
Successful save, dismissal, lock and account changes erase sensitive inputs and
undo history. Empty authentication values preserve stored credentials; explicit
clear flags remove them. Disabling balance or returning a proxy to inherited mode
erases only that removed draft, leaving unrelated edits intact. A loading dialog
can close; an in-flight mutation blocks ordinary dismissal, while lock always
retires the surface. Requests carry account ID and an independent settings epoch.

Presentation corrections include header/footer dimensions, import count placement,
per-row status/error line heights, provider picker scrolling, selected event colors,
compact icon-only removal, request-header column proportions, multiline body size,
field hints and disabled colors in both themes. A larger-window import case exposed
15.203125 px of accumulated height error that minimum-window scrolling concealed.

## Matched static evidence

Reference components are loaded from frozen baseline
`65a1f1f5a5e1b51311132659f9b972131c8ea551`, with the same synthetic JSON as the
native capture. Production Svelte components are not rewritten to match native output.

`tests/desktop-parity/capture-subscriptions.mjs` records the native binary hash,
source and fixture hashes, screenshot hashes, component bounds and bridge traces.
The declared tolerance is **3 logical pixels**. Required reference/native components
must exist; absent observations are failures. This is a geometry threshold, not a
claim of pixel-identical text rasterization, colors or animation.

The matrix covers 960×640 and 1280×820, English/Chinese, light/dark:

- `subscription-matrix-20261113-r2`: 24 scenarios × 8 configurations = 192 cases.
  Eight geometry violations in the English large-window import cases were retained
  as failure evidence. The other mapped components passed.
- `subscription-final-20261113`: 48 targeted rechecks, including all three import
  states, passed with a maximum mapped delta of 1 logical pixel.
- `subscription-runtime-final-20261113`: all four settings tabs plus balance and
  notification bottom-scroll states, 48 passing cases (maximum delta 0.890625 px). Internal field bounds include the
  body textarea, request headers, authentication controls and all 8 event rows.
- `subscription-finishing-20261113`: final notification scroll/color/icon recheck
  after the last small visual adjustment: all 8 cases passed, maximum delta 0.5 px.

Each directory is under `artifacts/desktop-parity/` and contains `report.json`,
paired screenshots and an `index.html` overlay review. Earlier failed probes are
preserved. These are multiple explicitly hashed checkpoints, not a claim that one
binary ran every case. The final loading-dismissal change is covered by regression
tests; it does not change the settled loaded fixtures.

The multi-checkpoint summary (`subscription-checks-20261113/geometry-summary.json`)
selects the latest applicable case for 208 unique combinations and 1,328 mapped
component observations. All pass the 3 px threshold; maximum delta is 2.203125 px.
It preserves each selected checkpoint and hash rather than attributing all cases
to the last binary.

Representative light/dark and English/Chinese settings screenshots were inspected,
including bottom scroll positions. Static captures use reduced motion. The frozen
English import fixture has a Chinese source-label fallback; native uses the correct
English dictionary value rather than copying that baseline localization defect.

## Local validation

Logs and command/exit-code records are in
`artifacts/desktop-parity/subscription-checks-20261113/`.

- macOS Homebrew Rust: `cargo fmt --all -- --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace` and `cargo build --workspace` passed. The workspace
  test checkpoint reported 1,428 passed tests. Later UI-only changes were rechecked
  with the native suite and workspace Clippy; see `final.json` for the final result.
- The final native parity-feature suite passed 141 tests and Clippy. The final targeted subscription
  run covers 25 tests, including load dismissal, failed-save retry, forced lock,
  late responses, independent refresh, keyboard tab focus and multiline input.
- Real authenticated disposable-Agent tests cover catalog retrieval, settings save,
  secret redaction, omitted-secret retention, explicit credential removal and
  locked read rejection. Existing provider/update integration tests remain in the
  native suite. No production vault was used.
- Node install, license audit, lint, typecheck, tests and build passed; unchanged
  inputs reused Turbo's matching local cache. Embedded control-panel output stayed
  unchanged. The desktop typecheck was also run directly earlier in this review.
- Repository size/ownership checks and comparison-tool tests are recorded alongside
  the final checks. New subscription modules remain below 1,000 production lines;
  tests remain below 1,500. State, actions, controls, motion, presentation, Agent
  requests and runtime settings are separate real Rust modules.

Comparison App packaging and ad hoc signature verification succeeded. These are
preview apps, not production DMG/updater artifacts. This turn did not publish,
commit or push, run remote CI, or certify the production artifact runtime gate.

## Remaining gaps and acceptance blockers

1. **OAuth CLI integration awaits interactive acceptance.** The follow-up implements
   native switch/reconnect/login/restore through Agent. Unit/state tests do not prove
   real vendor login, browser callbacks, live CLI credential replacement or visual
   parity of the newly added dialog.
2. **Popup rendering awaits visual acceptance on the new renderer.** Opacity,
   vertical displacement, reference timing curves, subtree scale (.96/.97), and
   same-frame 4 px backdrop blur are implemented. The renderer follow-up covers
   scaled input, clipping, cache invalidation, portals and text services. Its
   synthetic tests do not replace the pending light/dark, locale, focus and
   real-window motion comparison. Earlier screenshot checkpoints predate this
   renderer and are not acceptance evidence for the current binary.
3. **Real-window interaction is deferred by the user.** Before that instruction, the test process
   remained alive and idle, but CUA repeatedly returned `cgWindowNotFound`, including
   the final retry. No physical click, typing, focus, scrolling or native accessibility
   acceptance is claimed for this subscription checkpoint. The owned test process
   was terminated after diagnostics; screenshots are offscreen evidence.
4. **Onscreen animation performance is unaccepted.** This work does not resolve the
   zero-time Metal presentation callbacks documented in
   [the presentation report](ui-performance-20261113.md). Render-loop tests, static
   images and animation curve checks do not establish display frame pacing.
5. **Live provider authentication/probe/notification behavior was not exercised.**
   Agent ownership and request mapping are verified; actual vendor/browser/CLI
   sessions and remote endpoints require a dedicated interactive acceptance run.

These gaps prevent marking the entire subscription experience or GPUI desktop as
fully equivalent. No unfinished surface is counted as passing because its dialog
bounds or screenshot looked similar.
