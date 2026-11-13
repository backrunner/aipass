# Component and motion acceptance

The frozen reference remains `65a1f1f5a5e1b51311132659f9b972131c8ea551`.
The 2026-11-13 review requires measured component details, including spacing,
and engineering splits for large native files. A successful capture is not an
acceptance result.

## Comparison layers

1. **Coverage:** use the component and dynamic-state inventories. Track missing
   implementations and unmapped observations explicitly. Never treat an empty
   comparison set as passing.
2. **Geometry:** compare each mapped control's x/y/width/height in logical pixels,
   including labels, inputs, action targets, grouped rows and scrolling content.
   Half a logical pixel is the diagnostic threshold at 2× scale. A mismatch
   remains visible in the report; do not enlarge the threshold to hide it.
3. **Style:** retain the reference's computed padding, margin, gap, typography,
   colors, radii, borders, shadows, background and backdrop filter. Native probes
   serialize the style actually supplied to the observed element. Numeric
   declarations are checked separately from geometry. Inherited `normal` text
   metrics, rasterization, compositing and interactive styles still require
   visual review; a numeric-style pass does not approve those dimensions.
4. **Pixels:** preserve full original screenshots, source hashes, the native
   binary and raw Metal readback. Review side by side and with the opacity slider
   in `components.html` / `index.html`. The slider changes only review display;
   it never changes comparison inputs, conceals regions or waives discrepancies.
5. **Animation:** pause original CSS at the same elapsed times supplied to the
   production native renderer. Compare all five auth background transforms at
   starts, intermediate points, turnarounds and later cycles. Verify reduced
   motion at more than one timestamp. Check first-frame scene completeness and
   preparation cost separately. Then inspect live motion and resize in an actual
   native window: fixed-time images do not establish presented frame rate.
6. **Interaction:** exercise hover, press, keyboard focus, disabled/busy/error,
   reveal, open/close, long content, scroll edges and focus restoration for the
   affected controls. A rendered control is not proof that its action works.

## Matrix and reproducibility

- Window sizes: `960×640` and `1280×820`; no phone layout substitutes.
- Locales: English and Simplified Chinese; themes: light and dark.
- Auth states: unlock, busy, error, setup, create, recover, reset, reset scrolled
  to the bottom. Provider states: selected detail, add/edit, expanded advanced;
  credential and integration scenarios remain separate existing runners.
- Animation: normal and reduced motion. Background samples use `0`, `2250`,
  `4500`, `7000`, `18000`, `26000`, `52000` ms, plus `0` / `26000` ms in reduced
  mode. Lock/unlock curtains retain their separate transition runner.
- Every result belongs to its recorded binary/source snapshot. Recheck changed
  inputs; keep prior failed rounds as diagnostic evidence.

Start the frozen reference server with `AIPASS_PARITY_BASELINE=1` and the parity
Vite config, then run these from the repository root (with the local RTK wrapper):

```sh
node tests/desktop-parity/capture-presentation.mjs UNIQUE-AUTH-ROUND --frozen \
  --scenarios=locked,locked-busy,locked-error,setup,create,recover,reset,reset-bottom
node tests/desktop-parity/capture-providers.mjs UNIQUE-PROVIDER-ROUND
node tests/desktop-parity/capture-background.mjs UNIQUE-BACKGROUND-ROUND
node --test tests/desktop-parity/component-audit.test.mjs
```

Run the capture runners sequentially: native windows share pointer/focus state.
Use `--probe` only to diagnose a change, not as a substitute for the matrix.
Capture mode accepts synthetic data only. Observations do not serialize editor
values, and the native observation wrapper adds no layout box or event handler.

### Presented animation timing

Run `node tests/desktop-parity/capture-performance.mjs UNIQUE-ROUND` on macOS
with no other capture or build running. Keep the synthetic window visible. The
runner builds Release without `parity-render` so per-frame geometry/style JSON
serialization does not affect timing. It creates its own ad hoc signed bundle
and test-only observer; it never injects into the installed application. `--probe`
selects only minimum-size light unlock; `--case=locked-1280-dark` selects one
named matrix case.

The observer registers a callback on each Metal command-buffer presentation.
Its timestamp is [MTLDrawable.presentedTime](https://developer.apple.com/documentation/metal/mtldrawable/presentedtime),
which is zero for an unpresented/dropped drawable. Modern GPUI uses
`CAMetalDisplayLink`; `nextDrawable` alone is not sufficient coverage. Preserve
all zero callbacks and unmatched submissions. Three seconds of warmup are
explicitly separated from steady-state intervals, without removing long gaps.
Window activation notifications distinguish foreground changes from rendering
work. A wall-clock stop marker detects silent tails, with a 100 ms allowance
for process termination and in-flight drawables; interval timing itself uses
Metal's monotonic clock.

The conservative timing gate requires at least 10 seconds of steady data,
P95 within a 60 Hz budget plus 2 ms, P99 within 1.5 budgets, no interval over
50 ms, at most 1% intervals exceeding 1.5 budgets, no zero-time callbacks and
no stale unmatched submissions. It must fail on insufficient data. Reduced
motion is checked separately for at most 3 FPS after warmup, allowing the
focused input caret. These checks do not certify resize, interaction latency,
other machines or the production package. Keep the raw trace and report even
when this strict gate fails; never relax it to make a measured run green.

## Engineering boundaries

- `desktop.rs` constructs and owns the root entities. `desktop/session_sync.rs`
  applies Agent completions, `desktop/transition.rs` owns close/lock/curtain
  transitions, and `desktop/view.rs` composes the root window. Tests are separate.
- `workspace/providers.rs` coordinates provider state and actions;
  `workspace/provider_list.rs` renders list rows, and
  `workspace/provider_detail.rs` renders the selected provider.
- `backdrop.rs` owns scene lifetime and resize handoff; `backdrop/scene.rs`
  creates cached layers; `backdrop/motion.rs` owns their timelines;
  `backdrop/glass.rs` filters background textures with premultiplied alpha.
- Keep production files at or below 1,000 lines and tests at or below 1,500.
  Split by responsibility before reaching the limit. Do not use `include!`,
  compressed formatting or a raised size allowance to evade it.

## Acceptance boundary

The native migration is still incomplete. Missing copy/pricing actions, unpaired
components, text advance differences and scroll-end differences must remain open
until corrected and reviewed. Do not mark the overall UI or an inventory entry
accepted merely because its outer container, one locale or a reduced-motion
screenshot matches. Intentional prior design changes (such as titlebar alignment)
need an explicit recorded exception, not a silent baseline edit.
