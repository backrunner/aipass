# Native renderer dependencies

These are rendering/UI dependencies only. Subscription authentication, quota,
protocol conversion and storage remain in AIPass's Rust crates.

| Directory | Upstream | Pinned source | License |
| --- | --- | --- | --- |
| `gpui` | [Sonora GPUI](https://github.com/sonorahq/gpui) | `8ddac49c8b5a079149b33b4581e03f74e05237e0`, `crates/gpui` | `gpui/LICENSE`, `gpui/NOTICE` |
| `gpui_apple`, `gpui_wgpu` | [Sonora GPUI](https://github.com/sonorahq/gpui) | Same revision as core; `crates/gpui_apple`, `crates/gpui_wgpu` | Each directory retains `LICENSE` and `NOTICE` |
| `gpui-base` | [Longbridge GPUI Kit](https://github.com/longbridge/gpui-kit) | Published `gpui-base` 0.9.1, revision `39ab849a93b5f15507b969164d49891db8d5b470`, `crates/base` | `gpui-base/LICENSE-APACHE` |
| `block` | [rust-block](https://github.com/SSheldon/rust-block) | Published `block` 0.1.6; archive SHA256 in `upstream.json` | `block/LICENSE-MIT` (text supplied from upstream's declared MIT license) |

The root Cargo patch selects the local GPUI core and Apple/WGPU rendering crates,
all from the same pinned Sonora revision. Cargo.lock pins the complete
dependency graph; there is no runtime source or plugin download.

The user explicitly approved preserving oversized **third-party upstream files**
on 2026-11-13. This exception does not extend to AIPass-authored modules or tests.
`gpui/src/input_transform.rs` is a new AIPass module and is covered by the ordinary
repository size gate. Its integration and GPU regressions live under
`apps/desktop-native/src/composite/tests/`.
The same size gate covers `gpui_apple/src/system_font_weights.rs`.

## Maintained changes

- Metal/WGPU: clamp image sampling to each atlas tile's texel centers so retained
  light/dark textures cannot bleed into neighboring image edges.
- GPUI: crossfade complete named frames after deferred menus, dialogs and tooltips
  paint. Keep live input and accessibility dispatch exclusive to the current frame.
  Callers retain outgoing textures and cancel replay on resize or vault locking.

- GPUI text: preserve fractional intrinsic widths and line heights through layout and leave
  device-pixel snapping to the layout engine's absolute edges. Per-label rounding
  must not accumulate across adjacent badges. Include display scale in the
  measurement-cache key so moving between displays refreshes snapped line metrics.
- GPUI Base: preserve measured disclosure heights during animation. AIPass reveal
  subtrees retain fractional extents so stacked sections and bottom scrolling do
  not accumulate independently rounded heights.
- Apple text: map the named system font weights to their CSS classes before
  matching. Medium must not fall back to Regular, and Heavy and Black remain
  distinct. Other font families retain the upstream matching behavior.
- GPUI: add an input transform scoped to compositor subtrees. Keep hit testing,
  clips, pointer/scroll/gesture coordinates, cached views, accessibility bounds,
  native IME geometry and selection hit testing consistent with the painted scale.
  Unscaled elements retain the direct event path.
- GPUI/Apple/WGPU: composite subtree opacity once, preserving foreground colors
  on disabled controls and modal/menu fades. Keep overlapping translucent siblings
  in separate layers. Preserve shader/host structure alignment, including WebGL.
  SVGs can opt into inherited text tint so icon buttons follow their parent state.
- GPUI Base: clear pointer styles for disabled buttons and retain noninteractive
  popup content during its exit transition. Preserve focus restoration on close.
- GPUI Base: adapt its 0.9.1 API to the pinned renderer's scroll events, platform
  modifiers, quad smoothing and shared text-system ownership. Transform popup
  trigger bounds before positioning a portal outside the scaled subtree.
- Cargo manifests: resolve upstream workspace inheritance to explicit pinned
  dependencies. Retain the original manifests and upstream licenses.
- Blocks: replace an uninhabited enum used for an external static with an
  inhabited, opaque C-layout type. Only its address is used. This fixes Rust's
  future-incompatibility warning without changing the block layout or runtime
  copy/release calls. Make the existing C ABI and function-pointer conversions
  explicit and label the existing unsafe contracts for strict Clippy. Resolve
  the published test helper from crates.io because the upstream archive omitted
  its relative path. The standalone test lockfile pins that helper.

`upstream.json` records origin revisions, upstream hashes and locally changed
files. `patches/` contains reviewable diffs against those exact sources. Refresh
both when changing a patch. Do not edit Cargo's global checkout or registry cache.

For updates, review the rendering and input patches together, run the native
parity-feature tests, repository gate and full workspace Clippy, then repeat
960×640 light/dark and English/Chinese native interaction and presentation checks.
Synthetic GPU tests establish Metal compositing behavior; they do not establish
real-window visual equivalence, frame pacing or Windows/Linux renderer behavior.

WGPU shader parsing, storage layouts, text shaping and WebGL variants are tested
on macOS with its standalone manifest and lockfile. Test fonts come from the
same pinned upstream revision; their original SIL licenses remain beside them.
Those tests do not establish Windows/Linux GPU rendering or presentation.

Run `node --test tests/desktop-parity/vendor-provenance.test.mjs` after edits to
verify that every vendored file remains inventoried and matches its recorded hash.
