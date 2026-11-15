# Native system appearance, 2026-11-14

Normal native windows now resolve the shared theme preference at startup and
subscribe to macOS window appearance changes. The default is System; selecting
System clears the application appearance override. Explicit light/dark preview
arguments and static captures remain deterministic fixtures.

## Behavior

- Update mounted and hidden authentication, workspace, subscription, pricing,
  credential, notes, provider-tool, integration, toast and confirmation views in
  place. Editors retain their entity identity, contents, focus and pending state.
- Prepare alternate background materials off the UI thread before applying the
  palette. Retain both materials while outgoing paint may reference their atlas
  tiles, and preserve the login background's original animation clock.
- Crossfade the complete frame over 220ms using symmetric easing. Deferred menus,
  dialogs and tooltips participate in the same composite. Provider-row hover
  colors start from the new palette instead of layering a second theme animation.
- Reverse from the displayed composite, and stop replay on resize, authentication
  surface changes or the lock curtain. Replayed paint registers no input handlers,
  focus stops or accessibility nodes. Reduce Motion applies the endpoint directly.
- Clamp Metal/WGPU image filtering to the owning atlas tile. The new GPU negative
  control returned `[255, 138, 138, 255]` at a white image's edge before the fix;
  all four enlarged edges pass afterward. This also removes a thin login-background
  edge difference caused by keeping both appearance textures resident.

## Validation

The capture harness changes `NSApplication.appearance` in an isolated synthetic
process, exercising AppKit's actual window notification and the native observer.
It never changes macOS preferences, opens a vault or acquires the foreground.
Images come from Metal offscreen rendering, with the background animation clock
held constant so geometry and palette changes can be compared independently.

- `appearance-atlas-20261114`: login at 960×640 English and 1280×820 Simplified
  Chinese; 24 captures and 18 checks pass. Includes both directions at 0/110/220ms,
  interrupted reversal and Reduce Motion. The original pixel thresholds are intact.
- `appearance-matrix-20261114`: 18 cases, 152 captures and 98 checks pass with
  zero failures. Covers recovery, provider editing, Add field, credential editing,
  notes, usage refresh, pricing groups, subscription proxy settings and login code.
  Each runs at 960×640 English and 1280×820 Simplified Chinese, in both directions
  and with Reduce Motion. Combined with login: 20 cases, 176 captures, 116 checks.
  These are the two stated size/locale combinations, not the full cross product.
  Reports retain exact binary/source hashes, differences and the earlier failure.
  Only test formatting, the standalone WGPU lockfile restoration and capture
  inventory/filter logic changed after the login captures; native production
  sources and shaders match the final matrix. Local code results and hashes are
  recorded in `appearance-matrix-20261114/validation.json`.
- `appearance-final-20261114` remains an interrupted diagnostic round with the
  original atlas-edge failure. It is not acceptance evidence for the final renderer.
- 137 native UI/render tests pass, including appearance preference propagation,
  retained editor state, material lifetime/clock, complete-frame span retirement,
  live-only deferred-popup input and the image-atlas regression.
- Five WGPU shader/transport validation tests pass locally on macOS with the
  maintained local GPUI patch. This is shader validation, not Windows/Linux GPU
  presentation. Standalone upstream profiler/test-support code emitted existing
  deprecation/dead-code warnings; the workspace's strict Clippy gates are separate.

Run the standalone WGPU checks with the local core patch (otherwise Cargo selects
the unpatched git core, whose compositor contract is different):

```sh
rtk proxy cargo test --manifest-path vendor/gpui_wgpu/Cargo.toml --locked \
  --target-dir target/gpui-wgpu \
  --config 'patch."https://github.com/sonorahq/gpui".gpui.path="/Volumes/BRData/projects/aipass-gpui/vendor/gpui"' \
  --lib shader -- --test-threads=1
```

The final source passes `cargo clippy --workspace --all-targets -- -D warnings`
and `cargo clippy -p aipass-desktop-native --all-targets --features parity-render -- -D warnings` using Homebrew Rust on macOS. Repository size/ownership checks,
formatting, diff whitespace and all five vendor provenance checks also pass.
No commit, push, release or remote CI result is claimed.

The user subsequently waived foreground smoothness sampling. Its result remains
unverified, but it is no longer a cleanup prerequisite. The subsequent
[visible-window follow-up](native-acceptance-20261114.md#visible-window-follow-up)
exercises the macOS Appearance page and retains dark/light endpoint images with
an open menu and edited input. The window then reports Hidden and retains a stale
image when the system is back on Dark. The complete native round trip and focus
after switching remain unverified; the isolated AppKit notification captures above
remain the deterministic transition evidence. This does not establish full UI
acceptance. Tauri remains present; the current-source review of missing
pages and controls is in the [UI cleanup checkpoint](ui-control-close-20261114.md).
