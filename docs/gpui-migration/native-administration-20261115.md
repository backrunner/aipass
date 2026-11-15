# Native settings and local proxy checkpoint, 2026-11-15

This checkpoint implements the settings drawer and local-proxy workspace that
were absent in the [cutover audit](cutover-audit-20261114.md). It does not accept
the entire UI or remove Tauri. The latest 2026-11-15 user instruction waives
animation matching and foreground smoothness sampling, not static UI or functions.

## Implementation

- Settings: General, Security, Sync, Local Proxy, Backup and About tabs; system
  appearance/language preferences; import detection; security policy; local,
  iCloud, OneDrive and WebDAV targets; conflicts/devices; control-panel accounts,
  certificates and keys; model pricing; export and browser-extension controls.
- Local proxy: route list, context actions, configuration, credential search,
  editable member ordering/enabling/weights/models, retry settings, integration
  previews, search APIs/MCP, status grid, usage chart/table, channel details and
  selectable live logs. Usage-range menus support hover and keyboard activation;
  read-only integration previews cannot apply configuration. Empty/loading/error/
  confirmation branches are present.
- Administration requests use the existing authenticated Agent client. Surface
  and generation identity retire late responses after navigation or locking.
  Background refresh does not overwrite an open draft. Read-only loading can be
  dismissed; an outstanding write keeps its owner until completion. Save-and-sync
  first saves the edited destination. Password editors are retired after use.
- Native number controls preserve integer precision, keyboard stepping, pointer
  hold/repeat and cancellation on outside release or loss of focus. The credential
  search menu skips added rows, supports keyboard selection and restores focus.
  Indexed editor removal retains the correct surviving field identities.
- Logs use a virtual list and retain the reader's position when bounded history
  trims; tail following is separate. Logfmt highlighting handles quoted/escaped
  values and uses selectable native text. Fixtures contain no user vault data.
- The settings drawer uses separate horizontal/vertical scales. GPUI input,
  scroll and IME transforms and Metal/WGPU composites now use both axes. Vendor
  changes remain pinned, licensed and recorded in `vendor/upstream.json` and
  the maintained patches. Third-party size exceptions do not extend to app code.
- Browser-extension/native-host platform adapters moved from Tauri's large
  bridge into `aipass-desktop-platform::browser` modules. Tauri retains thin
  wrappers, and GPUI uses the same implementation. This is extraction, not
  deletion of the Tauri runtime or acceptance of production startup.

## Comparison and corrections

`tests/desktop-parity/capture-administration.mjs` compares the frozen reference
with the native GPU export using shared synthetic settings/proxy/log fixtures.
Each round retains paired PNGs, geometry, unsupported bridge calls, source hashes
and a copied executable with its SHA-256. The geometry tolerance is 0.5 logical
pixels; it has not been widened to hide differences.

The correction rounds found and fixed wrapped API badges, cumulative row-height
errors, advanced-card border/line-height differences, fixed-model hints, picker
placement, search endpoint height, number controls, dark footer corners, drawer
scroll-fade clipping, wide chart summaries, table column sizing, cost formatting
and channel indicators. Image review also found that binding member models hid
the inbound-protocol picker. Both selectors now remain visible, with their own
geometry assertions rather than relying on outer dialog bounds. Proxy dialogs
now include the scroll fade, matching field insets and the route form's separate
inner height limit; body/footer bounds are also compared. The closed advanced
header no longer paints an extra divider, including on hover.
Channel details are accessible through hover or keyboard focus; activity motion
respects Reduce Motion.

A second image review of the settings contents found differences that the outer
checks had missed: missing sleep/screen-lock descriptions, password reveal
controls absent from the reference, oversized placeholder dots, full-width
instead of inline actions, missing action icons, misplaced refresh buttons,
upstream mode labels, unequal segmented controls, and cumulative card spacing.
These are corrected. The observations now include every top-level settings card,
appearance/language/sync segments, and the three control-panel sections.
Segmented options retain their normal inactive hover text color change. Small
actions use the reference's 9px horizontal padding, including import, pricing,
access-code generation and proxy search; the import action has its own bounds
assertion.

Chinese review exposed a renderer issue: CoreText used a half-width alternate
for terminal CJK punctuation. Its shortened advance let a description remain on
one line and changed an adjacent control's flex allocation. The Apple backend
now retains the nominal punctuation advance; terminal/interior-width regression
tests and bilingual captures cover the correction. Inline segment sizing uses
shaped intrinsic widths and the reference's flex constraint, without fixed
per-label widths or altered comparison tolerances. Empty password inputs keep
the ordinary placeholder font; entered secrets retain the password glyph font.

Matrix reports are in ignored local artifact directories:

- `artifacts/desktop-parity/admin-settings-corners/`: all 56 settings cases
  passed the mapped geometry checks, after fixing the drawer's bottom corners.
- `artifacts/desktop-parity/admin-proxy-correction/`: all 80 proxy cases passed
  the mapped geometry checks. Subsequent image review found the missing protocol
  picker, which those checks did not cover; this is not a full visual pass.
- `artifacts/desktop-parity/admin-route-controls/`: all 40 route-form cases
  passed, including the individual strategy/protocol controls. Later body/footer
  assertions exposed the two-pixel capped-form difference, fixed by matching the
  reference's nested height constraint.
- `artifacts/desktop-parity/admin-verified-settings/` and
  `artifacts/desktop-parity/admin-verified-proxy/` contain the earlier field-inset,
  scroll-fade and nested-height reruns. They preceded the settings descendant
  review above and must not be cited as complete settings visual acceptance.
- `artifacts/desktop-parity/admin-settings-descendants-verified/` contains the
  56-case descendant check after the card/control and CJK fixes. A final settings
  rerun is recorded in `artifacts/desktop-parity/admin-settings-final/` after
  simplifying segmented intrinsic measurement and restoring inactive hover.
- `artifacts/desktop-parity/admin-proxy-descendants-verified/` repeats all 80
  proxy cases with the updated shared controls and Apple text backend.
- `artifacts/desktop-parity/admin-settings-hover/` compares the inactive theme
  option under synthetic hover in eight viewport/language/theme combinations.
- `artifacts/desktop-parity/admin-small-actions-final/` checks the 40 affected
  settings/general, control-panel, proxy header and search-dialog cases after
  small-button corrections. The import action now has an independent bounds
  assertion. `admin-proxy-header-final/` records the final header rerun.

All five reports above completed with their mapped geometry checks passing:
56 settings, 80 proxy, eight hover, 40 small-action follow-up and 16 final-header
cases. Follow-up counts overlap the main matrix; they are not additional unique
surface coverage. Each report identifies its exact source hashes and executable.

The matrix covers seven settings states and ten proxy states, each at 960×640
and 1280×820, English and Simplified Chinese, light and dark, with reduced motion.
Earlier failed captures remain available; they were not overwritten or relabeled.

These checks measure drawer/tab/body, settings cards/segments/control-panel
sections, pane/dialog and route form/picker bounds. They do not assert every
descendant's bounds or pixel equality; paired images remain part of review. The native blur
and the headless reference backdrop differ. The native narrow proxy header keeps
its complete title/actions on multiple rows; the frozen reference truncates the
title. These are explicit comparison boundaries, not hidden masks.

## Local validation

Native macOS, Homebrew Rust 1.99.1:

- `cargo test -p aipass-desktop-native --features parity-render`: 94 library,
  176 UI/binary and four integration tests passed. The real-Agent provider test
  verifies conflict/fail-closed behavior in disposable storage; it does not prove
  successful access to the user's Keychain.
- `cargo test -p aipass-desktop-platform --lib`: 24 passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed, without
  exclusions or added lint suppressions.
- Native `parity-render` build and additional feature-enabled strict Clippy:
  passed. `cargo fmt --all -- --check`: passed.
- WGPU's standalone library tests: 39 passed using the pinned local GPUI patch
  and locked dependencies. Existing upstream warnings remain visible. This is
  macOS shader/layout validation, not Linux/Windows native presentation.
- Vendor provenance: all five inventories/patch checks passed.
- Desktop typecheck: zero errors/warnings. Repository structure/size check:
  994 source files, 18 existing oversized exceptions; passed.

The mock updater feed check exposed an intermittent connection failure. Reading
the complete bounded HTTP header did not solve it. Diagnostic request capture
identified a LocalStack `GET /` port-discovery probe consuming the fixture's only
response before the updater connected. The fixture now rejects unrelated paths,
keeps accepting within a deadline, and explicitly restores blocking mode on
accepted macOS sockets. Every feed case first sends a deliberate discovery probe
and checks its 404, then checks the actual updater result. The full native test
command passed afterward. Production updater request behavior was not changed.

This is a focused development checkpoint. The complete workspace/packaged
artifact delivery gate and remote CI were not rerun for these uncommitted edits.
Earlier delivery results do not certify this new tree.

## Expanded populated-state review

The continuation adds populated sync conflicts/snapshots/devices, pricing/retry
rows, certificate controls, generated access codes, the inline bind editor and
installed-tool integration content. Corrections include:

- Conflict and device metadata, disabled invalid-snapshot actions, compact
  action rows, fallback-font metrics and fractional card geometry. Chinese
  metadata uses separate fields instead of the frozen reference's dotted joins.
- Retry dividers, pricing grid gaps and the half-width final optional price;
  remote-unlock guidance beside its switch; certificate disclosure spacing and
  compact file selectors. Native file-selector labels use the app locale and
  `Browse…`/`—`, rather than WebKit's platform-generated file-input text.
- Access-code line height, selectable text and compact guidance; explicit SVG
  paint colors where correctly sized icons had been invisible. Final image
  review also restored the running badge's green fill and the compact,
  transparent danger action instead of a full-width colored button.
- A focused inline bind editor with Enter/Escape, retained failed drafts and
  deferred-save handling behind a failed background refresh.
- Installed-tool icon tiles, status checks, unavailable reasons, small actions,
  endpoint/token columns and horizontal text fades. Integration presentation is
  a separate module from preview/apply ownership. Setup and the card share the
  protocol-specific endpoint, including Anthropic's base URL without `/v1`.

Additional retained reports (two sizes, two locales, two themes unless noted):

| Report directory under `artifacts/desktop-parity/` | Coverage | Result |
| --- | --- | --- |
| `admin-settings-populated-verified` | 96 settings cases, including conflicts, snapshots, populated proxy settings and certificate disclosure | All mapped geometry checks passed |
| `admin-certificate-visible-verified` | Eight scrolled certificate cases | All mapped geometry checks passed |
| `admin-certificate-scroll-final`, `admin-pricing-scroll-final` | Eight certificate and eight pricing cases after nearest-pixel reference scrolling | All mapped geometry checks passed |
| `admin-panel-access-final` | Eight generated access-code cases with remote-unlock guidance, before final badge/action paint correction | All mapped geometry checks passed |
| `admin-panel-access-accepted-geometry` | Eight access-code cases after badge/action paint correction, including the revoke button bounds | All mapped geometry checks passed; native interaction remains unaccepted |
| `admin-integration-final` | Eight installed-tool cases, including individual endpoint/token values and actions | All mapped geometry checks passed |
| `admin-cjk-providers` | 32 add-menu/provider-form cases after the CJK correction | Component/style checks passed; not full style acceptance |
| `admin-cjk-runtime` | 16 runtime proxy/balance cases | No mapped geometry violations |
| `admin-cjk-webhooks` | 16 notification cases, including the scrolled bottom | No mapped geometry violations |

The native narrow header retains its intentional wrapping difference. Bind-chip
and installed-integration comparisons therefore use explicitly reported local
coordinates while retaining raw viewport bounds; the scrolled integration card
must also be fully visible. These reports do not assert whole-page position or
pixel equality. Programmatic reference scrolling rounds to the nearest logical
pixel so WebKit's truncation does not add a systematic pixel of scroll error.
Failed rounds remain recorded, including the populated Chinese metadata drift,
wide integration reason wrapping and local HTTP-fixture failures.

Native test additions cover bind focus/keyboard behavior, rejection and queued
save completion, protocol-specific setup endpoints, horizontal scroll extent,
and rendering/retirement of the conditional access-code state. A correction
round caught duplicate hover registration on the revoke button; the action now
uses the existing shared danger control, and the conditional render is tested.
Logs for the final checks are in `admin-validation-20261115/`; the full native
test run is `final-reviewed-native-tests.log`, and the strict root workspace command
after the final paint correction is `accepted-workspace-clippy.log`. The latter is the mandatory
`cargo clippy --workspace --all-targets -- -D warnings`, with no exclusions.

The native computer-use connection recovered in this continuation. Fresh,
synthetic, ad hoc signed bundles were recorded in `admin-native-20261115/`.
Opening bind editing and dismissing settings changed accessibility state, but
screenshots stayed on old frames. Read-only NSWorkspace checks showed Edge
frontmost and both test apps inactive; Raise, click and Finder-open attempts did
not establish current presentation. Both owned test apps were closed and their
absence verified. This is neither a native visual pass nor proof of an app
rendering defect; input tests and GPU exports remain separate evidence.

## Remaining acceptance

- Actual integrated native keyboard/mouse behavior on these new pages and a
  visible system light/dark round trip remain open. The original native-pipe
  failure and the later stale-frame attempt above did not establish acceptance.
- The expanded matrix covers the populated branches above, not every failed
  operation, file-selection result, certificate error, tool setup/preview state
  or tool apply result. Normal-motion intermediate frames are now out of scope.
  Broader whole-app Chinese capture review
  is still incomplete despite the additional provider/runtime coverage. These
  boundaries must remain explicit before declaring every UI surface consistent.
- Functional completion is active under the latest instruction. The
  [failure and retry continuation](native-administration-results-20261115.md)
  corrects partial preference acknowledgements, sync-save results, draft retention
  and result banners. Production entry/lifecycle/updater wiring is not established
  by the administration UI. Failed proxy/control-panel saves keep their draft;
  complete native end-to-end acceptance remains open.

Tauri remains until the UI gate is satisfied. Shared Svelte controls/locales,
platform resources and executable packaging must be preserved during cleanup.
