# Native UI continuation, 2026-11-15

> Historical checkpoint. Production cutover is now implemented; see [the native cutover record](native-cutover-20261115.md) for current ownership and validation. The findings below describe the earlier state.

Scope follows the latest user instruction: skip Keychain-dependent regression,
animation matching and smoothness sampling. Reuse already-reviewed cases whose
rendering and behavior inputs are unchanged. These waivers are not passing results.

## Corrected UI functions

- Certificate selection/export now clears a previous failure when retrying.
  Cancellation, oversized keys, successful retry, acknowledged secret retirement
  and results arriving after close/lock/reopen have focused native tests.
- Tool configuration previews preserve read-only behavior, suppress duplicate
  writes while busy and clear a failed apply when retrying. Long errors scroll
  within 96px so the code and footer remain available at 960×640. Geometry mapping
  accounts for the inserted error region.
- Normal native launch now connects to the Agent without `--parity-preview`.
  Synthetic flags remain separate. Startup reads Agent language/security settings
  before unlock; login/reset/import labels update in place without losing input.
- Native provider links now open the existing editable form after unlock.
  Duplicate matching uses the full active catalog, normalized API endpoints and
  tenant queries. Duplicate confirmation, cancel/Escape, queued arrivals during
  editing, optional metadata, escaped assignments and lock-time secret retirement
  are covered. Receiving a link does not write to the vault. The OS callback and
  cold-launch arguments are wired; second-process forwarding still belongs to the
  pending singleton replacement.
- Provider endpoint/domain blur now performs the reference's inference while
  respecting separately touched provider/interface/auth choices. Agent endpoint
  evidence and domain matching moved into a registry module; native provider
  inference is checked against the existing shared-schema cases.
- The update-channel selector displays the worker's acknowledged feed. Missing
  workers and failed channel changes no longer leave an unconfirmed selection.
  This fixes UI state; production updater wiring is still separate.
- Reviewing the new Chinese confirmation exposed a renderer defect: a closing
  question mark wrapped onto a line by itself. GPUI now keeps CJK closing
  punctuation with the preceding character. Shaped-text regressions cover 13
  punctuation marks across five widths; affected Chinese captures are refreshed.

## Evidence and reuse

`artifacts/desktop-parity/admin-final-states-20261115/` retains focused test logs,
command exit codes and native GPU captures. `validation.json`,
`links-validation.json`, `final-validation.json` and `completion-validation.json`
identify the commands run, including retained failures and their later corrections.
`renderer-validation.json` is the final successful guard after the punctuation
fix: workspace strict Clippy, feature-enabled native strict Clippy/build, four
typography tests, five vendor-provenance tests, repository checks and formatting.
Repository checks cover 1,010 source files and 18 unchanged size-policy exceptions.
The registry and Agent inference regressions exercise the extracted shared code.
The native import test checks editable values and retained secret-editor handles
after retirement. No Keychain test or earlier broad UI matrix was repeated.

Six new proxy preview captures cover read-only, busy and long failure states in
English light and Chinese dark at 960×640. Eight provider-link captures cover
normal/long confirmations, both locales and both themes at the same minimum size.
The latter have a capture-time binary hash and source manifest; subsequent inference
changes do not alter the confirmation rendering. These are offscreen GPU images
and scripted input tests, not physical OS input or animation acceptance. Reviewed
images retain readable controls and visible action rows; no pixel-equality claim
is made. The earlier settings/proxy/provider matrices remain their original,
bounded evidence and are not described as rerun against the latest binary.
`cjk-corrected/` supersedes the four Chinese confirmation captures after the
punctuation fix, with its own binary/source hashes and geometry report. English
captures remain unchanged evidence. All six proxy-preview images and the normal
English-light/Chinese-light and long English-dark/Chinese-dark confirmations were
visually reviewed; the refreshed Chinese cases additionally check the corrected
punctuation placement.

## Tauri dependency cleanup

CLI installation and deep-link parsers moved from Tauri into
`aipass-desktop-platform`; their existing regressions run at the new location.
Native tool icons, logo and bundle icon now live under `apps/desktop-native/assets`
with source notices preserved. The copied images retain the original bytes.

The Tauri runtime is **not deleted**. Its tray/singleton activation, CloudKit
transport and production updater/package entry points still have live callers.
Deleting those now would remove behavior and break packaging. This is a code
dependency remaining to migrate, not a requirement to repeat Keychain, animation
or already-passed UI checks. Native visible input/system-appearance round-trip
evidence remains limited by the previously recorded window-interface failure;
this continuation did not retry that unchanged environment.

No commit, push, remote CI, release or packaged native cutover is claimed here.
