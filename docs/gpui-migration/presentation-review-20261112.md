# Presentation review, 2026-11-12

The user clarified that exact pixels are not required. Acceptance still requires
matching components, layout, shape and style, plus motion and interaction.
`compare.mjs` now requires a separate, documented verdict for each dimension;
an exact image match cannot bypass those verdicts. Missing or unreviewed evidence
still fails the coverage audit. Capture completion is not visual acceptance.

## Scope and evidence

The expanded matrix covers empty workspace, unlock, setup choice, create,
recover, application menu and provider filter. Each has English/Chinese,
light/dark, 960×640/1280×820, at 2× scale with reduced motion. Reference captures
run the unchanged baseline components in WebKit with synthetic data. Candidate
captures export the actual native GPUI scene. These are not 56 packaged-app
interaction passes.

Artifacts under `artifacts/desktop-parity/` retain paired images, pixel
diagnostics, DOM bounds/styles, native outer bounds, the binary, source archive
and hashes. Image comparison uses no masks, resizing or compositing. The native
card's vector shadow uses a rounded mask as part of its actual renderer; this
does not hide or alter any comparison pixels.

- `presentation-initial-20261112`: interrupted after 3 captures when entering
  create first exercised `session_touch`. The synthetic bridge now implements
  that no-op session activity notification. This incomplete attempt is retained.
- `presentation-expanded-20261112`: 56 pairs captured, no unsupported bridge
  calls or page errors. Executable SHA-256:
  `123c602dbae79496eb849bd67c53566dda201f2f2ce03268017db2dc84b08f8d`.
- `presentation-correction-20261112`: 56 pairs captured, no unsupported bridge
  calls or page errors. Executable SHA-256:
  `67d985e0fbf7fbf0970633e2354537bfbf5647cf3142d21b85fb8636ce99bc59`.

Across the 112 pairs, auth card outer bounds differ by at most 0.25 logical
pixels; pane rectangles match. `presentation-outer-layout-20261112.json`
records every rectangle comparison. This does not establish inner-control
parity. Both capture reports retain `presentationStatus: unreviewed`.

Direct visual inspection covered representative pairs, not all 112 images:
expanded create/menu at 960×640 English light and recover/filter at 960×640
Chinese dark; corrected filter at 960×640 English light, create at 1280×820
English light, and setup/unlock at 960×640 Chinese dark. Earlier initial-screen
comparisons and targeted font/card probes remain available separately. The
systemic gaps below block acceptance; the rest of the matrix is not implicitly
classified as reviewed or matching.

## Differences found and changes

| Area | Finding | Current result |
| --- | --- | --- |
| Password input | Dark caret/ink used the default light editor palette | Native input palette now uses the same theme tokens; live dark input checked |
| Password reveal | Switching the control to a semantic button exposed missing SVG foreground inheritance | Explicit icon foreground restored; button has accessible show/hide labels |
| Disabled primary button | Border/background and label opacity were composited independently | Transparent structural border and once-composited theme colors remove the extra blue edge and washed-out label |
| Create/recover form | Strength meter and confirmation feedback were absent | Four bars, reference strength scoring, localized guidance and success/error feedback added; UTF-16 scoring regression included |
| Auth card | Drop shadow showed through the translucent interior and was too diffuse | Native vector shadow excludes the rounded card interior and uses CSS blur sigma; inset highlight restored |
| Recovery key | Placeholder, monospace type and input height differed | Placeholder restored; Menlo and 36px input height applied |
| Provider filter | Only 6 of 10 fixed choices existed; selected marker absent | All 10 fixed choices and localized selected marker rendered; tag-driven choices still require the unported entry data path |
| Popup shadows | GPUI blur parameter was used as CSS blur radius | Native Gaussian sigma corrected from 16 to 8 for the reference's 16px CSS blur |
| Detail empty state | Icon background was absent in dark mode | Shared surface-2 circle restored |

The create form's Back button now has the reference ghost-button height and
uses a semantic native button. Initial focus is limited to unlock; opening create/recover
does not automatically focus the password. Busy controls expose disabled state.
The recovery flow's missing reset control and panel are **not** papered over with
an inert button.

Strength hints use separate fields rather than dotted Chinese copy, as required
by root `AGENTS.md`. This is an explicit copy/layout deviation from the frozen
reference and must be reviewed, not silently classified as identical.

## Native interaction observations

The updated native bundle was exercised through the visible macOS window at
960×640, using synthetic data only. No vault creation or destructive action
was submitted.

- Chinese dark create form: initially no input autofocus; short-password
  strength/hints, red mismatch, green match, and disabled/enabled submit states
  appeared as expected. Reveal changed both fields and its accessible label.
  Back returned to setup; reopening create cleared inputs and restored masking.
  Tab focused the first field. A longer synthetic password produced three blue
  bars and the localized good-strength label. Command-Q exited the preview.
- English light provider filter: all 10 choices were visible. End then Enter
  selected API accounts and closed the menu. Reopening showed its Selected
  marker and no incorrect active sidebar category.
- The subsequent Home/Enter return-to-All inspection could not be observed:
  capture returned `cgWindowNotFound`, including after reacquiring the app.
  Inventory still showed both parity apps running. The cause was not established.
  This last live step is **unverified**. The equivalent GPUI test-platform
  navigation regression passed, which is separate evidence.

These observations are recorded in the tool interaction trace; they are not a
complete saved native-window screenshot matrix. Full native Tauri/GPUI paired
interaction and animation acceptance remains open.

## Validation

- Native tests with `parity-render`: 24 passed, including the ten-choice filter
  keyboard regression and password scoring regression.
- Pixel/coverage comparator: 6 passed. The coverage audit still fails on all
  98 inventoried surfaces; no incomplete implementation was waived.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed on native
  macOS Homebrew Rust 1.99.1. The additional native-crate Clippy check with
  `--features parity-render` also passed.
- Native build, desktop Svelte typecheck (0 errors, 0 warnings),
  `cargo fmt --all -- --check` and `pnpm repo:check` (765 source files) passed.
- Full migration Rust/Node suites, artifact startup/update gate and remote CI
  have not run. No commit, push or release was performed.

## Still not accepted

- Recovery reset action/panel, import, recovery-kit export and structured auth
  errors remain incomplete. Busy spinners and field retention on failed requests
  also need comparison against the full reference behavior.
- Text weight/letter spacing and logo downsampling remain visible review items.
  The process-local font smoothing configuration follows the reference's
  antialiased text intent, but does not establish typography parity. It uses a
  [volatile Foundation domain](https://developer.apple.com/documentation/foundation/userdefaults)
  and does not write persisted system or application preferences.
- Auth backdrop blur/saturation, animated hero layers, hover/focus transitions,
  strength-color transitions and scroll masks remain unverified or unported.
- Menus still need complete typeahead, keyboard-open, focus and populated-data
  coverage. The synthetic workspace has no provider data integration yet.
- Provider/subscription operations, settings, Agent/proxy panels, nested dialogs,
  tray, updater and production packaging are not complete native replacements.

No entire inventory component is accepted by this review. Tauri remains the
production desktop. Historical pixel-exact failures are retained as diagnostics;
they are not the reason these functional and presentation gaps remain open.
