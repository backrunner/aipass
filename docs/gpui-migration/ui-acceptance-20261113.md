# Subscription overview and native acceptance — 2026-11-13

The subscription overview now follows the frozen Tauri card: its heading icon,
right-hand disclosure chevron, separate configure action, divider, row insets,
icon boxes, 12 px card corners, text hierarchy and summary labels match the reference. The empty
integration view no longer contributes an extra 18 px gap at the bottom of the
detail column. The overview implementation lives in its own 302-line module;
the runtime editor remains separate.

This checkpoint follows [renderer effects](renderer-effects-20261113.md). It
records local macOS preview evidence, not a production release or acceptance of
every surface in the migration inventory. All accounts and form values used in
the previews are synthetic.

## Corrections found through real interaction

Opening a runtime settings row with native accessibility enabled exposed a
panic in `window/a11y.rs`: the dialog root and close button registered the same
focus handle twice in one frame. They now have separate handles. The invariant
remains enabled. Headless tests alone did not expose this failure.

Closing settings also used to reset the overview's expansion and loaded state.
Closing now erases editable inputs and discards the draft while preserving the
saved summary, disclosure state and routes. Account changes and lock still clear
the complete state. A regression verifies that an unsaved interval change and
synthetic password are discarded, reopening restores saved values, and account
clear removes the retained state.

At the actual 960×640 native window, the following flows were observed:

- English/light: open Connection from its overview row, use Tab/arrow navigation
  between settings pages, scroll Notifications to its final controls, and Escape.
- Chinese/dark: all four overview rows open their corresponding pages; the
  configure action opens Quota. Change the refresh interval without saving and
  close: the overview stays expanded and retains the saved two-minute summary.
- Keyboard focus rings, Shift-Tab, Space to collapse/expand, and removal of
  collapsed rows from the accessibility tree work in the overview.
- Subscription catalog search filters to MiniMax; its connection form and login
  method menu open. Escape closes the inner menu before the containing dialog.
- Add field opens the grouped menu. Notes can be added, removed and added again;
  Chinese text and Return produce multiple lines. Advanced fields collapse out
  of the accessibility tree, and the form can close and reopen.

One earlier automated `typeText` sequence dismissed the form and later inputs
did not act. Its exact cause was not established. A fresh process passed explicit
paste/Return, remove/re-add, collapse, close and reopen. This does not establish
physical input-method composition or eliminate every intermittent input issue.
No vendor login, webhook delivery, credit reset or production save was submitted.

The first accessibility crash and interrupted capture rounds are retained; they
are not counted as passes. The corrected real-window bundle is under
`artifacts/desktop-parity/ui-acceptance-20261113-r1/acceptance-bundles/`. The final
shared-card radius correction followed those interaction checks; fresh captures
below use that final source. The input implementation is unchanged.

## Reproducible visual comparisons

Every matrix uses 960×640 and 1280×820, English and Simplified Chinese, light and
dark. Native images are offscreen Metal renders; interaction evidence above is
separate. Source and binary SHA-256 fingerprints accompany the reports.

| Matrix | Cases | Result |
| --- | ---: | --- |
| Overview expanded/collapsed | 16 | Pass |
| Subscription states | 208 | Pass at the existing 3 px geometry gate |
| Provider list states | 24 | Pass |
| Add field and advanced forms | 32 | Pass |

The overview checker compares both absolute viewport coordinates and coordinates
relative to the card, at a 0.75 logical-pixel tolerance. It checks declared numeric
styles, all four corners of mapped cards/buttons/icon boxes, surfaces, borders
and explicit text colors, and requires actual painted pixels in the heading/configure icons; correct
rectangles alone previously missed a blank configure icon. An earlier spacing-only
pass also missed the 10 px versus 12 px card corners; the added appearance gate
rejects that result. Collapsed rows must
not be painted. These mapped checks do not claim pixel identity across renderers.

Provider list/form geometry uses a 0.5 px tolerance. The broader subscription
runner retains its existing 3 px threshold: its maximum observed difference is
2.203125 px in the missing-Claude form body height, and 52 component mappings
exceed 0.75 px. Those are not accepted as exact spacing equivalence. The stricter
overview result is independent of that broader gate; neither result certifies
unmapped font shaping, hover clipping or every intermediate animation frame.

Review pages and raw evidence:

- [Overview](../../artifacts/desktop-parity/ui-acceptance-20261113-overview-final2/index.html)
- [Subscriptions](../../artifacts/desktop-parity/ui-acceptance-20261113-subscriptions-final/index.html)
- [Provider lists](../../artifacts/desktop-parity/ui-acceptance-20261113-provider-lists/index.html)
- [Optional fields](../../artifacts/desktop-parity/ui-acceptance-20261113-provider-fields/index.html)

## Presented-frame sampling

The new renderer passes all eight cases of the unchanged presentation gate in
`artifacts/desktop-parity/ui-acceptance-20261113-performance-matrix/report.json`.
Each uses an 18-second Release run, a 3-second warmup, no geometry auditing, and
actual `MTLDrawable.presentedTime`. No capture or build ran concurrently. Every
case recorded the app active and window key, with no resignation during sampling.

| Case | Steady FPS | P95 interval (ms) | Maximum interval (ms) |
| --- | ---: | ---: | ---: |
| Unlock, 960, light | 143.89 | 6.95 | 13.89 |
| Unlock, 960, dark | 143.76 | 6.95 | 13.89 |
| Unlock, 1280, light | 143.76 | 6.95 | 20.84 |
| Unlock, 1280, dark | 143.30 | 6.95 | 13.89 |
| Reset, 960, light | 143.90 | 6.95 | 13.89 |
| Reset, 960, dark | 143.96 | 6.95 | 6.95 |
| Reduced motion, 960, light | 1.92 | 520.97 | 520.97 |
| Reduced motion, 1280, dark | 1.92 | 527.91 | 527.91 |

All cases have zero invalid presentation callbacks and zero stale unpresented
frames. Reduced-motion cases intentionally retain only intermittent updates such
as caret blinking and pass their separate idle-rate gate. Frames in flight at the
runner's deliberate SIGTERM are recorded separately. These results supersede the
old renderer's failed timing evidence for the sampled auth paths; they do not
claim universal zero dropped frames or certify modal-animation frame pacing.

The performance build preceded the final dialog-focus and draft-discard fixes.
Those changes do not touch the sampled auth/background render path. Its exact
source fingerprint remains in the report instead of being represented as the
later interaction binary.

## Local checks and limits

Homebrew Rust 1.99.1 on macOS:

- `cargo clippy --workspace --all-targets -- -D warnings`: pass.
- Native `--features parity-render --all-targets` strict Clippy: pass.
- `cargo test -p aipass-desktop-native --features parity-render`: 157 tests pass
  (68 library, 85 UI/state, 4 integration; subprocess repetitions are not added).
- `cargo test -p aipass-native-host`: 24 tests pass after the user's Keychain
  interaction. No permission or credential bypass was used.
- Format, repository structure/size gate, component-audit tests and capture
  syntax checks pass. The revise skill validates with an isolated PyYAML
  environment; the initial system-Python attempt lacked that dependency.

Commands, logs and fingerprints are collected under
`artifacts/desktop-parity/ui-acceptance-20261113-r1/`. Earlier workspace build,
remaining workspace tests and Node gates are recorded in the renderer checkpoint;
they are not represented as a new complete workspace run here. Remote CI,
Windows/Linux, production startup/update artifacts, live vendor authentication
and a complete physical IME/resize sweep remain outside this checkpoint.

No commit, push, installation into the user's production app location or release
was performed.
