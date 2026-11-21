# BoardUI native first slice

Historical first-slice receipt. Production integration now follows
[the unified migration](boardui-production-migration.md). Original run results
and first-failure evidence below retain their original scope.

Status: first usable implementation complete; software-view and official React replay PASS. Baseline unic `ccf947c9f9a8e8b759203632e0b33e6f44d6b2a0`; official MIT BoardUI `ec5c5eb3e29c2441be63b9422cd54f411ef23466`; mygo `v0.8.4-0.20261108082913-9233663736ca`.

## Failure inventory (before implementation)

Wrong source or license; incomplete CSS resolution; unbound font/icon; hidden web runtime; missing demo/package; mismatched geometry/DPI/font; incorrect variant/size/state; unlabeled icon button; disabled activation; read-only mutation; invalid/error relation absent; lost CJK/emoji or IME composition; wrong Tab/arrow/Enter; nested Escape closes the dialog; popover escapes viewport; focus is not trapped or restored; uncontrolled value resets on render; stale state after closing; clipped focus ring; narrow layout overflow; reduced-motion keeps animation scheduled; cache grows by transient IDs; screenshot omits overlay; first failing artifact overwritten; screenshot-only interaction; production UI or system clipboard touched; font/compositor difference hidden in a similarity percentage.

## E2E protocol fixed before native component code

Use the actual official React Button/Input/Select and pinned settings-modal source/behavior, with a synthetic settings form. Preserve official source and CSS unchanged; fixture composition may select a bounded form. Native API tests cross the public component package, drive pointer and keyboard through mygo's complete software view, and save exercised PNG checkpoints plus assertions. They are E2E view scenarios, not native-window evidence or unit tests.

Operation chain: open Settings from launcher; focus Name; replace text with synthetic English/CJK/emoji; validate empty value and observe associated error; restore valid value; open Role select; use arrows/Enter; verify selection and focus; reopen select and Escape; verify dialog remains open; Tab/Shift+Tab stay in dialog; close with Escape; verify launcher focus; reopen and save; verify caller-owned state persists. Also exercise disabled and read-only inputs, password rendering and icon-only button names.

Capture common content geometry, source-resolved theme/type/shape data, font files, locale, scale, renderer and source identities. First viewport 800x760 DIP, then 360x720; 800x640 is a separate scroll-boundary case; light/dark at 1x/2x and reduced motion. Reference and native fixtures must share data and font bytes. Preserve initial failure; allow at most two retries per unchanged scenario before diagnosis. Never overwrite the first artifact. Native OS keyboard/IME/accessibility and Windows memory remain separate acceptance scopes.

Initial red gate: run the E2E entry before the native package/demo exists. Missing public package or missing protocol behavior must fail. A build failure is only the initial scaffold red; subsequent behavioral red/green records remain required.

## Approved first-slice interface

The public native package is `app/desktop/boardui`. `Kit{Dark bool}` holds only scheme selection. Button returns a native button element. Input accepts caller-owned `*string` and returns the true editor element. Select accepts caller-owned value/open pointers and stable item IDs; a nil Open uses mygo widget-local state. Dialog accepts an open pointer and a body function. The independent `showcase.App` owns synthetic form data and is shared by demo and software-view E2E. No core or service types cross this interface.

The dialog is a bounded composition derived from the pinned Settings Modal shell. It is not a claim that the unavailable newer generic Modal source was migrated. The reference fixture uses real exported React components with unchanged source/CSS. The native initial default viewport is 800x760 to display the source-sized fields; 360x720 covers narrow layout without shrinking typography.

## Failure evidence and corrections

- Initial scaffold red: `.build/boardui-e2e/initial-red/go-test.txt` proves the native package/showcase did not exist before the E2E specification.
- First behavior run `behavior-01` passed the light/1x complete operation chain. Its initial visual capture exposed a secondary Button width error of approximately 2 DIP and an initial-focus difference from React. Removed duplicate border compensation and focused the Dialog panel instead of the first field.
- `behavior-02` retained a real focus-restoration regression. mygo records an overlay's opener after running its build callback. Calling panel AutoFocus inside that callback recorded the panel itself as opener. The fix calls AutoFocus after DialogBase returns. Single-case `focus-diagnosis-01` passed; `behavior-03` then passed all four settings and gallery theme/scale subcases plus narrow reduced motion.
- The gallery originally requested an unbundled `add` glyph. Corrected the fixture to the actual approved `settings` glyph. No unknown-resource fallback was added.
- The reference browser screenshot helper emitted CSS-pixel images even at DPR 2. `raw:true` also failed the new PNG-size assertion. Preserved both attempts. Direct `Page.captureScreenshot` with an explicit clip produced real 1600x1520 device-pixel originals. No screenshot was resized to manufacture parity.
- `visual-03` passed all four default Dialog comparisons. Exact paired originals and unmasked difference metrics remain available. This does not accept every interactive state by implication.
- `outside-01` then reproduced a relevant Select mismatch: one outside Cancel press was swallowed by mygo SelectParts.Popup's backdrop. Pinned React Select explicitly uses a nonmodal popup and restores outside dismissal without consuming the clicked action. This failure blocked delivery until the correction below passed. MenuBase alone is not equivalent because initial highlight and group navigation differ. Do not replace the regression assertion with a second click.

## Select correction and measurement scope

The first outside-Cancel assertion used the Cancel button's center. The popup overlaps that point by a fraction of a DIP, so that failure alone did not prove outside propagation. The corrected E2E clicks Cancel's visible right interior and retains the same close/focus assertions. A separate real-view primitive probe confirmed the underlying mygo limitation at an uncovered point `(482.18, 622.5)`: the popup closed but Cancel did not activate. Its JSON and PNG are retained in `.build/boardui-e2e/primitive-outside-red/`.

The final Select composes fixed-dependency `PopoverBase`, `FocusGroup` and `ButtonBase`. The mature primitives own outside dismissal, overlay restoration, arrows/Home/End, disabled focus skipping and Enter/Space activation. The component glues initial selected focus, hover focus and controlled value/open state. It uses no replacement editor, reflection, module-cache edits or local dependency override. `outside-fix-02` passed the complete light/1x chain once with the uncovered Cancel point.

Typeahead is explicitly unsupported in this first slice. Rich option renderers, sections and asynchronous option loading are also outside the exposed API. ButtonLink/anchor behavior, Input adornments/tooltips and the full multi-page Settings Modal are not claimed. Full implementation coverage is recorded in [implementation-coverage.json](boardui-mygo/implementation-coverage.json), separately from the research inventory.

The first-slice numeric visual gates cover control rectangles and empty control-interior colors. Text-only header regions retain unmasked raster error and exact source type values; a sample point landing on an antialiased glyph is not treated as a flat-fill test. Popup comparison uses the painted popup surface in both implementations and also retains the React raw listbox-content rectangle. It does not compare an outer native panel to an inner web content box.

## Final configuration binding

The software cases explicitly set `en-US` before capturing controls. Reference
HTML and React Aria's DirectionProvider also use `en-US`. The replay verifies
the embedded font, notices, fixture scripts and npm lock against source-lock
before starting either side. The independent demo source is included in the
replay input hashes. The demo's supported minimum client area is 320x640 DIP;
its bounded form does not claim arbitrary-height or arbitrary-body scrolling.

A final source audit replaced mygo's polynomial default easing in Select with
the real compiled CSS values. The popover uses cubic-bezier(0,0,.2,1) over 150ms.
The caret uses cubic-bezier(.4,0,.2,1) over 200ms. These values are extracted and
generated, not inferred from the class name `ease`. The previous complete
`replay-02` passed; the final run binds the explicit locale and lock checks.

Temporal trajectories are not separately pixel-accepted frame by frame. Native
OS window composition, native keyboard/IME and platform accessibility bridges
remain NOT_RUN. CJK/emoji value editing and a mixed Chinese/Japanese/Korean
software field are exercised; CJK fallback font files and full long-label/value
overflow matrices are not pinned or visually accepted. The primary paired
captures use identical synthetic English data and the same Inter bytes.

## Final reference action correction

The explicit-locale replay `replay-final` stopped in the real React action
protocol because the popup partially covered the Cancel button's center.
Its screenshots, failure log, TaskSpace ID and hit-test diagnosis are retained.
The diagnosis demonstrated that the center hit the popup while the visible
right interior hit Cancel. Both implementations now use that visible interior
for the outside-action assertion. The reference helper uses a real mouse press,
not DOM `click()` or synthetic success. Recovery reuses TaskSpace 71 through
`--reference-space 71`; it does not create another space to bypass the failure.

Supplemental software-view edge investigation passed at widths 800, 360 and
320, height 640 and scales 1/2. All six popups flipped above the near-bottom
trigger, stayed inside the viewport, closed only the top Escape scope and
restored the dialog opener. It retains source, screenshots, JSON and checksums
in `.build/boardui-e2e/edge-probe/`. This supplements the canonical cases and
does not establish OS-native placement.

## Final acceptance receipt

The final `replay-final-02` passed both canonical cases with race instrumentation
and all 13 official React/native software-view comparisons. Its source hashes
remained unchanged and the asset lock verified. The maximum measured control
rectangle delta was 0.5 DIP. The maximum empty-interior channel error was 2/255.
Unmasked raster means and RMS values remain descriptive, not pixel-equality
claims. See [acceptance-receipt.json](boardui-mygo/acceptance-receipt.json).

The separate no-race light/1x view run measured 3.347 ms to the first ready
software frame, 360.350 ms for 100 popup event/render cycles, 51,936 KiB process
RSS and 3,334,216 bytes Go HeapAlloc. Font registration is excluded from the
first-frame metric. This is a test-process baseline, not production or Windows
memory. No explicit GC or working-set trimming was used.

Both standalone and existing Desktop builds passed. Scoped golangci-lint
v2.15.0 reported zero issues. Existing gui-navigation and production-boundary
E2E cases passed. The public Engine dependency boundary passed. No module
version or product UI was changed. Only Button, Input, Select and the derived
Dialog shell plus their required foundations are delivered; the remaining
inventory is explicitly tracked, not claimed as migrated.

The replay closed its owned Vite process group and TaskSpace 71. Downloaded
BoardUI source, node_modules, npm cache, the temporary mygo source audit and
owned compiled binaries were removed after verification. First failures and
final synthetic screenshots/checksums remain in ignored artifact directories.
Reference replay must prepare dependencies again. No Pro source, personal
data, large screenshots or downloaded dependency trees are committed.

Native OS windows, physical keyboard/IME, platform accessibility bridges,
Windows resource behavior and user visual approval remain NOT_RUN. The
independent demo is buildable and uses the same composition as the view E2E.
