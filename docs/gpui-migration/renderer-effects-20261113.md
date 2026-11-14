# Native modal compositing — 2026-11-13

Follow-up: [subscription overview and native acceptance](ui-acceptance-20261113.md)
records the later native interaction, screenshot matrices, resolved native-host
Keychain tests and successful presented-frame sampling. The deferred status below
describes this earlier renderer checkpoint.

The subscription popup now scales its complete painted subtree from `.96` to
`1` on entry and toward `.97` on exit. Opacity, position and scale keep independent
transition state, so reversing a partially entered popup preserves continuity.
The existing reference durations/easing and reduced-motion behavior remain in use.

Subscription, provider, integration preview/login and update-confirmation overlays
now apply a **4 logical-pixel backdrop blur** to content already painted in the
same frame. This includes foreground text and controls behind the overlay.
Production compositing stays on the GPU; it does not capture a previous window
frame, read GPU pixels back to the CPU or substitute a decorative blurred image.

## Dependency and input changes

The pinned `gpui-pre` API could not composite an arbitrary subtree or sample a
backdrop. The native preview now uses a fixed Sonora GPUI revision with its matching
platform crates. The existing GPUI Base 0.9.1 component library is retained with
a small compatibility patch. See [vendor sources](../../vendor/README.md) for
origins, licenses, hashes and reviewable patches. The user explicitly approved
preserving upstream third-party file structure; new AIPass modules still follow
the source-size policy. No subscription/provider engine was imported.

The upstream compositor's scale affects paint only. AIPass's input patch adds:

- Scaled hit regions and clipping, with mouse, drag, touch and pixel-scroll events
  expressed in the original layout coordinates.
- Matching accessibility bounds and native input-method candidate/selection bounds.
- Cache invalidation when only an ancestor's transform or clip changes.
- Popup portal anchors measured in visual window coordinates.
- Comparison geometry measured after transformation, matching browser bounding boxes.

The scale wrapper keeps space for modal shadows and uses the renderer's no-op path
at scale `1`. It also respects Metal's `GPUI_FILTERS=0` diagnostic bypass for input
geometry. This bypass is not a visual acceptance configuration.

## Automated evidence

Focused regressions cover transformed clicks, original-region rejection, pixel
scroll distance, nested transforms, clipping, cached children, popup anchors,
native text-service coordinates, and transition reversal/reduced motion.

The native parity-feature suite passes all 156 tests (68 library, 84 UI/state,
4 integration tests). The vendored Objective-C Blocks compatibility patch passes
its 6 upstream runtime tests and 2 documentation tests, including copying a
closure from stack to heap and calling it through Objective-C. Its old external
static used an uninhabited type; the patch uses an inhabited opaque C-layout
type and removes the future-incompatibility warning without suppressing lints.

Two synthetic Metal tests render to offscreen textures at 1× and 2×. They verify
that a foreground stripe contributes to backdrop blur in the current frame, that
changing the background does not reuse the previous frame, and that a parent and
its child receive the same compositor scale. Test-only pixel readback is used for
these assertions. They do not create or capture a native application window.

Local check logs and source fingerprints are recorded under
`artifacts/desktop-parity/renderer-effects-20261113/`.

On native macOS with Homebrew Rust 1.99.1:

- `cargo fmt --all -- --check`, the full
  `cargo clippy --workspace --all-targets -- -D warnings`, native parity-feature
  Clippy, and separate strict Clippy for all three patched dependencies pass.
- Node install with the frozen lockfile, license audit, lint, typecheck, tests
  and build pass. The control-panel embedded build stays unchanged. The source
  size gate, its regression tests and capture/packager script syntax pass.
- `cargo build --workspace` passes. Separate GPUI and Tauri comparison bundles
  were built under the evidence directory, with valid plist metadata and ad-hoc
  signatures. The GPUI bundle contains the retained license/source records.
  Neither bundle was launched; these are preview fixtures, not release artifacts.
- The initial `cargo test --workspace` run was stopped after native-host tests
  blocked in `SecItemCopyMatching`. The saved thread sample traces this through
  proxy-recovery key loading during unlock; other test threads wait on the same
  recovery lock. This is **not a passing full-workspace test result**. With that
  crate explicitly excluded, the remaining workspace tests pass. Re-run
  `cargo test -p aipass-native-host` when the user can complete native Keychain
  interaction. No credentials, Keychain permissions or production storage were
  changed to bypass the block.

## Acceptance boundary

The user deferred native-window validation until returning to the Mac. No visible
window, live vendor login, screenshot matrix or presentation sampling was used
for this patch. No commit, push or release was requested. Production DMG/updater
runtime validation and remote CI have not been run for this working tree.

Changing the renderer invalidates the earlier native screenshots as evidence for
this binary, even where app-level layout code did not change. Re-run the 960×640
and larger-window matrix in both themes and locales, keyboard/mouse/IME flows,
shadow/clip inspection and the real Metal presentation observer. Windows/Linux
backends remain subject to their CI and platform acceptance; local evidence is
macOS only. This patch does not certify complete GPUI/Tauri equivalence or resolve
the previously unaccepted onscreen presentation timestamps by assumption.
