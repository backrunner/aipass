# Optional fields and disclosures — 2026-11-13

This checkpoint covers Add field and its form sections in the GPUI migration.
It does not accept the complete UI or the outstanding presentation timing gate.
All previews use synthetic data; no production vault or release was modified.

## Behavior and structure

- The menu now preserves the reference Details/Advanced groups, field ordering,
  labels and every optional field. Visible fields disappear from the menu;
  removing one clears its inputs and makes it available again. The trigger
  disappears when all fields are present. Billing remains one menu choice with
  three inputs. Aliases, headers, group and billing populate the existing Agent
  Add DTO. The edit path continues preserving existing metadata.
- Menu width, padding, item heights, text alignment, shadow and edge clearance
  match the reference. It opens above the trigger when the space below is
  insufficient, including at 960×640. Notes, removal gaps, hint line heights and
  the final advanced-row border were corrected using individual measurements.
- Shared locale keys and theme colors cover English/Chinese and light/dark.
  No duplicate locale dictionary or untranslated new product copy was added.
- Add/edit advanced sections, credential billing and Quick integration share
  `disclosure.rs`: 220 ms height and 180 ms opacity/chevron with CSS `ease`,
  interrupted reversals and reduced motion. Closed content retains natural-size
  measurement but skips prepaint/paint, removing hidden hitboxes/focus/AX nodes.
  The opening zero-height frame registers the new input before it receives text.
- Field menus and generic selectors are separate Rust modules. Optional DTO
  parsing, disclosure measurement and tests also have their own modules. New
  files remain below repository size limits; no `include!` fragments were added.

## Geometry and appearance evidence

`artifacts/desktop-parity/ui-optional-20261113-r2/report.json` contains 32 cases:
four states (menu, all fields, closed advanced, open advanced), two viewports
(960×640 and 1280×820), two locales and two themes. All 32 mapped geometry and
numeric-style checks pass, with the unchanged 0.5 logical-pixel tolerance.
The previously reported one-pixel advanced-form discrepancy was traced to the
last-row border, hint line height and scrolling before final layout settlement.

The full matrix predates the final keyboard and closed-content registration
changes. `ui-optional-20261113-final-probe` rechecks all four states at minimum
size in English light mode after the measurement wrapper (4/4 passes). The
subsequent `ui-optional-20261113-final-r2` records the final production source
and passes all four geometry/numeric-style cases again.
Static captures use reduced motion and hidden windows; they are not animation
or native-interaction proof. Colors, text readability and menu placement were
also inspected in real English-light and Chinese-dark minimum-size windows.

Failed/intermediate rounds remain intact: `probe`, `probe-r2`, `probe-r3` and
`r1`. The `r1` third capture unexpectedly lost its form; its cause was not
established. Numeric-style checks explicitly retain `fullStyleAcceptance: false`:
font rasterization, compositing and all interaction states are not pixel-identity
claims.

## Interaction and motion evidence

Real bundles `ui-optional-native-20261113-r3` through `r5` and the diagnostic
bundle exercised menu grouping, upward placement, adding/removing a field,
keyboard selection, a single Escape closing only the menu, and advanced
expansion/collapse. The native AX tree loses Interface/Auth/concurrency when
fully collapsed and restores them when expanded.

In the final diagnostic bundle, selecting Custom headers automatically expanded
Advanced. After scrolling it into view, its caret/focus styling was visible;
without another field click, pasting `x-fixture=1` updated that exact input.
Cancel discarded the fixture form and the preview app was closed. This bundle
differs from the final production source only by a temporary dialog-reason log,
which has been removed.

Several automated character-key attempts closed the dialog. The diagnostic
record identified `BackdropPress`, rather than a keyboard cancellation, as the
close event. A long Escape attempt also differed from a short single press.
These attempts are retained as failed automation observations; their complete
cause is not established and they do not prove an input regression. Successful
single-key/menu and visible-input checks are recorded separately above.

GPUI regressions cover all optional fields and the exact Add payload, clearing
removed billing inputs and sensitive header drafts, menu arrows/Enter/Escape,
focus through opening and actual input in an activated window, interrupted
reversal, reduced motion, and closed-body prepaint/paint suppression. Deterministic
clock tests verify transition behavior; no new onscreen frame-time claim is made.

## Validation and remaining scope

Local macOS uses Homebrew Rust 1.99.1. Final logs are in
`artifacts/desktop-parity/ui-optional-validation-20261113-final/`; the earlier
`ui-optional-validation-20261113/` retains the formatting failure and its repair.
Validation includes workspace Clippy with `-D warnings`, parity-feature Clippy,
114 native tests, formatting, repository/source-size and diff checks. The 13
unchanged comparison/presentation-tool tests passed in the preceding validation
directory and are reused. No commit, push, release or remote CI was performed.

The earlier [presentation checkpoint](ui-performance-20261113.md) still has
unexplained zero-time Metal presentation callbacks. Full inventory acceptance,
unmapped states and visual details outside this field/disclosure scope remain
open. A passing static matrix does not close those items.
