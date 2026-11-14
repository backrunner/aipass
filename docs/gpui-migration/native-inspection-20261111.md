# Native inspection after screen unlock

This is a development inspection of synthetic fixtures on the local Mac.
The migration remains unaccepted. Screenshots and accessibility observations
were inspected through the computer-use bridge in the conversation; they are
not the paired, archived image/animation evidence required by the final gate.

## Programs

| Program | Executable SHA-256 |
| --- | --- |
| GPUI second correction inspection | `56948d496d730005b39d9258566e410c255099b0f4f9c79c69b148bc04b1bbac` |
| Native Tauri fixture reference | `caa61b6bb8a64dfc8523c09e5904889d2662f50eab422c5932783a497222a677` |

The programs are wrapped in local, ignored development bundles under
`artifacts/AIPass GPUI Parity.app` and `artifacts/AIPass Tauri Parity.app`.
GPUI runs with `--parity-preview` and no vault. Tauri uses the reference Vite
server on `127.0.0.1:14731`, the original Svelte application, and a synthetic
test bridge. Production IPC and window-control calls in that bridge remain
mocked; this host cannot prove production startup, vault, tray or updater behavior.

## Observations and corrections

| Check | Initial observation | Correction inspection |
| --- | --- | --- |
| Window screenshot/input | Previously blocked by lock/invisible session | Screenshots and native clicks work after unlock |
| Sidebar Favorites | Selection and empty state respond | Filtering is also disabled and exposed as disabled to accessibility |
| Search | Typing and clearing work; focus ring absent | Focus ring visible; synthetic query changes empty-state text |
| Escape / outside click | Menu stays open | Both dismiss the menu |
| Menu item layout | Headless button migration centered labels | Left alignment and popup shadow restored |
| Directional navigation | Not implemented | Down selects Settings, Down selects Lock; Enter reaches lock screen |
| Tab in menu | Original native Tauri dismisses and focuses All items | GPUI dismisses; internal focus order is regression-tested. Visible focus styling still differs |
| Initial workspace actions | Focus points to hidden auth input | Focus retained in the mounted desktop; initial lock key regression passes |
| Lock focus and subsequent quit | Search focus becomes unmounted after lock; quit does not respond | Password caret/focus ring visible after lock; Command-Q exits and process absence was checked |
| Accessibility | No workspace controls exposed | Workspace buttons, labels, search and disabled states exposed; focused/selected accessibility reporting remains incomplete |

Tests use the actual headless controls and key-down/key-up events, not just
direct state mutation. All 17 native-crate Rust tests passed. The full workspace
Clippy gate and the native crate's `parity-render` Clippy gate passed with
`-D warnings`. Repository checks and formatting also passed.

## Remaining differences

- The original native menu has distinct keyboard-open, typeahead and focus
  presentation behavior that is not fully ported. Popup motion is not ported.
- Native screenshots include different frame heights: the Tauri fixture produced
  1920×1282, while the resized GPUI preview produced 1920×1280. This is an
  unresolved native-pair calibration/layout difference, not an accepted crop.
- Empty-state typography, subtle background composition, shadows, focus styling
  and accessibility state still differ. No screenshot masks or tolerance changes
  were applied.
- Settings, provider operations, subscriptions, Agent/proxy panels and the rest
  of the inventory remain unported. A toolbar event is not a working operation.
- The lock/unlock curtain now has native rendering, multiple isolated frame
  comparison rounds, callback cancellation tests, and normal/reduced-motion
  native lock observations. Pixel acceptance still fails. Hero animation and
  production integration remain pending; see the detailed
  [curtain validation](curtain-validation-20261111.md).

No source component is marked accepted based on this inspection.
