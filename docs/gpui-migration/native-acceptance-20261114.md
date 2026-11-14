# Native interaction acceptance, 2026-11-14

Result: **partial; the complete desktop is not accepted and Tauri remains.**
This follow-up uses real macOS application windows with synthetic fixtures,
without connecting a vault or Agent. Foreground smoothness sampling remains
waived. No production lifecycle, Keychain or release validation is claimed.

## Observed controls

| Surface | Observation | Evidence boundary |
| --- | --- | --- |
| Agent credential picker | Down/Return selected Fallback; reopening with Space showed the selected item. Escape restored the trigger, and Tab moved to Switch. | Native key events, accessibility state and updated window images at 960×640, English/dark. |
| Agent manual guide | Choosing a Cline credential enabled Setup guide. The guide displayed the endpoint/model and backdrop blur. Tab reached the close button, Shift-Tab wrapped to the footer, and Escape restored the launch button's focus ring. | Native input and window images; this does not test applying external tool configuration. |
| WebDAV import | URL → Tab → username updated the intended native field values. | Accessibility-state evidence only. The window image was stale; password traversal and complete presented-input acceptance remain open. |
| Import Back button | The native accessibility tree exposed an unnamed button. Added the localized name; the rebuilt application reports `返回`. | The change affects accessibility, not geometry or copy. |
| Recovery copy | Added its localized accessibility name, including the copied state. The rebuilt application reports `复制`. Copy opens the native permission prompt; Cancel returns to the kit. | No key was copied or submitted. The key fixture is synthetic. |
| Recovery acknowledgment | Continue was initially disabled; the acknowledgment changed to checked and enabled Continue. Escape left the required acknowledgment dialog open. | Native accessibility state plus a matching updated dark/Chinese window image. Completion/exit animation is not accepted. |
| Subscription proxy settings, first attempt | Attempted URL/Tab interaction while the window image remained on an entrance frame. | This attempt was **not accepted**. The visible-window follow-up below supersedes the input-traversal gap, not the complete appearance gate. |

The corresponding artifacts are in
`artifacts/desktop-parity/native-acceptance-20261114/`: the Agent guide image,
import accessibility evidence, recovery accessibility/image evidence,
retained stale images, process sample, validation logs and `validation.json`.

## Visibility diagnosis

Application-side observation recorded the import window as `Hidden`, inactive,
at 960×640 both when opened and on its next-frame callback. The recovery trace
also recorded transitions between Visible and Hidden, including a visible/active
interval around the native confirmation dialog. GPUI's visibility describes
whether frames can be presented; it is not simply the existence of an AX window.
This explains why input state could change while the window-server image stayed
old. It does **not** establish whether the cause was occlusion, automation,
another space, a sleeping display or a locked session.

At the first checkpoint, the user had been asked to bring the test window forward;
the response arrived afterward. Consequently, those stale screenshots are retained as failed
observations and are not used to diagnose a renderer defect or claim acceptance.
Task-owned test processes were closed after the attempt.

For repeatable GUI launch, the parity feature now accepts a display-only
`AIPASS_PARITY_LAUNCH` JSON array when there are no CLI arguments. It requires
`--parity-preview`, accepts only scenario/theme/locale/size/reduced-motion flags,
and passes scenarios through the existing allowlist. It rejects vault,
preferences, updater and capture paths. Normal builds do not compile this path.
This lets test bundles point `CFBundleExecutable` at the actual Mach-O rather
than an exec wrapper. `AIPASS_PARITY_WINDOW_TRACE` optionally records visibility,
activation state and viewport from inside the isolated application; it does not
inject input, force visibility, sample frame pacing or record field contents.

## Visible-window follow-up

After the user confirmed the window was in front, the earlier test process had
already exited. Reopened the isolated subscription application and verified the
following at **960×640, English/light**, with updated native window images as well
as accessibility state:

| Control | Result |
| --- | --- |
| Proxy inputs | A pointer click visibly focused Proxy URL. Select-all/paste changed only that field. Tab moved to Username, then Password; Shift-Tab returned to Username. Password input was masked. |
| Clear saved authentication | Tab reached the checkbox after Password; Space toggled it. No real authentication data was saved or cleared. |
| Cancel and reopen | Cancel discarded the edited URL, username, password and checkbox. Reopening restored the original fixture URL, empty credential fields and disabled Save. |
| Connection selector | All five choices were visible. Home/Down/Return selected System proxy, removed the conditional custom-proxy inputs and enabled Save. |
| Balance form | Scrolling reached the request body and headers while the dialog header/footer stayed fixed. Add header inserted a second row; Remove deleted it. The probe action stayed disabled while changes were unsaved. |
| Notifications | Changing tabs reset the body scroll and showed webhook URL, authentication and event controls. The test action stayed disabled while changes were unsaved. |

The corresponding images, accessibility snapshot, window trace, binary/input
hashes and result manifest are in
`artifacts/desktop-parity/native-visible-20261114/`. All synthetic drafts were
cancelled or discarded on closing the task-owned applications; no network test,
vault operation or real settings save was performed.

### System appearance and automation boundary

Opened a separate **960×640, Simplified Chinese** fixture with no forced theme
argument. Source inspection confirms that this entry uses the default System
preference. The initial dark menu and a subsequent light menu image both retain
the open selector, Custom proxy selection and edited username. The accessibility
snapshot also retains the edited proxy URL. These are endpoint observations,
not complete system-switching acceptance.

The system Appearance page was exercised, and its original Dark selection was
confirmed before closing it. At that point the app's image remained light. Its
trace had changed from Visible to Hidden, active; Escape updated menu state but
the image still showed the earlier menu frame. Raising the window and reopening
its application through Finder did not restore presentation. The return-to-dark
frame, interactive focus after switching and interrupted transitions therefore
remain **unverified**. A stale frame is not evidence of a theme-observer defect.
Foreground smoothness sampling remains waived.

Rapid batched pointer/input attempts also twice ended on the background page.
Reopening the dialog and observing the pointer focus before sending keys produced
the passing sequence above. The root cause of the batched-input/visibility issue
is unresolved; this run does not certify that automation path. The title's middle
dot comes from the synthetic entry's literal account name, not newly concatenated
localized UI copy; no user-provided account title was rewritten.

## Validation

- Initial comparison inputs and binary matched the previous checkpoint hashes.
  Existing paired geometry matrices are reused for unchanged visual layouts;
  they do not cover this run's real-window interaction gaps.
- Native Rust regression: **94 library + 146 UI tests passed**.
- `cargo clippy --workspace --all-targets -- -D warnings`: **passed** with
  Homebrew Rust on macOS.
- Parity-render native all-targets strict Clippy: **passed**.
- `cargo fmt --all -- --check`, `git diff --check`, `pnpm repo:check`: **passed**.
  The repository gate checks 947 owned source files and retains 19 existing
  oversized-file exceptions. No new source-size exception was added.
- No commit, push, remote CI, installed production-app or functional acceptance
  is claimed.
- The visible-window follow-up changed no production code or test code. All 766
  recorded source/build-input hashes still match the validated checkpoint, so
  its Rust results are reused rather than represented as newly run tests.

## Subscription header hover refinement

The subsequent user screenshot identified the disclosure header's full-height
hover fill ending in a hard vertical edge beside Configure. The native header
now paints a rounded highlight inset by 4px, with an 8px corner radius. Configure
uses the same neutral surface color. Text, icon positions, hit areas and keyboard
focus remain unchanged. This is an intentional native hover improvement requested
by the user; the frozen Svelte hover paint is no longer the target for this detail.

`artifacts/desktop-parity/runtime-hover-20261114/` retains before/after images,
the comparison sheet, exact geometry, source/binary hashes and validation result.
At 960×640, 24 updated captures cover English/Chinese, light/dark, open/closed and
idle/header-hover/configure-hover states. All 20 pixel/geometry checks pass:
the four English idle comparisons are pixel-identical to the prior binary;
hover changes stay inside their intended control and never move layout.
Synthetic pointer activation still expands the disclosure and opens settings;
the keyboard focus image retains its visible focus ring. These are offscreen
Metal and simulated-event checks, not new physical-pointer or smoothness evidence.

The 13 existing subscription-options tests, native parity build,
`cargo clippy --workspace --all-targets -- -D warnings`, formatting and diff checks
pass on macOS. Only the native overview's hover paint changed in this follow-up;
the prior fractional text-layout fix is preserved. No commit or push was made.

## Remaining UI acceptance

1. Implement the global settings drawer and local-proxy pages already identified
   in the [onboarding/Agent checkpoint](native-onboarding-agents-20261114.md).
2. Finish visible-window input, pointer/keyboard interaction, popup/disclosure,
   resize and minimum-size acceptance across migrated pages.
3. Finish actual system appearance switching in both directions while retaining
   form input, focus and open overlays. The endpoint observations above and the
   earlier deterministic captures do not close the interrupted native round trip.

These are UI requirements. Deferred functional checks and waived smoothness
sampling are not being reintroduced as Tauri-removal prerequisites.
